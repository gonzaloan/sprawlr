# search: written by sprawlr on 2026-11-27 from the configuration of account 111122223333.

resource "awscc_apigateway_rest_api" "search_public_api" {
  description = "Public product search"
  endpoint_configuration = {
    types = ["REGIONAL"]
  }
  name = "search-public-api"
}

resource "awscc_lambda_function" "search_indexer" {
  architectures = ["arm64"]
  code = {
    s3_bucket = var.search_indexer_code_s3_bucket
    s3_key    = var.search_indexer_code_s3_key
  }
  environment = {
    variables = {
      "INDEX_NAME"          = var.search_indexer_index_name
      "OPENSEARCH_ENDPOINT" = var.search_indexer_opensearch_endpoint
    }
  }
  function_name = "search-indexer"
  handler       = "com.acme.Handler::handleRequest"
  memory_size   = 2048
  package_type  = "Zip"
  role          = "arn:aws:iam::111122223333:role/search-lambda-role"
  runtime       = "java21"
  tags          = []
  timeout       = 30
  tracing_config = {
    mode = "Active"
  }
  vpc_config = {
    security_group_ids = [awscc_ec2_security_group.checkout_api_sg.group_id]
    subnet_ids         = [awscc_ec2_subnet.prod_private_a.subnet_id, awscc_ec2_subnet.prod_private_b.subnet_id]
  }

  lifecycle {
    ignore_changes = [code]
  }
}

resource "awscc_lambda_function" "search_query_handler" {
  architectures = ["arm64"]
  code = {
    s3_bucket = var.search_query_handler_code_s3_bucket
    s3_key    = var.search_query_handler_code_s3_key
  }
  environment = {
    variables = {
      "OPENSEARCH_ENDPOINT" = var.search_query_handler_opensearch_endpoint
    }
  }
  function_name = "search-query-handler"
  handler       = "com.acme.Handler::handleRequest"
  memory_size   = 1024
  package_type  = "Zip"
  role          = "arn:aws:iam::111122223333:role/search-lambda-role"
  runtime       = "java21"
  tags          = []
  timeout       = 30
  tracing_config = {
    mode = "Active"
  }
  vpc_config = {
    security_group_ids = [awscc_ec2_security_group.checkout_api_sg.group_id]
    subnet_ids         = [awscc_ec2_subnet.prod_private_a.subnet_id, awscc_ec2_subnet.prod_private_b.subnet_id]
  }

  lifecycle {
    ignore_changes = [code]
  }
}

resource "awscc_opensearchservice_domain" "search_products" {
  cluster_config = {
    instance_count = 3
    instance_type  = "r6g.large.search"
    zone_awareness_config = {
      availability_zone_count = 2
    }
    zone_awareness_enabled = true
  }
  domain_name = "search-products"
  ebs_options = {
    ebs_enabled = true
    volume_size = 100
    volume_type = "gp3"
  }
  encryption_at_rest_options = {
    enabled    = true
    kms_key_id = "1f2e3d4c-data"
  }
  engine_version = "OpenSearch_2.17"
  node_to_node_encryption_options = {
    enabled = true
  }
  vpc_options = {
    security_group_ids = [awscc_ec2_security_group.checkout_api_sg.group_id]
    subnet_ids         = [awscc_ec2_subnet.prod_private_a.subnet_id, awscc_ec2_subnet.prod_private_b.subnet_id]
  }
}
