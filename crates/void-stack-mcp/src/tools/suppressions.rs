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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ManageSuppressionsRequest;
    use void_stack_core::global_config::{GlobalConfig, save_global_config};
    use void_stack_core::model::Project;

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    fn req(
        project: &str,
        action: &str,
        rule: Option<&str>,
        path: Option<&str>,
    ) -> ManageSuppressionsRequest {
        ManageSuppressionsRequest {
            project: project.to_string(),
            action: action.to_string(),
            rule: rule.map(str::to_string),
            path: path.map(str::to_string),
        }
    }

    /// Full list/add/remove lifecycle in one test so the isolated global
    /// config is written exactly once and cannot race with other tests.
    #[tokio::test]
    async fn test_manage_suppressions_lifecycle() {
        crate::tools::isolate_test_data_dir();
        let name = format!("suppress-fixture-{}", std::process::id());
        let tmp = tempfile::tempdir().unwrap();
        let config = GlobalConfig {
            projects: vec![Project {
                name: name.clone(),
                description: String::new(),
                path: tmp.path().to_string_lossy().to_string(),
                project_type: None,
                tags: vec![],
                services: vec![],
                hooks: None,
            }],
            ..Default::default()
        };
        save_global_config(&config).unwrap();
        let mcp = VoidStackMcp::new();

        // Empty list explains how to add a rule.
        let out = text_of(
            &manage_suppressions(&mcp, req(&name, "list", None, None))
                .await
                .unwrap(),
        );
        assert!(out.contains("No suppressions configured"), "got: {out}");

        // add: writes .void-audit-ignore in the project root.
        let out = text_of(
            &manage_suppressions(&mcp, req(&name, "add", Some("unwrap-*"), Some("src/**")))
                .await
                .unwrap(),
        );
        assert!(out.contains("Added suppression"), "got: {out}");
        assert!(tmp.path().join(".void-audit-ignore").exists());

        // Adding the same rule twice is a no-op.
        let out = text_of(
            &manage_suppressions(&mcp, req(&name, "add", Some("unwrap-*"), Some("src/**")))
                .await
                .unwrap(),
        );
        assert!(out.contains("already exists"), "got: {out}");

        // list now renders the rule table.
        let out = text_of(
            &manage_suppressions(&mcp, req(&name, "list", None, None))
                .await
                .unwrap(),
        );
        assert!(out.contains("(1 rules)"), "got: {out}");
        assert!(out.contains("| `unwrap-*` | `src/**` |"));

        // remove: deletes the rule; removing again reports NotFound.
        let out = text_of(
            &manage_suppressions(&mcp, req(&name, "remove", Some("unwrap-*"), Some("src/**")))
                .await
                .unwrap(),
        );
        assert!(out.contains("Removed suppression"), "got: {out}");
        let out = text_of(
            &manage_suppressions(&mcp, req(&name, "remove", Some("unwrap-*"), Some("src/**")))
                .await
                .unwrap(),
        );
        assert!(out.contains("not found"), "got: {out}");

        // add/remove without rule or path are invalid params.
        let err = manage_suppressions(&mcp, req(&name, "add", None, Some("src/**")))
            .await
            .unwrap_err();
        assert!(err.message.contains("requires 'rule'"));
        let err = manage_suppressions(&mcp, req(&name, "add", Some("r"), None))
            .await
            .unwrap_err();
        assert!(err.message.contains("requires 'path'"));

        // Unknown actions and unknown projects are rejected.
        let err = manage_suppressions(&mcp, req(&name, "wipe", None, None))
            .await
            .unwrap_err();
        assert!(err.message.contains("Unknown action 'wipe'"));
        let err = manage_suppressions(&mcp, req("no-such-project", "list", None, None))
            .await
            .unwrap_err();
        assert!(err.message.contains("not found"));
    }
}
