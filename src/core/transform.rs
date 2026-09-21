//! Port of `compiler-core/src/transform.ts`.

use std::collections::{HashMap, HashSet};

use super::ast::*;
use super::errors::{CompilerError, ErrorCode, create_compiler_error};
use super::options::*;

#[derive(Debug, Default, Clone, Copy)]
pub struct Scopes {
    pub v_for: i32,
    pub v_slot: i32,
    pub v_pre: i32,
    pub v_once: i32,
}

/// Exit callbacks, as owned data rather than closures.
#[derive(Debug, Clone)]
pub enum ExitFn {
    Once {
        node: NodeId,
    },
    IfRoot {
        if_node: NodeId,
        branch: NodeId,
        key: usize,
    },
    Memo {
        node: NodeId,
        exp: NodeId,
    },
    For {
        for_node: NodeId,
        key_prop: Option<NodeId>,
        is_stable_fragment: bool,
        memo: Option<NodeId>,
        fragment_flag: i32,
    },
    Expression {
        node: NodeId,
    },
    SlotOutlet {
        node: NodeId,
    },
    Element {
        node: NodeId,
    },
    SlotScopes,
    Text {
        node: NodeId,
    },
    Transition {
        node: NodeId,
    },
}

pub struct TransformContext {
    pub a: Arena,
    pub opts: TransformOptions,
    pub self_name: Option<String>,
    pub root: NodeId,
    /// insertion-ordered helper counts, like the JS `Map<symbol, number>`
    pub helpers: Vec<(RuntimeHelper, usize)>,
    pub components: Vec<String>,
    pub directives: Vec<String>,
    pub hoists: Vec<Option<NodeId>>,
    pub imports: Vec<ImportItem>,
    pub temps: usize,
    pub cached: Vec<Option<NodeId>>,
    pub constant_cache: HashMap<NodeId, ConstantType>,
    pub v_for_memo_keyed_nodes: HashSet<NodeId>,
    pub identifiers: HashMap<String, i32>,
    pub scopes: Scopes,
    pub parent: Option<NodeId>,
    pub child_index: usize,
    pub current_node: Option<NodeId>,
    pub in_v_once: bool,
    /// how many times `onNodeRemoved` fired for the innermost child loop
    pub removal_adjust: i64,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerError>,
}

impl TransformContext {
    pub fn new(arena: Arena, root: NodeId, opts: TransformOptions) -> Self {
        let self_name = self_name_from_filename(&opts.filename);
        TransformContext {
            a: arena,
            opts,
            self_name,
            root,
            helpers: Vec::new(),
            components: Vec::new(),
            directives: Vec::new(),
            hoists: Vec::new(),
            imports: Vec::new(),
            temps: 0,
            cached: Vec::new(),
            constant_cache: HashMap::new(),
            v_for_memo_keyed_nodes: HashSet::new(),
            identifiers: HashMap::new(),
            scopes: Scopes::default(),
            parent: None,
            child_index: 0,
            current_node: Some(root),
            in_v_once: false,
            removal_adjust: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn on_error(&mut self, e: CompilerError) {
        self.errors.push(e);
    }

    pub fn error(&mut self, code: ErrorCode, loc: Option<SourceLocation>) {
        let e = create_compiler_error(code, loc, None);
        self.errors.push(e);
    }

    pub fn helper(&mut self, name: RuntimeHelper) -> RuntimeHelper {
        match self.helpers.iter_mut().find(|(h, _)| *h == name) {
            Some((_, count)) => *count += 1,
            None => self.helpers.push((name, 1)),
        }
        name
    }

    pub fn remove_helper(&mut self, name: RuntimeHelper) {
        if let Some(pos) = self.helpers.iter().position(|(h, _)| *h == name) {
            let count = self.helpers[pos].1;
            if count > 0 {
                let next = count - 1;
                if next == 0 {
                    self.helpers.remove(pos);
                } else {
                    self.helpers[pos].1 = next;
                }
            }
        }
    }

    pub fn helper_string(&mut self, name: RuntimeHelper) -> String {
        format!("_{}", self.helper(name).name())
    }

    /// `context.helper(name)` returning the symbol as an arena node.
    pub fn helper_node(&mut self, name: RuntimeHelper) -> NodeId {
        self.helper(name);
        self.a.sym(name)
    }

    pub fn add_component(&mut self, name: String) {
        if !self.components.contains(&name) {
            self.components.push(name);
        }
    }

    pub fn add_directive(&mut self, name: String) {
        if !self.directives.contains(&name) {
            self.directives.push(name);
        }
    }

    pub fn replace_node(&mut self, node: NodeId) {
        let parent = self.parent.expect("cannot replace root node");
        let idx = self.child_index;
        self.a.children_of_mut(parent)[idx] = node;
        self.current_node = Some(node);
    }

    pub fn remove_node(&mut self, node: Option<NodeId>) {
        let parent = self.parent.expect("cannot remove root node");
        let removal_index = match node {
            Some(n) => self
                .a
                .children_of(parent)
                .iter()
                .position(|c| *c == n)
                .map(|i| i as i64)
                .unwrap_or(-1),
            None => {
                if self.current_node.is_some() {
                    self.child_index as i64
                } else {
                    -1
                }
            }
        };
        if removal_index < 0 {
            return;
        }
        if node.is_none() || node == self.current_node {
            self.current_node = None;
            self.removal_adjust += 1;
        } else if self.child_index as i64 > removal_index {
            self.child_index -= 1;
            self.removal_adjust += 1;
        }
        self.a
            .children_of_mut(parent)
            .remove(removal_index as usize);
    }

    pub fn add_identifiers(&mut self, exp: NodeId) {
        let ids = self.identifiers_of(exp);
        for id in ids {
            *self.identifiers.entry(id).or_insert(0) += 1;
        }
    }

    pub fn add_identifier_str(&mut self, id: &str) {
        *self.identifiers.entry(id.to_string()).or_insert(0) += 1;
    }

    pub fn remove_identifiers(&mut self, exp: NodeId) {
        let ids = self.identifiers_of(exp);
        for id in ids {
            if let Some(v) = self.identifiers.get_mut(&id) {
                *v -= 1;
            }
        }
    }

    pub fn remove_identifier_str(&mut self, id: &str) {
        if let Some(v) = self.identifiers.get_mut(id) {
            *v -= 1;
        }
    }

    fn identifiers_of(&self, exp: NodeId) -> Vec<String> {
        match self.a.node(exp) {
            Node::Str(s) => vec![s.clone()],
            Node::SimpleExpression(e) => {
                if e.identifiers.is_empty() {
                    vec![e.content.clone()]
                } else {
                    e.identifiers.clone()
                }
            }
            Node::CompoundExpression(c) => c.identifiers.clone(),
            _ => Vec::new(),
        }
    }

    pub fn has_identifier(&self, name: &str) -> bool {
        self.identifiers.get(name).copied().unwrap_or(0) > 0
    }

    pub fn hoist(&mut self, exp: NodeId) -> NodeId {
        self.hoists.push(Some(exp));
        let loc = self.a.loc(exp).clone();
        let name = format!("_hoisted_{}", self.hoists.len());
        let identifier =
            self.a
                .create_simple_expression(name, false, loc, ConstantType::CanCache);
        self.a.exp_mut(identifier).hoisted = Some(self.hoists.len() - 1);
        identifier
    }

    pub fn cache(&mut self, exp: NodeId, is_vnode: bool, in_v_once: bool) -> NodeId {
        let index = self.cached.len();
        let cache_exp = self.a.create_cache_expression(index, exp, is_vnode, in_v_once);
        self.cached.push(Some(cache_exp));
        cache_exp
    }
}

fn self_name_from_filename(filename: &str) -> Option<String> {
    // filename.replace(/\?.*$/, '').match(/([^/\\]+)\.\w+$/)
    let base = match filename.find('?') {
        Some(i) => &filename[..i],
        None => filename,
    };
    let last = base.rsplit(['/', '\\']).next()?;
    let dot = last.rfind('.')?;
    let (name, ext) = last.split_at(dot);
    if name.is_empty() || ext.len() < 2 || !ext[1..].chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(capitalize(&camelize(name)))
}

pub fn camelize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper = false;
    for c in s.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn hyphenate(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub fn transform(ctx: &mut TransformContext) {
    let root = ctx.root;
    traverse_node(root, ctx);
    if ctx.opts.hoist_static {
        super::transforms::cache_static::cache_static(root, ctx);
    }
    if !ctx.opts.ssr {
        create_root_codegen(root, ctx);
    }
    let helpers: Vec<RuntimeHelper> = ctx.helpers.iter().map(|(h, _)| *h).collect();
    let components = ctx.components.clone();
    let directives = ctx.directives.clone();
    let imports = ctx.imports.clone();
    let hoists = ctx.hoists.clone();
    let temps = ctx.temps;
    let cached = ctx.cached.clone();
    let r = ctx.a.root_mut(root);
    r.helpers = helpers;
    r.components = components;
    r.directives = directives;
    r.imports = imports;
    r.hoists = hoists;
    r.temps = temps;
    r.cached = cached;
    r.transformed = true;
}

fn create_root_codegen(root: NodeId, ctx: &mut TransformContext) {
    let children = ctx.a.root(root).children.clone();
    if children.len() == 1 {
        let single = super::transforms::cache_static::get_single_element_root(&ctx.a, root);
        match single.and_then(|s| ctx.a.el(s).codegen_node.map(|c| (s, c))) {
            Some((_, codegen_node)) => {
                if ctx.a.is(codegen_node, NodeType::VNodeCall) {
                    convert_to_block(codegen_node, ctx);
                }
                ctx.a.root_mut(root).codegen_node = Some(codegen_node);
            }
            None => {
                ctx.a.root_mut(root).codegen_node = Some(children[0]);
            }
        }
    } else if children.len() > 1 {
        let mut patch_flag = super::patch_flags::STABLE_FRAGMENT;
        if children
            .iter()
            .filter(|c| !ctx.a.is(**c, NodeType::Comment))
            .count()
            == 1
        {
            patch_flag |= super::patch_flags::DEV_ROOT_FRAGMENT;
        }
        let tag = ctx.helper_node(RuntimeHelper::FRAGMENT);
        let children_ref = ctx.a.children_ref(root);
        let node = create_vnode_call(
            Some(ctx),
            tag,
            None,
            Some(children_ref),
            Some(patch_flag),
            None,
            None,
            true,
            false,
            false,
            loc_stub(),
        );
        ctx.a.root_mut(root).codegen_node = Some(node);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_vnode_call(
    ctx: Option<&mut TransformContext>,
    tag: NodeId,
    props: Option<NodeId>,
    children: Option<NodeId>,
    patch_flag: Option<i32>,
    dynamic_props: Option<NodeId>,
    directives: Option<NodeId>,
    is_block: bool,
    disable_tracking: bool,
    is_component: bool,
    loc: SourceLocation,
) -> NodeId {
    let ctx = ctx.expect("createVNodeCall needs a context in this port");
    if is_block {
        ctx.helper(RuntimeHelper::OPEN_BLOCK);
        let h = get_vnode_block_helper(ctx.opts.in_ssr, is_component);
        ctx.helper(h);
    } else {
        let h = get_vnode_helper(ctx.opts.in_ssr, is_component);
        ctx.helper(h);
    }
    if directives.is_some() {
        ctx.helper(RuntimeHelper::WITH_DIRECTIVES);
    }
    ctx.a.add(Node::VNodeCall(Box::new(VNodeCall {
        tag,
        props,
        children,
        patch_flag,
        dynamic_props,
        directives,
        is_block,
        disable_tracking,
        is_component,
        loc,
    })))
}

pub fn convert_to_block(node: NodeId, ctx: &mut TransformContext) {
    let (is_block, is_component) = {
        let v = ctx.a.vnode(node);
        (v.is_block, v.is_component)
    };
    if !is_block {
        ctx.a.vnode_mut(node).is_block = true;
        let h = get_vnode_helper(ctx.opts.in_ssr, is_component);
        ctx.remove_helper(h);
        ctx.helper(RuntimeHelper::OPEN_BLOCK);
        let h = get_vnode_block_helper(ctx.opts.in_ssr, is_component);
        ctx.helper(h);
    }
}

pub fn traverse_children(parent: NodeId, ctx: &mut TransformContext) {
    let mut i: i64 = 0;
    let saved_adjust = ctx.removal_adjust;
    loop {
        let len = ctx.a.children_of(parent).len() as i64;
        if i >= len {
            break;
        }
        let child = ctx.a.children_of(parent)[i as usize];
        ctx.parent = Some(parent);
        ctx.child_index = i as usize;
        ctx.removal_adjust = 0;
        traverse_node(child, ctx);
        i += 1 - ctx.removal_adjust;
    }
    ctx.removal_adjust = saved_adjust;
}

pub fn traverse_node(node: NodeId, ctx: &mut TransformContext) {
    ctx.current_node = Some(node);
    let mut node = node;
    let mut exit_fns: Vec<ExitFn> = Vec::new();
    let transforms = ctx.opts.node_transforms.clone();
    for kind in transforms {
        let exits = super::transforms::apply_node_transform(kind, node, ctx);
        exit_fns.extend(exits);
        match ctx.current_node {
            None => return,
            Some(n) => node = n,
        }
    }

    match ctx.a.node_type(node) {
        NodeType::Comment => {
            if !ctx.opts.ssr {
                ctx.helper(RuntimeHelper::CREATE_COMMENT);
            }
        }
        NodeType::Interpolation => {
            if !ctx.opts.ssr {
                ctx.helper(RuntimeHelper::TO_DISPLAY_STRING);
            }
        }
        NodeType::If => {
            let branches = ctx.a.if_node(node).branches.clone();
            let saved_parent = ctx.parent;
            let saved_index = ctx.child_index;
            for b in branches {
                traverse_node(b, ctx);
            }
            ctx.parent = saved_parent;
            ctx.child_index = saved_index;
        }
        NodeType::IfBranch | NodeType::For | NodeType::Element | NodeType::Root => {
            let saved_parent = ctx.parent;
            let saved_index = ctx.child_index;
            traverse_children(node, ctx);
            ctx.parent = saved_parent;
            ctx.child_index = saved_index;
        }
        _ => {}
    }

    ctx.current_node = Some(node);
    for exit in exit_fns.into_iter().rev() {
        super::transforms::run_exit(exit, ctx);
    }
}

/// `createStructuralDirectiveTransform` — returns the matching directives
/// (already spliced out of `props`) for the caller to process.
pub fn take_structural_directives(
    node: NodeId,
    ctx: &mut TransformContext,
    matches: impl Fn(&str) -> bool,
) -> Vec<NodeId> {
    if !ctx.a.is(node, NodeType::Element) {
        return Vec::new();
    }
    if ctx.a.el(node).tag_type == ElementType::Template
        && ctx
            .a
            .el(node)
            .props
            .iter()
            .any(|p| super::utils::is_v_slot(&ctx.a, *p))
    {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut i = 0;
    while i < ctx.a.el(node).props.len() {
        let prop = ctx.a.el(node).props[i];
        let is_match = match ctx.a.node(prop) {
            Node::Directive(d) => matches(&d.name),
            _ => false,
        };
        if is_match {
            ctx.a.el_mut(node).props.remove(i);
            found.push(prop);
        } else {
            i += 1;
        }
    }
    found
}
