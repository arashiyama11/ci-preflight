use crate::action_catalog::{ActionCatalog, required_tools_for_uses};
use crate::actions_parser::actions_ast::ActionsAst;
use crate::actions_parser::arena::{AstArena, AstId};
use crate::actions_parser::sh_parser::sh_ast::ShAstNode;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct ToolCheckReport {
    pub required_tools: Vec<String>,
    pub found_tools: Vec<String>,
    pub missing_tools: Vec<String>,
    pub unknown_commands: Vec<String>,
    pub unknown_uses: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreflightStatus {
    Pass,
    FailMissingTools,
}

impl ToolCheckReport {
    pub fn status(&self) -> PreflightStatus {
        if self.missing_tools.is_empty() {
            PreflightStatus::Pass
        } else {
            PreflightStatus::FailMissingTools
        }
    }
}

pub fn check_workflow_tools(
    root: AstId,
    arena: &AstArena,
    catalog: &ActionCatalog,
) -> ToolCheckReport {
    let mut required = BTreeSet::new();
    let mut unknown_commands = BTreeSet::new();
    let mut unknown_uses = BTreeSet::new();

    collect_from_actions(
        root,
        arena,
        catalog,
        &mut required,
        &mut unknown_commands,
        &mut unknown_uses,
    );

    let required_tools: Vec<String> = required.into_iter().collect();
    let (found_tools, missing_tools) = check_tools_installed(&required_tools);

    ToolCheckReport {
        required_tools,
        found_tools,
        missing_tools,
        unknown_commands: unknown_commands.into_iter().collect(),
        unknown_uses: unknown_uses.into_iter().collect(),
    }
}

fn collect_from_actions(
    id: AstId,
    arena: &AstArena,
    catalog: &ActionCatalog,
    required: &mut BTreeSet<String>,
    unknown_commands: &mut BTreeSet<String>,
    unknown_uses: &mut BTreeSet<String>,
) {
    match arena.get_actions(&id) {
        ActionsAst::Workflow { jobs, .. } => {
            for job in jobs {
                collect_from_actions(
                    *job,
                    arena,
                    catalog,
                    required,
                    unknown_commands,
                    unknown_uses,
                );
            }
        }
        ActionsAst::Job { steps, .. } => {
            for step in steps {
                collect_from_actions(
                    *step,
                    arena,
                    catalog,
                    required,
                    unknown_commands,
                    unknown_uses,
                );
            }
        }
        ActionsAst::RunStep { run, .. } => {
            collect_from_sh(*run, arena, required, unknown_commands);
        }
        ActionsAst::UsesStep { uses, .. } => {
            if let Some(tools) = required_tools_for_uses(uses, catalog) {
                for tool in tools {
                    let normalized = tool.trim();
                    if !normalized.is_empty() {
                        required.insert(normalized.to_string());
                    }
                }
            } else {
                unknown_uses.insert(uses.clone());
            }
        }
        _ => {}
    }
}

fn collect_from_sh(
    id: AstId,
    arena: &AstArena,
    required: &mut BTreeSet<String>,
    unknown_commands: &mut BTreeSet<String>,
) {
    match arena.get_sh(id) {
        ShAstNode::List(items) => {
            for item in items {
                collect_from_sh(item.body, arena, required, unknown_commands);
            }
        }
        ShAstNode::AndOr { first, rest } => {
            collect_from_sh(*first, arena, required, unknown_commands);
            for node in rest {
                collect_from_sh(node.body, arena, required, unknown_commands);
            }
        }
        ShAstNode::Pipeline { first, rest } => {
            collect_from_sh(*first, arena, required, unknown_commands);
            for node in rest {
                collect_from_sh(*node, arena, required, unknown_commands);
            }
        }
        ShAstNode::SimpleCommand { argv, .. } => {
            let Some(first) = argv.first() else {
                return;
            };
            match arena.get_sh(*first) {
                ShAstNode::Word(word) => {
                    let cmd = word.trim();
                    if !cmd.is_empty() && !is_shell_builtin(cmd) {
                        required.insert(cmd.to_string());
                    }
                }
                _ => {
                    unknown_commands.insert("<non-word-command>".to_string());
                }
            }
        }
        ShAstNode::If {
            cond,
            then_part,
            else_part,
        } => {
            collect_from_sh(*cond, arena, required, unknown_commands);
            collect_from_sh(*then_part, arena, required, unknown_commands);
            if let Some(else_part) = else_part {
                collect_from_sh(*else_part, arena, required, unknown_commands);
            }
        }
        ShAstNode::While { cond, body } => {
            collect_from_sh(*cond, arena, required, unknown_commands);
            collect_from_sh(*body, arena, required, unknown_commands);
        }
        ShAstNode::For { var, items, body } => {
            collect_from_sh(*var, arena, required, unknown_commands);
            for item in items {
                collect_from_sh(*item, arena, required, unknown_commands);
            }
            collect_from_sh(*body, arena, required, unknown_commands);
        }
        ShAstNode::FunctionDef { name, body } => {
            collect_from_sh(*name, arena, required, unknown_commands);
            collect_from_sh(*body, arena, required, unknown_commands);
        }
        ShAstNode::Subshell { body } | ShAstNode::Group { body } => {
            collect_from_sh(*body, arena, required, unknown_commands);
        }
        ShAstNode::Word(_)
        | ShAstNode::Assignment(_)
        | ShAstNode::Redir { .. }
        | ShAstNode::Unknown => {}
    }
}

fn is_shell_builtin(cmd: &str) -> bool {
    matches!(
        cmd,
        "cd" | "echo" | "export" | "test" | ":" | "true" | "false"
    )
}

pub fn check_tools_installed(required_tools: &[String]) -> (Vec<String>, Vec<String>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();

    for tool in required_tools {
        if is_executable_on_path(tool) {
            found.push(tool.clone());
        } else {
            missing.push(tool.clone());
        }
    }

    (found, missing)
}

fn is_executable_on_path(tool: &str) -> bool {
    if tool.is_empty() {
        return false;
    }

    if tool.contains('/') {
        return is_executable_file(Path::new(tool));
    }

    let Some(path_os) = std::env::var_os("PATH") else {
        return false;
    };

    for dir in std::env::split_paths(&path_os) {
        let candidate: PathBuf = dir.join(tool);
        if is_executable_file(&candidate) {
            return true;
        }
    }

    false
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };

    if !meta.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{PreflightStatus, check_workflow_tools, is_shell_builtin};
    use crate::action_catalog::{ActionCatalog, ActionCatalogEntry, load_action_catalog};
    use crate::actions_parser;

    #[test]
    fn builtins_are_filtered() {
        assert!(is_shell_builtin("echo"));
        assert!(is_shell_builtin("cd"));
        assert!(!is_shell_builtin("cargo"));
    }

    #[test]
    fn collects_tools_from_run_and_uses() {
        let yaml = r#"
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          echo hello
          cargo test
"#;

        let mut source_map = actions_parser::source_map::SourceMap::new();
        let source_id = source_map.add_yaml(
            std::path::PathBuf::from("wf.yml"),
            "workflow".to_string(),
            yaml.to_string(),
        );
        let (root, arena, errs) =
            actions_parser::parse_actions_yaml(&mut source_map, &source_id).unwrap();
        assert!(errs.is_empty());

        let mut catalog = ActionCatalog::new();
        catalog.insert(
            "actions/checkout".to_string(),
            ActionCatalogEntry {
                required_tools: vec!["git".to_string()],
                cmd_kind: Some("EnvSetup".to_string()),
                special_action: Some("Checkout".to_string()),
                confidence: None,
                notes: None,
            },
        );

        let report = check_workflow_tools(root, &arena, &catalog);
        assert!(report.required_tools.contains(&"cargo".to_string()));
        assert!(report.required_tools.contains(&"git".to_string()));
    }

    #[test]
    fn fixture_mixed_uses_tracks_unknown_and_known() {
        let yaml = std::fs::read_to_string("test/uses_mixed.yml").unwrap();
        let mut source_map = actions_parser::source_map::SourceMap::new();
        let source_id = source_map.add_yaml(
            std::path::PathBuf::from("test/uses_mixed.yml"),
            "workflow".to_string(),
            yaml,
        );
        let (root, arena, errs) =
            actions_parser::parse_actions_yaml(&mut source_map, &source_id).unwrap();
        assert!(errs.is_empty());

        let catalog = load_action_catalog().unwrap();
        let report = check_workflow_tools(root, &arena, &catalog);

        assert!(report.required_tools.contains(&"cargo".to_string()));
        assert!(report.required_tools.contains(&"git".to_string()));
        assert!(
            report
                .unknown_uses
                .contains(&"octo-org/custom-action@v1".to_string())
        );
        assert!(
            !report
                .unknown_uses
                .contains(&"actions/checkout@v4".to_string())
        );
    }

    #[test]
    fn fixture_missing_tool_fails_preflight() {
        let yaml = std::fs::read_to_string("test/missing_tool.yml").unwrap();
        let mut source_map = actions_parser::source_map::SourceMap::new();
        let source_id = source_map.add_yaml(
            std::path::PathBuf::from("test/missing_tool.yml"),
            "workflow".to_string(),
            yaml,
        );
        let (root, arena, errs) =
            actions_parser::parse_actions_yaml(&mut source_map, &source_id).unwrap();
        assert!(errs.is_empty());

        let catalog: ActionCatalog = ActionCatalog::new();
        let report = check_workflow_tools(root, &arena, &catalog);

        assert_eq!(report.status(), PreflightStatus::FailMissingTools);
        assert!(
            report
                .missing_tools
                .contains(&"definitely_not_installed_tool".to_string())
        );
    }
}
