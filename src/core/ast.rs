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
    pub offset: usize,
    pub line: usize,
    pub column: usize,
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

#[derive(Debug, Clone)]
pub struct RootNode {
    pub source: String,
    pub children: Vec<Node>,
    /// insertion-ordered, like the JS `Set`
    pub helpers: Vec<RuntimeHelper>,
    pub components: Vec<String>,
    pub directives: Vec<String>,
    pub hoists: Vec<Option<Node>>,
    pub imports: Vec<ImportItem>,
    pub cached: Vec<Option<Node>>,
    pub temps: usize,
    pub codegen_node: Option<Node>,
    pub transformed: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ImportItem {
    pub exp: Node,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct ElementNode {
    pub ns: Namespace,
    pub tag: String,
    pub tag_type: ElementType,
    pub props: Vec<Node>,
    pub children: Vec<Node>,
    pub is_self_closing: bool,
    pub inner_loc: Option<SourceLocation>,
    pub codegen_node: Option<Node>,
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
    pub exp: Option<Node>,
    pub arg: Option<Node>,
    pub modifiers: Vec<Node>,
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
    pub content: Node,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct CompoundExpressionNode {
    pub children: Vec<Node>,
    pub ast: ExpAst,
    pub identifiers: Vec<String>,
    pub is_handler_key: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct IfNode {
    pub branches: Vec<Node>,
    pub codegen_node: Option<Node>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct IfBranchNode {
    pub condition: Option<Node>,
    pub children: Vec<Node>,
    pub user_key: Option<Node>,
    pub is_template_if: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ForParseResult {
    pub source: Node,
    pub value: Option<Node>,
    pub key: Option<Node>,
    pub index: Option<Node>,
    pub finalized: bool,
}

#[derive(Debug, Clone)]
pub struct ForNode {
    pub source: Node,
    pub value_alias: Option<Node>,
    pub key_alias: Option<Node>,
    pub object_index_alias: Option<Node>,
    pub parse_result: ForParseResult,
    pub children: Vec<Node>,
    pub codegen_node: Option<Node>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct TextCallNode {
    pub content: Node,
    pub codegen_node: Option<Node>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct VNodeCall {
    /// `string | symbol | CallExpression`
    pub tag: Node,
    pub props: Option<Node>,
    pub children: Option<Node>,
    pub patch_flag: Option<i32>,
    /// `string | SimpleExpressionNode`
    pub dynamic_props: Option<Node>,
    pub directives: Option<Node>,
    pub is_block: bool,
    pub disable_tracking: bool,
    pub is_component: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct CallExpression {
    /// `string | symbol`
    pub callee: Node,
    pub arguments: Vec<Node>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ObjectExpression {
    pub properties: Vec<Node>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct Property {
    pub key: Node,
    pub value: Node,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ArrayExpression {
    pub elements: Vec<Node>,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct FunctionExpression {
    /// `ExpressionNode | string | (ExpressionNode | string)[] | undefined`
    pub params: Option<Node>,
    pub returns: Option<Node>,
    pub body: Option<Node>,
    pub newline: bool,
    pub is_slot: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct ConditionalExpression {
    pub test: Node,
    pub consequent: Node,
    pub alternate: Node,
    pub newline: bool,
    pub loc: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct CacheExpression {
    pub index: usize,
    pub value: Node,
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
    /// a nested `TemplateChildNode[]` used where one member is expected
    Nodes(Vec<Node>),
    /// JS `undefined` / removed node
    #[default]
    None,
}

/// `NodeTypes` ordinals, needed wherever the JS code compares `node.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    pub fn is_none(&self) -> bool {
        matches!(self, Node::None)
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

    // --- narrowing helpers (panic on mismatch: the JS code is already typed) ---

    pub fn as_element(&self) -> &ElementNode {
        match self {
            Node::Element(e) => e,
            _ => panic!("expected element node"),
        }
    }
    pub fn as_element_mut(&mut self) -> &mut ElementNode {
        match self {
            Node::Element(e) => e,
            _ => panic!("expected element node"),
        }
    }
    pub fn as_simple_exp(&self) -> &SimpleExpressionNode {
        match self {
            Node::SimpleExpression(e) => e,
            _ => panic!("expected simple expression"),
        }
    }
    pub fn as_simple_exp_mut(&mut self) -> &mut SimpleExpressionNode {
        match self {
            Node::SimpleExpression(e) => e,
            _ => panic!("expected simple expression"),
        }
    }
    pub fn as_directive(&self) -> &DirectiveNode {
        match self {
            Node::Directive(d) => d,
            _ => panic!("expected directive"),
        }
    }
    pub fn as_attribute(&self) -> &AttributeNode {
        match self {
            Node::Attribute(a) => a,
            _ => panic!("expected attribute"),
        }
    }
    pub fn as_text(&self) -> &TextNode {
        match self {
            Node::Text(t) => t,
            _ => panic!("expected text"),
        }
    }
    pub fn as_root(&self) -> &RootNode {
        match self {
            Node::Root(r) => r,
            _ => panic!("expected root"),
        }
    }
    pub fn as_root_mut(&mut self) -> &mut RootNode {
        match self {
            Node::Root(r) => r,
            _ => panic!("expected root"),
        }
    }
}

static STUB_LOC: std::sync::LazyLock<SourceLocation> = std::sync::LazyLock::new(loc_stub);

// --- constructors -----------------------------------------------------------

pub fn create_root(children: Vec<Node>, source: String) -> RootNode {
    RootNode {
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
    }
}

pub fn create_simple_expression(
    content: impl Into<String>,
    is_static: bool,
    loc: SourceLocation,
    const_type: ConstantType,
) -> Node {
    Node::SimpleExpression(Box::new(SimpleExpressionNode {
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
    }))
}

/// `createSimpleExpression(content, isStatic)` with default loc/constType.
pub fn simple_exp(content: impl Into<String>, is_static: bool) -> Node {
    create_simple_expression(content, is_static, loc_stub(), ConstantType::NotConstant)
}

pub fn create_compound_expression(children: Vec<Node>, loc: SourceLocation) -> Node {
    Node::CompoundExpression(Box::new(CompoundExpressionNode {
        children,
        ast: ExpAst::Undefined,
        identifiers: Vec::new(),
        is_handler_key: false,
        loc,
    }))
}

pub fn create_object_property(key: Node, value: Node) -> Node {
    Node::Property(Box::new(Property {
        key,
        value,
        loc: loc_stub(),
    }))
}

pub fn create_object_expression(properties: Vec<Node>, loc: SourceLocation) -> Node {
    Node::ObjectExpression(Box::new(ObjectExpression { properties, loc }))
}

pub fn create_array_expression(elements: Vec<Node>, loc: SourceLocation) -> Node {
    Node::ArrayExpression(Box::new(ArrayExpression { elements, loc }))
}

pub fn create_call_expression(callee: Node, arguments: Vec<Node>, loc: SourceLocation) -> Node {
    Node::CallExpression(Box::new(CallExpression {
        callee,
        arguments,
        loc,
    }))
}

pub fn create_function_expression(
    params: Option<Node>,
    returns: Option<Node>,
    newline: bool,
    is_slot: bool,
    loc: SourceLocation,
) -> Node {
    Node::FunctionExpression(Box::new(FunctionExpression {
        params,
        returns,
        body: None,
        newline,
        is_slot,
        loc,
    }))
}

pub fn create_conditional_expression(
    test: Node,
    consequent: Node,
    alternate: Node,
    newline: bool,
) -> Node {
    Node::ConditionalExpression(Box::new(ConditionalExpression {
        test,
        consequent,
        alternate,
        newline,
        loc: loc_stub(),
    }))
}

pub fn create_cache_expression(
    index: usize,
    value: Node,
    need_pause_tracking: bool,
    in_v_once: bool,
) -> Node {
    Node::CacheExpression(Box::new(CacheExpression {
        index,
        value,
        need_pause_tracking,
        in_v_once,
        need_array_spread: false,
        loc: loc_stub(),
    }))
}

pub fn create_interpolation(content: Node, loc: SourceLocation) -> Node {
    Node::Interpolation(Box::new(InterpolationNode { content, loc }))
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
