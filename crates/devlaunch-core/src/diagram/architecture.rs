//! Service architecture diagram generator.

use std::path::Path;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Generate a Mermaid architecture diagram for a project's services.
pub fn generate(project: &Project) -> String {
    let mut lines = vec![
        "```mermaid".to_string(),
        "graph TB".to_string(),
        format!("    subgraph {} [\"{}\" ]", sanitize_id(&project.name), project.name),
    ];

    let mut connections: Vec<(String, String)> = Vec::new();

    for svc in &project.services {
        let id = sanitize_id(&svc.name);
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);

        // Detect what kind of service this is
        let (svc_type, port) = detect_service_info(dir_path, &svc.command);
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
            "        {}[\"{} {}{}\\n{}\"]",
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
                let (other_type, _) = detect_service_info(other_path, &other.command);
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
            let (svc_type, _) = detect_service_info(dir_path, &svc.command);
            if matches!(svc_type, ServiceType::Backend) {
                lines.push(format!("    {} -.-> {}", sanitize_id(&svc.name), ext_id));
            }
        }
    }

    // Styling
    lines.push("".to_string());
    lines.push("    classDef frontend fill:#4CAF50,stroke:#333,color:#fff".to_string());
    lines.push("    classDef backend fill:#2196F3,stroke:#333,color:#fff".to_string());
    lines.push("    classDef database fill:#FF9800,stroke:#333,color:#fff".to_string());
    lines.push("    classDef external fill:#9E9E9E,stroke:#333,color:#fff".to_string());

    for svc in &project.services {
        let id = sanitize_id(&svc.name);
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let (svc_type, _) = detect_service_info(Path::new(&strip_win_prefix(dir)), &svc.command);
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

    lines.push("```".to_string());
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum ServiceType {
    Frontend,
    Backend,
    Database,
    Worker,
    Unknown,
}

fn detect_service_info(dir: &Path, command: &str) -> (ServiceType, Option<u16>) {
    let cmd_lower = command.to_lowercase();

    // Check by command
    if cmd_lower.contains("npm run dev") || cmd_lower.contains("yarn dev") || cmd_lower.contains("vite") || cmd_lower.contains("next") {
        let port = extract_port_from_cmd(&cmd_lower).or(Some(3000));
        return (ServiceType::Frontend, port);
    }
    if cmd_lower.contains("uvicorn") || cmd_lower.contains("gunicorn") || cmd_lower.contains("flask") {
        let port = extract_port_from_cmd(&cmd_lower).or(Some(8000));
        return (ServiceType::Backend, port);
    }
    if cmd_lower.contains("django") || cmd_lower.contains("manage.py") {
        let port = extract_port_from_cmd(&cmd_lower).or(Some(8000));
        return (ServiceType::Backend, port);
    }
    if cmd_lower.contains("cargo run") || cmd_lower.contains("go run") {
        return (ServiceType::Backend, extract_port_from_cmd(&cmd_lower));
    }
    if cmd_lower.starts_with("python ") || cmd_lower.starts_with("python3 ") {
        let port = extract_port_from_cmd(&cmd_lower).or(Some(8000));
        return (ServiceType::Backend, port);
    }
    if cmd_lower.contains("docker compose") {
        return (ServiceType::Unknown, None);
    }
    if cmd_lower.contains("celery") || cmd_lower.contains("worker") {
        return (ServiceType::Worker, None);
    }

    // Check by directory contents
    if dir.join("package.json").exists() {
        // Check if it's a frontend or backend Node project
        if let Ok(content) = std::fs::read_to_string(dir.join("package.json")) {
            let lower = content.to_lowercase();
            if lower.contains("react") || lower.contains("vue") || lower.contains("svelte")
                || lower.contains("next") || lower.contains("vite") || lower.contains("nuxt")
            {
                return (ServiceType::Frontend, Some(3000));
            }
            if lower.contains("express") || lower.contains("fastify") || lower.contains("nest") {
                return (ServiceType::Backend, Some(3000));
            }
        }
        return (ServiceType::Frontend, Some(3000));
    }

    if dir.join("requirements.txt").exists() || dir.join("pyproject.toml").exists() {
        return (ServiceType::Backend, Some(8000));
    }

    (ServiceType::Unknown, None)
}

fn extract_port_from_cmd(cmd: &str) -> Option<u16> {
    // Match --port NNNN, -p NNNN, :NNNN
    let patterns = ["--port ", "-p "];
    for pat in &patterns {
        if let Some(pos) = cmd.find(pat) {
            let rest = &cmd[pos + pat.len()..];
            let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(port) = port_str.parse() {
                return Some(port);
            }
        }
    }
    // Match :NNNN in URLs
    if let Some(pos) = cmd.rfind(':') {
        let rest = &cmd[pos + 1..];
        let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if port_str.len() >= 4 {
            if let Ok(port) = port_str.parse() {
                return Some(port);
            }
        }
    }
    None
}

fn detect_external_services(root: &Path, project: &Project) -> Vec<String> {
    let mut externals = Vec::new();

    // Scan all service directories for external service references
    let mut dirs_to_scan: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    for svc in &project.services {
        if let Some(dir) = &svc.working_dir {
            dirs_to_scan.push(Path::new(&strip_win_prefix(dir)).to_path_buf());
        }
    }

    for dir in &dirs_to_scan {
        // Check .env files for common services
        for env_file in &[".env", ".env.example"] {
            if let Ok(content) = std::fs::read_to_string(dir.join(env_file)) {
                let upper = content.to_uppercase();
                if upper.contains("POSTGRES") || upper.contains("DATABASE_URL") {
                    if !externals.contains(&"PostgreSQL".to_string()) {
                        externals.push("PostgreSQL".to_string());
                    }
                }
                if upper.contains("REDIS") {
                    if !externals.contains(&"Redis".to_string()) {
                        externals.push("Redis".to_string());
                    }
                }
                if upper.contains("MONGO") {
                    if !externals.contains(&"MongoDB".to_string()) {
                        externals.push("MongoDB".to_string());
                    }
                }
                if upper.contains("OLLAMA") {
                    if !externals.contains(&"Ollama".to_string()) {
                        externals.push("Ollama".to_string());
                    }
                }
                if upper.contains("OPENAI") || upper.contains("ANTHROPIC") {
                    if !externals.contains(&"AI API".to_string()) {
                        externals.push("AI API".to_string());
                    }
                }
                if upper.contains("S3") || upper.contains("AWS") {
                    if !externals.contains(&"AWS S3".to_string()) {
                        externals.push("AWS S3".to_string());
                    }
                }
            }
        }

        // Check docker-compose for services
        for compose in &["docker-compose.yml", "docker-compose.yaml", "compose.yml"] {
            if let Ok(content) = std::fs::read_to_string(dir.join(compose)) {
                let lower = content.to_lowercase();
                if lower.contains("postgres") && !externals.contains(&"PostgreSQL".to_string()) {
                    externals.push("PostgreSQL".to_string());
                }
                if lower.contains("redis") && !externals.contains(&"Redis".to_string()) {
                    externals.push("Redis".to_string());
                }
                if lower.contains("mongo") && !externals.contains(&"MongoDB".to_string()) {
                    externals.push("MongoDB".to_string());
                }
                if lower.contains("rabbitmq") && !externals.contains(&"RabbitMQ".to_string()) {
                    externals.push("RabbitMQ".to_string());
                }
            }
        }
    }

    externals
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
