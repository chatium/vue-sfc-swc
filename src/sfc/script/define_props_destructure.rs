//! Port of `compiler-sfc/src/script/definePropsDestructure.ts`.

use std::collections::HashMap;

use swc_core::common::Spanned;
use swc_core::ecma::ast::*;

use crate::core::js_walk::{KnownIds, Walker};
use crate::core::options::BindingType;
use crate::core::transforms::transform_expression::gen_props_access_exp;

use super::context::{PropsDestructureBinding, ScriptCompileContext};
use super::defines::DEFINE_PROPS;
use super::utils::resolve_object_key;

pub fn process_props_destructure(
    ctx: &mut ScriptCompileContext,
    decl_id: &ObjectPat,
) -> Result<(), String> {
    if ctx.options.props_destructure == Some(false) {
        return Ok(());
    }
    ctx.props_destructure_decl = Some(decl_id.clone());

    for prop in &decl_id.props {
        match prop {
            ObjectPatProp::KeyValue(kv) => {
                let prop_key = match &kv.key {
                    PropName::Computed(_) => None,
                    other => resolve_object_key(other),
                };
                let prop_key = match prop_key {
                    Some(k) => k,
                    None => {
                        return Err(ctx.error(&format!(
                            "{DEFINE_PROPS}() destructure cannot use computed key."
                        )));
                    }
                };
                match &*kv.value {
                    Pat::Assign(a) => match &*a.left {
                        Pat::Ident(i) => register_binding(
                            ctx,
                            &prop_key,
                            &i.id.sym,
                            Some(a.right.clone()),
                        ),
                        _ => {
                            return Err(ctx.error(&format!(
                                "{DEFINE_PROPS}() destructure does not support nested patterns."
                            )));
                        }
                    },
                    Pat::Ident(i) => register_binding(ctx, &prop_key, &i.id.sym, None),
                    _ => {
                        return Err(ctx.error(&format!(
                            "{DEFINE_PROPS}() destructure does not support nested patterns."
                        )));
                    }
                }
            }
            ObjectPatProp::Assign(a) => {
                // shorthand `{ foo }` or `{ foo = 1 }`
                let key = a.key.id.sym.to_string();
                register_binding(ctx, &key, &key, a.value.clone());
            }
            ObjectPatProp::Rest(r) => {
                if let Pat::Ident(i) = &*r.arg {
                    let name = i.id.sym.to_string();
                    ctx.props_destructure_rest_id = Some(name.clone());
                    ctx.set_binding(&name, BindingType::SetupReactiveConst);
                }
            }
        }
    }
    Ok(())
}

fn register_binding(
    ctx: &mut ScriptCompileContext,
    key: &str,
    local: &str,
    default: Option<Box<Expr>>,
) {
    ctx.props_destructured_bindings.push((
        key.to_string(),
        PropsDestructureBinding {
            local: local.to_string(),
            default,
        },
    ));
    if local != key {
        ctx.set_binding(local, BindingType::PropsAliased);
        ctx.binding_metadata
            .props_aliases
            .insert(local.to_string(), key.to_string());
    }
}

pub fn transform_destructured_props(ctx: &mut ScriptCompileContext) -> Result<(), String> {
    if ctx.options.props_destructure == Some(false) {
        return Ok(());
    }
    let mut props_local_to_public: HashMap<String, String> = HashMap::new();
    for (key, b) in &ctx.props_destructured_bindings {
        props_local_to_public.insert(b.local.clone(), key.clone());
    }
    if props_local_to_public.is_empty() {
        return Ok(());
    }

    let ast = match ctx.script_setup_ast.clone() {
        Some(a) => a,
        None => return Ok(()),
    };

    let refs = {
        let mut known = KnownIds::default();
        let mut walker = Walker::new(&mut known, true);
        for item in &ast.body {
            if let ModuleItem::Stmt(s) = item {
                walker.walk_stmt(s);
            }
        }
        walker.finish()
    };

    let start_offset = ctx.start_offset;
    for r in refs {
        if !r.is_referenced || r.is_local {
            continue;
        }
        let public = match props_local_to_public.get(&r.name) {
            Some(p) => p.clone(),
            None => continue,
        };
        if r.assign_left.is_some() || r.update_arg.is_some() {
            return Err(
                ctx.error("Cannot assign to destructured props as they are readonly.")
            );
        }
        let access = gen_props_access_exp(&public);
        if r.shorthand_prop {
            ctx.s
                .append_left(r.end as usize + start_offset, &format!(": {access}"));
        } else {
            ctx.s.overwrite(
                r.start as usize + start_offset,
                r.end as usize + start_offset,
                &access,
            );
        }
    }

    // ponytail: `checkUsage` (watch/toRef misuse) is scope-insensitive here —
    // a local shadowing a destructured prop name would be a false positive.
    Ok(())
}

/// `genDestructuredDefaultValue`
pub fn gen_destructured_default_value(
    ctx: &mut ScriptCompileContext,
    key: &str,
    inferred_type: Option<&[String]>,
) -> Option<(String, bool)> {
    let default_val = ctx
        .props_destructured_bindings
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, b)| b.default.clone())?;
    let span = default_val.span();
    let value = ctx.get_string(span.lo.0 as usize, span.hi.0 as usize, true);
    let unwrapped = super::utils::unwrap_ts_node(&default_val).clone();

    let need_skip_factory = inferred_type.is_none()
        && matches!(unwrapped, Expr::Fn(_) | Expr::Arrow(_) | Expr::Ident(_));

    let need_factory_wrap = !need_skip_factory
        && !matches!(unwrapped, Expr::Lit(_))
        && !inferred_type
            .map(|t| t.iter().any(|x| x == "Function"))
            .unwrap_or(false);

    Some((
        if need_factory_wrap {
            format!("() => ({value})")
        } else {
            value
        },
        need_skip_factory,
    ))
}
