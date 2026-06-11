//! `void setup` — register void-stack-mcp in every installed MCP client.
//!
//! Detects Claude Desktop, Claude Code, Cursor, Windsurf, Cline and VS Code
//! by their config locations and upserts a `void-stack` server entry.
//! Idempotent: re-running updates the command path in place, never
//! duplicates. `--dry-run` prints what would change without writing.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// One supported MCP client: where its config lives and the JSON key that
/// holds the server map (`mcpServers` everywhere except VS Code's
/// `servers`).
struct ClientSpec {
    name: &'static str,
    config_path: PathBuf,
    /// Directory whose existence means "this client is installed".
    detect_dir: PathBuf,
    root_key: &'static str,
    /// Extra fields for the server entry (VS Code wants `"type": "stdio"`).
    extra: &'static [(&'static str, &'static str)],
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn client_specs() -> Vec<ClientSpec> {
    let home = home();
    #[cfg(target_os = "macos")]
    let app_support = home.join("Library/Application Support");
    #[cfg(target_os = "windows")]
    let app_support = dirs::config_dir().unwrap_or_else(|| home.join("AppData/Roaming"));
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let app_support = dirs::config_dir().unwrap_or_else(|| home.join(".config"));

    vec![
        ClientSpec {
            name: "Claude Desktop",
            config_path: app_support.join("Claude/claude_desktop_config.json"),
            detect_dir: app_support.join("Claude"),
            root_key: "mcpServers",
            extra: &[],
        },
        ClientSpec {
            name: "Claude Code",
            config_path: home.join(".claude.json"),
            detect_dir: home.join(".claude"),
            root_key: "mcpServers",
            extra: &[],
        },
        ClientSpec {
            name: "Cursor",
            config_path: home.join(".cursor/mcp.json"),
            detect_dir: home.join(".cursor"),
            root_key: "mcpServers",
            extra: &[],
        },
        ClientSpec {
            name: "Windsurf",
            config_path: home.join(".codeium/windsurf/mcp_config.json"),
            detect_dir: home.join(".codeium/windsurf"),
            root_key: "mcpServers",
            extra: &[],
        },
        ClientSpec {
            name: "Cline (VS Code)",
            config_path: app_support.join(
                "Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json",
            ),
            detect_dir: app_support.join("Code/User/globalStorage/saoudrizwan.claude-dev"),
            root_key: "mcpServers",
            extra: &[],
        },
        ClientSpec {
            name: "VS Code (native MCP)",
            config_path: app_support.join("Code/User/mcp.json"),
            detect_dir: app_support.join("Code/User"),
            root_key: "servers",
            extra: &[("type", "stdio")],
        },
    ]
}

/// Locate the void-stack-mcp binary to register.
fn find_mcp_binary() -> Option<PathBuf> {
    let candidates = [
        home().join(".local/bin/void-stack-mcp"),
        home().join(".cargo/bin/void-stack-mcp"),
    ];
    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }
    // Sibling of the current executable (cargo install layout).
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("void-stack-mcp")))
        .filter(|p| p.exists())
}

/// Upsert the `void-stack` entry into a client config. Returns a human
/// description of what changed (`registered`, `updated path`, `unchanged`).
pub(crate) fn upsert_server(
    config_path: &Path,
    root_key: &str,
    command: &str,
    extra: &[(&str, &str)],
    dry_run: bool,
) -> Result<&'static str> {
    let mut root: serde_json::Value = match std::fs::read_to_string(config_path) {
        Ok(c) if !c.trim().is_empty() => serde_json::from_str(&c)
            .with_context(|| format!("parsing {}", config_path.display()))?,
        _ => serde_json::json!({}),
    };

    let mut entry = serde_json::json!({ "command": command });
    for (k, v) in extra {
        entry[*k] = serde_json::Value::String((*v).to_string());
    }

    let servers = root
        .as_object_mut()
        .context("config root is not a JSON object")?
        .entry(root_key)
        .or_insert_with(|| serde_json::json!({}));
    let servers = servers
        .as_object_mut()
        .context("server map is not a JSON object")?;

    let outcome = match servers.get("void-stack") {
        Some(existing) if *existing == entry => "unchanged",
        Some(_) => "updated path",
        None => "registered",
    };
    if outcome == "unchanged" || dry_run {
        return Ok(outcome);
    }

    servers.insert("void-stack".to_string(), entry);
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, serde_json::to_string_pretty(&root)?)?;
    Ok(outcome)
}

pub fn cmd_setup(dry_run: bool, yes: bool, mcp_path: Option<&str>) -> Result<()> {
    let binary = match mcp_path {
        Some(p) => PathBuf::from(p),
        None => find_mcp_binary().context(
            "void-stack-mcp binary not found (looked in ~/.local/bin, ~/.cargo/bin). \
             Pass --mcp-path <path>.",
        )?,
    };
    let command = binary.to_string_lossy().to_string();
    println!("Registering MCP binary: {}\n", command);

    let interactive = std::io::stdin().is_terminal() && !yes && !dry_run;
    let mut touched = 0usize;

    for spec in client_specs() {
        if !spec.detect_dir.exists() {
            println!("· {} — not detected, skipping", spec.name);
            continue;
        }
        if interactive {
            print!("? {} detected — register void-stack? [Y/n] ", spec.name);
            std::io::stdout().flush().ok();
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if matches!(answer.trim().to_lowercase().as_str(), "n" | "no") {
                println!("  skipped");
                continue;
            }
        }
        match upsert_server(
            &spec.config_path,
            spec.root_key,
            &command,
            spec.extra,
            dry_run,
        ) {
            Ok(outcome) => {
                let prefix = if dry_run { "[dry-run] " } else { "" };
                println!(
                    "✓ {} — {}{} ({})",
                    spec.name,
                    prefix,
                    outcome,
                    spec.config_path.display()
                );
                if outcome != "unchanged" {
                    touched += 1;
                }
            }
            Err(e) => println!("✗ {} — failed: {}", spec.name, e),
        }
    }

    println!(
        "\n{} client(s) {}. Restart the clients to load the server.",
        touched,
        if dry_run { "would change" } else { "updated" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upsert_claude_desktop_format_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("claude_desktop_config.json");
        std::fs::write(
            &cfg,
            r#"{"mcpServers": {"other": {"command": "docker"}}, "preferences": {"theme": "dark"}}"#,
        )
        .unwrap();

        let r = upsert_server(
            &cfg,
            "mcpServers",
            "/usr/local/bin/void-stack-mcp",
            &[],
            false,
        )
        .unwrap();
        assert_eq!(r, "registered");

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["void-stack"]["command"],
            "/usr/local/bin/void-stack-mcp"
        );
        // Pre-existing keys untouched.
        assert_eq!(v["mcpServers"]["other"]["command"], "docker");
        assert_eq!(v["preferences"]["theme"], "dark");

        // Re-run with the same path: unchanged, no duplicate.
        let r = upsert_server(
            &cfg,
            "mcpServers",
            "/usr/local/bin/void-stack-mcp",
            &[],
            false,
        )
        .unwrap();
        assert_eq!(r, "unchanged");

        // New path: updated in place.
        let r = upsert_server(&cfg, "mcpServers", "/new/path/void-stack-mcp", &[], false).unwrap();
        assert_eq!(r, "updated path");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["void-stack"]["command"],
            "/new/path/void-stack-mcp"
        );
        let servers = v["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 2, "never duplicates");
    }

    #[test]
    fn test_upsert_cursor_creates_file_and_vscode_uses_servers_key() {
        let dir = tempfile::tempdir().unwrap();

        // Cursor: file doesn't exist yet.
        let cursor = dir.path().join(".cursor/mcp.json");
        let r = upsert_server(&cursor, "mcpServers", "/bin/void-stack-mcp", &[], false).unwrap();
        assert_eq!(r, "registered");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["void-stack"]["command"],
            "/bin/void-stack-mcp"
        );

        // VS Code: `servers` root key + type stdio.
        let vscode = dir.path().join("mcp.json");
        upsert_server(
            &vscode,
            "servers",
            "/bin/void-stack-mcp",
            &[("type", "stdio")],
            false,
        )
        .unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&vscode).unwrap()).unwrap();
        assert_eq!(v["servers"]["void-stack"]["type"], "stdio");
    }

    #[test]
    fn test_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("mcp.json");
        let r = upsert_server(&cfg, "mcpServers", "/bin/x", &[], true).unwrap();
        assert_eq!(r, "registered");
        assert!(!cfg.exists(), "dry-run must not create the file");
    }
}
