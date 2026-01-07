use core::str;

#[derive(PartialEq, Debug, Eq)]
pub struct Marker {
    index: usize,
    line: usize,
    col: usize,
    filename: String,
}

#[derive(Clone, PartialEq, Debug, Eq)]
pub enum Safery {
    SAFE,
    CAUTION,
    DANGEROUS,
}

impl Marker {
    fn new(index: usize, line: usize, col: usize, filename: &str) -> Marker {
        Marker {
            index,
            line,
            col,
            filename: String::from(filename),
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn col(&self) -> usize {
        self.col
    }
}

struct Workflow {
    name: String,
    jobs: Vec<Job>,
}

struct Job {
    name: String,
    runs_on: String,
    steps: Vec<Step>,
}

struct Step {
    name: Option<String>,
    uses: Option<String>,
    run: ShProgram,
}
struct ShProgram {
    stmts: Vec<Stmt>,
}

struct Word {
    raw: String,
}

enum RedirOp {}

struct Redir {
    fd: Option<i32>,
    op: RedirOp,
    target: Word, // 中身は評価しない
}

struct SimpleCmd {
    assigns: Vec<(String, Word)>,
    argv: Vec<Word>,
    redirs: Vec<Redir>,
}

enum Stmt {
    SimpleCmd(SimpleCmd),
    Seq(Vec<Stmt>),
    And(Box<Stmt>, Box<Stmt>),
    Or(Box<Stmt>, Box<Stmt>),
    Pipeline(Vec<Stmt>),

    If {
        cond: Vec<Stmt>,
        then: Vec<Stmt>,
        else_: Option<Vec<Stmt>>,
    },
    For {
        var: String,
        items: Vec<Word>, // 評価しない
        body: Vec<Stmt>,
    },
    While {
        cond: Vec<Stmt>,
        body: Vec<Stmt>,
    },
    FunctionDef {
        name: String,
        body: Vec<Stmt>,
    },
    Subshell(Vec<Stmt>),
    Group(Vec<Stmt>),

    Unknown,
}

/*Stmt =
| SimpleCmd
| Seq
| And
| Or
| If
| For
| FunctionDef
| FunctionCall
| Subshell
| Unknown */
