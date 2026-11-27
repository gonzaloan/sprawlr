# shared: written by sprawlr on 2026-11-27 from the configuration of account 111122223333.

resource "awscc_ec2_eip" "eip_203_0_113_24" {
  domain = "vpc"
}

resource "awscc_ec2_nat_gateway" "nat_legacy_dev" {
  allocation_id     = "eipalloc-0nat"
  connectivity_type = "public"
  subnet_id         = awscc_ec2_subnet.dev_private_a.subnet_id
  tags = [
    {
      key   = "Name"
      value = "nat-legacy-dev"
    },
  ]
}

resource "awscc_ec2_security_group" "checkout_alb_sg" {
  group_description = "checkout-alb-sg security group"
  group_name        = "checkout-alb-sg"
  security_group_egress = [
    {
      cidr_ip     = "0.0.0.0/0"
      ip_protocol = "-1"
    },
  ]
  security_group_ingress = [
    {
      cidr_ip     = "0.0.0.0/0"
      from_port   = 443
      ip_protocol = "tcp"
      to_port     = 443
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_security_group" "checkout_api_sg" {
  group_description = "checkout-api-sg security group"
  group_name        = "checkout-api-sg"
  security_group_egress = [
    {
      cidr_ip     = "0.0.0.0/0"
      ip_protocol = "-1"
    },
  ]
  security_group_ingress = [
    {
      from_port                = 8080
      ip_protocol              = "tcp"
      source_security_group_id = awscc_ec2_security_group.checkout_alb_sg.group_id
      to_port                  = 8080
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_security_group" "migration_2024_sg" {
  group_description = "migration-2024-sg security group"
  group_name        = "migration-2024-sg"
  security_group_egress = [
    {
      cidr_ip     = "0.0.0.0/0"
      ip_protocol = "-1"
    },
  ]
  security_group_ingress = [
    {
      cidr_ip     = "10.0.0.0/16"
      from_port   = 5432
      ip_protocol = "tcp"
      to_port     = 5432
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_security_group" "orders_db_sg" {
  group_description = "orders-db-sg security group"
  group_name        = "orders-db-sg"
  security_group_egress = [
    {
      cidr_ip     = "0.0.0.0/0"
      ip_protocol = "-1"
    },
  ]
  security_group_ingress = [
    {
      from_port                = 5432
      ip_protocol              = "tcp"
      source_security_group_id = awscc_ec2_security_group.checkout_api_sg.group_id
      to_port                  = 5432
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_subnet" "dev_private_a" {
  availability_zone       = "us-east-1a"
  cidr_block              = "10.1.10.0/24"
  map_public_ip_on_launch = false
  tags = [
    {
      key   = "Name"
      value = "dev-private-a"
    },
  ]
  vpc_id = awscc_ec2_vpc.dev_vpc.vpc_id
}

resource "awscc_ec2_subnet" "prod_private_a" {
  availability_zone       = "us-east-1a"
  cidr_block              = "10.0.10.0/24"
  map_public_ip_on_launch = false
  tags = [
    {
      key   = "Name"
      value = "prod-private-a"
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_subnet" "prod_private_b" {
  availability_zone       = "us-east-1b"
  cidr_block              = "10.0.11.0/24"
  map_public_ip_on_launch = false
  tags = [
    {
      key   = "Name"
      value = "prod-private-b"
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_subnet" "prod_public_a" {
  availability_zone       = "us-east-1a"
  cidr_block              = "10.0.0.0/24"
  map_public_ip_on_launch = true
  tags = [
    {
      key   = "Name"
      value = "prod-public-a"
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_subnet" "prod_public_b" {
  availability_zone       = "us-east-1b"
  cidr_block              = "10.0.1.0/24"
  map_public_ip_on_launch = true
  tags = [
    {
      key   = "Name"
      value = "prod-public-b"
    },
  ]
  vpc_id = awscc_ec2_vpc.prod_vpc.vpc_id
}

resource "awscc_ec2_vpc" "dev_vpc" {
  cidr_block           = "10.1.0.0/16"
  enable_dns_hostnames = true
  enable_dns_support   = true
  instance_tenancy     = "default"
  tags = [
    {
      key   = "Name"
      value = "dev-vpc"
    },
  ]
}

resource "awscc_ec2_vpc" "prod_vpc" {
  cidr_block           = "10.0.0.0/16"
  enable_dns_hostnames = true
  enable_dns_support   = true
  instance_tenancy     = "default"
  tags = [
    {
      key   = "Name"
      value = "prod-vpc"
    },
  ]
}

resource "awscc_events_event_bus" "platform_events" {
  name = "platform-events"
}

resource "awscc_iam_role" "ci_deploy_2023" {
  assume_role_policy_document = jsonencode({
    "Statement" = [
      {
        "Action" = "sts:AssumeRoleWithWebIdentity"
        "Condition" = {
          "StringEquals" = {
            "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
          }
          "StringLike" = {
            "token.actions.githubusercontent.com:sub" = "repo:acme/storefront:*"
          }
        }
        "Effect" = "Allow"
        "Principal" = {
          "Federated" = "arn:aws:iam::111122223333:oidc-provider/token.actions.githubusercontent.com"
        }
      },
    ]
    "Version" = "2012-10-17"
  })
  description          = "Deploys from GitHub Actions"
  managed_policy_arns  = ["arn:aws:iam::aws:policy/AdministratorAccess"]
  max_session_duration = 3600
  path                 = "/"
  role_name            = "ci-deploy-2023"
}

resource "awscc_iam_role" "reports_worker_role" {
  assume_role_policy_document = jsonencode({
    "Statement" = [
      {
        "Action" = "sts:AssumeRole"
        "Effect" = "Allow"
        "Principal" = {
          "Service" = "ec2.amazonaws.com"
        }
      },
    ]
    "Version" = "2012-10-17"
  })
  managed_policy_arns  = ["arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy"]
  max_session_duration = 3600
  path                 = "/"
  policies = [
    {
      policy_document = jsonencode({
        "Statement" = [
          {
            "Action"   = ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:ChangeMessageVisibility"]
            "Effect"   = "Allow"
            "Resource" = awscc_sqs_queue.reports_jobs.arn
          },
          {
            "Action"   = "s3:GetObject"
            "Effect"   = "Allow"
            "Resource" = "arn:aws:s3:::reports-uploads/*"
          },
          {
            "Action"   = ["s3:PutObject", "s3:PutObjectAcl"]
            "Effect"   = "Allow"
            "Resource" = "arn:aws:s3:::reports-output/*"
          },
          {
            "Action"   = "dynamodb:*"
            "Effect"   = "Allow"
            "Resource" = "*"
          },
        ]
        "Version" = "2012-10-17"
      })
      policy_name = "reports-worker-inline"
    },
  ]
  role_name = "reports-worker-role"
}

resource "awscc_kms_key" "data_key" {
  description         = "Encrypts checkout and reporting data"
  enable_key_rotation = true
  enabled             = true
  key_policy = jsonencode({
    "Id" = "key-default-1"
    "Statement" = [
      {
        "Action" = "kms:*"
        "Effect" = "Allow"
        "Principal" = {
          "AWS" = "arn:aws:iam::111122223333:root"
        }
        "Resource" = "*"
        "Sid"      = "Enable IAM User Permissions"
      },
    ]
    "Version" = "2012-10-17"
  })
  key_spec     = "SYMMETRIC_DEFAULT"
  key_usage    = "ENCRYPT_DECRYPT"
  multi_region = false
}

resource "awscc_lambda_function" "legacy_export_job" {
  architectures = ["arm64"]
  code = {
    s3_bucket = var.legacy_export_job_code_s3_bucket
    s3_key    = var.legacy_export_job_code_s3_key
  }
  environment = {
    variables = {
      "EXPORT_BUCKET" = awscc_s3_bucket.reports_output.bucket_name
    }
  }
  function_name = "legacy-export-job"
  handler       = "index.handler"
  memory_size   = 1024
  package_type  = "Zip"
  role          = "arn:aws:iam::111122223333:role/legacy-export-role"
  runtime       = "python3.9"
  tags          = []
  timeout       = 30
  tracing_config = {
    mode = "Active"
  }

  lifecycle {
    ignore_changes = [code]
  }
}
