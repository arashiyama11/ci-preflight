use crate::analyzer::{AnalysisResult, CmdKind};
use crate::parser::arena::AstId;

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
#[allow(dead_code)]
pub struct PlannedCommand {
    pub ast_id: AstId,
    pub kind: CmdKind,
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
