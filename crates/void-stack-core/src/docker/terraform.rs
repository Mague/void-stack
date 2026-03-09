//! Terraform HCL parser — extract infrastructure resources from `.tf` files.

use std::path::Path;

use super::{InfraResource, InfraResourceKind};

/// Recursively find all `.tf` files and extract infrastructure resources.
pub fn parse_terraform(project_path: &Path) -> Vec<InfraResource> {
    let mut resources = Vec::new();
    let mut tf_files = Vec::new();
    collect_tf_files(project_path, &mut tf_files, 0, 5);

    for path in &tf_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            parse_tf_content(&content, &mut resources);
        }
    }

    resources
}

fn collect_tf_files(dir: &Path, files: &mut Vec<std::path::PathBuf>, depth: usize, max_depth: usize) {
    if depth >= max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if matches!(name.as_str(), ".git" | "node_modules" | ".terraform" | "target" | "dist" | "build") {
                continue;
            }
            collect_tf_files(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|e| e.to_str()) == Some("tf") {
            files.push(path);
        }
    }
}

/// Parse a single `.tf` file content and extract resource blocks.
fn parse_tf_content(content: &str, resources: &mut Vec<InfraResource>) {
    // Match `resource "type" "name" {` blocks using a simple state machine.
    // HCL is complex; we only extract the top-level resource type/name and
    // try to find the `engine` attribute inside the block.
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Match: resource "aws_db_instance" "my_db" {
        if let Some(rest) = trimmed.strip_prefix("resource ") {
            if let Some((res_type, res_name)) = parse_resource_header(rest) {
                // Collect the block body to extract details
                let block_body = collect_block_body(&lines, i);
                let (kind, provider) = classify_terraform_resource(&res_type);
                let details = extract_tf_details(&res_type, &block_body);

                resources.push(InfraResource {
                    provider,
                    resource_type: res_type,
                    name: res_name,
                    kind,
                    details,
                });
            }
        }

        i += 1;
    }
}

/// Parse `"type" "name" {` from the rest of a resource line.
fn parse_resource_header(rest: &str) -> Option<(String, String)> {
    let rest = rest.trim();
    // Expect: "type" "name" { or "type" "name"{
    let mut parts = Vec::new();
    let mut chars = rest.chars().peekable();

    while parts.len() < 2 {
        // Skip whitespace
        while chars.peek().map(|c| c.is_whitespace()).unwrap_or(false) {
            chars.next();
        }

        if chars.peek() != Some(&'"') {
            break;
        }
        chars.next(); // skip opening "

        let s: String = chars.by_ref().take_while(|c| *c != '"').collect();
        if !s.is_empty() {
            parts.push(s);
        }
    }

    if parts.len() == 2 {
        Some((parts[0].clone(), parts[1].clone()))
    } else {
        None
    }
}

/// Collect text inside a `{ ... }` block starting from a given line.
fn collect_block_body(lines: &[&str], start: usize) -> String {
    let mut depth = 0;
    let mut body = String::new();
    let mut started = false;

    for line in &lines[start..] {
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
                started = true;
            } else if ch == '}' {
                depth -= 1;
                if started && depth == 0 {
                    return body;
                }
            }
        }
        if started {
            body.push_str(line);
            body.push('\n');
        }
        // Limit block scanning to 100 lines
        if body.lines().count() > 100 {
            break;
        }
    }

    body
}

/// Classify a Terraform resource type into kind and provider.
fn classify_terraform_resource(resource_type: &str) -> (InfraResourceKind, String) {
    let rt = resource_type.to_lowercase();

    let provider = if rt.starts_with("aws_") {
        "aws".to_string()
    } else if rt.starts_with("google_") {
        "gcp".to_string()
    } else if rt.starts_with("azurerm_") {
        "azure".to_string()
    } else {
        "terraform".to_string()
    };

    let kind = match rt.as_str() {
        // Databases
        "aws_db_instance" | "aws_rds_cluster" | "aws_dynamodb_table"
        | "google_sql_database_instance" | "google_spanner_instance"
        | "azurerm_postgresql_server" | "azurerm_postgresql_flexible_server"
        | "azurerm_mysql_server" | "azurerm_mysql_flexible_server"
        | "azurerm_cosmosdb_account" | "azurerm_mssql_server"
            => InfraResourceKind::Database,

        // Cache
        "aws_elasticache_cluster" | "aws_elasticache_replication_group"
        | "google_redis_instance"
        | "azurerm_redis_cache"
            => InfraResourceKind::Cache,

        // Storage
        "aws_s3_bucket" | "aws_s3_bucket_v2"
        | "google_storage_bucket"
        | "azurerm_storage_account" | "azurerm_storage_container"
            => InfraResourceKind::Storage,

        // Compute
        "aws_lambda_function" | "aws_ecs_service" | "aws_ecs_task_definition"
        | "aws_instance" | "aws_autoscaling_group"
        | "google_compute_instance" | "google_cloud_run_service"
        | "google_cloudfunctions_function"
        | "azurerm_function_app" | "azurerm_linux_web_app" | "azurerm_container_app"
            => InfraResourceKind::Compute,

        // Queue / Messaging
        "aws_sqs_queue" | "aws_sns_topic" | "aws_kinesis_stream"
        | "google_pubsub_topic"
        | "azurerm_servicebus_queue" | "azurerm_servicebus_topic"
            => InfraResourceKind::Queue,

        // Networking
        "aws_vpc" | "aws_subnet" | "aws_security_group" | "aws_lb" | "aws_alb"
        | "aws_cloudfront_distribution" | "aws_route53_record" | "aws_api_gateway_rest_api"
        | "google_compute_network" | "google_compute_firewall"
        | "azurerm_virtual_network" | "azurerm_network_security_group"
            => InfraResourceKind::Network,

        _ => InfraResourceKind::Other,
    };

    (kind, provider)
}

/// Extract useful details from a Terraform resource block body.
fn extract_tf_details(resource_type: &str, body: &str) -> Vec<String> {
    let mut details = Vec::new();
    let rt = resource_type.to_lowercase();

    // For database resources, try to find the engine
    if rt.contains("db_instance") || rt.contains("rds_cluster") {
        if let Some(engine) = extract_hcl_string_attr(body, "engine") {
            details.push(format!("engine={}", engine));
        }
        if let Some(ver) = extract_hcl_string_attr(body, "engine_version") {
            details.push(format!("version={}", ver));
        }
        if let Some(class) = extract_hcl_string_attr(body, "instance_class") {
            details.push(format!("class={}", class));
        }
    }

    // For ElastiCache, detect engine type
    if rt.contains("elasticache") {
        if let Some(engine) = extract_hcl_string_attr(body, "engine") {
            details.push(format!("engine={}", engine));
        }
        if let Some(node_type) = extract_hcl_string_attr(body, "node_type") {
            details.push(format!("node_type={}", node_type));
        }
    }

    // For Lambda, detect runtime
    if rt.contains("lambda") {
        if let Some(runtime) = extract_hcl_string_attr(body, "runtime") {
            details.push(format!("runtime={}", runtime));
        }
    }

    // For ECS, detect image
    if rt.contains("ecs_task_definition") {
        if let Some(image) = extract_hcl_string_attr(body, "image") {
            details.push(format!("image={}", image));
        }
    }

    // For S3, detect versioning
    if rt.contains("s3_bucket") {
        // Check for bucket name
        if let Some(bucket) = extract_hcl_string_attr(body, "bucket") {
            details.push(format!("bucket={}", bucket));
        }
    }

    details
}

/// Extract a string attribute value from an HCL-like block body.
/// Matches patterns like: `engine = "postgres"` or `engine="postgres"`
fn extract_hcl_string_attr(body: &str, attr_name: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        // Match: attr_name = "value" or attr_name="value"
        if let Some(rest) = trimmed.strip_prefix(attr_name) {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                if let Some(rest) = rest.strip_prefix('"') {
                    let value: String = rest.chars().take_while(|c| *c != '"').collect();
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_terraform_aws_resources() {
        let dir = tempfile::tempdir().unwrap();
        let tf_path = dir.path().join("main.tf");
        std::fs::write(&tf_path, r#"
resource "aws_db_instance" "main_db" {
  engine         = "postgres"
  engine_version = "15.4"
  instance_class = "db.t3.micro"
  allocated_storage = 20
}

resource "aws_elasticache_cluster" "cache" {
  engine    = "redis"
  node_type = "cache.t3.micro"
  num_cache_nodes = 1
}

resource "aws_s3_bucket" "uploads" {
  bucket = "my-app-uploads"
}

resource "aws_lambda_function" "handler" {
  runtime  = "python3.11"
  handler  = "main.handler"
}

resource "aws_sqs_queue" "tasks" {
  name = "task-queue"
}

resource "aws_sns_topic" "alerts" {
  name = "alerts"
}
"#).unwrap();

        let resources = parse_terraform(dir.path());
        assert_eq!(resources.len(), 6);

        // DB
        let db = &resources[0];
        assert_eq!(db.resource_type, "aws_db_instance");
        assert_eq!(db.name, "main_db");
        assert_eq!(db.provider, "aws");
        assert!(matches!(db.kind, InfraResourceKind::Database));
        assert!(db.details.contains(&"engine=postgres".to_string()));
        assert!(db.details.contains(&"version=15.4".to_string()));

        // ElastiCache
        let cache = &resources[1];
        assert_eq!(cache.resource_type, "aws_elasticache_cluster");
        assert!(matches!(cache.kind, InfraResourceKind::Cache));
        assert!(cache.details.contains(&"engine=redis".to_string()));

        // S3
        let s3 = &resources[2];
        assert_eq!(s3.resource_type, "aws_s3_bucket");
        assert!(matches!(s3.kind, InfraResourceKind::Storage));

        // Lambda
        let lambda = &resources[3];
        assert!(matches!(lambda.kind, InfraResourceKind::Compute));
        assert!(lambda.details.contains(&"runtime=python3.11".to_string()));

        // SQS
        let sqs = &resources[4];
        assert!(matches!(sqs.kind, InfraResourceKind::Queue));

        // SNS
        let sns = &resources[5];
        assert!(matches!(sns.kind, InfraResourceKind::Queue));
    }

    #[test]
    fn test_parse_terraform_gcp_azure() {
        let dir = tempfile::tempdir().unwrap();
        let tf_path = dir.path().join("infra.tf");
        std::fs::write(&tf_path, r#"
resource "google_sql_database_instance" "db" {
  database_version = "POSTGRES_15"
  region           = "us-central1"
}

resource "google_redis_instance" "cache" {
  name           = "my-redis"
  memory_size_gb = 1
}

resource "azurerm_postgresql_server" "pg" {
  sku_name = "B_Gen5_1"
}

resource "azurerm_redis_cache" "redis" {
  capacity = 1
  family   = "C"
}
"#).unwrap();

        let resources = parse_terraform(dir.path());
        assert_eq!(resources.len(), 4);

        assert_eq!(resources[0].provider, "gcp");
        assert!(matches!(resources[0].kind, InfraResourceKind::Database));

        assert_eq!(resources[1].provider, "gcp");
        assert!(matches!(resources[1].kind, InfraResourceKind::Cache));

        assert_eq!(resources[2].provider, "azure");
        assert!(matches!(resources[2].kind, InfraResourceKind::Database));

        assert_eq!(resources[3].provider, "azure");
        assert!(matches!(resources[3].kind, InfraResourceKind::Cache));
    }

    #[test]
    fn test_parse_terraform_nested_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let infra_dir = dir.path().join("infra");
        std::fs::create_dir(&infra_dir).unwrap();

        std::fs::write(infra_dir.join("rds.tf"), r#"
resource "aws_db_instance" "prod" {
  engine = "mysql"
}
"#).unwrap();

        std::fs::write(infra_dir.join("cache.tf"), r#"
resource "aws_elasticache_replication_group" "redis" {
  engine = "redis"
}
"#).unwrap();

        let resources = parse_terraform(dir.path());
        assert_eq!(resources.len(), 2);
    }

    #[test]
    fn test_empty_project_no_terraform() {
        let dir = tempfile::tempdir().unwrap();
        let resources = parse_terraform(dir.path());
        assert!(resources.is_empty());
    }
}
