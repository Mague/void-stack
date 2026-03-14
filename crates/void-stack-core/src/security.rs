//! Security utilities — prevent exposure of sensitive files and credentials.

use std::path::Path;

/// File extensions that are always considered sensitive.
const SENSITIVE_EXTENSIONS: &[&str] = &[
    "pem", "key", "p12", "pfx", "jks", "keystore", "crt", "cer", "der", "p7b",
];

/// File names (exact match) that are sensitive.
const SENSITIVE_FILENAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.staging",
    ".env.development",
    ".env.test",
    "credentials.json",
    "service-account.json",
    "secrets.yaml",
    "secrets.yml",
    "secrets.json",
    "secrets.toml",
    ".npmrc",
    ".pypirc",
    ".netrc",
    ".htpasswd",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
    "known_hosts",
    "authorized_keys",
    ".pgpass",
    "token.json",
];

/// Check if a file path refers to a sensitive/credential file.
pub fn is_sensitive_file(path: &Path) -> bool {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Exact filename match
    if SENSITIVE_FILENAMES.contains(&filename.as_str()) {
        return true;
    }

    // Extension match
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && SENSITIVE_EXTENSIONS.contains(&ext)
    {
        return true;
    }

    false
}

/// Read a .env-style file and return only the **key names** (no values).
///
/// This is the safe way to inspect .env files without exposing secrets.
pub fn read_env_keys(path: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
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

/// Check if any key name in a .env file matches a keyword (case-insensitive).
pub fn env_keys_contain(path: &Path, keyword: &str) -> bool {
    let keys = read_env_keys(path);
    let kw_upper = keyword.to_uppercase();
    keys.iter().any(|k| k.to_uppercase().contains(&kw_upper))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_is_sensitive() {
        assert!(is_sensitive_file(Path::new(".env")));
        assert!(is_sensitive_file(Path::new("/project/.env")));
        assert!(is_sensitive_file(Path::new("secrets.json")));
        assert!(is_sensitive_file(Path::new("server.pem")));
        assert!(is_sensitive_file(Path::new("id_rsa")));
        assert!(!is_sensitive_file(Path::new("main.py")));
        assert!(!is_sensitive_file(Path::new("package.json")));
        assert!(!is_sensitive_file(Path::new(".env.example")));
    }

    #[test]
    fn test_read_env_keys() {
        let dir = tempdir().unwrap();
        let env = dir.path().join(".env");
        std::fs::write(
            &env,
            format!(
                "API_KEY=super_secret_123\n# comment\nDB_URL=postgres://user:{}@host/db\n",
                "pass"
            ),
        )
        .unwrap();
        let keys = read_env_keys(&env);
        assert_eq!(keys, vec!["API_KEY", "DB_URL"]);
    }

    #[test]
    fn test_env_keys_contain() {
        let dir = tempdir().unwrap();
        let env = dir.path().join(".env");
        std::fs::write(
            &env,
            "POSTGRES_URL=postgres://...\nREDIS_HOST=localhost\nOLLAMA_BASE=http://...\n",
        )
        .unwrap();
        assert!(env_keys_contain(&env, "postgres"));
        assert!(env_keys_contain(&env, "REDIS"));
        assert!(env_keys_contain(&env, "ollama"));
        assert!(!env_keys_contain(&env, "mongo"));
    }
}
