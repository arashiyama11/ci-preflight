use super::{CmdKind, SpecialActionKind};
use crate::action_catalog::{ActionCatalog, action_entry_for_uses, normalize_uses};
use crate::actions_parser::arena::{AstArena, AstId};
use crate::actions_parser::sh_parser::parse_sh;
use crate::actions_parser::sh_parser::sh_ast::ShAstNode;
use crate::actions_parser::source_map::SourceMap;
use crate::cmd_kind_rules::{RuleCmdKind, classify_simple_command};
use std::path::PathBuf;

pub(super) fn parse_shell_command_words(script: &str) -> Vec<Vec<String>> {
    let mut source_map = SourceMap::new();
    let source_id = source_map.add_sh_file(PathBuf::from("<with-shell-input>"), script.to_string());
    let Ok((program, arena)) = parse_sh(&source_map, &source_id) else {
        return Vec::new();
    };
    extract_simple_commands(program.list, &arena)
        .into_iter()
        .filter_map(|id| read_simple_command_words(id, &arena))
        .filter(|words| !words.is_empty())
        .collect()
}

pub(super) fn extract_simple_commands(run_id: AstId, arena: &AstArena) -> Vec<AstId> {
    let mut out = Vec::new();
    collect_simple_commands(run_id, arena, &mut out);
    out
}

pub(super) fn read_simple_command_words(cmd_id: AstId, arena: &AstArena) -> Option<Vec<String>> {
    let ShAstNode::SimpleCommand { argv, .. } = arena.get_sh(cmd_id) else {
        return None;
    };
    let words = argv
        .iter()
        .filter_map(|id| match arena.get_sh(*id) {
            ShAstNode::Word(w) => {
                let s = w.trim();
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    Some(words)
}

pub(super) fn classify_simple_command_from_words(words: &[String]) -> CmdKind {
    match classify_simple_command(words) {
        Ok(kind) => map_rule_cmd_kind(kind),
        Err(_) => CmdKind::Other,
    }
}

pub(super) fn classify_uses_from_catalog(
    uses: &str,
    catalog: Option<&ActionCatalog>,
) -> (CmdKind, Option<SpecialActionKind>) {
    let Some(catalog) = catalog else {
        return (CmdKind::Other, None);
    };
    let Some(entry) = action_entry_for_uses(uses, catalog) else {
        return (CmdKind::Other, None);
    };
    (
        map_cmd_kind_label(entry.cmd_kind.as_deref()),
        map_special_action_label(entry.special_action.as_deref()),
    )
}

pub(super) fn is_unknown_uses(uses: &str, catalog: Option<&ActionCatalog>) -> bool {
    let Some(catalog) = catalog else {
        return true;
    };
    if normalize_uses(uses).is_none() {
        return true;
    }
    action_entry_for_uses(uses, catalog).is_none()
}

fn map_rule_cmd_kind(kind: RuleCmdKind) -> CmdKind {
    match kind {
        RuleCmdKind::EnvSetup => CmdKind::EnvSetup,
        RuleCmdKind::TestSetup => CmdKind::TestSetup,
        RuleCmdKind::Test => CmdKind::Test,
        RuleCmdKind::Assert => CmdKind::Assert,
        RuleCmdKind::Other => CmdKind::Other,
    }
}

fn map_cmd_kind_label(raw: Option<&str>) -> CmdKind {
    match raw {
        Some("EnvSetup") => CmdKind::EnvSetup,
        Some("TestSetup") => CmdKind::TestSetup,
        Some("Test") => CmdKind::Test,
        Some("Assert") => CmdKind::Assert,
        _ => CmdKind::Other,
    }
}

fn map_special_action_label(raw: Option<&str>) -> Option<SpecialActionKind> {
    match raw {
        Some("Checkout") => Some(SpecialActionKind::Checkout),
        Some("ArtifactUpload") => Some(SpecialActionKind::ArtifactUpload),
        Some("ArtifactDownload") => Some(SpecialActionKind::ArtifactDownload),
        _ => None,
    }
}

fn collect_simple_commands(id: AstId, arena: &AstArena, out: &mut Vec<AstId>) {
    match arena.get_sh(id) {
        ShAstNode::List(items) => {
            for item in items {
                collect_simple_commands(item.body, arena, out);
            }
        }
        ShAstNode::AndOr { first, rest } => {
            collect_simple_commands(*first, arena, out);
            for item in rest {
                collect_simple_commands(item.body, arena, out);
            }
        }
        ShAstNode::Pipeline { first, rest } => {
            collect_simple_commands(*first, arena, out);
            for item in rest {
                collect_simple_commands(*item, arena, out);
            }
        }
        ShAstNode::SimpleCommand { .. } => out.push(id),
        ShAstNode::If {
            cond,
            then_part,
            else_part,
        } => {
            collect_simple_commands(*cond, arena, out);
            collect_simple_commands(*then_part, arena, out);
            if let Some(else_part) = else_part {
                collect_simple_commands(*else_part, arena, out);
            }
        }
        ShAstNode::While { cond, body } => {
            collect_simple_commands(*cond, arena, out);
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::For { var, items, body } => {
            collect_simple_commands(*var, arena, out);
            for item in items {
                collect_simple_commands(*item, arena, out);
            }
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::FunctionDef { name, body } => {
            collect_simple_commands(*name, arena, out);
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::Subshell { body }
        | ShAstNode::CommandSubstitution { body }
        | ShAstNode::Group { body } => {
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::Word(_)
        | ShAstNode::Assignment(_)
        | ShAstNode::Redir { .. }
        | ShAstNode::Unknown => {}
    }
}
