pub(crate) mod actions_ast;
mod parser;
pub(crate) mod sh_parser;
pub(crate) mod source_map;

pub(crate) mod arena;

use crate::parser::arena::{AstArena, AstId};
use crate::parser::parser::ActionsParseError;
use crate::parser::source_map::{SourceId, SourceMap};

pub fn parse_actions_yaml(
    source_map: &mut SourceMap,
    source_id: &SourceId,
) -> Result<(AstId, AstArena, Vec<ActionsParseError>), ActionsParseError> {
    parser::parse_actions_yaml(source_map, source_id)
}

pub fn format_actions_tree(arena: &AstArena, root: &AstId) -> String {
    parser::format_actions_tree(arena, root)
}
