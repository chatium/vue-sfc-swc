//! Port of `compiler-core/src/codegen.ts`.
//!
//! Source-map generation is deliberately omitted: `compileTemplate` produces a
//! map, but the SFC pipeline we target never reads it.

use super::ast::*;
use super::options::{CodegenMode, CodegenOptions};
use super::utils::{is_simple_identifier, to_valid_asset_id};

const PURE_ANNOTATION: &str = "/*@__PURE__*/";

pub struct CodegenResult {
    pub code: String,
    pub preamble: String,
}

struct CodegenContext<'a> {
    a: &'a Arena,
    opts: CodegenOptions,
    code: String,
    indent_level: usize,
    pure: bool,
}

impl<'a> CodegenContext<'a> {
    fn push(&mut self, code: &str) {
        self.code.push_str(code);
    }
    fn helper(&self, h: RuntimeHelper) -> String {
        format!("_{}", h.name())
    }
    fn indent(&mut self) {
        self.indent_level += 1;
        self.newline_at(self.indent_level);
    }
    fn deindent(&mut self, without_newline: bool) {
        if without_newline {
            self.indent_level -= 1;
        } else {
            self.indent_level -= 1;
            self.newline_at(self.indent_level);
        }
    }
    fn newline(&mut self) {
        self.newline_at(self.indent_level);
    }
    fn newline_at(&mut self, n: usize) {
        self.code.push('\n');
        for _ in 0..n {
            self.code.push_str("  ");
        }
    }
}

fn alias_helper(h: RuntimeHelper) -> String {
    format!("{}: _{}", h.name(), h.name())
}

pub fn generate(a: &Arena, root_id: NodeId, opts: CodegenOptions) -> CodegenResult {
    let mut ctx = CodegenContext {
        a,
        opts: opts.clone(),
        code: String::new(),
        indent_level: 0,
        pure: false,
    };

    let root = a.root(root_id);
    let helpers = root.helpers.clone();
    let has_helpers = !helpers.is_empty();
    let use_with_block = !opts.prefix_identifiers && opts.mode != CodegenMode::Module;
    let gen_scope_id = opts.scope_id.is_some() && opts.mode == CodegenMode::Module;
    let is_setup_inlined = opts.inline;

    let mut preamble = String::new();
    if is_setup_inlined {
        let mut pre = CodegenContext {
            a,
            opts: opts.clone(),
            code: String::new(),
            indent_level: 0,
            pure: false,
        };
        if opts.mode == CodegenMode::Module {
            gen_module_preamble(root_id, &mut pre, gen_scope_id, true);
        } else {
            gen_function_preamble(root_id, &mut pre);
        }
        preamble = pre.code;
    } else if opts.mode == CodegenMode::Module {
        gen_module_preamble(root_id, &mut ctx, gen_scope_id, false);
    } else {
        gen_function_preamble(root_id, &mut ctx);
    }

    let function_name = if opts.ssr { "ssrRender" } else { "render" };
    let mut args: Vec<&str> = if opts.ssr {
        vec!["_ctx", "_push", "_parent", "_attrs"]
    } else {
        vec!["_ctx", "_cache"]
    };
    if opts.has_binding_metadata && !opts.inline {
        args.extend(["$props", "$setup", "$data", "$options"]);
    }
    let signature = if opts.is_ts {
        args.iter()
            .map(|a| format!("{a}: any"))
            .collect::<Vec<_>>()
            .join(",")
    } else {
        args.join(", ")
    };

    if is_setup_inlined {
        ctx.push(&format!("({signature}) => {{"));
    } else {
        ctx.push(&format!("function {function_name}({signature}) {{"));
    }
    ctx.indent();

    if use_with_block {
        ctx.push("with (_ctx) {");
        ctx.indent();
        if has_helpers {
            let aliases = helpers
                .iter()
                .map(|h| alias_helper(*h))
                .collect::<Vec<_>>()
                .join(", ");
            ctx.push(&format!("const {{ {aliases} }} = _Vue\n"));
            ctx.newline();
        }
    }

    if !root.components.is_empty() {
        gen_assets(&root.components, "component", &mut ctx);
        if !root.directives.is_empty() || root.temps > 0 {
            ctx.newline();
        }
    }
    if !root.directives.is_empty() {
        gen_assets(&root.directives, "directive", &mut ctx);
        if root.temps > 0 {
            ctx.newline();
        }
    }
    if root.temps > 0 {
        ctx.push("let ");
        for i in 0..root.temps {
            ctx.push(&format!("{}_temp{i}", if i > 0 { ", " } else { "" }));
        }
    }
    if !root.components.is_empty() || !root.directives.is_empty() || root.temps > 0 {
        ctx.push("\n");
        ctx.newline();
    }

    if !opts.ssr {
        ctx.push("return ");
    }
    match root.codegen_node {
        Some(n) => gen_node(n, &mut ctx),
        None => ctx.push("null"),
    }

    if use_with_block {
        ctx.deindent(false);
        ctx.push("}");
    }
    ctx.deindent(false);
    ctx.push("}");

    CodegenResult {
        code: ctx.code,
        preamble,
    }
}

fn gen_function_preamble(root_id: NodeId, ctx: &mut CodegenContext) {
    let root = ctx.a.root(root_id);
    let vue_binding = if ctx.opts.ssr {
        format!(
            "require({})",
            serde_json::to_string(&ctx.opts.runtime_module_name).unwrap()
        )
    } else {
        ctx.opts.runtime_global_name.clone()
    };
    let helpers = root.helpers.clone();
    let hoists_len = root.hoists.len();
    if !helpers.is_empty() {
        if ctx.opts.prefix_identifiers {
            let aliases = helpers
                .iter()
                .map(|h| alias_helper(*h))
                .collect::<Vec<_>>()
                .join(", ");
            ctx.push(&format!("const {{ {aliases} }} = {vue_binding}\n"));
        } else {
            ctx.push(&format!("const _Vue = {vue_binding}\n"));
            if hoists_len > 0 {
                let static_helpers = [
                    RuntimeHelper::CREATE_VNODE,
                    RuntimeHelper::CREATE_ELEMENT_VNODE,
                    RuntimeHelper::CREATE_COMMENT,
                    RuntimeHelper::CREATE_TEXT,
                    RuntimeHelper::CREATE_STATIC,
                ]
                .iter()
                .filter(|h| helpers.contains(h))
                .map(|h| alias_helper(*h))
                .collect::<Vec<_>>()
                .join(", ");
                ctx.push(&format!("const {{ {static_helpers} }} = _Vue\n"));
            }
        }
    }
    if !root.ssr_helpers.is_empty() {
        let aliases = root
            .ssr_helpers
            .iter()
            .map(|h| alias_helper(*h))
            .collect::<Vec<_>>()
            .join(", ");
        ctx.push(&format!(
            "const {{ {aliases} }} = require(\"{}\")\n",
            ctx.opts.ssr_runtime_module_name
        ));
    }
    gen_hoists(root_id, ctx);
    ctx.newline();
    ctx.push("return ");
}

fn gen_module_preamble(
    root_id: NodeId,
    ctx: &mut CodegenContext,
    _gen_scope_id: bool,
    inline: bool,
) {
    let root = ctx.a.root(root_id);
    let helpers = root.helpers.clone();
    let imports = root.imports.clone();
    if !helpers.is_empty() {
        if ctx.opts.optimize_imports {
            let names = helpers
                .iter()
                .map(|h| h.name().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            ctx.push(&format!(
                "import {{ {names} }} from {}\n",
                serde_json::to_string(&ctx.opts.runtime_module_name).unwrap()
            ));
            let binds = helpers
                .iter()
                .map(|h| format!("_{} = {}", h.name(), h.name()))
                .collect::<Vec<_>>()
                .join(", ");
            ctx.push(&format!(
                "\n// Binding optimization for webpack code-split\nconst {binds}\n"
            ));
        } else {
            let names = helpers
                .iter()
                .map(|h| format!("{} as _{}", h.name(), h.name()))
                .collect::<Vec<_>>()
                .join(", ");
            ctx.push(&format!(
                "import {{ {names} }} from {}\n",
                serde_json::to_string(&ctx.opts.runtime_module_name).unwrap()
            ));
        }
    }

    if !root.ssr_helpers.is_empty() {
        let names = root
            .ssr_helpers
            .iter()
            .map(|h| format!("{} as _{}", h.name(), h.name()))
            .collect::<Vec<_>>()
            .join(", ");
        ctx.push(&format!(
            "import {{ {names} }} from \"{}\"\n",
            ctx.opts.ssr_runtime_module_name
        ));
    }

    if !imports.is_empty() {
        for imp in &imports {
            ctx.push("import ");
            gen_node(imp.exp, ctx);
            ctx.push(&format!(" from '{}'", imp.path));
            ctx.newline();
        }
        ctx.newline();
    }

    gen_hoists(root_id, ctx);
    ctx.newline();
    if !inline {
        ctx.push("export ");
    }
}

fn gen_assets(assets: &[String], kind: &str, ctx: &mut CodegenContext) {
    let resolver = ctx.helper(if kind == "component" {
        RuntimeHelper::RESOLVE_COMPONENT
    } else {
        RuntimeHelper::RESOLVE_DIRECTIVE
    });
    for i in 0..assets.len() {
        let mut id = assets[i].clone();
        let maybe_self_reference = id.ends_with("__self");
        if maybe_self_reference {
            id.truncate(id.len() - 6);
        }
        ctx.push(&format!(
            "const {} = {resolver}({}{}){}",
            to_valid_asset_id(&id, kind),
            serde_json::to_string(&id).unwrap(),
            if maybe_self_reference { ", true" } else { "" },
            if ctx.opts.is_ts { "!" } else { "" }
        ));
        if i < assets.len() - 1 {
            ctx.newline();
        }
    }
}

fn gen_hoists(root_id: NodeId, ctx: &mut CodegenContext) {
    let hoists = ctx.a.root(root_id).hoists.clone();
    if hoists.is_empty() {
        return;
    }
    ctx.pure = true;
    ctx.newline();
    for (i, exp) in hoists.iter().enumerate() {
        if let Some(exp) = exp {
            ctx.push(&format!("const _hoisted_{} = ", i + 1));
            gen_node(*exp, ctx);
            ctx.newline();
        }
    }
    ctx.pure = false;
}

fn is_text_like(a: &Arena, n: NodeId) -> bool {
    matches!(
        a.node(n),
        Node::Str(_)
            | Node::SimpleExpression(_)
            | Node::Text(_)
            | Node::Interpolation(_)
            | Node::CompoundExpression(_)
    )
}

fn gen_node_list_as_array(nodes: &[NodeId], ctx: &mut CodegenContext) {
    let multilines = nodes.len() > 3
        || nodes
            .iter()
            .any(|n| ctx.a.is_list_like(*n) || !is_text_like(ctx.a, *n));
    ctx.push("[");
    if multilines {
        ctx.indent();
    }
    gen_node_list(nodes, ctx, multilines, true);
    if multilines {
        ctx.deindent(false);
    }
    ctx.push("]");
}

fn gen_node_list(nodes: &[NodeId], ctx: &mut CodegenContext, multilines: bool, comma: bool) {
    for i in 0..nodes.len() {
        let node = nodes[i];
        if ctx.a.is_list_like(node) {
            let list = ctx.a.list(node).clone();
            gen_node_list_as_array(&list, ctx);
        } else {
            gen_node(node, ctx);
        }
        if i < nodes.len() - 1 {
            if multilines {
                if comma {
                    ctx.push(",");
                }
                ctx.newline();
            } else if comma {
                ctx.push(", ");
            }
        }
    }
}

fn gen_node(node: NodeId, ctx: &mut CodegenContext) {
    // the arena outlives `ctx`, so its nodes can be read while pushing
    let a = ctx.a;
    match a.node(node) {
        Node::Str(s) => ctx.push(s),
        Node::Sym(h) => {
            let s = ctx.helper(*h);
            ctx.push(&s);
        }
        Node::Element(e) => {
            let c = e.codegen_node.expect("codegen node missing for element");
            gen_node(c, ctx);
        }
        Node::If(n) => {
            let c = n.codegen_node.expect("codegen node missing for v-if");
            gen_node(c, ctx);
        }
        Node::For(n) => {
            let c = n.codegen_node.expect("codegen node missing for v-for");
            gen_node(c, ctx);
        }
        Node::Text(t) => {
            let s = serde_json::to_string(&t.content).unwrap();
            ctx.push(&s);
        }
        Node::SimpleExpression(e) => {
            if e.is_static {
                ctx.push(&serde_json::to_string(&e.content).unwrap());
            } else {
                ctx.push(&e.content);
            }
        }
        Node::Interpolation(i) => {
            let content = i.content;
            if ctx.pure {
                ctx.push(PURE_ANNOTATION);
            }
            let h = ctx.helper(RuntimeHelper::TO_DISPLAY_STRING);
            ctx.push(&format!("{h}("));
            gen_node(content, ctx);
            ctx.push(")");
        }
        Node::TextCall(t) => {
            let c = t.codegen_node.unwrap();
            gen_node(c, ctx);
        }
        Node::CompoundExpression(c) => {
            for &child in &c.children {
                gen_node(child, ctx);
            }
        }
        Node::Comment(c) => {
            let content = serde_json::to_string(&c.content).unwrap();
            if ctx.pure {
                ctx.push(PURE_ANNOTATION);
            }
            let h = ctx.helper(RuntimeHelper::CREATE_COMMENT);
            ctx.push(&format!("{h}({content})"));
        }
        Node::VNodeCall(_) => gen_vnode_call(node, ctx),
        Node::CallExpression(_) => gen_call_expression(node, ctx),
        Node::ObjectExpression(_) => gen_object_expression(node, ctx),
        Node::ArrayExpression(_) => {
            let elements = ctx.a.list(node).clone();
            gen_node_list_as_array(&elements, ctx);
        }
        Node::FunctionExpression(_) => gen_function_expression(node, ctx),
        Node::ConditionalExpression(_) => gen_conditional_expression(node, ctx),
        Node::CacheExpression(_) => gen_cache_expression(node, ctx),
        Node::BlockStatement(body) => {
            let body = body.clone();
            gen_node_list(&body, ctx, true, false);
        }
        Node::TemplateLiteral(_) => gen_template_literal(node, ctx),
        Node::IfStatement(_) => gen_if_statement(node, ctx),
        Node::AssignmentExpression(l, r) => {
            let (l, r) = (*l, *r);
            gen_node(l, ctx);
            ctx.push(" = ");
            gen_node(r, ctx);
        }
        Node::SequenceExpression(e) => {
            let e = e.clone();
            ctx.push("(");
            gen_node_list(&e, ctx, false, true);
            ctx.push(")");
        }
        Node::ReturnStatement(r) => {
            let r = *r;
            ctx.push("return ");
            match ctx.a.node(r) {
                Node::Nodes(_) | Node::ChildrenRef(_) => {
                    let list = ctx.a.list(r).clone();
                    gen_node_list_as_array(&list, ctx);
                }
                _ => gen_node(r, ctx),
            }
        }
        Node::IfBranch(_) | Node::None => {}
        Node::Nodes(v) => {
            let v = v.clone();
            gen_node_list_as_array(&v, ctx);
        }
        Node::ChildrenRef(_) => {
            let list = ctx.a.list(node).clone();
            gen_node_list_as_array(&list, ctx);
        }
        other => panic!("unhandled codegen node type: {:?}", other.node_type()),
    }
}

fn gen_template_literal(node: NodeId, ctx: &mut CodegenContext) {
    let elements = match ctx.a.node(node) {
        Node::TemplateLiteral(e) => e.clone(),
        _ => unreachable!(),
    };
    ctx.push("`");
    let multilines = elements.len() > 3;
    for e in &elements {
        if let Node::Str(raw) = ctx.a.node(*e) {
            let escaped: String = raw
                .chars()
                .flat_map(|c| {
                    let esc = matches!(c, '`' | '$' | '\\');
                    esc.then_some('\\').into_iter().chain(std::iter::once(c))
                })
                .collect();
            ctx.push(&escaped);
        } else {
            ctx.push("${");
            if multilines {
                ctx.indent();
            }
            gen_node(*e, ctx);
            if multilines {
                ctx.deindent(false);
            }
            ctx.push("}");
        }
    }
    ctx.push("`");
}

fn gen_if_statement(node: NodeId, ctx: &mut CodegenContext) {
    let (test, consequent, alternate) = match ctx.a.node(node) {
        Node::IfStatement(i) => (i.test, i.consequent, i.alternate),
        _ => unreachable!(),
    };
    ctx.push("if (");
    gen_node(test, ctx);
    ctx.push(") {");
    ctx.indent();
    gen_node(consequent, ctx);
    ctx.deindent(false);
    ctx.push("}");
    if let Some(alt) = alternate {
        ctx.push(" else ");
        if matches!(ctx.a.node(alt), Node::IfStatement(_)) {
            gen_if_statement(alt, ctx);
        } else {
            ctx.push("{");
            ctx.indent();
            gen_node(alt, ctx);
            ctx.deindent(false);
            ctx.push("}");
        }
    }
}

const PATCH_FLAGS_ASC: &[(i32, &str)] = &[
    (1, "TEXT"),
    (2, "CLASS"),
    (4, "STYLE"),
    (8, "PROPS"),
    (16, "FULL_PROPS"),
    (32, "NEED_HYDRATION"),
    (64, "STABLE_FRAGMENT"),
    (128, "KEYED_FRAGMENT"),
    (256, "UNKEYED_FRAGMENT"),
    (512, "NEED_PATCH"),
    (1024, "DYNAMIC_SLOTS"),
    (2048, "DEV_ROOT_FRAGMENT"),
];

fn patch_flag_string(flag: i32) -> String {
    if flag < 0 {
        format!(
            "{flag} /* {} */",
            super::patch_flags::patch_flag_name(flag)
        )
    } else {
        let names = PATCH_FLAGS_ASC
            .iter()
            .filter(|(n, _)| flag & n != 0)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        format!("{flag} /* {names} */")
    }
}

fn gen_vnode_call(node: NodeId, ctx: &mut CodegenContext) {
    let v = ctx.a.vnode(node).clone();
    let patch_flag_str = v.patch_flag.filter(|f| *f != 0).map(patch_flag_string);

    if v.directives.is_some() {
        let h = ctx.helper(RuntimeHelper::WITH_DIRECTIVES);
        ctx.push(&format!("{h}("));
    }
    if v.is_block {
        let h = ctx.helper(RuntimeHelper::OPEN_BLOCK);
        ctx.push(&format!(
            "({h}({}), ",
            if v.disable_tracking { "true" } else { "" }
        ));
    }
    if ctx.pure {
        ctx.push(PURE_ANNOTATION);
    }
    let call_helper = if v.is_block {
        get_vnode_block_helper(ctx.opts.in_ssr, v.is_component)
    } else {
        get_vnode_helper(ctx.opts.in_ssr, v.is_component)
    };
    let h = ctx.helper(call_helper);
    ctx.push(&format!("{h}("));

    let patch_flag_node = patch_flag_str.map(|s| NodeOrStr::Str(s));
    let args: Vec<Option<NodeOrStr>> = vec![
        Some(NodeOrStr::Node(v.tag)),
        v.props.map(NodeOrStr::Node),
        v.children.map(NodeOrStr::Node),
        patch_flag_node,
        v.dynamic_props.map(NodeOrStr::Node),
    ];
    gen_nullable_args(args, ctx);
    ctx.push(")");
    if v.is_block {
        ctx.push(")");
    }
    if let Some(d) = v.directives {
        ctx.push(", ");
        gen_node(d, ctx);
        ctx.push(")");
    }
}

enum NodeOrStr {
    Node(NodeId),
    Str(String),
}

fn gen_nullable_args(args: Vec<Option<NodeOrStr>>, ctx: &mut CodegenContext) {
    let mut i = args.len();
    while i > 0 {
        if args[i - 1].is_some() {
            break;
        }
        i -= 1;
    }
    let items: Vec<Option<NodeOrStr>> = args.into_iter().take(i).collect();
    let len = items.len();
    for (idx, item) in items.into_iter().enumerate() {
        match item {
            Some(NodeOrStr::Node(n)) => {
                if ctx.a.is_list_like(n) {
                    let list = ctx.a.list(n).clone();
                    gen_node_list_as_array(&list, ctx);
                } else {
                    gen_node(n, ctx);
                }
            }
            Some(NodeOrStr::Str(s)) => ctx.push(&s),
            None => ctx.push("null"),
        }
        if idx < len - 1 {
            ctx.push(", ");
        }
    }
}

fn gen_call_expression(node: NodeId, ctx: &mut CodegenContext) {
    let c = ctx.a.call(node).clone();
    let callee = match ctx.a.node(c.callee) {
        Node::Str(s) => s.clone(),
        Node::Sym(h) => ctx.helper(*h),
        _ => String::new(),
    };
    if ctx.pure {
        ctx.push(PURE_ANNOTATION);
    }
    ctx.push(&format!("{callee}("));
    gen_node_list(&c.arguments, ctx, false, true);
    ctx.push(")");
}

fn gen_expression_as_property_key(node: NodeId, ctx: &mut CodegenContext) {
    if ctx.a.is(node, NodeType::CompoundExpression) {
        ctx.push("[");
        let children = ctx.a.compound(node).children.clone();
        for c in children {
            gen_node(c, ctx);
        }
        ctx.push("]");
    } else {
        let e = ctx.a.exp(node);
        if e.is_static {
            let text = if is_simple_identifier(&e.content) {
                e.content.clone()
            } else {
                serde_json::to_string(&e.content).unwrap()
            };
            ctx.push(&text);
        } else {
            let t = format!("[{}]", e.content);
            ctx.push(&t);
        }
    }
}

fn gen_object_expression(node: NodeId, ctx: &mut CodegenContext) {
    let properties = ctx.a.obj(node).properties.clone();
    if properties.is_empty() {
        ctx.push("{}");
        return;
    }
    let multilines = properties.len() > 1
        || properties
            .iter()
            .any(|p| !ctx.a.is(ctx.a.prop(*p).value, NodeType::SimpleExpression));
    ctx.push(if multilines { "{" } else { "{ " });
    if multilines {
        ctx.indent();
    }
    for i in 0..properties.len() {
        let (key, value) = {
            let p = ctx.a.prop(properties[i]);
            (p.key, p.value)
        };
        gen_expression_as_property_key(key, ctx);
        ctx.push(": ");
        gen_node(value, ctx);
        if i < properties.len() - 1 {
            ctx.push(",");
            ctx.newline();
        }
    }
    if multilines {
        ctx.deindent(false);
    }
    ctx.push(if multilines { "}" } else { " }" });
}

fn gen_function_expression(node: NodeId, ctx: &mut CodegenContext) {
    let f = ctx.a.func(node).clone();
    if f.is_slot {
        ctx.push(&format!("_{}(", RuntimeHelper::WITH_CTX.name()));
    }
    ctx.push("(");
    if let Some(params) = f.params {
        if ctx.a.is_list_like(params) {
            let list = ctx.a.list(params).clone();
            gen_node_list(&list, ctx, false, true);
        } else {
            gen_node(params, ctx);
        }
    }
    ctx.push(") => ");
    if f.newline || f.body.is_some() {
        ctx.push("{");
        ctx.indent();
    }
    if let Some(returns) = f.returns {
        if f.newline {
            ctx.push("return ");
        }
        if ctx.a.is_list_like(returns) {
            let list = ctx.a.list(returns).clone();
            gen_node_list_as_array(&list, ctx);
        } else {
            gen_node(returns, ctx);
        }
    } else if let Some(body) = f.body {
        gen_node(body, ctx);
    }
    if f.newline || f.body.is_some() {
        ctx.deindent(false);
        ctx.push("}");
    }
    if f.is_slot {
        ctx.push(")");
    }
}

fn gen_conditional_expression(node: NodeId, ctx: &mut CodegenContext) {
    let c = ctx.a.cond(node).clone();
    if ctx.a.is(c.test, NodeType::SimpleExpression) {
        let content = ctx.a.exp(c.test).content.clone();
        let needs_parens = !is_simple_identifier(&content);
        if needs_parens {
            ctx.push("(");
        }
        gen_node(c.test, ctx);
        if needs_parens {
            ctx.push(")");
        }
    } else {
        ctx.push("(");
        gen_node(c.test, ctx);
        ctx.push(")");
    }
    if c.newline {
        ctx.indent();
    }
    ctx.indent_level += 1;
    if !c.newline {
        ctx.push(" ");
    }
    ctx.push("? ");
    gen_node(c.consequent, ctx);
    ctx.indent_level -= 1;
    if c.newline {
        ctx.newline();
    } else {
        ctx.push(" ");
    }
    ctx.push(": ");
    let is_nested = ctx.a.is(c.alternate, NodeType::JsConditionalExpression);
    if !is_nested {
        ctx.indent_level += 1;
    }
    gen_node(c.alternate, ctx);
    if !is_nested {
        ctx.indent_level -= 1;
    }
    if c.newline {
        ctx.deindent(true);
    }
}

fn gen_cache_expression(node: NodeId, ctx: &mut CodegenContext) {
    let c = ctx.a.cache(node).clone();
    if c.need_array_spread {
        ctx.push("[...(");
    }
    ctx.push(&format!("_cache[{}] || (", c.index));
    if c.need_pause_tracking {
        ctx.indent();
        let h = ctx.helper(RuntimeHelper::SET_BLOCK_TRACKING);
        ctx.push(&format!("{h}(-1"));
        if c.in_v_once {
            ctx.push(", true");
        }
        ctx.push("),");
        ctx.newline();
        ctx.push("(");
    }
    ctx.push(&format!("_cache[{}] = ", c.index));
    gen_node(c.value, ctx);
    if c.need_pause_tracking {
        ctx.push(&format!(").cacheIndex = {},", c.index));
        ctx.newline();
        let h = ctx.helper(RuntimeHelper::SET_BLOCK_TRACKING);
        ctx.push(&format!("{h}(1),"));
        ctx.newline();
        ctx.push(&format!("_cache[{}]", c.index));
        ctx.deindent(false);
    }
    ctx.push(")");
    if c.need_array_spread {
        ctx.push(")]");
    }
}
