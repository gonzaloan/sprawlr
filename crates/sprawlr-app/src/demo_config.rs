//! The demo account's configuration, as Cloud Control would return it, for exports. Fictional,
//! with the real CloudFormation property names, so the demo's code passes `terraform validate`.
//! Values a real account would keep secret are here too (an API key in an environment
//! variable, user data), to show that an export never writes them.

use serde_json::{json, Value};
use sprawlr_domain::{ArchGraph, Node, NodeId, NodeKind};
use std::collections::BTreeMap;

const ACCT: &str = "111122223333";
const REGION: &str = "us-east-1";

fn arn(service: &str, resource: &str) -> String {
    format!("arn:aws:{service}:{REGION}:{ACCT}:{resource}")
}

fn last(id: &NodeId) -> String {
    id.0.rsplit(['/', ':']).next().unwrap_or_default().to_string()
}

fn tags(n: &Node) -> Value {
    Value::Array(n.tags.iter().map(|(k, v)| json!({ "Key": k, "Value": v })).collect())
}

fn named(n: &Node) -> Value {
    json!([{ "Key": "Name", "Value": n.name }])
}

/// The id of the subnet with this name.
fn subnet_id(g: &ArchGraph, name: &str) -> Option<String> {
    g.nodes().find(|s| s.kind == NodeKind::Subnet && s.name == name).map(|s| last(&s.id))
}

fn subnets(g: &ArchGraph, n: &Node) -> Value {
    let names: Vec<String> =
        n.attrs.get("subnets").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    Value::Array(names.iter().filter_map(|s| subnet_id(g, s)).map(Value::String).collect())
}

fn security_groups(n: &Node) -> Value {
    let ids = n.attrs.get("security_groups").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
    Value::Array(ids.map(|a| Value::String(a.rsplit('/').next().unwrap_or(a).to_string())).collect())
}

fn statement(action: &str, resource: impl Into<Value>) -> Value {
    json!({ "Effect": "Allow", "Action": action, "Resource": resource.into() })
}

fn document(statements: Vec<Value>) -> Value {
    json!({ "Version": "2012-10-17", "Statement": statements })
}

fn service_trust(service: &str) -> Value {
    document(vec![json!({ "Effect": "Allow", "Principal": { "Service": service }, "Action": "sts:AssumeRole" })])
}

fn managed(names: &Value) -> Value {
    let arn = |n: &str| match n {
        "AWSLambdaBasicExecutionRole" => format!("arn:aws:iam::aws:policy/service-role/{n}"),
        _ => format!("arn:aws:iam::aws:policy/{n}"),
    };
    Value::Array(names.as_array().into_iter().flatten().filter_map(Value::as_str).map(|n| Value::String(arn(n))).collect())
}

fn lambda_env(name: &str) -> Value {
    let topic = arn("sns", "checkout-notifications");
    let search = "https://vpc-search-products-7hq2.us-east-1.es.amazonaws.com";
    match name {
        "checkout-email-sender" => json!({ "NOTIFICATIONS_TOPIC_ARN": topic, "SENDER_ADDRESS": "orders@example.com" }),
        "search-query-handler" => json!({ "OPENSEARCH_ENDPOINT": search }),
        "search-indexer" => json!({ "OPENSEARCH_ENDPOINT": search, "INDEX_NAME": "products" }),
        "report-generator" => {
            json!({ "DB_HOST": "checkout-orders-db-dev.cluster-c9x.us-east-1.rds.amazonaws.com", "REPORTS_API_TOKEN": "demo-token-not-real" })
        }
        "legacy-export-job" => json!({ "EXPORT_BUCKET": "reports-output" }),
        _ => json!({}),
    }
}

fn role(name: &str) -> Value {
    let account_condition = json!({ "StringEquals": { "aws:SourceAccount": ACCT } });
    match name {
        "checkout-api-task-role" => json!({
            "AssumeRolePolicyDocument": document(vec![json!({ "Effect": "Allow", "Principal": { "Service": "ecs-tasks.amazonaws.com" },
                "Action": "sts:AssumeRole", "Condition": account_condition })]),
            "Description": "What checkout-api's tasks may do",
            "Policies": [{ "PolicyName": "checkout-api-task-policy", "PolicyDocument": document(vec![
                statement("sqs:SendMessage", arn("sqs", "checkout-order-events")),
                statement("sns:Publish", arn("sns", "checkout-notifications")),
                statement("events:PutEvents", arn("events", "event-bus/platform-events")),
                statement("secretsmanager:GetSecretValue", arn("secretsmanager", "secret:checkout/prod/db-credentials-AbC123")),
            ]) }]
        }),
        "reports-worker-role" => json!({
            "AssumeRolePolicyDocument": service_trust("ec2.amazonaws.com"),
            "Policies": [{ "PolicyName": "reports-worker-inline", "PolicyDocument": document(vec![
                json!({ "Effect": "Allow", "Action": ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:ChangeMessageVisibility"], "Resource": arn("sqs", "reports-jobs") }),
                statement("s3:GetObject", "arn:aws:s3:::reports-uploads/*"),
                json!({ "Effect": "Allow", "Action": ["s3:PutObject", "s3:PutObjectAcl"], "Resource": "arn:aws:s3:::reports-output/*" }),
                statement("dynamodb:*", "*"),
            ]) }]
        }),
        "ci-deploy-2023" => json!({
            "AssumeRolePolicyDocument": document(vec![json!({ "Effect": "Allow", "Action": "sts:AssumeRoleWithWebIdentity",
                "Principal": { "Federated": format!("arn:aws:iam::{ACCT}:oidc-provider/token.actions.githubusercontent.com") },
                "Condition": { "StringEquals": { "token.actions.githubusercontent.com:aud": "sts.amazonaws.com" },
                               "StringLike": { "token.actions.githubusercontent.com:sub": "repo:acme/storefront:*" } } })]),
            "Description": "Deploys from GitHub Actions"
        }),
        _ => json!({ "AssumeRolePolicyDocument": service_trust("lambda.amazonaws.com") }),
    }
}

fn merge(mut base: Value, extra: Value) -> Value {
    if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    base
}

/// The configuration Cloud Control would return for a demo resource.
pub fn configuration(g: &ArchGraph, n: &Node, type_name: &str) -> Option<Value> {
    let id = last(&n.id);
    let a = &n.attrs;
    let parent = n.parent.as_ref().map(last).unwrap_or_default();
    let kms = arn("kms", "key/1f2e3d4c-data");
    let alb_dns = "checkout-alb-1234567890.us-east-1.elb.amazonaws.com";
    let props = match type_name {
        "AWS::EC2::VPC" => json!({ "VpcId": id, "CidrBlock": a["cidr"], "EnableDnsHostnames": true, "EnableDnsSupport": true,
            "InstanceTenancy": "default", "DefaultSecurityGroup": "sg-0default", "Tags": tags(n) }),
        "AWS::EC2::Subnet" => {
            let cidr = match n.name.as_str() {
                "prod-public-a" => "10.0.0.0/24",
                "prod-public-b" => "10.0.1.0/24",
                "prod-private-a" => "10.0.10.0/24",
                "prod-private-b" => "10.0.11.0/24",
                _ => "10.1.10.0/24",
            };
            json!({ "SubnetId": id, "VpcId": parent, "CidrBlock": cidr, "AvailabilityZone": a["az"],
                "MapPublicIpOnLaunch": a["auto_public_ip"], "Tags": named(n) })
        }
        "AWS::EC2::SecurityGroup" => {
            let ingress = match id.as_str() {
                "sg-alb" => json!({ "IpProtocol": "tcp", "FromPort": 443, "ToPort": 443, "CidrIp": "0.0.0.0/0" }),
                "sg-api" => json!({ "IpProtocol": "tcp", "FromPort": 8080, "ToPort": 8080, "SourceSecurityGroupId": "sg-alb" }),
                "sg-db" => json!({ "IpProtocol": "tcp", "FromPort": 5432, "ToPort": 5432, "SourceSecurityGroupId": "sg-api" }),
                _ => json!({ "IpProtocol": "tcp", "FromPort": 5432, "ToPort": 5432, "CidrIp": "10.0.0.0/16" }),
            };
            json!({ "Id": id, "GroupId": id, "GroupName": n.name, "GroupDescription": format!("{} security group", n.name), "VpcId": parent,
                "SecurityGroupIngress": [ingress], "SecurityGroupEgress": [{ "IpProtocol": "-1", "CidrIp": "0.0.0.0/0" }] })
        }
        "AWS::EC2::NatGateway" => json!({ "NatGatewayId": id, "SubnetId": "subnet-dev-a", "AllocationId": "eipalloc-0nat",
            "ConnectivityType": "public", "Tags": named(n) }),
        "AWS::EC2::EIP" => json!({ "PublicIp": n.name, "AllocationId": id, "Domain": "vpc" }),
        "AWS::EC2::Instance" => json!({ "InstanceId": id, "ImageId": "ami-0c1a2b3c4d5e6f708", "InstanceType": a["instance_type"],
            "SubnetId": subnets(g, n)[0], "SecurityGroupIds": security_groups(n), "IamInstanceProfile": "reports-worker-profile",
            "UserData": "IyEvYmluL2Jhc2gKZXhwb3J0IFJFUE9SVFNfVE9LRU49bm90LXJlYWwK", "PrivateIp": "10.0.10.25", "Tags": tags(n) }),
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => json!({ "LoadBalancerArn": n.id.0, "LoadBalancerName": n.name, "Name": n.name,
            "DNSName": alb_dns, "Scheme": a["scheme"], "Type": "application", "IpAddressType": "ipv4",
            "Subnets": subnets(g, n), "SecurityGroups": security_groups(n),
            "LoadBalancerAttributes": [{ "Key": "idle_timeout.timeout_seconds", "Value": "60" }], "Tags": tags(n) }),
        "AWS::ECS::Service" => {
            let cluster = a["cluster"].as_str().unwrap_or_default();
            let network = if cluster == "dev-cluster" {
                json!({ "AssignPublicIp": "DISABLED", "Subnets": ["subnet-dev-a"], "SecurityGroups": [] })
            } else {
                json!({ "AssignPublicIp": "DISABLED", "Subnets": subnets(g, n), "SecurityGroups": security_groups(n) })
            };
            json!({ "ServiceArn": n.id.0, "Name": n.name, "ServiceName": n.name, "Cluster": arn("ecs", &format!("cluster/{cluster}")),
                "TaskDefinition": arn("ecs", &format!("task-definition/{}", a["task_definition"].as_str().unwrap_or_default())),
                "DesiredCount": a["desired"], "LaunchType": a["launch_type"], "PropagateTags": "SERVICE",
                "NetworkConfiguration": { "AwsvpcConfiguration": network }, "Tags": tags(n) })
        }
        "AWS::RDS::DBCluster" => {
            let prod = a["multi_az"] == json!(true);
            json!({ "DBClusterArn": n.id.0, "DBClusterIdentifier": n.name, "DBClusterResourceId": "cluster-ABCDEFGHIJ",
                "Engine": a["engine"], "EngineVersion": "16.4", "DatabaseName": "orders", "MasterUsername": "orders_admin",
                "ManageMasterUserPassword": true, "StorageEncrypted": a["encrypted"], "KmsKeyId": kms, "Port": 5432,
                "VpcSecurityGroupIds": if prod { json!(["sg-db"]) } else { json!([]) },
                "DBSubnetGroupName": if prod { "orders-db-subnets" } else { "orders-db-dev-subnets" },
                "BackupRetentionPeriod": if prod { 7 } else { 1 }, "DeletionProtection": prod,
                "Endpoint": { "Address": format!("{}.cluster-c9x.us-east-1.rds.amazonaws.com", n.name), "Port": "5432" }, "Tags": tags(n) })
        }
        "AWS::CloudFront::Distribution" => json!({ "Id": id, "DomainName": a["domain"], "Tags": tags(n), "DistributionConfig": {
            "Enabled": true, "Comment": "checkout storefront", "HttpVersion": "http2",
            "Origins": [{ "Id": "checkout-alb", "DomainName": alb_dns, "CustomOriginConfig": { "OriginProtocolPolicy": "https-only", "HTTPSPort": 443 } }],
            "DefaultCacheBehavior": { "TargetOriginId": "checkout-alb", "ViewerProtocolPolicy": "redirect-to-https",
                "CachePolicyId": "4135ea2d-6df8-44a3-9df3-4b5a84be39ad" } } }),
        "AWS::ApiGateway::RestApi" => {
            json!({ "RestApiId": id, "RootResourceId": "r00t", "Name": n.name, "Description": "Public product search",
            "EndpointConfiguration": { "Types": [a["endpoint_type"]] } })
        }
        "AWS::Lambda::Function" => {
            let in_vpc = n.name.starts_with("search-");
            let handler = if a["runtime"].as_str().is_some_and(|r| r.starts_with("java")) {
                "com.acme.Handler::handleRequest"
            } else {
                "index.handler"
            };
            let role = match n.name.as_str() {
                "checkout-email-sender" => "checkout-email-sender-role",
                "report-generator" => "report-generator-role",
                "legacy-export-job" => "legacy-export-role",
                _ => "search-lambda-role",
            };
            let mut p = json!({ "Arn": n.id.0, "FunctionName": n.name, "Runtime": a["runtime"], "Handler": handler,
                "MemorySize": a.get("memory_mb").cloned().unwrap_or(json!(128)), "Timeout": a.get("timeout_s").cloned().unwrap_or(json!(30)),
                "Role": format!("arn:aws:iam::{ACCT}:role/{role}"), "PackageType": "Zip", "Architectures": ["arm64"],
                "TracingConfig": { "Mode": "Active" }, "Environment": { "Variables": lambda_env(&n.name) }, "Tags": tags(n) });
            if in_vpc {
                p["VpcConfig"] = json!({ "SubnetIds": ["subnet-priv-a", "subnet-priv-b"], "SecurityGroupIds": ["sg-api"] });
            }
            p
        }
        "AWS::SQS::Queue" => {
            let mut p = json!({ "Arn": n.id.0, "QueueUrl": format!("https://sqs.{REGION}.amazonaws.com/{ACCT}/{}", n.name), "QueueName": n.name,
                "VisibilityTimeout": a.get("visibility_timeout_s").cloned().unwrap_or(json!(30)), "MessageRetentionPeriod": 345600,
                "SqsManagedSseEnabled": true, "Tags": tags(n) });
            if let Some(dlq) = a.get("dead_letter_queue") {
                p["RedrivePolicy"] = json!({ "deadLetterTargetArn": dlq, "maxReceiveCount": 5 });
            }
            p
        }
        "AWS::SNS::Topic" => json!({ "TopicArn": n.id.0, "TopicName": n.name, "DisplayName": "Checkout notifications",
            "KmsMasterKeyId": "alias/aws/sns", "Subscription": [{ "Endpoint": arn("lambda", "function:checkout-email-sender"), "Protocol": "lambda" }],
            "Tags": tags(n) }),
        "AWS::Events::EventBus" => json!({ "Arn": n.id.0, "Name": n.name }),
        "AWS::SecretsManager::Secret" => {
            json!({ "Id": n.id.0, "Name": n.name, "Description": "Credentials checkout-api uses for the orders database",
            "KmsKeyId": kms, "Tags": tags(n) })
        }
        "AWS::IAM::Role" => merge(
            json!({ "Arn": n.id.0, "RoleId": "AROAEXAMPLEID", "RoleName": n.name, "Path": "/", "MaxSessionDuration": 3600,
                "ManagedPolicyArns": managed(&a["aws_managed_policies"]) }),
            role(&n.name),
        ),
        "AWS::KMS::Key" => json!({ "Arn": n.id.0, "KeyId": id, "Description": "Encrypts checkout and reporting data", "Enabled": true,
            "EnableKeyRotation": a["rotation_enabled"], "KeySpec": "SYMMETRIC_DEFAULT", "KeyUsage": "ENCRYPT_DECRYPT", "MultiRegion": false,
            "KeyPolicy": { "Version": "2012-10-17", "Id": "key-default-1", "Statement": [{ "Sid": "Enable IAM User Permissions", "Effect": "Allow",
                "Principal": { "AWS": format!("arn:aws:iam::{ACCT}:root") }, "Action": "kms:*", "Resource": "*" }] } }),
        "AWS::S3::Bucket" => {
            let mut p = json!({ "Arn": n.id.0, "BucketName": n.name, "DomainName": format!("{}.s3.amazonaws.com", n.name),
                "PublicAccessBlockConfiguration": { "BlockPublicAcls": true, "BlockPublicPolicy": true, "IgnorePublicAcls": true, "RestrictPublicBuckets": true },
                "BucketEncryption": { "ServerSideEncryptionConfiguration": [{ "ServerSideEncryptionByDefault": { "SSEAlgorithm": "AES256" }, "BucketKeyEnabled": false }] },
                "Tags": tags(n) });
            if a["versioning"] == json!(true) {
                p["VersioningConfiguration"] = json!({ "Status": "Enabled" });
            }
            if n.name == "reports-uploads" {
                p["NotificationConfiguration"] =
                    json!({ "QueueConfigurations": [{ "Event": "s3:ObjectCreated:*", "Queue": arn("sqs", "reports-jobs") }] });
            }
            p
        }
        "AWS::OpenSearchService::Domain" => json!({ "Arn": n.id.0, "DomainArn": n.id.0, "DomainName": n.name, "Id": n.name,
            "DomainEndpoint": "vpc-search-products-7hq2.us-east-1.es.amazonaws.com", "EngineVersion": a["engine"],
            "ClusterConfig": { "InstanceType": "r6g.large.search", "InstanceCount": a["instances"], "ZoneAwarenessEnabled": true,
                "ZoneAwarenessConfig": { "AvailabilityZoneCount": 2 } },
            "EBSOptions": { "EBSEnabled": true, "VolumeSize": 100, "VolumeType": "gp3" },
            "VPCOptions": { "SubnetIds": ["subnet-priv-a", "subnet-priv-b"], "SecurityGroupIds": ["sg-api"] },
            "EncryptionAtRestOptions": { "Enabled": true, "KmsKeyId": "1f2e3d4c-data" }, "NodeToNodeEncryptionOptions": { "Enabled": true } }),
        _ => return None,
    };
    Some(props)
}

/// Each demo type's schema, with what an export needs from the real one: what AWS sets, what
/// it never returns, what is required and what identifies a resource.
pub fn schemas() -> BTreeMap<String, Value> {
    let tag =
        json!({ "type": "object", "properties": { "Key": {"type": "string"}, "Value": {"type": "string"} }, "required": ["Key", "Value"] });
    let s = |t: &str, read_only: &[&str], write_only: &[&str], required: &[&str], primary: &[&str], properties: Value| {
        let p = |list: &[&str]| list.iter().map(|x| format!("/properties/{x}")).collect::<Vec<_>>();
        (
            t.to_string(),
            json!({ "typeName": t, "definitions": { "Tag": tag }, "properties": properties,
                "readOnlyProperties": p(read_only), "writeOnlyProperties": p(write_only),
                "required": required, "primaryIdentifier": p(primary) }),
        )
    };
    let tags = json!({ "Tags": { "type": "array", "items": { "$ref": "#/definitions/Tag" } } });
    BTreeMap::from([
        s(
            "AWS::EC2::VPC",
            &["VpcId", "DefaultSecurityGroup", "CidrBlockAssociations", "DefaultNetworkAcl", "Ipv6CidrBlocks"],
            &[],
            &[],
            &["VpcId"],
            tags.clone(),
        ),
        s("AWS::EC2::Subnet", &["SubnetId", "NetworkAclAssociationId", "Ipv6CidrBlocks"], &[], &["VpcId"], &["SubnetId"], tags.clone()),
        s("AWS::EC2::SecurityGroup", &["GroupId", "Id"], &[], &["GroupDescription"], &["Id"], tags.clone()),
        s("AWS::EC2::NatGateway", &["NatGatewayId"], &[], &["SubnetId"], &["NatGatewayId"], tags.clone()),
        s("AWS::EC2::EIP", &["PublicIp", "AllocationId"], &[], &[], &["PublicIp", "AllocationId"], tags.clone()),
        s(
            "AWS::EC2::Instance",
            &["InstanceId", "PrivateIp", "PublicIp", "PrivateDnsName", "PublicDnsName", "VpcId"],
            &[],
            &[],
            &["InstanceId"],
            tags.clone(),
        ),
        s(
            "AWS::ElasticLoadBalancingV2::LoadBalancer",
            &["LoadBalancerArn", "DNSName", "CanonicalHostedZoneID", "LoadBalancerFullName", "LoadBalancerName"],
            &[],
            &[],
            &["LoadBalancerArn"],
            tags.clone(),
        ),
        s("AWS::ECS::Service", &["ServiceArn", "Name"], &[], &[], &["ServiceArn", "Cluster"], tags.clone()),
        s(
            "AWS::RDS::DBCluster",
            &["DBClusterArn", "DBClusterResourceId", "Endpoint", "ReadEndpoint"],
            &["MasterUserPassword"],
            &["Engine"],
            &["DBClusterIdentifier"],
            tags.clone(),
        ),
        s("AWS::CloudFront::Distribution", &["Id", "DomainName"], &[], &["DistributionConfig"], &["Id"], tags.clone()),
        s("AWS::ApiGateway::RestApi", &["RestApiId", "RootResourceId"], &["Body", "BodyS3Location"], &[], &["RestApiId"], tags.clone()),
        s(
            "AWS::Lambda::Function",
            &["Arn", "SnapStartResponse"],
            &["Code"],
            &["Code", "Role"],
            &["FunctionName"],
            merge(
                tags.clone(),
                json!({ "Environment": { "type": "object", "properties": { "Variables": { "type": "object",
                    "patternProperties": { "[a-zA-Z][a-zA-Z0-9_]+": { "type": "string" } } } } } }),
            ),
        ),
        s(
            "AWS::SQS::Queue",
            &["Arn", "QueueUrl"],
            &[],
            &[],
            &["QueueUrl"],
            merge(tags.clone(), json!({ "RedrivePolicy": { "type": ["object", "string"] } })),
        ),
        s("AWS::SNS::Topic", &["TopicArn"], &[], &[], &["TopicArn"], tags.clone()),
        s("AWS::Events::EventBus", &["Arn"], &[], &["Name"], &["Name"], tags.clone()),
        s("AWS::SecretsManager::Secret", &["Id"], &["SecretString", "GenerateSecretString"], &[], &["Id"], tags.clone()),
        s("AWS::IAM::Role", &["Arn", "RoleId"], &[], &["AssumeRolePolicyDocument"], &["RoleName"], tags.clone()),
        s("AWS::KMS::Key", &["Arn", "KeyId"], &["PendingWindowInDays", "BypassPolicyLockoutSafetyCheck"], &[], &["KeyId"], tags.clone()),
        s(
            "AWS::S3::Bucket",
            &["Arn", "DomainName", "DualStackDomainName", "RegionalDomainName", "WebsiteURL"],
            &[],
            &[],
            &["BucketName"],
            tags.clone(),
        ),
        s(
            "AWS::OpenSearchService::Domain",
            &["Arn", "DomainArn", "DomainEndpoint", "DomainEndpoints", "Id", "ServiceSoftwareOptions"],
            &[],
            &[],
            &["DomainName"],
            tags,
        ),
    ])
}
