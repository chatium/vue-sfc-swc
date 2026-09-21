//! Port of `compiler-core/src/ast.ts` (+ `runtimeHelpers.ts`).
//!
//! The JS AST is a single dynamically-typed tree whose arrays mix nodes, raw
//! strings and helper symbols, so `Node` mirrors that shape exactly: one enum,
//! with `Str`/`Sym`/`Nodes`/`None` standing in for the non-node members.

use swc_core::ecma::ast as swc_ast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Html = 0,
    Svg = 1,
    MathMl = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    Element = 0,
    Component = 1,
    Slot = 2,
    Template = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConstantType {
    NotConstant = 0,
    CanSkipPatch = 1,
    CanCache = 2,
    CanStringify = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types, dead_code)]
pub enum RuntimeHelper {
    FRAGMENT,
    TELEPORT,
    SUSPENSE,
    KEEP_ALIVE,
    BASE_TRANSITION,
    OPEN_BLOCK,
    CREATE_BLOCK,
    CREATE_ELEMENT_BLOCK,
    CREATE_VNODE,
    CREATE_ELEMENT_VNODE,
    CREATE_COMMENT,
    CREATE_TEXT,
    CREATE_STATIC,
    RESOLVE_COMPONENT,
    RESOLVE_DYNAMIC_COMPONENT,
    RESOLVE_DIRECTIVE,
    RESOLVE_FILTER,
    WITH_DIRECTIVES,
    RENDER_LIST,
    RENDER_SLOT,
    CREATE_SLOTS,
    TO_DISPLAY_STRING,
    MERGE_PROPS,
    NORMALIZE_CLASS,
    NORMALIZE_STYLE,
    NORMALIZE_PROPS,
    GUARD_REACTIVE_PROPS,
    TO_HANDLERS,
    CAMELIZE,
    CAPITALIZE,
    TO_HANDLER_KEY,
    SET_BLOCK_TRACKING,
    PUSH_SCOPE_ID,
    POP_SCOPE_ID,
    WITH_CTX,
    UNREF,
    IS_REF,
    WITH_MEMO,
    IS_MEMO_SAME,
    // compiler-dom
    V_MODEL_RADIO,
    V_MODEL_CHECKBOX,
    V_MODEL_TEXT,
    V_MODEL_SELECT,
    V_MODEL_DYNAMIC,
    V_ON_WITH_MODIFIERS,
    V_ON_WITH_KEYS,
    V_SHOW,
    TRANSITION,
    TRANSITION_GROUP,
}

impl RuntimeHelper {
    pub fn name(self) -> &'static str {
        use RuntimeHelper::*;
        match self {
            FRAGMENT => "Fragment",
            TELEPORT => "Teleport",
            SUSPENSE => "Suspense",
            KEEP_ALIVE => "KeepAlive",
            BASE_TRANSITION => "BaseTransition",
            OPEN_BLOCK => "openBlock",
            CREATE_BLOCK => "createBlock",
            CREATE_ELEMENT_BLOCK => "createElementBlock",
            CREATE_VNODE => "createVNode",
            CREATE_ELEMENT_VNODE => "createElementVNode",
            CREATE_COMMENT => "createCommentVNode",
            CREATE_TEXT => "createTextVNode",
            CREATE_STATIC => "createStaticVNode",
            RESOLVE_COMPONENT => "resolveComponent",
            RESOLVE_DYNAMIC_COMPONENT => "resolveDynamicComponent",
            RESOLVE_DIRECTIVE => "resolveDirective",
            RESOLVE_FILTER => "resolveFilter",
            WITH_DIRECTIVES => "withDirectives",
            RENDER_LIST => "renderList",
            RENDER_SLOT => "renderSlot",
            CREATE_SLOTS => "createSlots",
            TO_DISPLAY_STRING => "toDisplayString",
            MERGE_PROPS => "mergeProps",
            NORMALIZE_CLASS => "normalizeClass",
            NORMALIZE_STYLE => "normalizeStyle",
            NORMALIZE_PROPS => "normalizeProps",
            GUARD_REACTIVE_PROPS => "guardReactiveProps",
            TO_HANDLERS => "toHandlers",
            CAMELIZE => "camelize",
            CAPITALIZE => "capitalize",
            TO_HANDLER_KEY => "toHandlerKey",
            SET_BLOCK_TRACKING => "setBlockTracking",
            PUSH_SCOPE_ID => "pushScopeId",
            POP_SCOPE_ID => "popScopeId",
            WITH_CTX => "withCtx",
            UNREF => "unref",
            IS_REF => "isRef",
            WITH_MEMO => "withMemo",
            IS_MEMO_SAME => "isMemoSame",
            V_MODEL_RADIO => "vModelRadio",
            V_MODEL_CHECKBOX => "vModelCheckbox",
            V_MODEL_TEXT => "vModelText",
            V_MODEL_SELECT => "vModelSelect",
            V_MODEL_DYNAMIC => "vModelDynamic",
            V_ON_WITH_MODIFIERS => "withModifiers",
            V_ON_WITH_KEYS => "withKeys",
            V_SHOW => "vShow",
            TRANSITION => "Transition",
            TRANSITION_GROUP => "TransitionGroup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// signed, because the JS parser can emit `-1` offsets for truncated input
    pub offset: i64,
    pub line: i64,
    pub column: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub start: Position,
    pub end: Position,
    pub source: String,
}

pub fn loc_stub() -> SourceLocation {
    SourceLocation {
        start: Position {
            line: 1,
            column: 1,
            offset: 0,
        },
        end: Position {
            line: 1,
            column: 1,
            offset: 0,
        },
        source: String::new(),
    }
}

/// `SimpleExpressionNode.ast`: `undefined` (not parsed), `null` (plain
/// identifier fast path), `false` (parse error) or a parsed JS node.
#[derive(Debug, Clone, Default)]
pub enum ExpAst {
    #[default]
    Undefined,
    /// JS `null` — content is a simple identifier, no parse needed.
    Null,
    /// JS `false` — the expression failed to parse.
    Failed,
    Expr(Box<swc_ast::Expr>),
    Program(Box<swc_ast::Program>),
}

impl ExpAst {
    pub fn is_undefined(&self) -> bool {
        matches!(self, ExpAst::Undefined)
    }
    pub fn is_null(&self) -> bool {
        matches!(self, ExpAst::Null)
    }
    pub fn is_failed(&self) -> bool {
        matches!(self, ExpAst::Failed)
    }
}

/// Index into [`Arena`]. Node identity is what the JS compiler relies on:
/// codegen nodes alias template nodes and their children arrays, and later
/// passes mutate through those aliases.
pub type NodeId = u32;

#[derive(Debug, Clone)]
pub struct RootNode {
    pub source: String,
    pub children: Vec<NodeId>,
    /// insertion-ordered, like the JS `Set`
    pub helpers: Vec<RuntimeHelper>,
    pub components: Vec<String>,
    pub directives: Vec<String>,
    pub hoists: Vec<Option<NodeId>>,
    pub imports: Vec<ImportItem>,
    pub cached: Vec<Option<NodeId>>,
    pub temps: usize,
    pub codegen_node: Option<NodeId>,
    pub transformed: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ImportItem {
    pub exp: NodeId,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct ElementNode {
    pub ns: Namespace,
    pub tag: String,
    pub tag_type: ElementType,
    pub props: Vec<NodeId>,
    pub children: Vec<NodeId>,
    pub is_self_closing: bool,
    pub inner_loc: Option<SourceLocation>,
    pub codegen_node: Option<NodeId>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct TextNode {
    pub content: String,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct CommentNode {
    pub content: String,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct AttributeNode {
    pub name: String,
    pub name_loc: SourceLocation,
    pub value: Option<TextNode>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct DirectiveNode {
    pub name: String,
    pub raw_name: Option<String>,
    pub exp: Option<NodeId>,
    pub arg: Option<NodeId>,
    pub modifiers: Vec<NodeId>,
    pub for_parse_result: Option<ForParseResult>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct SimpleExpressionNode {
    pub content: String,
    pub is_static: bool,
    pub const_type: ConstantType,
    pub ast: ExpAst,
    /// points at a hoisted node by index into `RootNode::hoists`
    pub hoisted: Option<usize>,
    pub identifiers: Vec<String>,
    pub is_handler_key: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct InterpolationNode {
    pub content: NodeId,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct CompoundExpressionNode {
    pub children: Vec<NodeId>,
    pub ast: ExpAst,
    pub identifiers: Vec<String>,
    pub is_handler_key: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct IfNode {
    pub branches: Vec<NodeId>,
    pub codegen_node: Option<NodeId>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct IfBranchNode {
    pub condition: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub user_key: Option<NodeId>,
    pub is_template_if: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ForParseResult {
    pub source: NodeId,
    pub value: Option<NodeId>,
    pub key: Option<NodeId>,
    pub index: Option<NodeId>,
    pub finalized: bool,
}

#[derive(Debug, Clone)]
pub struct ForNode {
    pub source: NodeId,
    pub value_alias: Option<NodeId>,
    pub key_alias: Option<NodeId>,
    pub object_index_alias: Option<NodeId>,
    pub parse_result: ForParseResult,
    pub children: Vec<NodeId>,
    pub codegen_node: Option<NodeId>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct TextCallNode {
    pub content: NodeId,
    pub codegen_node: Option<NodeId>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct VNodeCall {
    /// `string | symbol | CallExpression`
    pub tag: NodeId,
    pub props: Option<NodeId>,
    pub children: Option<NodeId>,
    pub patch_flag: Option<i32>,
    /// `string | SimpleExpressionNode`
    pub dynamic_props: Option<NodeId>,
    pub directives: Option<NodeId>,
    pub is_block: bool,
    pub disable_tracking: bool,
    pub is_component: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct CallExpression {
    /// `string | symbol`
    pub callee: NodeId,
    pub arguments: Vec<NodeId>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ObjectExpression {
    pub properties: Vec<NodeId>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct Property {
    pub key: NodeId,
    pub value: NodeId,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ArrayExpression {
    pub elements: Vec<NodeId>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct FunctionExpression {
    /// `ExpressionNode | string | (ExpressionNode | string)[] | undefined`
    pub params: Option<NodeId>,
    pub returns: Option<NodeId>,
    pub body: Option<NodeId>,
    pub newline: bool,
    pub is_slot: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ConditionalExpression {
    pub test: NodeId,
    pub consequent: NodeId,
    pub alternate: NodeId,
    pub newline: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct CacheExpression {
    pub index: usize,
    pub value: NodeId,
    pub need_pause_tracking: bool,
    pub in_v_once: bool,
    pub need_array_spread: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone, Default)]
pub enum Node {
    Root(Box<RootNode>),
    Element(Box<ElementNode>),
    Text(Box<TextNode>),
    Comment(Box<CommentNode>),
    SimpleExpression(Box<SimpleExpressionNode>),
    Interpolation(Box<InterpolationNode>),
    Attribute(Box<AttributeNode>),
    Directive(Box<DirectiveNode>),
    CompoundExpression(Box<CompoundExpressionNode>),
    If(Box<IfNode>),
    IfBranch(Box<IfBranchNode>),
    For(Box<ForNode>),
    TextCall(Box<TextCallNode>),
    VNodeCall(Box<VNodeCall>),
    CallExpression(Box<CallExpression>),
    ObjectExpression(Box<ObjectExpression>),
    Property(Box<Property>),
    ArrayExpression(Box<ArrayExpression>),
    FunctionExpression(Box<FunctionExpression>),
    ConditionalExpression(Box<ConditionalExpression>),
    CacheExpression(Box<CacheExpression>),
    /// a raw string member of a heterogeneous array (emitted verbatim)
    Str(String),
    /// a runtime-helper symbol member of a heterogeneous array
    Sym(RuntimeHelper),
    /// an owned list (a JS array that is not shared with any node)
    Nodes(Vec<NodeId>),
    /// an alias of another node's `children` array — the JS compiler passes
    /// those arrays around by reference and mutates them in place
    ChildrenRef(NodeId),
    /// JS `undefined`
    #[default]
    None,
}

/// `NodeTypes` ordinals, needed wherever the JS code compares `node.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum NodeType {
    Root = 0,
    Element = 1,
    Text = 2,
    Comment = 3,
    SimpleExpression = 4,
    Interpolation = 5,
    Attribute = 6,
    Directive = 7,
    CompoundExpression = 8,
    If = 9,
    IfBranch = 10,
    For = 11,
    TextCall = 12,
    VNodeCall = 13,
    JsCallExpression = 14,
    JsObjectExpression = 15,
    JsProperty = 16,
    JsArrayExpression = 17,
    JsFunctionExpression = 18,
    JsConditionalExpression = 19,
    JsCacheExpression = 20,
    Other = 99,
}

impl Node {
    pub fn node_type(&self) -> NodeType {
        match self {
            Node::Root(_) => NodeType::Root,
            Node::Element(_) => NodeType::Element,
            Node::Text(_) => NodeType::Text,
            Node::Comment(_) => NodeType::Comment,
            Node::SimpleExpression(_) => NodeType::SimpleExpression,
            Node::Interpolation(_) => NodeType::Interpolation,
            Node::Attribute(_) => NodeType::Attribute,
            Node::Directive(_) => NodeType::Directive,
            Node::CompoundExpression(_) => NodeType::CompoundExpression,
            Node::If(_) => NodeType::If,
            Node::IfBranch(_) => NodeType::IfBranch,
            Node::For(_) => NodeType::For,
            Node::TextCall(_) => NodeType::TextCall,
            Node::VNodeCall(_) => NodeType::VNodeCall,
            Node::CallExpression(_) => NodeType::JsCallExpression,
            Node::ObjectExpression(_) => NodeType::JsObjectExpression,
            Node::Property(_) => NodeType::JsProperty,
            Node::ArrayExpression(_) => NodeType::JsArrayExpression,
            Node::FunctionExpression(_) => NodeType::JsFunctionExpression,
            Node::ConditionalExpression(_) => NodeType::JsConditionalExpression,
            Node::CacheExpression(_) => NodeType::JsCacheExpression,
            _ => NodeType::Other,
        }
    }

    pub fn loc(&self) -> &SourceLocation {
        match self {
            Node::Root(n) => &n.loc,
            Node::Element(n) => &n.loc,
            Node::Text(n) => &n.loc,
            Node::Comment(n) => &n.loc,
            Node::SimpleExpression(n) => &n.loc,
            Node::Interpolation(n) => &n.loc,
            Node::Attribute(n) => &n.loc,
            Node::Directive(n) => &n.loc,
            Node::CompoundExpression(n) => &n.loc,
            Node::If(n) => &n.loc,
            Node::IfBranch(n) => &n.loc,
            Node::For(n) => &n.loc,
            Node::TextCall(n) => &n.loc,
            Node::VNodeCall(n) => &n.loc,
            Node::CallExpression(n) => &n.loc,
            Node::ObjectExpression(n) => &n.loc,
            Node::Property(n) => &n.loc,
            Node::ArrayExpression(n) => &n.loc,
            Node::FunctionExpression(n) => &n.loc,
            Node::ConditionalExpression(n) => &n.loc,
            Node::CacheExpression(n) => &n.loc,
            _ => &STUB_LOC,
        }
    }
}

static STUB_LOC: std::sync::LazyLock<SourceLocation> = std::sync::LazyLock::new(loc_stub);

/// Owns every node. Ids are stable; nothing is ever freed.
#[derive(Debug, Default)]
pub struct Arena {
    nodes: Vec<Node>,
}

macro_rules! typed_accessors {
    ($($get:ident, $get_mut:ident, $variant:ident, $ty:ty);* $(;)?) => {
        impl Arena {
            $(
                #[track_caller]
                pub fn $get(&self, id: NodeId) -> &$ty {
                    match self.node(id) {
                        Node::$variant(n) => n,
                        other => panic!(concat!("expected ", stringify!($variant), ", got {:?}"), other.node_type()),
                    }
                }
                #[track_caller]
                pub fn $get_mut(&mut self, id: NodeId) -> &mut $ty {
                    match self.node_mut(id) {
                        Node::$variant(n) => n,
                        other => panic!(concat!("expected ", stringify!($variant), ", got {:?}"), other.node_type()),
                    }
                }
            )*
        }
    };
}

typed_accessors! {
    root, root_mut, Root, RootNode;
    el, el_mut, Element, ElementNode;
    text, text_mut, Text, TextNode;
    comment, comment_mut, Comment, CommentNode;
    exp, exp_mut, SimpleExpression, SimpleExpressionNode;
    interp, interp_mut, Interpolation, InterpolationNode;
    attr, attr_mut, Attribute, AttributeNode;
    dir, dir_mut, Directive, DirectiveNode;
    compound, compound_mut, CompoundExpression, CompoundExpressionNode;
    if_node, if_node_mut, If, IfNode;
    branch, branch_mut, IfBranch, IfBranchNode;
    for_node, for_node_mut, For, ForNode;
    text_call, text_call_mut, TextCall, TextCallNode;
    vnode, vnode_mut, VNodeCall, VNodeCall;
    call, call_mut, CallExpression, CallExpression;
    obj, obj_mut, ObjectExpression, ObjectExpression;
    prop, prop_mut, Property, Property;
    array, array_mut, ArrayExpression, ArrayExpression;
    func, func_mut, FunctionExpression, FunctionExpression;
    cond, cond_mut, ConditionalExpression, ConditionalExpression;
    cache, cache_mut, CacheExpression, CacheExpression;
}

impl Arena {
    pub fn new() -> Self {
        Arena { nodes: Vec::new() }
    }

    pub fn add(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        (self.nodes.len() - 1) as NodeId
    }

    #[track_caller]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    #[track_caller]
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    pub fn node_type(&self, id: NodeId) -> NodeType {
        self.node(id).node_type()
    }

    pub fn loc(&self, id: NodeId) -> &SourceLocation {
        self.node(id).loc()
    }

    pub fn is(&self, id: NodeId, t: NodeType) -> bool {
        self.node_type(id) == t
    }

    /// `node.children` for the container types, or the list an array-valued
    /// node stands for.
    #[track_caller]
    pub fn list(&self, id: NodeId) -> &Vec<NodeId> {
        match self.node(id) {
            Node::Nodes(v) => v,
            Node::ChildrenRef(owner) => self.children_of(*owner),
            Node::Root(r) => &r.children,
            Node::Element(e) => &e.children,
            Node::IfBranch(b) => &b.children,
            Node::For(f) => &f.children,
            other => panic!("not a list node: {:?}", other.node_type()),
        }
    }

    #[track_caller]
    pub fn list_mut(&mut self, id: NodeId) -> &mut Vec<NodeId> {
        let target = match self.node(id) {
            Node::ChildrenRef(owner) => *owner,
            _ => id,
        };
        match self.node_mut(target) {
            Node::Nodes(v) => v,
            Node::Root(r) => &mut r.children,
            Node::Element(e) => &mut e.children,
            Node::IfBranch(b) => &mut b.children,
            Node::For(f) => &mut f.children,
            other => panic!("not a list node: {:?}", other.node_type()),
        }
    }

    #[track_caller]
    pub fn children_of(&self, id: NodeId) -> &Vec<NodeId> {
        match self.node(id) {
            Node::Root(r) => &r.children,
            Node::Element(e) => &e.children,
            Node::IfBranch(b) => &b.children,
            Node::For(f) => &f.children,
            other => panic!("no children on {:?}", other.node_type()),
        }
    }

    #[track_caller]
    pub fn children_of_mut(&mut self, id: NodeId) -> &mut Vec<NodeId> {
        match self.node_mut(id) {
            Node::Root(r) => &mut r.children,
            Node::Element(e) => &mut e.children,
            Node::IfBranch(b) => &mut b.children,
            Node::For(f) => &mut f.children,
            other => panic!("no children on {:?}", other.node_type()),
        }
    }

    pub fn is_list_like(&self, id: NodeId) -> bool {
        matches!(self.node(id), Node::Nodes(_) | Node::ChildrenRef(_))
    }

    pub fn str_of(&self, id: NodeId) -> Option<&str> {
        match self.node(id) {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn sym_of(&self, id: NodeId) -> Option<RuntimeHelper> {
        match self.node(id) {
            Node::Sym(h) => Some(*h),
            _ => None,
        }
    }

    // --- constructors -------------------------------------------------------

    pub fn nodes(&mut self, list: Vec<NodeId>) -> NodeId {
        self.add(Node::Nodes(list))
    }

    pub fn children_ref(&mut self, owner: NodeId) -> NodeId {
        self.add(Node::ChildrenRef(owner))
    }

    pub fn string(&mut self, s: impl Into<String>) -> NodeId {
        self.add(Node::Str(s.into()))
    }

    pub fn sym(&mut self, h: RuntimeHelper) -> NodeId {
        self.add(Node::Sym(h))
    }

    pub fn create_root(&mut self, children: Vec<NodeId>, source: String) -> NodeId {
        self.add(Node::Root(Box::new(RootNode {
            source,
            children,
            helpers: Vec::new(),
            components: Vec::new(),
            directives: Vec::new(),
            hoists: Vec::new(),
            imports: Vec::new(),
            cached: Vec::new(),
            temps: 0,
            codegen_node: None,
            transformed: false,
            loc: loc_stub(),
        })))
    }

    pub fn create_simple_expression(
        &mut self,
        content: impl Into<String>,
        is_static: bool,
        loc: SourceLocation,
        const_type: ConstantType,
    ) -> NodeId {
        self.add(Node::SimpleExpression(Box::new(SimpleExpressionNode {
            content: content.into(),
            is_static,
            const_type: if is_static {
                ConstantType::CanStringify
            } else {
                const_type
            },
            ast: ExpAst::Undefined,
            hoisted: None,
            identifiers: Vec::new(),
            is_handler_key: false,
            loc,
        })))
    }

    /// `createSimpleExpression(content, isStatic)` with default loc/constType.
    pub fn simple_exp(&mut self, content: impl Into<String>, is_static: bool) -> NodeId {
        self.create_simple_expression(content, is_static, loc_stub(), ConstantType::NotConstant)
    }

    pub fn create_compound_expression(
        &mut self,
        children: Vec<NodeId>,
        loc: SourceLocation,
    ) -> NodeId {
        self.add(Node::CompoundExpression(Box::new(CompoundExpressionNode {
            children,
            ast: ExpAst::Undefined,
            identifiers: Vec::new(),
            is_handler_key: false,
            loc,
        })))
    }

    pub fn create_object_property(&mut self, key: NodeId, value: NodeId) -> NodeId {
        self.add(Node::Property(Box::new(Property {
            key,
            value,
            loc: loc_stub(),
        })))
    }

    /// `createObjectProperty(stringKey, value)`
    pub fn create_object_property_str(&mut self, key: &str, value: NodeId) -> NodeId {
        let k = self.simple_exp(key, true);
        self.create_object_property(k, value)
    }

    pub fn create_object_expression(&mut self, properties: Vec<NodeId>) -> NodeId {
        self.add(Node::ObjectExpression(Box::new(ObjectExpression {
            properties,
            loc: loc_stub(),
        })))
    }

    pub fn create_array_expression(&mut self, elements: Vec<NodeId>) -> NodeId {
        self.add(Node::ArrayExpression(Box::new(ArrayExpression {
            elements,
            loc: loc_stub(),
        })))
    }

    pub fn create_call_expression(&mut self, callee: NodeId, arguments: Vec<NodeId>) -> NodeId {
        self.add(Node::CallExpression(Box::new(CallExpression {
            callee,
            arguments,
            loc: loc_stub(),
        })))
    }

    pub fn create_call_helper(&mut self, callee: RuntimeHelper, arguments: Vec<NodeId>) -> NodeId {
        let c = self.sym(callee);
        self.create_call_expression(c, arguments)
    }

    pub fn create_function_expression(
        &mut self,
        params: Option<NodeId>,
        returns: Option<NodeId>,
        newline: bool,
        is_slot: bool,
        loc: SourceLocation,
    ) -> NodeId {
        self.add(Node::FunctionExpression(Box::new(FunctionExpression {
            params,
            returns,
            body: None,
            newline,
            is_slot,
            loc,
        })))
    }

    pub fn create_conditional_expression(
        &mut self,
        test: NodeId,
        consequent: NodeId,
        alternate: NodeId,
        newline: bool,
    ) -> NodeId {
        self.add(Node::ConditionalExpression(Box::new(
            ConditionalExpression {
                test,
                consequent,
                alternate,
                newline,
                loc: loc_stub(),
            },
        )))
    }

    pub fn create_cache_expression(
        &mut self,
        index: usize,
        value: NodeId,
        need_pause_tracking: bool,
        in_v_once: bool,
    ) -> NodeId {
        self.add(Node::CacheExpression(Box::new(CacheExpression {
            index,
            value,
            need_pause_tracking,
            in_v_once,
            need_array_spread: false,
            loc: loc_stub(),
        })))
    }

    pub fn create_interpolation(&mut self, content: NodeId, loc: SourceLocation) -> NodeId {
        self.add(Node::Interpolation(Box::new(InterpolationNode {
            content,
            loc,
        })))
    }
}

pub fn get_vnode_helper(ssr: bool, is_component: bool) -> RuntimeHelper {
    if ssr || is_component {
        RuntimeHelper::CREATE_VNODE
    } else {
        RuntimeHelper::CREATE_ELEMENT_VNODE
    }
}

pub fn get_vnode_block_helper(ssr: bool, is_component: bool) -> RuntimeHelper {
    if ssr || is_component {
        RuntimeHelper::CREATE_BLOCK
    } else {
        RuntimeHelper::CREATE_ELEMENT_BLOCK
    }
}
