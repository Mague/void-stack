//! Generate docker-compose.yml from project services and detected dependencies.

use std::collections::HashSet;
use std::path::Path;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Infrastructure service to include in compose.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum InfraService {
    Postgres,
    Mysql,
    Mongo,
    Redis,
    RabbitMQ,
    #[allow(dead_code)]
    Kafka,
}

impl InfraService {
    fn compose_block(&self) -> String {
        match self {
            InfraService::Postgres => r#"  postgres:
    image: postgres:16-alpine
    restart: unless-stopped
    ports:
      - "5432:5432"
    environment:
      POSTGRES_DB: app
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "postgres"]
      interval: 10s
      timeout: 5s
      retries: 5"#
                .to_string(),

            InfraService::Mysql => r#"  mysql:
    image: mysql:8
    restart: unless-stopped
    ports:
      - "3306:3306"
    environment:
      MYSQL_ROOT_PASSWORD: root
      MYSQL_DATABASE: app
    volumes:
      - mysqldata:/var/lib/mysql
    healthcheck:
      test: ["CMD", "mysqladmin", "ping", "-h", "localhost"]
      interval: 10s
      timeout: 5s
      retries: 5"#
                .to_string(),

            InfraService::Mongo => r#"  mongo:
    image: mongo:7
    restart: unless-stopped
    ports:
      - "27017:27017"
    volumes:
      - mongodata:/data/db
    healthcheck:
      test: ["CMD", "mongosh", "--eval", "db.adminCommand('ping')"]
      interval: 10s
      timeout: 5s
      retries: 5"#
                .to_string(),

            InfraService::Redis => r#"  redis:
    image: redis:7-alpine
    restart: unless-stopped
    ports:
      - "6379:6379"
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5"#
                .to_string(),

            InfraService::RabbitMQ => r#"  rabbitmq:
    image: rabbitmq:3-management-alpine
    restart: unless-stopped
    ports:
      - "5672:5672"
      - "15672:15672"
    healthcheck:
      test: ["CMD", "rabbitmq-diagnostics", "check_running"]
      interval: 10s
      timeout: 5s
      retries: 5"#
                .to_string(),

            InfraService::Kafka => r#"  kafka:
    image: bitnami/kafka:latest
    restart: unless-stopped
    ports:
      - "9092:9092"
    environment:
      KAFKA_CFG_NODE_ID: 0
      KAFKA_CFG_PROCESS_ROLES: controller,broker
      KAFKA_CFG_LISTENERS: PLAINTEXT://:9092,CONTROLLER://:9093
      KAFKA_CFG_CONTROLLER_QUORUM_VOTERS: 0@kafka:9093
      KAFKA_CFG_CONTROLLER_LISTENER_NAMES: CONTROLLER"#
                .to_string(),
        }
    }

    fn volume_name(&self) -> Option<&str> {
        match self {
            InfraService::Postgres => Some("pgdata"),
            InfraService::Mysql => Some("mysqldata"),
            InfraService::Mongo => Some("mongodata"),
            _ => None,
        }
    }

    fn service_name(&self) -> &str {
        match self {
            InfraService::Postgres => "postgres",
            InfraService::Mysql => "mysql",
            InfraService::Mongo => "mongo",
            InfraService::Redis => "redis",
            InfraService::RabbitMQ => "rabbitmq",
            InfraService::Kafka => "kafka",
        }
    }
}

/// Generate a docker-compose.yml from a project definition.
pub fn generate(project: &Project, project_path: &Path) -> String {
    let mut infra: HashSet<InfraService> = HashSet::new();
    let mut svc_blocks = Vec::new();
    let mut port_counter: u16 = 3000;

    for svc in &project.services {
        let svc_dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let clean_dir = strip_win_prefix(svc_dir);
        let svc_path = Path::new(&clean_dir);

        // Detect infrastructure needs from code
        detect_infra_from_code(svc_path, &mut infra);

        // Detect from .env
        detect_infra_from_env(svc_path, &mut infra);

        // Build compose service block
        let svc_name = svc.name.replace(' ', "-").to_lowercase();
        let port = extract_port_from_command(&svc.command).unwrap_or_else(|| {
            let p = port_counter;
            port_counter += 1;
            p
        });

        let relative_dir = make_relative(project_path, svc_path);

        let deps: Vec<String> = infra.iter().map(|i| i.service_name().to_string()).collect();
        let depends_on = if deps.is_empty() {
            String::new()
        } else {
            let items: String = deps
                .iter()
                .map(|d| format!("      - {}", d))
                .collect::<Vec<_>>()
                .join("\n");
            format!("\n    depends_on:\n{}", items)
        };

        svc_blocks.push(format!(
            r#"  {name}:
    build: {dir}
    ports:
      - "{port}:{port}"
    env_file:
      - .env{depends_on}"#,
            name = svc_name,
            dir = relative_dir,
            port = port,
            depends_on = depends_on,
        ));
    }

    // Assemble the compose file
    let mut output = String::new();
    output.push_str("services:\n");

    // App services
    for block in &svc_blocks {
        output.push_str(block);
        output.push_str("\n\n");
    }

    // Infrastructure services
    for svc in &infra {
        output.push_str(&svc.compose_block());
        output.push_str("\n\n");
    }

    // Volumes
    let volumes: Vec<&str> = infra.iter().filter_map(|i| i.volume_name()).collect();
    if !volumes.is_empty() {
        output.push_str("volumes:\n");
        for vol in &volumes {
            output.push_str(&format!("  {}:\n", vol));
        }
        output.push('\n');
    }

    // Network
    output.push_str("networks:\n  default:\n    name: void-stack-net\n");

    output
}

/// Detect infrastructure needs by scanning source files for ORM/driver imports.
fn detect_infra_from_code(path: &Path, infra: &mut HashSet<InfraService>) {
    let patterns = [
        // PostgreSQL
        (
            &[
                "sqlalchemy",
                "psycopg",
                "asyncpg",
                "pg ",
                "pg\"",
                "pg'",
                "sequelize",
                "prisma",
                "typeorm",
                "diesel",
                "sqlx",
                "gorm",
                "database/sql",
                "pgx",
                "knex",
            ] as &[&str],
            InfraService::Postgres,
        ),
        // MySQL
        (
            &["mysql2", "mysqlclient", "pymysql", "mysql-connector"] as &[&str],
            InfraService::Mysql,
        ),
        // MongoDB
        (
            &["mongoose", "pymongo", "motor", "mongodb", "mongo-driver"] as &[&str],
            InfraService::Mongo,
        ),
        // Redis
        (
            &["redis", "ioredis", "aioredis", "redis-py", "fred"] as &[&str],
            InfraService::Redis,
        ),
        // RabbitMQ
        (
            &["amqplib", "pika", "celery", "lapin", "amqp"] as &[&str],
            InfraService::RabbitMQ,
        ),
    ];

    // Scan dependency manifests
    let manifest_files = [
        "requirements.txt",
        "pyproject.toml",
        "Pipfile",
        "package.json",
        "Cargo.toml",
        "go.mod",
        "go.sum",
        "pubspec.yaml",
    ];

    for manifest in &manifest_files {
        let file_path = path.join(manifest);
        if let Ok(content) = std::fs::read_to_string(&file_path) {
            let lower = content.to_lowercase();
            for (keywords, svc) in &patterns {
                for kw in *keywords {
                    if lower.contains(kw) {
                        infra.insert(svc.clone());
                        break;
                    }
                }
            }
        }
    }
}

/// Detect infrastructure from .env DATABASE_URL patterns.
fn detect_infra_from_env(path: &Path, infra: &mut HashSet<InfraService>) {
    let env_path = path.join(".env");
    let example_path = path.join(".env.example");

    for p in &[env_path, example_path] {
        if let Ok(content) = std::fs::read_to_string(p) {
            let lower = content.to_lowercase();
            if lower.contains("postgres://") || lower.contains("postgresql://") {
                infra.insert(InfraService::Postgres);
            }
            if lower.contains("mysql://") {
                infra.insert(InfraService::Mysql);
            }
            if lower.contains("mongodb://") || lower.contains("mongodb+srv://") {
                infra.insert(InfraService::Mongo);
            }
            if lower.contains("redis://") {
                infra.insert(InfraService::Redis);
            }
            if lower.contains("amqp://") {
                infra.insert(InfraService::RabbitMQ);
            }
        }
    }
}

/// Extract port from a command string (e.g., "--port 8000", ":3000").
fn extract_port_from_command(cmd: &str) -> Option<u16> {
    // --port 8000 / -p 8000
    let re = regex::Regex::new(r"(?:--port|-p)\s+(\d+)").ok()?;
    if let Some(cap) = re.captures(cmd) {
        return cap[1].parse().ok();
    }
    // :PORT patterns (e.g., localhost:3000, 0.0.0.0:5000)
    let re2 = regex::Regex::new(r"(?:localhost|0\.0\.0\.0|127\.0\.0\.1):(\d+)").ok()?;
    if let Some(cap) = re2.captures(cmd) {
        return cap[1].parse().ok();
    }
    None
}

/// Make a path relative to a base.
fn make_relative(base: &Path, target: &Path) -> String {
    let base_str = strip_win_prefix(&base.to_string_lossy());
    let target_str = strip_win_prefix(&target.to_string_lossy());

    if target_str.starts_with(&base_str) {
        let relative = target_str[base_str.len()..].trim_start_matches(['/', '\\']);
        if relative.is_empty() {
            ".".to_string()
        } else {
            format!("./{}", relative.replace('\\', "/"))
        }
    } else {
        target_str
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, ProjectType, Service, Target};

    fn make_project(dir: &Path) -> Project {
        Project {
            name: "test-app".to_string(),
            description: String::new(),
            path: dir.to_string_lossy().to_string(),
            project_type: Some(ProjectType::Node),
            tags: vec![],
            services: vec![Service {
                name: "backend".to_string(),
                command: "npm run dev --port 8000".to_string(),
                target: Target::Windows,
                working_dir: Some(dir.join("backend").to_string_lossy().to_string()),
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        }
    }

    #[test]
    fn test_extract_port_from_command() {
        assert_eq!(
            extract_port_from_command("uvicorn main:app --port 8000"),
            Some(8000)
        );
        assert_eq!(extract_port_from_command("npm run dev -p 3000"), Some(3000));
        assert_eq!(extract_port_from_command("node server.js"), None);
    }

    #[test]
    fn test_detect_infra_from_code_postgres() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        std::fs::write(
            backend.join("requirements.txt"),
            "fastapi\nsqlalchemy\npsycopg2\n",
        )
        .unwrap();

        let mut infra = HashSet::new();
        detect_infra_from_code(&backend, &mut infra);
        assert!(infra.contains(&InfraService::Postgres));
    }

    #[test]
    fn test_detect_infra_from_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            format!(
                "DATABASE_URL=postgres://user:{}@localhost/db\nREDIS_URL=redis://localhost:6379\n",
                "pass"
            ),
        )
        .unwrap();

        let mut infra = HashSet::new();
        detect_infra_from_env(dir.path(), &mut infra);
        assert!(infra.contains(&InfraService::Postgres));
        assert!(infra.contains(&InfraService::Redis));
    }

    #[test]
    fn test_generate_compose() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        std::fs::write(
            backend.join("package.json"),
            r#"{"dependencies":{"express":"^4","mongoose":"^7"}}"#,
        )
        .unwrap();

        let project = make_project(dir.path());
        let result = generate(&project, dir.path());

        assert!(result.contains("services:"));
        assert!(result.contains("backend:"));
        assert!(result.contains("8000:8000"));
        assert!(result.contains("mongo:"));
        assert!(result.contains("void-stack-net"));
    }

    #[test]
    fn test_make_relative() {
        assert_eq!(
            make_relative(Path::new("/app"), Path::new("/app/backend")),
            "./backend"
        );
        assert_eq!(make_relative(Path::new("/app"), Path::new("/app")), ".");
    }
}
