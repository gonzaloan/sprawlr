# reporting: written by sprawlr on 2026-11-27 from the configuration of account 111122223333.

resource "awscc_ec2_instance" "reports_worker" {
  iam_instance_profile = "reports-worker-profile"
  image_id             = "ami-0c1a2b3c4d5e6f708"
  instance_type        = "m6i.large"
  security_group_ids   = [awscc_ec2_security_group.checkout_api_sg.group_id]
  subnet_id            = awscc_ec2_subnet.prod_private_a.subnet_id
  tags = [
    {
      key   = "app"
      value = "reporting"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  user_data = var.reports_worker_user_data
}

resource "awscc_lambda_function" "report_generator" {
  architectures = ["arm64"]
  code = {
    s3_bucket = var.report_generator_code_s3_bucket
    s3_key    = var.report_generator_code_s3_key
  }
  environment = {
    variables = {
      "DB_HOST"           = var.report_generator_db_host
      "REPORTS_API_TOKEN" = var.report_generator_reports_api_token
    }
  }
  function_name = "report-generator"
  handler       = "index.handler"
  memory_size   = 128
  package_type  = "Zip"
  role          = "arn:aws:iam::111122223333:role/report-generator-role"
  runtime       = "python3.12"
  tags = [
    {
      key   = "app"
      value = "reporting"
    },
    {
      key   = "env"
      value = "dev"
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

resource "awscc_s3_bucket" "reports_output" {
  bucket_encryption = {
    server_side_encryption_configuration = [
      {
        bucket_key_enabled = false
        server_side_encryption_by_default = {
          sse_algorithm = "AES256"
        }
      },
    ]
  }
  bucket_name = "reports-output"
  public_access_block_configuration = {
    block_public_acls       = true
    block_public_policy     = true
    ignore_public_acls      = true
    restrict_public_buckets = true
  }
  tags = [
    {
      key   = "app"
      value = "reporting"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  versioning_configuration = {
    status = "Enabled"
  }
}

resource "awscc_s3_bucket" "reports_uploads" {
  bucket_encryption = {
    server_side_encryption_configuration = [
      {
        bucket_key_enabled = false
        server_side_encryption_by_default = {
          sse_algorithm = "AES256"
        }
      },
    ]
  }
  bucket_name = "reports-uploads"
  notification_configuration = {
    queue_configurations = [
      {
        event = "s3:ObjectCreated:*"
        queue = awscc_sqs_queue.reports_jobs.arn
      },
    ]
  }
  public_access_block_configuration = {
    block_public_acls       = true
    block_public_policy     = true
    ignore_public_acls      = true
    restrict_public_buckets = true
  }
  tags = [
    {
      key   = "app"
      value = "reporting"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
}

resource "awscc_sqs_queue" "reports_jobs" {
  message_retention_period = 345600
  queue_name               = "reports-jobs"
  redrive_policy = jsonencode({
    "deadLetterTargetArn" = awscc_sqs_queue.reports_jobs_dlq.arn
    "maxReceiveCount"     = 5
  })
  sqs_managed_sse_enabled = true
  tags = [
    {
      key   = "app"
      value = "reporting"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  visibility_timeout = 900
}

resource "awscc_sqs_queue" "reports_jobs_dlq" {
  message_retention_period = 345600
  queue_name               = "reports-jobs-dlq"
  sqs_managed_sse_enabled  = true
  tags = [
    {
      key   = "app"
      value = "reporting"
    },
    {
      key   = "env"
      value = "prod"
    },
  ]
  visibility_timeout = 30
}
