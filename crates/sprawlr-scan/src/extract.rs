//! Relationship extractors: one pure function per resource type, from Cloud Control JSON
//! (the CloudFormation resource model) to a node plus the links it declares.
//!
//! Adding support for a new AWS service is a new `match` arm and a fixture test; no SDK needed.

use crate::refs::{classify, normalize_arn, Finding};
use crate::{evidence, sg_id, PendingLink, Ref, ScanContext};
use sprawlr_core::{EdgeKind, EvidenceKind, Node, NodeId, NodeKind};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};

/// Facts that need more than one resource to interpret.
#[derive(Default)]
pub struct Lookup {
    /// target group ARN → load balancer ARNs
    pub tg_lbs: HashMap<String, Vec<String>>,
    /// Aurora cluster identifier → (member instance ids, their AZs)
    pub cluster_members: HashMap<String, (Vec<String>, BTreeSet<String>)>,
    /// task definition ARN → its resource model
    pub task_defs: HashMap<String, Value>,
    /// Elastic IP allocations used by NAT gateways (attached even if the EIP model says nothing).
    pub nat_allocations: BTreeSet<String>,
}

impl Lookup {
    pub fn learn(&mut self, type_name: &str, p: &Value) {
        match type_name {
            "AWS::ElasticLoadBalancingV2::TargetGroup" => {
                if let Some(tg) = s(p, "TargetGroupArn") {
                    self.tg_lbs.insert(tg.into(), strs(p, "LoadBalancerArns"));
                }
            }
            "AWS::RDS::DBInstance" => {
                if let (Some(cluster), Some(id)) = (s(p, "DBClusterIdentifier"), s(p, "DBInstanceIdentifier")) {
                    let e = self.cluster_members.entry(cluster.to_ascii_lowercase()).or_default();
                    e.0.push(id.into());
                    if let Some(az) = s(p, "AvailabilityZone") {
                        e.1.insert(az.into());
                    }
                }
            }
            "AWS::EC2::NatGateway" => {
                if let Some(a) = s(p, "AllocationId") {
                    self.nat_allocations.insert(a.into());
                }
            }
            "AWS::ECS::TaskDefinition" => {
                if let Some(arn) = s(p, "TaskDefinitionArn") {
                    self.task_defs.insert(arn.into(), p.clone());
                }
            }
            _ => {}
        }
    }
}

#[derive(Default, Debug)]
pub struct Extracted {
    pub node: Option<Node>,
    pub links: Vec<PendingLink>,
}

// ------------------------------------------------------------------ json helpers

fn s<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k)?.as_str().filter(|v| !v.is_empty())
}
fn arr<'a>(p: &'a Value, k: &str) -> impl Iterator<Item = &'a Value> {
    p.get(k).and_then(|v| v.as_array()).into_iter().flatten()
}
fn strs(p: &Value, k: &str) -> Vec<String> {
    arr(p, k).filter_map(|v| v.as_str().map(String::from)).collect()
}
fn b(p: &Value, k: &str) -> Option<bool> {
    match p.get(k)? {
        Value::Bool(x) => Some(*x),
        Value::String(x) => x.parse().ok(),
        _ => None,
    }
}
fn host(url: &str) -> String {
    url.split("://").last().unwrap_or(url).split(['/', ':', '?']).next().unwrap_or("").to_ascii_lowercase()
}
fn last(arn: &str) -> &str {
    arn.rsplit(['/', ':']).next().unwrap_or(arn)
}
/// `arn:aws:lambda:r:a:function:name:alias` → unqualified function ARN.
pub fn lambda_arn(a: &str) -> String {
    a.split(':').take(7).collect::<Vec<_>>().join(":")
}

fn node(ctx: &ScanContext, id: &str, kind: NodeKind, name: &str) -> Node {
    let mut n = Node::new(id, kind, name);
    n.account = Some(ctx.account.clone());
    n.region = Some(ctx.region.clone());
    n.parent = Some(ctx.region_id());
    n.aliases.push(name.to_string());
    n
}

fn link(from: &str, to: Ref, kind: Option<EdgeKind>, ev: EvidenceKind, source: &str, detail: Option<String>) -> PendingLink {
    PendingLink { from: NodeId(from.into()), to, kind, evidence: evidence(ev, source, detail) }
}

fn declared(from: &str, to: Ref, kind: Option<EdgeKind>, source: &str) -> PendingLink {
    link(from, to, kind, EvidenceKind::Declared, source, None)
}

fn internet(to: &str, source: &str) -> PendingLink {
    declared("internet", Ref::Id(NodeId(to.into())), Some(EdgeKind::RoutesTo), source)
}

fn kms(from: &str, key: Option<&str>, source: &str) -> Option<PendingLink> {
    let k = key?;
    let to = if k.starts_with("arn:") { Ref::Id(NodeId(k.into())) } else if k.starts_with("alias/") { return None } else { Ref::Alias(k.into()) };
    Some(declared(from, to, Some(EdgeKind::EncryptedWith), source))
}

fn sgs(ctx: &ScanContext, ids: Vec<String>) -> Vec<String> {
    ids.iter().map(|g| sg_id(ctx, g).0).collect()
}

/// Secret reference in ECS `Secrets[].ValueFrom` or similar.
fn secret_ref(ctx: &ScanContext, v: &str) -> Ref {
    if v.starts_with("arn:aws:secretsmanager") || v.starts_with("arn:aws:ssm") {
        Ref::Id(NodeId(normalize_arn(v)))
    } else {
        let name = if v.starts_with('/') { v.to_string() } else { format!("/{v}") };
        Ref::Id(ctx.arn("ssm", &format!("parameter{name}")))
    }
}

// ------------------------------------------------------------------ dispatch

pub fn extract(ctx: &ScanContext, type_name: &str, ident: &str, p: &Value, lk: &Lookup) -> Extracted {
    match type_name {
        "AWS::ECS::Service" => ecs_service(ctx, p, lk),
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => load_balancer(ctx, p),
        "AWS::ElasticLoadBalancingV2::TargetGroup" => target_group(p),
        "AWS::RDS::DBCluster" => rds_cluster(ctx, ident, p, lk),
        "AWS::RDS::DBInstance" => rds_instance(ctx, ident, p),
        "AWS::ApiGatewayV2::Api" => http_api(ctx, p),
        "AWS::ApiGatewayV2::Integration" => http_integration(ctx, p),
        "AWS::ApiGateway::RestApi" => rest_api(ctx, p),
        "AWS::SNS::Topic" => sns_topic(ctx, p),
        "AWS::SQS::Queue" => sqs_queue(ctx, ident, p),
        "AWS::DynamoDB::Table" => dynamo(ctx, ident, p),
        "AWS::S3::Bucket" => bucket(ctx, ident, p),
        "AWS::KMS::Key" => kms_key(ctx, ident, p),
        "AWS::StepFunctions::StateMachine" => state_machine(ctx, p),
        "AWS::Events::EventBus" => event_bus(ctx, p),
        "AWS::Events::Rule" => event_rule(ctx, p),
        "AWS::CloudFront::Distribution" => distribution(ctx, p),
        "AWS::EC2::NatGateway" => nat_gateway(ctx, ident, p),
        "AWS::EC2::Instance" => ec2_instance(ctx, ident, p),
        "AWS::EC2::EIP" => elastic_ip(ctx, ident, p, lk),
        // Folded into other resources or only used through Lookup.
        "AWS::ECS::Cluster" | "AWS::ECS::TaskDefinition" => Extracted::default(),
        _ => generic(ctx, type_name, ident, p),
    }
}

/// Inventory-only node for types without a dedicated extractor.
fn generic(ctx: &ScanContext, type_name: &str, ident: &str, p: &Value) -> Extracted {
    let n = crate::cloudcontrol::map_resource(ctx, type_name, ident, Some(&p.to_string()));
    Extracted { node: Some(n), links: Vec::new() }
}

// ------------------------------------------------------------------ compute & edge

fn ecs_service(ctx: &ScanContext, p: &Value, lk: &Lookup) -> Extracted {
    let Some(arn) = s(p, "ServiceArn") else { return Extracted::default() };
    let name = s(p, "ServiceName").unwrap_or_else(|| last(arn));
    let cluster = s(p, "Cluster").map(last).unwrap_or("default");
    let mut n = node(ctx, arn, NodeKind::EcsService, name);
    n.aliases.push(format!("ecs:{cluster}/{name}"));
    let mut links = Vec::new();

    for lb in arr(p, "LoadBalancers") {
        let Some(tg) = s(lb, "TargetGroupArn") else { continue };
        for lb_arn in lk.tg_lbs.get(tg).into_iter().flatten() {
            links.push(link(lb_arn, Ref::Id(NodeId(arn.into())), Some(EdgeKind::RoutesTo), EvidenceKind::Declared, "elb:target_group", Some(last(tg).into())));
        }
    }

    let net = p.pointer("/NetworkConfiguration/AwsvpcConfiguration").cloned().unwrap_or_default();
    let mut images = Vec::new();
    let mut env_keys = Vec::new();
    let mut plaintext = Vec::new();
    let td_arn = s(p, "TaskDefinition").unwrap_or_default();
    let td = lk.task_defs.get(td_arn);
    if let Some(td) = td {
        if let Some(role) = s(td, "TaskRoleArn") {
            links.push(declared(arn, Ref::Id(NodeId(role.into())), Some(EdgeKind::AssumesRole), "ecs:task_role"));
        }
        for c in arr(td, "ContainerDefinitions") {
            if let Some(img) = s(c, "Image") {
                images.push(img.to_string());
            }
            for e in arr(c, "Environment") {
                let (Some(k), Some(v)) = (s(e, "Name"), s(e, "Value")) else { continue };
                env_keys.push(k.to_string());
                match classify(k, v) {
                    Finding::Reference(r) => links.push(declared(arn, r, None, &format!("env:{k}"))),
                    Finding::PlaintextSecret => plaintext.push(k.to_string()),
                    Finding::Nothing => {}
                }
            }
            for sec in arr(c, "Secrets") {
                let (Some(k), Some(from)) = (s(sec, "Name"), s(sec, "ValueFrom")) else { continue };
                env_keys.push(k.to_string());
                links.push(declared(arn, secret_ref(ctx, from), Some(EdgeKind::UsesSecret), "ecs:task_definition.secrets"));
            }
        }
    }
    env_keys.sort();
    env_keys.dedup();
    n.attrs = json!({
        "cluster": cluster,
        "launch_type": s(p, "LaunchType"),
        "desired": p.get("DesiredCount"),
        "task_definition": last(td_arn),
        "cpu": td.and_then(|t| s(t, "Cpu")),
        "memory": td.and_then(|t| s(t, "Memory")),
        "images": images,
        "subnets": strs(&net, "Subnets"),
        "security_groups": sgs(ctx, strs(&net, "SecurityGroups")),
        "public_ip": s(&net, "AssignPublicIp") == Some("ENABLED"),
        "env_keys": env_keys,
        "env_plaintext_secret_keys": plaintext,
    });
    Extracted { node: Some(n), links }
}

fn load_balancer(ctx: &ScanContext, p: &Value) -> Extracted {
    let Some(arn) = s(p, "LoadBalancerArn") else { return Extracted::default() };
    let name = s(p, "LoadBalancerName").or(s(p, "Name")).unwrap_or_else(|| arn.split('/').nth(2).unwrap_or(arn));
    let mut n = node(ctx, arn, NodeKind::LoadBalancer, name);
    if let Some(dns) = s(p, "DNSName") {
        n.aliases.push(dns.to_ascii_lowercase());
    }
    // CloudWatch dimension: `app/<name>/<id>`
    if let Some((_, frag)) = arn.split_once(":loadbalancer/") {
        n.aliases.push(frag.to_string());
    }
    let mut subnets = strs(p, "Subnets");
    subnets.extend(arr(p, "SubnetMappings").filter_map(|m| s(m, "SubnetId").map(String::from)));
    let scheme = s(p, "Scheme").unwrap_or("internal");
    n.attrs = json!({
        "type": s(p, "Type").unwrap_or("application"),
        "scheme": scheme,
        "dns": s(p, "DNSName"),
        "subnets": subnets,
        "security_groups": sgs(ctx, strs(p, "SecurityGroups")),
    });
    let links = if scheme == "internet-facing" { vec![internet(arn, "elb:internet_facing")] } else { vec![] };
    Extracted { node: Some(n), links }
}

/// Lambda targets behind a load balancer (ECS targets are linked from the service).
fn target_group(p: &Value) -> Extracted {
    if s(p, "TargetType") != Some("lambda") {
        return Extracted::default();
    }
    let lbs = strs(p, "LoadBalancerArns");
    let links = arr(p, "Targets")
        .filter_map(|t| s(t, "Id"))
        .flat_map(|f| {
            lbs.iter().map(move |lb| link(lb, Ref::Id(NodeId(lambda_arn(f))), Some(EdgeKind::RoutesTo), EvidenceKind::Declared, "elb:target_group", None))
        })
        .collect();
    Extracted { node: None, links }
}

// ------------------------------------------------------------------ instances

/// An EC2 instance. What it talks to is in its code, which we cannot read: its instance
/// profile's role is the map (see `sprawlr_core::iam`). User data is never read.
fn ec2_instance(ctx: &ScanContext, ident: &str, p: &Value) -> Extracted {
    let id = s(p, "InstanceId").unwrap_or(ident);
    let arn = ctx.arn("ec2", &format!("instance/{id}")).0;
    let name = arr(p, "Tags").find(|t| s(t, "Key") == Some("Name")).and_then(|t| s(t, "Value")).unwrap_or(id);
    let mut n = node(ctx, &arn, NodeKind::Ec2Instance, name);
    n.aliases.push(id.to_string());
    let mut groups = strs(p, "SecurityGroupIds");
    groups.extend(arr(p, "NetworkInterfaces").flat_map(|i| strs(i, "GroupSet")));
    groups.sort();
    groups.dedup();
    n.attrs = json!({
        "instance_id": id,
        "instance_type": s(p, "InstanceType"),
        "state": p.pointer("/State/Name").and_then(|v| v.as_str()),
        "public_ip": s(p, "PublicIp").or(s(p, "PublicIpAddress")).is_some(),
        "subnets": s(p, "SubnetId").into_iter().collect::<Vec<_>>(),
        "security_groups": sgs(ctx, groups),
    });
    let links = s(p, "IamInstanceProfile")
        .map(|profile| {
            // A name, or an ARN: both are aliases of the role that owns the profile.
            let key = profile.rsplit('/').next().unwrap_or(profile);
            declared(&arn, Ref::Alias(key.into()), Some(EdgeKind::AssumesRole), "ec2:instance_profile")
        })
        .into_iter()
        .collect();
    Extracted { node: Some(n), links }
}

// ------------------------------------------------------------------ network leftovers

fn nat_gateway(ctx: &ScanContext, ident: &str, p: &Value) -> Extracted {
    let id = s(p, "NatGatewayId").unwrap_or(ident);
    let arn = ctx.arn("ec2", &format!("natgateway/{id}")).0;
    let mut n = node(ctx, &arn, NodeKind::NatGateway, id);
    n.aliases.push(id.to_string());
    n.attrs = json!({
        "nat_gateway_id": id,
        "connectivity": s(p, "ConnectivityType").unwrap_or("public"),
        "subnets": s(p, "SubnetId").into_iter().collect::<Vec<_>>(),
    });
    Extracted { node: Some(n), links: vec![] }
}

/// Elastic IPs cost money while unattached; attachment comes from the EIP itself or a NAT gateway.
fn elastic_ip(ctx: &ScanContext, ident: &str, p: &Value, lk: &Lookup) -> Extracted {
    let alloc = s(p, "AllocationId").unwrap_or(ident);
    let ip = s(p, "PublicIp").unwrap_or(alloc);
    let mut n = node(ctx, &ctx.arn("ec2", &format!("eip/{alloc}")).0, NodeKind::Other("AWS::EC2::EIP".into()), ip);
    n.aliases.push(alloc.to_string());
    let associated = ["InstanceId", "NetworkInterfaceId", "AssociationId"].iter().any(|k| s(p, k).is_some()) || lk.nat_allocations.contains(alloc);
    n.attrs = json!({ "allocation_id": alloc, "associated": associated });
    Extracted { node: Some(n), links: vec![] }
}

// ------------------------------------------------------------------ data

fn rds_cluster(ctx: &ScanContext, ident: &str, p: &Value, lk: &Lookup) -> Extracted {
    let id = s(p, "DBClusterIdentifier").unwrap_or(ident);
    let arn = s(p, "DBClusterArn").map(String::from).unwrap_or_else(|| ctx.arn("rds", &format!("cluster:{id}")).0);
    let mut n = node(ctx, &arn, NodeKind::RdsCluster, id);
    for path in ["/Endpoint/Address", "/ReadEndpoint/Address"] {
        if let Some(h) = p.pointer(path).and_then(|v| v.as_str()) {
            n.aliases.push(h.to_ascii_lowercase());
        }
    }
    let members = lk.cluster_members.get(&id.to_ascii_lowercase());
    if let Some((ids, _)) = members {
        n.aliases.extend(ids.iter().cloned());
    }
    let multi_az = members.map(|(_, azs)| azs.len() > 1).or(b(p, "MultiAZ"));
    n.attrs = json!({
        "engine": s(p, "Engine"),
        "engine_version": s(p, "EngineVersion"),
        "encrypted": b(p, "StorageEncrypted").unwrap_or(false),
        "multi_az": multi_az,
        "instances": members.map(|(i, _)| i.len()),
        "deletion_protection": b(p, "DeletionProtection"),
        "backup_retention_days": p.get("BackupRetentionPeriod"),
        "security_groups": sgs(ctx, strs(p, "VpcSecurityGroupIds")),
        "subnet_group": s(p, "DBSubnetGroupName"),
    });
    let links = kms(&arn, s(p, "KmsKeyId"), "rds:kms_key_id").into_iter().collect();
    Extracted { node: Some(n), links }
}

fn rds_instance(ctx: &ScanContext, ident: &str, p: &Value) -> Extracted {
    // Aurora members are folded into their cluster.
    if s(p, "DBClusterIdentifier").is_some() {
        return Extracted::default();
    }
    let id = s(p, "DBInstanceIdentifier").unwrap_or(ident);
    let arn = s(p, "DBInstanceArn").map(String::from).unwrap_or_else(|| ctx.arn("rds", &format!("db:{id}")).0);
    let mut n = node(ctx, &arn, NodeKind::RdsInstance, id);
    if let Some(h) = p.pointer("/Endpoint/Address").and_then(|v| v.as_str()) {
        n.aliases.push(h.to_ascii_lowercase());
    }
    n.attrs = json!({
        "engine": s(p, "Engine"),
        "class": s(p, "DBInstanceClass"),
        "encrypted": b(p, "StorageEncrypted").unwrap_or(false),
        "multi_az": b(p, "MultiAZ").unwrap_or(false),
        "publicly_accessible": b(p, "PubliclyAccessible").unwrap_or(false),
        "deletion_protection": b(p, "DeletionProtection"),
        "backup_retention_days": p.get("BackupRetentionPeriod"),
        "security_groups": sgs(ctx, strs(p, "VPCSecurityGroups")),
        "subnet_group": s(p, "DBSubnetGroupName"),
    });
    let links = kms(&arn, s(p, "KmsKeyId"), "rds:kms_key_id").into_iter().collect();
    Extracted { node: Some(n), links }
}

fn dynamo(ctx: &ScanContext, ident: &str, p: &Value) -> Extracted {
    let name = s(p, "TableName").unwrap_or(ident);
    let arn = s(p, "Arn").map(String::from).unwrap_or_else(|| ctx.arn("dynamodb", &format!("table/{name}")).0);
    let mut n = node(ctx, &arn, NodeKind::DynamoTable, name);
    n.aliases.push(format!("dynamodb:{name}"));
    n.attrs = json!({
        "billing_mode": s(p, "BillingMode").unwrap_or("PROVISIONED"),
        "pitr": p.pointer("/PointInTimeRecoverySpecification/PointInTimeRecoveryEnabled").and_then(|v| v.as_bool()).unwrap_or(false),
        "encrypted_with_cmk": p.pointer("/SSESpecification/KMSMasterKeyId").is_some(),
        "stream": p.pointer("/StreamSpecification/StreamViewType"),
    });
    let links = kms(&arn, p.pointer("/SSESpecification/KMSMasterKeyId").and_then(|v| v.as_str()), "dynamodb:sse").into_iter().collect();
    Extracted { node: Some(n), links }
}

fn bucket(ctx: &ScanContext, ident: &str, p: &Value) -> Extracted {
    let name = s(p, "BucketName").unwrap_or(ident);
    let arn = s(p, "Arn").map(String::from).unwrap_or_else(|| format!("arn:aws:s3:::{name}"));
    let mut n = node(ctx, &arn, NodeKind::S3Bucket, name);
    n.region = None; // bucket names are global
    n.aliases.push(format!("s3:{name}"));
    n.aliases.push(format!("{name}.s3.amazonaws.com"));
    for k in ["DomainName", "RegionalDomainName", "DualStackDomainName", "WebsiteURL"] {
        if let Some(h) = s(p, k) {
            n.aliases.push(host(h));
        }
    }
    let pab = p.get("PublicAccessBlockConfiguration");
    let blocked = pab.is_some_and(|c| ["BlockPublicAcls", "BlockPublicPolicy", "IgnorePublicAcls", "RestrictPublicBuckets"].iter().all(|k| b(c, k) == Some(true)));
    // Event notifications: an upload starts work elsewhere.
    let notif = p.get("NotificationConfiguration").cloned().unwrap_or_default();
    let mut links = Vec::new();
    let events = |c: &Value| -> Option<String> { s(c, "Event").map(String::from) };
    for c in arr(&notif, "LambdaConfigurations") {
        if let Some(f) = s(c, "Function") {
            links.push(link(&arn, Ref::Id(NodeId(lambda_arn(f))), Some(EdgeKind::Invokes), EvidenceKind::Declared, "s3:notification", events(c)));
        }
    }
    for (list, key) in [("QueueConfigurations", "Queue"), ("TopicConfigurations", "Topic")] {
        for c in arr(&notif, list) {
            if let Some(t) = s(c, key) {
                links.push(link(&arn, Ref::Id(NodeId(t.into())), Some(EdgeKind::Publishes), EvidenceKind::Declared, "s3:notification", events(c)));
            }
        }
    }
    n.attrs = json!({
        "public_access_block": blocked,
        "versioning": p.pointer("/VersioningConfiguration/Status").and_then(|v| v.as_str()) == Some("Enabled"),
        "website": s(p, "WebsiteURL").is_some(),
        "eventbridge_notifications": notif.pointer("/EventBridgeConfiguration/EventBridgeEnabled").and_then(|v| v.as_bool()).unwrap_or(false),
    });
    Extracted { node: Some(n), links }
}

fn kms_key(ctx: &ScanContext, ident: &str, p: &Value) -> Extracted {
    let key_id = s(p, "KeyId").unwrap_or(ident);
    let arn = s(p, "Arn").map(String::from).unwrap_or_else(|| ctx.arn("kms", &format!("key/{key_id}")).0);
    let name = s(p, "Description").filter(|d| d.len() <= 48).unwrap_or(key_id);
    let mut n = node(ctx, &arn, NodeKind::KmsKey, name);
    n.aliases.push(key_id.to_string());
    n.attrs = json!({ "rotation_enabled": b(p, "EnableKeyRotation").unwrap_or(false), "usage": s(p, "KeyUsage") });
    Extracted { node: Some(n), links: vec![] }
}

// ------------------------------------------------------------------ integration

fn sns_topic(ctx: &ScanContext, p: &Value) -> Extracted {
    let Some(arn) = s(p, "TopicArn") else { return Extracted::default() };
    let mut n = node(ctx, arn, NodeKind::SnsTopic, s(p, "TopicName").unwrap_or_else(|| last(arn)));
    n.attrs = json!({ "fifo": b(p, "FifoTopic").unwrap_or(false) });
    let links = kms(arn, s(p, "KmsMasterKeyId"), "sns:kms_master_key").into_iter().collect();
    Extracted { node: Some(n), links }
}

fn sqs_queue(ctx: &ScanContext, ident: &str, p: &Value) -> Extracted {
    let url = s(p, "QueueUrl").unwrap_or(ident);
    let name = s(p, "QueueName").map(String::from).unwrap_or_else(|| last(url).to_string());
    let arn = s(p, "Arn").map(String::from).unwrap_or_else(|| ctx.arn("sqs", &name).0);
    let mut n = node(ctx, &arn, NodeKind::SqsQueue, &name);
    n.aliases.push(url.trim_end_matches('/').to_string());
    // RedrivePolicy is JSON, sometimes delivered as a string.
    let redrive = match p.get("RedrivePolicy") {
        Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or_default(),
        Some(v) => v.clone(),
        None => Value::Null,
    };
    let dlq = s(&redrive, "deadLetterTargetArn");
    n.attrs = json!({
        "fifo": b(p, "FifoQueue").unwrap_or(false),
        "visibility_timeout_s": p.get("VisibilityTimeout"),
        "dead_letter_queue": dlq,
        "max_receive_count": redrive.get("maxReceiveCount"),
    });
    let mut links: Vec<PendingLink> = kms(&arn, s(p, "KmsMasterKeyId"), "sqs:kms_master_key").into_iter().collect();
    if let Some(d) = dlq {
        links.push(link(&arn, Ref::Id(NodeId(d.into())), Some(EdgeKind::Publishes), EvidenceKind::Declared, "sqs:redrive_policy", Some("dead-letter queue".into())));
    }
    Extracted { node: Some(n), links }
}

fn event_bus(ctx: &ScanContext, p: &Value) -> Extracted {
    let Some(arn) = s(p, "Arn") else { return Extracted::default() };
    let n = node(ctx, arn, NodeKind::EventBus, s(p, "Name").unwrap_or_else(|| last(arn)));
    Extracted { node: Some(n), links: vec![] }
}

/// Pattern rules: each target subscribes to the bus. Schedules are recorded on the target link detail.
fn event_rule(ctx: &ScanContext, p: &Value) -> Extracted {
    if s(p, "State") == Some("DISABLED") {
        return Extracted::default();
    }
    let rule = s(p, "Name").unwrap_or("rule");
    let bus = s(p, "EventBusName").unwrap_or("default");
    let bus_arn = if bus.starts_with("arn:") { bus.to_string() } else { ctx.arn("events", &format!("event-bus/{bus}")).0 };
    let Some(_) = p.get("EventPattern") else { return Extracted::default() };
    let links = arr(p, "Targets")
        .filter_map(|t| s(t, "Arn"))
        .map(|t| {
            let from = if t.starts_with("arn:aws:lambda") { lambda_arn(t) } else { t.to_string() };
            link(&from, Ref::Id(NodeId(bus_arn.clone())), Some(EdgeKind::Subscribes), EvidenceKind::Declared, &format!("events:rule/{rule}"), None)
        })
        .collect();
    Extracted { node: None, links }
}

fn state_machine(ctx: &ScanContext, p: &Value) -> Extracted {
    let Some(arn) = s(p, "Arn") else { return Extracted::default() };
    let mut n = node(ctx, arn, NodeKind::StateMachine, s(p, "StateMachineName").unwrap_or_else(|| last(arn)));
    n.attrs = json!({ "type": s(p, "StateMachineType").unwrap_or("STANDARD") });
    let def: Value = s(p, "DefinitionString").and_then(|d| serde_json::from_str(d).ok()).unwrap_or_default();
    let mut links = Vec::new();
    if let Some(role) = s(p, "RoleArn") {
        links.push(declared(arn, Ref::Id(NodeId(role.into())), Some(EdgeKind::AssumesRole), "states:role"));
    }
    for (to, kind) in task_targets(&def) {
        links.push(declared(arn, to, Some(kind), "states:definition"));
    }
    Extracted { node: Some(n), links }
}

/// Every service a state machine definition calls, walking Parallel and Map states.
pub fn task_targets(def: &Value) -> Vec<(Ref, EdgeKind)> {
    let mut out = Vec::new();
    walk_states(def, &mut out);
    out.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    out.dedup();
    out
}

fn walk_states(def: &Value, out: &mut Vec<(Ref, EdgeKind)>) {
    let Some(states) = def.get("States").and_then(|v| v.as_object()) else { return };
    for st in states.values() {
        for branch in arr(st, "Branches") {
            walk_states(branch, out);
        }
        for key in ["ItemProcessor", "Iterator"] {
            if let Some(inner) = st.get(key) {
                walk_states(inner, out);
            }
        }
        if s(st, "Type") != Some("Task") {
            continue;
        }
        let res = s(st, "Resource").unwrap_or_default();
        // `Parameters` (JSONPath) or `Arguments` (JSONata); keys ending in `.$` are dynamic.
        let params = st.get("Parameters").or(st.get("Arguments")).cloned().unwrap_or_default();
        let param = |k: &str| s(&params, k).filter(|v| !v.starts_with('$') && !v.starts_with("{%"));
        let svc = res.strip_prefix("arn:aws:states:::").map(|r| r.split('.').next().unwrap_or(r));
        match svc {
            None if res.starts_with("arn:aws:lambda") => out.push((Ref::Id(NodeId(lambda_arn(res))), EdgeKind::Invokes)),
            Some("lambda:invoke") => {
                if let Some(f) = param("FunctionName") {
                    let to = if f.starts_with("arn:") { Ref::Id(NodeId(lambda_arn(f))) } else { Ref::Alias(f.to_string()) };
                    out.push((to, EdgeKind::Invokes));
                }
            }
            Some("sqs:sendMessage") => {
                if let Some(q) = param("QueueUrl") {
                    out.push((Ref::Alias(q.trim_end_matches('/').to_string()), EdgeKind::Publishes));
                }
            }
            Some("sns:publish") => {
                if let Some(t) = param("TopicArn") {
                    out.push((Ref::Id(NodeId(t.into())), EdgeKind::Publishes));
                }
            }
            Some(op) if op.starts_with("dynamodb:") => {
                if let Some(t) = param("TableName") {
                    let kind = if op.contains("get") || op.contains("query") || op.contains("scan") { EdgeKind::Reads } else { EdgeKind::Writes };
                    out.push((Ref::Alias(format!("dynamodb:{t}")), kind));
                }
            }
            Some("states:startExecution") => {
                if let Some(m) = param("StateMachineArn") {
                    out.push((Ref::Id(NodeId(m.into())), EdgeKind::Invokes));
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ APIs and CDN

pub fn http_api_arn(region: &str, id: &str) -> String {
    format!("arn:aws:apigateway:{region}::/apis/{id}")
}

pub fn rest_api_arn(region: &str, id: &str) -> String {
    format!("arn:aws:apigateway:{region}::/restapis/{id}")
}

fn http_api(ctx: &ScanContext, p: &Value) -> Extracted {
    let Some(id) = s(p, "ApiId") else { return Extracted::default() };
    let arn = http_api_arn(&ctx.region, id);
    let mut n = node(ctx, &arn, NodeKind::ApiGateway, s(p, "Name").unwrap_or(id));
    n.aliases.push(id.to_string());
    if let Some(e) = s(p, "ApiEndpoint") {
        n.aliases.push(host(e));
    }
    let public = b(p, "DisableExecuteApiEndpoint") != Some(true);
    n.attrs = json!({ "protocol": s(p, "ProtocolType"), "endpoint": s(p, "ApiEndpoint"), "public": public });
    let links = if public { vec![internet(&arn, "apigateway:public_endpoint")] } else { vec![] };
    Extracted { node: Some(n), links }
}

fn http_integration(ctx: &ScanContext, p: &Value) -> Extracted {
    let (Some(api), Some(uri)) = (s(p, "ApiId"), s(p, "IntegrationUri")) else { return Extracted::default() };
    let links = integration_target(uri)
        .map(|(to, kind)| declared(&http_api_arn(&ctx.region, api), to, Some(kind), "apigateway:integration"))
        .into_iter()
        .collect();
    Extracted { node: None, links }
}

/// What an API Gateway integration URI points at.
pub fn integration_target(uri: &str) -> Option<(Ref, EdgeKind)> {
    if let Some(i) = uri.find("functions/") {
        let f = uri[i + 10..].split("/invocations").next()?;
        return Some((Ref::Id(NodeId(lambda_arn(f))), EdgeKind::Invokes));
    }
    if uri.starts_with("arn:aws:lambda") {
        return Some((Ref::Id(NodeId(lambda_arn(uri))), EdgeKind::Invokes));
    }
    if uri.starts_with("arn:aws:elasticloadbalancing") && uri.contains(":listener/") {
        // listener/app/<name>/<lb-id>/<listener-id> → loadbalancer/app/<name>/<lb-id>
        let lb = uri.replacen(":listener/", ":loadbalancer/", 1);
        let lb = lb.rsplit_once('/').map(|(a, _)| a.to_string())?;
        return Some((Ref::Id(NodeId(lb)), EdgeKind::RoutesTo));
    }
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return Some((Ref::External(host(uri)), EdgeKind::RoutesTo));
    }
    None
}

fn rest_api(ctx: &ScanContext, p: &Value) -> Extracted {
    let Some(id) = s(p, "RestApiId") else { return Extracted::default() };
    let arn = rest_api_arn(&ctx.region, id);
    let mut n = node(ctx, &arn, NodeKind::ApiGateway, s(p, "Name").unwrap_or(id));
    n.aliases.push(id.to_string());
    n.aliases.push(format!("{id}.execute-api.{}.amazonaws.com", ctx.region));
    let types = p.pointer("/EndpointConfiguration/Types").cloned().unwrap_or_default();
    let private = types.as_array().is_some_and(|t| t.iter().any(|x| x == "PRIVATE"));
    n.attrs = json!({ "endpoint_types": types, "public": !private });
    let links = if private { vec![] } else { vec![internet(&arn, "apigateway:public_endpoint")] };
    Extracted { node: Some(n), links }
}

fn distribution(ctx: &ScanContext, p: &Value) -> Extracted {
    let Some(id) = s(p, "Id") else { return Extracted::default() };
    let arn = format!("arn:aws:cloudfront::{}:distribution/{id}", ctx.account);
    let cfg = p.get("DistributionConfig").cloned().unwrap_or_default();
    let aliases = strs(&cfg, "Aliases");
    let name = aliases.first().map(String::as_str).or(s(p, "DomainName")).unwrap_or(id);
    let mut n = node(ctx, &arn, NodeKind::CloudFront, name);
    n.region = None;
    n.parent = Some(ctx.account_id());
    n.aliases.push(id.to_string());
    if let Some(d) = s(p, "DomainName") {
        n.aliases.push(d.to_ascii_lowercase());
    }
    n.aliases.extend(aliases.iter().map(|a| a.to_ascii_lowercase()));
    n.attrs = json!({
        "domain": s(p, "DomainName"),
        "aliases": aliases,
        "waf": s(&cfg, "WebACLId").is_some(),
        "enabled": b(&cfg, "Enabled"),
    });
    let mut links = vec![internet(&arn, "cloudfront:viewer")];
    for o in arr(&cfg, "Origins") {
        if let Some(d) = s(o, "DomainName") {
            links.push(link(&arn, Ref::External(d.to_ascii_lowercase()), Some(EdgeKind::RoutesTo), EvidenceKind::Declared, "cloudfront:origin", s(o, "Id").map(String::from)));
        }
    }
    Extracted { node: Some(n), links }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ScanContext {
        ScanContext { sdk: aws_config::SdkConfig::builder().build(), account: "123".into(), region: "us-east-1".into() }
    }

    fn ids(links: &[PendingLink]) -> Vec<String> {
        links.iter().map(|l| format!("{} {:?} {:?}", l.from.0, l.kind, l.to)).collect()
    }

    #[test]
    fn ecs_service_wires_lb_secrets_role_and_env() {
        let mut lk = Lookup::default();
        lk.learn("AWS::ElasticLoadBalancingV2::TargetGroup", &json!({"TargetGroupArn": "tg1", "LoadBalancerArns": ["lb1"]}));
        lk.learn("AWS::ECS::TaskDefinition", &json!({
            "TaskDefinitionArn": "td:1",
            "TaskRoleArn": "arn:aws:iam::123:role/api",
            "ContainerDefinitions": [{
                "Image": "123.dkr.ecr.us-east-1.amazonaws.com/api:4.2",
                "Environment": [{"Name": "ORDERS_TABLE", "Value": "orders"}, {"Name": "API_TOKEN", "Value": "abcd1234efgh"}],
                "Secrets": [{"Name": "DB_PASSWORD", "ValueFrom": "arn:aws:secretsmanager:us-east-1:123:secret:db-AbC123:password::"}]
            }]
        }));
        let p = json!({
            "ServiceArn": "arn:aws:ecs:us-east-1:123:service/prod/api", "ServiceName": "api", "Cluster": "arn:aws:ecs:us-east-1:123:cluster/prod",
            "DesiredCount": 3, "TaskDefinition": "td:1",
            "LoadBalancers": [{"TargetGroupArn": "tg1"}],
            "NetworkConfiguration": {"AwsvpcConfiguration": {"Subnets": ["subnet-a"], "SecurityGroups": ["sg-1"]}}
        });
        let e = extract(&ctx(), "AWS::ECS::Service", "", &p, &lk);
        let n = e.node.unwrap();
        assert!(n.aliases.contains(&"ecs:prod/api".to_string()));
        assert_eq!(n.attrs["env_plaintext_secret_keys"][0], "API_TOKEN");
        assert!(!n.attrs.to_string().contains("abcd1234efgh"), "values never stored");
        let l = ids(&e.links);
        assert!(l.iter().any(|x| x.starts_with("lb1 Some(RoutesTo)")), "{l:?}");
        assert!(l.iter().any(|x| x.contains("UsesSecret") && x.contains("secret:db-AbC123\"")), "{l:?}");
        assert!(l.iter().any(|x| x.contains("AssumesRole")), "{l:?}");
        assert!(l.iter().any(|x| x.contains("dynamodb:orders")), "{l:?}");
    }

    #[test]
    fn aurora_members_fold_into_cluster() {
        let mut lk = Lookup::default();
        for (i, az) in [("db-1", "us-east-1a"), ("db-2", "us-east-1b")] {
            lk.learn("AWS::RDS::DBInstance", &json!({"DBInstanceIdentifier": i, "DBClusterIdentifier": "Orders", "AvailabilityZone": az}));
        }
        assert!(extract(&ctx(), "AWS::RDS::DBInstance", "db-1", &json!({"DBClusterIdentifier": "Orders"}), &lk).node.is_none());
        let c = extract(&ctx(), "AWS::RDS::DBCluster", "orders", &json!({
            "DBClusterIdentifier": "orders", "DBClusterArn": "arn:aws:rds:us-east-1:123:cluster:orders",
            "Endpoint": {"Address": "orders.cluster-x.us-east-1.rds.amazonaws.com"}, "StorageEncrypted": true,
            "KmsKeyId": "arn:aws:kms:us-east-1:123:key/k1"
        }), &lk);
        let n = c.node.unwrap();
        assert_eq!(n.attrs["multi_az"], true);
        assert_eq!(n.attrs["instances"], 2);
        assert!(n.aliases.contains(&"db-2".to_string()), "instance ids resolve to the cluster for metrics");
        assert_eq!(c.links.len(), 1);
    }

    #[test]
    fn internet_facing_lb_gets_entry_edge_and_metric_alias() {
        let e = extract(&ctx(), "AWS::ElasticLoadBalancingV2::LoadBalancer", "", &json!({
            "LoadBalancerArn": "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/web/abc", "Scheme": "internet-facing",
            "DNSName": "Web-1.us-east-1.elb.amazonaws.com", "SecurityGroups": ["sg-1"], "Subnets": ["subnet-a", "subnet-b"]
        }), &Lookup::default());
        let n = e.node.unwrap();
        assert!(n.aliases.contains(&"app/web/abc".to_string()));
        assert!(n.aliases.contains(&"web-1.us-east-1.elb.amazonaws.com".to_string()));
        assert_eq!(e.links[0].from.0, "internet");
    }

    #[test]
    fn step_functions_definition_is_walked() {
        let def = json!({"States": {
            "Fan": {"Type": "Parallel", "Branches": [{"States": {"A": {"Type": "Task", "Resource": "arn:aws:lambda:us-east-1:123:function:a:live"}}}]},
            "B": {"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke", "Parameters": {"FunctionName": "arn:aws:lambda:us-east-1:123:function:b:$LATEST", "Payload.$": "$"}},
            "C": {"Type": "Task", "Resource": "arn:aws:states:::dynamodb:putItem", "Parameters": {"TableName": "invoices"}},
            "D": {"Type": "Task", "Resource": "arn:aws:states:::sqs:sendMessage.waitForTaskToken", "Parameters": {"QueueUrl": "https://sqs.us-east-1.amazonaws.com/123/q"}},
            "E": {"Type": "Map", "ItemProcessor": {"States": {"F": {"Type": "Task", "Resource": "arn:aws:states:::sns:publish", "Parameters": {"TopicArn": "arn:aws:sns:us-east-1:123:t"}}}}},
            "G": {"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke", "Parameters": {"FunctionName.$": "$.fn"}}
        }});
        let t = task_targets(&def);
        let s = format!("{t:?}");
        assert_eq!(t.len(), 5, "{s}");
        assert!(s.contains("function:a\""), "qualifier stripped: {s}");
        assert!(s.contains("function:b\""), "{s}");
        assert!(s.contains("dynamodb:invoices") && s.contains("Writes"), "{s}");
    }

    #[test]
    fn integration_uris() {
        let l = integration_target("arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:123:function:f/invocations").unwrap();
        assert_eq!(l.0, Ref::Id(NodeId("arn:aws:lambda:us-east-1:123:function:f".into())));
        let l = integration_target("arn:aws:elasticloadbalancing:us-east-1:123:listener/app/web/abc/lis1").unwrap();
        assert_eq!(l.0, Ref::Id(NodeId("arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/web/abc".into())));
        assert_eq!(integration_target("https://api.stripe.com/v1").unwrap().0, Ref::External("api.stripe.com".into()));
    }

    #[test]
    fn nat_and_elastic_ips() {
        let mut lk = Lookup::default();
        lk.learn("AWS::EC2::NatGateway", &json!({"NatGatewayId": "nat-1", "AllocationId": "eipalloc-nat"}));
        let nat = extract(&ctx(), "AWS::EC2::NatGateway", "nat-1", &json!({"NatGatewayId": "nat-1", "SubnetId": "subnet-a"}), &lk).node.unwrap();
        assert_eq!(nat.attrs["subnets"][0], "subnet-a");
        let free = extract(&ctx(), "AWS::EC2::EIP", "eipalloc-1", &json!({"AllocationId": "eipalloc-1", "PublicIp": "203.0.113.9"}), &lk).node.unwrap();
        assert_eq!(free.attrs["associated"], false);
        assert_eq!(free.name, "203.0.113.9");
        let used_by_nat = extract(&ctx(), "AWS::EC2::EIP", "eipalloc-nat", &json!({"AllocationId": "eipalloc-nat"}), &lk).node.unwrap();
        assert_eq!(used_by_nat.attrs["associated"], true);
    }

    #[test]
    fn queue_with_dead_letter_queue() {
        let e = extract(&ctx(), "AWS::SQS::Queue", "https://sqs.us-east-1.amazonaws.com/123/orders", &json!({
            "Arn": "arn:aws:sqs:us-east-1:123:orders", "QueueUrl": "https://sqs.us-east-1.amazonaws.com/123/orders",
            "RedrivePolicy": "{\"deadLetterTargetArn\":\"arn:aws:sqs:us-east-1:123:orders-dlq\",\"maxReceiveCount\":5}"
        }), &Lookup::default());
        assert_eq!(e.node.unwrap().attrs["max_receive_count"], 5);
        assert_eq!(e.links[0].evidence.detail.as_deref(), Some("dead-letter queue"));
    }

    #[test]
    fn cloudfront_origins_are_external_until_resolved() {
        let e = extract(&ctx(), "AWS::CloudFront::Distribution", "E1", &json!({
            "Id": "E1", "DomainName": "d1.cloudfront.net",
            "DistributionConfig": {"Aliases": ["shop.example.com"], "Origins": [{"Id": "s3", "DomainName": "assets.s3.us-east-1.amazonaws.com"}]}
        }), &Lookup::default());
        assert_eq!(e.node.unwrap().name, "shop.example.com");
        assert_eq!(e.links.len(), 2);
        assert_eq!(e.links[1].to, Ref::External("assets.s3.us-east-1.amazonaws.com".into()));
    }

    #[test]
    fn s3_bucket_public_access() {
        let up = extract(&ctx(), "AWS::S3::Bucket", "uploads", &json!({
            "BucketName": "uploads",
            "NotificationConfiguration": {
                "QueueConfigurations": [{"Event": "s3:ObjectCreated:*", "Queue": "arn:aws:sqs:us-east-1:123:jobs"}],
                "LambdaConfigurations": [{"Event": "s3:ObjectRemoved:*", "Function": "arn:aws:lambda:us-east-1:123:function:Cleanup:live"}]
            }
        }), &Lookup::default());
        let l: Vec<String> = up.links.iter().map(|l| format!("{:?} {:?}", l.kind, l.to)).collect();
        assert!(l.iter().any(|x| x.contains("Publishes") && x.contains("jobs")), "{l:?}");
        assert!(l.iter().any(|x| x.contains("Invokes") && x.contains("function:Cleanup\")")), "alias suffix dropped: {l:?}");

        let i = extract(&ctx(), "AWS::EC2::Instance", "i-0abc", &json!({
            "InstanceId": "i-0abc", "InstanceType": "t3.large", "SubnetId": "subnet-a",
            "SecurityGroupIds": ["sg-1"], "IamInstanceProfile": "reports-worker-profile",
            "Tags": [{"Key": "Name", "Value": "reports-worker"}], "State": {"Name": "running"}
        }), &Lookup::default());
        let inst = i.node.unwrap();
        assert_eq!(inst.name, "reports-worker");
        assert_eq!(inst.attrs["instance_type"], "t3.large");
        assert!(format!("{:?}", i.links[0].to).contains("reports-worker-profile"));

        let e = extract(&ctx(), "AWS::S3::Bucket", "assets", &json!({
            "BucketName": "assets", "RegionalDomainName": "assets.s3.us-east-1.amazonaws.com",
            "PublicAccessBlockConfiguration": {"BlockPublicAcls": true, "BlockPublicPolicy": true, "IgnorePublicAcls": true, "RestrictPublicBuckets": false}
        }), &Lookup::default());
        let n = e.node.unwrap();
        assert_eq!(n.attrs["public_access_block"], false);
        assert!(n.aliases.contains(&"assets.s3.us-east-1.amazonaws.com".to_string()));
    }
}
