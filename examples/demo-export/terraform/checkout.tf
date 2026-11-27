# checkout: written by sprawlr on 2026-11-27 from the configuration of account 111122223333.

resource "awscc_ecs_service" "checkout_api" {
  cluster       = "arn:aws:ecs:us-east-1:111122223333:cluster/dev-cluster"
  desired_count = 1
  launch_type   = "FARGATE"
  network_configuration = {
    awsvpc_configuration = {
      assign_public_ip = "DISABLED"
      security_groups  = []
      subnets          = [awscc_ec2_subnet.dev_private_a.subnet_id]
    }
  }
  propagate_tags = "SERVICE"
  service_name   = "checkout-api"
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "dev"
    },
  ]
  task_definition = "arn:aws:ecs:us-east-1:111122223333:task-definition/checkout-api:47"
}

resource "awscc_ecs_service" "checkout_api_2" {
  cluster       = "arn:aws:ecs:us-east-1:111122223333:cluster/prod-cluster"
  desired_count = 4
  launch_type   = "FARGATE"
  network_configuration = {
    awsvpc_configuration = {
      assign_public_ip = "DISABLED"
      security_groups  = [awscc_ec2_security_group.checkout_api_sg.group_id]
      subnets          = [awscc_ec2_subnet.prod_private_a.subnet_id, awscc_ec2_subnet.prod_private_b.subnet_id]
    }
  }
  propagate_tags = "SERVICE"
  service_name   = "checkout-api"
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  task_definition = "arn:aws:ecs:us-east-1:111122223333:task-definition/checkout-api:42"
}

resource "awscc_elasticloadbalancingv2_load_balancer" "checkout_alb" {
  ip_address_type = "ipv4"
  load_balancer_attributes = [
    {
      key   = "idle_timeout.timeout_seconds"
      value = "60"
    },
  ]
  name            = "checkout-alb"
  scheme          = "internet-facing"
  security_groups = [awscc_ec2_security_group.checkout_alb_sg.group_id]
  subnets         = [awscc_ec2_subnet.prod_public_a.subnet_id, awscc_ec2_subnet.prod_public_b.subnet_id]
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  type = "application"
}

resource "awscc_iam_role" "checkout_api_task_role" {
  assume_role_policy_document = jsonencode({
    "Statement" = [
      {
        "Action" = "sts:AssumeRole"
        "Condition" = {
          "StringEquals" = {
            "aws:SourceAccount" = "111122223333"
          }
        }
        "Effect" = "Allow"
        "Principal" = {
          "Service" = "ecs-tasks.amazonaws.com"
        }
      },
    ]
    "Version" = "2012-10-17"
  })
  description          = "What checkout-api's tasks may do"
  managed_policy_arns  = []
  max_session_duration = 3600
  path                 = "/"
  policies = [
    {
      policy_document = jsonencode({
        "Statement" = [
          {
            "Action"   = "sqs:SendMessage"
            "Effect"   = "Allow"
            "Resource" = awscc_sqs_queue.checkout_order_events.arn
          },
          {
            "Action"   = "sns:Publish"
            "Effect"   = "Allow"
            "Resource" = awscc_sns_topic.checkout_notifications.topic_arn
          },
          {
            "Action"   = "events:PutEvents"
            "Effect"   = "Allow"
            "Resource" = awscc_events_event_bus.platform_events.arn
          },
          {
            "Action"   = "secretsmanager:GetSecretValue"
            "Effect"   = "Allow"
            "Resource" = awscc_secretsmanager_secret.checkout_prod_db_credentials.id
          },
        ]
        "Version" = "2012-10-17"
      })
      policy_name = "checkout-api-task-policy"
    },
  ]
  role_name = "checkout-api-task-role"
}

resource "awscc_lambda_function" "checkout_email_sender" {
  architectures = ["arm64"]
  code = {
    s3_bucket = var.checkout_email_sender_code_s3_bucket
    s3_key    = var.checkout_email_sender_code_s3_key
  }
  environment = {
    variables = {
      "NOTIFICATIONS_TOPIC_ARN" = awscc_sns_topic.checkout_notifications.topic_arn
      "SENDER_ADDRESS"          = var.checkout_email_sender_sender_address
    }
  }
  function_name = "checkout-email-sender"
  handler       = "index.handler"
  memory_size   = 256
  package_type  = "Zip"
  role          = "arn:aws:iam::111122223333:role/checkout-email-sender-role"
  runtime       = "nodejs22.x"
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  timeout = 30
  tracing_config = {
    mode = "Active"
  }

  lifecycle {
    ignore_changes = [code]
  }
}

resource "awscc_rds_db_cluster" "checkout_orders_db" {
  backup_retention_period     = 7
  db_cluster_identifier       = "checkout-orders-db"
  db_subnet_group_name        = "orders-db-subnets"
  database_name               = "orders"
  deletion_protection         = true
  engine                      = "aurora-postgresql"
  engine_version              = "16.4"
  kms_key_id                  = awscc_kms_key.data_key.arn
  manage_master_user_password = true
  master_username             = "orders_admin"
  port                        = 5432
  storage_encrypted           = true
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  vpc_security_group_ids = ["sg-db"]
}

resource "awscc_rds_db_cluster" "checkout_orders_db_dev" {
  backup_retention_period     = 1
  db_cluster_identifier       = "checkout-orders-db-dev"
  db_subnet_group_name        = "orders-db-dev-subnets"
  database_name               = "orders"
  deletion_protection         = false
  engine                      = "aurora-postgresql"
  engine_version              = "16.4"
  kms_key_id                  = awscc_kms_key.data_key.arn
  manage_master_user_password = true
  master_username             = "orders_admin"
  port                        = 5432
  storage_encrypted           = true
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "dev"
    },
  ]
  vpc_security_group_ids = []
}

resource "awscc_sns_topic" "checkout_notifications" {
  display_name      = "Checkout notifications"
  kms_master_key_id = "alias/aws/sns"
  subscription = [
    {
      endpoint = "arn:aws:lambda:us-east-1:111122223333:function:checkout-email-sender"
      protocol = "lambda"
    },
  ]
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  topic_name = "checkout-notifications"
}

resource "awscc_sqs_queue" "checkout_order_events" {
  message_retention_period = 345600
  queue_name               = "checkout-order-events"
  sqs_managed_sse_enabled  = true
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  visibility_timeout = 30
}

resource "awscc_secretsmanager_secret" "checkout_prod_db_credentials" {
  description = "Credentials checkout-api uses for the orders database"
  kms_key_id  = awscc_kms_key.data_key.arn
  name        = "checkout/prod/db-credentials"
  tags = [
    {
      key   = "app"
      value = "checkout"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
}
