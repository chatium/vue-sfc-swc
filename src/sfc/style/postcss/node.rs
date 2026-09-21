//! The postcss node model, as an arena so plugins can walk and mutate it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssKind {
    Root,
    Rule,
    AtRule,
    Decl,
    Comment,
}

#[derive(Debug, Clone, Default)]
pub struct Raws {
    pub before: Option<String>,
    pub after: Option<String>,
    pub between: Option<String>,
    pub semicolon: Option<bool>,
    pub own_semicolon: Option<String>,
    pub after_name: Option<String>,
    pub important: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
    /// `raws.value` / `raws.selector` / `raws.params`: (raw, value)
    pub value: Option<(String, String)>,
    pub selector: Option<(String, String)>,
    pub params: Option<(String, String)>,
    pub indent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CssNode {
    pub kind: CssKind,
    pub parent: Option<usize>,
    pub nodes: Option<Vec<usize>>,
    pub raws: Raws,
    // rule
    pub selector: String,
    // at-rule
    pub name: String,
    pub params: String,
    // decl
    pub prop: String,
    pub value: String,
    pub important: bool,
    // comment
    pub text: String,
    /// start offset in the source (UTF-16 units)
    pub start: usize,
}

impl CssNode {
    pub fn new(kind: CssKind) -> Self {
        CssNode {
            kind,
            parent: None,
            nodes: if matches!(kind, CssKind::Root) {
                Some(Vec::new())
            } else {
                None
            },
            raws: Raws::default(),
            selector: String::new(),
            name: String::new(),
            params: String::new(),
            prop: String::new(),
            value: String::new(),
            important: false,
            text: String::new(),
            start: 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct CssTree {
    pub nodes: Vec<CssNode>,
    pub root: usize,
}

impl CssTree {
    pub fn new() -> Self {
        let mut t = CssTree {
            nodes: Vec::new(),
            root: 0,
        };
        t.nodes.push(CssNode::new(CssKind::Root));
        t
    }

    pub fn add(&mut self, node: CssNode) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    pub fn get(&self, id: usize) -> &CssNode {
        &self.nodes[id]
    }

    pub fn get_mut(&mut self, id: usize) -> &mut CssNode {
        &mut self.nodes[id]
    }

    pub fn push_child(&mut self, parent: usize, child: usize) {
        self.nodes[child].parent = Some(parent);
        if let Some(list) = self.nodes[parent].nodes.as_mut() {
            list.push(child);
        }
    }

    pub fn children(&self, id: usize) -> Vec<usize> {
        self.nodes[id].nodes.clone().unwrap_or_default()
    }

    pub fn remove(&mut self, id: usize) {
        if let Some(parent) = self.nodes[id].parent {
            if let Some(list) = self.nodes[parent].nodes.as_mut() {
                list.retain(|c| *c != id);
            }
        }
        self.nodes[id].parent = None;
    }

    /// depth-first walk, skipping removed subtrees
    pub fn walk(&self, id: usize, f: &mut impl FnMut(&CssTree, usize) -> bool) -> bool {
        for child in self.children(id) {
            if !f(self, child) {
                return false;
            }
            if self.nodes[child].nodes.is_some() && !self.walk(child, f) {
                return false;
            }
        }
        true
    }

    pub fn walk_ids(&self, id: usize) -> Vec<usize> {
        let mut out = Vec::new();
        self.collect(id, &mut out);
        out
    }

    fn collect(&self, id: usize, out: &mut Vec<usize>) {
        for child in self.children(id) {
            out.push(child);
            if self.nodes[child].nodes.is_some() {
                self.collect(child, out);
            }
        }
    }

    pub fn depth(&self, id: usize) -> usize {
        let mut depth = 0;
        let mut cur = self.nodes[id].parent;
        while let Some(p) = cur {
            if self.nodes[p].kind == CssKind::Root {
                break;
            }
            depth += 1;
            cur = self.nodes[p].parent;
        }
        depth
    }
}
