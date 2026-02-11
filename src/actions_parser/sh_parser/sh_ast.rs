use crate::actions_parser::arena::AstId;

#[derive(Debug, Clone)]
pub struct ShProgram {
    pub list: AstId,
}
#[derive(Debug, Clone)]
pub struct ListItem {
    pub body: AstId,
    pub sep: SeparatorKind,
}

#[derive(Debug, Clone)]
pub enum SeparatorKind {
    Seq,
    Background,
}

#[derive(Debug, Clone)]
pub struct AndOrItem {
    pub op: AndOrOp,
    pub body: AstId,
}

#[derive(Debug, Clone)]
pub enum AndOrOp {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub enum ShAstNode {
    List(Vec<ListItem>),
    AndOr {
        first: AstId,
        rest: Vec<AndOrItem>,
    },
    Pipeline {
        first: AstId,
        rest: Vec<AstId>,
    },
    SimpleCommand {
        assignments: Vec<AstId>,
        argv: Vec<AstId>,
        redirs: Vec<AstId>,
    },

    If {
        cond: AstId,
        then_part: AstId,
        else_part: Option<AstId>,
    },

    While {
        cond: AstId,
        body: AstId,
    },

    For {
        var: AstId,
        items: Vec<AstId>,
        body: AstId,
    },
    FunctionDef {
        name: AstId,
        body: AstId,
    },
    Subshell {
        body: AstId,
    },
    Group {
        body: AstId,
    },

    Word(String),
    Assignment(String),
    Redir {
        op: String,
        body: String,
    },
    Unknown,
}
