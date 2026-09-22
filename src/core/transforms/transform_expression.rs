//! Port of `compiler-core/src/transforms/transformExpression.ts`.

use std::collections::HashMap;

use crate::core::ast::*;
use crate::core::errors::{ErrorCode, create_compiler_error};
use crate::core::js_walk::{IdRef, KnownIds, Walker};
use crate::core::options::BindingType;
use crate::core::transform::TransformContext;
use crate::core::utils::{advance_position_with_clone, find_dir, is_simple_identifier};

const GLOBALS_ALLOWED: &[&str] = &[
    "Infinity",
    "undefined",
    "NaN",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "Math",
    "Number",
    "Date",
    "Array",
    "Object",
    "Boolean",
    "String",
    "RegExp",
    "Map",
    "Set",
    "JSON",
    "Intl",
    "BigInt",
    "console",
    "Error",
    "Symbol",
];

pub fn is_globally_allowed(name: &str) -> bool {
    GLOBALS_ALLOWED.contains(&name)
}

fn is_literal_whitelisted(name: &str) -> bool {
    matches!(name, "true" | "false" | "null" | "this")
}

/// `genPropsAccessExp`
pub fn gen_props_access_exp(name: &str) -> String {
    let mut chars = name.chars();
    let ok = match chars.next() {
        Some(c) => {
            (c == '_' || c == '$' || c.is_ascii_alphabetic() || (c as u32) >= 0xA0)
                && chars.all(|c| {
                    c == '_' || c == '$' || c.is_ascii_alphanumeric() || (c as u32) >= 0xA0
                })
        }
        None => false,
    };
    if ok {
        format!("__props.{name}")
    } else {
        format!("__props[{}]", serde_json::to_string(name).unwrap())
    }
}

fn is_const(t: Option<BindingType>) -> bool {
    matches!(
        t,
        Some(BindingType::SetupConst) | Some(BindingType::LiteralConst)
    )
}

pub fn transform_expression(node: NodeId, ctx: &mut TransformContext) {
    match ctx.a.node_type(node) {
        NodeType::Interpolation => {
            let content = ctx.a.interp(node).content;
            let processed = process_expression(content, ctx, false, false, None);
            ctx.a.interp_mut(node).content = processed;
        }
        NodeType::Element => {
            let memo = find_dir(&ctx.a, node, "memo", false);
            let props = ctx.a.el(node).props.clone();
            for prop in props {
                if !ctx.a.is(prop, NodeType::Directive) {
                    continue;
                }
                let (name, exp, arg) = {
                    let d = ctx.a.dir(prop);
                    (d.name.clone(), d.exp, d.arg)
                };
                if name == "for" {
                    continue;
                }
                if let Some(exp) = exp {
                    let arg_is_key = arg
                        .map(|a| {
                            matches!(ctx.a.node(a), Node::SimpleExpression(e) if e.content == "key")
                        })
                        .unwrap_or(false);
                    let skip_for_memo_key = memo.is_some()
                        && ctx.v_for_memo_keyed_nodes.contains(&node)
                        && arg.is_some()
                        && arg
                            .map(|a| ctx.a.is(a, NodeType::SimpleExpression))
                            .unwrap_or(false)
                        && arg_is_key;
                    if ctx.a.is(exp, NodeType::SimpleExpression)
                        && !(name == "on" && arg.is_some())
                        && !skip_for_memo_key
                    {
                        let processed = process_expression(exp, ctx, name == "slot", false, None);
                        ctx.a.dir_mut(prop).exp = Some(processed);
                    }
                }
                if let Some(arg) = arg {
                    let is_dynamic =
                        matches!(ctx.a.node(arg), Node::SimpleExpression(e) if !e.is_static);
                    if is_dynamic {
                        let processed = process_expression(arg, ctx, false, false, None);
                        ctx.a.dir_mut(prop).arg = Some(processed);
                    }
                }
            }
        }
        _ => {}
    }
}

fn byte_to_utf16(source: &str, byte: usize) -> usize {
    let byte = byte.min(source.len());
    source[..byte].encode_utf16().count()
}

struct RewriteCtx {
    inline: bool,
    is_ts: bool,
}

#[allow(clippy::too_many_arguments)]
fn rewrite_identifier(
    raw: &str,
    id: Option<&IdRef>,
    ctx: &mut TransformContext,
    local_vars: &HashMap<String, i32>,
    raw_exp_u16: &[u16],
    wrapped_source: &str,
    known_ids: &KnownIds,
    rc: &RewriteCtx,
) -> (String, Option<(u32, u32)>) {
    let ty = ctx.opts.binding_metadata.get(raw);
    if rc.inline {
        let is_assignment_lval = id.map(|i| i.assign_left.is_some()).unwrap_or(false);
        let is_update_arg = id.map(|i| i.update_arg.is_some()).unwrap_or(false);
        let is_destructure_assignment =
            id.map(|i| i.in_destructure_assignment).unwrap_or(false);
        let is_new_expression = id.map(|i| i.in_new_expression).unwrap_or(false);

        if is_const(ty)
            || ty == Some(BindingType::SetupReactiveConst)
            || local_vars.get(raw).copied().unwrap_or(0) > 0
        {
            return (raw.to_string(), None);
        } else if ty == Some(BindingType::SetupRef) {
            return (format!("{raw}.value"), None);
        } else if ty == Some(BindingType::SetupMaybeRef) {
            return if is_assignment_lval || is_update_arg || is_destructure_assignment {
                (format!("{raw}.value"), None)
            } else {
                let unref = ctx.helper_string(RuntimeHelper::UNREF);
                let wrapped = format!("{unref}({raw})");
                (
                    if is_new_expression {
                        format!("({wrapped})")
                    } else {
                        wrapped
                    },
                    None,
                )
            };
        } else if ty == Some(BindingType::SetupLet) {
            if let Some(info) = id.and_then(|i| i.assign_left.clone()) {
                // `rawExp.slice(rVal.start - 1, rVal.end - 1)`
                let r_start = byte_to_utf16(wrapped_source, info.right_start as usize)
                    .saturating_sub(1);
                let r_end =
                    byte_to_utf16(wrapped_source, info.right_end as usize).saturating_sub(1);
                let r_exp = slice_u16(raw_exp_u16, r_start, r_end);
                let tmp = ctx.a.simple_exp(r_exp, false);
                let processed =
                    process_expression(tmp, ctx, false, false, Some(known_ids.flat()));
                let r_exp_string = stringify_expression(&ctx.a, processed);
                let is_ref = ctx.helper_string(RuntimeHelper::IS_REF);
                let ts = if rc.is_ts { " //@ts-ignore\n" } else { "" };
                return (
                    format!(
                        "{is_ref}({raw}){ts} ? {raw}.value {} {r_exp_string} : {raw}",
                        info.op
                    ),
                    None,
                );
            } else if let Some(info) = id.and_then(|i| i.update_arg.clone()) {
                let prefix = if info.prefix { info.op.clone() } else { String::new() };
                let postfix = if info.prefix { String::new() } else { info.op.clone() };
                let is_ref = ctx.helper_string(RuntimeHelper::IS_REF);
                let ts = if rc.is_ts { " //@ts-ignore\n" } else { "" };
                return (
                    format!(
                        "{is_ref}({raw}){ts} ? {prefix}{raw}.value{postfix} : {prefix}{raw}{postfix}"
                    ),
                    Some((info.start, info.end)),
                );
            } else if is_destructure_assignment {
                return (raw.to_string(), None);
            } else {
                let unref = ctx.helper_string(RuntimeHelper::UNREF);
                let wrapped = format!("{unref}({raw})");
                return (
                    if is_new_expression {
                        format!("({wrapped})")
                    } else {
                        wrapped
                    },
                    None,
                );
            }
        } else if ty == Some(BindingType::Props) {
            return (gen_props_access_exp(raw), None);
        } else if ty == Some(BindingType::PropsAliased) {
            let aliased = ctx
                .opts
                .binding_metadata
                .props_aliases
                .get(raw)
                .cloned()
                .unwrap_or_default();
            return (gen_props_access_exp(&aliased), None);
        }
    } else {
        if let Some(t) = ty {
            if t.as_str().starts_with("setup") || t == BindingType::LiteralConst {
                return (format!("$setup.{raw}"), None);
            } else if t == BindingType::PropsAliased {
                let aliased = ctx
                    .opts
                    .binding_metadata
                    .props_aliases
                    .get(raw)
                    .cloned()
                    .unwrap_or_default();
                return (format!("$props['{aliased}']"), None);
            } else {
                return (format!("${}.{raw}", t.as_str()), None);
            }
        }
    }
    (format!("_ctx.{raw}"), None)
}

pub fn stringify_expression(a: &Arena, exp: NodeId) -> String {
    match a.node(exp) {
        Node::Str(s) => s.clone(),
        Node::SimpleExpression(e) => e.content.clone(),
        Node::CompoundExpression(c) => c
            .children
            .iter()
            .map(|c| stringify_expression(a, *c))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

pub fn process_expression(
    node: NodeId,
    ctx: &mut TransformContext,
    as_params: bool,
    as_raw_statements: bool,
    local_vars: Option<HashMap<String, i32>>,
) -> NodeId {
    if !ctx.opts.prefix_identifiers {
        return node;
    }
    let raw_exp = ctx.a.exp(node).content.clone();
    if raw_exp.trim().is_empty() {
        return node;
    }
    let local_vars = local_vars.unwrap_or_else(|| ctx.identifiers.clone());
    let rc = RewriteCtx {
        inline: ctx.opts.inline,
        is_ts: ctx.opts.is_ts,
    };

    let ast = ctx.a.exp(node).ast.clone();
    if ast.is_failed() {
        return node;
    }

    // fast path: plain identifier
    if ast.is_null() || (ast.is_undefined() && is_simple_identifier(&raw_exp)) {
        let is_scope_var_reference = ctx.identifiers.get(&raw_exp).copied().unwrap_or(0) > 0;
        let is_allowed_global = is_globally_allowed(&raw_exp);
        let is_literal = is_literal_whitelisted(&raw_exp);
        let has_binding = ctx.opts.binding_metadata.get(&raw_exp).is_some();
        if !as_params
            && !is_scope_var_reference
            && !is_literal
            && (!is_allowed_global || has_binding)
        {
            if is_const(ctx.opts.binding_metadata.get(&raw_exp)) {
                ctx.a.exp_mut(node).const_type = ConstantType::CanSkipPatch;
            }
            let known = KnownIds::new(ctx.identifiers.clone());
            let (content, _) = rewrite_identifier(
                &raw_exp,
                None,
                ctx,
                &local_vars,
                &[],
                "",
                &known,
                &rc,
            );
            ctx.a.exp_mut(node).content = content;
        } else if !is_scope_var_reference {
            ctx.a.exp_mut(node).const_type = if is_literal {
                ConstantType::CanStringify
            } else {
                ConstantType::CanCache
            };
        }
        return node;
    }

    // parse if the parser did not already
    let wrapped_source = if as_raw_statements {
        format!(" {raw_exp} ")
    } else {
        format!("({raw_exp}){}", if as_params { "=>{}" } else { "" })
    };
    let parsed: ExpAst = if ast.is_undefined() {
        let ts = true;
        let r = if as_raw_statements {
            crate::core::jsparse::parse_program(&wrapped_source, ts)
                .map(|p| ExpAst::Program(Box::new(p)))
        } else {
            crate::core::jsparse::parse_expression(&wrapped_source, ts)
                .map(|e| ExpAst::Expr(Box::new(e)))
        };
        match r {
            Ok(a) => a,
            Err(msg) => {
                let loc = ctx.a.exp(node).loc.clone();
                let e = create_compiler_error(
                    ErrorCode::X_INVALID_EXPRESSION,
                    Some(loc),
                    Some(&msg),
                );
                ctx.on_error(e);
                return node;
            }
        }
    } else {
        ast
    };

    let mut known_ids = KnownIds::new(ctx.identifiers.clone());
    let refs: Vec<IdRef> = {
        let mut walker = Walker::new(&mut known_ids, true);
        match &parsed {
            ExpAst::Expr(e) => walker.walk_expr(e),
            ExpAst::Program(p) => walker.walk_program(p),
            _ => {}
        }
        walker.finish()
    };

    let raw_u16: Vec<u16> = raw_exp.encode_utf16().collect();

    struct Resolved {
        name: String,
        start: usize,
        end: usize,
        prefix: Option<String>,
        is_constant: bool,
    }

    let mut ids: Vec<Resolved> = Vec::new();
    for id in &refs {
        let need_prefix = id.is_referenced && can_prefix(&id.name);
        if need_prefix && !id.is_local {
            let prefix = if id.shorthand_prop {
                Some(format!("{}: ", id.name))
            } else {
                None
            };
            let (name, span_override) = rewrite_identifier(
                &id.name,
                Some(id),
                ctx,
                &local_vars,
                &raw_u16,
                &wrapped_source,
                &known_ids,
                &rc,
            );
            let (bstart, bend) = span_override.unwrap_or((id.start, id.end));
            ids.push(Resolved {
                name,
                start: byte_to_utf16(&wrapped_source, bstart as usize),
                end: byte_to_utf16(&wrapped_source, bend as usize),
                prefix,
                is_constant: false,
            });
        } else {
            let is_constant = !(need_prefix && id.is_local)
                && (id.no_parent || !id.parent_call_new_member);
            ids.push(Resolved {
                name: id.name.clone(),
                start: byte_to_utf16(&wrapped_source, id.start as usize),
                end: byte_to_utf16(&wrapped_source, id.end as usize),
                prefix: None,
                is_constant,
            });
        }
    }

    ids.sort_by_key(|i| i.start);

    let loc_start = ctx.a.exp(node).loc.start;
    let mut children: Vec<NodeId> = Vec::new();
    let len = ids.len();
    for i in 0..len {
        // range is offset by -1 due to the wrapping parens when parsed
        let start = ids[i].start.saturating_sub(1);
        let end = ids[i].end.saturating_sub(1);
        let last_end = if i > 0 {
            ids[i - 1].end.saturating_sub(1)
        } else {
            0
        };
        let leading: String = slice_u16(&raw_u16, last_end, start);
        if !leading.is_empty() || ids[i].prefix.is_some() {
            let text = format!("{leading}{}", ids[i].prefix.clone().unwrap_or_default());
            let n = ctx.a.string(text);
            children.push(n);
        }
        let source = slice_u16(&raw_u16, start, end);
        let loc = SourceLocation {
            start: advance_position_with_clone(&loc_start, &source, Some(start)),
            end: advance_position_with_clone(&loc_start, &source, Some(end)),
            source: source.clone().into(),
        };
        let const_type = if ids[i].is_constant {
            ConstantType::CanStringify
        } else {
            ConstantType::NotConstant
        };
        let exp = ctx
            .a
            .create_simple_expression(ids[i].name.clone(), false, loc, const_type);
        children.push(exp);
        if i == len - 1 && end < raw_u16.len() {
            let rest = slice_u16(&raw_u16, end, raw_u16.len());
            let n = ctx.a.string(rest);
            children.push(n);
        }
    }

    let own_keys = {
        let mut k = known_ids.own_keys();
        k.sort();
        k
    };

    if !children.is_empty() {
        let loc = ctx.a.exp(node).loc.clone();
        let ret = ctx.a.create_compound_expression(children, loc);
        ctx.a.compound_mut(ret).ast = parsed;
        ctx.a.compound_mut(ret).identifiers = own_keys;
        ret
    } else {
        ctx.a.exp_mut(node).const_type = ConstantType::CanStringify;
        ctx.a.exp_mut(node).identifiers = own_keys;
        node
    }
}

fn slice_u16(units: &[u16], start: usize, end: usize) -> String {
    let start = start.min(units.len());
    let end = end.min(units.len()).max(start);
    String::from_utf16_lossy(&units[start..end])
}

fn can_prefix(name: &str) -> bool {
    if is_globally_allowed(name) {
        return false;
    }
    if name == "require" {
        return false;
    }
    true
}
