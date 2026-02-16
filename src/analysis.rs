#![allow(dead_code)]

use crate::action_catalog::{
    ActionCatalog, action_entry_for_uses, load_action_catalog, normalize_uses,
    shell_input_keys_for_uses,
};
use crate::actions_parser::actions_ast::ActionsAst;
use crate::actions_parser::arena::{AstArena, AstId};
use crate::actions_parser::sh_parser::parse_sh;
use crate::actions_parser::sh_parser::sh_ast::ShAstNode;
use crate::actions_parser::source_map::SourceMap;
use crate::cmd_kind_rules::{RuleCmdKind, classify_simple_command};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CmdKind {
    EnvSetup,
    TestSetup,
    Test,
    Assert,
    Other,
}

impl CmdKind {
    fn as_str(&self) -> &'static str {
        match self {
            CmdKind::EnvSetup => "EnvSetup",
            CmdKind::TestSetup => "TestSetup",
            CmdKind::Test => "Test",
            CmdKind::Assert => "Assert",
            CmdKind::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecialActionKind {
    Checkout,
    ArtifactUpload,
    ArtifactDownload,
}

impl SpecialActionKind {
    fn as_str(&self) -> &'static str {
        match self {
            SpecialActionKind::Checkout => "Checkout",
            SpecialActionKind::ArtifactUpload => "ArtifactUpload",
            SpecialActionKind::ArtifactDownload => "ArtifactDownload",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Attr {
    pub kind: Option<CmdKind>,
    pub special_action: Option<SpecialActionKind>,
    pub confidence: f32,
    pub notes: Vec<String>,
    pub tools: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CommandPlan {
    pub ast_id: AstId,
    pub attr: Attr,
}

#[derive(Clone, Debug)]
pub struct StepPlan {
    pub step_id: AstId,
    pub commands: Vec<CommandPlan>,
}

#[derive(Clone, Debug, Default)]
pub struct AnalysisResult {
    pub steps: Vec<StepPlan>,
    pub unknown_uses: Vec<String>,
    pub errors: Vec<AnalysisError>,
}

#[derive(Clone, Debug)]
pub struct AnalysisError {
    pub message: String,
    pub at: Option<AstId>,
}

#[derive(Clone, Debug)]
pub struct PlanOptions {
    pub include_env_setup: bool,
    pub include_other: bool,
}

#[derive(Clone, Debug)]
pub struct ExecutionPlan {
    pub commands: Vec<PlannedCommand>,
}

#[derive(Clone, Debug)]
pub struct PlannedCommand {
    pub ast_id: AstId,
    pub kind: CmdKind,
}

pub fn analyze_actions(root: AstId, arena: &AstArena) -> AnalysisResult {
    let mut errors = Vec::new();
    let catalog = match load_action_catalog() {
        Ok(catalog) => Some(catalog),
        Err(err) => {
            errors.push(AnalysisError {
                message: format!("failed to load action catalog: {err}"),
                at: None,
            });
            None
        }
    };

    let mut steps = Vec::new();
    let mut unknown_uses = Vec::new();
    collect_steps(root, arena, catalog.as_ref(), &mut steps, &mut unknown_uses);
    AnalysisResult {
        steps,
        unknown_uses,
        errors,
    }
}

pub fn analyze_step(step_id: AstId, arena: &AstArena) -> StepPlan {
    let catalog = load_action_catalog().ok();
    analyze_step_with_catalog(step_id, arena, catalog.as_ref())
}

fn analyze_step_with_catalog(
    step_id: AstId,
    arena: &AstArena,
    catalog: Option<&ActionCatalog>,
) -> StepPlan {
    let commands = match arena.get_actions(&step_id) {
        ActionsAst::RunStep { run, .. } => analyze_run_step(*run, arena),
        ActionsAst::UsesStep { uses, with, .. } => {
            let mut commands = vec![CommandPlan {
                ast_id: step_id,
                attr: analyze_uses_step(uses, catalog),
            }];
            commands.extend(analyze_uses_shell_inputs(
                step_id,
                uses,
                with.as_ref(),
                catalog,
            ));
            commands
        }
        _ => Vec::new(),
    };
    StepPlan { step_id, commands }
}

pub fn analyze_run_step(run_id: AstId, arena: &AstArena) -> Vec<CommandPlan> {
    extract_simple_commands(run_id, arena)
        .into_iter()
        .map(|cmd_id| CommandPlan {
            ast_id: cmd_id,
            attr: analyze_simple_command(cmd_id, arena),
        })
        .collect()
}

pub fn analyze_simple_command(cmd_id: AstId, arena: &AstArena) -> Attr {
    let mut attr = Attr {
        confidence: 0.4,
        ..Attr::default()
    };
    let Some(argv) = read_simple_command_words(cmd_id, arena) else {
        return attr;
    };
    if argv.is_empty() {
        return attr;
    }
    attr.tools.push(argv.join(" "));
    attr.kind = Some(classify_simple_command_from_words(&argv));
    attr.confidence = 0.9;
    attr
}

fn analyze_uses_step(uses: &str, catalog: Option<&ActionCatalog>) -> Attr {
    let mut attr = Attr {
        confidence: 0.9,
        ..Attr::default()
    };
    let (kind, special_action) = classify_uses_from_catalog(uses, catalog);
    attr.kind = Some(kind);
    attr.special_action = special_action;
    attr.tools.push(uses.to_string());
    attr
}

fn analyze_uses_shell_inputs(
    step_id: AstId,
    uses: &str,
    with: Option<&BTreeMap<String, String>>,
    catalog: Option<&ActionCatalog>,
) -> Vec<CommandPlan> {
    let Some(with) = with else {
        return Vec::new();
    };
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    let Some(keys) = shell_input_keys_for_uses(uses, catalog) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for key in keys {
        let Some(script) = with.get(key) else {
            continue;
        };
        for words in parse_shell_command_words(script) {
            if words.is_empty() {
                continue;
            }
            out.push(CommandPlan {
                ast_id: step_id,
                attr: Attr {
                    kind: Some(classify_simple_command_from_words(&words)),
                    confidence: 0.8,
                    tools: vec![words.join(" ")],
                    ..Attr::default()
                },
            });
        }
    }
    out
}

fn parse_shell_command_words(script: &str) -> Vec<Vec<String>> {
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

pub fn extract_simple_commands(run_id: AstId, arena: &AstArena) -> Vec<AstId> {
    let mut out = Vec::new();
    collect_simple_commands(run_id, arena, &mut out);
    out
}

pub fn build_execution_plan(analysis: &AnalysisResult, opts: &PlanOptions) -> ExecutionPlan {
    let mut commands = Vec::new();
    for step in &analysis.steps {
        for command in &step.commands {
            let kind = command.attr.kind.clone().unwrap_or(CmdKind::Other);
            if matches!(kind, CmdKind::EnvSetup) && !opts.include_env_setup {
                continue;
            }
            if matches!(kind, CmdKind::Other) && !opts.include_other {
                continue;
            }
            commands.push(PlannedCommand {
                ast_id: command.ast_id,
                kind,
            });
        }
    }
    ExecutionPlan { commands }
}

pub fn format_cmd_kind_lines(analysis: &AnalysisResult) -> Vec<String> {
    let mut lines = Vec::new();
    for step in &analysis.steps {
        if step.commands.is_empty() {
            continue;
        }
        let left = step
            .commands
            .iter()
            .map(|cmd| {
                cmd.attr
                    .tools
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "<unknown>".to_string())
            })
            .collect::<Vec<_>>()
            .join(" && ");
        let right = step
            .commands
            .iter()
            .map(format_command_kind)
            .collect::<Vec<_>>()
            .join(" && ");
        lines.push(format!("{left} --- {right}"));
    }
    lines
}

pub fn annotate_yaml_with_cmd_kind(yaml: &str, analysis: &AnalysisResult) -> String {
    let mut step_index = 0usize;
    let mut out = String::new();
    let lines = yaml.lines().collect::<Vec<_>>();
    let catalog = load_action_catalog().ok();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if is_uses_line(trimmed) {
            let Some(step) = nth_non_empty_step(analysis, step_index) else {
                out.push_str(line);
                out.push('\n');
                i += 1;
                continue;
            };
            step_index += 1;

            let uses_value = extract_uses_value(trimmed);
            let shell_input_keys = uses_value
                .as_deref()
                .and_then(|uses| {
                    catalog
                        .as_ref()
                        .and_then(|c| shell_input_keys_for_uses(uses, c))
                })
                .map(|keys| keys.iter().map(|k| k.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();

            if shell_input_keys.is_empty() || step.commands.len() <= 1 {
                out.push_str(line);
                out.push_str(" --- ");
                out.push_str(&format_step_kinds(step));
                out.push('\n');
                i += 1;
                continue;
            }

            out.push_str(line);
            out.push_str(" --- ");
            out.push_str(&format_command_kind(&step.commands[0]));
            out.push('\n');
            i += 1;

            let step_base_indent = leading_spaces(line);
            let mut command_index = 1usize;
            while i < lines.len() {
                let body_line = lines[i];
                let body_trimmed = body_line.trim();
                let body_indent = leading_spaces(body_line);
                if !body_trimmed.is_empty()
                    && body_indent <= step_base_indent
                    && body_trimmed.starts_with('-')
                {
                    break;
                }

                if let Some((key, value)) = parse_mapping_key_value(body_trimmed) {
                    if shell_input_keys.iter().any(|k| *k == key) {
                        if value == "|" || value == ">" {
                            out.push_str(body_line);
                            out.push('\n');
                            i += 1;

                            let key_indent = body_indent;
                            let mut in_continuation = false;
                            while i < lines.len() {
                                let script_line = lines[i];
                                let script_trimmed = script_line.trim();
                                let script_indent = leading_spaces(script_line);
                                if !script_trimmed.is_empty() && script_indent <= key_indent {
                                    break;
                                }

                                if script_trimmed.is_empty()
                                    || is_shell_comment_line(script_trimmed)
                                {
                                    out.push_str(script_line);
                                } else {
                                    out.push_str(script_line);
                                    if !in_continuation {
                                        let line_cmd_count =
                                            estimate_simple_command_count_in_line(script_trimmed);
                                        if line_cmd_count > 0 {
                                            let mut labels = Vec::new();
                                            if command_index < step.commands.len() {
                                                let end = (command_index + line_cmd_count)
                                                    .min(step.commands.len());
                                                labels.extend(
                                                    step.commands[command_index..end]
                                                        .iter()
                                                        .map(format_command_kind),
                                                );
                                                command_index = end;
                                            }
                                            if labels.len() < line_cmd_count {
                                                let fallback =
                                                    classify_line_kinds_by_words(script_trimmed);
                                                labels.extend(
                                                    fallback
                                                        .into_iter()
                                                        .take(
                                                            line_cmd_count
                                                                .saturating_sub(labels.len()),
                                                        )
                                                        .map(|k| k.as_str().to_string()),
                                                );
                                            }
                                            if labels.is_empty() {
                                                labels.push(CmdKind::Other.as_str().to_string());
                                            }
                                            out.push_str(" --- ");
                                            out.push_str(&labels.join(" && "));
                                        }
                                    }
                                }
                                in_continuation = has_trailing_unescaped_backslash(script_trimmed);
                                out.push('\n');
                                i += 1;
                            }
                            continue;
                        }

                        out.push_str(body_line);
                        let line_cmd_count = estimate_simple_command_count_in_line(value);
                        if line_cmd_count > 0 {
                            let mut labels = Vec::new();
                            if command_index < step.commands.len() {
                                let end = (command_index + line_cmd_count).min(step.commands.len());
                                labels.extend(
                                    step.commands[command_index..end]
                                        .iter()
                                        .map(format_command_kind),
                                );
                                command_index = end;
                            }
                            if labels.len() < line_cmd_count {
                                let fallback = classify_line_kinds_by_words(value);
                                labels.extend(
                                    fallback
                                        .into_iter()
                                        .take(line_cmd_count.saturating_sub(labels.len()))
                                        .map(|k| k.as_str().to_string()),
                                );
                            }
                            if labels.is_empty() {
                                labels.push(CmdKind::Other.as_str().to_string());
                            }
                            out.push_str(" --- ");
                            out.push_str(&labels.join(" && "));
                        }
                        out.push('\n');
                        i += 1;
                        continue;
                    }
                }

                out.push_str(body_line);
                out.push('\n');
                i += 1;
            }
            continue;
        }

        if is_run_line(trimmed) {
            let Some(step) = nth_non_empty_step(analysis, step_index) else {
                out.push_str(line);
                out.push('\n');
                i += 1;
                continue;
            };
            step_index += 1;

            if is_block_run_line(trimmed) {
                out.push_str(line);
                out.push('\n');
                i += 1;

                let base_indent = leading_spaces(line);
                let mut command_index = 0usize;
                let mut in_continuation = false;
                while i < lines.len() {
                    let body_line = lines[i];
                    let body_trimmed = body_line.trim();
                    let body_indent = leading_spaces(body_line);
                    if !body_trimmed.is_empty() && body_indent <= base_indent {
                        break;
                    }

                    if body_trimmed.is_empty() || is_shell_comment_line(body_trimmed) {
                        out.push_str(body_line);
                    } else {
                        out.push_str(body_line);
                        if !in_continuation {
                            let line_cmd_count =
                                estimate_simple_command_count_in_line(body_trimmed);
                            let mut labels = Vec::new();
                            if line_cmd_count > 0 {
                                if command_index < step.commands.len() {
                                    let end =
                                        (command_index + line_cmd_count).min(step.commands.len());
                                    labels.extend(
                                        step.commands[command_index..end]
                                            .iter()
                                            .map(format_command_kind),
                                    );
                                    command_index = end;
                                }
                                if labels.len() < line_cmd_count {
                                    let fallback = classify_line_kinds_by_words(body_trimmed);
                                    labels.extend(
                                        fallback
                                            .into_iter()
                                            .take(line_cmd_count.saturating_sub(labels.len()))
                                            .map(|k| k.as_str().to_string()),
                                    );
                                }
                                if labels.is_empty() {
                                    labels.push(CmdKind::Other.as_str().to_string());
                                }
                                out.push_str(" --- ");
                                out.push_str(&labels.join(" && "));
                            }
                        }
                    }
                    in_continuation = has_trailing_unescaped_backslash(body_trimmed);
                    out.push('\n');
                    i += 1;
                }
                continue;
            }

            out.push_str(line);
            out.push_str(" --- ");
            out.push_str(&format_step_kinds(step));
            out.push('\n');
            i += 1;
            continue;
        }

        out.push_str(line);
        out.push('\n');
        i += 1;
    }

    out
}

fn is_uses_line(trimmed: &str) -> bool {
    trimmed.starts_with("- uses:") || trimmed.starts_with("uses:")
}

fn extract_uses_value(trimmed: &str) -> Option<String> {
    let rest = if let Some(rest) = trimmed.strip_prefix("- uses:") {
        rest
    } else {
        trimmed.strip_prefix("uses:")?
    };
    let value = rest.split('#').next().unwrap_or(rest).trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_mapping_key_value(trimmed: &str) -> Option<(&str, &str)> {
    let (key, value) = trimmed.split_once(':')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim()))
}

fn is_run_line(trimmed: &str) -> bool {
    trimmed.starts_with("- run:") || trimmed.starts_with("run:")
}

fn is_block_run_line(trimmed: &str) -> bool {
    trimmed.starts_with("- run: |")
        || trimmed.starts_with("- run: >")
        || trimmed.starts_with("run: |")
        || trimmed.starts_with("run: >")
}

fn is_shell_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with('#')
}

fn estimate_simple_command_count_in_line(line: &str) -> usize {
    classify_line_kinds_by_words(line).len()
}

fn classify_line_kinds_by_words(line: &str) -> Vec<CmdKind> {
    let segments = split_shell_segments(line);
    let mut out = Vec::new();
    for segment in segments {
        let trimmed = segment.trim();
        if trimmed.is_empty() || is_shell_comment_line(trimmed) {
            continue;
        }
        let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }
        let first = tokens[0];
        if matches!(first, "if" | "while" | "until") {
            if let Some(second) = tokens.get(1).copied()
                && !is_shell_control_keyword(second)
                && !is_simple_assignment_token(second)
            {
                out.push(classify_simple_command_from_words(&[second.to_string()]));
            }
            continue;
        }
        if is_simple_assignment_token(first) {
            if let Some(cmd) = command_in_assignment_substitution(first) {
                out.push(classify_simple_command_from_words(&[cmd]));
            }
            continue;
        }
        if is_shell_control_keyword(first) || is_simple_assignment_token(first) {
            continue;
        }
        let words = tokens.iter().map(|t| (*t).to_string()).collect::<Vec<_>>();
        out.push(classify_simple_command_from_words(&words));
    }
    out
}

fn split_shell_segments(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let chars = trimmed.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    let mut single_quote = false;
    let mut double_quote = false;
    let mut backtick_quote = false;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !double_quote && !backtick_quote {
            single_quote = !single_quote;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '"' && !single_quote && !backtick_quote {
            double_quote = !double_quote;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '`' && !single_quote && !double_quote {
            backtick_quote = !backtick_quote;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() {
            current.push(c);
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if !single_quote && !double_quote && !backtick_quote {
            if c == '|' && chars.get(i + 1) == Some(&'|') {
                segments.push(current.trim().to_string());
                current.clear();
                i += 2;
                continue;
            }
            if c == '&' && chars.get(i + 1) == Some(&'&') {
                segments.push(current.trim().to_string());
                current.clear();
                i += 2;
                continue;
            }
            if c == ';' || c == '|' {
                segments.push(current.trim().to_string());
                current.clear();
                i += 1;
                continue;
            }
        }
        current.push(c);
        i += 1;
    }
    if !current.trim().is_empty() {
        segments.push(current.trim().to_string());
    }
    segments
}

fn is_shell_control_keyword(token: &str) -> bool {
    matches!(
        token,
        "if" | "then" | "else" | "elif" | "fi" | "for" | "while" | "until" | "do" | "done"
    )
}

fn is_simple_assignment_token(token: &str) -> bool {
    let Some((name, _value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn command_in_assignment_substitution(token: &str) -> Option<String> {
    let (_name, value) = token.split_once('=')?;
    let body = value.strip_prefix("$(")?;
    let cmd = body
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ')' && *c != '"' && *c != '\'')
        .collect::<String>();
    if cmd.is_empty() { None } else { Some(cmd) }
}

fn has_trailing_unescaped_backslash(line: &str) -> bool {
    let trimmed = line.trim_end();
    if !trimmed.ends_with('\\') {
        return false;
    }
    let count = trimmed.chars().rev().take_while(|c| *c == '\\').count();
    count % 2 == 1
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn nth_non_empty_step(analysis: &AnalysisResult, mut idx: usize) -> Option<&StepPlan> {
    for step in &analysis.steps {
        if step.commands.is_empty() {
            continue;
        }
        if idx == 0 {
            return Some(step);
        }
        idx -= 1;
    }
    None
}

fn format_step_annotation(step: &StepPlan) -> String {
    let left = step
        .commands
        .iter()
        .map(|cmd| {
            cmd.attr
                .tools
                .first()
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string())
        })
        .collect::<Vec<_>>()
        .join(" && ");
    let right = step
        .commands
        .iter()
        .map(|cmd| {
            cmd.attr
                .kind
                .clone()
                .unwrap_or(CmdKind::Other)
                .as_str()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" && ");
    format!("{left} --- {right}")
}

fn format_step_kinds(step: &StepPlan) -> String {
    step.commands
        .iter()
        .map(format_command_kind)
        .collect::<Vec<_>>()
        .join(" && ")
}

fn format_command_kind(command: &CommandPlan) -> String {
    let kind = command.attr.kind.clone().unwrap_or(CmdKind::Other);
    format_kind_label(&kind, command.attr.special_action.as_ref())
}

fn format_kind_label(kind: &CmdKind, special: Option<&SpecialActionKind>) -> String {
    match special {
        Some(special) => format!("{} ({})", kind.as_str(), special.as_str()),
        None => kind.as_str().to_string(),
    }
}

pub fn classify_step_kind(step: &ActionsAst) -> Option<CmdKind> {
    let catalog = load_action_catalog().ok();
    match step {
        ActionsAst::UsesStep { uses, .. } => {
            Some(classify_uses_from_catalog(uses, catalog.as_ref()).0)
        }
        _ => None,
    }
}

fn collect_steps(
    id: AstId,
    arena: &AstArena,
    catalog: Option<&ActionCatalog>,
    out: &mut Vec<StepPlan>,
    unknown_uses: &mut Vec<String>,
) {
    match arena.get_actions(&id) {
        ActionsAst::Workflow { jobs, .. } => {
            for job in jobs {
                collect_steps(*job, arena, catalog, out, unknown_uses);
            }
        }
        ActionsAst::Job { steps, .. } => {
            for step_id in steps {
                out.push(analyze_step_with_catalog(*step_id, arena, catalog));
                if let ActionsAst::UsesStep { uses, .. } = arena.get_actions(step_id)
                    && is_unknown_uses(uses, catalog)
                {
                    unknown_uses.push(uses.clone());
                }
            }
        }
        _ => {}
    }
}

fn read_simple_command_words(cmd_id: AstId, arena: &AstArena) -> Option<Vec<String>> {
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

fn classify_simple_command_from_words(words: &[String]) -> CmdKind {
    match classify_simple_command(words) {
        Ok(kind) => map_rule_cmd_kind(kind),
        Err(_) => CmdKind::Other,
    }
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

fn classify_uses_from_catalog(
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

fn is_unknown_uses(uses: &str, catalog: Option<&ActionCatalog>) -> bool {
    let Some(catalog) = catalog else {
        return true;
    };
    if normalize_uses(uses).is_none() {
        return true;
    }
    action_entry_for_uses(uses, catalog).is_none()
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

#[cfg(test)]
mod tests {
    use super::{
        CmdKind, PlanOptions, analyze_actions, analyze_simple_command, annotate_yaml_with_cmd_kind,
        build_execution_plan, format_cmd_kind_lines,
    };
    use crate::action_catalog::{ActionCatalog, ActionCatalogEntry};
    use crate::actions_parser::actions_ast::{ActionsAst, RunsOn};
    use crate::actions_parser::arena::AstArena;
    use crate::actions_parser::sh_parser::sh_ast::{ListItem, SeparatorKind, ShAstNode};
    use std::collections::BTreeMap;

    fn alloc_simple_command(
        arena: &mut AstArena,
        words: &[&str],
    ) -> crate::actions_parser::arena::AstId {
        let argv = words
            .iter()
            .map(|w| arena.alloc_sh(ShAstNode::Word((*w).to_string())))
            .collect::<Vec<_>>();
        arena.alloc_sh(ShAstNode::SimpleCommand {
            assignments: vec![],
            argv,
            redirs: vec![],
        })
    }

    #[test]
    fn classify_simple_command_kinds() {
        let mut arena = AstArena::new();

        let cargo_test = alloc_simple_command(&mut arena, &["cargo", "test"]);
        let cargo_build = alloc_simple_command(&mut arena, &["cargo", "build"]);
        let npm_install = alloc_simple_command(&mut arena, &["npm", "install"]);
        let bracket_test = alloc_simple_command(&mut arena, &["[", "-n", "$X", "]"]);
        let echo = alloc_simple_command(&mut arena, &["echo", "ok"]);

        assert_eq!(
            analyze_simple_command(cargo_test, &arena).kind,
            Some(CmdKind::Test)
        );
        assert_eq!(
            analyze_simple_command(cargo_build, &arena).kind,
            Some(CmdKind::TestSetup)
        );
        assert_eq!(
            analyze_simple_command(npm_install, &arena).kind,
            Some(CmdKind::EnvSetup)
        );
        assert_eq!(
            analyze_simple_command(bracket_test, &arena).kind,
            Some(CmdKind::Assert)
        );
        assert_eq!(
            analyze_simple_command(echo, &arena).kind,
            Some(CmdKind::Other)
        );
    }

    #[test]
    fn uses_step_is_classified_and_unknown_collected() {
        let mut arena = AstArena::new();
        let run_cmd = alloc_simple_command(&mut arena, &["cargo", "test"]);
        let run_list = arena.alloc_sh(ShAstNode::List(vec![ListItem {
            body: run_cmd,
            sep: SeparatorKind::Seq,
        }]));

        let step_run = arena.alloc_actions(ActionsAst::RunStep {
            run: run_list,
            name: None,
            id: None,
            if_cond: None,
            env: None,
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let step_uses_known = arena.alloc_actions(ActionsAst::UsesStep {
            uses: "actions/checkout@v4".to_string(),
            name: None,
            id: None,
            if_cond: None,
            env: None,
            with: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let step_uses_unknown = arena.alloc_actions(ActionsAst::UsesStep {
            uses: "./.github/actions/setup".to_string(),
            name: None,
            id: None,
            if_cond: None,
            env: None,
            with: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let job = arena.alloc_actions(ActionsAst::Job {
            name: None,
            runs_on: RunsOn::String("ubuntu-latest".to_string()),
            steps: vec![step_run, step_uses_known, step_uses_unknown],
            needs: None,
            env: None,
            defaults: None,
            permissions: None,
            if_cond: None,
            strategy: None,
            container: None,
            services: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let on = arena.alloc_actions(ActionsAst::OnString("push".to_string()));
        let root = arena.alloc_actions(ActionsAst::Workflow {
            name: None,
            run_name: None,
            jobs: vec![job],
            on,
            env: None,
            defaults: None,
            permissions: None,
            concurrency: None,
        });

        let analysis = analyze_actions(root, &arena);
        let uses_known = analysis.steps[1].commands[0].attr.kind.clone();
        let uses_unknown = analysis.steps[2].commands[0].attr.kind.clone();

        assert_eq!(uses_known, Some(CmdKind::EnvSetup));
        assert_eq!(uses_unknown, Some(CmdKind::Other));
        assert_eq!(
            analysis.unknown_uses,
            vec!["./.github/actions/setup".to_string()]
        );
    }

    #[test]
    fn uses_step_shell_inputs_are_classified_as_commands() {
        let mut arena = AstArena::new();
        let mut with = BTreeMap::new();
        with.insert("command".to_string(), "echo hello\ncargo test".to_string());
        let step = arena.alloc_actions(ActionsAst::UsesStep {
            uses: "nick-fields/retry@v3".to_string(),
            name: None,
            id: None,
            if_cond: None,
            env: None,
            with: Some(with),
            timeout_minutes: None,
            continue_on_error: None,
        });

        let mut catalog: ActionCatalog = ActionCatalog::new();
        catalog.insert(
            "nick-fields/retry".to_string(),
            ActionCatalogEntry {
                required_tools: vec![],
                shell_inputs: vec!["command".to_string()],
                cmd_kind: Some("Other".to_string()),
                special_action: None,
                confidence: None,
                notes: None,
            },
        );

        let plan = super::analyze_step_with_catalog(step, &arena, Some(&catalog));
        assert_eq!(plan.commands.len(), 3);
        assert_eq!(plan.commands[0].attr.kind, Some(CmdKind::Other));
        assert_eq!(plan.commands[1].attr.kind, Some(CmdKind::Other));
        assert_eq!(plan.commands[2].attr.kind, Some(CmdKind::Test));
    }

    #[test]
    fn build_execution_plan_filters_only_env_and_other() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(1),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::EnvSetup),
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(12),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };

        let plan = build_execution_plan(
            &analysis,
            &PlanOptions {
                include_env_setup: false,
                include_other: false,
            },
        );
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].kind, CmdKind::Test);
    }

    #[test]
    fn format_lines_join_commands_and_kinds() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(1),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::TestSetup),
                            tools: vec!["cargo build".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            tools: vec!["cargo test".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };

        let lines = format_cmd_kind_lines(&analysis);
        assert_eq!(
            lines,
            vec!["cargo build && cargo test --- TestSetup && Test".to_string()]
        );
    }

    #[test]
    fn annotate_yaml_keeps_unrelated_lines() {
        let analysis = super::AnalysisResult {
            steps: vec![
                super::StepPlan {
                    step_id: crate::actions_parser::arena::AstId(1),
                    commands: vec![super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::EnvSetup),
                            special_action: Some(super::SpecialActionKind::Checkout),
                            tools: vec!["actions/checkout@v4".to_string()],
                            ..super::Attr::default()
                        },
                    }],
                },
                super::StepPlan {
                    step_id: crate::actions_parser::arena::AstId(2),
                    commands: vec![
                        super::CommandPlan {
                            ast_id: crate::actions_parser::arena::AstId(11),
                            attr: super::Attr {
                                kind: Some(CmdKind::TestSetup),
                                tools: vec!["cargo build".to_string()],
                                ..super::Attr::default()
                            },
                        },
                        super::CommandPlan {
                            ast_id: crate::actions_parser::arena::AstId(12),
                            attr: super::Attr {
                                kind: Some(CmdKind::Test),
                                tools: vec!["cargo test".to_string()],
                                ..super::Attr::default()
                            },
                        },
                    ],
                },
            ],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"name: CI
jobs:
  test:
    steps:
      - uses: actions/checkout@v4
      - name: build and test
        run: cargo build && cargo test
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);

        assert!(annotated.contains("name: CI\n"));
        assert!(annotated.contains("jobs:\n"));
        assert!(annotated.contains("- uses: actions/checkout@v4 --- EnvSetup (Checkout)\n"));
        assert!(annotated.contains("run: cargo build && cargo test --- TestSetup && Test\n"));
    }

    #[test]
    fn annotate_yaml_prints_multiline_run_per_command() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(2),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::TestSetup),
                            tools: vec!["cargo build".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(12),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            tools: vec!["cargo test".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - run: |
          cargo build
          cargo test
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);

        assert!(annotated.contains("      - run: |\n"));
        assert!(annotated.contains("          cargo build --- TestSetup\n"));
        assert!(annotated.contains("          cargo test --- Test\n"));
    }

    #[test]
    fn annotate_yaml_prints_uses_shell_input_on_script_lines() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(1),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            tools: vec!["reactivecircus/android-emulator-runner@v2".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            tools: vec!["adb install -r app.apk".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(12),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            tools: vec!["cargo test".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - uses: reactivecircus/android-emulator-runner@v2
        with:
          script: |
            adb install -r app.apk
            cargo test
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);

        assert!(
            annotated.contains("- uses: reactivecircus/android-emulator-runner@v2 --- Other\n")
        );
        assert!(annotated.contains("            adb install -r app.apk --- Other\n"));
        assert!(annotated.contains("            cargo test --- Test\n"));
    }

    #[test]
    fn annotate_yaml_skips_comment_lines_in_multiline_run() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(2),
                commands: vec![super::CommandPlan {
                    ast_id: crate::actions_parser::arena::AstId(12),
                    attr: super::Attr {
                        kind: Some(CmdKind::Test),
                        tools: vec!["cargo test".to_string()],
                        ..super::Attr::default()
                    },
                }],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - name: Run tests
        run: |
          # 単体テスト
          cargo test
"#;

        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
        assert!(annotated.contains("          # 単体テスト\n"));
        assert!(annotated.contains("          cargo test --- Test\n"));
    }

    #[test]
    fn annotate_yaml_skips_control_and_assignment_lines() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(2),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            tools: vec!["[".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(12),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            tools: vec!["cargo test".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - run: |
          failed=false
          if [ "$failed" = true ]; then
            cargo test
          fi
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
        assert!(annotated.contains("          failed=false\n"));
        assert!(annotated.contains("          if [ \"$failed\" = true ]; then --- Other\n"));
        assert!(annotated.contains("            cargo test --- Test\n"));
        assert!(annotated.contains("          fi\n"));
    }

    #[test]
    fn annotate_yaml_multiline_keeps_other_for_simple_commands() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(2),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            tools: vec!["set -x".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(12),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            tools: vec!["cargo test".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - run: |
          set -x
          cargo test
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
        assert!(annotated.contains("          set -x --- Other\n"));
        assert!(annotated.contains("          cargo test --- Test\n"));
    }

    #[test]
    fn annotate_yaml_multiline_line_continuation_is_single_command() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(2),
                commands: vec![super::CommandPlan {
                    ast_id: crate::actions_parser::arena::AstId(11),
                    attr: super::Attr {
                        kind: Some(CmdKind::Other),
                        tools: vec!["sudo apt-get install bison zsh".to_string()],
                        ..super::Attr::default()
                    },
                }],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - run: |
          sudo apt-get install \
            bison \
            zsh
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
        assert!(annotated.contains("          sudo apt-get install \\ --- Other\n"));
        assert!(annotated.contains("            bison \\\n"));
        assert!(annotated.contains("            zsh\n"));
        assert!(!annotated.contains("            bison \\ --- "));
        assert!(!annotated.contains("            zsh --- "));
    }

    #[test]
    fn annotate_yaml_multiline_assignment_substitution_gets_annotation() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(2),
                commands: vec![super::CommandPlan {
                    ast_id: crate::actions_parser::arena::AstId(11),
                    attr: super::Attr {
                        kind: Some(CmdKind::Other),
                        tools: vec!["curl -s".to_string()],
                        ..super::Attr::default()
                    },
                }],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - run: |
          LATEST_RELEASE=$(curl -s -H "Authorization: token $GITHUB_TOKEN" \
            "https://api.github.com/repos/${{ github.repository }}/releases/latest" \
            | jq -r '.tag_name')
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
        assert!(annotated.contains("          LATEST_RELEASE=$(curl -s -H "));
        assert!(annotated.contains("--- Other\n"));
    }

    #[test]
    fn format_lines_include_special_action_kind() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(1),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::EnvSetup),
                            special_action: Some(super::SpecialActionKind::Checkout),
                            tools: vec!["actions/checkout@v4".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            special_action: Some(super::SpecialActionKind::ArtifactUpload),
                            tools: vec!["actions/upload-artifact@v4".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };

        let lines = format_cmd_kind_lines(&analysis);
        assert_eq!(
            lines,
            vec![
                "actions/checkout@v4 && actions/upload-artifact@v4 --- EnvSetup (Checkout) && Other (ArtifactUpload)".to_string()
            ]
        );
    }
}
