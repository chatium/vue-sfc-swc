//! Port of `compiler-sfc/src/script/define{Props,Emits,Model,Options,Slots,Expose}.ts`.

use swc_core::common::Spanned;
use swc_core::ecma::ast::*;

use crate::core::options::BindingType;

use super::analyze_script_bindings::object_or_array_keys;
use super::context::{ModelDecl, ScriptCompileContext};
use super::resolve_type::{infer_runtime_type, infer_runtime_type_of_prop, resolve_type_elements};
use super::utils::*;

pub const DEFINE_PROPS: &str = "defineProps";
pub const WITH_DEFAULTS: &str = "withDefaults";
pub const DEFINE_EMITS: &str = "defineEmits";
pub const DEFINE_EXPOSE: &str = "defineExpose";
pub const DEFINE_OPTIONS: &str = "defineOptions";
pub const DEFINE_SLOTS: &str = "defineSlots";
pub const DEFINE_MODEL: &str = "defineModel";

pub const MACROS: &[&str] = &[
    DEFINE_PROPS,
    DEFINE_EMITS,
    DEFINE_EXPOSE,
    DEFINE_OPTIONS,
    DEFINE_SLOTS,
    DEFINE_MODEL,
    WITH_DEFAULTS,
];

fn span(e: &impl Spanned) -> (usize, usize) {
    let s = e.span();
    (s.lo.0 as usize, s.hi.0 as usize)
}

pub fn process_define_props(
    ctx: &mut ScriptCompileContext,
    node: &Expr,
    decl_id: Option<&Pat>,
    is_with_defaults: bool,
) -> Result<bool, String> {
    let call = match as_call(node, |n| n == DEFINE_PROPS) {
        Some(c) => c,
        None => return process_with_defaults(ctx, node, decl_id),
    };
    if ctx.has_define_props_call {
        return Err(ctx.error(&format!("duplicate {DEFINE_PROPS}() call")));
    }
    ctx.has_define_props_call = true;
    ctx.props_runtime_decl = call.args.first().map(|a| a.expr.clone());

    if let Some(decl) = &ctx.props_runtime_decl {
        for key in object_or_array_keys(decl) {
            if ctx.binding_metadata.get(&key).is_none() {
                ctx.set_binding(&key, BindingType::Props);
            }
        }
    }

    if let Some(tp) = &call.type_args {
        if ctx.props_runtime_decl.is_some() {
            return Err(ctx.error(&format!(
                "{DEFINE_PROPS}() cannot accept both type and non-type arguments at the same time. Use one or the other."
            )));
        }
        ctx.props_type_decl = tp.params.first().cloned();
    }

    if !is_with_defaults {
        if let Some(Pat::Object(o)) = decl_id {
            super::define_props_destructure::process_props_destructure(ctx, &o.clone())?;
        }
    }

    ctx.props_call = Some(span(call));
    ctx.props_decl = decl_id.cloned();
    Ok(true)
}

fn process_with_defaults(
    ctx: &mut ScriptCompileContext,
    node: &Expr,
    decl_id: Option<&Pat>,
) -> Result<bool, String> {
    let call = match as_call(node, |n| n == WITH_DEFAULTS) {
        Some(c) => c.clone(),
        None => return Ok(false),
    };
    let first = call.args.first().map(|a| (*a.expr).clone());
    let ok = match &first {
        Some(e) => process_define_props(ctx, e, decl_id, true)?,
        None => false,
    };
    if !ok {
        return Err(ctx.error(&format!(
            "{WITH_DEFAULTS}' first argument must be a {DEFINE_PROPS} call."
        )));
    }
    if ctx.props_runtime_decl.is_some() {
        return Err(ctx.error(&format!(
            "{WITH_DEFAULTS} can only be used with type-based {DEFINE_PROPS} declaration."
        )));
    }
    ctx.props_runtime_defaults = call.args.get(1).map(|a| a.expr.clone());
    if ctx.props_runtime_defaults.is_none() {
        return Err(ctx.error(&format!("The 2nd argument of {WITH_DEFAULTS} is required.")));
    }
    ctx.props_call = Some(span(&call));
    Ok(true)
}

pub fn process_define_emits(
    ctx: &mut ScriptCompileContext,
    node: &Expr,
    decl_id: Option<&Pat>,
) -> Result<bool, String> {
    let call = match as_call(node, |n| n == DEFINE_EMITS) {
        Some(c) => c,
        None => return Ok(false),
    };
    if ctx.has_define_emit_call {
        return Err(ctx.error(&format!("duplicate {DEFINE_EMITS}() call")));
    }
    ctx.has_define_emit_call = true;
    ctx.emits_runtime_decl = call.args.first().map(|a| a.expr.clone());
    if let Some(tp) = &call.type_args {
        if ctx.emits_runtime_decl.is_some() {
            return Err(ctx.error(&format!(
                "{DEFINE_EMITS}() cannot accept both type and non-type arguments at the same time. Use one or the other."
            )));
        }
        ctx.emits_type_decl = tp.params.first().cloned();
    }
    ctx.emit_decl = decl_id.cloned();
    Ok(true)
}

pub fn process_define_expose(ctx: &mut ScriptCompileContext, node: &Expr) -> Result<bool, String> {
    if as_call(node, |n| n == DEFINE_EXPOSE).is_none() {
        return Ok(false);
    }
    if ctx.has_define_expose_call {
        return Err(ctx.error(&format!("duplicate {DEFINE_EXPOSE}() call")));
    }
    ctx.has_define_expose_call = true;
    Ok(true)
}

pub fn process_define_options(ctx: &mut ScriptCompileContext, node: &Expr) -> Result<bool, String> {
    let call = match as_call(node, |n| n == DEFINE_OPTIONS) {
        Some(c) => c,
        None => return Ok(false),
    };
    if ctx.has_define_options_call {
        return Err(ctx.error(&format!("duplicate {DEFINE_OPTIONS}() call")));
    }
    if call.type_args.is_some() {
        return Err(ctx.error(&format!("{DEFINE_OPTIONS}() cannot accept type arguments")));
    }
    let arg0 = match call.args.first() {
        Some(a) => a.expr.clone(),
        None => return Ok(true),
    };
    ctx.has_define_options_call = true;
    let unwrapped = unwrap_ts_node(&arg0).clone();

    if let Expr::Object(o) = &unwrapped {
        for p in &o.props {
            if let PropOrSpread::Prop(prop) = p {
                let key = match &**prop {
                    Prop::KeyValue(kv) => match &kv.key {
                        PropName::Ident(i) => Some(i.sym.to_string()),
                        _ => None,
                    },
                    Prop::Method(m) => match &m.key {
                        PropName::Ident(i) => Some(i.sym.to_string()),
                        _ => None,
                    },
                    _ => None,
                };
                match key.as_deref() {
                    Some("props") => {
                        return Err(ctx.error(&format!(
                            "{DEFINE_OPTIONS}() cannot be used to declare props. Use {DEFINE_PROPS}() instead."
                        )));
                    }
                    Some("emits") => {
                        return Err(ctx.error(&format!(
                            "{DEFINE_OPTIONS}() cannot be used to declare emits. Use {DEFINE_EMITS}() instead."
                        )));
                    }
                    Some("expose") => {
                        return Err(ctx.error(&format!(
                            "{DEFINE_OPTIONS}() cannot be used to declare expose. Use {DEFINE_EXPOSE}() instead."
                        )));
                    }
                    Some("slots") => {
                        return Err(ctx.error(&format!(
                            "{DEFINE_OPTIONS}() cannot be used to declare slots. Use {DEFINE_SLOTS}() instead."
                        )));
                    }
                    _ => {}
                }
            }
        }
    }
    ctx.options_runtime_decl = Some(Box::new(unwrapped));
    Ok(true)
}

pub fn process_define_slots(
    ctx: &mut ScriptCompileContext,
    node: &Expr,
    decl_id: Option<&Pat>,
) -> Result<bool, String> {
    let call = match as_call(node, |n| n == DEFINE_SLOTS) {
        Some(c) => c.clone(),
        None => return Ok(false),
    };
    if ctx.has_define_slots_call {
        return Err(ctx.error(&format!("duplicate {DEFINE_SLOTS}() call")));
    }
    ctx.has_define_slots_call = true;
    if !call.args.is_empty() {
        return Err(ctx.error(&format!("{DEFINE_SLOTS}() cannot accept arguments")));
    }
    if decl_id.is_some() {
        let (s, e) = span(&call);
        let helper = ctx.helper("useSlots");
        let start = ctx.start_offset;
        ctx.s
            .overwrite(start + s, start + e, &format!("{helper}()"));
    }
    Ok(true)
}

pub fn process_define_model(
    ctx: &mut ScriptCompileContext,
    node: &Expr,
    decl_id: Option<&Pat>,
) -> Result<bool, String> {
    let call = match as_call(node, |n| n == DEFINE_MODEL) {
        Some(c) => c.clone(),
        None => return Ok(false),
    };
    ctx.has_define_model_call = true;

    let type_ann = call
        .type_args
        .as_ref()
        .and_then(|t| t.params.first().cloned());
    let arg0 = call.args.first().map(|a| unwrap_ts_node(&a.expr).clone());
    let has_name = match &arg0 {
        Some(Expr::Lit(Lit::Str(_))) => true,
        Some(Expr::Tpl(t)) => t.exprs.is_empty(),
        _ => false,
    };
    let (model_name, options) = if has_name {
        let name = match arg0.as_ref().unwrap() {
            Expr::Lit(Lit::Str(s)) => atom(&s.value),
            Expr::Tpl(t) => t
                .quasis
                .iter()
                .map(|q| q.cooked.as_ref().map(atom).unwrap_or_default())
                .collect(),
            _ => unreachable!(),
        };
        (name, call.args.get(1).map(|a| (*a.expr).clone()))
    } else {
        ("modelValue".to_string(), arg0.clone())
    };

    if ctx.model_decls.iter().any(|(n, _)| *n == model_name) {
        return Err(ctx.error(&format!(
            "duplicate model name {}",
            serde_json::to_string(&model_name).unwrap()
        )));
    }

    let mut options_string = options
        .as_ref()
        .map(|o| {
            let (s, e) = span(o);
            ctx.get_string(s, e, true)
        });
    let mut options_removed = options.is_none();
    let mut runtime_option_spans: Vec<(usize, usize)> = Vec::new();

    if let Some(Expr::Object(o)) = &options {
        let simple = !o.props.iter().any(|p| match p {
            PropOrSpread::Spread(_) => true,
            PropOrSpread::Prop(prop) => matches!(
                &**prop,
                Prop::KeyValue(KeyValueProp {
                    key: PropName::Computed(_),
                    ..
                })
            ),
        });
        if simple {
            let (opt_start, opt_end) = span(o);
            let mut removed = 0usize;
            let mut opts_str = options_string.clone().unwrap_or_default();
            for i in (0..o.props.len()).rev() {
                let p = &o.props[i];
                let next = o.props.get(i + 1);
                let (start, _) = match p {
                    PropOrSpread::Prop(prop) => span(&**prop),
                    PropOrSpread::Spread(s) => span(s),
                };
                let end = match next {
                    Some(PropOrSpread::Prop(n)) => span(&**n).0,
                    Some(PropOrSpread::Spread(n)) => span(n).0,
                    None => opt_end - 1,
                };
                let key_name = match p {
                    PropOrSpread::Prop(prop) => match &**prop {
                        Prop::KeyValue(kv) => resolve_object_key(&kv.key),
                        Prop::Method(m) => resolve_object_key(&m.key),
                        Prop::Getter(g) => resolve_object_key(&g.key),
                        Prop::Setter(s) => resolve_object_key(&s.key),
                        Prop::Shorthand(i) => Some(i.sym.to_string()),
                        Prop::Assign(_) => None,
                    },
                    _ => None,
                };
                if matches!(key_name.as_deref(), Some("get") | Some("set")) {
                    opts_str = format!(
                        "{}{}",
                        &opts_str[..start - opt_start],
                        &opts_str[end - opt_start..]
                    );
                } else {
                    removed += 1;
                    let so = ctx.start_offset;
                    ctx.s.remove(so + start, so + end);
                    runtime_option_spans.push((start, end));
                }
            }
            options_string = Some(opts_str);
            if removed == o.props.len() {
                options_removed = true;
                let so = ctx.start_offset;
                let from = if has_name {
                    span(arg0.as_ref().unwrap()).1
                } else {
                    opt_start
                };
                ctx.s.remove(so + from, so + opt_end);
            }
        }
    }

    let identifier = match decl_id {
        Some(Pat::Ident(i)) => Some(i.id.sym.to_string()),
        _ => None,
    };
    ctx.model_decls.push((
        model_name.clone(),
        ModelDecl {
            type_ann,
            options: options_string,
            identifier,
            runtime_option_spans,
        },
    ));
    ctx.set_binding(&model_name, BindingType::Props);

    let callee_span = match &call.callee {
        Callee::Expr(e) => span(&**e),
        _ => (0, 0),
    };
    let helper = ctx.helper("useModel");
    let so = ctx.start_offset;
    ctx.s
        .overwrite(so + callee_span.0, so + callee_span.1, &helper);
    let insert_at = if !call.args.is_empty() {
        span(&*call.args[0].expr).0
    } else {
        span(&call).1 - 1
    };
    let injected = format!(
        "__props, {}",
        if has_name {
            String::new()
        } else {
            format!(
                "{}{}",
                serde_json::to_string(&model_name).unwrap(),
                if options_removed { "" } else { ", " }
            )
        }
    );
    ctx.s.append_left(so + insert_at, &injected);
    Ok(true)
}

pub fn gen_model_props(ctx: &mut ScriptCompileContext) -> Option<String> {
    if !ctx.has_define_model_call {
        return None;
    }
    let is_prod = ctx.options.is_prod;
    let mut decl_out = String::new();
    let decls = ctx.model_decls.clone();
    for (name, model) in decls {
        let mut skip_check = false;
        let mut codegen_options = String::new();
        let runtime_types = model
            .type_ann
            .as_ref()
            .map(|t| infer_runtime_type(ctx, t));
        if let Some(mut types) = runtime_types {
            let has_boolean = types.iter().any(|t| t == "Boolean");
            let has_function = types.iter().any(|t| t == "Function");
            let has_unknown = types.iter().any(|t| t == UNKNOWN_TYPE);
            if has_unknown {
                if has_boolean || has_function {
                    types.retain(|t| t != UNKNOWN_TYPE);
                    skip_check = true;
                } else {
                    types = vec!["null".to_string()];
                }
            }
            if !is_prod {
                codegen_options = format!(
                    "type: {}{}",
                    to_runtime_type_string(&types),
                    if skip_check { ", skipCheck: true" } else { "" }
                );
            } else if has_boolean || (model.options.is_some() && has_function) {
                codegen_options = format!("type: {}", to_runtime_type_string(&types));
            }
        }

        let runtime_options = model.options.clone().filter(|s| !s.is_empty());
        let decl = match (!codegen_options.is_empty(), &runtime_options) {
            (true, Some(ro)) => {
                if ctx.is_ts {
                    format!("{{ {codegen_options}, ...{ro} }}")
                } else {
                    format!("Object.assign({{ {codegen_options} }}, {ro})")
                }
            }
            (true, None) => format!("{{ {codegen_options} }}"),
            (false, Some(ro)) => ro.clone(),
            (false, None) => "{}".to_string(),
        };
        decl_out.push_str(&format!(
            "\n    {}: {decl},",
            serde_json::to_string(&name).unwrap()
        ));
        let modifier_prop_name = if name == "modelValue" {
            "modelModifiers".to_string()
        } else {
            format!("{name}Modifiers")
        };
        decl_out.push_str(&format!(
            "\n    {}: {{}},",
            serde_json::to_string(&modifier_prop_name).unwrap()
        ));
    }
    Some(format!("{{{decl_out}\n  }}"))
}

pub fn gen_runtime_emits(ctx: &mut ScriptCompileContext) -> Option<String> {
    let mut emits_decl = String::new();
    if let Some(decl) = ctx.emits_runtime_decl.clone() {
        let (s, e) = span(&decl);
        emits_decl = ctx.get_string(s, e, true).trim().to_string();
    } else if ctx.emits_type_decl.is_some() {
        let names = extract_runtime_emits(ctx);
        emits_decl = if names.is_empty() {
            String::new()
        } else {
            format!(
                "[{}]",
                names
                    .iter()
                    .map(|k| serde_json::to_string(k).unwrap())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
    }
    if ctx.has_define_model_call {
        let model_emits = format!(
            "[{}]",
            ctx.model_decls
                .iter()
                .map(|(n, _)| serde_json::to_string(&format!("update:{n}")).unwrap())
                .collect::<Vec<_>>()
                .join(", ")
        );
        emits_decl = if !emits_decl.is_empty() {
            let helper = ctx.helper("mergeModels");
            format!("/*@__PURE__*/{helper}({emits_decl}, {model_emits})")
        } else {
            model_emits
        };
    }
    if emits_decl.is_empty() {
        None
    } else {
        Some(emits_decl)
    }
}

pub fn extract_runtime_emits(ctx: &mut ScriptCompileContext) -> Vec<String> {
    let node = match ctx.emits_type_decl.clone() {
        Some(n) => n,
        None => return Vec::new(),
    };
    let mut emits: Vec<String> = Vec::new();
    if let TsType::TsFnOrConstructorType(TsFnOrConstructorType::TsFnType(f)) = &*node {
        extract_event_names(ctx, f.params.first(), &mut emits);
        return emits;
    }
    let resolved = match resolve_type_elements(ctx, &node) {
        Ok(r) => r,
        Err(e) => {
            ctx.errors.push(e);
            return emits;
        }
    };
    let has_property = !resolved.props.is_empty();
    for (key, _) in &resolved.props {
        if !emits.contains(key) {
            emits.push(key.clone());
        }
    }
    if !resolved.calls.is_empty() {
        if has_property {
            ctx.errors.push(ctx.error(
                "defineEmits() type cannot mixed call signature and property syntax.",
            ));
        }
        for call in resolved.calls.clone() {
            extract_event_names(ctx, call.params.first(), &mut emits);
        }
    }
    emits
}

fn extract_event_names(
    ctx: &mut ScriptCompileContext,
    param: Option<&TsFnParam>,
    emits: &mut Vec<String>,
) {
    let ident = match param {
        Some(TsFnParam::Ident(i)) => i,
        _ => return,
    };
    let ann = match &ident.type_ann {
        Some(a) => a.type_ann.clone(),
        None => return,
    };
    for t in resolve_union_type(ctx, &ann) {
        if let TsType::TsLitType(l) = &*t {
            match &l.lit {
                TsLit::Str(s) => {
                    let v = atom(&s.value);
                    if !emits.contains(&v) {
                        emits.push(v);
                    }
                }
                TsLit::Number(n) => {
                    let v = crate::core::js_value::number_to_string(n.value);
                    if !emits.contains(&v) {
                        emits.push(v);
                    }
                }
                TsLit::Bool(b) => {
                    let v = b.value.to_string();
                    if !emits.contains(&v) {
                        emits.push(v);
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn resolve_union_type(
    ctx: &mut ScriptCompileContext,
    node: &TsType,
) -> Vec<Box<TsType>> {
    match node {
        TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(u)) => {
            u.types.clone()
        }
        TsType::TsTypeRef(r) => {
            if let TsEntityName::Ident(i) = &r.type_name {
                if let Some(super::context::TypeDecl::Alias(a)) =
                    ctx.type_decls.get(&i.sym.to_string()).cloned()
                {
                    return resolve_union_type(ctx, &a.type_ann);
                }
            }
            vec![Box::new(node.clone())]
        }
        _ => vec![Box::new(node.clone())],
    }
}

pub fn gen_runtime_props(ctx: &mut ScriptCompileContext) -> Option<String> {
    let mut props_decls: Option<String> = None;
    if let Some(decl) = ctx.props_runtime_decl.clone() {
        let (s, e) = span(&decl);
        let mut decls = ctx.get_string(s, e, true).trim().to_string();
        if ctx.props_destructure_decl.is_some() {
            let keys: Vec<String> = ctx
                .props_destructured_bindings
                .iter()
                .map(|(k, _)| k.clone())
                .collect();
            let mut defaults = Vec::new();
            for key in keys {
                if let Some((value_string, need_skip_factory)) =
                    super::define_props_destructure::gen_destructured_default_value(
                        ctx, &key, None,
                    )
                {
                    let final_key = get_escaped_prop_name(&key);
                    defaults.push(format!(
                        "{final_key}: {value_string}{}",
                        if need_skip_factory {
                            format!(", __skip_{final_key}: true")
                        } else {
                            String::new()
                        }
                    ));
                }
            }
            if !defaults.is_empty() {
                let helper = ctx.helper("mergeDefaults");
                decls = format!(
                    "/*@__PURE__*/{helper}({decls}, {{\n  {}\n}})",
                    defaults.join(",\n  ")
                );
            }
        }
        props_decls = Some(decls);
    } else if ctx.props_type_decl.is_some() {
        props_decls = extract_runtime_props(ctx);
    }
    let models_decls = gen_model_props(ctx);
    match (props_decls, models_decls) {
        (Some(p), Some(m)) => {
            let helper = ctx.helper("mergeModels");
            Some(format!("/*@__PURE__*/{helper}({p}, {m})"))
        }
        (p, m) => m.or(p),
    }
}

struct PropTypeData {
    key: String,
    ty: Vec<String>,
    required: bool,
    skip_check: bool,
}

pub fn extract_runtime_props(ctx: &mut ScriptCompileContext) -> Option<String> {
    let type_decl = ctx.props_type_decl.clone()?;
    let resolved = match resolve_type_elements(ctx, &type_decl) {
        Ok(r) => r,
        Err(e) => {
            ctx.errors.push(e);
            return None;
        }
    };
    let mut props: Vec<PropTypeData> = Vec::new();
    for (key, sig) in &resolved.props {
        let mut ty = infer_runtime_type_of_prop(ctx, sig);
        let mut skip_check = false;
        if ty.iter().any(|t| t == UNKNOWN_TYPE) {
            if ty.iter().any(|t| t == "Boolean" || t == "Function") {
                ty.retain(|t| t != UNKNOWN_TYPE);
                skip_check = true;
            } else {
                ty = vec!["null".to_string()];
            }
        }
        props.push(PropTypeData {
            key: key.clone(),
            required: !sig.optional,
            ty,
            skip_check,
        });
    }
    if props.is_empty() {
        return None;
    }

    let has_static_defaults = has_static_with_defaults(ctx);
    let mut prop_strings = Vec::new();
    for prop in &props {
        prop_strings.push(gen_runtime_prop_from_type(ctx, prop, has_static_defaults));
        if ctx.binding_metadata.get(&prop.key).is_none() {
            ctx.set_binding(&prop.key, BindingType::Props);
        }
    }

    let mut props_decls = format!("{{\n    {}\n  }}", prop_strings.join(",\n    "));
    if ctx.props_runtime_defaults.is_some() && !has_static_defaults {
        let defaults = ctx.props_runtime_defaults.clone().unwrap();
        let (s, e) = span(&defaults);
        let defaults_str = ctx.get_string(s, e, true);
        let helper = ctx.helper("mergeDefaults");
        props_decls = format!("/*@__PURE__*/{helper}({props_decls}, {defaults_str})");
    }
    Some(props_decls)
}

fn has_static_with_defaults(ctx: &ScriptCompileContext) -> bool {
    match &ctx.props_runtime_defaults {
        Some(d) => match &**d {
            Expr::Object(o) => o.props.iter().all(|p| match p {
                PropOrSpread::Spread(_) => false,
                PropOrSpread::Prop(prop) => !matches!(
                    &**prop,
                    Prop::KeyValue(KeyValueProp {
                        key: PropName::Computed(_),
                        ..
                    })
                ),
            }),
            _ => false,
        },
        None => false,
    }
}

fn gen_runtime_prop_from_type(
    ctx: &mut ScriptCompileContext,
    prop: &PropTypeData,
    has_static_defaults: bool,
) -> String {
    let mut default_string: Option<String> = None;
    if let Some((value_string, need_skip_factory)) =
        super::define_props_destructure::gen_destructured_default_value(
            ctx,
            &prop.key,
            Some(&prop.ty),
        )
    {
        default_string = Some(format!(
            "default: {value_string}{}",
            if need_skip_factory { ", skipFactory: true" } else { "" }
        ));
    } else if has_static_defaults {
        if let Some(defaults) = ctx.props_runtime_defaults.clone() {
            if let Expr::Object(o) = &*defaults {
                for p in &o.props {
                    let PropOrSpread::Prop(pr) = p else { continue };
                    match &**pr {
                        Prop::KeyValue(kv) => {
                            if resolve_object_key(&kv.key).as_deref() == Some(prop.key.as_str()) {
                                let (s, e) = span(&*kv.value);
                                default_string =
                                    Some(format!("default: {}", ctx.get_string(s, e, true)));
                            }
                        }
                        Prop::Method(m) => {
                            if resolve_object_key(&m.key).as_deref() == Some(prop.key.as_str()) {
                                let params_string = if !m.function.params.is_empty() {
                                    let start = span(&m.function.params[0].pat).0;
                                    let end =
                                        span(&m.function.params.last().unwrap().pat).1;
                                    ctx.get_string(start, end, true)
                                } else {
                                    String::new()
                                };
                                let body = m
                                    .function
                                    .body
                                    .as_ref()
                                    .map(|b| {
                                        let (s, e) = span(b);
                                        ctx.get_string(s, e, true)
                                    })
                                    .unwrap_or_default();
                                default_string = Some(format!(
                                    "{}default({params_string}) {body}",
                                    if m.function.is_async { "async " } else { "" }
                                ));
                            }
                        }
                        Prop::Getter(g) => {
                            if resolve_object_key(&g.key).as_deref() == Some(prop.key.as_str()) {
                                let body = g
                                    .function
                                    .body
                                    .as_ref()
                                    .map(|b| {
                                        let (s, e) = span(b);
                                        ctx.get_string(s, e, true)
                                    })
                                    .unwrap_or_default();
                                default_string = Some(format!("get default() {body}"));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let final_key = get_escaped_prop_name(&prop.key);
    if !ctx.options.is_prod {
        format!(
            "{final_key}: {{ {} }}",
            concat_strings(vec![
                Some(format!("type: {}", to_runtime_type_string(&prop.ty))),
                Some(format!("required: {}", prop.required)),
                if prop.skip_check {
                    Some("skipCheck: true".to_string())
                } else {
                    None
                },
                default_string,
            ])
        )
    } else if prop.ty.iter().any(|el| {
        el == "Boolean" || ((!has_static_defaults || default_string.is_some()) && el == "Function")
    }) {
        format!(
            "{final_key}: {{ {} }}",
            concat_strings(vec![
                Some(format!("type: {}", to_runtime_type_string(&prop.ty))),
                default_string,
            ])
        )
    } else if ctx.is_ce {
        match default_string {
            Some(d) => format!(
                "{final_key}: {{ {d}, type: {} }}",
                to_runtime_type_string(&prop.ty)
            ),
            None => format!("{final_key}: {{type: {}}}", to_runtime_type_string(&prop.ty)),
        }
    } else {
        match default_string {
            Some(d) => format!("{final_key}: {{ {d} }}"),
            None => format!("{final_key}: {{}}"),
        }
    }
}
