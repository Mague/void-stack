//! Tool profiles: expose a subset of the tools.
//!
//! Every connected client receives the schema of every tool in its system
//! prompt — with ~75 tools that is ~35,000 characters (≈9-11k tokens) per
//! conversation before the first question. A client that only needs the
//! code-context tools can ask for those:
//!
//! ```text
//! void-stack-mcp --tools context
//! VOID_STACK_MCP_TOOLS=context,get_logs void-stack-mcp
//! ```
//!
//! A value is a comma-separated mix of profile names and tool names.
//! Unset or `full` keeps every tool (the default, unchanged behaviour).

use std::collections::BTreeSet;

pub const ENV: &str = "VOID_STACK_MCP_TOOLS";

/// `--tools` from the command line (wins over the environment).
pub static FLAG: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The active spec: `--tools`, else the environment variable.
pub fn spec() -> Option<String> {
    FLAG.get().cloned().or_else(|| std::env::var(ENV).ok())
}

/// Understanding and navigating code.
const CONTEXT: &[&str] = &[
    "list_projects",
    "session_context",
    "project_map",
    "list_entrypoints",
    "semantic_search",
    "get_chunk",
    "get_index_stats",
    "index_project_codebase",
    "graph_rag_search",
    "graph_rag_search_cross",
    "build_structural_graph",
    "query_graph",
    "get_impact_radius",
    "get_api_contracts",
    "check_contracts",
    "read_project_file",
    "read_all_docs",
    "review_diff",
    "suggest_tests_for_diff",
    "find_dead_code",
];

/// Running and fixing services.
const DEVOPS: &[&str] = &[
    "list_projects",
    "project_status",
    "start_project",
    "stop_project",
    "start_service",
    "stop_service",
    "get_logs",
    "add_project",
    "update_project",
    "remove_project",
    "add_service",
    "update_service",
    "remove_service",
    "scan_directory",
    "check_dependencies",
    "check_env",
    "docker_analyze",
    "docker_generate",
    "doctor",
    "scan_project_space",
    "scan_global_space",
];

/// Quality and planning.
const REVIEW: &[&str] = &[
    "list_projects",
    "full_analysis",
    "analyze_project",
    "audit_project",
    "manage_suppressions",
    "review_diff",
    "suggest_commit_message",
    "suggest_tests_for_diff",
    "find_dead_code",
    "save_debt_snapshot",
    "list_debt_snapshots",
    "compare_debt",
    "board_list",
    "board_add_task",
    "board_move_task",
    "daily_briefing",
    "session_handoff",
];

/// Tool names to keep, or `None` for all of them.
pub fn selected(spec: Option<&str>) -> Option<BTreeSet<String>> {
    let spec = spec.map(str::trim).filter(|s| !s.is_empty())?;
    let mut keep = BTreeSet::new();
    for part in spec.split(',').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "" => {}
            "full" | "all" => return None,
            "context" | "code" => keep.extend(CONTEXT.iter().map(|s| s.to_string())),
            "devops" | "ops" | "services" => keep.extend(DEVOPS.iter().map(|s| s.to_string())),
            "review" | "quality" => keep.extend(REVIEW.iter().map(|s| s.to_string())),
            name => {
                keep.insert(name.to_string());
            }
        }
    }
    Some(keep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_and_names_combine() {
        assert_eq!(selected(None), None);
        assert_eq!(selected(Some("full")), None);
        assert_eq!(selected(Some("  ")), None);
        let ctx = selected(Some("context, get_logs")).unwrap();
        assert!(ctx.contains("semantic_search") && ctx.contains("get_logs"));
        assert!(!ctx.contains("start_project"));
    }

    #[test]
    fn every_profile_tool_exists() {
        let all: BTreeSet<String> = crate::server::VoidStackMcp::tool_names()
            .into_iter()
            .collect();
        for name in CONTEXT.iter().chain(DEVOPS).chain(REVIEW) {
            assert!(all.contains(*name), "profile lists unknown tool {name}");
        }
    }
}
