//! Port of `compiler-core/src/transforms/vBind.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::transform::{DirectiveTransformResult, TransformContext, camelize};

pub fn transform_bind(dir: NodeId, ctx: &mut TransformContext) -> DirectiveTransformResult {
    let (modifiers, loc, arg, exp) = {
        let d = ctx.a.dir(dir);
        (d.modifiers.clone(), d.loc.clone(), d.arg.unwrap(), d.exp)
    };

    if let Some(e) = exp {
        if matches!(ctx.a.node(e), Node::SimpleExpression(s) if s.content.trim().is_empty()) {
            ctx.error(ErrorCode::X_V_BIND_NO_EXPRESSION, Some(loc.clone()));
            let empty = ctx.a.create_simple_expression(
                "",
                true,
                loc.clone(),
                ConstantType::NotConstant,
            );
            let p = ctx.a.create_object_property(arg, empty);
            return DirectiveTransformResult {
                props: vec![p],
                need_runtime: None,
            };
        }
    }
    if !ctx.a.is(arg, NodeType::SimpleExpression) {
        let open = ctx.a.string("(");
        let close = ctx.a.string(") || \"\"");
        ctx.a.compound_mut(arg).children.insert(0, open);
        ctx.a.compound_mut(arg).children.push(close);
    } else if !ctx.a.exp(arg).is_static {
        let content = ctx.a.exp(arg).content.clone();
        ctx.a.exp_mut(arg).content = if content.is_empty() {
            "\"\"".to_string()
        } else {
            format!("{content} || \"\"")
        };
    }

    let has_camel = modifiers
        .iter()
        .any(|m| ctx.a.exp(*m).content == "camel");
    if has_camel {
        if ctx.a.is(arg, NodeType::SimpleExpression) {
            if ctx.a.exp(arg).is_static {
                let c = camelize(&ctx.a.exp(arg).content);
                ctx.a.exp_mut(arg).content = c;
            } else {
                let helper = ctx.helper_string(RuntimeHelper::CAMELIZE);
                let c = ctx.a.exp(arg).content.clone();
                ctx.a.exp_mut(arg).content = format!("{helper}({c})");
            }
        } else {
            let helper = ctx.helper_string(RuntimeHelper::CAMELIZE);
            let open = ctx.a.string(format!("{helper}("));
            let close = ctx.a.string(")");
            ctx.a.compound_mut(arg).children.insert(0, open);
            ctx.a.compound_mut(arg).children.push(close);
        }
    }

    if !ctx.opts.in_ssr {
        if modifiers.iter().any(|m| ctx.a.exp(*m).content == "prop") {
            inject_prefix(ctx, arg, ".");
        }
        if modifiers.iter().any(|m| ctx.a.exp(*m).content == "attr") {
            inject_prefix(ctx, arg, "^");
        }
    }

    let exp = match ctx.a.dir(dir).exp {
        Some(e) => e,
        None => ctx.a.add(Node::None),
    };
    let p = ctx.a.create_object_property(arg, exp);
    DirectiveTransformResult {
        props: vec![p],
        need_runtime: None,
    }
}

fn inject_prefix(ctx: &mut TransformContext, arg: NodeId, prefix: &str) {
    if ctx.a.is(arg, NodeType::SimpleExpression) {
        if ctx.a.exp(arg).is_static {
            let c = ctx.a.exp(arg).content.clone();
            ctx.a.exp_mut(arg).content = format!("{prefix}{c}");
        } else {
            let c = ctx.a.exp(arg).content.clone();
            ctx.a.exp_mut(arg).content = format!("`{prefix}${{{c}}}`");
        }
    } else {
        let open = ctx.a.string(format!("'{prefix}' + ("));
        let close = ctx.a.string(")");
        ctx.a.compound_mut(arg).children.insert(0, open);
        ctx.a.compound_mut(arg).children.push(close);
    }
}
