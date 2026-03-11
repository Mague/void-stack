//! Service architecture diagram generator.

use std::path::Path;

use crate::docker;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::security;

use super::service_detection::{self, ServiceType};

/// Generate a Mermaid architecture diagram for a project's services.
pub fn generate(project: &Project) -> String {
    let mut lines = vec![
        "```mermaid".to_string(),
        "graph TB".to_string(),
        format!("    subgraph proj_{} [\"{}\" ]", sanitize_id(&project.name), project.name),
    ];

    let mut connections: Vec<(String, String)> = Vec::new();

    for svc in &project.services {
        let id = sanitize_id(&svc.name);
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);

        // Detect what kind of service this is
        let (svc_type, port) = service_detection::detect_service_info(dir_path, &svc.command);
        let icon = match svc_type {
            ServiceType::Frontend => "🌐",
            ServiceType::Backend => "⚙️",
            ServiceType::Database => "🗄️",
            ServiceType::Worker => "⚡",
            ServiceType::Unknown => "📦",
        };

        let port_label = port
            .map(|p| format!(" :{}",p))
            .unwrap_or_default();

        lines.push(format!(
            "        {}[\"{} {}{}<br/>{}\"]",
            id, icon, svc.name, port_label,
            match svc_type {
                ServiceType::Frontend => "Frontend",
                ServiceType::Backend => "API",
                ServiceType::Database => "Database",
                ServiceType::Worker => "Worker",
                ServiceType::Unknown => &svc.command,
            }
        ));

        // Auto-detect connections
        if matches!(svc_type, ServiceType::Frontend) {
            // Frontend likely connects to backends
            for other in &project.services {
                let other_dir = other.working_dir.as_deref().unwrap_or(&project.path);
                let other_dir_clean = strip_win_prefix(other_dir);
                let other_path = Path::new(&other_dir_clean);
                let (other_type, _) = service_detection::detect_service_info(other_path, &other.command);
                if matches!(other_type, ServiceType::Backend) {
                    connections.push((id.clone(), sanitize_id(&other.name)));
                }
            }
        }
    }

    lines.push("    end".to_string());

    // Add external services
    let root = strip_win_prefix(&project.path);
    let root_path = Path::new(&root);
    let externals = detect_external_services(root_path, project);
    for ext in &externals {
        lines.push(format!("    {}[(\"{}\")]", sanitize_id(ext), ext));
    }

    // Detect Rust crate relationships from Cargo.toml workspace
    let crate_links = detect_crate_relationships(root_path);
    if !crate_links.is_empty() {
        lines.push(format!("    subgraph crates [\"Rust Crates\"]"));
        let mut crate_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (from, to) in &crate_links {
            crate_names.insert(from);
            crate_names.insert(to);
        }
        for name in &crate_names {
            let cid = format!("crate_{}", sanitize_id(name));
            lines.push(format!("        {}[\"📦 {}\"]", cid, name));
        }
        lines.push("    end".to_string());
        for (from, to) in &crate_links {
            let fid = format!("crate_{}", sanitize_id(from));
            let tid = format!("crate_{}", sanitize_id(to));
            lines.push(format!("    {} -->|dep| {}", fid, tid));
        }
    }

    // Add connections
    for (from, to) in &connections {
        lines.push(format!("    {} -->|API| {}", from, to));
    }
    for ext in &externals {
        let ext_id = sanitize_id(ext);
        // Connect backends to external services
        for svc in &project.services {
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let dir_stripped = strip_win_prefix(dir);
            let dir_path = Path::new(&dir_stripped);
            let (svc_type, _) = service_detection::detect_service_info(dir_path, &svc.command);
            if matches!(svc_type, ServiceType::Backend) {
                lines.push(format!("    {} -.-> {}", sanitize_id(&svc.name), ext_id));
            }
        }
    }

    // Infrastructure: Terraform, Kubernetes, Helm
    let docker_analysis = docker::analyze_docker(root_path);
    let infra_node_ids = generate_infra_subgraphs(&docker_analysis, &mut lines);

    // Connect backend services to infra resources
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_stripped = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_stripped);
        let (svc_type, _) = service_detection::detect_service_info(dir_path, &svc.command);
        if matches!(svc_type, ServiceType::Backend) {
            for infra_id in &infra_node_ids {
                lines.push(format!("    {} -.-> {}", sanitize_id(&svc.name), infra_id));
            }
        }
    }

    // Styling
    lines.push("".to_string());
    lines.push("    classDef frontend fill:#4CAF50,stroke:#333,color:#fff".to_string());
    lines.push("    classDef backend fill:#2196F3,stroke:#333,color:#fff".to_string());
    lines.push("    classDef database fill:#FF9800,stroke:#333,color:#fff".to_string());
    lines.push("    classDef external fill:#9E9E9E,stroke:#333,color:#fff".to_string());
    lines.push("    classDef crate fill:#E65100,stroke:#BF360C,color:#fff".to_string());
    lines.push("    classDef infra_db fill:#E91E63,stroke:#880E4F,color:#fff".to_string());
    lines.push("    classDef infra_cache fill:#FF5722,stroke:#BF360C,color:#fff".to_string());
    lines.push("    classDef infra_storage fill:#607D8B,stroke:#37474F,color:#fff".to_string());
    lines.push("    classDef infra_compute fill:#9C27B0,stroke:#4A148C,color:#fff".to_string());
    lines.push("    classDef infra_queue fill:#FFC107,stroke:#FF8F00,color:#000".to_string());
    lines.push("    classDef k8s fill:#326CE5,stroke:#1A3F7A,color:#fff".to_string());
    lines.push("    classDef helm fill:#0F1689,stroke:#091058,color:#fff".to_string());

    for svc in &project.services {
        let id = sanitize_id(&svc.name);
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let (svc_type, _) = service_detection::detect_service_info(Path::new(&strip_win_prefix(dir)), &svc.command);
        let class = match svc_type {
            ServiceType::Frontend => "frontend",
            ServiceType::Backend => "backend",
            ServiceType::Database => "database",
            _ => "backend",
        };
        lines.push(format!("    class {} {}", id, class));
    }
    for ext in &externals {
        lines.push(format!("    class {} external", sanitize_id(ext)));
    }
    for (from, to) in &crate_links {
        lines.push(format!("    class crate_{} crate", sanitize_id(from)));
        lines.push(format!("    class crate_{} crate", sanitize_id(to)));
    }

    lines.push("```".to_string());
    lines.join("\n")
}

// ServiceType, detect_service_info, and extract_port are now in
// super::service_detection (shared with drawio.rs).

fn detect_external_services(root: &Path, project: &Project) -> Vec<String> {
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
        // 1. Check .env files (keys + localhost URLs for service cross-referencing)
        detect_from_env(dir, &mut externals, &port_to_service);

        // 2. Check docker-compose (safe — not a credentials file)
        detect_from_compose(dir, &mut externals);

        // 3. Scan source code for HTTP client calls and extract URLs
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

        // Parse env file for localhost URLs to cross-reference with services
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

        // Internal API references (project's own services calling each other)
        let internal_patterns = ["_API_URL", "_CORE_URL", "_SERVICE_URL", "_REMOTE_URL", "_INTERNAL"];
        for key in &keys {
            let upper = key.to_uppercase();
            if internal_patterns.iter().any(|p| upper.contains(p)) {
                add_unique(externals, "Internal APIs");
            }
        }
    }
}

/// Parse .env file for localhost URLs and cross-reference ports with project services.
/// Only reads URL values (http://localhost:...) — ignores secrets/tokens/passwords.
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

        if let Some((key, value)) = trimmed.split_once('=') {
            let val = value.trim().trim_matches('"').trim_matches('\'');
            let val_lower = val.to_lowercase();

            // Only process values that are localhost URLs
            if !(val_lower.starts_with("http://localhost") || val_lower.starts_with("http://127.0.0.1")
                || val_lower.starts_with("http://0.0.0.0")) {
                continue;
            }

            // Extract port
            let after_scheme = val_lower
                .strip_prefix("http://").unwrap_or(&val_lower);
            let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
            let port: Option<u16> = host_port.split(':').nth(1).and_then(|p| p.parse().ok());

            if let Some(p) = port {
                let key_clean = key.trim().to_uppercase();
                if let Some(svc_name) = port_map.get(&p) {
                    add_unique(externals, &format!("→ {} ({})", svc_name, key_clean));
                } else {
                    // Unknown port — derive name from env var key
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

/// Scan source code for HTTP client calls and extract the actual URLs/domains being consumed.
/// Language-agnostic: works with any language that makes HTTP calls.
fn detect_from_source_code(dir: &Path, externals: &mut Vec<String>, port_map: &std::collections::HashMap<u16, String>) {
    let code_exts = ["ts", "js", "py", "go", "rs", "dart", "java", "kt", "rb", "php"];

    // Scan all source files (up to 3 levels deep)
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

        // Extract URLs from source code (string literals containing http:// or https://)
        for url in extract_urls_from_source(&content) {
            if let Some(label) = classify_url(&url, port_map) {
                if seen.insert(label.clone()) {
                    add_unique(externals, &label);
                }
            }
        }

        // Also check for env var references that imply external/internal service URLs
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
            // Skip irrelevant dirs
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

/// Extract URLs from source code string literals.
fn extract_urls_from_source(content: &str) -> Vec<String> {
    let mut urls = Vec::new();

    // Match URLs in string literals: "https://...", 'https://...', `https://...`
    // Also http://
    for quote in &['"', '\'', '`'] {
        let prefix_http = format!("{}http://", quote);
        let prefix_https = format!("{}https://", quote);
        for prefix in &[&prefix_http, &prefix_https] {
            let mut start = 0;
            while let Some(pos) = content[start..].find(prefix.as_str()) {
                let abs_pos = start + pos + 1; // skip opening quote
                let rest = &content[abs_pos..];
                // Find the closing quote or whitespace
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

/// Classify a URL into a meaningful service name.
/// For localhost URLs, cross-references the port with known project services.
/// For external URLs, extracts the domain.
fn classify_url(url: &str, port_map: &std::collections::HashMap<u16, String>) -> Option<String> {
    let lower = url.to_lowercase();

    // Skip template/placeholder URLs
    if lower.contains("${") || lower.contains("{{") || lower.contains("example.com")
        || lower.contains("placeholder") {
        return None;
    }

    // Extract host:port from URL
    let after_scheme = lower
        .strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = host_port.split(':').next().unwrap_or(host_port);

    let is_local = host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0";

    if is_local {
        // Extract port from localhost URL
        let port: Option<u16> = host_port
            .split(':')
            .nth(1)
            .and_then(|p| p.parse().ok());

        if let Some(p) = port {
            // Cross-reference with project services
            if let Some(svc_name) = port_map.get(&p) {
                // Extract the API path for context
                let path = after_scheme.find('/').map(|i| &after_scheme[i..]).unwrap_or("");
                let path_hint = if path.len() > 1 {
                    // Take first 2 path segments for context
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
            // Unknown localhost port — still an internal service call
            return Some(format!("Internal :{}", p));
        }
        // localhost without port — not useful
        return None;
    }

    // External URL — extract domain
    let domain = host;

    // For unknown domains, use the domain itself
    if domain.contains('.') && !domain.ends_with(".local") && !domain.ends_with(".internal") {
        let parts: Vec<&str> = domain.split('.').collect();
        if parts.len() >= 2 {
            let name = parts[parts.len()-2..].join(".");
            return Some(format!("API: {}", name));
        }
    }

    None
}

/// Detect env var references that suggest external service URLs.
fn extract_env_url_refs(content: &str, externals: &mut Vec<String>) {
    // Look for env var names containing URL/URI/ENDPOINT patterns
    // These indicate the code consumes an external or internal service
    let env_patterns = [
        "process.env.", "os.environ", "os.getenv", "env::var", "System.getenv",
        "Environment.GetEnvironmentVariable", "viper.Get",
    ];

    for line in content.lines() {
        let trimmed = line.trim();
        for pat in &env_patterns {
            if let Some(pos) = trimmed.find(pat) {
                let rest = &trimmed[pos + pat.len()..];
                // Extract the variable name
                let var_name: String = rest.trim_start_matches(&['[', '(', '"', '\'', '`'][..])
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let upper = var_name.to_uppercase();
                if (upper.contains("_URL") || upper.contains("_URI") || upper.contains("_ENDPOINT")
                    || upper.contains("_HOST") || upper.contains("_API"))
                    && !upper.contains("DATABASE") && !upper.contains("REDIS") && !upper.contains("MONGO")
                {
                    // This implies the code calls an external/internal API
                    // The var name itself describes the service
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

/// Detect internal crate dependency relationships from Cargo.toml workspace.
///
/// Reads the workspace Cargo.toml to find members, then reads each member's
/// Cargo.toml to find dependencies on other workspace members.
fn detect_crate_relationships(root: &Path) -> Vec<(String, String)> {
    let workspace_toml = root.join("Cargo.toml");
    let content = match std::fs::read_to_string(&workspace_toml) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let parsed: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    // Extract workspace members
    let members = match parsed
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    {
        Some(m) => m,
        None => return Vec::new(),
    };

    let member_paths: Vec<String> = members
        .iter()
        .filter_map(|m| m.as_str().map(|s| s.to_string()))
        .collect();

    // Collect workspace crate names
    let mut crate_names: std::collections::HashMap<String, String> = std::collections::HashMap::new(); // name -> member_path
    for member_path in &member_paths {
        let member_toml = root.join(member_path).join("Cargo.toml");
        if let Ok(c) = std::fs::read_to_string(&member_toml) {
            if let Ok(v) = c.parse::<toml::Value>() {
                if let Some(name) = v
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                {
                    crate_names.insert(name.to_string(), member_path.clone());
                }
            }
        }
    }

    // Find internal dependencies
    let mut links: Vec<(String, String)> = Vec::new();
    for member_path in &member_paths {
        let member_toml = root.join(member_path).join("Cargo.toml");
        let content = match std::fs::read_to_string(&member_toml) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let crate_name = match parsed
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Check [dependencies] and [dev-dependencies]
        for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(deps) = parsed.get(section).and_then(|d| d.as_table()) {
                for dep_name in deps.keys() {
                    if crate_names.contains_key(dep_name) && *dep_name != crate_name {
                        let link = (crate_name.clone(), dep_name.clone());
                        if !links.contains(&link) {
                            links.push(link);
                        }
                    }
                }
            }
        }
    }

    links
}

/// Generate Mermaid subgraphs for Terraform, Kubernetes, and Helm resources.
/// Returns a list of node IDs for infrastructure resources (used for connections).
fn generate_infra_subgraphs(analysis: &docker::DockerAnalysis, lines: &mut Vec<String>) -> Vec<String> {
    let mut infra_ids = Vec::new();

    // Terraform resources
    if !analysis.terraform.is_empty() {
        lines.push("    subgraph infra [\"Infrastructure (Terraform)\"]".to_string());
        for res in &analysis.terraform {
            let id = format!("tf_{}_{}", sanitize_id(&res.provider), sanitize_id(&res.name));
            let details = if res.details.is_empty() {
                String::new()
            } else {
                format!("<br/>{}", res.details.join(", "))
            };

            let node = match res.kind {
                docker::InfraResourceKind::Database => {
                    format!("        {}[(\"{} {}{}\")]", id, res.resource_type, res.name, details)
                }
                docker::InfraResourceKind::Compute => {
                    format!("        {}{{\"{} {}{}\"}}", id, res.resource_type, res.name, details)
                }
                docker::InfraResourceKind::Storage => {
                    format!("        {}[/\"{} {}{}\"/]", id, res.resource_type, res.name, details)
                }
                docker::InfraResourceKind::Queue => {
                    format!("        {}[[\"{} {}{}\"]]", id, res.resource_type, res.name, details)
                }
                _ => {
                    format!("        {}[\"{} {}{}\"]", id, res.resource_type, res.name, details)
                }
            };
            lines.push(node);

            let class = match res.kind {
                docker::InfraResourceKind::Database => "infra_db",
                docker::InfraResourceKind::Cache => "infra_cache",
                docker::InfraResourceKind::Storage => "infra_storage",
                docker::InfraResourceKind::Compute => "infra_compute",
                docker::InfraResourceKind::Queue => "infra_queue",
                _ => "external",
            };
            // Defer class assignment — collect for later
            infra_ids.push(format!("{}:{}", id, class));
        }
        lines.push("    end".to_string());
    }

    // Kubernetes resources
    if !analysis.kubernetes.is_empty() {
        lines.push("    subgraph k8s [\"Kubernetes\"]".to_string());
        for res in &analysis.kubernetes {
            let id = format!("k8s_{}_{}", sanitize_id(&res.kind), sanitize_id(&res.name));
            let extras = build_k8s_extras(res);

            let node = match res.kind.as_str() {
                "Deployment" | "StatefulSet" | "DaemonSet" => {
                    format!("        {}[\"{}: {}{}\"]", id, res.kind, res.name, extras)
                }
                "Service" => {
                    format!("        {}([\"{}: {}{}\"])", id, res.kind, res.name, extras)
                }
                "Ingress" => {
                    format!("        {}>{{\"{}: {}{}\"}}]", id, res.kind, res.name, extras)
                }
                _ => {
                    format!("        {}[\"{}: {}\"]", id, res.kind, res.name)
                }
            };
            lines.push(node);
        }
        lines.push("    end".to_string());

        // Add connections between K8s Service → Deployment (by name matching)
        let deployments: Vec<&docker::K8sResource> = analysis.kubernetes.iter()
            .filter(|r| r.kind == "Deployment" || r.kind == "StatefulSet")
            .collect();
        let services: Vec<&docker::K8sResource> = analysis.kubernetes.iter()
            .filter(|r| r.kind == "Service")
            .collect();
        for svc in &services {
            for deploy in &deployments {
                // Heuristic: service name contains deployment name or vice versa
                if svc.name.contains(&deploy.name) || deploy.name.contains(&svc.name) {
                    let svc_id = format!("k8s_{}_{}", sanitize_id(&svc.kind), sanitize_id(&svc.name));
                    let dep_id = format!("k8s_{}_{}", sanitize_id(&deploy.kind), sanitize_id(&deploy.name));
                    lines.push(format!("    {} --> {}", svc_id, dep_id));
                }
            }
        }
    }

    // Helm chart
    if let Some(ref chart) = analysis.helm {
        lines.push(format!("    subgraph helm_chart [\"Helm: {} v{}\"]", chart.name, chart.version));
        for dep in &chart.dependencies {
            let id = format!("helm_{}", sanitize_id(&dep.name));
            lines.push(format!("        {}[\"{} ({})\"]", id, dep.name, dep.version));
            // Apply helm class later
        }
        lines.push("    end".to_string());
    }

    // Apply styling classes for infra nodes
    let mut result_ids = Vec::new();
    for entry in &infra_ids {
        if let Some((id, class)) = entry.split_once(':') {
            lines.push(format!("    class {} {}", id, class));
            result_ids.push(id.to_string());
        }
    }

    // Apply k8s class
    for res in &analysis.kubernetes {
        let id = format!("k8s_{}_{}", sanitize_id(&res.kind), sanitize_id(&res.name));
        lines.push(format!("    class {} k8s", id));
    }

    // Apply helm class
    if let Some(ref chart) = analysis.helm {
        for dep in &chart.dependencies {
            let id = format!("helm_{}", sanitize_id(&dep.name));
            lines.push(format!("    class {} helm", id));
        }
    }

    result_ids
}

fn build_k8s_extras(res: &docker::K8sResource) -> String {
    let mut parts = Vec::new();
    if let Some(r) = res.replicas {
        parts.push(format!("x{}", r));
    }
    if !res.images.is_empty() {
        parts.push(res.images.join(", "));
    }
    if !res.ports.is_empty() {
        let ports: Vec<String> = res.ports.iter().map(|p| p.to_string()).collect();
        parts.push(format!(":{}", ports.join(",")));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("<br/>{}", parts.join(" | "))
    }
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
