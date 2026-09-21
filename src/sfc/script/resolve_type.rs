//! Port of the parts of `compiler-sfc/src/script/resolveType.ts` that the SFC
//! macros need: local type resolution (interfaces, aliases, literals, unions,
//! intersections and the supported built-in utility types) plus runtime type
//! inference. Cross-file type imports are not resolved (they require a file
//! system, which the SFC pipeline we target does not provide).

use swc_core::ecma::ast::*;

use super::context::{ScriptCompileContext, TypeDecl};
use super::utils::{UNKNOWN_TYPE, atom};

#[derive(Debug, Clone)]
pub struct PropSig {
    pub key: String,
    pub optional: bool,
    /// the declared types; more than one when merged from a union/intersection
    pub types: Vec<Option<Box<TsType>>>,
    pub merge_union: bool,
    pub is_method: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedElements {
    pub props: Vec<(String, PropSig)>,
    pub calls: Vec<TsFnType>,
}

fn key_of(key: &Expr, computed: bool) -> Option<String> {
    if computed {
        if let Expr::Tpl(t) = key {
            if t.exprs.is_empty() {
                return Some(
                    t.quasis
                        .iter()
                        .map(|q| {
                            q.cooked
                                .as_ref()
                                .map(|c| atom(c))
                                .unwrap_or_else(|| q.raw.to_string())
                        })
                        .collect(),
                );
            }
        }
        return None;
    }
    match key {
        Expr::Ident(i) => Some(i.sym.to_string()),
        Expr::Lit(Lit::Str(s)) => Some(atom(&s.value)),
        Expr::Lit(Lit::Num(n)) => Some(crate::core::js_value::number_to_string(n.value)),
        _ => None,
    }
}

pub fn resolve_type_elements(
    ctx: &mut ScriptCompileContext,
    node: &TsType,
) -> Result<ResolvedElements, String> {
    match node {
        TsType::TsTypeLit(lit) => type_elements_to_map(ctx, &lit.members),
        TsType::TsParenthesizedType(p) => resolve_type_elements(ctx, &p.type_ann),
        TsType::TsFnOrConstructorType(TsFnOrConstructorType::TsFnType(f)) => {
            Ok(ResolvedElements {
                props: Vec::new(),
                calls: vec![f.clone()],
            })
        }
        TsType::TsUnionOrIntersectionType(u) => {
            let (types, is_union) = match u {
                TsUnionOrIntersectionType::TsUnionType(t) => (&t.types, true),
                TsUnionOrIntersectionType::TsIntersectionType(t) => (&t.types, false),
            };
            let mut maps = Vec::new();
            for t in types {
                maps.push(resolve_type_elements(ctx, t)?);
            }
            Ok(merge_elements(maps, is_union))
        }
        TsType::TsTypeRef(r) => resolve_type_ref(ctx, r),
        _ => Err(ctx.error("Unresolvable type reference or unsupported built-in utility type")),
    }
}

fn type_ref_name(r: &TsTypeRef) -> Option<String> {
    match &r.type_name {
        TsEntityName::Ident(i) => Some(i.sym.to_string()),
        TsEntityName::TsQualifiedName(q) => match &q.right {
            i => Some(i.sym.to_string()),
        },
    }
}

fn resolve_type_ref(
    ctx: &mut ScriptCompileContext,
    r: &TsTypeRef,
) -> Result<ResolvedElements, String> {
    let name = match type_ref_name(r) {
        Some(n) => n,
        None => {
            return Err(
                ctx.error("Unresolvable type reference or unsupported built-in utility type")
            );
        }
    };

    if let Some(decl) = ctx.type_decls.get(&name).cloned() {
        return match decl {
            TypeDecl::Interface(i) => resolve_interface_members(ctx, &i),
            TypeDecl::Alias(a) => resolve_type_elements(ctx, &a.type_ann),
            TypeDecl::Enum(_) => Err(ctx.error(
                "Unresolvable type reference or unsupported built-in utility type",
            )),
        };
    }

    // built-in utility types
    let params: Vec<Box<TsType>> = r
        .type_params
        .as_ref()
        .map(|p| p.params.clone())
        .unwrap_or_default();
    match name.as_str() {
        "Partial" | "Required" | "Readonly" if !params.is_empty() => {
            let mut resolved = resolve_type_elements(ctx, &params[0])?;
            if name == "Partial" {
                for (_, p) in resolved.props.iter_mut() {
                    p.optional = true;
                }
            } else if name == "Required" {
                for (_, p) in resolved.props.iter_mut() {
                    p.optional = false;
                }
            }
            Ok(resolved)
        }
        "Pick" if params.len() >= 2 => {
            let resolved = resolve_type_elements(ctx, &params[0])?;
            let picked = resolve_string_type(ctx, &params[1]);
            Ok(ResolvedElements {
                props: resolved
                    .props
                    .into_iter()
                    .filter(|(k, _)| picked.contains(k))
                    .collect(),
                calls: Vec::new(),
            })
        }
        "Omit" if params.len() >= 2 => {
            let resolved = resolve_type_elements(ctx, &params[0])?;
            let omitted = resolve_string_type(ctx, &params[1]);
            Ok(ResolvedElements {
                props: resolved
                    .props
                    .into_iter()
                    .filter(|(k, _)| !omitted.contains(k))
                    .collect(),
                calls: Vec::new(),
            })
        }
        _ => Err(ctx.error("Unresolvable type reference or unsupported built-in utility type")),
    }
}

/// `resolveStringType` for the key lists of `Pick` / `Omit`
fn resolve_string_type(ctx: &mut ScriptCompileContext, node: &TsType) -> Vec<String> {
    match node {
        TsType::TsLitType(l) => match &l.lit {
            TsLit::Str(s) => vec![atom(&s.value)],
            _ => Vec::new(),
        },
        TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(u)) => u
            .types
            .iter()
            .flat_map(|t| resolve_string_type(ctx, t))
            .collect(),
        TsType::TsParenthesizedType(p) => resolve_string_type(ctx, &p.type_ann),
        TsType::TsTypeRef(r) => {
            if let Some(name) = type_ref_name(r) {
                if let Some(TypeDecl::Alias(a)) = ctx.type_decls.get(&name).cloned() {
                    return resolve_string_type(ctx, &a.type_ann);
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn resolve_interface_members(
    ctx: &mut ScriptCompileContext,
    node: &TsInterfaceDecl,
) -> Result<ResolvedElements, String> {
    let mut base = type_elements_to_map(ctx, &node.body.body)?;
    for ext in &node.extends {
        if let Expr::Ident(i) = &*ext.expr {
            let name = i.sym.to_string();
            if let Some(decl) = ctx.type_decls.get(&name).cloned() {
                let resolved = match decl {
                    TypeDecl::Interface(i) => resolve_interface_members(ctx, &i)?,
                    TypeDecl::Alias(a) => resolve_type_elements(ctx, &a.type_ann)?,
                    TypeDecl::Enum(_) => continue,
                };
                for (k, v) in resolved.props {
                    if !base.props.iter().any(|(bk, _)| *bk == k) {
                        base.props.push((k, v));
                    }
                }
            }
        }
    }
    Ok(base)
}

fn type_elements_to_map(
    ctx: &mut ScriptCompileContext,
    members: &[TsTypeElement],
) -> Result<ResolvedElements, String> {
    let mut res = ResolvedElements::default();
    for e in members {
        match e {
            TsTypeElement::TsPropertySignature(p) => {
                let name = match key_of(&p.key, p.computed) {
                    Some(n) => n,
                    None => {
                        return Err(
                            ctx.error("Unsupported computed key in type referenced by a macro")
                        );
                    }
                };
                res.props.push((
                    name.clone(),
                    PropSig {
                        key: name,
                        optional: p.optional,
                        types: vec![p.type_ann.as_ref().map(|t| t.type_ann.clone())],
                        merge_union: false,
                        is_method: false,
                    },
                ));
            }
            TsTypeElement::TsMethodSignature(m) => {
                let name = match key_of(&m.key, m.computed) {
                    Some(n) => n,
                    None => {
                        return Err(
                            ctx.error("Unsupported computed key in type referenced by a macro")
                        );
                    }
                };
                res.props.push((
                    name.clone(),
                    PropSig {
                        key: name,
                        optional: m.optional,
                        types: vec![None],
                        merge_union: false,
                        is_method: true,
                    },
                ));
            }
            TsTypeElement::TsCallSignatureDecl(c) => {
                res.calls.push(TsFnType {
                    span: c.span,
                    params: c.params.clone(),
                    type_params: c.type_params.clone(),
                    type_ann: c
                        .type_ann
                        .clone()
                        .unwrap_or_else(|| {
                            Box::new(TsTypeAnn {
                                span: c.span,
                                type_ann: Box::new(TsType::TsKeywordType(TsKeywordType {
                                    span: c.span,
                                    kind: TsKeywordTypeKind::TsAnyKeyword,
                                })),
                            })
                        }),
                });
            }
            _ => {}
        }
    }
    Ok(res)
}

fn merge_elements(maps: Vec<ResolvedElements>, is_union: bool) -> ResolvedElements {
    if maps.len() == 1 {
        return maps.into_iter().next().unwrap();
    }
    let mut res = ResolvedElements::default();
    for m in maps {
        for (key, prop) in m.props {
            match res.props.iter_mut().find(|(k, _)| *k == key) {
                Some((_, existing)) => {
                    let optional = existing.optional || prop.optional;
                    existing.types.extend(prop.types);
                    existing.merge_union = is_union;
                    existing.optional = optional;
                }
                None => res.props.push((key, prop)),
            }
        }
        res.calls.extend(m.calls);
    }
    res
}

pub fn infer_runtime_type_of_prop(ctx: &mut ScriptCompileContext, prop: &PropSig) -> Vec<String> {
    if prop.is_method {
        return vec!["Function".to_string()];
    }
    let mut types: Vec<String> = Vec::new();
    for t in prop.types.clone() {
        match t {
            Some(t) => {
                for x in infer_runtime_type(ctx, &t) {
                    if !types.contains(&x) {
                        types.push(x);
                    }
                }
            }
            None => {
                if !types.contains(&UNKNOWN_TYPE.to_string()) {
                    types.push(UNKNOWN_TYPE.to_string());
                }
            }
        }
    }
    if types.is_empty() {
        types.push(UNKNOWN_TYPE.to_string());
    }
    types
}

pub fn infer_runtime_type(ctx: &mut ScriptCompileContext, node: &TsType) -> Vec<String> {
    match node {
        TsType::TsKeywordType(k) => match k.kind {
            TsKeywordTypeKind::TsStringKeyword => vec!["String".into()],
            TsKeywordTypeKind::TsNumberKeyword => vec!["Number".into()],
            TsKeywordTypeKind::TsBooleanKeyword => vec!["Boolean".into()],
            TsKeywordTypeKind::TsObjectKeyword => vec!["Object".into()],
            TsKeywordTypeKind::TsNullKeyword => vec!["null".into()],
            _ => vec![UNKNOWN_TYPE.into()],
        },
        TsType::TsTypeLit(lit) => {
            let mut types: Vec<String> = Vec::new();
            for m in &lit.members {
                let t = match m {
                    TsTypeElement::TsCallSignatureDecl(_)
                    | TsTypeElement::TsConstructSignatureDecl(_) => "Function",
                    _ => "Object",
                };
                if !types.iter().any(|x| x == t) {
                    types.push(t.to_string());
                }
            }
            if types.is_empty() {
                vec!["Object".into()]
            } else {
                types
            }
        }
        TsType::TsFnOrConstructorType(_) => vec!["Function".into()],
        TsType::TsArrayType(_) | TsType::TsTupleType(_) => vec!["Array".into()],
        TsType::TsLitType(l) => match &l.lit {
            TsLit::Str(_) => vec!["String".into()],
            TsLit::Bool(_) => vec!["Boolean".into()],
            TsLit::Number(_) | TsLit::BigInt(_) => vec!["Number".into()],
            _ => vec![UNKNOWN_TYPE.into()],
        },
        TsType::TsParenthesizedType(p) => infer_runtime_type(ctx, &p.type_ann),
        TsType::TsUnionOrIntersectionType(u) => match u {
            TsUnionOrIntersectionType::TsUnionType(t) => flatten_types(ctx, &t.types),
            TsUnionOrIntersectionType::TsIntersectionType(t) => flatten_types(ctx, &t.types)
                .into_iter()
                .filter(|x| x != UNKNOWN_TYPE)
                .collect(),
        },
        TsType::TsTypeRef(r) => infer_type_ref(ctx, r),
        _ => vec![UNKNOWN_TYPE.into()],
    }
}

fn infer_type_ref(ctx: &mut ScriptCompileContext, r: &TsTypeRef) -> Vec<String> {
    let name = match type_ref_name(r) {
        Some(n) => n,
        None => return vec![UNKNOWN_TYPE.into()],
    };
    if let Some(decl) = ctx.type_decls.get(&name).cloned() {
        return match decl {
            TypeDecl::Alias(a) => {
                if matches!(
                    &*a.type_ann,
                    TsType::TsFnOrConstructorType(TsFnOrConstructorType::TsFnType(_))
                ) {
                    vec!["Function".into()]
                } else {
                    infer_runtime_type(ctx, &a.type_ann)
                }
            }
            TypeDecl::Interface(i) => {
                let mut types: Vec<String> = Vec::new();
                for m in &i.body.body {
                    let t = match m {
                        TsTypeElement::TsCallSignatureDecl(_)
                        | TsTypeElement::TsConstructSignatureDecl(_) => "Function",
                        _ => "Object",
                    };
                    if !types.iter().any(|x| x == t) {
                        types.push(t.to_string());
                    }
                }
                if types.is_empty() {
                    vec!["Object".into()]
                } else {
                    types
                }
            }
            TypeDecl::Enum(_) => vec![UNKNOWN_TYPE.into()],
        };
    }
    let params: Vec<Box<TsType>> = r
        .type_params
        .as_ref()
        .map(|p| p.params.clone())
        .unwrap_or_default();
    match name.as_str() {
        "Array" | "Function" | "Object" | "Set" | "Map" | "WeakSet" | "WeakMap" | "Date"
        | "Promise" | "Error" => vec![name],
        "Partial" | "Required" | "Readonly" | "Record" | "Pick" | "Omit" | "InstanceType" => {
            vec!["Object".into()]
        }
        "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" => vec!["String".into()],
        "Parameters" | "ConstructorParameters" | "ReadonlyArray" => vec!["Array".into()],
        "ReadonlyMap" => vec!["Map".into()],
        "ReadonlySet" => vec!["Set".into()],
        "Ref" | "ShallowRef" | "ComputedRef" | "WritableComputedRef" => vec!["Object".into()],
        "MaybeRef" | "MaybeRefOrGetter" => {
            let mut types = vec!["Object".to_string()];
            if name == "MaybeRefOrGetter" {
                types.push("Function".into());
            }
            if let Some(p) = params.first() {
                for t in infer_runtime_type(ctx, p) {
                    if !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            types
        }
        "NonNullable" if !params.is_empty() => infer_runtime_type(ctx, &params[0])
            .into_iter()
            .filter(|t| t != "null")
            .collect(),
        "Extract" if params.len() >= 2 => infer_runtime_type(ctx, &params[1]),
        "Exclude" | "OmitThisParameter" if !params.is_empty() => {
            infer_runtime_type(ctx, &params[0])
        }
        _ => vec![UNKNOWN_TYPE.into()],
    }
}

fn flatten_types(ctx: &mut ScriptCompileContext, types: &[Box<TsType>]) -> Vec<String> {
    if types.len() == 1 {
        return infer_runtime_type(ctx, &types[0]);
    }
    let mut out: Vec<String> = Vec::new();
    for t in types {
        for x in infer_runtime_type(ctx, t) {
            if !out.contains(&x) {
                out.push(x);
            }
        }
    }
    if out.len() > 1 {
        out.retain(|t| t != UNKNOWN_TYPE);
        if out.is_empty() {
            out.push(UNKNOWN_TYPE.into());
        }
    }
    out
}

/// collects the local type declarations a macro's type argument may reference
pub fn collect_type_decls(ctx: &mut ScriptCompileContext) {
    let mut decls = Vec::new();
    for module in [ctx.script_ast.as_ref(), ctx.script_setup_ast.as_ref()]
        .into_iter()
        .flatten()
    {
        for item in &module.body {
            let decl = match item {
                ModuleItem::Stmt(Stmt::Decl(d)) => Some(d),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(e)) => Some(&e.decl),
                _ => None,
            };
            match decl {
                Some(Decl::TsInterface(i)) => {
                    decls.push((i.id.sym.to_string(), TypeDecl::Interface(i.clone())))
                }
                Some(Decl::TsTypeAlias(a)) => {
                    decls.push((a.id.sym.to_string(), TypeDecl::Alias(a.clone())))
                }
                Some(Decl::TsEnum(e)) => {
                    decls.push((e.id.sym.to_string(), TypeDecl::Enum(e.clone())))
                }
                _ => {}
            }
        }
    }
    for (k, v) in decls {
        ctx.type_decls.insert(k, v);
    }
}
