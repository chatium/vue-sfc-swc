//! Port of `compiler-core/src/transforms/transformVBindShorthand.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::transform::{TransformContext, camelize};

pub fn transform_v_bind_shorthand(node: NodeId, ctx: &mut TransformContext) {
    if !ctx.a.is(node, NodeType::Element) {
        return;
    }
    let props = ctx.a.el(node).props.clone();
    for prop in props {
        let (is_bind, exp, arg) = match ctx.a.node(prop) {
            Node::Directive(d) => (d.name == "bind", d.exp, d.arg),
            _ => continue,
        };
        if !is_bind || exp.is_some() {
            continue;
        }
        let arg = match arg {
            Some(a) => a,
            None => continue,
        };
        let is_static_simple =
            matches!(ctx.a.node(arg), Node::SimpleExpression(e) if e.is_static);
        if !is_static_simple {
            let loc = ctx.a.loc(arg).clone();
            ctx.error(ErrorCode::X_V_BIND_INVALID_SAME_NAME_ARGUMENT, Some(loc.clone()));
            let e = ctx
                .a
                .create_simple_expression("", true, loc, ConstantType::NotConstant);
            ctx.a.dir_mut(prop).exp = Some(e);
        } else {
            let content = ctx.a.exp(arg).content.clone();
            let prop_name = camelize(&content);
            let first = prop_name.chars().next();
            let valid = first
                .map(|c| {
                    c == '-'
                        || c == '_'
                        || c == '$'
                        || c.is_ascii_alphabetic()
                        || (c as u32) >= 0xA0
                })
                .unwrap_or(false);
            if valid {
                let loc = ctx.a.loc(arg).clone();
                let e = ctx.a.create_simple_expression(
                    prop_name,
                    false,
                    loc,
                    ConstantType::NotConstant,
                );
                ctx.a.dir_mut(prop).exp = Some(e);
            }
        }
    }
}
