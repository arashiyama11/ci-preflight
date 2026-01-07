use super::sh_ast::{AndOrItem, AndOrOp, ListItem, SeparatorKind, ShAstNode, ShNodeId, ShProgram};
use super::{
    arena::ShAstArena,
    sh_token::{ShToken, ShTokenKind, WordKind},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("InternalError $0")]
    InternalError(&'static str),
    #[error("UnexpectedToken $0")]
    UnexpectedToken(ShToken),
    #[error("Unexpected EOF")]
    UnexpectedEof,
}
pub struct ShParser {
    pub input: Vec<ShToken>,
    pub src: String,
    pub arena: ShAstArena,
    pos: usize,
    errors: Vec<ParseError>,
}

impl ShParser {
    pub fn new(tokens: Vec<ShToken>, src: String) -> ShParser {
        ShParser {
            input: tokens,
            src,
            pos: 0,
            arena: ShAstArena::new(),
            errors: vec![],
        }
    }

    fn expect_current_word(&self, word: &[&str]) -> Result<(), ParseError> {
        let tok = &self.input[self.pos];
        match tok.kind {
            ShTokenKind::Word(_) if word.contains(&tok.text(&self.src)) => Ok(()),
            _ => {
                panic!(
                    "Unexpected current word: expected: {:?}, actual: {:?}",
                    word, tok
                );
            } //Err(ParseError::UnexpectedToken(tok.clone())),
        }
    }

    fn expect_current_token(&self, kind: ShTokenKind) -> Result<(), ParseError> {
        if self.input[self.pos].kind == kind {
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken(self.input[self.pos].clone()))
        }
    }

    fn expect_next_token(&self, kind: ShTokenKind) -> Result<(), ParseError> {
        if self.input.len() >= self.pos + 1 {
            Err(ParseError::UnexpectedEof)
        } else if self.input[self.pos + 1].kind == kind {
            Ok(())
        } else {
            panic!();
            Err(ParseError::UnexpectedToken(self.input[self.pos].clone()))
        }
    }

    fn next_token(&self) -> Option<&ShToken> {
        self.input.get(self.pos + 1)
    }

    fn require_end_of_line(&self) -> Result<(), ParseError> {
        if self
            .input
            .get(self.pos)
            .is_some_and(|t| t.kind == ShTokenKind::NewLine)
        {
            Ok(())
        } else {
            eprintln!("require_end_of_line: unexpected end of line");
            Err(ParseError::InternalError("require_end_of_line"))
        }
    }

    pub fn parse_program(&mut self) -> Result<ShProgram, ParseError> {
        loop {
            match self.input[self.pos].kind {
                ShTokenKind::NewLine | ShTokenKind::Comment => {
                    self.pos += 1;
                    continue;
                }
                _ => break,
            }
        }
        let list = self.parse_list(&[], &[ShTokenKind::Eof])?;
        Ok(ShProgram { list })
    }

    // ;&\n dで区切られたcommand列のparse
    fn parse_list(
        &mut self,
        end_words: &[&str],
        end_tokens: &[ShTokenKind],
    ) -> Result<ShNodeId, ParseError> {
        let mut items: Vec<ListItem> = vec![];
        let mut should_break = false;

        loop {
            if self.input[self.pos].kind == ShTokenKind::Eof {
                break;
            }

            while matches!(
                self.input[self.pos].kind,
                ShTokenKind::NewLine | ShTokenKind::Comment
            ) {
                self.pos += 1;
            }

            if end_words.contains(&self.input[self.pos].text(&self.src)) {
                break;
            }

            let body = self.parse_and_or(end_words, end_tokens)?;

            while self
                .input
                .get(self.pos)
                .is_some_and(|t| t.kind == ShTokenKind::Comment)
            {
                self.pos += 1;
            }

            let tok = &self.input[self.pos];

            if end_tokens.contains(&tok.kind) {
                should_break = true;
            }

            println!(
                "parse_list tok: {:?} {} {:?}",
                tok,
                tok.text(&self.src),
                end_words
            );

            let sep = match &tok.kind {
                ShTokenKind::NewLine | ShTokenKind::SemiColon | ShTokenKind::Eof => {
                    SeparatorKind::Seq
                }
                ShTokenKind::Word(_) if end_words.contains(&tok.text(&self.src)) => {
                    should_break = true;
                    SeparatorKind::Seq
                }
                ShTokenKind::BackgroundExec => SeparatorKind::Background,
                _ => {
                    if end_tokens.contains(&tok.kind) {
                        should_break = true;
                        SeparatorKind::Seq
                    } else {
                        panic!()
                    }
                }
            };

            items.push(ListItem { body, sep });
            if should_break {
                break;
            } else {
                self.pos += 1;
            }
        }

        let node = ShAstNode::List(items);
        Ok(self.arena.alloc(node))
    }

    // sepで終端
    fn parse_and_or(
        &mut self,
        end_words: &[&str],
        end_tokens: &[ShTokenKind],
    ) -> Result<ShNodeId, ParseError> {
        let mut first: Option<ShNodeId> = None;
        let mut rest: Vec<AndOrItem> = vec![];

        loop {
            let tok = &self.input[self.pos];
            println!("and_or: {:?} {}", tok, tok.text(&self.src));
            if end_tokens.contains(&tok.kind) {
                break;
            }

            if tok.kind == ShTokenKind::Comment {
                self.pos += 1;
                continue;
            }

            if matches!(tok.kind, ShTokenKind::Word(_)) && end_words.contains(&tok.text(&self.src))
            {
                println!("break since word");
                break;
            }

            if matches!(
                tok.kind,
                ShTokenKind::NewLine
                    | ShTokenKind::SemiColon
                    | ShTokenKind::BackgroundExec
                    | ShTokenKind::Eof
            ) {
                break;
            }

            if first == None {
                first = Some(self.parse_pipeline(end_words, end_tokens)?);
            } else {
                let op = match self.input[self.pos].kind {
                    ShTokenKind::And => AndOrOp::And,
                    ShTokenKind::Or => AndOrOp::Or,
                    _ => break,
                };
                self.pos += 1;

                let body = self.parse_pipeline(end_words, end_tokens)?;

                rest.push(AndOrItem { op, body })
            }
        }

        println!("first: {:?}", first);
        let first = first.ok_or(ParseError::InternalError("parse_and_or"))?;

        let node = ShAstNode::AndOr { first, rest };
        Ok(self.arena.alloc(node))
    }

    // and or sepで終端
    fn parse_pipeline(
        &mut self,
        end_words: &[&str],
        end_tokens: &[ShTokenKind],
    ) -> Result<ShNodeId, ParseError> {
        let mut first: Option<ShNodeId> = None;
        let mut rest: Vec<ShNodeId> = vec![];

        loop {
            let tok = &self.input[self.pos];
            if end_tokens.contains(&tok.kind) {
                break;
            }

            if tok.kind == ShTokenKind::Comment {
                self.pos += 1;
                continue;
            }

            if matches!(tok.kind, ShTokenKind::Word(_)) && end_words.contains(&tok.text(&self.src))
            {
                break;
            }

            if matches!(
                tok.kind,
                ShTokenKind::NewLine
                    | ShTokenKind::SemiColon
                    | ShTokenKind::BackgroundExec
                    | ShTokenKind::Eof
            ) {
                break;
            }

            if first == None {
                first = Some(self.parse_command(end_words, end_tokens)?);
            } else {
                if self.input[self.pos].kind != ShTokenKind::Pipe {
                    break;
                }
                self.pos += 1;
                let body = self.parse_command(end_words, end_tokens)?;
                rest.push(body)
            }
        }

        let first = first.ok_or(ParseError::InternalError("parse_pipeline"))?;
        let node = ShAstNode::Pipeline { first, rest };
        Ok(self.arena.alloc(node))
    }

    // pipe and or sep で終端
    fn parse_command(
        &mut self,
        end_words: &[&str],
        end_tokens: &[ShTokenKind],
    ) -> Result<ShNodeId, ParseError> {
        let tok = &self.input[self.pos];
        let id = match &tok.kind {
            ShTokenKind::Word(word_kind) => {
                match word_kind {
                    // 制御構文, 関数
                    WordKind::Name => match tok.text(&self.src) {
                        "if" => self.parse_if(),
                        "for" => self.parse_for(),
                        "while" | "until" => self.parse_while(),
                        _ => {
                            if self
                                .next_token()
                                .is_some_and(|t| t.kind == ShTokenKind::LParen)
                                && self
                                    .input
                                    .get(self.pos + 2)
                                    .is_some_and(|t| t.kind == ShTokenKind::RParen)
                            {
                                self.parse_function()
                            } else {
                                self.parse_simple_command(end_words, end_tokens)
                            }
                        }
                    },

                    // 代入、SimpleCommand
                    WordKind::Word | WordKind::Path => {
                        self.parse_simple_command(end_words, end_tokens)
                    }
                }
            }

            ShTokenKind::LParen => self.parse_subshell(),
            ShTokenKind::LBrace => self.parse_group(),
            _ => {
                eprintln!("reach {:?}", tok);
                return Err(ParseError::UnexpectedToken(tok.clone()));
            }
        };

        id
    }

    fn parse_if(&mut self) -> Result<ShNodeId, ParseError> {
        self.expect_current_word(&["if", "elif"])?;
        self.pos += 1;
        let cond = self.parse_list(&["then"], &[])?;
        println!("!!!!!parse cond {:?}", self.input[self.pos]);
        println!("{}", self.input[self.pos].text(&self.src));
        self.expect_current_word(&["then"])?;
        self.pos += 1;

        let then_part: ShNodeId = self.parse_list(&["else", "fi", "elif"], &[])?;

        println!("!!!!!parse then block {:?}", self.input[self.pos]);
        println!("{}", self.input[self.pos].text(&self.src));

        let else_part: Option<ShNodeId> = match self.input[self.pos].text(&self.src) {
            "fi" => None,
            "else" => {
                self.pos += 1;
                Some(self.parse_list(&["fi"], &[])?)
            }
            "elif" => Some(self.parse_if()?),
            _ => {
                if self.input[self.pos].kind == ShTokenKind::Eof {
                    None
                } else {
                    panic!()
                }
            }
        };

        self.pos += 1;

        let node = ShAstNode::If {
            cond,
            then_part,
            else_part,
        };
        Ok(self.arena.alloc(node))
    }

    fn parse_for(&mut self) -> Result<ShNodeId, ParseError> {
        self.expect_current_word(&["for"])?;
        self.pos += 1;

        let var_tok = self.input.get(self.pos).ok_or(ParseError::UnexpectedEof)?;
        let ShTokenKind::Word(_) = var_tok.kind else {
            return Err(ParseError::UnexpectedToken(var_tok.clone()));
        };
        let var = self
            .arena
            .alloc(ShAstNode::Word(var_tok.text(&self.src).to_string()));
        self.pos += 1;

        let mut items: Vec<ShNodeId> = vec![];
        if self.input.get(self.pos).is_some_and(|t| {
            t.kind == ShTokenKind::Word(WordKind::Name) && t.text(&self.src) == "in"
        }) {
            self.pos += 1;
            loop {
                let tok = self.input.get(self.pos).ok_or(ParseError::UnexpectedEof)?;
                match tok.kind {
                    ShTokenKind::Word(_) => {
                        let node = ShAstNode::Word(tok.text(&self.src).to_string());
                        items.push(self.arena.alloc(node));
                        self.pos += 1;
                    }
                    ShTokenKind::SemiColon | ShTokenKind::NewLine | ShTokenKind::BackgroundExec => {
                        break;
                    }
                    _ => return Err(ParseError::UnexpectedToken(tok.clone())),
                }
            }
        }

        let sep = self.input.get(self.pos).ok_or(ParseError::UnexpectedEof)?;
        match sep.kind {
            ShTokenKind::SemiColon | ShTokenKind::NewLine | ShTokenKind::BackgroundExec => {
                self.pos += 1;
            }
            _ => return Err(ParseError::UnexpectedToken(sep.clone())),
        }

        while self
            .input
            .get(self.pos)
            .is_some_and(|t| matches!(t.kind, ShTokenKind::NewLine | ShTokenKind::Comment))
        {
            self.pos += 1;
        }

        self.expect_current_word(&["do"])?;
        self.pos += 1;
        let body = self.parse_list(&["done"], &[])?;
        self.expect_current_word(&["done"])?;
        self.pos += 1;

        let node = ShAstNode::For { var, items, body };
        Ok(self.arena.alloc(node))
    }

    fn parse_while(&mut self) -> Result<ShNodeId, ParseError> {
        self.expect_current_word(&["while", "until"])?;
        self.pos += 1;
        let cond = self.parse_list(&["do"], &[])?;
        self.expect_current_word(&["do"])?;
        self.pos += 1;
        let body = self.parse_list(&["done"], &[])?;
        self.expect_current_word(&["done"])?;
        self.pos += 1;

        let node = ShAstNode::While { cond, body };
        Ok(self.arena.alloc(node))
    }

    fn parse_function(&mut self) -> Result<ShNodeId, ParseError> {
        let name_tok = self.input.get(self.pos).ok_or(ParseError::UnexpectedEof)?;
        let ShTokenKind::Word(_) = name_tok.kind else {
            return Err(ParseError::UnexpectedToken(name_tok.clone()));
        };
        let name = self
            .arena
            .alloc(ShAstNode::Word(name_tok.text(&self.src).to_string()));

        self.pos += 1;
        self.expect_current_token(ShTokenKind::LParen)?;
        self.pos += 1;
        self.expect_current_token(ShTokenKind::RParen)?;
        self.pos += 1;

        while self
            .input
            .get(self.pos)
            .is_some_and(|t| matches!(t.kind, ShTokenKind::NewLine | ShTokenKind::Comment))
        {
            self.pos += 1;
        }

        let body = match self.input.get(self.pos).map(|t| &t.kind) {
            Some(ShTokenKind::Word(_)) => match self.input[self.pos].text(&self.src) {
                "if" => self.parse_if()?,
                "for" => self.parse_for()?,
                "while" | "until" => self.parse_while()?,
                _ => return Err(ParseError::UnexpectedToken(self.input[self.pos].clone())),
            },
            Some(ShTokenKind::LParen) => self.parse_subshell()?,
            Some(ShTokenKind::LBrace) => self.parse_group()?,
            Some(_) => return Err(ParseError::UnexpectedToken(self.input[self.pos].clone())),
            None => return Err(ParseError::UnexpectedEof),
        };

        let node = ShAstNode::FunctionDef { name, body };
        Ok(self.arena.alloc(node))
    }

    fn parse_simple_command(
        &mut self,
        end_words: &[&str],
        end_tokens: &[ShTokenKind],
    ) -> Result<ShNodeId, ParseError> {
        let mut assignments: Vec<ShNodeId> = vec![];
        let mut argv: Vec<ShNodeId> = vec![];
        let mut redirs: Vec<ShNodeId> = vec![];
        let mut heredoc_op: Option<String> = None;
        let mut heredoc_delim: Option<&str> = None;
        let mut heredoc_place: Option<usize> = None;
        let mut pending_io_number: Option<String> = None;
        loop {
            let tok = match self.input.get(self.pos) {
                Some(t) => t,
                None => break,
            };

            if end_tokens.contains(&tok.kind) {
                break;
            }

            let s = tok.text(&self.src);

            println!("simple: {:?} {}", tok, s);
            match &tok.kind {
                ShTokenKind::Word(_) => {
                    if is_digits(s) {
                        if let Some(next) = self.next_token() {
                            if next.kind == ShTokenKind::Redir
                                && tok.span.index + tok.span.len == next.span.index
                            {
                                pending_io_number = Some(s.to_string());
                                self.pos += 1;
                                continue;
                            }
                        }
                    }
                    if s.contains('=') {
                        let node = ShAstNode::Assignment(s.to_string());
                        assignments.push(self.arena.alloc(node));
                    } else if end_words.contains(&s) {
                        println!("break end_word");
                        break;
                    } else {
                        let node = ShAstNode::Word(s.to_string());
                        argv.push(self.arena.alloc(node));
                    }
                }

                ShTokenKind::Redir => {
                    let mut op = s.to_string();
                    if let Some(io) = pending_io_number.take() {
                        op = format!("{io}{op}");
                    }
                    if s == "<<" || s == "<<-" {
                        heredoc_op = Some(op);
                        heredoc_delim = Some(self.input[self.pos + 1].text(&self.src));
                        heredoc_place = Some(redirs.len());
                        self.pos += 1;
                    } else {
                        let body: &str = {
                            let Some(next) = self.next_token() else {
                                return Err(ParseError::UnexpectedEof);
                            };

                            let ShTokenKind::Word(_) = next.kind else {
                                return Err(ParseError::UnexpectedEof);
                            };
                            next.text(&self.src)
                        };

                        let node = ShAstNode::Redir {
                            op,
                            body: body.to_string(),
                        };
                        self.pos += 1;
                        redirs.push(self.arena.alloc(node));
                    }
                }

                ShTokenKind::Comment => {
                    self.pos += 1;
                    continue;
                }

                ShTokenKind::Eof
                | ShTokenKind::NewLine
                | ShTokenKind::SemiColon
                | ShTokenKind::BackgroundExec
                | ShTokenKind::Pipe
                | ShTokenKind::And
                | ShTokenKind::Or => {
                    break;
                }
                _ => return panic!(), // Err(ParseError::UnexpectedToken(tok.clone())),
            }

            self.pos += 1;
        }

        if let Some(delim) = heredoc_delim {
            let start = self.input[self.pos].span.index + 1;
            let mut i = self.pos;
            loop {
                i += 1;
                match self.input.get(i) {
                    Some(t) if t.text(&self.src) == delim => break,
                    None => return Err(ParseError::UnexpectedEof),
                    _ => continue,
                };
            }
            let end = self.input[i].span.index - 1;
            let body = self.src[start..end].to_string();
            let op = heredoc_op.unwrap();
            let node = ShAstNode::Redir { op, body };
            let id = self.arena.alloc(node);
            self.pos = i + 1;
            redirs.insert(heredoc_place.unwrap(), id);
        }

        let node = ShAstNode::SimpleCommand {
            assignments,
            argv,
            redirs,
        };

        Ok(self.arena.alloc(node))
    }

    fn parse_subshell(&mut self) -> Result<ShNodeId, ParseError> {
        self.expect_current_token(ShTokenKind::LParen)?;
        self.pos += 1;
        let body = self.parse_list(&[], &[ShTokenKind::RParen])?;
        self.expect_current_token(ShTokenKind::RParen)?;
        self.pos += 1;
        let node = ShAstNode::Subshell { body };
        Ok(self.arena.alloc(node))
    }

    fn parse_group(&mut self) -> Result<ShNodeId, ParseError> {
        self.expect_current_token(ShTokenKind::LBrace)?;
        self.pos += 1;
        let body = self.parse_list(&[], &[ShTokenKind::RBrace])?;
        self.expect_current_token(ShTokenKind::RBrace)?;
        self.pos += 1;
        let node = ShAstNode::Group { body };
        Ok(self.arena.alloc(node))
    }

    fn recover_to_stmt_boundary(&mut self) {
        eprintln!("RECOVER");
        while let Some(tok) = self.next_token() {
            match tok.kind {
                ShTokenKind::NewLine
                | ShTokenKind::SemiColon
                | ShTokenKind::RParen
                | ShTokenKind::RBrace => {
                    break;
                }
                _ => self.pos += 1,
            }
        }
    }
}

pub fn format_ast_tree(program: &ShProgram, arena: &ShAstArena) -> String {
    let mut out = String::new();
    push_line(&mut out, 0, "Program");
    fmt_node(program.list, arena, 1, &mut out);
    out
}

pub fn debug_print_ast(program: &ShProgram, arena: &ShAstArena) {
    println!("{}", format_ast_tree(program, arena));
}

fn fmt_node(id: ShNodeId, arena: &ShAstArena, indent: usize, out: &mut String) {
    let node = arena.get(id);
    match node {
        ShAstNode::Pipeline { first, rest } => {
            push_line(out, indent, "Pipeline");
            fmt_node(*first, arena, indent + 1, out);
            for cmd in rest.iter() {
                fmt_node(*cmd, arena, indent + 1, out);
            }
        }
        ShAstNode::SimpleCommand {
            assignments,
            argv,
            redirs,
        } => {
            push_line(out, indent, "SimpleCommand");
            push_line(out, indent + 1, "assignments");
            for (index, node_id) in assignments.iter().enumerate() {
                fmt_node(*node_id, arena, indent + 2, out);
            }
            push_line(out, indent + 1, "argv");
            for (index, node_id) in argv.iter().enumerate() {
                fmt_node(*node_id, arena, indent + 2, out);
            }
            push_line(out, indent + 1, "redirs");
            for (index, node_id) in redirs.iter().enumerate() {
                fmt_node(*node_id, arena, indent + 2, out);
            }
        }
        ShAstNode::If {
            cond,
            then_part,
            else_part,
        } => {
            push_line(out, indent, "If");
            push_line(out, indent + 1, "cond");
            fmt_node(*cond, arena, indent + 1, out);
            push_line(out, indent + 1, "then");
            fmt_node(*then_part, arena, indent + 1, out);
            if let Some(else_id) = else_part {
                push_line(out, indent + 1, "else");
                fmt_node(*else_id, arena, indent + 1, out);
            }
        }
        ShAstNode::While { cond, body } => {
            push_line(out, indent, "While");
            push_line(out, indent + 1, "cond");
            fmt_node(*cond, arena, indent + 1, out);
            push_line(out, indent + 1, "body");
            fmt_node(*body, arena, indent + 1, out);
        }
        ShAstNode::For { var, items, body } => {
            push_line(out, indent, "For");
            push_line(out, indent + 1, "var");
            fmt_node(*var, arena, indent + 1, out);
            push_line(out, indent + 1, "items");
            for (index, item) in items.iter().enumerate() {
                push_line(out, indent + 1, &format!("item[{index}]"));
                fmt_node(*item, arena, indent + 3, out);
            }
            push_line(out, indent + 1, "body");
            fmt_node(*body, arena, indent + 1, out);
        }
        ShAstNode::FunctionDef { name, body } => {
            push_line(out, indent, "FunctionDef");
            push_line(out, indent + 1, "name");
            fmt_node(*name, arena, indent + 1, out);
            push_line(out, indent + 1, "body");
            fmt_node(*body, arena, indent + 1, out);
        }
        ShAstNode::Subshell { body } => {
            push_line(out, indent, "Subshell");
            fmt_node(*body, arena, indent + 1, out);
        }
        ShAstNode::Group { body } => {
            push_line(out, indent, "Group");
            fmt_node(*body, arena, indent + 1, out);
        }
        ShAstNode::Word(value) => {
            push_line(out, indent, &format!("Word \"{value}\""));
        }
        ShAstNode::Assignment(value) => {
            push_line(out, indent, &format!("Assignment \"{value}\""));
        }
        ShAstNode::Redir { op, body } => {
            push_line(out, indent, &format!("Redir \"{op}\" \"{body}\""));
        }
        ShAstNode::Unknown => {
            push_line(out, indent, "Unknown");
        }
        ShAstNode::List(v) => {
            push_line(out, indent, "List");
            v.iter().for_each(|item| {
                fmt_node(item.body, arena, indent + 1, out);
            });
        }
        ShAstNode::AndOr { first, rest } => {
            push_line(out, indent, "AndOr");
            fmt_node(*first, arena, indent + 1, out);
            rest.iter().for_each(|cmd| {
                push_line(out, indent + 1, &format!("{:?}", cmd.op));
                fmt_node(cmd.body, arena, indent + 1, out);
            })
        }
    }
}

fn push_line(out: &mut String, indent: usize, text: &str) {
    for _ in 0..indent {
        out.push_str("  ");
    }
    out.push_str(text);
    out.push('\n');
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod parser_test {

    use crate::actions_parser::sh_parser::{
        sh_lexer::Lexer,
        sh_parser::{ShParser, format_ast_tree},
        sh_token::ShToken,
    };
    use crate::actions_parser::source_map::SourceId;

    #[test]
    fn test() {
        let program = r#"
cmd > out.txt 2>&1
"#;
        let tokens: Vec<ShToken> = Lexer::new(program.chars().collect(), SourceId(0))
            .map(|it| it.unwrap())
            .collect();
        println!("{:?}", tokens);
        println!(
            "token str: {:?}",
            tokens
                .iter()
                .map(|f| f.text(program))
                .collect::<Vec<&str>>()
                .join(" ")
        );
        let mut parser = ShParser::new(tokens, program.to_string());

        let program = parser.parse_program();
        println!("{:?}", program);
        println!("{}", format_ast_tree(&program.unwrap(), &parser.arena));
        println!("{:?}", parser.arena)
    }

    #[test]
    fn simple_test() {
        let program = "
cat hello
HOGE=1 hoge
VAR=1
echo hoge > hello.txt
aaa | bbb && cc || dd
";
        let tokens: Vec<ShToken> = Lexer::new(program.chars().collect(), SourceId(0))
            .map(|it| it.unwrap())
            .collect();
        let mut parser = ShParser::new(tokens, program.to_string());

        let program = parser.parse_program().unwrap();

        println!("{}", format_ast_tree(&program, &parser.arena));
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        SimpleCommand
          assignments
          argv
            Word "cat"
            Word "hello"
          redirs
    AndOr
      Pipeline
        SimpleCommand
          assignments
            Assignment "HOGE=1"
          argv
            Word "hoge"
          redirs
    AndOr
      Pipeline
        SimpleCommand
          assignments
            Assignment "VAR=1"
          argv
          redirs
    AndOr
      Pipeline
        SimpleCommand
          assignments
          argv
            Word "echo"
            Word "hoge"
          redirs
            Redir ">" "hello.txt"
    AndOr
      Pipeline
        SimpleCommand
          assignments
          argv
            Word "aaa"
          redirs
        SimpleCommand
          assignments
          argv
            Word "bbb"
          redirs
      And
      Pipeline
        SimpleCommand
          assignments
          argv
            Word "cc"
          redirs
      Or
      Pipeline
        SimpleCommand
          assignments
          argv
            Word "dd"
          redirs
"#,
            format_ast_tree(&program, &parser.arena)
        );
    }

    #[test]
    fn test_if() {
        let program = r#"
if [ "$n" -lt 0 ]; then
    echo "negative"
elif [ "$n" -eq 0 ]; then
    echo "zero"
else
    echo "positive"
fi
"#;
        let tokens: Vec<ShToken> = Lexer::new(program.chars().collect(), SourceId(0))
            .map(|it| it.unwrap())
            .collect();
        println!("{:?}", tokens);
        println!(
            "token str: {:?}",
            tokens
                .iter()
                .map(|f| f.text(program))
                .collect::<Vec<&str>>()
                .join(" ")
        );
        let mut parser = ShParser::new(tokens, program.to_string());

        let program = parser.parse_program();
        println!("{:?}", program);
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        If
          cond
          List
            AndOr
              Pipeline
                SimpleCommand
                  assignments
                  argv
                    Word "["
                    Word ""$n""
                    Word "-lt"
                    Word "0"
                    Word "]"
                  redirs
          then
          List
            AndOr
              Pipeline
                SimpleCommand
                  assignments
                  argv
                    Word "echo"
                    Word ""negative""
                  redirs
          else
          If
            cond
            List
              AndOr
                Pipeline
                  SimpleCommand
                    assignments
                    argv
                      Word "["
                      Word ""$n""
                      Word "-eq"
                      Word "0"
                      Word "]"
                    redirs
            then
            List
              AndOr
                Pipeline
                  SimpleCommand
                    assignments
                    argv
                      Word "echo"
                      Word ""zero""
                    redirs
            else
            List
              AndOr
                Pipeline
                  SimpleCommand
                    assignments
                    argv
                      Word "echo"
                      Word ""positive""
                    redirs
"#,
            format_ast_tree(&program.unwrap(), &parser.arena)
        );
        println!("{:?}", parser.arena)
    }

    fn parse_and_format(program: &str) -> String {
        let tokens: Vec<ShToken> = Lexer::new(program.chars().collect(), SourceId(0))
            .map(|it| it.unwrap())
            .collect();
        println!("{:?}", tokens);
        println!(
            "{:?}",
            tokens
                .iter()
                .map(|f| f.text(program).to_string())
                .collect::<Vec<String>>()
                .join("/ ")
        );

        let mut parser = ShParser::new(tokens, program.to_string());
        let program = parser.parse_program().unwrap();
        println!("{}", format_ast_tree(&program, &parser.arena));
        format_ast_tree(&program, &parser.arena)
    }

    #[test]
    fn test_for() {
        let program = "for i in 1 2 3; do echo $i; done";
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        For
          var
          Word "i"
          items
          item[0]
              Word "1"
          item[1]
              Word "2"
          item[2]
              Word "3"
          body
          List
            AndOr
              Pipeline
                SimpleCommand
                  assignments
                  argv
                    Word "echo"
                    Word "$i"
                  redirs
"#,
            parse_and_format(program)
        );
    }

    #[test]
    fn test_while() {
        let program = "while echo hi; do echo done; done";
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        While
          cond
          List
            AndOr
              Pipeline
                SimpleCommand
                  assignments
                  argv
                    Word "echo"
                    Word "hi"
                  redirs
          body
          List
            AndOr
              Pipeline
                SimpleCommand
                  assignments
                  argv
                    Word "echo"
                  redirs
    AndOr
      Pipeline
        SimpleCommand
          assignments
          argv
            Word "done"
          redirs
"#,
            parse_and_format(program)
        );
    }

    #[test]
    fn test_function_def() {
        let program = "foo() { echo hi }";
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        FunctionDef
          name
          Word "foo"
          body
          Group
            List
              AndOr
                Pipeline
                  SimpleCommand
                    assignments
                    argv
                      Word "echo"
                      Word "hi"
                    redirs
"#,
            parse_and_format(program)
        );
    }

    #[test]
    fn test_subshell() {
        let program = "(echo hi)";
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        Subshell
          List
            AndOr
              Pipeline
                SimpleCommand
                  assignments
                  argv
                    Word "echo"
                    Word "hi"
                  redirs
"#,
            parse_and_format(program)
        );
    }

    #[test]
    fn test_group() {
        let program = "{ echo hi }";
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        Group
          List
            AndOr
              Pipeline
                SimpleCommand
                  assignments
                  argv
                    Word "echo"
                    Word "hi"
                  redirs
"#,
            parse_and_format(program)
        );
    }

    #[test]
    fn test_redir_io_numbers() {
        let program = "echo hi 2>out 1>>log 0<in 3<>rw 4>|clob 5>&2 6<&1 7>-";
        assert_eq!(
            r#"Program
  List
    AndOr
      Pipeline
        SimpleCommand
          assignments
          argv
            Word "echo"
            Word "hi"
          redirs
            Redir "2>" "out"
            Redir "1>>" "log"
            Redir "0<" "in"
            Redir "3<>" "rw"
            Redir "4>|" "clob"
            Redir "5>&" "2"
            Redir "6<&" "1"
            Redir "7>" "-"
"#,
            parse_and_format(program)
        );
    }
}
