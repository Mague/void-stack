//! Parse Dockerfile and docker-compose.yml into structured types.

use serde_yaml::Value;
use std::path::{Path, PathBuf};

use super::*;

/// Find the compose file in a directory (checks common names).
pub fn find_compose_file(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ];
    for name in &candidates {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Parse a Dockerfile into structured metadata.
pub fn parse_dockerfile(path: &Path) -> Option<DockerfileInfo> {
    let content = std::fs::read_to_string(path).ok()?;

    let mut stages = Vec::new();
    let mut exposed_ports = Vec::new();
    let mut entrypoint = None;
    let mut cmd = None;
    let mut env_vars = Vec::new();
    let mut workdir = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let upper = trimmed.to_uppercase();

        if upper.starts_with("FROM ") {
            let rest = trimmed[5..].trim();
            // FROM image:tag AS name
            let parts: Vec<&str> = rest.splitn(3, char::is_whitespace).collect();
            let base_image = parts[0].to_string();
            let name = if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("AS") {
                Some(parts[2].to_string())
            } else {
                None
            };
            stages.push(DockerStage { name, base_image });
        } else if upper.starts_with("EXPOSE ") {
            let rest = trimmed[7..].trim();
            for tok in rest.split_whitespace() {
                // EXPOSE 8080/tcp → 8080
                let port_str = tok.split('/').next().unwrap_or(tok);
                if let Ok(port) = port_str.parse::<u16>() {
                    exposed_ports.push(port);
                }
            }
        } else if upper.starts_with("ENTRYPOINT ") {
            let rest = trimmed[11..].trim();
            entrypoint = Some(parse_cmd_value(rest));
        } else if upper.starts_with("CMD ") {
            let rest = trimmed[4..].trim();
            cmd = Some(parse_cmd_value(rest));
        } else if upper.starts_with("ENV ") {
            let rest = trimmed[4..].trim();
            if let Some((k, v)) = rest.split_once('=') {
                env_vars.push((k.trim().to_string(), v.trim().trim_matches('"').to_string()));
            } else if let Some((k, v)) = rest.split_once(' ') {
                env_vars.push((k.trim().to_string(), v.trim().to_string()));
            }
        } else if upper.starts_with("WORKDIR ") {
            workdir = Some(trimmed[8..].trim().to_string());
        }
    }

    if stages.is_empty() {
        return None;
    }

    Some(DockerfileInfo {
        stages,
        exposed_ports,
        entrypoint,
        cmd,
        env_vars,
        workdir,
    })
}

/// Parse CMD/ENTRYPOINT value — handles both exec form ["a","b"] and shell form.
fn parse_cmd_value(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with('[') {
        // Exec form: ["uvicorn", "main:app"] → "uvicorn main:app"
        trimmed
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|p| p.trim().trim_matches('"').trim_matches('\''))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        trimmed.to_string()
    }
}

/// Parse a docker-compose.yml file.
pub fn parse_compose(path: &Path) -> Option<ComposeProject> {
    let content = std::fs::read_to_string(path).ok()?;
    let doc: Value = serde_yaml::from_str(&content).ok()?;

    let services_map = doc.get("services")?.as_mapping()?;

    let mut services = Vec::new();
    for (key, val) in services_map {
        let name = key.as_str()?.to_string();
        let svc_map = val.as_mapping()?;

        let image = val
            .get("image")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let build = parse_compose_build(val.get("build"));
        let ports = parse_compose_ports(val.get("ports"));
        let volumes = parse_compose_volumes(val.get("volumes"));
        let env_vars = parse_compose_env(val.get("environment"));
        let depends_on = parse_compose_depends_on(val.get("depends_on"));
        let healthcheck = parse_compose_healthcheck(val.get("healthcheck"));

        let kind = classify_service_kind(&name, image.as_deref().unwrap_or(""), svc_map);

        services.push(ComposeService {
            name,
            image,
            build,
            ports,
            volumes,
            env_vars,
            depends_on,
            healthcheck,
            kind,
        });
    }

    // Parse top-level networks
    let networks = doc
        .get("networks")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.keys()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Parse top-level volumes
    let volumes = doc
        .get("volumes")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.keys()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Some(ComposeProject {
        services,
        networks,
        volumes,
    })
}

fn parse_compose_build(val: Option<&Value>) -> Option<ComposeBuild> {
    let val = val?;
    match val {
        Value::String(s) => Some(ComposeBuild {
            context: s.clone(),
            dockerfile: None,
            target: None,
        }),
        Value::Mapping(m) => {
            let context = m
                .get(Value::String("context".into()))
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            let dockerfile = m
                .get(Value::String("dockerfile".into()))
                .and_then(|v| v.as_str())
                .map(String::from);
            let target = m
                .get(Value::String("target".into()))
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(ComposeBuild {
                context,
                dockerfile,
                target,
            })
        }
        _ => None,
    }
}

fn parse_compose_ports(val: Option<&Value>) -> Vec<PortMapping> {
    let val = match val {
        Some(Value::Sequence(seq)) => seq,
        _ => return Vec::new(),
    };

    let mut ports = Vec::new();
    for item in val {
        let s = match item {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => continue,
        };
        // "8080:80", "5432:5432", "3000"
        if let Some((host_str, container_str)) = s.split_once(':') {
            // Handle "0.0.0.0:8080:80" → take last two parts
            let parts: Vec<&str> = s.split(':').collect();
            let (h, c) = if parts.len() >= 3 {
                (parts[parts.len() - 2], parts[parts.len() - 1])
            } else {
                (host_str, container_str)
            };
            if let (Ok(host), Ok(container)) = (h.parse::<u16>(), c.parse::<u16>()) {
                ports.push(PortMapping { host, container });
            }
        } else if let Ok(port) = s.parse::<u16>() {
            ports.push(PortMapping {
                host: port,
                container: port,
            });
        }
    }
    ports
}

fn parse_compose_volumes(val: Option<&Value>) -> Vec<VolumeMount> {
    let val = match val {
        Some(Value::Sequence(seq)) => seq,
        _ => return Vec::new(),
    };

    let mut vols = Vec::new();
    for item in val {
        match item {
            Value::String(s) => {
                // "./data:/var/lib/postgres" or "pgdata:/var/lib/postgres"
                if let Some((src, tgt)) = s.split_once(':') {
                    let named =
                        !src.starts_with('.') && !src.starts_with('/') && !src.contains('\\');
                    vols.push(VolumeMount {
                        source: src.to_string(),
                        target: tgt.to_string(),
                        named,
                    });
                }
            }
            Value::Mapping(m) => {
                let source = m
                    .get(Value::String("source".into()))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let target = m
                    .get(Value::String("target".into()))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let named = !source.starts_with('.') && !source.starts_with('/');
                vols.push(VolumeMount {
                    source,
                    target,
                    named,
                });
            }
            _ => {}
        }
    }
    vols
}

fn parse_compose_env(val: Option<&Value>) -> Vec<(String, String)> {
    let val = match val {
        Some(v) => v,
        None => return Vec::new(),
    };

    match val {
        Value::Sequence(seq) => {
            // ["KEY=value", "KEY2=value2"]
            seq.iter()
                .filter_map(|item| {
                    let s = item.as_str()?;
                    let (k, v) = s.split_once('=')?;
                    Some((k.to_string(), v.to_string()))
                })
                .collect()
        }
        Value::Mapping(m) => {
            // { KEY: value, KEY2: value2 }
            m.iter()
                .filter_map(|(k, v)| {
                    let key = k.as_str()?.to_string();
                    let val = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        Value::Null => String::new(),
                        _ => return None,
                    };
                    Some((key, val))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn parse_compose_depends_on(val: Option<&Value>) -> Vec<String> {
    let val = match val {
        Some(v) => v,
        None => return Vec::new(),
    };

    match val {
        Value::Sequence(seq) => seq
            .iter()
            .filter_map(|item| item.as_str().map(String::from))
            .collect(),
        Value::Mapping(m) => {
            // depends_on: { db: { condition: service_healthy } }
            m.keys()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        }
        _ => Vec::new(),
    }
}

fn parse_compose_healthcheck(val: Option<&Value>) -> Option<HealthCheck> {
    let val = val?.as_mapping()?;

    let test = val.get(Value::String("test".into())).and_then(|v| {
        match v {
            Value::String(s) => Some(s.clone()),
            Value::Sequence(seq) => {
                // ["CMD", "pg_isready"] or ["CMD-SHELL", "curl -f ..."]
                let parts: Vec<&str> = seq.iter().filter_map(|i| i.as_str()).collect();
                if parts.first().map(|s| s.starts_with("CMD")).unwrap_or(false) {
                    Some(parts[1..].join(" "))
                } else {
                    Some(parts.join(" "))
                }
            }
            _ => None,
        }
    })?;

    let interval = val
        .get(Value::String("interval".into()))
        .and_then(|v| v.as_str())
        .map(String::from);
    let timeout = val
        .get(Value::String("timeout".into()))
        .and_then(|v| v.as_str())
        .map(String::from);
    let retries = val
        .get(Value::String("retries".into()))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    Some(HealthCheck {
        test,
        interval,
        timeout,
        retries,
    })
}

/// Classify a compose service by name and image.
fn classify_service_kind(
    name: &str,
    image: &str,
    _svc: &serde_yaml::Mapping,
) -> ComposeServiceKind {
    let name_lower = name.to_lowercase();
    let image_lower = image.to_lowercase();

    let combined = format!("{} {}", name_lower, image_lower);

    // Databases
    if matches_any(
        &combined,
        &[
            "postgres",
            "mysql",
            "mariadb",
            "mongo",
            "sqlite",
            "cockroach",
            "timescale",
            "cassandra",
            "dynamodb",
            "supabase",
            "mssql",
            "sqlserver",
        ],
    ) {
        return ComposeServiceKind::Database;
    }

    // Caches
    if matches_any(
        &combined,
        &["redis", "memcache", "valkey", "dragonfly", "keydb"],
    ) {
        return ComposeServiceKind::Cache;
    }

    // Queues
    if matches_any(
        &combined,
        &[
            "rabbit", "kafka", "nats", "pulsar", "celery", "bullmq", "sqs",
        ],
    ) {
        return ComposeServiceKind::Queue;
    }

    // Proxies
    if matches_any(
        &combined,
        &[
            "nginx", "traefik", "caddy", "haproxy", "envoy", "kong", "gateway",
        ],
    ) {
        return ComposeServiceKind::Proxy;
    }

    // Workers
    if matches_any(&combined, &["worker", "cron", "scheduler", "job"]) {
        return ComposeServiceKind::Worker;
    }

    // If it has a build context, it's likely an app service
    if _svc.get(Value::String("build".into())).is_some() {
        return ComposeServiceKind::App;
    }

    ComposeServiceKind::Unknown
}

fn matches_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_dockerfile_multistage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Dockerfile");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"FROM rust:1.77 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/myapp .
EXPOSE 8080
ENV APP_ENV=production
CMD ["./myapp"]
"#
        )
        .unwrap();

        let info = parse_dockerfile(&path).unwrap();
        assert_eq!(info.stages.len(), 2);
        assert_eq!(info.stages[0].base_image, "rust:1.77");
        assert_eq!(info.stages[0].name.as_deref(), Some("builder"));
        assert_eq!(info.stages[1].base_image, "debian:bookworm-slim");
        assert_eq!(info.stages[1].name, None);
        assert_eq!(info.exposed_ports, vec![8080]);
        assert_eq!(info.cmd.as_deref(), Some("./myapp"));
        assert_eq!(
            info.env_vars,
            vec![("APP_ENV".to_string(), "production".to_string())]
        );
        assert_eq!(info.workdir.as_deref(), Some("/app"));
    }

    #[test]
    fn test_parse_compose() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"version: "3.8"
services:
  api:
    build: ./backend
    ports:
      - "8000:8000"
    depends_on:
      - db
      - redis
    environment:
      - DATABASE_URL=postgres://localhost/mydb
  db:
    image: postgres:16
    ports:
      - "5432:5432"
    volumes:
      - pgdata:/var/lib/postgresql/data
    environment:
      POSTGRES_DB: mydb
      POSTGRES_PASSWORD: secret
    healthcheck:
      test: ["CMD", "pg_isready"]
      interval: 10s
      timeout: 5s
      retries: 3
  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"

volumes:
  pgdata:

networks:
  default:
"#
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        assert_eq!(project.services.len(), 3);

        let api = &project.services[0];
        assert_eq!(api.name, "api");
        assert_eq!(api.kind, ComposeServiceKind::App);
        assert_eq!(api.ports.len(), 1);
        assert_eq!(api.ports[0].host, 8000);
        assert_eq!(api.depends_on, vec!["db", "redis"]);

        let db = &project.services[1];
        assert_eq!(db.name, "db");
        assert_eq!(db.kind, ComposeServiceKind::Database);
        assert_eq!(db.image.as_deref(), Some("postgres:16"));
        assert!(db.healthcheck.is_some());
        assert_eq!(db.healthcheck.as_ref().unwrap().test, "pg_isready");
        assert_eq!(db.volumes.len(), 1);
        assert!(db.volumes[0].named);

        let redis = &project.services[2];
        assert_eq!(redis.name, "redis");
        assert_eq!(redis.kind, ComposeServiceKind::Cache);

        assert_eq!(project.volumes, vec!["pgdata"]);
        assert_eq!(project.networks, vec!["default"]);
    }

    #[test]
    fn test_find_compose_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_compose_file(dir.path()).is_none());

        std::fs::write(dir.path().join("compose.yaml"), "services: {}").unwrap();
        assert!(find_compose_file(dir.path()).is_some());
    }

    #[test]
    fn test_classify_service_kinds() {
        let empty = serde_yaml::Mapping::new();
        assert_eq!(
            classify_service_kind("postgres", "postgres:16", &empty),
            ComposeServiceKind::Database
        );
        assert_eq!(
            classify_service_kind("cache", "redis:7", &empty),
            ComposeServiceKind::Cache
        );
        assert_eq!(
            classify_service_kind("broker", "rabbitmq:3", &empty),
            ComposeServiceKind::Queue
        );
        assert_eq!(
            classify_service_kind("proxy", "nginx:latest", &empty),
            ComposeServiceKind::Proxy
        );
        assert_eq!(
            classify_service_kind("bg-worker", "", &empty),
            ComposeServiceKind::Worker
        );
    }

    #[test]
    fn test_parse_dockerfile_simple() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Dockerfile");
        std::fs::write(
            &path,
            "FROM node:20-alpine\nEXPOSE 3000 5173\nENTRYPOINT [\"node\", \"server.js\"]\n",
        )
        .unwrap();

        let info = parse_dockerfile(&path).unwrap();
        assert_eq!(info.stages.len(), 1);
        assert_eq!(info.exposed_ports, vec![3000, 5173]);
        assert_eq!(info.entrypoint.as_deref(), Some("node server.js"));
    }

    #[test]
    fn test_parse_cmd_value_exec_form() {
        assert_eq!(
            parse_cmd_value(r#"["uvicorn", "main:app", "--host", "0.0.0.0"]"#),
            "uvicorn main:app --host 0.0.0.0"
        );
    }

    #[test]
    fn test_parse_cmd_value_shell_form() {
        assert_eq!(
            parse_cmd_value("python manage.py runserver"),
            "python manage.py runserver"
        );
    }

    #[test]
    fn test_parse_dockerfile_env_space_separator() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Dockerfile");
        std::fs::write(&path, "FROM alpine\nENV MY_VAR my_value\n").unwrap();

        let info = parse_dockerfile(&path).unwrap();
        assert_eq!(
            info.env_vars,
            vec![("MY_VAR".to_string(), "my_value".to_string())]
        );
    }

    #[test]
    fn test_parse_dockerfile_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Dockerfile");
        std::fs::write(&path, "# just a comment\n").unwrap();

        assert!(parse_dockerfile(&path).is_none());
    }

    #[test]
    fn test_parse_dockerfile_expose_with_protocol() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Dockerfile");
        std::fs::write(&path, "FROM node:20\nEXPOSE 8080/tcp 9090/udp\n").unwrap();

        let info = parse_dockerfile(&path).unwrap();
        assert_eq!(info.exposed_ports, vec![8080, 9090]);
    }

    #[test]
    fn test_parse_dockerfile_nonexistent() {
        assert!(parse_dockerfile(Path::new("/nonexistent/Dockerfile")).is_none());
    }

    #[test]
    fn test_parse_compose_env_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  app:
    image: node:20
    environment:
      NODE_ENV: production
      PORT: 3000
      DEBUG: true
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        let env = &project.services[0].env_vars;
        assert!(
            env.iter()
                .any(|(k, v)| k == "NODE_ENV" && v == "production")
        );
        assert!(env.iter().any(|(k, v)| k == "PORT" && v == "3000"));
        assert!(env.iter().any(|(k, v)| k == "DEBUG" && v == "true"));
    }

    #[test]
    fn test_parse_compose_build_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  app:
    build:
      context: ./backend
      dockerfile: Dockerfile.prod
      target: production
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        let build = project.services[0].build.as_ref().unwrap();
        assert_eq!(build.context, "./backend");
        assert_eq!(build.dockerfile.as_deref(), Some("Dockerfile.prod"));
        assert_eq!(build.target.as_deref(), Some("production"));
    }

    #[test]
    fn test_parse_compose_depends_on_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  app:
    image: node:20
    depends_on:
      db:
        condition: service_healthy
      redis:
        condition: service_started
  db:
    image: postgres:16
  redis:
    image: redis:7
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        let deps = &project.services[0].depends_on;
        assert!(deps.contains(&"db".to_string()));
        assert!(deps.contains(&"redis".to_string()));
    }

    #[test]
    fn test_parse_compose_volumes_mapping_form() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  db:
    image: postgres:16
    volumes:
      - source: pgdata
        target: /var/lib/postgresql/data
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        let vols = &project.services[0].volumes;
        assert_eq!(vols.len(), 1);
        assert_eq!(vols[0].source, "pgdata");
        assert_eq!(vols[0].target, "/var/lib/postgresql/data");
        assert!(vols[0].named);
    }

    #[test]
    fn test_parse_compose_volumes_bind_mount() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  app:
    image: node:20
    volumes:
      - "./src:/app/src"
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        let vols = &project.services[0].volumes;
        assert_eq!(vols.len(), 1);
        assert!(!vols[0].named);
    }

    #[test]
    fn test_parse_compose_healthcheck_string() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  db:
    image: postgres:16
    healthcheck:
      test: pg_isready -U postgres
      interval: 30s
      timeout: 10s
      retries: 5
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        let hc = project.services[0].healthcheck.as_ref().unwrap();
        assert_eq!(hc.test, "pg_isready -U postgres");
        assert_eq!(hc.interval.as_deref(), Some("30s"));
        assert_eq!(hc.timeout.as_deref(), Some("10s"));
        assert_eq!(hc.retries, Some(5));
    }

    #[test]
    fn test_parse_compose_single_port() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  app:
    image: node:20
    ports:
      - 3000
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        assert_eq!(project.services[0].ports[0].host, 3000);
        assert_eq!(project.services[0].ports[0].container, 3000);
    }

    #[test]
    fn test_classify_service_kind_unknown() {
        let empty = serde_yaml::Mapping::new();
        assert_eq!(
            classify_service_kind("myapp", "myimage:latest", &empty),
            ComposeServiceKind::Unknown
        );
    }

    #[test]
    fn test_parse_compose_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(&path, "not: valid: yaml: {{{}}}").unwrap();

        assert!(parse_compose(&path).is_none());
    }

    #[test]
    fn test_parse_compose_ports_with_host() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &path,
            r#"services:
  web:
    image: nginx
    ports:
      - "0.0.0.0:8080:80"
"#,
        )
        .unwrap();

        let project = parse_compose(&path).unwrap();
        assert_eq!(project.services[0].ports[0].host, 8080);
        assert_eq!(project.services[0].ports[0].container, 80);
    }
}
