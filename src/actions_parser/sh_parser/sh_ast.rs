#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShNodeId(pub u32);

#[derive(Debug, Clone)]
pub struct ShProgram {
    pub list: ShNodeId,
}
#[derive(Debug, Clone)]
pub struct ListItem {
    pub body: ShNodeId,
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
    pub body: ShNodeId,
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
        first: ShNodeId,
        rest: Vec<AndOrItem>,
    },
    Pipeline {
        first: ShNodeId,
        rest: Vec<ShNodeId>,
    },
    SimpleCommand {
        assignments: Vec<ShNodeId>,
        argv: Vec<ShNodeId>,
        redirs: Vec<ShNodeId>,
    },

    If {
        cond: ShNodeId,
        then_part: ShNodeId,
        else_part: Option<ShNodeId>,
    },

    While {
        cond: ShNodeId,
        body: ShNodeId,
    },

    For {
        var: ShNodeId,
        items: Vec<ShNodeId>,
        body: ShNodeId,
    },
    FunctionDef {
        name: ShNodeId,
        body: ShNodeId,
    },
    Subshell {
        body: ShNodeId,
    },
    Group {
        body: ShNodeId,
    },

    Word(String),
    Assignment(String),
    Redir {
        op: String,
        body: String,
    },

    Unknown,
}
