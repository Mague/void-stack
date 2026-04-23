//! MCP tool for managing audit suppressions (CRUD over `.void-audit-ignore`).

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use crate::server::VoidStackMcp;
use crate::types::ManageSuppressionsRequest;

use void_stack_core::audit::suppress::{
    AddResult, RemoveResult, add_rule, list_rules, remove_rule,
};

pub async fn manage_suppressions(
    _mcp: &VoidStackMcp,
    req: ManageSuppressionsRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let root = std::path::Path::new(&project.path);

    let output = match req.action.as_str() {
        "list" => {
            let rules = list_rules(root).map_err(|e| McpError::internal_error(e, None))?;
            if rules.is_empty() {
                format!(
                    "No suppressions configured for '{}'.\n\
                     Add one: manage_suppressions(action=\"add\", project=\"{}\", \
                     rule=\"unwrap-*\", path=\"src/**\")",
                    project.name, project.name
                )
            } else {
                let mut md = format!(
                    "Suppressions for '{}' ({} rules):\n\n\
                     | Rule | Path |\n|------|------|\n",
                    project.name,
                    rules.len()
                );
                for r in &rules {
                    md.push_str(&format!("| `{}` | `{}` |\n", r.rule, r.path));
                }
                md
            }
        }
        "add" => {
            let rule = req.rule.as_deref().ok_or_else(|| {
                McpError::invalid_params("add requires 'rule' parameter".to_string(), None)
            })?;
            let path = req.path.as_deref().ok_or_else(|| {
                McpError::invalid_params("add requires 'path' parameter".to_string(), None)
            })?;
            match add_rule(root, rule, path).map_err(|e| McpError::internal_error(e, None))? {
                AddResult::Added => format!(
                    "Added suppression to '{}': `{} {}`\n\
                     Run audit_project to see the effect.",
                    project.name, rule, path
                ),
                AddResult::AlreadyExists => format!(
                    "Rule `{} {}` already exists in '{}' — no change.",
                    rule, path, project.name
                ),
            }
        }
        "remove" => {
            let rule = req.rule.as_deref().ok_or_else(|| {
                McpError::invalid_params("remove requires 'rule' parameter".to_string(), None)
            })?;
            let path = req.path.as_deref().ok_or_else(|| {
                McpError::invalid_params("remove requires 'path' parameter".to_string(), None)
            })?;
            match remove_rule(root, rule, path).map_err(|e| McpError::internal_error(e, None))? {
                RemoveResult::Removed => {
                    format!(
                        "Removed suppression from '{}': `{} {}`",
                        project.name, rule, path
                    )
                }
                RemoveResult::NotFound => format!(
                    "Rule `{} {}` not found in '{}'. Use action=\"list\" to see active rules.",
                    rule, path, project.name
                ),
            }
        }
        other => {
            return Err(McpError::invalid_params(
                format!("Unknown action '{}' — use: list, add, remove", other),
                None,
            ));
        }
    };

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
