use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::file_reader;
use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

use super::list_doc_files;

/// Logic for read_project_docs tool.
pub fn read_project_docs(project: &Project, filename: &str) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let doc_path = std::path::Path::new(&root).join(filename);

    // Security: only allow reading markdown/text files within the project
    let allowed_extensions = ["md", "txt", "toml", "json", "yml", "yaml"];
    let ext = doc_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !allowed_extensions.contains(&ext) {
        return Err(McpError::invalid_params(
            format!(
                "File type '.{}' not allowed. Use: {}",
                ext,
                allowed_extensions.join(", ")
            ),
            None,
        ));
    }
    // Block sensitive files (secrets, credentials, .env)
    if void_stack_core::security::is_sensitive_file(&doc_path) {
        return Err(McpError::invalid_params(
            "Cannot read sensitive/credential files".to_string(),
            None,
        ));
    }

    match std::fs::read_to_string(&doc_path) {
        Ok(content) => {
            // Truncate very large files
            let truncated = if content.len() > 50_000 {
                format!(
                    "{}...\n\n[truncated, {} bytes total]",
                    &content[..50_000],
                    content.len()
                )
            } else {
                content
            };
            Ok(CallToolResult::success(vec![Content::text(truncated)]))
        }
        Err(_) => {
            // List available doc files
            let available = list_doc_files(&root);
            let msg = if available.is_empty() {
                format!(
                    "'{}' not found in '{}'. No documentation files found.",
                    filename, project.name
                )
            } else {
                format!(
                    "'{}' not found in '{}'. Available files:\n{}",
                    filename,
                    project.name,
                    available.join("\n")
                )
            };
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        }
    }
}

/// Logic for read_all_docs tool.
pub fn read_all_docs(project: &Project) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let doc_extensions = ["md", "txt"];
    let mut docs = Vec::new();
    let mut total_size = 0usize;
    let max_total = 100_000; // 100KB total limit

    // Scan root directory for doc files
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut files: Vec<_> = entries
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let ext = std::path::Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                doc_extensions.contains(&ext)
            })
            .collect();
        files.sort_by_key(|e| e.file_name());

        // Prioritize important files first
        let priority = ["README.md", "CLAUDE.md", "CHANGELOG.md", "CONTRIBUTING.md"];
        files.sort_by_key(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let idx = priority.iter().position(|p| p.eq_ignore_ascii_case(&name));
            idx.unwrap_or(priority.len())
        });

        for entry in files {
            if total_size >= max_total {
                docs.push(format!(
                    "\n---\n[Truncated: reached {}KB limit]\n",
                    max_total / 1000
                ));
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if let Ok(content) = std::fs::read_to_string(&path) {
                let remaining = max_total - total_size;
                let truncated = if content.len() > remaining {
                    format!(
                        "{}...\n[truncated, {} bytes total]",
                        &content[..remaining],
                        content.len()
                    )
                } else {
                    content.clone()
                };
                total_size += truncated.len();
                docs.push(format!("# === {} ===\n\n{}\n", name, truncated));
            }
        }
    }

    // Also check for void-stack-analysis.md
    let analysis_path = std::path::Path::new(&root).join("void-stack-analysis.md");
    if analysis_path.exists()
        && total_size < max_total
        && let Ok(content) = std::fs::read_to_string(&analysis_path)
    {
        let remaining = max_total - total_size;
        let truncated = if content.len() > remaining {
            format!("{}...\n[truncated]", &content[..remaining])
        } else {
            content
        };
        docs.push(format!(
            "# === void-stack-analysis.md ===\n\n{}\n",
            truncated
        ));
    }

    if docs.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No documentation files found in '{}'.",
            project.name
        ))]));
    }

    Ok(CallToolResult::success(vec![Content::text(
        docs.join("\n---\n\n"),
    )]))
}

/// Logic for read_project_file tool.
pub fn read_project_file(project: &Project, path: &str) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let project_path = std::path::Path::new(&root);

    match file_reader::read_project_file(project_path, path) {
        Ok(content) => Ok(CallToolResult::success(vec![Content::text(content)])),
        Err(e) => Err(McpError::invalid_params(e.to_string(), None)),
    }
}

/// Most lines one get_chunk call returns.
const MAX_CHUNK_LINES: usize = 400;

/// Logic for get_chunk: a line range of a project file, numbered — what a
/// compact search hit expands to. Same access rules as read_project_file.
pub(crate) fn get_chunk(
    project: &Project,
    req: &crate::types::GetChunkRequest,
) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let content = file_reader::read_project_file(std::path::Path::new(&root), &req.file_path)
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(slice_lines(
        &req.file_path,
        &content,
        req.line_start,
        req.line_end,
    ))]))
}

fn slice_lines(path: &str, content: &str, start: usize, end: usize) -> String {
    let total = content.lines().count();
    let start = start.max(1);
    let end = end.max(start).min(total).min(start + MAX_CHUNK_LINES - 1);
    if start > total {
        return format!("{path} has only {total} lines.");
    }
    let mut out = format!("{path}:L{start}-L{end} (of {total})\n");
    for (i, line) in content
        .lines()
        .enumerate()
        .skip(start - 1)
        .take(end + 1 - start)
    {
        out.push_str(&format!("{:>5}│ {}\n", i + 1, line));
    }
    out
}

/// Logic for generate_claudeignore tool.
pub fn generate_claudeignore_tool(
    project: &Project,
    dry_run: bool,
) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let project_path = std::path::Path::new(&root);

    let result = void_stack_core::claudeignore::generate_claudeignore(project_path);

    let mut output = result.content.clone();
    output.push_str(&format!(
        "\n---\n{} patterns | ~{} files ignored",
        result.patterns_count, result.estimated_files_ignored
    ));

    if !dry_run {
        match void_stack_core::claudeignore::save_claudeignore(project_path, &result.content) {
            Ok(path) => {
                output.push_str(&format!("\n✓ Saved to {}", path.display()));
            }
            Err(e) => {
                output.push_str(&format!("\n✗ Failed to save: {}", e));
            }
        }
    } else {
        output.push_str("\n(dry run — file not saved)");
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

/// Logic for list_project_files tool.
pub fn list_project_files_tool(project: &Project) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let project_path = std::path::Path::new(&root);
    let files = file_reader::list_project_files(project_path);

    if files.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No files found in '{}'.",
            project.name
        ))]));
    }

    let output = format!(
        "Project '{}' — {} files:\n\n{}",
        project.name,
        files.len(),
        files.join("\n")
    );
    Ok(CallToolResult::success(vec![Content::text(output)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    fn project_at(root: &Path) -> Project {
        Project {
            name: "demo".to_string(),
            description: String::new(),
            path: root.to_string_lossy().to_string(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    // ── read_project_docs ───────────────────────────────────

    #[test]
    fn test_read_project_docs_reads_allowed_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("README.md"), "# Demo\n\nHello.").unwrap();
        let out = text_of(&read_project_docs(&project_at(tmp.path()), "README.md").unwrap());
        assert_eq!(out, "# Demo\n\nHello.");
    }

    #[test]
    fn test_read_project_docs_rejects_disallowed_extension() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("run.sh"), "echo hi").unwrap();
        let err = read_project_docs(&project_at(tmp.path()), "run.sh").unwrap_err();
        assert!(err.message.contains("not allowed"), "got: {}", err.message);
    }

    #[test]
    fn test_read_project_docs_blocks_sensitive_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Allowed extension (.toml) but a sensitive filename.
        std::fs::write(tmp.path().join("secrets.toml"), "token = \"x\"").unwrap();
        let err = read_project_docs(&project_at(tmp.path()), "secrets.toml").unwrap_err();
        assert!(err.message.contains("sensitive"), "got: {}", err.message);
    }

    #[test]
    fn test_read_project_docs_missing_file_lists_available() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("NOTES.md"), "notes").unwrap();
        let out = text_of(&read_project_docs(&project_at(tmp.path()), "MISSING.md").unwrap());
        assert!(
            out.contains("'MISSING.md' not found in 'demo'"),
            "got: {out}"
        );
        assert!(out.contains("NOTES.md"), "must list available docs");
    }

    #[test]
    fn test_read_project_docs_truncates_large_files() {
        let tmp = tempfile::tempdir().unwrap();
        let big = "x".repeat(60_000);
        std::fs::write(tmp.path().join("BIG.md"), &big).unwrap();
        let out = text_of(&read_project_docs(&project_at(tmp.path()), "BIG.md").unwrap());
        assert!(
            out.contains("[truncated, 60000 bytes total]"),
            "got tail: {}",
            &out[out.len() - 60..]
        );
        assert!(out.len() < big.len());
    }

    // ── read_all_docs ───────────────────────────────────────

    #[test]
    fn test_read_all_docs_prioritizes_readme() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("AAA.md"), "first alphabetically").unwrap();
        std::fs::write(tmp.path().join("README.md"), "the readme").unwrap();
        let out = text_of(&read_all_docs(&project_at(tmp.path())).unwrap());
        let readme_pos = out.find("=== README.md ===").unwrap();
        let aaa_pos = out.find("=== AAA.md ===").unwrap();
        assert!(readme_pos < aaa_pos, "README must come first");
    }

    #[test]
    fn test_read_all_docs_empty_project() {
        let tmp = tempfile::tempdir().unwrap();
        let out = text_of(&read_all_docs(&project_at(tmp.path())).unwrap());
        assert_eq!(out, "No documentation files found in 'demo'.");
    }

    // ── read_project_file / list_project_files ──────────────

    #[test]
    fn test_read_project_file_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let project = project_at(tmp.path());

        let out = text_of(&read_project_file(&project, "src/lib.rs").unwrap());
        assert!(out.contains("pub fn x() {}"));

        assert!(read_project_file(&project, "src/nope.rs").is_err());
    }

    #[test]
    fn test_list_project_files_tool_lists_and_handles_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(tmp.path());
        let out = text_of(&list_project_files_tool(&project).unwrap());
        assert_eq!(out, "No files found in 'demo'.");

        std::fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let out = text_of(&list_project_files_tool(&project).unwrap());
        assert!(out.contains("Project 'demo' — 1 files:"), "got: {out}");
        assert!(out.contains("main.rs"));
    }

    // ── generate_claudeignore ───────────────────────────────

    #[test]
    fn test_generate_claudeignore_dry_run_does_not_save() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let out = text_of(&generate_claudeignore_tool(&project_at(tmp.path()), true).unwrap());
        assert!(out.contains("(dry run — file not saved)"), "got: {out}");
        assert!(out.contains("patterns |"), "must report pattern stats");
        assert!(
            !tmp.path().join(".claudeignore").exists(),
            "dry run must not write the file"
        );
    }
}

#[cfg(test)]
mod chunk_tests {
    use super::slice_lines;

    #[test]
    fn slices_numbered_and_clamped() {
        let text = "a\nb\nc\nd\n";
        let out = slice_lines("x.rs", text, 2, 3);
        assert_eq!(out, "x.rs:L2-L3 (of 4)\n    2│ b\n    3│ c\n");
        // End past EOF clamps; start 0 is treated as 1.
        let out = slice_lines("x.rs", text, 0, 99);
        assert!(out.starts_with("x.rs:L1-L4 (of 4)"));
        assert!(slice_lines("x.rs", text, 9, 10).contains("only 4 lines"));
    }
}
