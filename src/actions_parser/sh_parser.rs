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

pub fn parse_sh(
    source_map: &SourceMap,
    source_id: &SourceId,
) -> Result<(ShProgram, AstArena), ShParseError> {
    let text = source_map.get_text(source_id).ok_or_else(|| {
        ShParseError::Parser(sh_parser::ParseError::InternalError("missing source"))
    })?;
    let mut tokens = Vec::new();
    for tok in Lexer::new(text.chars().collect::<Vec<char>>(), *source_id) {
        tokens.push(tok?);
    }

    let mut parser = ShParser::new(tokens, text.to_string());
    let result = parser.parse_program()?;
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
    let mut tokens = Vec::new();
    for tok in Lexer::new(text.chars().collect::<Vec<char>>(), *source_id) {
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

    let mut parser = ShParser::new_with_arena(tokens, text.to_string(), arena);
    match parser.parse_program() {
        Ok(result) => Ok((result, parser.arena)),
        Err(err) => Err(ShParseErrorWithArena {
            error: ShParseError::Parser(err),
            arena: parser.arena,
        }),
    }
}
