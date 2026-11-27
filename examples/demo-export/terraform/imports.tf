# Each resource as it runs today: `terraform plan` imports it and should change nothing.

import {
  to = awscc_ecs_service.checkout_api
  id = "arn:aws:ecs:us-east-1:111122223333:service/dev-cluster/checkout-api|arn:aws:ecs:us-east-1:111122223333:cluster/dev-cluster"
}

import {
  to = awscc_ecs_service.checkout_api_2
  id = "arn:aws:ecs:us-east-1:111122223333:service/prod-cluster/checkout-api|arn:aws:ecs:us-east-1:111122223333:cluster/prod-cluster"
}

import {
  to = awscc_elasticloadbalancingv2_load_balancer.checkout_alb
  id = "arn:aws:elasticloadbalancing:us-east-1:111122223333:loadbalancer/app/checkout-alb/50dc6c495c0c9188"
}

import {
  to = awscc_iam_role.checkout_api_task_role
  id = "checkout-api-task-role"
}

import {
  to = awscc_lambda_function.checkout_email_sender
  id = "checkout-email-sender"
}

import {
  to = awscc_rds_db_cluster.checkout_orders_db
  id = "checkout-orders-db"
}

import {
  to = awscc_rds_db_cluster.checkout_orders_db_dev
  id = "checkout-orders-db-dev"
}

import {
  to = awscc_sns_topic.checkout_notifications
  id = "arn:aws:sns:us-east-1:111122223333:checkout-notifications"
}

import {
  to = awscc_sqs_queue.checkout_order_events
  id = "https://sqs.us-east-1.amazonaws.com/111122223333/checkout-order-events"
}

import {
  to = awscc_secretsmanager_secret.checkout_prod_db_credentials
  id = "arn:aws:secretsmanager:us-east-1:111122223333:secret:checkout/prod/db-credentials-AbC123"
}

import {
  to = awscc_ec2_instance.reports_worker
  id = "i-0reports7a1"
}

import {
  to = awscc_lambda_function.report_generator
  id = "report-generator"
}

import {
  to = awscc_s3_bucket.reports_output
  id = "reports-output"
}

import {
  to = awscc_s3_bucket.reports_uploads
  id = "reports-uploads"
}

import {
  to = awscc_sqs_queue.reports_jobs
  id = "https://sqs.us-east-1.amazonaws.com/111122223333/reports-jobs"
}

import {
  to = awscc_sqs_queue.reports_jobs_dlq
  id = "https://sqs.us-east-1.amazonaws.com/111122223333/reports-jobs-dlq"
}

import {
  to = awscc_apigateway_rest_api.search_public_api
  id = "a1b2c3search"
}

import {
  to = awscc_lambda_function.search_indexer
  id = "search-indexer"
}

import {
  to = awscc_lambda_function.search_query_handler
  id = "search-query-handler"
}

import {
  to = awscc_opensearchservice_domain.search_products
  id = "search-products"
}

import {
  to = awscc_ec2_eip.eip_203_0_113_24
  id = "203.0.113.24|eipalloc-0legacy"
}

import {
  to = awscc_ec2_nat_gateway.nat_legacy_dev
  id = "nat-0legacy"
}

import {
  to = awscc_ec2_security_group.checkout_alb_sg
  id = "sg-alb"
}

import {
  to = awscc_ec2_security_group.checkout_api_sg
  id = "sg-api"
}

import {
  to = awscc_ec2_security_group.migration_2024_sg
  id = "sg-migration"
}

import {
  to = awscc_ec2_security_group.orders_db_sg
  id = "sg-db"
}

import {
  to = awscc_ec2_subnet.dev_private_a
  id = "subnet-dev-a"
}

import {
  to = awscc_ec2_subnet.prod_private_a
  id = "subnet-priv-a"
}

import {
  to = awscc_ec2_subnet.prod_private_b
  id = "subnet-priv-b"
}

import {
  to = awscc_ec2_subnet.prod_public_a
  id = "subnet-pub-a"
}

import {
  to = awscc_ec2_subnet.prod_public_b
  id = "subnet-pub-b"
}

import {
  to = awscc_ec2_vpc.dev_vpc
  id = "vpc-0b2dev"
}

import {
  to = awscc_ec2_vpc.prod_vpc
  id = "vpc-0a1prod"
}

import {
  to = awscc_events_event_bus.platform_events
  id = "platform-events"
}

import {
  to = awscc_iam_role.ci_deploy_2023
  id = "ci-deploy-2023"
}

import {
  to = awscc_iam_role.reports_worker_role
  id = "reports-worker-role"
}

import {
  to = awscc_kms_key.data_key
  id = "1f2e3d4c-data"
}

import {
  to = awscc_lambda_function.legacy_export_job
  id = "legacy-export-job"
}
