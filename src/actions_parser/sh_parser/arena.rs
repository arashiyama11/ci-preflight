use super::sh_ast::{ShAstNode, ShNodeId};

#[derive(Debug)]
pub struct ShAstArena {
    nodes: Vec<ShAstNode>,
}

impl ShAstArena {
    pub fn new() -> ShAstArena {
        ShAstArena { nodes: vec![] }
    }

    pub fn alloc(&mut self, node: ShAstNode) -> ShNodeId {
        let id = ShNodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    pub fn get(&self, id: ShNodeId) -> &ShAstNode {
        &self.nodes[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: ShNodeId) -> &mut ShAstNode {
        &mut self.nodes[id.0 as usize]
    }
}
