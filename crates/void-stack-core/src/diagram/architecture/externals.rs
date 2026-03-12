//! External service detection from env files, docker-compose, and source code.

use std::path::Path;

use crate::runner::local::strip_win_prefix;
use crate::security;

use super::service_detection;

pub(super) fn detect_external_services(root: &Path, project: &crate::model::Project) -> Vec<String> {
    let mut externals = Vec::new();

    let mut dirs_to_scan: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    for svc in &project.services {
        if let Some(dir) = &svc.working_dir {
            dirs_to_scan.push(Path::new(&strip_win_prefix(dir)).to_path_buf());
        }
    }

    // Build a map of port → service name for localhost cross-referencing
    let mut port_to_service: std::collections::HashMap<u16, String> = std::collections::HashMap::new();
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

fn detect_from_env(dir: &Path, externals: &mut Vec<String>, port_map: &std::collections::HashMap<u16, String>) {
    for env_file in &[".env", ".env.example", ".env.local"] {
        let keys = security::read_env_keys(&dir.join(env_file));
        let keys_upper: String = keys.join(" ").to_uppercase();

        parse_env_localhost_urls(&dir.join(env_file), externals, port_map);

        // Databases
        if keys_upper.contains("POSTGRES") || keys_upper.contains("DATABASE_URL") || keys_upper.contains("PG_") {
            add_unique(externals, "PostgreSQL");
        }
        if keys_upper.contains("MYSQL") || keys_upper.contains("MARIADB") {
            add_unique(externals, "MySQL");
        }
        if keys_upper.contains("REDIS") {
            add_unique(externals, "Redis");
        }
        if keys_upper.contains("MONGO") {
            add_unique(externals, "MongoDB");
        }
        if keys_upper.contains("ELASTIC") || keys_upper.contains("OPENSEARCH") {
            add_unique(externals, "Elasticsearch");
        }

        // AI / ML services
        if keys_upper.contains("OLLAMA") { add_unique(externals, "Ollama"); }
        if keys_upper.contains("OPENAI") { add_unique(externals, "OpenAI"); }
        if keys_upper.contains("ANTHROPIC") { add_unique(externals, "Anthropic"); }

        // Cloud / Storage
        if keys_upper.contains("S3") || keys_upper.contains("AWS") { add_unique(externals, "AWS S3"); }
        if keys_upper.contains("AZURE") { add_unique(externals, "Azure"); }
        if keys_upper.contains("GCP") || keys_upper.contains("GOOGLE_CLOUD") { add_unique(externals, "GCP"); }
        if keys_upper.contains("CLOUDINARY") { add_unique(externals, "Cloudinary"); }

        // Messaging / Queues
        if keys_upper.contains("RABBITMQ") || keys_upper.contains("AMQP") { add_unique(externals, "RabbitMQ"); }
        if keys_upper.contains("KAFKA") { add_unique(externals, "Kafka"); }

        // Email / Notifications
        if keys_upper.contains("SMTP") || keys_upper.contains("SENDGRID") || keys_upper.contains("MAILGUN") {
            add_unique(externals, "Email Service");
        }
        if keys_upper.contains("TWILIO") || keys_upper.contains("SMS") {
            add_unique(externals, "SMS Service");
        }
        if keys_upper.contains("FIREBASE") { add_unique(externals, "Firebase"); }
        if keys_upper.contains("STRIPE") { add_unique(externals, "Stripe"); }
        if keys_upper.contains("SENTRY") { add_unique(externals, "Sentry"); }

        // Internal API references
        let internal_patterns = ["_API_URL", "_CORE_URL", "_SERVICE_URL", "_REMOTE_URL", "_INTERNAL"];
        for key in &keys {
            let upper = key.to_uppercase();
            if internal_patterns.iter().any(|p| upper.contains(p)) {
                add_unique(externals, "Internal APIs");
            }
        }
    }
}

fn parse_env_localhost_urls(path: &Path, externals: &mut Vec<String>, port_map: &std::collections::HashMap<u16, String>) {
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

            if !(val_lower.starts_with("http://localhost") || val_lower.starts_with("http://127.0.0.1")
                || val_lower.starts_with("http://0.0.0.0")) {
                continue;
            }

            let after_scheme = val_lower
                .strip_prefix("http://").unwrap_or(&val_lower);
            let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
            let port: Option<u16> = host_port.split(':').nth(1).and_then(|p| p.parse().ok());

            if let Some(p) = port {
                let key_clean = _key.trim().to_uppercase();
                if let Some(svc_name) = port_map.get(&p) {
                    add_unique(externals, &format!("→ {} ({})", svc_name, key_clean));
                } else {
                    let label = key_clean
                        .replace("_URL", "").replace("_URI", "")
                        .replace("_BASE", "").replace("_HOST", "")
                        .replace("_ENDPOINT", "").replace("_API", "")
                        .replace('_', " ").trim().to_string();
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
            if lower.contains("postgres") { add_unique(externals, "PostgreSQL"); }
            if lower.contains("mysql") || lower.contains("mariadb") { add_unique(externals, "MySQL"); }
            if lower.contains("redis") { add_unique(externals, "Redis"); }
            if lower.contains("mongo") { add_unique(externals, "MongoDB"); }
            if lower.contains("rabbitmq") { add_unique(externals, "RabbitMQ"); }
            if lower.contains("kafka") { add_unique(externals, "Kafka"); }
            if lower.contains("elasticsearch") || lower.contains("opensearch") { add_unique(externals, "Elasticsearch"); }
            if lower.contains("minio") { add_unique(externals, "MinIO/S3"); }
            if lower.contains("nginx") { add_unique(externals, "Nginx"); }
        }
    }
}

fn detect_from_source_code(dir: &Path, externals: &mut Vec<String>, port_map: &std::collections::HashMap<u16, String>) {
    let code_exts = ["ts", "js", "py", "go", "rs", "dart", "java", "kt", "rb", "php"];

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
            if let Some(label) = classify_url(&url, port_map) {
                if seen.insert(label.clone()) {
                    add_unique(externals, &label);
                }
            }
        }

        extract_env_url_refs(&content, externals);
    }
}

fn collect_source_files(dir: &Path, exts: &[&str], files: &mut Vec<std::path::PathBuf>, depth: usize, max_depth: usize) {
    if depth >= max_depth { return; }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if matches!(name.as_str(), "node_modules" | ".git" | "dist" | "build" | "target" | "__pycache__" | ".venv" | "vendor") {
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
                let end = rest.find(|c: char| c == *quote || c == ' ' || c == '\n' || c == '\r')
                    .unwrap_or(rest.len());
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

    if lower.contains("${") || lower.contains("{{") || lower.contains("example.com")
        || lower.contains("placeholder") {
        return None;
    }

    let after_scheme = lower
        .strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = host_port.split(':').next().unwrap_or(host_port);

    let is_local = host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0";

    if is_local {
        let port: Option<u16> = host_port.split(':').nth(1).and_then(|p| p.parse().ok());
        if let Some(p) = port {
            if let Some(svc_name) = port_map.get(&p) {
                let path = after_scheme.find('/').map(|i| &after_scheme[i..]).unwrap_or("");
                let path_hint = if path.len() > 1 {
                    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).take(2).collect();
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
            let name = parts[parts.len()-2..].join(".");
            return Some(format!("API: {}", name));
        }
    }

    None
}

fn extract_env_url_refs(content: &str, externals: &mut Vec<String>) {
    let env_patterns = [
        "process.env.", "os.environ", "os.getenv", "env::var", "System.getenv",
        "Environment.GetEnvironmentVariable", "viper.Get",
    ];

    for line in content.lines() {
        let trimmed = line.trim();
        for pat in &env_patterns {
            if let Some(pos) = trimmed.find(pat) {
                let rest = &trimmed[pos + pat.len()..];
                let var_name: String = rest.trim_start_matches(&['[', '(', '"', '\'', '`'][..])
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let upper = var_name.to_uppercase();
                if (upper.contains("_URL") || upper.contains("_URI") || upper.contains("_ENDPOINT")
                    || upper.contains("_HOST") || upper.contains("_API"))
                    && !upper.contains("DATABASE") && !upper.contains("REDIS") && !upper.contains("MONGO")
                {
                    let service_name = var_name
                        .replace("_URL", "").replace("_URI", "")
                        .replace("_ENDPOINT", "").replace("_HOST", "")
                        .replace("_API", "").replace("_BASE", "")
                        .replace('_', " ").trim().to_string();
                    if !service_name.is_empty() && service_name.len() > 2 {
                        add_unique(externals, &format!("API: {}", service_name));
                    }
                }
            }
        }
    }
}
