//! `ssrTransformElement` / `ssrProcessElement`.

use crate::core::ast::*;
use crate::core::options::DirectiveTransformKind;
use crate::core::transform::TransformContext;
use crate::core::transforms::transform_element::build_props;
use crate::core::utils::{
    find_dir, has_dynamic_key_v_bind, is_built_in_directive, is_static_arg_of, is_static_exp,
};
use crate::dom::attrs::{escape_html, is_boolean_attr};
use crate::dom::tags::is_void_tag;

use super::codegen::{Parent, SsrCtx, process_children};
use super::component::build_ssr_props;
use super::{ssr_error, ssr_helper};

/// `@vue/shared`'s `propsToAttrMap`
fn props_to_attr(name: &str) -> Option<&'static str> {
    match name {
        "acceptCharset" => Some("accept-charset"),
        "className" => Some("class"),
        "htmlFor" => Some("for"),
        "httpEquiv" => Some("http-equiv"),
        _ => None,
    }
}

/// `isSSRSafeAttrName`
fn is_ssr_safe_attr_name(name: &str) -> bool {
    !name
        .chars()
        .any(|c| matches!(c, '>' | '/' | '=' | '"' | '\'' | '\t' | '\n' | '\u{c}' | ' '))
}

fn has_content_override_directive(node: NodeId, ctx: &TransformContext) -> bool {
    find_dir(&ctx.a, node, "text", false).is_some()
        || find_dir(&ctx.a, node, "html", false).is_some()
}

fn find_v_model(node: NodeId, ctx: &TransformContext) -> Option<NodeId> {
    ctx.a.el(node).props.iter().copied().find(|p| {
        matches!(ctx.a.node(*p), Node::Directive(d) if d.name == "model" && d.exp.is_some())
    })
}

fn is_true_false_value(prop: NodeId, ctx: &TransformContext) -> bool {
    match ctx.a.node(prop) {
        Node::Directive(d) => {
            d.name == "bind"
                && d.arg.map(|a| is_static_exp(&ctx.a, a)).unwrap_or(false)
                && matches!(
                    ctx.a.exp(d.arg.unwrap()).content.as_str(),
                    "true-value" | "false-value"
                )
        }
        Node::Attribute(a) => a.name == "true-value" || a.name == "false-value",
        _ => false,
    }
}

fn is_textarea_with_value(node: NodeId, prop: NodeId, ctx: &TransformContext) -> bool {
    ctx.a.el(node).tag == "textarea"
        && matches!(ctx.a.node(prop), Node::Directive(d)
            if d.name == "bind" && is_static_arg_of(&ctx.a, &d.arg, "value"))
}

/// `mergeCall`
fn merge_call(call: NodeId, arg: NodeId, ctx: &mut TransformContext) {
    let existing = ctx.a.call(call).arguments[0];
    if ctx.a.is(existing, NodeType::JsArrayExpression) {
        ctx.a.list_mut(existing).push(arg);
    } else {
        let array = ctx.a.create_array_expression(vec![existing, arg]);
        ctx.a.call_mut(call).arguments[0] = array;
    }
}

pub fn ssr_transform_element_exit(node: NodeId, ctx: &mut TransformContext) {
    let tag = ctx.a.el(node).tag.clone();
    let mut open_tag: Vec<NodeId> = vec![ctx.a.string(format!("<{tag}"))];
    let need_tag_for_runtime = tag == "textarea" || tag.contains('-') && !tag.starts_with('-');
    let has_dynamic_v_bind = has_dynamic_key_v_bind(&ctx.a, node);
    let has_custom_dir = ctx.a.el(node).props.iter().any(|p| {
        matches!(ctx.a.node(*p), Node::Directive(d) if !is_built_in_directive(&d.name))
    });

    // v-show is applied last so its style wins the merge
    let v_show_index = ctx
        .a
        .el(node)
        .props
        .iter()
        .position(|p| matches!(ctx.a.node(*p), Node::Directive(d) if d.name == "show"));
    if let Some(i) = v_show_index {
        let p = ctx.a.el_mut(node).props.remove(i);
        ctx.a.el_mut(node).props.push(p);
    }

    let need_merge_props = has_dynamic_v_bind || has_custom_dir;
    if need_merge_props {
        let props = ctx.a.el(node).props.clone();
        let r = build_props(node, ctx, Some(props), false, false, true);
        if r.props.is_some() || !r.directives.is_empty() {
            let merged_props = build_ssr_props(r.props, &r.directives, ctx);
            let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_ATTRS);
            let props_exp = ctx.a.create_call_expression(helper, vec![merged_props]);

            if tag == "textarea" {
                let existing_text = ctx.a.el(node).children.first().copied();
                let is_interpolation = existing_text
                    .map(|t| ctx.a.is(t, NodeType::Interpolation))
                    .unwrap_or(false);
                if !has_content_override_directive(node, ctx) && !is_interpolation {
                    let temp_id = format!("_temp{}", ctx.temps);
                    ctx.temps += 1;
                    let left = ctx.a.simple_exp(temp_id.clone(), false);
                    let assign = ctx.a.add(Node::AssignmentExpression(left, merged_props));
                    ctx.a.call_mut(props_exp).arguments = vec![assign];
                    let test = ctx.a.simple_exp(format!("\"value\" in {temp_id}"), false);
                    let consequent = ctx.a.simple_exp(format!("{temp_id}.value"), false);
                    let existing_content = existing_text
                        .and_then(|t| match ctx.a.node(t) {
                            Node::Text(t) => Some(t.content.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let alternate = ctx.a.simple_exp(existing_content, true);
                    let cond = ctx
                        .a
                        .create_conditional_expression(test, consequent, alternate, false);
                    let interp = ssr_helper(ctx, RuntimeHelper::SSR_INTERPOLATE);
                    let call = ctx.a.create_call_expression(interp, vec![cond]);
                    ctx.ssr_state.raw_children.insert(node, call);
                }
            } else if tag == "input" {
                if let Some(v_model) = find_v_model(node, ctx) {
                    let temp_id = format!("_temp{}", ctx.temps);
                    ctx.temps += 1;
                    let temp_exp = ctx.a.simple_exp(temp_id, false);
                    let assign = ctx.a.add(Node::AssignmentExpression(temp_exp, merged_props));
                    let model = ctx.a.dir(v_model).exp.unwrap();
                    let get_props =
                        ssr_helper(ctx, RuntimeHelper::SSR_GET_DYNAMIC_MODEL_PROPS);
                    let get_call = ctx
                        .a
                        .create_call_expression(get_props, vec![temp_exp, model]);
                    let merge = ctx.helper_node(RuntimeHelper::MERGE_PROPS);
                    let merge_call_id = ctx
                        .a
                        .create_call_expression(merge, vec![temp_exp, get_call]);
                    let seq = ctx
                        .a
                        .add(Node::SequenceExpression(vec![assign, merge_call_id]));
                    ctx.a.call_mut(props_exp).arguments = vec![seq];
                }
            } else if !r.directives.is_empty() && ctx.a.el(node).children.is_empty() {
                if !has_content_override_directive(node, ctx) {
                    let temp_id = format!("_temp{}", ctx.temps);
                    ctx.temps += 1;
                    let left = ctx.a.simple_exp(temp_id.clone(), false);
                    let assign = ctx.a.add(Node::AssignmentExpression(left, merged_props));
                    ctx.a.call_mut(props_exp).arguments = vec![assign];
                    let test = ctx
                        .a
                        .simple_exp(format!("\"textContent\" in {temp_id}"), false);
                    let content = ctx.a.simple_exp(format!("{temp_id}.textContent"), false);
                    let interp = ssr_helper(ctx, RuntimeHelper::SSR_INTERPOLATE);
                    let consequent = ctx.a.create_call_expression(interp, vec![content]);
                    let alternate = ctx
                        .a
                        .simple_exp(format!("{temp_id}.innerHTML ?? ''"), false);
                    let cond = ctx
                        .a
                        .create_conditional_expression(test, consequent, alternate, false);
                    ctx.ssr_state.raw_children.insert(node, cond);
                }
            }

            if need_tag_for_runtime {
                let t = ctx.a.string(format!("\"{tag}\""));
                ctx.a.call_mut(props_exp).arguments.push(t);
            }
            open_tag.push(props_exp);
        }
    }

    let mut dynamic_class_binding: Option<NodeId> = None;
    let mut static_class_binding: Option<String> = None;
    let mut dynamic_style_binding: Option<NodeId> = None;

    let props = ctx.a.el(node).props.clone();
    for prop in props {
        if tag == "input" && is_true_false_value(prop, ctx) {
            continue;
        }
        if !ctx.a.is(prop, NodeType::Directive) {
            let (name, value) = {
                let a = ctx.a.attr(prop);
                (a.name.clone(), a.value.as_ref().map(|v| v.content.clone()))
            };
            if tag == "textarea" && name == "value" && value.is_some() {
                let escaped = escape_html(value.as_deref().unwrap());
                let n = ctx.a.string(escaped);
                ctx.ssr_state.raw_children.insert(node, n);
            } else if !need_merge_props {
                if name == "key" || name == "ref" {
                    continue;
                }
                if name == "class" {
                    if let Some(v) = &value {
                        static_class_binding = Some(serde_json::to_string(v).unwrap());
                    }
                }
                let part = match &value {
                    Some(v) => format!(" {name}=\"{}\"", escape_html(v)),
                    None => format!(" {name}"),
                };
                let n = ctx.a.string(part);
                open_tag.push(n);
            }
            continue;
        }

        let dir_name = ctx.a.dir(prop).name.clone();
        let dir_exp = ctx.a.dir(prop).exp;
        if dir_name == "html" && dir_exp.is_some() {
            let open = ctx.a.string("(");
            let close = ctx.a.string(") ?? ''");
            let c = ctx.a.create_compound_expression(
                vec![open, dir_exp.unwrap(), close],
                loc_stub(),
            );
            ctx.ssr_state.raw_children.insert(node, c);
        } else if dir_name == "text" && dir_exp.is_some() {
            let loc = ctx.a.loc(prop).clone();
            let interp = ctx.a.create_interpolation(dir_exp.unwrap(), loc);
            ctx.a.el_mut(node).children = vec![interp];
        } else if dir_name == "slot" {
            let loc = ctx.a.loc(prop).clone();
            ctx.error(
                crate::core::errors::ErrorCode::X_V_SLOT_MISPLACED,
                Some(loc),
            );
        } else if is_textarea_with_value(node, prop, ctx) && dir_exp.is_some() {
            if !need_merge_props {
                let loc = ctx.a.loc(prop).clone();
                let interp = ctx.a.create_interpolation(dir_exp.unwrap(), loc);
                ctx.a.el_mut(node).children = vec![interp];
            }
        } else if !need_merge_props && dir_name != "on" {
            let transform = ctx.opts.directive_transforms.get(&dir_name).copied();
            let transform = match transform {
                Some(t) => t,
                None => continue,
            };
            let r = ssr_directive_transform(transform, prop, node, ctx, &mut open_tag);
            for p in r {
                let (key, value) = {
                    let p = ctx.a.prop(p);
                    (p.key, p.value)
                };
                if is_static_exp(&ctx.a, key) {
                    let mut attr_name = ctx.a.exp(key).content.clone();
                    if attr_name == "key" || attr_name == "ref" {
                        continue;
                    }
                    if attr_name == "class" {
                        let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_CLASS);
                        let call = ctx.a.create_call_expression(helper, vec![value]);
                        dynamic_class_binding = Some(call);
                        let a = ctx.a.string(" class=\"");
                        let b = ctx.a.string("\"");
                        open_tag.extend([a, call, b]);
                    } else if attr_name == "style" {
                        if let Some(existing) = dynamic_style_binding {
                            merge_call(existing, value, ctx);
                        } else {
                            let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_STYLE);
                            let call = ctx.a.create_call_expression(helper, vec![value]);
                            dynamic_style_binding = Some(call);
                            let a = ctx.a.string(" style=\"");
                            let b = ctx.a.string("\"");
                            open_tag.extend([a, call, b]);
                        }
                    } else {
                        attr_name = if tag.contains('-') && !tag.starts_with('-') {
                            attr_name
                        } else {
                            props_to_attr(&attr_name)
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| attr_name.to_lowercase())
                        };
                        if is_boolean_attr(&attr_name) {
                            let helper =
                                ssr_helper(ctx, RuntimeHelper::SSR_INCLUDE_BOOLEAN_ATTR);
                            let call = ctx.a.create_call_expression(helper, vec![value]);
                            let consequent = ctx.a.simple_exp(format!(" {attr_name}"), true);
                            let alternate = ctx.a.simple_exp("", true);
                            let cond = ctx.a.create_conditional_expression(
                                call, consequent, alternate, false,
                            );
                            open_tag.push(cond);
                        } else if is_ssr_safe_attr_name(&attr_name) {
                            let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_ATTR);
                            let call = ctx.a.create_call_expression(helper, vec![key, value]);
                            open_tag.push(call);
                        } else {
                            let loc = ctx.a.loc(key).clone();
                            ssr_error(ctx, 65, Some(loc));
                        }
                    }
                } else {
                    let mut args = vec![key, value];
                    if need_tag_for_runtime {
                        let t = ctx.a.string(format!("\"{tag}\""));
                        args.push(t);
                    }
                    let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_DYNAMIC_ATTR);
                    let call = ctx.a.create_call_expression(helper, args);
                    open_tag.push(call);
                }
            }
        }
    }

    if let (Some(dynamic), Some(static_class)) = (dynamic_class_binding, &static_class_binding) {
        let arg = ctx.a.string(static_class.clone());
        merge_call(dynamic, arg, ctx);
        // `removeStaticBinding`
        let i = open_tag.iter().position(|e| {
            ctx.a
                .str_of(*e)
                .map(|s| s.starts_with(" class=\"") && s.ends_with('"') && s.len() > 9)
                .unwrap_or(false)
        });
        if let Some(i) = i {
            open_tag.remove(i);
        }
    }

    if let Some(scope_id) = ctx.opts.scope_id.clone() {
        let n = ctx.a.string(format!(" {scope_id}"));
        open_tag.push(n);
    }

    let lit = ctx.a.add(Node::TemplateLiteral(open_tag));
    ctx.a.el_mut(node).ssr_codegen_node = Some(lit);
}

/// Runs one directive transform, forwarding `ssrTagParts` into the open tag.
fn ssr_directive_transform(
    kind: DirectiveTransformKind,
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
    open_tag: &mut Vec<NodeId>,
) -> Vec<NodeId> {
    match kind {
        DirectiveTransformKind::SsrModel => {
            let r = super::v_model::ssr_transform_model(dir, node, ctx);
            open_tag.extend(r.ssr_tag_parts);
            r.props
        }
        DirectiveTransformKind::SsrShow => super::v_show::ssr_transform_show(dir, ctx).props,
        other => crate::core::transforms::apply_directive_transform(other, dir, node, ctx).props,
    }
}

pub fn ssr_process_element(node: NodeId, sctx: &mut SsrCtx, ctx: &mut TransformContext) {
    let lit = match ctx.a.el(node).ssr_codegen_node {
        Some(l) => l,
        None => return,
    };
    let elements = match ctx.a.node(lit) {
        Node::TemplateLiteral(e) => e.clone(),
        _ => unreachable!(),
    };
    for e in elements {
        sctx.push_node_part(&mut ctx.a, e);
    }
    if sctx.with_slot_scope_id {
        let s = ctx.a.simple_exp("_scopeId", false);
        sctx.push_node_part(&mut ctx.a, s);
    }
    sctx.push_str_part(&mut ctx.a, ">");

    let raw = ctx.ssr_state.raw_children.get(&node).copied();
    if let Some(raw) = raw {
        sctx.push_node_part(&mut ctx.a, raw);
    } else if !ctx.a.el(node).children.is_empty() {
        process_children(Parent::Node(node), sctx, ctx, false, false, false);
    }

    let tag = ctx.a.el(node).tag.clone();
    if !is_void_tag(&tag) {
        sctx.push_str_part(&mut ctx.a, &format!("</{tag}>"));
    }
}
