use crate::actions_parser::arena::AstArena;
use crate::actions_parser::sh_parser::sh_ast::ShProgram;
use crate::actions_parser::sh_parser::sh_parser::ShParser;
use crate::actions_parser::source_map::{SourceId, SourceMap};
use sh_lexer::Lexer;
use thiserror::Error;

pub mod sh_ast;
mod sh_lexer;
mod sh_parser;
mod sh_token;

pub use sh_lexer::LexerError;
pub use sh_parser::ParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprPlaceholder {
    pub placeholder: String,
    pub original: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExprPlaceholderMap {
    replacements: Vec<ExprPlaceholder>,
}

impl ExprPlaceholderMap {
    pub fn restore(&self, input: &str) -> String {
        let mut out = input.to_string();
        for rep in &self.replacements {
            out = out.replace(&rep.placeholder, &rep.original);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessedShell {
    pub text: String,
    pub placeholders: ExprPlaceholderMap,
}

pub fn preprocess_github_expressions(input: &str) -> PreprocessedShell {
    let mut out = String::with_capacity(input.len());
    let mut replacements = Vec::new();
    let mut i = 0;

    while i < input.len() {
        if input[i..].starts_with("${{") {
            let body_start = i + 3;
            if let Some(rel_end) = input[body_start..].find("}}") {
                let end = body_start + rel_end + 2;
                let original = &input[i..end];
                let char_len = original.chars().count();

                if let Some(placeholder) = build_expr_placeholder(replacements.len(), char_len) {
                    out.push_str(&placeholder);
                    replacements.push(ExprPlaceholder {
                        placeholder,
                        original: original.to_string(),
                    });
                    i = end;
                    continue;
                }
            }
        }

        let ch = input[i..].chars().next().unwrap_or_default();
        out.push(ch);
        i += ch.len_utf8();
    }

    PreprocessedShell {
        text: out,
        placeholders: ExprPlaceholderMap { replacements },
    }
}

fn build_expr_placeholder(index: usize, char_len: usize) -> Option<String> {
    // Short expressions like `${{}}` are already parse-safe enough, so skip replacement.
    if char_len < 6 {
        return None;
    }

    let core = format!("__E{}__", to_base36(index));
    let core_len = core.chars().count();
    if core_len > char_len {
        return None;
    }
    let mut s = core;
    s.push_str(&"_".repeat(char_len - core_len));
    Some(s)
}

fn to_base36(mut n: usize) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(ALPHABET[n % 36] as char);
        n /= 36;
    }
    buf.iter().rev().collect()
}

#[derive(Error, Debug)]
pub enum ShParseError {
    #[error("Lexer error: {0}")]
    Lexer(#[from] sh_lexer::LexerError),
    #[error("Parser error: {0}")]
    Parser(#[from] sh_parser::ParseError),
}

#[derive(Debug)]
pub struct ShParseErrorWithArena {
    pub error: ShParseError,
    pub arena: AstArena,
}

#[allow(dead_code)]
pub fn parse_sh(
    source_map: &SourceMap,
    source_id: &SourceId,
) -> Result<(ShProgram, AstArena), ShParseError> {
    let text = source_map.get_text(source_id).ok_or_else(|| {
        ShParseError::Parser(sh_parser::ParseError::InternalError("missing source"))
    })?;
    let preprocessed = preprocess_github_expressions(text);
    let parse_text = preprocessed.text;
    let mut tokens = Vec::new();
    for tok in Lexer::new(parse_text.chars().collect::<Vec<char>>(), *source_id) {
        tokens.push(tok?);
    }

    let mut parser = ShParser::new(tokens, parse_text);
    let result = parser.parse_program()?;
    restore_expr_placeholders_in_ast(result.list, &mut parser.arena, &preprocessed.placeholders);
    Ok((result, parser.arena))
}

pub fn parse_sh_with_arena(
    source_map: &SourceMap,
    source_id: &SourceId,
    arena: AstArena,
) -> Result<(ShProgram, AstArena), ShParseErrorWithArena> {
    let text = match source_map.get_text(source_id) {
        Some(text) => text,
        None => {
            return Err(ShParseErrorWithArena {
                error: ShParseError::Parser(sh_parser::ParseError::InternalError("missing source")),
                arena,
            });
        }
    };
    let preprocessed = preprocess_github_expressions(text);
    let parse_text = preprocessed.text;
    let mut tokens = Vec::new();
    for tok in Lexer::new(parse_text.chars().collect::<Vec<char>>(), *source_id) {
        match tok {
            Ok(t) => tokens.push(t),
            Err(err) => {
                return Err(ShParseErrorWithArena {
                    error: ShParseError::Lexer(err),
                    arena,
                });
            }
        }
    }

    let mut parser = ShParser::new_with_arena(tokens, parse_text, arena);
    match parser.parse_program() {
        Ok(result) => {
            restore_expr_placeholders_in_ast(
                result.list,
                &mut parser.arena,
                &preprocessed.placeholders,
            );
            Ok((result, parser.arena))
        }
        Err(err) => Err(ShParseErrorWithArena {
            error: ShParseError::Parser(err),
            arena: parser.arena,
        }),
    }
}

fn restore_expr_placeholders_in_ast(
    root: crate::actions_parser::arena::AstId,
    arena: &mut AstArena,
    placeholders: &ExprPlaceholderMap,
) {
    if placeholders.replacements.is_empty() {
        return;
    }

    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let node = arena.get_sh(id).clone();
        match node {
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::List(items) => {
                for item in items {
                    stack.push(item.body);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::AndOr { first, rest } => {
                stack.push(first);
                for n in rest {
                    stack.push(n.body);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Pipeline { first, rest } => {
                stack.push(first);
                for n in rest {
                    stack.push(n);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::SimpleCommand {
                assignments,
                argv,
                redirs,
            } => {
                for n in assignments {
                    stack.push(n);
                }
                for n in argv {
                    stack.push(n);
                }
                for n in redirs {
                    stack.push(n);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::If {
                cond,
                then_part,
                else_part,
            } => {
                stack.push(cond);
                stack.push(then_part);
                if let Some(n) = else_part {
                    stack.push(n);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::While { cond, body } => {
                stack.push(cond);
                stack.push(body);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Subshell { body }
            | crate::actions_parser::sh_parser::sh_ast::ShAstNode::CommandSubstitution { body }
            | crate::actions_parser::sh_parser::sh_ast::ShAstNode::Group { body } => {
                stack.push(body);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::For { var, items, body } => {
                stack.push(var);
                for n in items {
                    stack.push(n);
                }
                stack.push(body);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::FunctionDef { name, body } => {
                stack.push(name);
                stack.push(body);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Word(_)
            | crate::actions_parser::sh_parser::sh_ast::ShAstNode::Assignment(_)
            | crate::actions_parser::sh_parser::sh_ast::ShAstNode::Redir { .. }
            | crate::actions_parser::sh_parser::sh_ast::ShAstNode::Unknown => {}
        }

        match arena.get_sh_mut(id) {
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Word(s)
            | crate::actions_parser::sh_parser::sh_ast::ShAstNode::Assignment(s) => {
                *s = placeholders.restore(s);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Redir { op, body } => {
                *op = placeholders.restore(op);
                *body = placeholders.restore(body);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::preprocess_github_expressions;

    #[test]
    fn preprocess_and_restore_github_expressions() {
        let src = r#"echo "${{ github.repository }}" && URL="${{ github.server_url }}/${{ github.repository }}""#;
        let pre = preprocess_github_expressions(src);
        assert_ne!(pre.text, src);
        assert_eq!(pre.placeholders.replacements.len(), 3);
        let restored = pre.placeholders.restore(&pre.text);
        assert_eq!(restored, src);
    }

    #[test]
    fn preprocess_ignores_unclosed_expression() {
        let src = r#"echo "${{ github.repository }""#;
        let pre = preprocess_github_expressions(src);
        assert_eq!(pre.text, src);
        assert!(pre.placeholders.replacements.is_empty());
    }
}
