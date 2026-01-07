use crate::actions_parser::actions_ast::{ActionsAst, ActionsAstId};

#[derive(Debug)]
pub struct ActionsAstArena {
    nodes: Vec<ActionsAst>,
}

impl ActionsAstArena {
    pub fn new() -> ActionsAstArena {
        ActionsAstArena { nodes: vec![] }
    }

    pub fn alloc(&mut self, node: ActionsAst) -> ActionsAstId {
        let id = ActionsAstId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    pub fn get(&self, id: &ActionsAstId) -> &ActionsAst {
        &self.nodes[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: &ActionsAstId) -> &mut ActionsAst {
        &mut self.nodes[id.0 as usize]
    }
}
