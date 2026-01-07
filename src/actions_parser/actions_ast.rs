use super::sh_parser::sh_ast::{ShAstNode, ShProgram};

#[derive(Clone, PartialEq, PartialOrd, Debug, Eq, Ord, Hash)]
pub struct ActionsAstId(pub u32);

// https://docs.github.com/ja/actions/reference/workflows-and-actions/workflow-syntax
#[derive(Clone, Debug)]
pub enum ActionsAst {
    // ignore on
    Workflow {
        name: String,
        jobs: Vec<ActionsAstId>,
        on: ActionsAstId,
    },
    OnString(String),
    OnArray(Vec<String>),
    OnObject,
    Job {
        runs_on: String,
        steps: Vec<ActionsAstId>,
    },
    RunStep{
        run: String,
    },
    UsesStep {
        uses: String
    },
    Sh(ShAstNode),
}
