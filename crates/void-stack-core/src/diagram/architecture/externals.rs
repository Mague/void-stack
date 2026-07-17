//! External service detection from env files, docker-compose, and source code.

use std::path::Path;

use crate::runner::local::strip_win_prefix;
use crate::security;

use super::service_detection;

pub(super) fn detect_external_services(
    root: &Path,
    project: &crate::model::Project,
) -> Vec<String> {
    let mut externals = Vec::new();

    let mut dirs_to_scan: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    for svc in &project.services {
        if let Some(dir) = &svc.working_dir {
            dirs_to_scan.push(Path::new(&strip_win_prefix(dir)).to_path_buf());
        }
    }

    // Build a map of port → service name for localhost cross-referencing
    let mut port_to_service: std::collections::HashMap<u16, String> =
        std::collections::HashMap::new();
    for svc in &project.services {
        let svc_dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let svc_dir_clean = strip_win_prefix(svc_dir);
        let svc_path = Path::new(&svc_dir_clean);
        let (_, port) = service_detection::detect_service_info(svc_path, &svc.command);
        if let Some(p) = port {
            port_to_service.insert(p, svc.name.clone());
        }
    }

    for dir in &dirs_to_scan {
        detect_from_env(dir, &mut externals, &port_to_service);
        detect_from_compose(dir, &mut externals);
        detect_from_source_code(dir, &mut externals, &port_to_service);
    }

    externals
}

fn add_unique(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|x| x == item) {
        list.push(item.to_string());
    }
}

/// Split an env key into uppercase segments on `_`, `-`, `.` and digits-vs-
/// letters boundaries are NOT considered — "AWS_S3_BUCKET" → ["AWS","S3","BUCKET"].
fn key_segments(key: &str) -> Vec<String> {
    key.to_uppercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// True when any key has a segment exactly equal to one of `exact`, or a
/// segment starting with one of `prefixes`. Segment matching avoids the
/// false positives of substring matching on the joined key string
/// (e.g. "S3" matching "K8S3_NODE", "SMS" matching "SMSC_LEGACY_ID").
fn keys_match(keys: &[String], exact: &[&str], prefixes: &[&str]) -> bool {
    keys.iter().any(|key| {
        key_segments(key).iter().any(|seg| {
            exact.iter().any(|e| seg == e) || prefixes.iter().any(|p| seg.starts_with(p))
        })
    })
}

fn detect_from_env(
    dir: &Path,
    externals: &mut Vec<String>,
    port_map: &std::collections::HashMap<u16, String>,
) {
    for env_file in &[".env", ".env.example", ".env.local"] {
        let keys = security::read_env_keys(&dir.join(env_file));

        parse_env_localhost_urls(&dir.join(env_file), externals, port_map);

        // Databases
        if keys_match(&keys, &["PG"], &["POSTGRES"])
            || keys.iter().any(|k| k.eq_ignore_ascii_case("DATABASE_URL"))
        {
            add_unique(externals, "PostgreSQL");
        }
        if keys_match(&keys, &[], &["MYSQL", "MARIADB"]) {
            add_unique(externals, "MySQL");
        }
        if keys_match(&keys, &[], &["REDIS"]) {
            add_unique(externals, "Redis");
        }
        if keys_match(&keys, &[], &["MONGO"]) {
            add_unique(externals, "MongoDB");
        }
        if keys_match(&keys, &[], &["ELASTIC", "OPENSEARCH"]) {
            add_unique(externals, "Elasticsearch");
        }

        // AI / ML services
        if keys_match(&keys, &[], &["OLLAMA"]) {
            add_unique(externals, "Ollama");
        }
        if keys_match(&keys, &[], &["OPENAI"]) {
            add_unique(externals, "OpenAI");
        }
        if keys_match(&keys, &[], &["ANTHROPIC"]) {
            add_unique(externals, "Anthropic");
        }

        // Cloud / Storage
        if keys_match(&keys, &["S3", "AWS"], &[]) {
            add_unique(externals, "AWS S3");
        }
        if keys_match(&keys, &[], &["AZURE"]) {
            add_unique(externals, "Azure");
        }
        if keys_match(&keys, &["GCP"], &[])
            || keys
                .iter()
                .any(|k| k.to_uppercase().starts_with("GOOGLE_CLOUD"))
        {
            add_unique(externals, "GCP");
        }
        if keys_match(&keys, &[], &["CLOUDINARY"]) {
            add_unique(externals, "Cloudinary");
        }

        // Messaging / Queues
        if keys_match(&keys, &["AMQP"], &["RABBITMQ"]) {
            add_unique(externals, "RabbitMQ");
        }
        if keys_match(&keys, &[], &["KAFKA"]) {
            add_unique(externals, "Kafka");
        }

        // Email / Notifications
        if keys_match(&keys, &[], &["SMTP", "SENDGRID", "MAILGUN"]) {
            add_unique(externals, "Email Service");
        }
        if keys_match(&keys, &["SMS"], &["TWILIO"]) {
            add_unique(externals, "SMS Service");
        }
        if keys_match(&keys, &[], &["FIREBASE"]) {
            add_unique(externals, "Firebase");
        }
        if keys_match(&keys, &[], &["STRIPE"]) {
            add_unique(externals, "Stripe");
        }
        if keys_match(&keys, &[], &["SENTRY"]) {
            add_unique(externals, "Sentry");
        }

        // Internal API references
        let internal_patterns = [
            "_API_URL",
            "_CORE_URL",
            "_SERVICE_URL",
            "_REMOTE_URL",
            "_INTERNAL",
        ];
        for key in &keys {
            let upper = key.to_uppercase();
            if internal_patterns.iter().any(|p| upper.contains(p)) {
                add_unique(externals, "Internal APIs");
            }
        }
    }
}

fn parse_env_localhost_urls(
    path: &Path,
    externals: &mut Vec<String>,
    port_map: &std::collections::HashMap<u16, String>,
) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((_key, value)) = trimmed.split_once('=') {
            let val = value.trim().trim_matches('"').trim_matches('\'');
            let val_lower = val.to_lowercase();

            if !(val_lower.starts_with("http://localhost")
                || val_lower.starts_with("http://127.0.0.1")
                || val_lower.starts_with("http://0.0.0.0"))
            {
                continue;
            }

            let after_scheme = val_lower.strip_prefix("http://").unwrap_or(&val_lower);
            let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
            let port: Option<u16> = host_port.split(':').nth(1).and_then(|p| p.parse().ok());

            if let Some(p) = port {
                let key_clean = _key.trim().to_uppercase();
                if let Some(svc_name) = port_map.get(&p) {
                    add_unique(externals, &format!("→ {} ({})", svc_name, key_clean));
                } else {
                    let label = key_clean
                        .replace("_URL", "")
                        .replace("_URI", "")
                        .replace("_BASE", "")
                        .replace("_HOST", "")
                        .replace("_ENDPOINT", "")
                        .replace("_API", "")
                        .replace('_', " ")
                        .trim()
                        .to_string();
                    if !label.is_empty() && label.len() > 2 {
                        add_unique(externals, &format!("Internal: {} :{}", label, p));
                    } else {
                        add_unique(externals, &format!("Internal :{}", p));
                    }
                }
            }
        }
    }
}

fn detect_from_compose(dir: &Path, externals: &mut Vec<String>) {
    for compose in &["docker-compose.yml", "docker-compose.yaml", "compose.yml"] {
        if let Ok(content) = std::fs::read_to_string(dir.join(compose)) {
            let lower = content.to_lowercase();
            if lower.contains("postgres") {
                add_unique(externals, "PostgreSQL");
            }
            if lower.contains("mysql") || lower.contains("mariadb") {
                add_unique(externals, "MySQL");
            }
            if lower.contains("redis") {
                add_unique(externals, "Redis");
            }
            if lower.contains("mongo") {
                add_unique(externals, "MongoDB");
            }
            if lower.contains("rabbitmq") {
                add_unique(externals, "RabbitMQ");
            }
            if lower.contains("kafka") {
                add_unique(externals, "Kafka");
            }
            if lower.contains("elasticsearch") || lower.contains("opensearch") {
                add_unique(externals, "Elasticsearch");
            }
            if lower.contains("minio") {
                add_unique(externals, "MinIO/S3");
            }
            if lower.contains("nginx") {
                add_unique(externals, "Nginx");
            }
        }
    }
}

fn detect_from_source_code(
    dir: &Path,
    externals: &mut Vec<String>,
    port_map: &std::collections::HashMap<u16, String>,
) {
    let code_exts = [
        "ts", "js", "py", "go", "rs", "dart", "java", "kt", "rb", "php", "verse",
    ];

    let mut files_to_scan: Vec<std::path::PathBuf> = Vec::new();
    collect_source_files(dir, &code_exts, &mut files_to_scan, 0, 3);
    for base in &["src", "app", "lib", "internal", "pkg"] {
        let sub = dir.join(base);
        if sub.is_dir() {
            collect_source_files(&sub, &code_exts, &mut files_to_scan, 0, 3);
        }
    }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for path in &files_to_scan {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for url in extract_urls_from_source(&content) {
            if let Some(label) = classify_url(&url, port_map)
                && seen.insert(label.clone())
            {
                add_unique(externals, &label);
            }
        }

        extract_env_url_refs(&content, externals);
    }
}

fn collect_source_files(
    dir: &Path,
    exts: &[&str],
    files: &mut Vec<std::path::PathBuf>,
    depth: usize,
    max_depth: usize,
) {
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
            if matches!(
                name.as_str(),
                "node_modules"
                    | ".git"
                    | "dist"
                    | "build"
                    | "target"
                    | "__pycache__"
                    | ".venv"
                    | "vendor"
            ) {
                continue;
            }
            collect_source_files(&path, exts, files, depth + 1, max_depth);
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if exts.contains(&ext) {
                files.push(path);
            }
        }
    }
}

fn extract_urls_from_source(content: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for quote in &['"', '\'', '`'] {
        let prefix_http = format!("{}http://", quote);
        let prefix_https = format!("{}https://", quote);
        for prefix in &[&prefix_http, &prefix_https] {
            let mut start = 0;
            while let Some(pos) = content[start..].find(prefix.as_str()) {
                let abs_pos = start + pos + 1;
                let rest = &content[abs_pos..];
                let end = rest.find([*quote, ' ', '\n', '\r']).unwrap_or(rest.len());
                let url = &rest[..end];
                if url.len() > 10 {
                    urls.push(url.to_string());
                }
                start = abs_pos + end;
            }
        }
    }
    urls
}

fn classify_url(url: &str, port_map: &std::collections::HashMap<u16, String>) -> Option<String> {
    let lower = url.to_lowercase();

    if lower.contains("${")
        || lower.contains("{{")
        || lower.contains("example.com")
        || lower.contains("placeholder")
    {
        return None;
    }

    let after_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = host_port.split(':').next().unwrap_or(host_port);

    let is_local = host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0";

    if is_local {
        let port: Option<u16> = host_port.split(':').nth(1).and_then(|p| p.parse().ok());
        if let Some(p) = port {
            if let Some(svc_name) = port_map.get(&p) {
                let path = after_scheme
                    .find('/')
                    .map(|i| &after_scheme[i..])
                    .unwrap_or("");
                let path_hint = if path.len() > 1 {
                    let segments: Vec<&str> =
                        path.split('/').filter(|s| !s.is_empty()).take(2).collect();
                    if !segments.is_empty() {
                        format!(" (/{}/…)", segments.join("/"))
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                return Some(format!("→ {}{}", svc_name, path_hint));
            }
            return Some(format!("Internal :{}", p));
        }
        return None;
    }

    let domain = host;
    if domain.contains('.') && !domain.ends_with(".local") && !domain.ends_with(".internal") {
        let parts: Vec<&str> = domain.split('.').collect();
        if parts.len() >= 2 {
            let name = parts[parts.len() - 2..].join(".");
            return Some(format!("API: {}", name));
        }
    }

    None
}

pub(super) fn extract_env_url_refs(content: &str, externals: &mut Vec<String>) {
    let env_patterns = [
        "process.env.",
        "os.environ",
        "os.getenv",
        "env::var",
        "System.getenv",
        "Environment.GetEnvironmentVariable",
        "viper.Get",
    ];

    for line in content.lines() {
        let trimmed = line.trim();
        for pat in &env_patterns {
            if let Some(pos) = trimmed.find(pat) {
                let rest = &trimmed[pos + pat.len()..];
                let var_name: String = rest
                    .trim_start_matches(&['[', '(', '"', '\'', '`'][..])
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let upper = var_name.to_uppercase();
                if (upper.contains("_URL")
                    || upper.contains("_URI")
                    || upper.contains("_ENDPOINT")
                    || upper.contains("_HOST")
                    || upper.contains("_API"))
                    && !upper.contains("DATABASE")
                    && !upper.contains("REDIS")
                    && !upper.contains("MONGO")
                {
                    let service_name = var_name
                        .replace("_URL", "")
                        .replace("_URI", "")
                        .replace("_ENDPOINT", "")
                        .replace("_HOST", "")
                        .replace("_API", "")
                        .replace("_BASE", "")
                        .replace('_', " ")
                        .trim()
                        .to_string();
                    if !service_name.is_empty() && service_name.len() > 2 {
                        add_unique(externals, &format!("API: {}", service_name));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_add_unique() {
        let mut list = Vec::new();
        add_unique(&mut list, "Redis");
        add_unique(&mut list, "Redis");
        add_unique(&mut list, "PostgreSQL");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_detect_from_env_postgres() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "DATABASE_URL=postgres://localhost/db\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "PostgreSQL"));
    }

    #[test]
    fn test_detect_from_env_redis() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "REDIS_URL=redis://localhost:6379\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "Redis"));
    }

    #[test]
    fn test_detect_from_env_segment_matching_no_false_positives() {
        // Regression: substring matching on the joined key string flagged
        // "AWS S3" for K8S3_NODE_NAME and "SMS Service" for SMSC_LEGACY_ID.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "K8S3_NODE_NAME=node1\nSMSC_LEGACY_ID=42\nMY_AWESOME_FLAG=1\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(
            !externals.iter().any(|e| e == "AWS S3"),
            "K8S3_NODE_NAME must not detect AWS S3: {:?}",
            externals
        );
        assert!(
            !externals.iter().any(|e| e == "SMS Service"),
            "SMSC_LEGACY_ID must not detect SMS Service: {:?}",
            externals
        );
    }

    #[test]
    fn test_detect_from_env_segment_matching_true_positives() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "AWS_S3_BUCKET=my-bucket\nSMS_PROVIDER_KEY=x\nTWILIO_SID=y\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "AWS S3"), "{:?}", externals);
        assert!(
            externals.iter().any(|e| e == "SMS Service"),
            "{:?}",
            externals
        );
    }

    #[test]
    fn test_detect_from_env_multiple_services() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            r#"
MONGO_URI=mongodb://localhost/app
AWS_S3_BUCKET=my-bucket
STRIPE_SECRET_KEY=sk_test_xxx
SENTRY_DSN=https://xxx@sentry.io/123
SMTP_HOST=smtp.gmail.com
"#,
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "MongoDB"));
        assert!(externals.iter().any(|e| e == "AWS S3"));
        assert!(externals.iter().any(|e| e == "Stripe"));
        assert!(externals.iter().any(|e| e == "Sentry"));
        assert!(externals.iter().any(|e| e == "Email Service"));
    }

    #[test]
    fn test_detect_from_env_ai_services() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "OLLAMA_HOST=localhost\nOPENAI_API_KEY=sk-xxx\nANTHROPIC_KEY=xxx\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "Ollama"));
        assert!(externals.iter().any(|e| e == "OpenAI"));
        assert!(externals.iter().any(|e| e == "Anthropic"));
    }

    #[test]
    fn test_detect_from_env_internal_api() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "AUTH_API_URL=http://localhost:4000\nCORE_SERVICE_URL=http://localhost:5000\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "Internal APIs"));
    }

    #[test]
    fn test_detect_from_compose() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("docker-compose.yml"),
            r#"
services:
  postgres:
    image: postgres:16
  redis:
    image: redis:7
  rabbitmq:
    image: rabbitmq:3-management
"#,
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_compose(dir.path(), &mut externals);
        assert!(externals.iter().any(|e| e == "PostgreSQL"));
        assert!(externals.iter().any(|e| e == "Redis"));
        assert!(externals.iter().any(|e| e == "RabbitMQ"));
    }

    #[test]
    fn test_detect_from_compose_more_services() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("docker-compose.yml"),
            r#"
services:
  mongo:
    image: mongo:7
  kafka:
    image: confluentinc/cp-kafka
  elasticsearch:
    image: elasticsearch:8
  minio:
    image: minio/minio
  nginx:
    image: nginx:alpine
"#,
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_compose(dir.path(), &mut externals);
        assert!(externals.iter().any(|e| e == "MongoDB"));
        assert!(externals.iter().any(|e| e == "Kafka"));
        assert!(externals.iter().any(|e| e == "Elasticsearch"));
        assert!(externals.iter().any(|e| e.contains("MinIO")));
        assert!(externals.iter().any(|e| e == "Nginx"));
    }

    #[test]
    fn test_classify_url_external() {
        let port_map = HashMap::new();
        let result = classify_url("https://api.stripe.com/v1/charges", &port_map);
        assert_eq!(result, Some("API: stripe.com".to_string()));
    }

    #[test]
    fn test_classify_url_localhost() {
        let port_map = HashMap::new();
        let result = classify_url("http://localhost:4000/api", &port_map);
        assert_eq!(result, Some("Internal :4000".to_string()));
    }

    #[test]
    fn test_classify_url_localhost_with_known_service() {
        let mut port_map = HashMap::new();
        port_map.insert(4000, "auth-api".to_string());
        let result = classify_url("http://localhost:4000/api/login", &port_map);
        assert!(result.is_some());
        assert!(result.unwrap().contains("auth-api"));
    }

    #[test]
    fn test_classify_url_placeholder_ignored() {
        let port_map = HashMap::new();
        assert!(classify_url("https://${API_HOST}/v1", &port_map).is_none());
        assert!(classify_url("https://example.com/test", &port_map).is_none());
        assert!(classify_url("https://placeholder.test/api", &port_map).is_none());
    }

    #[test]
    fn test_extract_urls_from_source() {
        let content = r#"
const API_URL = "https://api.stripe.com/v1/charges";
const DB = 'http://localhost:5432/mydb';
"#;
        let urls = extract_urls_from_source(content);
        assert!(urls.len() >= 2);
        assert!(urls.iter().any(|u| u.contains("stripe.com")));
        assert!(urls.iter().any(|u| u.contains("localhost:5432")));
    }

    #[test]
    fn test_extract_env_url_refs() {
        let content = r#"
const host = process.env.PAYMENT_SERVICE_URL;
const api = process.env.AUTH_API_HOST;
"#;
        let mut externals = Vec::new();
        extract_env_url_refs(content, &mut externals);
        assert!(!externals.is_empty());
    }

    #[test]
    fn test_parse_env_localhost_urls() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            r#"
API_URL=http://localhost:3000/api
FRONTEND_URL=http://127.0.0.1:8080
"#,
        )
        .unwrap();
        let mut externals = Vec::new();
        parse_env_localhost_urls(&dir.path().join(".env"), &mut externals, &HashMap::new());
        assert!(externals.len() >= 2);
    }

    #[test]
    fn test_parse_env_localhost_with_port_map() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "API_URL=http://localhost:4000\n").unwrap();
        let mut port_map = HashMap::new();
        port_map.insert(4000, "backend".to_string());
        let mut externals = Vec::new();
        parse_env_localhost_urls(&dir.path().join(".env"), &mut externals, &port_map);
        assert!(externals.iter().any(|e| e.contains("backend")));
    }

    #[test]
    fn test_detect_from_env_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.is_empty());
    }

    #[test]
    fn test_key_segments_split_on_separators() {
        // Splits on any non-alphanumeric character and uppercases.
        let segs = key_segments("aws_s3-bucket.name");
        assert_eq!(segs, vec!["AWS", "S3", "BUCKET", "NAME"]);
        assert!(key_segments("").is_empty());
        assert!(key_segments("___").is_empty());
    }

    #[test]
    fn test_keys_match_exact_and_prefix() {
        let keys = vec!["AWS_S3_BUCKET".to_string(), "POSTGRESQL_HOST".to_string()];
        // Exact segment match
        assert!(keys_match(&keys, &["S3"], &[]));
        // Prefix segment match ("POSTGRESQL" starts with "POSTGRES")
        assert!(keys_match(&keys, &[], &["POSTGRES"]));
        // Neither exact nor prefix matches
        assert!(!keys_match(&keys, &["REDIS"], &["MONGO"]));
        // Empty key list never matches
        assert!(!keys_match(&[], &["S3"], &["POSTGRES"]));
    }

    #[test]
    fn test_detect_from_env_mysql_elastic_and_pg_segment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "MYSQL_HOST=db\nOPENSEARCH_ENDPOINT=search:9200\nPG_HOST=pg\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "MySQL"), "{:?}", externals);
        assert!(
            externals.iter().any(|e| e == "Elasticsearch"),
            "{:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e == "PostgreSQL"),
            "PG segment must map to PostgreSQL: {:?}",
            externals
        );
    }

    #[test]
    fn test_detect_from_env_cloud_providers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "AZURE_STORAGE_KEY=x\nGOOGLE_CLOUD_PROJECT=y\nCLOUDINARY_URL=cloudinary://x\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "Azure"), "{:?}", externals);
        assert!(
            externals.iter().any(|e| e == "GCP"),
            "GOOGLE_CLOUD prefix must map to GCP: {:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e == "Cloudinary"),
            "{:?}",
            externals
        );
    }

    #[test]
    fn test_detect_from_env_gcp_exact_segment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "GCP_REGION=us-central1\n").unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "GCP"), "{:?}", externals);
    }

    #[test]
    fn test_detect_from_env_messaging_and_firebase() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "AMQP_URL=amqp://mq\nKAFKA_BROKERS=k:9092\nFIREBASE_PROJECT_ID=f\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(
            externals.iter().any(|e| e == "RabbitMQ"),
            "AMQP must map to RabbitMQ: {:?}",
            externals
        );
        assert!(externals.iter().any(|e| e == "Kafka"), "{:?}", externals);
        assert!(externals.iter().any(|e| e == "Firebase"), "{:?}", externals);
    }

    #[test]
    fn test_detect_from_env_reads_example_and_local_variants() {
        // Detection must also read .env.example and .env.local, not just .env.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env.example"), "KAFKA_BROKER=k:9092\n").unwrap();
        std::fs::write(dir.path().join(".env.local"), "STRIPE_SECRET_KEY=sk\n").unwrap();
        let mut externals = Vec::new();
        detect_from_env(dir.path(), &mut externals, &HashMap::new());
        assert!(externals.iter().any(|e| e == "Kafka"), "{:?}", externals);
        assert!(externals.iter().any(|e| e == "Stripe"), "{:?}", externals);
    }

    #[test]
    fn test_parse_env_localhost_skips_comments_and_external_urls() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            r#"
# API_URL=http://localhost:3000

EXTERNAL_URL=https://api.foo.com/v1
NOPORT_URL=http://localhost/health
NOT_A_URL=hello
"#,
        )
        .unwrap();
        let mut externals = Vec::new();
        parse_env_localhost_urls(&dir.path().join(".env"), &mut externals, &HashMap::new());
        assert!(
            externals.is_empty(),
            "comments, non-localhost and portless values must be skipped: {:?}",
            externals
        );
    }

    #[test]
    fn test_parse_env_localhost_short_label_falls_back_to_port_only() {
        // "UI_URL" reduces to "UI" (len 2) which is too short for a label.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "UI_URL=http://localhost:9999\n").unwrap();
        let mut externals = Vec::new();
        parse_env_localhost_urls(&dir.path().join(".env"), &mut externals, &HashMap::new());
        assert_eq!(externals, vec!["Internal :9999".to_string()]);
    }

    #[test]
    fn test_parse_env_localhost_zero_host_and_quoted_value() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "METRICS_HOST=\"http://0.0.0.0:7070\"\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        parse_env_localhost_urls(&dir.path().join(".env"), &mut externals, &HashMap::new());
        assert_eq!(externals, vec!["Internal: METRICS :7070".to_string()]);
    }

    #[test]
    fn test_parse_env_localhost_missing_file_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let mut externals = Vec::new();
        parse_env_localhost_urls(
            &dir.path().join("does-not-exist.env"),
            &mut externals,
            &HashMap::new(),
        );
        assert!(externals.is_empty());
    }

    #[test]
    fn test_detect_from_compose_alternate_filenames() {
        // docker-compose.yaml variant
        let dir1 = tempfile::tempdir().unwrap();
        std::fs::write(
            dir1.path().join("docker-compose.yaml"),
            "services:\n  db:\n    image: mysql:8\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_compose(dir1.path(), &mut externals);
        assert!(externals.iter().any(|e| e == "MySQL"), "{:?}", externals);

        // compose.yml variant
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(
            dir2.path().join("compose.yml"),
            "services:\n  db:\n    image: mongo:7\n",
        )
        .unwrap();
        let mut externals = Vec::new();
        detect_from_compose(dir2.path(), &mut externals);
        assert!(externals.iter().any(|e| e == "MongoDB"), "{:?}", externals);
    }

    #[test]
    fn test_detect_from_compose_without_file_detects_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut externals = Vec::new();
        detect_from_compose(dir.path(), &mut externals);
        assert!(externals.is_empty());
    }

    #[test]
    fn test_detect_from_source_code_scans_and_skips_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/api.ts"),
            r#"
const gh = "https://api.github.com/repos/x";
const pay = process.env.PAYMENT_SERVICE_URL;
const local = "http://localhost:4000/api/users";
"#,
        )
        .unwrap();
        // Files under node_modules must never be scanned.
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(
            dir.path().join("node_modules/vendor.ts"),
            "const evil = \"https://api.evilcorp.com/x\";\n",
        )
        .unwrap();

        let mut port_map = HashMap::new();
        port_map.insert(4000, "backend".to_string());
        let mut externals = Vec::new();
        detect_from_source_code(dir.path(), &mut externals, &port_map);

        assert!(
            externals.iter().any(|e| e == "API: github.com"),
            "{:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e.contains("PAYMENT SERVICE")),
            "env var reference must be reported: {:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e.contains("backend")),
            "localhost URL must cross-reference the port map: {:?}",
            externals
        );
        assert!(
            !externals.iter().any(|e| e.contains("evilcorp")),
            "node_modules must be skipped: {:?}",
            externals
        );
    }

    #[test]
    fn test_collect_source_files_depth_limit_and_skip_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.ts"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        std::fs::write(dir.path().join("a/b/mid.ts"), "").unwrap();
        std::fs::write(dir.path().join("a/b/c/deep.ts"), "").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/skip.ts"), "").unwrap();
        // Non-matching extension is ignored.
        std::fs::write(dir.path().join("readme.md"), "").unwrap();

        let mut files = Vec::new();
        collect_source_files(dir.path(), &["ts"], &mut files, 0, 3);
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"main.ts".to_string()), "{:?}", names);
        assert!(names.contains(&"mid.ts".to_string()), "{:?}", names);
        assert!(
            !names.contains(&"deep.ts".to_string()),
            "depth 3 must not be scanned: {:?}",
            names
        );
        assert!(
            !names.contains(&"skip.ts".to_string()),
            "target/ must be skipped: {:?}",
            names
        );
        assert!(!names.contains(&"readme.md".to_string()));
    }

    #[test]
    fn test_extract_urls_from_source_backtick_and_short_urls() {
        let content = "const u = `https://api.service.io/v1`;\nconst s = \"http://a.b\";\n";
        let urls = extract_urls_from_source(content);
        assert!(
            urls.iter().any(|u| u.contains("api.service.io")),
            "backtick-quoted URLs must be extracted: {:?}",
            urls
        );
        assert!(
            !urls.iter().any(|u| u == "http://a.b"),
            "URLs of 10 chars or fewer must be discarded: {:?}",
            urls
        );
    }

    #[test]
    fn test_classify_url_skip_branches() {
        let port_map = HashMap::new();
        // Template placeholders
        assert!(classify_url("https://{{HOST}}/api", &port_map).is_none());
        // Reserved-style TLDs
        assert!(classify_url("https://internal.local/x", &port_map).is_none());
        assert!(classify_url("https://svc.internal/api", &port_map).is_none());
        // Localhost without a port carries no information
        assert!(classify_url("http://localhost/health", &port_map).is_none());
        // Bare hostname without a dot
        assert!(classify_url("https://singlehost/api", &port_map).is_none());
    }

    #[test]
    fn test_classify_url_subdomain_reduced_to_registrable_domain() {
        let port_map = HashMap::new();
        let result = classify_url("https://deep.api.stripe.com/v1/x", &port_map);
        assert_eq!(result, Some("API: stripe.com".to_string()));
    }

    #[test]
    fn test_extract_env_url_refs_language_patterns() {
        let content = r#"
url = os.getenv("PAYMENT_URL")
let e = env::var("BILLING_ENDPOINT").unwrap();
String c = System.getenv("CATALOG_API");
host := viper.Get("ORDERS_HOST")
val = os.environ["INVENTORY_URI"]
"#;
        let mut externals = Vec::new();
        extract_env_url_refs(content, &mut externals);
        assert!(
            externals.iter().any(|e| e == "API: PAYMENT"),
            "{:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e == "API: BILLING"),
            "{:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e == "API: CATALOG"),
            "{:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e == "API: ORDERS"),
            "{:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e == "API: INVENTORY"),
            "{:?}",
            externals
        );
    }

    #[test]
    fn test_extract_env_url_refs_excludes_datastores_and_short_names() {
        let content = r#"
a = process.env.DATABASE_URL
b = process.env.REDIS_HOST
c = process.env.MONGO_URI
d = process.env.UI_URL
e = process.env.PORT
"#;
        let mut externals = Vec::new();
        extract_env_url_refs(content, &mut externals);
        assert!(
            externals.is_empty(),
            "datastore vars, short names and non-URL vars must be excluded: {:?}",
            externals
        );
    }

    #[test]
    fn test_detect_external_services_aggregates_project_dirs() {
        use crate::model::{Project, Service, Target};

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".env"), "REDIS_URL=redis://localhost\n").unwrap();
        std::fs::write(
            root.path().join("docker-compose.yml"),
            "services:\n  db:\n    image: postgres:16\n",
        )
        .unwrap();

        // A second directory reached through a service working_dir.
        let svc_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            svc_dir.path().join(".env"),
            "MONGO_URI=mongodb://localhost/app\n",
        )
        .unwrap();

        let project = Project {
            name: "demo".to_string(),
            description: String::new(),
            path: root.path().to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services: vec![Service {
                name: "api".to_string(),
                command: "run-api.exe".to_string(),
                target: Target::Windows,
                working_dir: Some(svc_dir.path().to_string_lossy().to_string()),
                enabled: true,
                env_vars: Vec::new(),
                depends_on: Vec::new(),
                docker: None,
            }],
            hooks: None,
        };

        let externals = detect_external_services(root.path(), &project);
        assert!(externals.iter().any(|e| e == "Redis"), "{:?}", externals);
        assert!(
            externals.iter().any(|e| e == "PostgreSQL"),
            "{:?}",
            externals
        );
        assert!(
            externals.iter().any(|e| e == "MongoDB"),
            "service working_dir must be scanned too: {:?}",
            externals
        );
    }
}
