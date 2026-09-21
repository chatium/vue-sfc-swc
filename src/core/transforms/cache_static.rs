//! Port of `compiler-core/src/transforms/cacheStatic.ts`.

use crate::core::ast::*;
use crate::core::patch_flags as pf;
use crate::core::transform::TransformContext;
use crate::core::utils::find_dir;

pub fn cache_static(root: NodeId, ctx: &mut TransformContext) {
    let do_not_hoist = get_single_element_root(&ctx.a, root).is_some();
    walk(root, None, ctx, do_not_hoist, false);
}

pub fn get_single_element_root(a: &Arena, root: NodeId) -> Option<NodeId> {
    let children: Vec<NodeId> = a
        .root(root)
        .children
        .iter()
        .copied()
        .filter(|c| !a.is(*c, NodeType::Comment))
        .collect();
    if children.len() == 1
        && a.is(children[0], NodeType::Element)
        && a.el(children[0]).tag_type != ElementType::Slot
    {
        Some(children[0])
    } else {
        None
    }
}

fn get_node_props(a: &Arena, node: NodeId) -> Option<NodeId> {
    let codegen = a.el(node).codegen_node?;
    if a.is(codegen, NodeType::VNodeCall) {
        a.vnode(codegen).props
    } else {
        None
    }
}

fn walk(
    node: NodeId,
    parent: Option<NodeId>,
    ctx: &mut TransformContext,
    do_not_hoist_node: bool,
    in_for: bool,
) {
    let children = ctx.a.children_of(node).clone();
    let mut to_cache: Vec<NodeId> = Vec::new();

    for child in &children {
        let child = *child;
        let ty = ctx.a.node_type(child);
        if ty == NodeType::Element && ctx.a.el(child).tag_type == ElementType::Element {
            let constant_type = if do_not_hoist_node {
                ConstantType::NotConstant
            } else {
                get_constant_type(child, ctx)
            };
            if constant_type > ConstantType::NotConstant {
                if constant_type >= ConstantType::CanCache {
                    let codegen = ctx.a.el(child).codegen_node.unwrap();
                    ctx.a.vnode_mut(codegen).patch_flag = Some(pf::CACHED);
                    to_cache.push(child);
                    continue;
                }
            } else {
                let codegen_node = ctx.a.el(child).codegen_node;
                if let Some(codegen_node) = codegen_node {
                    if ctx.a.is(codegen_node, NodeType::VNodeCall) {
                        let flag = ctx.a.vnode(codegen_node).patch_flag;
                        if (flag.is_none()
                            || flag == Some(pf::NEED_PATCH)
                            || flag == Some(pf::TEXT))
                            && get_generated_props_constant_type(child, ctx)
                                >= ConstantType::CanCache
                        {
                            if let Some(props) = get_node_props(&ctx.a, child) {
                                let hoisted = ctx.hoist(props);
                                ctx.a.vnode_mut(codegen_node).props = Some(hoisted);
                            }
                        }
                        let dynamic_props = ctx.a.vnode(codegen_node).dynamic_props;
                        if let Some(dp) = dynamic_props {
                            let hoisted = ctx.hoist(dp);
                            ctx.a.vnode_mut(codegen_node).dynamic_props = Some(hoisted);
                        }
                    }
                }
            }
        } else if ty == NodeType::TextCall {
            let constant_type = if do_not_hoist_node {
                ConstantType::NotConstant
            } else {
                get_constant_type(child, ctx)
            };
            if constant_type >= ConstantType::CanCache {
                let codegen = ctx.a.text_call(child).codegen_node;
                if let Some(codegen) = codegen {
                    if ctx.a.is(codegen, NodeType::JsCallExpression)
                        && !ctx.a.call(codegen).arguments.is_empty()
                    {
                        let text = format!(
                            "{}{}",
                            pf::CACHED,
                            format_args!(" /* {} */", pf::patch_flag_name(pf::CACHED))
                        );
                        let n = ctx.a.string(text);
                        ctx.a.call_mut(codegen).arguments.push(n);
                    }
                }
                to_cache.push(child);
                continue;
            }
        }

        // walk further
        let ty = ctx.a.node_type(child);
        if ty == NodeType::Element {
            let is_component = ctx.a.el(child).tag_type == ElementType::Component;
            if is_component {
                ctx.scopes.v_slot += 1;
            }
            walk(child, Some(node), ctx, false, in_for);
            if is_component {
                ctx.scopes.v_slot -= 1;
            }
        } else if ty == NodeType::For {
            let single = ctx.a.for_node(child).children.len() == 1;
            walk(child, Some(node), ctx, single, true);
        } else if ty == NodeType::If {
            let branches = ctx.a.if_node(child).branches.clone();
            for b in branches {
                let single = ctx.a.branch(b).children.len() == 1;
                walk(b, Some(node), ctx, single, in_for);
            }
        }
    }

    let mut cached_as_array = false;
    if to_cache.len() == children.len() && ctx.a.is(node, NodeType::Element) {
        let tag_type = ctx.a.el(node).tag_type;
        let codegen = ctx.a.el(node).codegen_node;
        if tag_type == ElementType::Element
            && codegen
                .map(|c| ctx.a.is(c, NodeType::VNodeCall))
                .unwrap_or(false)
            && ctx
                .a
                .vnode(codegen.unwrap())
                .children
                .map(|c| ctx.a.is_list_like(c))
                .unwrap_or(false)
        {
            let codegen = codegen.unwrap();
            let list = ctx.a.vnode(codegen).children.unwrap();
            let arr = ctx.a.create_array_expression_ref(list);
            let cached = get_cache_expression(arr, ctx);
            ctx.a.vnode_mut(codegen).children = Some(cached);
            cached_as_array = true;
        } else if tag_type == ElementType::Component
            && codegen
                .map(|c| ctx.a.is(c, NodeType::VNodeCall))
                .unwrap_or(false)
        {
            let codegen = codegen.unwrap();
            let ch = ctx.a.vnode(codegen).children;
            if let Some(ch) = ch {
                if !ctx.a.is_list_like(ch) && ctx.a.is(ch, NodeType::JsObjectExpression) {
                    if let Some(slot) = get_slot_node(&ctx.a, codegen, SlotKey::Name("default")) {
                        let returns = ctx.a.func(slot).returns;
                        if let Some(returns) = returns {
                            let arr = ctx.a.create_array_expression_ref(returns);
                            let cached = get_cache_expression(arr, ctx);
                            ctx.a.func_mut(slot).returns = Some(cached);
                            cached_as_array = true;
                        }
                    }
                }
            }
        } else if tag_type == ElementType::Template {
            if let Some(parent) = parent {
                if ctx.a.is(parent, NodeType::Element)
                    && ctx.a.el(parent).tag_type == ElementType::Component
                {
                    if let Some(pcodegen) = ctx.a.el(parent).codegen_node {
                        if ctx.a.is(pcodegen, NodeType::VNodeCall) {
                            let ch = ctx.a.vnode(pcodegen).children;
                            if let Some(ch) = ch {
                                if !ctx.a.is_list_like(ch)
                                    && ctx.a.is(ch, NodeType::JsObjectExpression)
                                {
                                    let slot_name = find_dir(&ctx.a, node, "slot", true)
                                        .and_then(|d| ctx.a.dir(d).arg);
                                    if let Some(arg) = slot_name {
                                        if let Some(slot) =
                                            get_slot_node(&ctx.a, pcodegen, SlotKey::Node(arg))
                                        {
                                            let returns = ctx.a.func(slot).returns;
                                            if let Some(returns) = returns {
                                                let arr =
                                                    ctx.a.create_array_expression_ref(returns);
                                                let cached = get_cache_expression(arr, ctx);
                                                ctx.a.func_mut(slot).returns = Some(cached);
                                                cached_as_array = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !cached_as_array {
        for child in &to_cache {
            let codegen = match ctx.a.node_type(*child) {
                NodeType::Element => ctx.a.el(*child).codegen_node,
                NodeType::TextCall => ctx.a.text_call(*child).codegen_node,
                _ => None,
            };
            if let Some(codegen) = codegen {
                let cached = ctx.cache(codegen, false, false);
                match ctx.a.node_type(*child) {
                    NodeType::Element => ctx.a.el_mut(*child).codegen_node = Some(cached),
                    NodeType::TextCall => ctx.a.text_call_mut(*child).codegen_node = Some(cached),
                    _ => {}
                }
            }
        }
    }

    if !to_cache.is_empty() && ctx.opts.transform_hoist {
        crate::dom::transforms::stringify_static::stringify_static(node, ctx);
    }
}

fn get_cache_expression(value: NodeId, ctx: &mut TransformContext) -> NodeId {
    let exp = ctx.cache(value, false, false);
    ctx.a.cache_mut(exp).need_array_spread = true;
    exp
}

enum SlotKey<'a> {
    Name(&'a str),
    Node(NodeId),
}

fn get_slot_node(a: &Arena, node: NodeId, name: SlotKey) -> Option<NodeId> {
    let children = a.vnode(node).children?;
    if a.is_list_like(children) || !a.is(children, NodeType::JsObjectExpression) {
        return None;
    }
    for p in &a.obj(children).properties {
        let key = a.prop(*p).key;
        let matched = match name {
            SlotKey::Name(n) => {
                matches!(a.node(key), Node::SimpleExpression(e) if e.content == n)
            }
            SlotKey::Node(id) => {
                key == id
                    || match (a.node(key), a.node(id)) {
                        (Node::SimpleExpression(a1), Node::SimpleExpression(b1)) => {
                            a1.content == b1.content
                        }
                        _ => false,
                    }
            }
        };
        if matched {
            return Some(a.prop(*p).value);
        }
    }
    None
}

pub fn get_constant_type(node: NodeId, ctx: &mut TransformContext) -> ConstantType {
    match ctx.a.node_type(node) {
        NodeType::Element => {
            if ctx.a.el(node).tag_type != ElementType::Element {
                return ConstantType::NotConstant;
            }
            if let Some(cached) = ctx.constant_cache.get(&node) {
                return *cached;
            }
            let codegen_node = match ctx.a.el(node).codegen_node {
                Some(c) => c,
                None => return ConstantType::NotConstant,
            };
            if !ctx.a.is(codegen_node, NodeType::VNodeCall) {
                return ConstantType::NotConstant;
            }
            let tag = ctx.a.el(node).tag.clone();
            if ctx.a.vnode(codegen_node).is_block
                && tag != "svg"
                && tag != "foreignObject"
                && tag != "math"
            {
                return ConstantType::NotConstant;
            }
            if ctx.a.vnode(codegen_node).patch_flag.is_none() {
                let mut return_type = ConstantType::CanStringify;
                let generated_props_type = get_generated_props_constant_type(node, ctx);
                if generated_props_type == ConstantType::NotConstant {
                    ctx.constant_cache.insert(node, ConstantType::NotConstant);
                    return ConstantType::NotConstant;
                }
                if generated_props_type < return_type {
                    return_type = generated_props_type;
                }

                let children = ctx.a.el(node).children.clone();
                for child in children {
                    let child_type = get_constant_type(child, ctx);
                    if child_type == ConstantType::NotConstant {
                        ctx.constant_cache.insert(node, ConstantType::NotConstant);
                        return ConstantType::NotConstant;
                    }
                    if child_type < return_type {
                        return_type = child_type;
                    }
                }

                if return_type > ConstantType::CanSkipPatch {
                    let props = ctx.a.el(node).props.clone();
                    for p in props {
                        let exp = match ctx.a.node(p) {
                            Node::Directive(d) if d.name == "bind" && d.exp.is_some() => d.exp,
                            _ => None,
                        };
                        if let Some(exp) = exp {
                            let exp_type = get_constant_type(exp, ctx);
                            if exp_type == ConstantType::NotConstant {
                                ctx.constant_cache.insert(node, ConstantType::NotConstant);
                                return ConstantType::NotConstant;
                            }
                            if exp_type < return_type {
                                return_type = exp_type;
                            }
                        }
                    }
                }

                if ctx.a.vnode(codegen_node).is_block {
                    let props = ctx.a.el(node).props.clone();
                    for p in props {
                        if ctx.a.is(p, NodeType::Directive) {
                            ctx.constant_cache.insert(node, ConstantType::NotConstant);
                            return ConstantType::NotConstant;
                        }
                    }
                    ctx.remove_helper(RuntimeHelper::OPEN_BLOCK);
                    let is_component = ctx.a.vnode(codegen_node).is_component;
                    let h = get_vnode_block_helper(ctx.opts.in_ssr, is_component);
                    ctx.remove_helper(h);
                    ctx.a.vnode_mut(codegen_node).is_block = false;
                    let h = get_vnode_helper(ctx.opts.in_ssr, is_component);
                    ctx.helper(h);
                }

                ctx.constant_cache.insert(node, return_type);
                return_type
            } else {
                ctx.constant_cache.insert(node, ConstantType::NotConstant);
                ConstantType::NotConstant
            }
        }
        NodeType::Text | NodeType::Comment => ConstantType::CanStringify,
        NodeType::If | NodeType::For | NodeType::IfBranch => ConstantType::NotConstant,
        NodeType::Interpolation => {
            let c = ctx.a.interp(node).content;
            get_constant_type(c, ctx)
        }
        NodeType::TextCall => {
            let c = ctx.a.text_call(node).content;
            get_constant_type(c, ctx)
        }
        NodeType::SimpleExpression => ctx.a.exp(node).const_type,
        NodeType::CompoundExpression => {
            let mut return_type = ConstantType::CanStringify;
            let children = ctx.a.compound(node).children.clone();
            for child in children {
                if matches!(ctx.a.node(child), Node::Str(_) | Node::Sym(_)) {
                    continue;
                }
                let child_type = get_constant_type(child, ctx);
                if child_type == ConstantType::NotConstant {
                    return ConstantType::NotConstant;
                } else if child_type < return_type {
                    return_type = child_type;
                }
            }
            return_type
        }
        NodeType::JsCacheExpression => ConstantType::CanCache,
        _ => ConstantType::NotConstant,
    }
}

fn get_constant_type_of_helper_call(value: NodeId, ctx: &mut TransformContext) -> ConstantType {
    if ctx.a.is(value, NodeType::JsCallExpression) {
        let callee = ctx.a.call(value).callee;
        let allowed = matches!(
            ctx.a.sym_of(callee),
            Some(RuntimeHelper::NORMALIZE_CLASS)
                | Some(RuntimeHelper::NORMALIZE_STYLE)
                | Some(RuntimeHelper::NORMALIZE_PROPS)
                | Some(RuntimeHelper::GUARD_REACTIVE_PROPS)
        );
        if allowed {
            let arg = ctx.a.call(value).arguments[0];
            if ctx.a.is(arg, NodeType::SimpleExpression) {
                return get_constant_type(arg, ctx);
            } else if ctx.a.is(arg, NodeType::JsCallExpression) {
                return get_constant_type_of_helper_call(arg, ctx);
            }
        }
    }
    ConstantType::NotConstant
}

fn get_generated_props_constant_type(node: NodeId, ctx: &mut TransformContext) -> ConstantType {
    let mut return_type = ConstantType::CanStringify;
    let props = get_node_props(&ctx.a, node);
    if let Some(props) = props {
        if ctx.a.is(props, NodeType::JsObjectExpression) {
            let properties = ctx.a.obj(props).properties.clone();
            for p in properties {
                let (key, value) = {
                    let pr = ctx.a.prop(p);
                    (pr.key, pr.value)
                };
                let key_type = get_constant_type(key, ctx);
                if key_type == ConstantType::NotConstant {
                    return key_type;
                }
                if key_type < return_type {
                    return_type = key_type;
                }
                let value_type = if ctx.a.is(value, NodeType::SimpleExpression) {
                    get_constant_type(value, ctx)
                } else if ctx.a.is(value, NodeType::JsCallExpression) {
                    get_constant_type_of_helper_call(value, ctx)
                } else {
                    ConstantType::NotConstant
                };
                if value_type == ConstantType::NotConstant {
                    return value_type;
                }
                if value_type < return_type {
                    return_type = value_type;
                }
            }
        }
    }
    return_type
}
