//! VPCs, subnets and security groups (+ SG reachability as Permitted edges).

use crate::{evidence, sg_id, subnet_id, vpc_id, PendingLink, Ref, ScanContext, ScanOutput};
use aws_sdk_ec2::types::{SecurityGroup, Subnet, Tag, Vpc};
use sprawlr_domain::{EdgeKind, EvidenceKind, Node, NodeId, NodeKind};
use std::collections::BTreeMap;

pub async fn scan(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let ec2 = aws_sdk_ec2::Client::new(&ctx.sdk);
    let vpcs: Vec<Vpc> = ec2.describe_vpcs().into_paginator().items().send().collect::<Result<_, _>>().await?;
    let subnets: Vec<Subnet> = ec2.describe_subnets().into_paginator().items().send().collect::<Result<_, _>>().await?;
    let sgs: Vec<SecurityGroup> = ec2.describe_security_groups().into_paginator().items().send().collect::<Result<_, _>>().await?;

    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    for v in &vpcs {
        out.nodes.push(map_vpc(ctx, v));
    }
    for s in &subnets {
        out.nodes.push(map_subnet(ctx, s));
    }
    for sg in &sgs {
        let (node, links) = map_sg(ctx, sg);
        out.nodes.push(node);
        out.links.extend(links);
    }
    Ok(out)
}

fn tags(t: &[Tag]) -> BTreeMap<String, String> {
    t.iter().filter_map(|t| Some((t.key()?.to_string(), t.value().unwrap_or_default().to_string()))).collect()
}

fn named(id: &str, t: &BTreeMap<String, String>) -> String {
    t.get("Name").cloned().unwrap_or_else(|| id.to_string())
}

fn base(ctx: &ScanContext, id: NodeId, kind: NodeKind, name: String, parent: NodeId) -> Node {
    let mut n = Node::new(id, kind, name);
    n.account = Some(ctx.account.clone());
    n.region = Some(ctx.region.clone());
    n.parent = Some(parent);
    n
}

pub fn map_vpc(ctx: &ScanContext, v: &Vpc) -> Node {
    let id = v.vpc_id().unwrap_or_default();
    let t = tags(v.tags());
    let mut n = base(ctx, vpc_id(ctx, id), NodeKind::Vpc, named(id, &t), ctx.region_id());
    n.attrs = serde_json::json!({
        "vpc_id": id,
        "cidr": v.cidr_block(),
        "is_default": v.is_default(),
    });
    n.aliases.push(id.to_string());
    n.tags = t;
    n
}

pub fn map_subnet(ctx: &ScanContext, s: &Subnet) -> Node {
    let id = s.subnet_id().unwrap_or_default();
    let t = tags(s.tags());
    let parent = vpc_id(ctx, s.vpc_id().unwrap_or_default());
    let mut n = base(ctx, subnet_id(ctx, id), NodeKind::Subnet, named(id, &t), parent);
    n.attrs = serde_json::json!({
        "subnet_id": id,
        "az": s.availability_zone(),
        "cidr": s.cidr_block(),
        // Heuristic until route tables are scanned (route to an IGW = truly public).
        "auto_public_ip": s.map_public_ip_on_launch(),
    });
    n.aliases.push(id.to_string());
    n.tags = t;
    n
}

/// SG node plus reachability: `source --network_reachable--> this SG` for each ingress rule.
pub fn map_sg(ctx: &ScanContext, sg: &SecurityGroup) -> (Node, Vec<PendingLink>) {
    let gid = sg.group_id().unwrap_or_default();
    let t = tags(sg.tags());
    let parent = sg.vpc_id().map(|v| vpc_id(ctx, v)).unwrap_or_else(|| ctx.region_id());
    let me = sg_id(ctx, gid);
    let mut n = base(ctx, me.clone(), NodeKind::SecurityGroup, sg.group_name().unwrap_or(gid).to_string(), parent);
    n.aliases.push(gid.to_string());
    n.tags = t;

    let mut links = Vec::new();
    let mut rules = Vec::new();
    for p in sg.ip_permissions() {
        let ports = match (p.from_port(), p.to_port()) {
            (Some(-1), _) | (None, _) => "all".to_string(),
            (Some(a), Some(b)) if a == b => a.to_string(),
            (Some(a), Some(b)) => format!("{a}-{b}"),
            (Some(a), None) => a.to_string(),
        };
        let detail = format!("{} {ports}", p.ip_protocol().unwrap_or("-1"));
        let open_v4 = p.ip_ranges().iter().any(|r| r.cidr_ip() == Some("0.0.0.0/0"));
        let open_v6 = p.ipv6_ranges().iter().any(|r| r.cidr_ipv6() == Some("::/0"));
        if open_v4 || open_v6 {
            links.push(PendingLink {
                from: NodeId("internet".into()),
                to: Ref::Id(me.clone()),
                kind: Some(EdgeKind::NetworkReachable),
                evidence: evidence(EvidenceKind::Permitted, "ec2:security_group_ingress", Some(detail.clone())),
            });
        }
        for pair in p.user_id_group_pairs() {
            if let Some(src) = pair.group_id() {
                links.push(PendingLink {
                    from: sg_id(ctx, src),
                    to: Ref::Id(me.clone()),
                    kind: Some(EdgeKind::NetworkReachable),
                    evidence: evidence(EvidenceKind::Permitted, "ec2:security_group_ingress", Some(detail.clone())),
                });
            }
        }
        rules.push(detail);
    }
    n.attrs = serde_json::json!({ "group_id": gid, "ingress": rules, "open_to_internet": links.iter().any(|l| l.from.0 == "internet") });
    (n, links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_ec2::types::{IpPermission, IpRange, UserIdGroupPair};

    pub(crate) fn ctx() -> ScanContext {
        ScanContext { sdk: aws_config::SdkConfig::builder().build(), account: "123456789012".into(), region: "us-east-1".into() }
    }

    #[test]
    fn subnet_is_contained_in_vpc() {
        let c = ctx();
        let s = Subnet::builder().subnet_id("subnet-1").vpc_id("vpc-1").availability_zone("us-east-1a").build();
        let n = map_subnet(&c, &s);
        assert_eq!(n.parent.unwrap().0, "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-1");
        assert_eq!(n.attrs["az"], "us-east-1a");
    }

    #[test]
    fn sg_rules_become_reachability_edges() {
        let c = ctx();
        let sg = SecurityGroup::builder()
            .group_id("sg-db")
            .group_name("db")
            .vpc_id("vpc-1")
            .ip_permissions(
                IpPermission::builder()
                    .ip_protocol("tcp")
                    .from_port(5432)
                    .to_port(5432)
                    .user_id_group_pairs(UserIdGroupPair::builder().group_id("sg-app").build())
                    .build(),
            )
            .ip_permissions(
                IpPermission::builder()
                    .ip_protocol("tcp")
                    .from_port(443)
                    .to_port(443)
                    .ip_ranges(IpRange::builder().cidr_ip("0.0.0.0/0").build())
                    .build(),
            )
            .build();
        let (n, links) = map_sg(&c, &sg);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].from.0, "arn:aws:ec2:us-east-1:123456789012:security-group/sg-app");
        assert_eq!(links[1].from.0, "internet");
        assert_eq!(n.attrs["open_to_internet"], true);
    }
}
