variable "checkout_email_sender_sender_address" {
  description = "Value of the environment variable SENDER_ADDRESS of checkout-email-sender. sprawlr never writes it."
  type        = string
  sensitive   = true
}

variable "checkout_email_sender_code_s3_bucket" {
  description = "CodeS3Bucket of checkout-email-sender: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "checkout_email_sender_code_s3_key" {
  description = "CodeS3Key of checkout-email-sender: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "reports_worker_user_data" {
  description = "User data of reports-worker: it may hold secrets, so sprawlr never writes it."
  type        = string
  sensitive   = true
}

variable "report_generator_db_host" {
  description = "Value of the environment variable DB_HOST of report-generator. sprawlr never writes it."
  type        = string
  sensitive   = true
}

variable "report_generator_reports_api_token" {
  description = "Value of the environment variable REPORTS_API_TOKEN of report-generator. sprawlr never writes it."
  type        = string
  sensitive   = true
}

variable "report_generator_code_s3_bucket" {
  description = "CodeS3Bucket of report-generator: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "report_generator_code_s3_key" {
  description = "CodeS3Key of report-generator: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "search_indexer_index_name" {
  description = "Value of the environment variable INDEX_NAME of search-indexer. sprawlr never writes it."
  type        = string
  sensitive   = true
}

variable "search_indexer_opensearch_endpoint" {
  description = "Value of the environment variable OPENSEARCH_ENDPOINT of search-indexer. sprawlr never writes it."
  type        = string
  sensitive   = true
}

variable "search_indexer_code_s3_bucket" {
  description = "CodeS3Bucket of search-indexer: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "search_indexer_code_s3_key" {
  description = "CodeS3Key of search-indexer: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "search_query_handler_opensearch_endpoint" {
  description = "Value of the environment variable OPENSEARCH_ENDPOINT of search-query-handler. sprawlr never writes it."
  type        = string
  sensitive   = true
}

variable "search_query_handler_code_s3_bucket" {
  description = "CodeS3Bucket of search-query-handler: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "search_query_handler_code_s3_key" {
  description = "CodeS3Key of search-query-handler: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "legacy_export_job_code_s3_bucket" {
  description = "CodeS3Bucket of legacy-export-job: AWS never returns it. Set it to what you deploy."
  type        = string
}

variable "legacy_export_job_code_s3_key" {
  description = "CodeS3Key of legacy-export-job: AWS never returns it. Set it to what you deploy."
  type        = string
}
