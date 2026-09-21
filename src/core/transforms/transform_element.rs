//! Port of `compiler-core/src/transforms/transformElement.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::options::BindingType;
use crate::core::patch_flags as pf;
use crate::core::transform::{ExitFn, TransformContext, camelize, capitalize, create_vnode_call};
use crate::core::utils::{
    find_prop, is_built_in_directive, is_core_component, is_on, is_reserved_prop, is_static_arg_of,
    is_static_exp, to_valid_asset_id,
};

use super::cache_static::get_constant_type;
use super::transform_expression::process_expression;
use super::v_slot::build_slots;

pub fn transform_element(_node: NodeId, _ctx: &mut TransformContext) -> Vec<ExitFn> {
    vec![ExitFn::Element { node: _node }]
}

pub fn exit_element(_node: NodeId, ctx: &mut TransformContext) {
    let node = match ctx.current_node {
        Some(n) => n,
        None => return,
    };
    if !ctx.a.is(node, NodeType::Element) {
        return;
    }
    let tag_type = ctx.a.el(node).tag_type;
    if tag_type != ElementType::Element && tag_type != ElementType::Component {
        return;
    }

    let tag = ctx.a.el(node).tag.clone();
    let is_component = tag_type == ElementType::Component;

    let vnode_tag: NodeId = if is_component {
        resolve_component_type(node, ctx, false)
    } else {
        ctx.a.string(format!("\"{tag}\""))
    };

    let is_dynamic_component = ctx.a.is(vnode_tag, NodeType::JsCallExpression)
        && ctx.a.sym_of(ctx.a.call(vnode_tag).callee)
            == Some(RuntimeHelper::RESOLVE_DYNAMIC_COMPONENT);

    let mut vnode_props: Option<NodeId> = None;
    let mut vnode_children: Option<NodeId> = None;
    let mut patch_flag: i32 = 0;
    let mut vnode_dynamic_props: Option<NodeId> = None;
    let mut dynamic_prop_names: Vec<String> = Vec::new();
    let mut vnode_directives: Option<NodeId> = None;

    let vnode_tag_sym = ctx.a.sym_of(vnode_tag);
    let mut should_use_block = is_dynamic_component
        || vnode_tag_sym == Some(RuntimeHelper::TELEPORT)
        || vnode_tag_sym == Some(RuntimeHelper::SUSPENSE)
        || (!is_component && (tag == "svg" || tag == "foreignObject" || tag == "math"));

    if !ctx.a.el(node).props.is_empty() {
        let result = build_props(node, ctx, None, is_component, is_dynamic_component, false);
        vnode_props = result.props;
        patch_flag = result.patch_flag;
        dynamic_prop_names = result.dynamic_prop_names;
        if !result.directives.is_empty() {
            let args: Vec<NodeId> = result
                .directives
                .iter()
                .map(|d| build_directive_args(*d, ctx))
                .collect();
            vnode_directives = Some(ctx.a.create_array_expression(args));
        }
        if result.should_use_block {
            should_use_block = true;
        }
    }

    if !ctx.a.el(node).children.is_empty() {
        if vnode_tag_sym == Some(RuntimeHelper::KEEP_ALIVE) {
            should_use_block = true;
            patch_flag |= pf::DYNAMIC_SLOTS;
            let children = ctx.a.el(node).children.clone();
            if children.len() > 1 {
                let loc = SourceLocation {
                    start: ctx.a.loc(children[0]).start,
                    end: ctx.a.loc(*children.last().unwrap()).end,
                    source: String::new(),
                };
                ctx.error(ErrorCode::X_KEEP_ALIVE_INVALID_CHILDREN, Some(loc));
            }
        }

        let should_build_as_slots = is_component
            && vnode_tag_sym != Some(RuntimeHelper::TELEPORT)
            && vnode_tag_sym != Some(RuntimeHelper::KEEP_ALIVE);

        if should_build_as_slots {
            let r = build_slots(node, ctx, crate::core::transforms::v_slot::SlotFnKind::Client);
            vnode_children = Some(r.slots);
            if r.has_dynamic_slots {
                patch_flag |= pf::DYNAMIC_SLOTS;
            }
        } else if ctx.a.el(node).children.len() == 1
            && vnode_tag_sym != Some(RuntimeHelper::TELEPORT)
        {
            let child = ctx.a.el(node).children[0];
            let ty = ctx.a.node_type(child);
            let has_dynamic_text_child =
                ty == NodeType::Interpolation || ty == NodeType::CompoundExpression;
            if has_dynamic_text_child
                && get_constant_type(child, ctx) == ConstantType::NotConstant
            {
                patch_flag |= pf::TEXT;
            }
            if has_dynamic_text_child || ty == NodeType::Text {
                vnode_children = Some(child);
            } else {
                vnode_children = Some(ctx.a.children_ref(node));
            }
        } else {
            vnode_children = Some(ctx.a.children_ref(node));
        }
    }

    if !dynamic_prop_names.is_empty() {
        let s = stringify_dynamic_prop_names(&dynamic_prop_names);
        vnode_dynamic_props = Some(ctx.a.string(s));
    }

    let loc = ctx.a.loc(node).clone();
    let codegen = create_vnode_call(
        Some(ctx),
        vnode_tag,
        vnode_props,
        vnode_children,
        if patch_flag == 0 { None } else { Some(patch_flag) },
        vnode_dynamic_props,
        vnode_directives,
        should_use_block,
        false,
        is_component,
        loc,
    );
    ctx.a.el_mut(node).codegen_node = Some(codegen);
}

pub fn resolve_component_type(
    node: NodeId,
    ctx: &mut TransformContext,
    ssr: bool,
) -> NodeId {
    let mut tag = ctx.a.el(node).tag.clone();

    // 1. dynamic component
    let is_explicit_dynamic = is_component_tag(&tag);
    let is_prop = find_prop(&ctx.a, node, "is", false, true);
    if let Some(is_prop) = is_prop {
        if is_explicit_dynamic {
            let exp: Option<NodeId> = match ctx.a.node(is_prop) {
                Node::Attribute(a) => a.value.as_ref().map(|v| v.content.clone()).map(|c| {
                    let _ = &c;
                    c
                }),
                _ => None,
            }
            .map(|c| ctx.a.simple_exp(c, true))
            .or_else(|| {
                if ctx.a.is(is_prop, NodeType::Directive) {
                    let exp = ctx.a.dir(is_prop).exp;
                    match exp {
                        Some(e) => Some(e),
                        None => {
                            let arg_loc = ctx.a.loc(ctx.a.dir(is_prop).arg.unwrap()).clone();
                            let e = ctx.a.create_simple_expression(
                                "is",
                                false,
                                arg_loc,
                                ConstantType::NotConstant,
                            );
                            let processed = process_expression(e, ctx, false, false, None);
                            ctx.a.dir_mut(is_prop).exp = Some(processed);
                            Some(processed)
                        }
                    }
                } else {
                    None
                }
            });
            if let Some(exp) = exp {
                let helper = ctx.helper_node(RuntimeHelper::RESOLVE_DYNAMIC_COMPONENT);
                return ctx.a.create_call_expression(helper, vec![exp]);
            }
        } else if ctx.a.is(is_prop, NodeType::Attribute) {
            let v = ctx
                .a
                .attr(is_prop)
                .value
                .as_ref()
                .map(|v| v.content.clone())
                .unwrap_or_default();
            if v.starts_with("vue:") {
                tag = v[4..].to_string();
            }
        }
    }

    // 2. built-in components
    let built_in = is_core_component(&tag).or_else(|| {
        ctx.opts
            .is_built_in_component
            .and_then(|f| f(&tag))
    });
    if let Some(built_in) = built_in {
        if !ssr {
            ctx.helper(built_in);
        }
        return ctx.a.sym(built_in);
    }

    // 3. user component from setup bindings
    if let Some(from_setup) = resolve_setup_reference(&tag, ctx) {
        return ctx.a.string(from_setup);
    }
    if let Some(dot_index) = tag.find('.') {
        if dot_index > 0 {
            if let Some(ns) = resolve_setup_reference(&tag[..dot_index], ctx) {
                let s = format!("{ns}{}", &tag[dot_index..]);
                return ctx.a.string(s);
            }
        }
    }

    // 4. self reference
    if let Some(self_name) = ctx.self_name.clone() {
        if capitalize(&camelize(&tag)) == self_name {
            ctx.helper(RuntimeHelper::RESOLVE_COMPONENT);
            ctx.add_component(format!("{tag}__self"));
            let id = to_valid_asset_id(&tag, "component");
            return ctx.a.string(id);
        }
    }

    // 5. user component (resolve)
    ctx.helper(RuntimeHelper::RESOLVE_COMPONENT);
    ctx.add_component(tag.clone());
    let id = to_valid_asset_id(&tag, "component");
    ctx.a.string(id)
}

fn resolve_setup_reference(name: &str, ctx: &mut TransformContext) -> Option<String> {
    let bindings = &ctx.opts.binding_metadata;
    if bindings.is_script_setup == Some(false) {
        return None;
    }
    let camel_name = camelize(name);
    let pascal_name = capitalize(&camel_name);
    let check_type = |ty: BindingType, ctx: &TransformContext| -> Option<String> {
        let b = &ctx.opts.binding_metadata;
        if b.get(name) == Some(ty) {
            return Some(name.to_string());
        }
        if b.get(&camel_name) == Some(ty) {
            return Some(camel_name.clone());
        }
        if b.get(&pascal_name) == Some(ty) {
            return Some(pascal_name.clone());
        }
        None
    };

    let from_const = check_type(BindingType::SetupConst, ctx)
        .or_else(|| check_type(BindingType::SetupReactiveConst, ctx))
        .or_else(|| check_type(BindingType::LiteralConst, ctx));
    if let Some(c) = from_const {
        return Some(if ctx.opts.inline {
            c
        } else {
            format!("$setup[{}]", serde_json::to_string(&c).unwrap())
        });
    }

    let from_maybe_ref = check_type(BindingType::SetupLet, ctx)
        .or_else(|| check_type(BindingType::SetupRef, ctx))
        .or_else(|| check_type(BindingType::SetupMaybeRef, ctx));
    if let Some(r) = from_maybe_ref {
        return Some(if ctx.opts.inline {
            let unref = ctx.helper_string(RuntimeHelper::UNREF);
            format!("{unref}({r})")
        } else {
            format!("$setup[{}]", serde_json::to_string(&r).unwrap())
        });
    }

    let from_props = check_type(BindingType::Props, ctx);
    if let Some(p) = from_props {
        let unref = ctx.helper_string(RuntimeHelper::UNREF);
        let obj = if ctx.opts.inline { "__props" } else { "$props" };
        return Some(format!(
            "{unref}({obj}[{}])",
            serde_json::to_string(&p).unwrap()
        ));
    }
    None
}

pub struct BuildPropsResult {
    pub props: Option<NodeId>,
    pub directives: Vec<NodeId>,
    pub patch_flag: i32,
    pub dynamic_prop_names: Vec<String>,
    pub should_use_block: bool,
}

pub fn build_props(
    node: NodeId,
    ctx: &mut TransformContext,
    props_override: Option<Vec<NodeId>>,
    is_component: bool,
    is_dynamic_component: bool,
    ssr: bool,
) -> BuildPropsResult {
    let tag = ctx.a.el(node).tag.clone();
    let element_loc = ctx.a.loc(node).clone();
    let has_children = !ctx.a.el(node).children.is_empty();
    let props = props_override.unwrap_or_else(|| ctx.a.el(node).props.clone());

    let mut properties: Vec<NodeId> = Vec::new();
    let mut merge_args: Vec<NodeId> = Vec::new();
    let mut runtime_directives: Vec<NodeId> = Vec::new();
    let mut should_use_block = false;

    let mut patch_flag = 0i32;
    let mut has_ref = false;
    let mut has_class_binding = false;
    let mut has_style_binding = false;
    let mut has_hydration_event_binding = false;
    let mut has_dynamic_keys = false;
    let mut has_vnode_hook = false;
    let mut dynamic_prop_names: Vec<String> = Vec::new();

    macro_rules! push_merge_arg {
        ($ctx:expr, $arg:expr) => {{
            if !properties.is_empty() {
                let deduped = dedupe_properties($ctx, std::mem::take(&mut properties));
                let obj = $ctx.a.create_object_expression(deduped);
                $ctx.a.obj_mut(obj).loc = element_loc.clone();
                merge_args.push(obj);
            }
            if let Some(a) = $arg {
                merge_args.push(a);
            }
        }};
    }

    for i in 0..props.len() {
        let prop = props[i];
        if ctx.a.is(prop, NodeType::Attribute) {
            let (loc, name, name_loc, value) = {
                let a = ctx.a.attr(prop);
                (a.loc.clone(), a.name.clone(), a.name_loc.clone(), a.value.clone())
            };
            let mut is_static = true;
            if name == "ref" {
                has_ref = true;
                if ctx.scopes.v_for > 0 {
                    let k = ctx.a.simple_exp("ref_for", true);
                    let v = ctx.a.simple_exp("true", false);
                    let p = ctx.a.create_object_property(k, v);
                    properties.push(p);
                }
                if let Some(v) = &value {
                    if ctx.opts.inline {
                        let binding = ctx.opts.binding_metadata.get(&v.content);
                        if matches!(
                            binding,
                            Some(BindingType::SetupLet)
                                | Some(BindingType::SetupRef)
                                | Some(BindingType::SetupMaybeRef)
                        ) {
                            is_static = false;
                            let k = ctx.a.simple_exp("ref_key", true);
                            let val = ctx.a.create_simple_expression(
                                v.content.clone(),
                                true,
                                v.loc.clone(),
                                ConstantType::NotConstant,
                            );
                            let p = ctx.a.create_object_property(k, val);
                            properties.push(p);
                        }
                    }
                }
            }
            if name == "is"
                && (is_component_tag(&tag)
                    || value
                        .as_ref()
                        .map(|v| v.content.starts_with("vue:"))
                        .unwrap_or(false))
            {
                continue;
            }
            let key = ctx.a.create_simple_expression(
                name,
                true,
                name_loc,
                ConstantType::NotConstant,
            );
            let val_content = value.as_ref().map(|v| v.content.clone()).unwrap_or_default();
            let val_loc = value.as_ref().map(|v| v.loc.clone()).unwrap_or(loc);
            let val = ctx.a.create_simple_expression(
                val_content,
                is_static,
                val_loc,
                ConstantType::NotConstant,
            );
            let p = ctx.a.create_object_property(key, val);
            properties.push(p);
        } else {
            let (name, arg, exp, loc, modifiers) = {
                let d = ctx.a.dir(prop);
                (
                    d.name.clone(),
                    d.arg,
                    d.exp,
                    d.loc.clone(),
                    d.modifiers.clone(),
                )
            };
            let is_v_bind = name == "bind";
            let is_v_on = name == "on";

            if name == "slot" {
                if !is_component {
                    ctx.error(ErrorCode::X_V_SLOT_MISPLACED, Some(loc.clone()));
                }
                continue;
            }
            if name == "once" || name == "memo" {
                continue;
            }
            if name == "is"
                || (is_v_bind && is_static_arg_of(&ctx.a, &arg, "is") && is_component_tag(&tag))
            {
                continue;
            }
            if is_v_on && ssr {
                continue;
            }

            if (is_v_bind && is_static_arg_of(&ctx.a, &arg, "key"))
                || (is_v_on
                    && has_children
                    && is_static_arg_of(&ctx.a, &arg, "vue:before-update"))
            {
                should_use_block = true;
            }

            if is_v_bind && is_static_arg_of(&ctx.a, &arg, "ref") && ctx.scopes.v_for > 0 {
                let k = ctx.a.simple_exp("ref_for", true);
                let v = ctx.a.simple_exp("true", false);
                let p = ctx.a.create_object_property(k, v);
                properties.push(p);
            }

            if arg.is_none() && (is_v_bind || is_v_on) {
                has_dynamic_keys = true;
                if let Some(exp) = exp {
                    if is_v_bind {
                        if ctx.scopes.v_for > 0 {
                            let k = ctx.a.simple_exp("ref_for", true);
                            let v = ctx.a.simple_exp("true", false);
                            let p = ctx.a.create_object_property(k, v);
                            properties.push(p);
                        }
                        push_merge_arg!(ctx, None::<NodeId>);
                        merge_args.push(exp);
                    } else {
                        let helper = ctx.helper_node(RuntimeHelper::TO_HANDLERS);
                        let args = if is_component {
                            vec![exp]
                        } else {
                            let t = ctx.a.string("true");
                            vec![exp, t]
                        };
                        let call = ctx.a.create_call_expression(helper, args);
                        ctx.a.call_mut(call).loc = loc.clone();
                        push_merge_arg!(ctx, Some(call));
                    }
                } else {
                    let code = if is_v_bind {
                        ErrorCode::X_V_BIND_NO_EXPRESSION
                    } else {
                        ErrorCode::X_V_ON_NO_EXPRESSION
                    };
                    ctx.error(code, Some(loc.clone()));
                }
                continue;
            }

            if is_v_bind
                && modifiers
                    .iter()
                    .any(|m| ctx.a.exp(*m).content == "prop")
            {
                patch_flag |= pf::NEED_HYDRATION;
            }

            let transform = ctx.opts.directive_transforms.get(&name).copied();
            if let Some(kind) = transform {
                let result = super::apply_directive_transform(kind, prop, node, ctx);
                if !ssr {
                    for p in &result.props {
                        analyze_patch_flag(
                            *p,
                            ctx,
                            is_component,
                            is_dynamic_component,
                            &mut has_hydration_event_binding,
                            &mut has_vnode_hook,
                            &mut has_ref,
                            &mut has_class_binding,
                            &mut has_style_binding,
                            &mut has_dynamic_keys,
                            &mut dynamic_prop_names,
                        );
                    }
                }
                let arg_is_dynamic = arg
                    .map(|a| !is_static_exp(&ctx.a, a))
                    .unwrap_or(false);
                if is_v_on && arg.is_some() && arg_is_dynamic {
                    let obj = ctx.a.create_object_expression(result.props.clone());
                    ctx.a.obj_mut(obj).loc = element_loc.clone();
                    push_merge_arg!(ctx, Some(obj));
                } else {
                    properties.extend(result.props);
                }
                if let Some(rt) = result.need_runtime {
                    runtime_directives.push(prop);
                    if let Some(sym) = rt {
                        ctx.directive_import_map.insert(prop, sym);
                    }
                }
            } else if !is_built_in_directive(&name) {
                runtime_directives.push(prop);
                if has_children {
                    should_use_block = true;
                }
            }
        }
    }

    let mut props_expression: Option<NodeId> = None;

    if !merge_args.is_empty() {
        push_merge_arg!(ctx, None::<NodeId>);
        if merge_args.len() > 1 {
            let helper = ctx.helper_node(RuntimeHelper::MERGE_PROPS);
            let call = ctx.a.create_call_expression(helper, merge_args.clone());
            ctx.a.call_mut(call).loc = element_loc.clone();
            props_expression = Some(call);
        } else {
            props_expression = Some(merge_args[0]);
        }
    } else if !properties.is_empty() {
        let deduped = dedupe_properties(ctx, properties);
        let obj = ctx.a.create_object_expression(deduped);
        ctx.a.obj_mut(obj).loc = element_loc.clone();
        props_expression = Some(obj);
    }

    if has_dynamic_keys {
        patch_flag |= pf::FULL_PROPS;
    } else {
        if has_class_binding && !is_component {
            patch_flag |= pf::CLASS;
        }
        if has_style_binding && !is_component {
            patch_flag |= pf::STYLE;
        }
        if !dynamic_prop_names.is_empty() {
            patch_flag |= pf::PROPS;
        }
        if has_hydration_event_binding {
            patch_flag |= pf::NEED_HYDRATION;
        }
    }
    if !should_use_block
        && (patch_flag == 0 || patch_flag == pf::NEED_HYDRATION)
        && (has_ref || has_vnode_hook || !runtime_directives.is_empty())
    {
        patch_flag |= pf::NEED_PATCH;
    }

    if !ctx.opts.in_ssr {
        if let Some(pe) = props_expression {
            match ctx.a.node_type(pe) {
                NodeType::JsObjectExpression => {
                    let mut class_key_index: i64 = -1;
                    let mut style_key_index: i64 = -1;
                    let mut has_dynamic_key = false;
                    let properties = ctx.a.obj(pe).properties.clone();
                    for (i, p) in properties.iter().enumerate() {
                        let key = ctx.a.prop(*p).key;
                        if is_static_exp(&ctx.a, key) {
                            let c = ctx.a.exp(key).content.clone();
                            if c == "class" {
                                class_key_index = i as i64;
                            } else if c == "style" {
                                style_key_index = i as i64;
                            }
                        } else {
                            let is_handler_key = match ctx.a.node(key) {
                                Node::SimpleExpression(e) => e.is_handler_key,
                                Node::CompoundExpression(e) => e.is_handler_key,
                                _ => false,
                            };
                            if !is_handler_key {
                                has_dynamic_key = true;
                            }
                        }
                    }
                    let class_prop = if class_key_index >= 0 {
                        Some(properties[class_key_index as usize])
                    } else {
                        None
                    };
                    let style_prop = if style_key_index >= 0 {
                        Some(properties[style_key_index as usize])
                    } else {
                        None
                    };
                    if !has_dynamic_key {
                        if let Some(cp) = class_prop {
                            let v = ctx.a.prop(cp).value;
                            if !is_static_exp(&ctx.a, v) {
                                let helper = ctx.helper_node(RuntimeHelper::NORMALIZE_CLASS);
                                let call = ctx.a.create_call_expression(helper, vec![v]);
                                ctx.a.prop_mut(cp).value = call;
                            }
                        }
                        if let Some(sp) = style_prop {
                            let v = ctx.a.prop(sp).value;
                            let needs = has_style_binding
                                || matches!(ctx.a.node(v), Node::SimpleExpression(e)
                                    if e.content.trim().starts_with('['))
                                || ctx.a.is(v, NodeType::JsArrayExpression);
                            if needs {
                                let helper = ctx.helper_node(RuntimeHelper::NORMALIZE_STYLE);
                                let call = ctx.a.create_call_expression(helper, vec![v]);
                                ctx.a.prop_mut(sp).value = call;
                            }
                        }
                    } else {
                        let helper = ctx.helper_node(RuntimeHelper::NORMALIZE_PROPS);
                        props_expression = Some(ctx.a.create_call_expression(helper, vec![pe]));
                    }
                }
                NodeType::JsCallExpression => {}
                _ => {
                    // helper() call order matters for the import list
                    let helper = ctx.helper_node(RuntimeHelper::NORMALIZE_PROPS);
                    let guard = ctx.helper_node(RuntimeHelper::GUARD_REACTIVE_PROPS);
                    let inner = ctx.a.create_call_expression(guard, vec![pe]);
                    props_expression = Some(ctx.a.create_call_expression(helper, vec![inner]));
                }
            }
        }
    }

    BuildPropsResult {
        props: props_expression,
        directives: runtime_directives,
        patch_flag,
        dynamic_prop_names,
        should_use_block,
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_patch_flag(
    prop: NodeId,
    ctx: &mut TransformContext,
    is_component: bool,
    is_dynamic_component: bool,
    has_hydration_event_binding: &mut bool,
    has_vnode_hook: &mut bool,
    has_ref: &mut bool,
    has_class_binding: &mut bool,
    has_style_binding: &mut bool,
    has_dynamic_keys: &mut bool,
    dynamic_prop_names: &mut Vec<String>,
) {
    let key = ctx.a.prop(prop).key;
    let mut value = ctx.a.prop(prop).value;
    if is_static_exp(&ctx.a, key) {
        let name = ctx.a.exp(key).content.clone();
        let is_event_handler = is_on(&name);
        if is_event_handler
            && (!is_component || is_dynamic_component)
            && name.to_lowercase() != "onclick"
            && name != "onUpdate:modelValue"
            && !is_reserved_prop(&name)
        {
            *has_hydration_event_binding = true;
        }
        if is_event_handler && is_reserved_prop(&name) {
            *has_vnode_hook = true;
        }
        if is_event_handler && ctx.a.is(value, NodeType::JsCallExpression) {
            value = ctx.a.call(value).arguments[0];
        }
        let skip = ctx.a.is(value, NodeType::JsCacheExpression)
            || ((ctx.a.is(value, NodeType::SimpleExpression)
                || ctx.a.is(value, NodeType::CompoundExpression))
                && get_constant_type(value, ctx) > ConstantType::NotConstant);
        if skip {
            return;
        }
        if name == "ref" {
            *has_ref = true;
        } else if name == "class" {
            *has_class_binding = true;
        } else if name == "style" {
            *has_style_binding = true;
        } else if name != "key" && !dynamic_prop_names.contains(&name) {
            dynamic_prop_names.push(name.clone());
        }
        if is_component
            && (name == "class" || name == "style")
            && !dynamic_prop_names.contains(&name)
        {
            dynamic_prop_names.push(name);
        }
    } else {
        *has_dynamic_keys = true;
    }
}

fn dedupe_properties(ctx: &mut TransformContext, properties: Vec<NodeId>) -> Vec<NodeId> {
    let mut known: Vec<(String, NodeId)> = Vec::new();
    let mut deduped: Vec<NodeId> = Vec::new();
    for prop in properties {
        let key = ctx.a.prop(prop).key;
        let is_dynamic = ctx.a.is(key, NodeType::CompoundExpression)
            || !matches!(ctx.a.node(key), Node::SimpleExpression(e) if e.is_static);
        if is_dynamic {
            deduped.push(prop);
            continue;
        }
        let name = ctx.a.exp(key).content.clone();
        match known.iter().find(|(n, _)| *n == name).map(|(_, p)| *p) {
            Some(existing) => {
                if name == "style" || name == "class" || is_on(&name) {
                    merge_as_array(ctx, existing, prop);
                }
            }
            None => {
                known.push((name, prop));
                deduped.push(prop);
            }
        }
    }
    deduped
}

fn merge_as_array(ctx: &mut TransformContext, existing: NodeId, incoming: NodeId) {
    let ev = ctx.a.prop(existing).value;
    let iv = ctx.a.prop(incoming).value;
    if ctx.a.is(ev, NodeType::JsArrayExpression) {
        ctx.a.list_mut(ev).push(iv);
    } else {
        let arr = ctx.a.create_array_expression(vec![ev, iv]);
        let loc = ctx.a.prop(existing).loc.clone();
        ctx.a.array_mut(arr).loc = loc;
        ctx.a.prop_mut(existing).value = arr;
    }
}

pub fn build_directive_args(dir: NodeId, ctx: &mut TransformContext) -> NodeId {
    let mut dir_args: Vec<NodeId> = Vec::new();
    let runtime = ctx.directive_import_map.get(&dir).copied();
    if let Some(runtime) = runtime {
        let s = ctx.helper_string(runtime);
        let n = ctx.a.string(s);
        dir_args.push(n);
    } else {
        let name = ctx.a.dir(dir).name.clone();
        let from_setup = resolve_setup_reference(&format!("v-{name}"), ctx);
        if let Some(fs) = from_setup {
            let n = ctx.a.string(fs);
            dir_args.push(n);
        } else {
            ctx.helper(RuntimeHelper::RESOLVE_DIRECTIVE);
            ctx.add_directive(name.clone());
            let id = to_valid_asset_id(&name, "directive");
            let n = ctx.a.string(id);
            dir_args.push(n);
        }
    }
    let (loc, exp, arg, modifiers) = {
        let d = ctx.a.dir(dir);
        (d.loc.clone(), d.exp, d.arg, d.modifiers.clone())
    };
    if let Some(exp) = exp {
        dir_args.push(exp);
    }
    if let Some(arg) = arg {
        if exp.is_none() {
            let v = ctx.a.string("void 0");
            dir_args.push(v);
        }
        dir_args.push(arg);
    }
    if !modifiers.is_empty() {
        if arg.is_none() {
            if exp.is_none() {
                let v = ctx.a.string("void 0");
                dir_args.push(v);
            }
            let v = ctx.a.string("void 0");
            dir_args.push(v);
        }
        let true_exp =
            ctx.a
                .create_simple_expression("true", false, loc.clone(), ConstantType::NotConstant);
        let props: Vec<NodeId> = modifiers
            .iter()
            .map(|m| ctx.a.create_object_property(*m, true_exp))
            .collect();
        let obj = ctx.a.create_object_expression(props);
        ctx.a.obj_mut(obj).loc = loc;
        dir_args.push(obj);
    }
    let arr = ctx.a.create_array_expression(dir_args);
    let dloc = ctx.a.loc(dir).clone();
    ctx.a.array_mut(arr).loc = dloc;
    arr
}

fn stringify_dynamic_prop_names(props: &[String]) -> String {
    let mut s = String::from("[");
    for (i, p) in props.iter().enumerate() {
        s.push_str(&serde_json::to_string(p).unwrap());
        if i < props.len() - 1 {
            s.push_str(", ");
        }
    }
    s.push(']');
    s
}

fn is_component_tag(tag: &str) -> bool {
    tag == "component" || tag == "Component"
}
