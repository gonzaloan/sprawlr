//! Merge scanner outputs into one graph and resolve pending references.

use crate::{external_node, PendingLink, Ref, ScanOutput, ScanReport};
use sprawlr_domain::{ArchGraph, EdgeKind, Node, NodeId, NodeKind};
use std::collections::HashMap;

pub fn link(outputs: Vec<ScanOutput>) -> ScanReport {
    let mut g = ArchGraph::new();
    let mut warnings = Vec::new();
    let mut links: Vec<PendingLink> = Vec::new();
    let mut tags = Vec::new();

    // 1. Authoritative nodes first, generic inventory only fills gaps.
    let (auth, generic): (Vec<_>, Vec<_>) = outputs.into_iter().partition(|o| o.authoritative);
    for o in auth.into_iter().chain(generic) {
        let authoritative = o.authoritative;
        for n in o.nodes {
            if authoritative || !g.contains(&n.id) {
                g.upsert_node(n);
            }
        }
        links.extend(o.links);
        tags.extend(o.tags);
        warnings.extend(o.warnings);
    }

    // 2. Tags from the tagging API (only for nodes without inline tags).
    for (id, t) in tags {
        if let Some(n) = g.node_mut(&id) {
            if n.tags.is_empty() {
                n.tags = t;
            }
        }
    }

    // 3. Alias index → resolve references. Names repeat across regions, so prefer a match
    //    in the referring resource's region.
    let mut alias: HashMap<String, Vec<(Option<String>, NodeId)>> = HashMap::new();
    for n in g.nodes() {
        for a in &n.aliases {
            alias.entry(a.clone()).or_default().push((n.region.clone(), n.id.clone()));
        }
    }
    let resolve = |g: &ArchGraph, from: &NodeId, a: &str| -> Option<NodeId> {
        let cands = alias.get(a).or_else(|| alias.get(&a.to_ascii_lowercase()))?;
        let region = g.node(from).and_then(|n| n.region.clone());
        cands.iter().find(|(r, _)| *r == region).or(cands.first()).map(|(_, id)| id.clone())
    };

    let mut unresolved = Vec::new();
    for l in links {
        let target = match &l.to {
            Ref::Id(id) if g.contains(id) => Some(id.clone()),
            Ref::Id(id) => placeholder(&mut g, id),
            Ref::Alias(a) => resolve(&g, &l.from, a),
            Ref::External(h) => resolve(&g, &l.from, h).or_else(|| {
                let ext = external_node(h);
                let id = ext.id.clone();
                if !g.contains(&id) {
                    g.upsert_node(ext);
                }
                Some(id)
            }),
        };
        let Some(to) = target else {
            unresolved.push(format!("{} -> {:?} ({})", l.from.0, l.to, l.evidence.source));
            continue;
        };
        let kind = l.kind.unwrap_or_else(|| infer_kind(&g.node(&to).expect("resolved").kind));
        if !g.add_evidence(&l.from, &to, kind, l.evidence) {
            unresolved.push(format!("{} -> {} (unknown source)", l.from.0, to.0));
        }
    }
    place_in_vpc(&mut g);
    ScanReport { graph: g, warnings, unresolved }
}

/// Resources describe their network through subnets and security groups; the VPC they
/// belong to is the parent of either. Places them inside it so maps and docs group them.
pub fn place_in_vpc(g: &mut ArchGraph) {
    let mut moves = Vec::new();
    for n in g.nodes() {
        let in_vpc = n.parent.as_ref().and_then(|p| g.node(p)).is_some_and(|p| p.kind == NodeKind::Vpc || p.kind == NodeKind::Subnet);
        if in_vpc {
            continue;
        }
        let list = |k: &str| -> Vec<String> {
            n.attrs
                .get(k)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default()
        };
        let (Some(acct), Some(region)) = (&n.account, &n.region) else { continue };
        let candidates = list("security_groups")
            .into_iter()
            .map(NodeId)
            .chain(list("subnets").into_iter().map(|s| NodeId(format!("arn:aws:ec2:{region}:{acct}:subnet/{s}"))));
        let vpc = candidates.filter_map(|id| g.node(&id)?.parent.clone()).find(|p| g.node(p).is_some_and(|v| v.kind == NodeKind::Vpc));
        if let Some(v) = vpc {
            moves.push((n.id.clone(), v));
        }
    }
    for (id, vpc) in moves {
        if let Some(n) = g.node_mut(&id) {
            n.parent = Some(vpc);
        }
    }
}

/// A referenced ARN we didn't scan (e.g. an IAM role): keep it as a placeholder node
/// so the edge is not lost, typed from the ARN.
fn placeholder(g: &mut ArchGraph, id: &NodeId) -> Option<NodeId> {
    let kind = kind_from_arn(&id.0)?;
    let name = id.0.rsplit(['/', ':']).next().unwrap_or(&id.0).to_string();
    let mut n = Node::new(id.clone(), kind, name);
    n.attrs = serde_json::json!({ "placeholder": true });
    g.upsert_node(n);
    Some(id.clone())
}

pub fn kind_from_arn(arn: &str) -> Option<NodeKind> {
    let mut p = arn.split(':');
    let (_, _, service) = (p.next()?, p.next()?, p.next()?);
    let resource = arn.splitn(6, ':').nth(5).unwrap_or("");
    Some(match service {
        "iam" if resource.starts_with("role/") => NodeKind::IamRole,
        "iam" if resource.starts_with("policy/") => NodeKind::IamPolicy,
        "secretsmanager" => NodeKind::Secret,
        "ssm" => NodeKind::SsmParameter,
        "kms" => NodeKind::KmsKey,
        "sqs" => NodeKind::SqsQueue,
        "sns" => NodeKind::SnsTopic,
        "dynamodb" => NodeKind::DynamoTable,
        "s3" => NodeKind::S3Bucket,
        "lambda" => NodeKind::LambdaFunction,
        "states" => NodeKind::StateMachine,
        "kinesis" => NodeKind::KinesisStream,
        "events" if resource.starts_with("event-bus/") => NodeKind::EventBus,
        _ => return None,
    })
}

/// When config only says "this references X", pick the edge from what X is.
pub fn infer_kind(target: &NodeKind) -> EdgeKind {
    use NodeKind::*;
    match target {
        Secret | SsmParameter => EdgeKind::UsesSecret,
        KmsKey => EdgeKind::EncryptedWith,
        SqsQueue | SnsTopic | EventBus | KinesisStream => EdgeKind::Publishes,
        LambdaFunction | StateMachine | ApiGateway | LoadBalancer | EcsService | ExternalEndpoint => EdgeKind::Invokes,
        IamRole => EdgeKind::AssumesRole,
        // Access mode (read vs write) unknown from config alone; refined later by IAM/observed data.
        _ => EdgeKind::Reads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence;
    use sprawlr_domain::EvidenceKind;

    fn out(nodes: Vec<Node>, links: Vec<PendingLink>, authoritative: bool) -> ScanOutput {
        ScanOutput { nodes, links, authoritative, ..Default::default() }
    }

    fn pl(from: &str, to: Ref) -> PendingLink {
        PendingLink { from: from.into(), to, kind: None, evidence: evidence(EvidenceKind::Declared, "env:X", None) }
    }

    #[test]
    fn resolves_aliases_and_creates_placeholders() {
        let lambda = Node::new("fn", NodeKind::LambdaFunction, "f");
        let mut table = Node::new("arn:aws:dynamodb:us-east-1:1:table/orders", NodeKind::DynamoTable, "orders");
        table.aliases.push("dynamodb:orders".into());
        let r = link(vec![
            out(
                vec![lambda],
                vec![
                    pl("fn", Ref::Alias("dynamodb:orders".into())),
                    pl("fn", Ref::Id("arn:aws:secretsmanager:us-east-1:1:secret:db-AbC123".into())),
                    pl("fn", Ref::Alias("s3:nope".into())),
                ],
                true,
            ),
            out(vec![table], vec![], false),
        ]);
        let kinds: Vec<_> = r.graph.edges().map(|(_, t, e)| (t.name.clone(), e.kind)).collect();
        assert!(kinds.contains(&("orders".into(), EdgeKind::Reads)));
        assert!(kinds.contains(&("db-AbC123".into(), EdgeKind::UsesSecret)));
        assert_eq!(r.unresolved.len(), 1);
    }

    #[test]
    fn external_hosts_resolve_or_become_third_parties() {
        let mut bucket = Node::new("arn:aws:s3:::assets", NodeKind::S3Bucket, "assets");
        bucket.aliases.push("assets.s3.us-east-1.amazonaws.com".into());
        let cdn = Node::new("cdn", NodeKind::CloudFront, "cdn");
        let r = link(vec![out(
            vec![bucket, cdn],
            vec![
                pl("cdn", Ref::External("assets.s3.us-east-1.amazonaws.com".into())),
                pl("cdn", Ref::External("legacy.example.com".into())),
            ],
            true,
        )]);
        let targets: Vec<_> = r.graph.edges().map(|(_, t, _)| (t.kind.clone(), t.name.clone())).collect();
        assert!(targets.contains(&(NodeKind::S3Bucket, "assets".into())));
        assert!(targets.contains(&(NodeKind::ExternalEndpoint, "legacy.example.com".into())));
    }

    #[test]
    fn aliases_prefer_the_same_region() {
        let mk = |id: &str, region: &str, kind| {
            let mut n = Node::new(id, kind, id);
            n.region = Some(region.into());
            n
        };
        let mut east = mk("t-east", "us-east-1", NodeKind::DynamoTable);
        east.aliases.push("dynamodb:orders".into());
        let mut west = mk("t-west", "us-west-2", NodeKind::DynamoTable);
        west.aliases.push("dynamodb:orders".into());
        let f = mk("fn-west", "us-west-2", NodeKind::LambdaFunction);
        let r = link(vec![out(vec![east, west, f], vec![pl("fn-west", Ref::Alias("dynamodb:orders".into()))], true)]);
        assert_eq!(r.graph.edges().next().unwrap().1.id.0, "t-west");
    }

    #[test]
    fn resources_are_placed_in_their_vpc() {
        let mut sg = Node::new("arn:aws:ec2:us-east-1:1:security-group/sg-1", NodeKind::SecurityGroup, "sg");
        sg.parent = Some("vpc".into());
        let vpc = Node::new("vpc", NodeKind::Vpc, "main");
        let mut db = Node::new("db", NodeKind::RdsCluster, "db");
        db.account = Some("1".into());
        db.region = Some("us-east-1".into());
        db.parent = Some("region".into());
        db.attrs = serde_json::json!({"security_groups": ["arn:aws:ec2:us-east-1:1:security-group/sg-1"]});
        let r = link(vec![out(vec![vpc, sg, db], vec![], true)]);
        assert_eq!(r.graph.node(&"db".into()).unwrap().parent.as_ref().unwrap().0, "vpc");
    }

    #[test]
    fn authoritative_nodes_win_over_generic_inventory() {
        let mut generic = Node::new("x", NodeKind::Other("AWS::Lambda::Function".into()), "generic");
        generic.attrs = serde_json::json!({"from": "cc"});
        let specific = Node::new("x", NodeKind::LambdaFunction, "specific");
        let r = link(vec![out(vec![generic], vec![], false), out(vec![specific], vec![], true)]);
        assert_eq!(r.graph.node(&"x".into()).unwrap().name, "specific");
    }
}
