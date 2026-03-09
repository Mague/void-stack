//! Detection of hardcoded secrets in source code.

use std::path::Path;
use regex::Regex;

use super::findings::{FindingCategory, SecurityFinding, Severity};

/// Patterns that indicate hardcoded secrets.
struct SecretPattern {
    name: &'static str,
    regex: &'static str,
    severity: Severity,
    remediation: &'static str,
}

const SECRET_PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        name: "AWS Access Key",
        regex: r#"(?i)(AKIA[0-9A-Z]{16})"#,
        severity: Severity::Critical,
        remediation: "Rotar la clave AWS y moverla a variables de entorno o AWS Secrets Manager",
    },
    SecretPattern {
        name: "AWS Secret Key",
        regex: r#"(?i)aws[_\-]?secret[_\-]?access[_\-]?key\s*[=:]\s*["']?([A-Za-z0-9/+=]{40})"#,
        severity: Severity::Critical,
        remediation: "Rotar la clave AWS y moverla a variables de entorno",
    },
    SecretPattern {
        name: "Generic API Key",
        regex: r#"(?i)(api[_\-]?key|apikey)\s*[=:]\s*["']([a-zA-Z0-9_\-]{20,})["']"#,
        severity: Severity::High,
        remediation: "Mover la API key a variables de entorno o un gestor de secretos",
    },
    SecretPattern {
        name: "Generic Secret/Token",
        regex: r#"(?i)(secret|token|password|passwd|pwd)\s*[=:]\s*["']([^"'\s]{8,})["']"#,
        severity: Severity::High,
        remediation: "Mover el secret/token a variables de entorno",
    },
    SecretPattern {
        name: "Private Key",
        regex: r#"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----"#,
        severity: Severity::Critical,
        remediation: "Eliminar la clave privada del código y usar un gestor de secretos",
    },
    SecretPattern {
        name: "JWT Secret hardcoded",
        regex: r#"(?i)(jwt[_\-]?secret|jwt[_\-]?key)\s*[=:]\s*["']([^"'\s]{8,})["']"#,
        severity: Severity::Critical,
        remediation: "Mover el JWT secret a variables de entorno",
    },
    SecretPattern {
        name: "Database URL con credenciales",
        regex: r#"(?i)(postgres|mysql|mongodb|redis)://[^:]+:[^@]+@[^\s"']+"#,
        severity: Severity::High,
        remediation: "Mover la URL de conexión a variables de entorno (DATABASE_URL)",
    },
    SecretPattern {
        name: "GitHub Token",
        regex: r#"(ghp_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9_]{82})"#,
        severity: Severity::Critical,
        remediation: "Revocar el token y usar variables de entorno o GitHub Secrets",
    },
    SecretPattern {
        name: "Slack Token",
        regex: r#"(xox[bprs]-[a-zA-Z0-9\-]{10,})"#,
        severity: Severity::High,
        remediation: "Revocar el token de Slack y usar variables de entorno",
    },
    SecretPattern {
        name: "Google API Key",
        regex: r#"AIza[0-9A-Za-z\-_]{35}"#,
        severity: Severity::High,
        remediation: "Restringir la API key en Google Cloud Console y moverla a env vars",
    },
    SecretPattern {
        name: "Stripe Key",
        regex: r#"(sk_live_[a-zA-Z0-9]{24,}|rk_live_[a-zA-Z0-9]{24,})"#,
        severity: Severity::Critical,
        remediation: "Revocar la clave de Stripe y moverla a variables de entorno",
    },
    SecretPattern {
        name: "SendGrid API Key",
        regex: r#"SG\.[a-zA-Z0-9_\-]{22}\.[a-zA-Z0-9_\-]{43}"#,
        severity: Severity::High,
        remediation: "Revocar la API key de SendGrid y usar variables de entorno",
    },
];

/// File extensions to scan for secrets.
const SCANNABLE_EXTENSIONS: &[&str] = &[
    "py", "js", "ts", "jsx", "tsx", "rs", "go", "java", "rb", "php",
    "yml", "yaml", "json", "toml", "ini", "cfg", "conf", "xml",
    "sh", "bash", "zsh", "ps1", "bat", "cmd",
    "env.example", "env.sample", "env.template",
    "dart", "kt", "swift", "cs", "c", "cpp", "h",
    "dockerfile", "docker-compose.yml",
];

/// Directories to skip when scanning.
const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", "dist", "build", "__pycache__",
    ".venv", "venv", ".next", ".nuxt", "vendor", ".dart_tool",
    ".gradle", ".idea", ".vscode", "coverage", ".tox",
];

/// Files that are part of the security/audit detection system itself.
/// Matches against the relative path (forward-slash normalized).
const SELF_REFERENCING_FILES: &[&str] = &[
    "audit/secrets.rs",
    "audit/mod.rs",
    "audit/vuln_patterns.rs",
    "audit/config_check.rs",
    "security.rs",
    "docker/generate_compose.rs",
];

/// Returns true if the line looks like a regex pattern definition rather than
/// an actual hardcoded secret. Checks for common regex metacharacters.
fn is_regex_pattern_line(line: &str) -> bool {
    let trimmed = line.trim();
    // Lines containing regex metacharacters typical of pattern definitions
    let regex_indicators = [r"\b", r"\w+", r"\s*", r"\d+", r"[A-Z", r"[a-z", r"[0-9", r"(?i)", r"(?:", r"\-]"];
    let indicator_count = regex_indicators.iter().filter(|ind| trimmed.contains(*ind)).count();
    // Need at least 2 regex indicators to be confident it's a pattern definition
    indicator_count >= 2
}

/// Returns true if the line contains template/placeholder syntax that
/// indicates it's generating content rather than containing real secrets.
fn is_template_line(line: &str) -> bool {
    let trimmed = line.trim();
    // Rust format strings with placeholders: format!("...{}...", var)
    // Docker compose template variables, string interpolation
    if (trimmed.contains("format!(") || trimmed.contains("format_args!("))
        && trimmed.contains("{}")
    {
        return true;
    }
    // Lines that are building/concatenating strings with variables (template generation)
    if trimmed.contains("push_str") && (trimmed.contains("{}") || trimmed.contains("{")) {
        return true;
    }
    false
}

/// Scan project files for hardcoded secrets.
pub fn scan_secrets(project_path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let compiled: Vec<(&SecretPattern, Regex)> = SECRET_PATTERNS
        .iter()
        .filter_map(|p| Regex::new(p.regex).ok().map(|r| (p, r)))
        .collect();

    scan_dir_recursive(project_path, project_path, &compiled, &mut findings, 0);
    findings
}

fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    patterns: &[(&SecretPattern, Regex)],
    findings: &mut Vec<SecurityFinding>,
    depth: u32,
) {
    if depth > 6 {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if SKIP_DIRS.iter().any(|s| name_str.eq_ignore_ascii_case(s)) {
                continue;
            }
            scan_dir_recursive(root, &path, patterns, findings, depth + 1);
            continue;
        }

        // Check extension
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let filename_lower = name_str.to_lowercase();

        let should_scan = SCANNABLE_EXTENSIONS.iter().any(|e| ext == *e)
            || filename_lower == "dockerfile"
            || filename_lower.starts_with("docker-compose")
            || filename_lower.ends_with(".env.example");

        if !should_scan {
            continue;
        }

        // Skip actual .env files (those contain real secrets, handled by security.rs)
        if filename_lower == ".env"
            || filename_lower == ".env.local"
            || filename_lower == ".env.production"
        {
            continue;
        }

        // Read file (limit to 1MB)
        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > 1_048_576 {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        // Normalize path separators for matching
        let rel_path_normalized = rel_path.replace('\\', "/");

        // Skip files that are part of the security detection system itself
        let is_self_referencing = SELF_REFERENCING_FILES
            .iter()
            .any(|f| rel_path_normalized.ends_with(f));
        if is_self_referencing {
            continue;
        }

        for (line_num, line) in content.lines().enumerate() {
            // Skip comments
            let trimmed = line.trim();
            if trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
            {
                // Still check — secrets in comments are still leaked
                // but reduce severity
            }

            // Skip test/example/mock files
            let is_test_file = rel_path.contains("test")
                || rel_path.contains("spec")
                || rel_path.contains("mock")
                || rel_path.contains("fixture")
                || rel_path.contains("example")
                || rel_path.contains("audit")
                || content.contains("#[cfg(test)]")
                || content.contains("#[test]");

            for (pattern, regex) in patterns {
                if regex.is_match(line) {
                    // Skip false positives: env var references like process.env.X or os.environ
                    if line.contains("process.env")
                        || line.contains("os.environ")
                        || line.contains("os.getenv")
                        || line.contains("env::var")
                        || line.contains("std::env")
                        || line.contains("os.Getenv")
                        || line.contains("${")
                        || line.contains("$ENV")
                    {
                        continue;
                    }

                    // Skip placeholder values
                    let lower = line.to_lowercase();
                    if lower.contains("xxx")
                        || lower.contains("placeholder")
                        || lower.contains("your_")
                        || lower.contains("change_me")
                        || lower.contains("todo")
                        || lower.contains("<your")
                        || lower.contains("example")
                    {
                        continue;
                    }

                    // Skip lines that are regex pattern definitions
                    if is_regex_pattern_line(line) {
                        continue;
                    }

                    // Skip lines that are template/format string generation
                    if is_template_line(line) {
                        continue;
                    }

                    // Skip lines in Rust that are defining string literals for
                    // pattern matching or const definitions containing regex-like content
                    if ext == "rs" {
                        let trimmed = line.trim();
                        if trimmed.starts_with("regex:")
                            || trimmed.starts_with("r#\"")
                            || trimmed.starts_with("r\"")
                            || (trimmed.contains("Regex::new") && trimmed.contains("r#\""))
                            || trimmed.starts_with("name:")
                        {
                            continue;
                        }
                    }

                    // Skip lines in TSX/JSX/TS/JS that are UI display strings,
                    // object key mappings, or i18n translation keys
                    if matches!(ext.as_str(), "tsx" | "jsx" | "ts" | "js") {
                        let trimmed = line.trim();
                        // JSX elements and component props
                        if trimmed.contains("</")
                            || trimmed.contains("/>")
                            || trimmed.starts_with('<')
                            || trimmed.contains("className")
                        {
                            continue;
                        }
                        // Object literals mapping identifiers (e.g., `Key: 'value',`)
                        // where the value is a simple camelCase/PascalCase identifier
                        // (not a real secret)
                        if let Some(val_start) = trimmed.find(": '").or_else(|| trimmed.find(": \"")) {
                            let after = &trimmed[val_start + 3..];
                            let val_end = after.find('\'').or_else(|| after.find('"')).unwrap_or(0);
                            if val_end > 0 {
                                let val = &after[..val_end];
                                // Pure alphanumeric camelCase identifiers are not secrets
                                if val.chars().all(|c| c.is_alphanumeric() || c == '_')
                                    && val.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)
                                    && val.chars().any(|c| c.is_uppercase())
                                    && val.chars().any(|c| c.is_lowercase())
                                {
                                    continue;
                                }
                            }
                        }
                    }

                    let severity = if is_test_file {
                        Severity::Low
                    } else {
                        pattern.severity.clone()
                    };

                    findings.push(SecurityFinding {
                        id: format!(
                            "secret-{}-{}",
                            pattern.name.to_lowercase().replace(' ', "-"),
                            findings.len()
                        ),
                        severity,
                        category: FindingCategory::HardcodedSecret,
                        title: pattern.name.to_string(),
                        description: format!(
                            "Posible {} encontrado en {}:{}",
                            pattern.name,
                            rel_path,
                            line_num + 1
                        ),
                        file_path: Some(rel_path.clone()),
                        line_number: Some((line_num + 1) as u32),
                        remediation: pattern.remediation.to_string(),
                    });

                    break; // One finding per line
                }
            }
        }
    }
}
