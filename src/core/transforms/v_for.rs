//! Port of `compiler-core/src/transforms/vFor.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::patch_flags as pf;
use crate::core::transform::{ExitFn, TransformContext, create_vnode_call};
use crate::core::utils::{find_dir, find_prop, inject_prop, is_slot_outlet, is_template_node};

use super::transform_expression::process_expression;

pub fn transform_for(
    node: NodeId,
    dir: NodeId,
    ctx: &mut TransformContext,
    codegen: bool,
) -> Vec<ExitFn> {
    if ctx.a.dir(dir).exp.is_none() {
        let loc = ctx.a.loc(dir).clone();
        ctx.error(ErrorCode::X_V_FOR_NO_EXPRESSION, Some(loc));
        return Vec::new();
    }
    if ctx.a.dir(dir).for_parse_result.is_none() {
        let loc = ctx.a.loc(dir).clone();
        ctx.error(ErrorCode::X_V_FOR_MALFORMED_EXPRESSION, Some(loc));
        return Vec::new();
    }

    finalize_for_parse_result_on_dir(dir, ctx);
    let parse_result = ctx.a.dir(dir).for_parse_result.clone().unwrap();
    let (source, value, key, index) = (
        parse_result.source,
        parse_result.value,
        parse_result.key,
        parse_result.index,
    );

    let children = if is_template_node(&ctx.a, node) {
        ctx.a.el(node).children.clone()
    } else {
        vec![node]
    };
    let loc = ctx.a.loc(dir).clone();
    let for_node = ctx.a.add(Node::For(Box::new(ForNode {
        source,
        value_alias: value,
        key_alias: key,
        object_index_alias: index,
        parse_result,
        children,
        codegen_node: None,
        loc,
    })));
    ctx.replace_node(for_node);

    ctx.scopes.v_for += 1;
    if ctx.opts.prefix_identifiers {
        if let Some(v) = value {
            ctx.add_identifiers(v);
        }
        if let Some(k) = key {
            ctx.add_identifiers(k);
        }
        if let Some(i) = index {
            ctx.add_identifiers(i);
        }
    }

    // `processFor` without a codegen callback still tears the scope down
    if !codegen {
        return vec![ExitFn::ForTeardown { value, key, index }];
    }

    // --- processCodegen (enter half) ---
    let render_list = ctx.helper_node(RuntimeHelper::RENDER_LIST);
    let render_exp = ctx.a.create_call_expression(render_list, vec![source]);
    let is_template = is_template_node(&ctx.a, node);
    let memo = find_dir(&ctx.a, node, "memo", false);
    let key_prop = find_prop(&ctx.a, node, "key", false, true);
    let is_dir_key = key_prop
        .map(|k| ctx.a.is(k, NodeType::Directive))
        .unwrap_or(false);
    let mut key_exp: Option<NodeId> = match key_prop {
        None => None,
        Some(k) => match ctx.a.node(k) {
            Node::Attribute(attr) => attr.value.as_ref().map(|v| v.content.clone()).map(|c| {
                // placeholder; the expression is built below (needs &mut arena)
                c
            }),
            Node::Directive(d) => {
                let _ = d;
                None
            }
            _ => None,
        }
        .map(|content| ctx.a.simple_exp(content, true)),
    };
    if is_dir_key {
        key_exp = ctx.a.dir(key_prop.unwrap()).exp;
    }
    let key_property = key_exp.map(|k| ctx.a.create_object_property_str("key", k));

    if ctx.opts.prefix_identifiers {
        if is_template && memo.is_some() {
            let memo = memo.unwrap();
            let exp = ctx.a.dir(memo).exp.unwrap();
            let processed = process_expression(exp, ctx, false, false, None);
            ctx.a.dir_mut(memo).exp = Some(processed);
        }
        if (is_template || memo.is_some()) && key_property.is_some() && is_dir_key {
            let kp = key_property.unwrap();
            let value = ctx.a.prop(kp).value;
            let processed = process_expression(value, ctx, false, false, None);
            ctx.a.prop_mut(kp).value = processed;
            ctx.a.dir_mut(key_prop.unwrap()).exp = Some(processed);
            key_exp = Some(processed);
            if memo.is_some() {
                ctx.v_for_memo_keyed_nodes.insert(node);
            }
        }
    }

    let is_stable_fragment = ctx.a.is(source, NodeType::SimpleExpression)
        && ctx.a.exp(source).const_type > ConstantType::NotConstant;
    let fragment_flag = if is_stable_fragment {
        pf::STABLE_FRAGMENT
    } else if key_prop.is_some() {
        pf::KEYED_FRAGMENT
    } else {
        pf::UNKEYED_FRAGMENT
    };

    let tag = ctx.helper_node(RuntimeHelper::FRAGMENT);
    let node_loc = ctx.a.loc(node).clone();
    let codegen = create_vnode_call(
        Some(ctx),
        tag,
        None,
        Some(render_exp),
        Some(fragment_flag),
        None,
        None,
        true,
        !is_stable_fragment,
        false,
        node_loc,
    );
    ctx.a.for_node_mut(for_node).codegen_node = Some(codegen);

    vec![ExitFn::For {
        for_node,
        node,
        render_exp,
        key_property,
        key_exp,
        memo,
        is_stable_fragment,
        is_template,
        value,
        key,
        index,
    }]
}

#[allow(clippy::too_many_arguments)]
pub fn exit_for(
    for_node: NodeId,
    node: NodeId,
    render_exp: NodeId,
    key_property: Option<NodeId>,
    key_exp: Option<NodeId>,
    memo: Option<NodeId>,
    is_stable_fragment: bool,
    is_template: bool,
    value: Option<NodeId>,
    key: Option<NodeId>,
    index: Option<NodeId>,
    ctx: &mut TransformContext,
) {
    // --- codegen exit ---
    let children = ctx.a.for_node(for_node).children.clone();

    if is_template {
        let node_children = ctx.a.el(node).children.clone();
        for c in node_children {
            if ctx.a.is(c, NodeType::Element) {
                if let Some(k) = find_prop(&ctx.a, c, "key", false, false) {
                    let loc = ctx.a.loc(k).clone();
                    ctx.error(ErrorCode::X_V_FOR_TEMPLATE_KEY_PLACEMENT, Some(loc));
                    break;
                }
            }
        }
    }

    let need_fragment_wrapper =
        children.len() != 1 || !ctx.a.is(children[0], NodeType::Element);
    let slot_outlet = if is_slot_outlet(&ctx.a, node) {
        Some(node)
    } else if is_template
        && ctx.a.el(node).children.len() == 1
        && is_slot_outlet(&ctx.a, ctx.a.el(node).children[0])
    {
        Some(ctx.a.el(node).children[0])
    } else {
        None
    };

    let child_block: NodeId;
    if let Some(slot_outlet) = slot_outlet {
        child_block = ctx.a.el(slot_outlet).codegen_node.unwrap();
        if is_template {
            if let Some(kp) = key_property {
                let merge = ctx.a.sym(RuntimeHelper::MERGE_PROPS);
                if inject_prop(&mut ctx.a, child_block, kp, merge) {
                    ctx.helper(RuntimeHelper::MERGE_PROPS);
                }
            }
        }
    } else if need_fragment_wrapper {
        let tag = ctx.helper_node(RuntimeHelper::FRAGMENT);
        let props = key_property.map(|kp| ctx.a.create_object_expression(vec![kp]));
        let children_ref = ctx.a.children_ref(node);
        child_block = create_vnode_call(
            Some(ctx),
            tag,
            props,
            Some(children_ref),
            Some(pf::STABLE_FRAGMENT),
            None,
            None,
            true,
            false,
            false,
            loc_stub(),
        );
    } else {
        child_block = ctx.a.el(children[0]).codegen_node.unwrap();
        if is_template {
            if let Some(kp) = key_property {
                let merge = ctx.a.sym(RuntimeHelper::MERGE_PROPS);
                if inject_prop(&mut ctx.a, child_block, kp, merge) {
                    ctx.helper(RuntimeHelper::MERGE_PROPS);
                }
            }
        }
        let is_block = ctx.a.vnode(child_block).is_block;
        let is_component = ctx.a.vnode(child_block).is_component;
        if is_block != !is_stable_fragment {
            if is_block {
                ctx.remove_helper(RuntimeHelper::OPEN_BLOCK);
                let h = get_vnode_block_helper(ctx.opts.in_ssr, is_component);
                ctx.remove_helper(h);
            } else {
                let h = get_vnode_helper(ctx.opts.in_ssr, is_component);
                ctx.remove_helper(h);
            }
        }
        ctx.a.vnode_mut(child_block).is_block = !is_stable_fragment;
        if !is_stable_fragment {
            ctx.helper(RuntimeHelper::OPEN_BLOCK);
            let h = get_vnode_block_helper(ctx.opts.in_ssr, is_component);
            ctx.helper(h);
        } else {
            let h = get_vnode_helper(ctx.opts.in_ssr, is_component);
            ctx.helper(h);
        }
    }

    if let Some(memo) = memo {
        let cached_param = ctx.a.simple_exp("_cached", false);
        let parse_result = ctx.a.for_node(for_node).parse_result.clone();
        let params = create_for_loop_params(&mut ctx.a, &parse_result, vec![cached_param]);
        let params_node = ctx.a.nodes(params);
        let loop_fn =
            ctx.a
                .create_function_expression(Some(params_node), None, false, false, loc_stub());

        let memo_exp = ctx.a.dir(memo).exp.unwrap();
        let s1 = ctx.a.string("const _memo = (");
        let s2 = ctx.a.string(")");
        let stmt1 = ctx
            .a
            .create_compound_expression(vec![s1, memo_exp, s2], loc_stub());

        let mut c2: Vec<NodeId> = Vec::new();
        let head = ctx.a.string("if (_cached && _cached.el");
        c2.push(head);
        if let Some(k) = key_exp {
            let s = ctx.a.string(" && _cached.key === ");
            c2.push(s);
            c2.push(k);
        }
        let is_memo_same = ctx.helper_string(RuntimeHelper::IS_MEMO_SAME);
        let tail = ctx
            .a
            .string(format!(" && {is_memo_same}(_cached, _memo)) return _cached"));
        c2.push(tail);
        let stmt2 = ctx.a.create_compound_expression(c2, loc_stub());

        let s3 = ctx.a.string("const _item = ");
        let stmt3 = ctx
            .a
            .create_compound_expression(vec![s3, child_block], loc_stub());
        let stmt4 = ctx.a.simple_exp("_item.memo = _memo", false);
        let stmt5 = ctx.a.simple_exp("return _item", false);
        let body = ctx
            .a
            .add(Node::BlockStatement(vec![stmt1, stmt2, stmt3, stmt4, stmt5]));
        ctx.a.func_mut(loop_fn).body = Some(body);

        let cache_str = ctx.a.simple_exp("_cache", false);
        let idx = ctx.a.simple_exp(ctx.cached.len().to_string(), false);
        ctx.a
            .call_mut(render_exp)
            .arguments
            .extend([loop_fn, cache_str, idx]);
        ctx.cached.push(None);
    } else {
        let parse_result = ctx.a.for_node(for_node).parse_result.clone();
        let params = create_for_loop_params(&mut ctx.a, &parse_result, Vec::new());
        let params_node = ctx.a.nodes(params);
        let f = ctx.a.create_function_expression(
            Some(params_node),
            Some(child_block),
            true,
            false,
            loc_stub(),
        );
        ctx.a.call_mut(render_exp).arguments.push(f);
    }

    // --- processFor exit (scope cleanup) ---
    ctx.scopes.v_for -= 1;
    if ctx.opts.prefix_identifiers {
        if let Some(v) = value {
            ctx.remove_identifiers(v);
        }
        if let Some(k) = key {
            ctx.remove_identifiers(k);
        }
        if let Some(i) = index {
            ctx.remove_identifiers(i);
        }
    }
}

pub fn finalize_for_parse_result_on_dir(dir: NodeId, ctx: &mut TransformContext) {
    let mut result = match ctx.a.dir(dir).for_parse_result.clone() {
        Some(r) => r,
        None => return,
    };
    if result.finalized {
        return;
    }
    if ctx.opts.prefix_identifiers {
        result.source = process_expression(result.source, ctx, false, false, None);
        if let Some(k) = result.key {
            result.key = Some(process_expression(k, ctx, true, false, None));
        }
        if let Some(i) = result.index {
            result.index = Some(process_expression(i, ctx, true, false, None));
        }
        if let Some(v) = result.value {
            result.value = Some(process_expression(v, ctx, true, false, None));
        }
    }
    result.finalized = true;
    ctx.a.dir_mut(dir).for_parse_result = Some(result);
}

pub fn create_for_loop_params(
    a: &mut Arena,
    parse_result: &ForParseResult,
    memo_args: Vec<NodeId>,
) -> Vec<NodeId> {
    let mut args: Vec<Option<NodeId>> = vec![parse_result.value, parse_result.key, parse_result.index];
    for m in memo_args {
        args.push(Some(m));
    }
    let mut i = args.len();
    while i > 0 {
        if args[i - 1].is_some() {
            break;
        }
        i -= 1;
    }
    args.truncate(i);
    args.into_iter()
        .enumerate()
        .map(|(i, arg)| match arg {
            Some(a) => a,
            None => a.simple_exp("_".repeat(i + 1), false),
        })
        .collect()
}
