use crate::actions_parser::actions_ast::ActionsAst;
use crate::actions_parser::sh_parser::sh_ast::ShAstNode;
use std::collections::BTreeMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AstId(pub u32);

#[derive(Debug, Clone)]
pub enum AstNode {
    Actions(ActionsAst),
    Sh(ShAstNode),
}

#[derive(Debug, Clone, Default)]
pub struct Attributes {
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AstArena {
    nodes: Vec<AstNode>,
    attrs: Vec<Attributes>,
}

impl AstArena {
    pub fn new() -> AstArena {
        AstArena {
            nodes: vec![],
            attrs: vec![],
        }
    }

    fn alloc_node(&mut self, node: AstNode) -> AstId {
        let id = AstId(self.nodes.len() as u32);
        self.nodes.push(node);
        self.attrs.push(Attributes::default());
        id
    }

    pub fn alloc_actions(&mut self, node: ActionsAst) -> AstId {
        self.alloc_node(AstNode::Actions(node))
    }

    pub fn alloc_sh(&mut self, node: ShAstNode) -> AstId {
        self.alloc_node(AstNode::Sh(node))
    }

    pub fn get_actions(&self, id: &AstId) -> &ActionsAst {
        match &self.nodes[id.0 as usize] {
            AstNode::Actions(node) => node,
            _ => panic!("AstId is not ActionsAst"),
        }
    }

    pub fn get_actions_mut(&mut self, id: &AstId) -> &mut ActionsAst {
        match &mut self.nodes[id.0 as usize] {
            AstNode::Actions(node) => node,
            _ => panic!("AstId is not ActionsAst"),
        }
    }

    pub fn get_sh(&self, id: AstId) -> &ShAstNode {
        match &self.nodes[id.0 as usize] {
            AstNode::Sh(node) => node,
            _ => panic!("AstId is not ShAstNode"),
        }
    }

    pub fn get_sh_mut(&mut self, id: AstId) -> &mut ShAstNode {
        match &mut self.nodes[id.0 as usize] {
            AstNode::Sh(node) => node,
            _ => panic!("AstId is not ShAstNode"),
        }
    }

    pub fn get_attr(&self, id: &AstId) -> &Attributes {
        &self.attrs[id.0 as usize]
    }

    pub fn get_attr_mut(&mut self, id: &AstId) -> &mut Attributes {
        &mut self.attrs[id.0 as usize]
    }
}
