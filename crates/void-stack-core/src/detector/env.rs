use std::collections::HashSet;
use std::path::Path;

use async_trait::async_trait;

use super::{CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct EnvDetector;

#[async_trait]
impl DependencyDetector for EnvDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Env
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join(".env.example").exists() || project_path.join(".env.sample").exists()
    }

    async fn check(&self, project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Env);

        // Find the example file
        let example_path = if project_path.join(".env.example").exists() {
            project_path.join(".env.example")
        } else {
            project_path.join(".env.sample")
        };

        let example_vars = parse_env_keys(&example_path);
        if example_vars.is_empty() {
            status.details.push("No variables in example file".into());
            return status;
        }

        let env_path = project_path.join(".env");
        if !env_path.exists() {
            status.status = CheckStatus::NeedsSetup;
            let example_name = example_path.file_name().unwrap().to_string_lossy();
            status.details.push(format!(
                ".env not found ({} has {} variables)",
                example_name,
                example_vars.len()
            ));
            status.fix_hint = Some(format!("cp {} .env  # then edit the values", example_name));
            return status;
        }

        let env_vars = parse_env_keys(&env_path);
        let missing: Vec<&String> = example_vars
            .iter()
            .filter(|k| !env_vars.contains(*k))
            .collect();

        if missing.is_empty() {
            status.details.push(format!(
                ".env has all {} variables from example",
                example_vars.len()
            ));
        } else {
            status.status = CheckStatus::NeedsSetup;
            status.details.push(format!(
                "Missing {} variable(s): {}",
                missing.len(),
                missing
                    .iter()
                    .take(5)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            if missing.len() > 5 {
                status
                    .details
                    .push(format!("  ... and {} more", missing.len() - 5));
            }
            status.fix_hint = Some("Edit .env and add the missing variables".into());
        }

        status
    }
}

/// Parse a .env file and return the set of variable names (keys).
fn parse_env_keys(path: &Path) -> HashSet<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return HashSet::new(),
    };

    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            trimmed.split('=').next().map(|k| k.trim().to_string())
        })
        .filter(|k| !k.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_env_keys() {
        let dir = tempdir().unwrap();
        let env_file = dir.path().join(".env");
        std::fs::write(
            &env_file,
            "API_KEY=abc123\n# comment\nDB_URL=postgres://\nEMPTY=\n",
        )
        .unwrap();

        let keys = parse_env_keys(&env_file);
        assert!(keys.contains("API_KEY"));
        assert!(keys.contains("DB_URL"));
        assert!(keys.contains("EMPTY"));
        assert_eq!(keys.len(), 3);
    }

    #[tokio::test]
    async fn test_env_missing_dotenv() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".env.example"), "API_KEY=\nSECRET=\n").unwrap();

        let detector = EnvDetector;
        assert!(detector.is_relevant(dir.path()));

        let result = detector.check(dir.path()).await;
        assert!(matches!(result.status, CheckStatus::NeedsSetup));
        assert!(result.fix_hint.unwrap().contains("cp"));
    }

    #[tokio::test]
    async fn test_env_complete() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".env.example"), "API_KEY=\nSECRET=\n").unwrap();
        std::fs::write(dir.path().join(".env"), "API_KEY=abc\nSECRET=xyz\n").unwrap();

        let detector = EnvDetector;
        let result = detector.check(dir.path()).await;
        assert!(matches!(result.status, CheckStatus::Ok));
    }

    #[tokio::test]
    async fn test_env_partial() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env.example"),
            "API_KEY=\nSECRET=\nDB_URL=\n",
        )
        .unwrap();
        std::fs::write(dir.path().join(".env"), "API_KEY=abc\n").unwrap();

        let detector = EnvDetector;
        let result = detector.check(dir.path()).await;
        assert!(matches!(result.status, CheckStatus::NeedsSetup));
        assert!(result.details.iter().any(|d| d.contains("Missing 2")));
    }
}
