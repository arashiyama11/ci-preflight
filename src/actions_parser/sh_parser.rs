use crate::actions_parser::sh_parser::arena::ShAstArena;
use crate::actions_parser::sh_parser::sh_ast::ShProgram;
use crate::actions_parser::sh_parser::sh_parser::ShParser;
use crate::actions_parser::source_map::{SourceId, SourceMap};
use sh_lexer::Lexer;

mod arena;
pub mod sh_ast;
mod sh_lexer;
mod sh_parser;
mod sh_token;

pub fn parse_sh(source_map: &SourceMap, source_id: &SourceId) -> (ShProgram, ShAstArena) {
    let text = source_map.get_text(source_id).unwrap();
    let tokens = Lexer::new(text.chars().collect::<Vec<char>>(), *source_id)
        .map(|it| it.unwrap())
        .collect();

    let mut parser = ShParser::new(tokens, text.to_string());
    let result = parser.parse_program().unwrap();
    (result, parser.arena)
}
