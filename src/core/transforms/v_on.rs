//! Port of `compiler-core/src/transforms/vOn.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::transform::{DirectiveTransformResult, TransformContext, camelize};
use crate::core::utils::{has_scope_ref, is_fn_expression, is_member_expression, to_handler_key};

use super::transform_expression::process_expression;

pub fn transform_on(
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
    augmentor: Option<fn(DirectiveTransformResult, NodeId, NodeId, &mut TransformContext) -> DirectiveTransformResult>,
) -> DirectiveTransformResult {
    let (loc, modifiers, arg) = {
        let d = ctx.a.dir(dir);
        (d.loc.clone(), d.modifiers.clone(), d.arg.unwrap())
    };
    if ctx.a.dir(dir).exp.is_none() && modifiers.is_empty() {
        ctx.error(ErrorCode::X_V_ON_NO_EXPRESSION, Some(loc.clone()));
    }

    let event_name: NodeId;
    if ctx.a.is(arg, NodeType::SimpleExpression) {
        if ctx.a.exp(arg).is_static {
            let mut raw_name = ctx.a.exp(arg).content.clone();
            if raw_name.starts_with("vnode") {
                let l = ctx.a.loc(arg).clone();
                ctx.error(ErrorCode::X_VNODE_HOOKS, Some(l));
            }
            if let Some(rest) = raw_name.strip_prefix("vue:") {
                raw_name = format!("vnode-{rest}");
            }
            let tag_type = ctx.a.el(node).tag_type;
            let event_string = if tag_type != ElementType::Element
                || raw_name.starts_with("vnode")
                || !raw_name.chars().any(|c| c.is_ascii_uppercase())
            {
                to_handler_key(&camelize(&raw_name))
            } else {
                format!("on:{raw_name}")
            };
            let l = ctx.a.loc(arg).clone();
            event_name =
                ctx.a
                    .create_simple_expression(event_string, true, l, ConstantType::NotConstant);
        } else {
            let helper = ctx.helper_string(RuntimeHelper::TO_HANDLER_KEY);
            let open = ctx.a.string(format!("{helper}("));
            let close = ctx.a.string(")");
            event_name = ctx
                .a
                .create_compound_expression(vec![open, arg, close], loc_stub());
        }
    } else {
        let helper = ctx.helper_string(RuntimeHelper::TO_HANDLER_KEY);
        let open = ctx.a.string(format!("{helper}("));
        let close = ctx.a.string(")");
        ctx.a.compound_mut(arg).children.insert(0, open);
        ctx.a.compound_mut(arg).children.push(close);
        event_name = arg;
    }

    let mut exp = ctx.a.dir(dir).exp;
    if let Some(e) = exp {
        if ctx.a.exp(e).content.trim().is_empty() {
            exp = None;
        }
    }

    let mut should_cache = ctx.opts.cache_handlers && exp.is_none() && !ctx.in_v_once;
    if let Some(e) = exp {
        let is_member_exp = is_member_expression(&ctx.a, e);
        let is_inline_statement = !(is_member_exp || is_fn_expression(&ctx.a, e));
        let has_multiple_statements = ctx.a.exp(e).content.contains(';');

        let mut exp_node = e;
        if ctx.opts.prefix_identifiers {
            if is_inline_statement {
                ctx.add_identifier_str("$event");
            }
            exp_node = process_expression(e, ctx, false, has_multiple_statements, None);
            ctx.a.dir_mut(dir).exp = Some(exp_node);
            if is_inline_statement {
                ctx.remove_identifier_str("$event");
            }
            let is_const_exp = matches!(ctx.a.node(exp_node), Node::SimpleExpression(s)
                if s.const_type > ConstantType::NotConstant);
            let ids = ctx.identifiers.clone();
            should_cache = ctx.opts.cache_handlers
                && !ctx.in_v_once
                && !is_const_exp
                && !(is_member_exp && ctx.a.el(node).tag_type == ElementType::Component)
                && !has_scope_ref(&ctx.a, Some(exp_node), &ids);
            if should_cache && is_member_exp {
                if ctx.a.is(exp_node, NodeType::SimpleExpression) {
                    let c = ctx.a.exp(exp_node).content.clone();
                    ctx.a.exp_mut(exp_node).content = format!("{c} && {c}(...args)");
                } else {
                    let children = ctx.a.compound(exp_node).children.clone();
                    let amp = ctx.a.string(" && ");
                    let args = ctx.a.string("(...args)");
                    let mut new_children = children.clone();
                    new_children.push(amp);
                    new_children.extend(children);
                    new_children.push(args);
                    ctx.a.compound_mut(exp_node).children = new_children;
                }
            }
        }

        if is_inline_statement || (should_cache && is_member_exp) {
            let head = if is_inline_statement {
                if ctx.opts.is_ts {
                    "($event: any)".to_string()
                } else {
                    "$event".to_string()
                }
            } else if ctx.opts.is_ts {
                "\n//@ts-ignore\n(...args)".to_string()
            } else {
                "(...args)".to_string()
            };
            let open = ctx.a.string(format!(
                "{head} => {}",
                if has_multiple_statements { "{" } else { "(" }
            ));
            let close = ctx
                .a
                .string(if has_multiple_statements { "}" } else { ")" });
            exp_node = ctx
                .a
                .create_compound_expression(vec![open, exp_node, close], loc_stub());
        }
        exp = Some(exp_node);
    }

    let value = match exp {
        Some(e) => e,
        None => ctx.a.create_simple_expression(
            "() => {}",
            false,
            loc.clone(),
            ConstantType::NotConstant,
        ),
    };
    let p = ctx.a.create_object_property(event_name, value);
    let mut ret = DirectiveTransformResult {
        props: vec![p],
        need_runtime: None,
    };

    if let Some(aug) = augmentor {
        ret = aug(ret, dir, node, ctx);
    }

    if should_cache {
        let v = ctx.a.prop(ret.props[0]).value;
        let cached = ctx.cache(v, false, false);
        ctx.a.prop_mut(ret.props[0]).value = cached;
    }

    for p in &ret.props {
        let key = ctx.a.prop(*p).key;
        match ctx.a.node_mut(key) {
            Node::SimpleExpression(e) => e.is_handler_key = true,
            Node::CompoundExpression(e) => e.is_handler_key = true,
            _ => {}
        }
    }
    ret
}
