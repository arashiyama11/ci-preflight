#![allow(dead_code)]

use crate::action_catalog::{ActionCatalog, load_action_catalog, shell_input_keys_for_uses};
use crate::actions_parser::actions_ast::ActionsAst;
use crate::actions_parser::arena::{AstArena, AstId};
use std::collections::BTreeMap;

mod annotate;
mod classify;
pub use annotate::annotate_yaml_with_cmd_kind;
use classify::{
    classify_simple_command_from_words, classify_uses_from_catalog, extract_simple_commands,
    is_unknown_uses, parse_shell_command_words, read_simple_command_words,
};

#[cfg(test)]
mod tests;

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
