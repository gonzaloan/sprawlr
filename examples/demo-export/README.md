# Code for the whole account

Written by sprawlr on 2026-11-27 from the live configuration of account 111122223333: 39 resources, as Terraform and as CloudFormation. Use one of them for each resource, not both.

| Folder | What it holds |
|---|---|
| `docs/` | The documentation of the account, as sprawlr shows it. |
| `terraform/` | A Terraform project with the awscc provider: a file per app and an import block per resource. |
| `cloudformation/` | A template per app, and the resources to import into its stack. |

This folder holds details of the account a snapshot does not: identifiers, network ranges, policy conditions. Keep it where you keep code.

## Before you use it

### Fill in 17 values

sprawlr never writes values that could be secrets. Set them in `terraform/terraform.tfvars` (start from `terraform.tfvars.example`), or pass them as parameters to CloudFormation.

| Resource | Value | Why it is not written |
|---|---|---|
| checkout-email-sender | `SENDER_ADDRESS` | Environment variable values are never written. |
| checkout-email-sender | `CodeS3Bucket` | AWS never returns it: set it to what you deploy. |
| checkout-email-sender | `CodeS3Key` | AWS never returns it: set it to what you deploy. |
| reports-worker | `UserData` | User data may hold secrets. |
| report-generator | `DB_HOST` | Environment variable values are never written. |
| report-generator | `REPORTS_API_TOKEN` | Environment variable values are never written. |
| report-generator | `CodeS3Bucket` | AWS never returns it: set it to what you deploy. |
| report-generator | `CodeS3Key` | AWS never returns it: set it to what you deploy. |
| search-indexer | `INDEX_NAME` | Environment variable values are never written. |
| search-indexer | `OPENSEARCH_ENDPOINT` | Environment variable values are never written. |
| search-indexer | `CodeS3Bucket` | AWS never returns it: set it to what you deploy. |
| search-indexer | `CodeS3Key` | AWS never returns it: set it to what you deploy. |
| search-query-handler | `OPENSEARCH_ENDPOINT` | Environment variable values are never written. |
| search-query-handler | `CodeS3Bucket` | AWS never returns it: set it to what you deploy. |
| search-query-handler | `CodeS3Key` | AWS never returns it: set it to what you deploy. |
| legacy-export-job | `CodeS3Bucket` | AWS never returns it: set it to what you deploy. |
| legacy-export-job | `CodeS3Key` | AWS never returns it: set it to what you deploy. |

### Left out (6)

| Resource | Why |
|---|---|
| InvoicesTable | Managed by the CloudFormation stack BillingStack-prod: that stack's template is its code. Get it with: aws cloudformation get-template --stack-name BillingStack-prod |
| billing-invoice-role | Managed by the CloudFormation stack BillingStack-prod: that stack's template is its code. Get it with: aws cloudformation get-template --stack-name BillingStack-prod |
| InvoiceProcessor | Managed by the CloudFormation stack BillingStack-prod: that stack's template is its code. Get it with: aws cloudformation get-template --stack-name BillingStack-prod |
| PdfRenderer | Managed by the CloudFormation stack BillingStack-prod: that stack's template is its code. Get it with: aws cloudformation get-template --stack-name BillingStack-prod |
| invoicepdfs | Managed by the CloudFormation stack BillingStack-prod: that stack's template is its code. Get it with: aws cloudformation get-template --stack-name BillingStack-prod |
| MonthlyClose | Managed by the CloudFormation stack BillingStack-prod: that stack's template is its code. Get it with: aws cloudformation get-template --stack-name BillingStack-prod |

## Check the Terraform

```bash
cd terraform
cp terraform.tfvars.example terraform.tfvars   # then fill in the values
terraform init
terraform plan
```

The plan should end with `Plan: 38 to import, 0 to add, 0 to change, 0 to destroy.` Anything else is a difference between this code and the account: read it before you apply. Applying a plan that only imports records the resources in Terraform's state and changes nothing in AWS.

Only in CloudFormation: checkout-cdn (AWS::CloudFront::Distribution). The awscc provider has no resource for its type, so Terraform refers to it by its identifier.

Properties AWS never returns, such as a Lambda function's code, are listed under `lifecycle { ignore_changes }`. Terraform does not compare them, so it never deploys code from a guess. Remove the line once the value is yours.

## Check the CloudFormation

```bash
pip install cfn-lint
cfn-lint cloudformation/*.json
```

To bring the resources of a template under a stack without changing them, create an IMPORT change set and look at it before you run it:

```bash
aws cloudformation create-change-set --stack-name checkout --change-set-name sprawlr-import \
  --change-set-type IMPORT --template-body file://cloudformation/checkout.json \
  --resources-to-import file://cloudformation/checkout.import.json \
  --parameters ParameterKey=CheckoutEmailSenderSenderAddress,ParameterValue=... ParameterKey=CheckoutEmailSenderCodeS3Bucket,ParameterValue=... ParameterKey=CheckoutEmailSenderCodeS3Key,ParameterValue=...
aws cloudformation describe-change-set --stack-name checkout --change-set-name sprawlr-import
```

```bash
aws cloudformation create-change-set --stack-name reporting --change-set-name sprawlr-import \
  --change-set-type IMPORT --template-body file://cloudformation/reporting.json \
  --resources-to-import file://cloudformation/reporting.import.json \
  --parameters ParameterKey=ReportsWorkerUserData,ParameterValue=... ParameterKey=ReportGeneratorDbHost,ParameterValue=... ParameterKey=ReportGeneratorReportsApiToken,ParameterValue=... ParameterKey=ReportGeneratorCodeS3Bucket,ParameterValue=... ParameterKey=ReportGeneratorCodeS3Key,ParameterValue=...
aws cloudformation describe-change-set --stack-name reporting --change-set-name sprawlr-import
```

```bash
aws cloudformation create-change-set --stack-name search --change-set-name sprawlr-import \
  --change-set-type IMPORT --template-body file://cloudformation/search.json \
  --resources-to-import file://cloudformation/search.import.json \
  --parameters ParameterKey=SearchIndexerIndexName,ParameterValue=... ParameterKey=SearchIndexerOpensearchEndpoint,ParameterValue=... ParameterKey=SearchIndexerCodeS3Bucket,ParameterValue=... ParameterKey=SearchIndexerCodeS3Key,ParameterValue=... ParameterKey=SearchQueryHandlerOpensearchEndpoint,ParameterValue=... ParameterKey=SearchQueryHandlerCodeS3Bucket,ParameterValue=... ParameterKey=SearchQueryHandlerCodeS3Key,ParameterValue=...
aws cloudformation describe-change-set --stack-name search --change-set-name sprawlr-import
```

```bash
aws cloudformation create-change-set --stack-name shared --change-set-name sprawlr-import \
  --change-set-type IMPORT --template-body file://cloudformation/shared.json \
  --resources-to-import file://cloudformation/shared.import.json \
  --parameters ParameterKey=LegacyExportJobCodeS3Bucket,ParameterValue=... ParameterKey=LegacyExportJobCodeS3Key,ParameterValue=...
aws cloudformation describe-change-set --stack-name shared --change-set-name sprawlr-import
```

Every change should be `Import`. Run it with `aws cloudformation execute-change-set`; then `aws cloudformation detect-stack-drift --stack-name <stack>` compares the template with the account. A template over 51,200 bytes has to go through S3 (`--template-url`). Every resource has `DeletionPolicy: Retain`, so deleting a stack leaves its resources running.

## Resources

### checkout

| Resource | Type | Terraform | CloudFormation |
|---|---|---|---|
| checkout-cdn | AWS::CloudFront::Distribution | not in awscc | `CheckoutCdn` in `checkout.json` |
| checkout-api | AWS::ECS::Service | `awscc_ecs_service.checkout_api` | `CheckoutApi` in `checkout.json` |
| checkout-api | AWS::ECS::Service | `awscc_ecs_service.checkout_api_2` | `CheckoutApi2` in `checkout.json` |
| checkout-alb | AWS::ElasticLoadBalancingV2::LoadBalancer | `awscc_elasticloadbalancingv2_load_balancer.checkout_alb` | `CheckoutAlb` in `checkout.json` |
| checkout-api-task-role | AWS::IAM::Role | `awscc_iam_role.checkout_api_task_role` | `CheckoutApiTaskRole` in `checkout.json` |
| checkout-email-sender | AWS::Lambda::Function | `awscc_lambda_function.checkout_email_sender` | `CheckoutEmailSender` in `checkout.json` |
| checkout-orders-db | AWS::RDS::DBCluster | `awscc_rds_db_cluster.checkout_orders_db` | `CheckoutOrdersDb` in `checkout.json` |
| checkout-orders-db-dev | AWS::RDS::DBCluster | `awscc_rds_db_cluster.checkout_orders_db_dev` | `CheckoutOrdersDbDev` in `checkout.json` |
| checkout-notifications | AWS::SNS::Topic | `awscc_sns_topic.checkout_notifications` | `CheckoutNotifications` in `checkout.json` |
| checkout-order-events | AWS::SQS::Queue | `awscc_sqs_queue.checkout_order_events` | `CheckoutOrderEvents` in `checkout.json` |
| checkout/prod/db-credentials | AWS::SecretsManager::Secret | `awscc_secretsmanager_secret.checkout_prod_db_credentials` | `CheckoutProdDbCredentials` in `checkout.json` |

### reporting

| Resource | Type | Terraform | CloudFormation |
|---|---|---|---|
| reports-worker | AWS::EC2::Instance | `awscc_ec2_instance.reports_worker` | `ReportsWorker` in `reporting.json` |
| report-generator | AWS::Lambda::Function | `awscc_lambda_function.report_generator` | `ReportGenerator` in `reporting.json` |
| reports-output | AWS::S3::Bucket | `awscc_s3_bucket.reports_output` | `ReportsOutput` in `reporting.json` |
| reports-uploads | AWS::S3::Bucket | `awscc_s3_bucket.reports_uploads` | `ReportsUploads` in `reporting.json` |
| reports-jobs | AWS::SQS::Queue | `awscc_sqs_queue.reports_jobs` | `ReportsJobs` in `reporting.json` |
| reports-jobs-dlq | AWS::SQS::Queue | `awscc_sqs_queue.reports_jobs_dlq` | `ReportsJobsDlq` in `reporting.json` |

### search

| Resource | Type | Terraform | CloudFormation |
|---|---|---|---|
| search-public-api | AWS::ApiGateway::RestApi | `awscc_apigateway_rest_api.search_public_api` | `SearchPublicApi` in `search.json` |
| search-indexer | AWS::Lambda::Function | `awscc_lambda_function.search_indexer` | `SearchIndexer` in `search.json` |
| search-query-handler | AWS::Lambda::Function | `awscc_lambda_function.search_query_handler` | `SearchQueryHandler` in `search.json` |
| search-products | AWS::OpenSearchService::Domain | `awscc_opensearchservice_domain.search_products` | `SearchProducts` in `search.json` |

### shared

| Resource | Type | Terraform | CloudFormation |
|---|---|---|---|
| 203.0.113.24 | AWS::EC2::EIP | `awscc_ec2_eip.eip_203_0_113_24` | `EIP203011324` in `shared.json` |
| nat-legacy-dev | AWS::EC2::NatGateway | `awscc_ec2_nat_gateway.nat_legacy_dev` | `NatLegacyDev` in `shared.json` |
| checkout-alb-sg | AWS::EC2::SecurityGroup | `awscc_ec2_security_group.checkout_alb_sg` | `CheckoutAlbSg` in `shared.json` |
| checkout-api-sg | AWS::EC2::SecurityGroup | `awscc_ec2_security_group.checkout_api_sg` | `CheckoutApiSg` in `shared.json` |
| migration-2024-sg | AWS::EC2::SecurityGroup | `awscc_ec2_security_group.migration_2024_sg` | `Migration2024Sg` in `shared.json` |
| orders-db-sg | AWS::EC2::SecurityGroup | `awscc_ec2_security_group.orders_db_sg` | `OrdersDbSg` in `shared.json` |
| dev-private-a | AWS::EC2::Subnet | `awscc_ec2_subnet.dev_private_a` | `DevPrivateA` in `shared.json` |
| prod-private-a | AWS::EC2::Subnet | `awscc_ec2_subnet.prod_private_a` | `ProdPrivateA` in `shared.json` |
| prod-private-b | AWS::EC2::Subnet | `awscc_ec2_subnet.prod_private_b` | `ProdPrivateB` in `shared.json` |
| prod-public-a | AWS::EC2::Subnet | `awscc_ec2_subnet.prod_public_a` | `ProdPublicA` in `shared.json` |
| prod-public-b | AWS::EC2::Subnet | `awscc_ec2_subnet.prod_public_b` | `ProdPublicB` in `shared.json` |
| dev-vpc | AWS::EC2::VPC | `awscc_ec2_vpc.dev_vpc` | `DevVpc` in `shared.json` |
| prod-vpc | AWS::EC2::VPC | `awscc_ec2_vpc.prod_vpc` | `ProdVpc` in `shared.json` |
| platform-events | AWS::Events::EventBus | `awscc_events_event_bus.platform_events` | `PlatformEvents` in `shared.json` |
| ci-deploy-2023 | AWS::IAM::Role | `awscc_iam_role.ci_deploy_2023` | `CiDeploy2023` in `shared.json` |
| reports-worker-role | AWS::IAM::Role | `awscc_iam_role.reports_worker_role` | `ReportsWorkerRole` in `shared.json` |
| data-key | AWS::KMS::Key | `awscc_kms_key.data_key` | `DataKey` in `shared.json` |
| legacy-export-job | AWS::Lambda::Function | `awscc_lambda_function.legacy_export_job` | `LegacyExportJob` in `shared.json` |
