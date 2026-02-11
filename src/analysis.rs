use crate::actions_parser::actions_ast::ActionsAst;
use crate::actions_parser::arena::{AstArena, AstId};
use crate::actions_parser::sh_parser::sh_ast::ShAstNode;

#[derive(Clone, Debug)]
pub enum CmdKind {
    EnvSetup,
    TestSetup,
    Test,
    Other,
}

#[derive(Clone, Debug, Default)]
pub struct Attr {
    pub kind: Option<CmdKind>,
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
    let mut result = AnalysisResult::default();
    collect_steps(root, arena, &mut result.steps);
    result
}

pub fn analyze_step(step_id: AstId, arena: &AstArena) -> StepPlan {
    let commands = match arena.get_actions(&step_id) {
        ActionsAst::RunStep { run, .. } => analyze_run_step(*run, arena),
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
        confidence: 0.5,
        ..Attr::default()
    };
    if let ShAstNode::SimpleCommand { argv, .. } = arena.get_sh(cmd_id)
        && let Some(first) = argv.first()
        && let ShAstNode::Word(word) = arena.get_sh(*first)
    {
        let cmd = word.trim();
        if !cmd.is_empty() {
            attr.tools.push(cmd.to_string());
            attr.confidence = 0.9;
        }
    }
    attr
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

pub fn classify_step_kind(step: &ActionsAst) -> Option<CmdKind> {
    match step {
        ActionsAst::RunStep { .. } => Some(CmdKind::Other),
        ActionsAst::UsesStep { .. } => Some(CmdKind::EnvSetup),
        _ => None,
    }
}

fn collect_steps(id: AstId, arena: &AstArena, out: &mut Vec<StepPlan>) {
    match arena.get_actions(&id) {
        ActionsAst::Workflow { jobs, .. } => {
            for job in jobs {
                collect_steps(*job, arena, out);
            }
        }
        ActionsAst::Job { steps, .. } => {
            for step_id in steps {
                out.push(analyze_step(*step_id, arena));
            }
        }
        _ => {}
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
        ShAstNode::Subshell { body } | ShAstNode::Group { body } => {
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::Word(_)
        | ShAstNode::Assignment(_)
        | ShAstNode::Redir { .. }
        | ShAstNode::Unknown => {}
    }
}
