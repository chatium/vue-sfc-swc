//! Port of `compiler-sfc/src/compileScript.ts`.

use std::collections::HashSet;

use swc_core::common::Spanned;
use swc_core::ecma::ast::*;

use crate::core::ast::Arena;
use crate::core::options::{BindingMetadata, BindingType};

use super::parse::{SfcBlock, SfcDescriptor};
use super::script::context::{
    ImportBinding, ScriptCompileContext, ScriptCompileOptions, utf16_to_byte,
};
use super::script::defines::*;
use super::script::import_usage_check::analyze_template;
use super::script::normal_script::{NORMAL_SCRIPT_DEFAULT_VAR, process_normal_script};
use super::script::resolve_type::collect_type_decls;
use super::script::utils::*;

pub struct ScriptBlockResult {
    pub block: SfcBlock,
    pub content: String,
    pub bindings: BindingMetadata,
    pub imports: Vec<(String, ImportBinding)>,
}

fn sp(e: &impl Spanned) -> (usize, usize) {
    let s = e.span();
    (s.lo.0 as usize, s.hi.0 as usize)
}

fn is_static_node(node: &Expr) -> bool {
    let node = unwrap_ts_node(node);
    match node {
        Expr::Unary(u) => is_static_node(&u.arg),
        Expr::Bin(b) => is_static_node(&b.left) && is_static_node(&b.right),
        Expr::Cond(c) => {
            is_static_node(&c.test) && is_static_node(&c.cons) && is_static_node(&c.alt)
        }
        Expr::Seq(s) => s.exprs.iter().all(|e| is_static_node(e)),
        Expr::Tpl(t) => t.exprs.iter().all(|e| is_static_node(e)),
        Expr::Paren(p) => is_static_node(&p.expr),
        // the literal kinds `isStaticNode` lists: a regex is not one of them
        Expr::Lit(l) => !matches!(l, Lit::Regex(_) | Lit::JSXText(_)),
        _ => false,
    }
}

fn can_never_be_ref(node: &Expr, user_reactive_import: Option<&str>) -> bool {
    if let Some(r) = user_reactive_import {
        if is_call_of(Some(node), |n| n == r) {
            return true;
        }
    }
    match node {
        // Babel splits `&&`, `||` and `??` out into `LogicalExpression`,
        // which this list does not cover
        Expr::Bin(b) => !matches!(
            b.op,
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
        ),
        Expr::Unary(_)
        | Expr::Array(_)
        | Expr::Object(_)
        | Expr::Fn(_)
        | Expr::Arrow(_)
        | Expr::Update(_)
        | Expr::Class(_)
        | Expr::TaggedTpl(_) => true,
        Expr::Seq(s) => s
            .exprs
            .last()
            .map(|e| can_never_be_ref(e, user_reactive_import))
            .unwrap_or(false),
        other => is_literal_node(other),
    }
}

struct Bindings(Vec<(String, BindingType)>);

impl Bindings {
    fn new() -> Self {
        Bindings(Vec::new())
    }
    fn set(&mut self, name: &str, ty: BindingType) {
        match self.0.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = ty,
            None => self.0.push((name.to_string(), ty)),
        }
    }
}

fn walk_pattern(pat: &Pat, bindings: &mut Bindings, is_const: bool, is_define_call: bool) {
    match pat {
        Pat::Ident(i) => {
            let ty = if is_define_call {
                BindingType::SetupConst
            } else if is_const {
                BindingType::SetupMaybeRef
            } else {
                BindingType::SetupLet
            };
            bindings.set(&i.id.sym, ty);
        }
        Pat::Rest(r) => {
            let ty = if is_const {
                BindingType::SetupConst
            } else {
                BindingType::SetupLet
            };
            if let Pat::Ident(i) = &*r.arg {
                bindings.set(&i.id.sym, ty);
            }
        }
        Pat::Object(o) => walk_object_pattern(o, bindings, is_const, false),
        Pat::Array(a) => {
            for e in a.elems.iter().flatten() {
                walk_pattern(e, bindings, is_const, false);
            }
        }
        Pat::Assign(a) => match &*a.left {
            Pat::Ident(i) => {
                let ty = if is_define_call {
                    BindingType::SetupConst
                } else if is_const {
                    BindingType::SetupMaybeRef
                } else {
                    BindingType::SetupLet
                };
                bindings.set(&i.id.sym, ty);
            }
            other => walk_pattern(other, bindings, is_const, false),
        },
        _ => {}
    }
}

fn walk_object_pattern(
    node: &ObjectPat,
    bindings: &mut Bindings,
    is_const: bool,
    is_define_call: bool,
) {
    for p in &node.props {
        match p {
            ObjectPatProp::Assign(a) => {
                // shorthand: const { x } = ...
                let ty = if is_define_call {
                    BindingType::SetupConst
                } else if is_const {
                    BindingType::SetupMaybeRef
                } else {
                    BindingType::SetupLet
                };
                bindings.set(&a.key.id.sym, ty);
            }
            ObjectPatProp::KeyValue(kv) => {
                walk_pattern(&kv.value, bindings, is_const, is_define_call)
            }
            ObjectPatProp::Rest(r) => {
                let ty = if is_const {
                    BindingType::SetupConst
                } else {
                    BindingType::SetupLet
                };
                if let Pat::Ident(i) = &*r.arg {
                    bindings.set(&i.id.sym, ty);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_declaration(
    from_script: bool,
    decl: &Decl,
    bindings: &mut Bindings,
    vue_import_aliases: &[(String, String)],
    hoist_static: bool,
    is_props_destructure_enabled: bool,
) -> bool {
    let alias = |name: &str| -> Option<String> {
        vue_import_aliases
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    let mut is_all_literal = false;
    match decl {
        Decl::Var(v) => {
            let is_const = v.kind == VarDeclKind::Const;
            is_all_literal = is_const
                && v.decls.iter().all(|d| {
                    matches!(d.name, Pat::Ident(_))
                        && d.init.as_ref().map(|i| is_static_node(i)).unwrap_or(false)
                });
            for d in &v.decls {
                let init = d.init.as_ref().map(|i| unwrap_ts_node(i).clone());
                let is_const_macro_call = is_const
                    && init.as_ref().map(|i| {
                        is_call_of(Some(i), |c| {
                            c == DEFINE_PROPS
                                || c == DEFINE_EMITS
                                || c == WITH_DEFAULTS
                                || c == DEFINE_SLOTS
                        })
                    }).unwrap_or(false);
                match &d.name {
                    Pat::Ident(id) => {
                        let user_reactive = alias("reactive");
                        let binding_type = if (hoist_static || from_script)
                            && (is_all_literal
                                || (is_const
                                    && init
                                        .as_ref()
                                        .map(|i| is_static_node(i))
                                        .unwrap_or(false)))
                        {
                            BindingType::LiteralConst
                        } else if init
                            .as_ref()
                            .map(|i| {
                                user_reactive
                                    .as_deref()
                                    .map(|r| is_call_of(Some(i), |c| c == r))
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false)
                        {
                            if is_const {
                                BindingType::SetupReactiveConst
                            } else {
                                BindingType::SetupLet
                            }
                        } else if is_const_macro_call
                            || (is_const
                                && init
                                    .as_ref()
                                    .map(|i| can_never_be_ref(i, user_reactive.as_deref()))
                                    .unwrap_or(false))
                        {
                            if init
                                .as_ref()
                                .map(|i| is_call_of(Some(i), |c| c == DEFINE_PROPS))
                                .unwrap_or(false)
                            {
                                BindingType::SetupReactiveConst
                            } else {
                                BindingType::SetupConst
                            }
                        } else if is_const {
                            let is_ref_call = init
                                .as_ref()
                                .map(|i| {
                                    is_call_of(Some(i), |c| {
                                        [
                                            "ref",
                                            "computed",
                                            "shallowRef",
                                            "customRef",
                                            "toRef",
                                            "useTemplateRef",
                                        ]
                                        .iter()
                                        .any(|k| alias(k).as_deref() == Some(c))
                                            || c == DEFINE_MODEL
                                    })
                                })
                                .unwrap_or(false);
                            if is_ref_call {
                                BindingType::SetupRef
                            } else {
                                BindingType::SetupMaybeRef
                            }
                        } else {
                            BindingType::SetupLet
                        };
                        bindings.set(&id.id.sym, binding_type);
                    }
                    other => {
                        let is_define_props = init
                            .as_ref()
                            .map(|i| is_call_of(Some(i), |c| c == DEFINE_PROPS))
                            .unwrap_or(false);
                        if is_define_props && is_props_destructure_enabled {
                            continue;
                        }
                        match other {
                            Pat::Object(o) => {
                                walk_object_pattern(o, bindings, is_const, is_const_macro_call)
                            }
                            Pat::Array(a) => {
                                for e in a.elems.iter().flatten() {
                                    walk_pattern(e, bindings, is_const, is_const_macro_call);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Decl::TsEnum(e) => {
            is_all_literal = e.members.iter().all(|m| {
                m.init
                    .as_ref()
                    .map(|i| is_static_node(i))
                    .unwrap_or(true)
            });
            bindings.set(
                &e.id.sym,
                if is_all_literal {
                    BindingType::LiteralConst
                } else {
                    BindingType::SetupConst
                },
            );
        }
        Decl::Fn(f) => bindings.set(&f.ident.sym, BindingType::SetupConst),
        Decl::Class(c) => bindings.set(&c.ident.sym, BindingType::SetupConst),
        _ => {}
    }
    is_all_literal
}

pub fn compile_script(
    sfc: &SfcDescriptor,
    arena: &Arena,
    options: ScriptCompileOptions,
) -> Result<ScriptBlockResult, String> {
    let script = sfc.script.clone();
    let script_setup = sfc.script_setup.clone();
    let source = sfc.source.clone();

    let hoist_static = options.hoist_static.unwrap_or(true) && script.is_none();
    let scope_id = options
        .id
        .strip_prefix("data-v-")
        .unwrap_or(&options.id)
        .to_string();
    let script_lang = script.as_ref().and_then(|s| s.lang.clone());
    let setup_lang = script_setup.as_ref().and_then(|s| s.lang.clone());
    let is_js_or_ts = super::script::context::is_js(&script_lang)
        || super::script::context::is_js(&setup_lang)
        || super::script::context::is_ts(&script_lang)
        || super::script::context::is_ts(&setup_lang);

    if script.is_some() && script_setup.is_some() && script_lang != setup_lang {
        return Err(
            "[@vue/compiler-sfc] <script> and <script setup> must have the same language type."
                .to_string(),
        );
    }

    if script_setup.is_none() {
        let script = match script {
            Some(s) => s,
            None => return Err("[@vue/compiler-sfc] SFC contains no <script> tags.".to_string()),
        };
        if script.lang.is_some() && !is_js_or_ts {
            return Ok(ScriptBlockResult {
                content: script.content.clone(),
                block: script,
                bindings: BindingMetadata::default(),
                imports: Vec::new(),
            });
        }
        let mut ctx = ScriptCompileContext::new(sfc, options)?;
        let r = process_normal_script(&mut ctx, &scope_id);
        return Ok(ScriptBlockResult {
            block: r.block,
            content: r.content,
            bindings: r.bindings,
            imports: Vec::new(),
        });
    }

    if setup_lang.is_some() && !is_js_or_ts {
        let s = script_setup.unwrap();
        return Ok(ScriptBlockResult {
            content: s.content.clone(),
            block: s,
            bindings: BindingMetadata::default(),
            imports: Vec::new(),
        });
    }

    let script_setup = script_setup.unwrap();
    let mut ctx = ScriptCompileContext::new(sfc, options.clone())?;
    collect_type_decls(&mut ctx);

    let mut script_bindings = Bindings::new();
    let mut setup_bindings = Bindings::new();
    let mut has_default_export = false;
    let mut has_await = false;

    let start_offset = ctx.start_offset;
    let end_offset = ctx.end_offset;
    let script_start_offset = script
        .as_ref()
        .map(|s| utf16_to_byte(&source, s.loc.start.offset.max(0) as usize));
    let script_end_offset = script
        .as_ref()
        .map(|s| utf16_to_byte(&source, s.loc.end.offset.max(0) as usize));

    let template_used_ids: Option<HashSet<String>> = if !options.inline_template {
        sfc.template
            .as_ref()
            .filter(|t| t.src.is_none() && t.lang.is_none())
            .and_then(|t| t.ast.as_ref())
            .map(|children| analyze_template(arena, children).used_ids)
    } else {
        None
    };

    let script_ast = ctx.script_ast.clone();
    let setup_ast = ctx.script_setup_ast.clone().unwrap();

    // 1.1 imports of <script>
    if let Some(ast) = &script_ast {
        for item in &ast.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(imp)) = item {
                for spec in &imp.specifiers {
                    let imported = get_imported_name(spec);
                    let local = match spec {
                        ImportSpecifier::Named(n) => n.local.sym.to_string(),
                        ImportSpecifier::Default(d) => d.local.sym.to_string(),
                        ImportSpecifier::Namespace(n) => n.local.sym.to_string(),
                    };
                    let is_type = imp.type_only
                        || matches!(spec, ImportSpecifier::Named(n) if n.is_type_only);
                    register_user_import(
                        &mut ctx,
                        atom(&imp.src.value),
                        local,
                        imported,
                        is_type,
                        false,
                        &template_used_ids,
                        !options.inline_template,
                    );
                }
            }
        }
    }

    // 1.2 imports of <script setup>
    for item in &setup_ast.body {
        let imp = match item {
            ModuleItem::ModuleDecl(ModuleDecl::Import(i)) => i,
            _ => continue,
        };
        hoist_node(&mut ctx, sp(imp), &source, start_offset, end_offset);

        let mut removed = 0usize;
        let specs = &imp.specifiers;
        for i in 0..specs.len() {
            let spec = &specs[i];
            let local = match spec {
                ImportSpecifier::Named(n) => n.local.sym.to_string(),
                ImportSpecifier::Default(d) => d.local.sym.to_string(),
                ImportSpecifier::Namespace(n) => n.local.sym.to_string(),
            };
            let imported = get_imported_name(spec);
            let src = atom(&imp.src.value);
            let existing = ctx
                .user_imports
                .iter()
                .find(|(k, _)| *k == local)
                .map(|(_, v)| v.clone());
            let mut remove_this = false;
            if src == "vue" && MACROS.contains(&imported.as_str()) {
                if local != imported {
                    return Err(ctx.error(&format!(
                        "`{imported}` is a compiler macro and cannot be aliased to a different name."
                    )));
                }
                remove_this = true;
            } else if let Some(existing) = existing {
                if existing.source == src && existing.imported == imported {
                    remove_this = true;
                } else {
                    return Err(ctx.error("different imports aliased to same local name."));
                }
            } else {
                let is_type = imp.type_only
                    || matches!(spec, ImportSpecifier::Named(n) if n.is_type_only);
                register_user_import(
                    &mut ctx,
                    src,
                    local,
                    imported,
                    is_type,
                    true,
                    &template_used_ids,
                    !options.inline_template,
                );
            }
            if remove_this {
                let remove_left = i > removed;
                removed += 1;
                let cur = sp(spec);
                let next = specs.get(i + 1).map(sp);
                let from = if remove_left {
                    sp(&specs[i - 1]).1 + start_offset
                } else {
                    cur.0 + start_offset
                };
                let to = match (next, remove_left) {
                    (Some(n), false) => n.0 + start_offset,
                    _ => cur.1 + start_offset,
                };
                ctx.s.remove(from, to);
            }
        }
        if !specs.is_empty() && removed == specs.len() {
            let (s, e) = sp(imp);
            ctx.s.remove(s + start_offset, e + start_offset);
        }
    }

    // 1.3 vue import aliases
    let mut vue_import_aliases: Vec<(String, String)> = Vec::new();
    for (_, b) in &ctx.user_imports {
        if b.source == "vue" {
            vue_import_aliases.push((b.imported.clone(), b.local.clone()));
        }
    }

    // 2.1 normal <script> body
    if let (Some(script), Some(ast)) = (&script, &script_ast) {
        let sso = script_start_offset.unwrap();
        let seo = script_end_offset.unwrap();
        for item in &ast.body {
            match item {
                ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(e)) => {
                    has_default_export = true;
                    if let Expr::Object(o) = &*e.expr {
                        check_default_export_options(&mut ctx, &o.props);
                    } else if let Expr::Call(c) = &*e.expr {
                        if let Some(a) = c.args.first() {
                            if let Expr::Object(o) = &*a.expr {
                                check_default_export_options(&mut ctx, &o.props);
                            }
                        }
                    }
                    let start = sp(e).0 + sso;
                    let end = sp(&*e.expr).0 + sso;
                    ctx.s
                        .overwrite(start, end, &format!("const {NORMAL_SCRIPT_DEFAULT_VAR} = "));
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(d)) => {
                    has_default_export = true;
                    let start = sp(d).0 + sso;
                    let end = match &d.decl {
                        DefaultDecl::Class(c) => c.class.span.lo.0 as usize,
                        DefaultDecl::Fn(f) => f.function.span.lo.0 as usize,
                        DefaultDecl::TsInterfaceDecl(t) => t.span.lo.0 as usize,
                    } + sso;
                    ctx.s
                        .overwrite(start, end, &format!("const {NORMAL_SCRIPT_DEFAULT_VAR} = "));
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(n)) => {
                    let default_spec = n.specifiers.iter().find_map(|s| match s {
                        ExportSpecifier::Named(named) => {
                            let exported = named.exported.as_ref().unwrap_or(&named.orig);
                            let name = match exported {
                                ModuleExportName::Ident(i) => i.sym.to_string(),
                                ModuleExportName::Str(s) => atom(&s.value),
                            };
                            if name == "default" { Some(named) } else { None }
                        }
                        _ => None,
                    });
                    if let Some(spec) = default_spec {
                        has_default_export = true;
                        if n.specifiers.len() > 1 {
                            let (s, e) = sp(spec);
                            ctx.s.remove(s + sso, e + sso);
                        } else {
                            let (s, e) = sp(n);
                            ctx.s.remove(s + sso, e + sso);
                        }
                        let local = match &spec.orig {
                            ModuleExportName::Ident(i) => i.sym.to_string(),
                            ModuleExportName::Str(s) => atom(&s.value),
                        };
                        if let Some(src) = &n.src {
                            ctx.s.prepend(&format!(
                                "import {{ {local} as {NORMAL_SCRIPT_DEFAULT_VAR} }} from '{}'\n",
                                atom(&src.value)
                            ));
                        } else {
                            ctx.s.append_left(
                                seo,
                                &format!("\nconst {NORMAL_SCRIPT_DEFAULT_VAR} = {local}\n"),
                            );
                        }
                    }
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(e)) => {
                    walk_declaration(
                        true,
                        &e.decl,
                        &mut script_bindings,
                        &vue_import_aliases,
                        hoist_static,
                        false,
                    );
                }
                ModuleItem::Stmt(Stmt::Decl(d)) => {
                    if !is_declare(d) {
                        walk_declaration(
                            true,
                            d,
                            &mut script_bindings,
                            &vue_import_aliases,
                            hoist_static,
                            false,
                        );
                    }
                }
                _ => {}
            }
        }

        if sso > start_offset {
            if !script.content.trim_end().ends_with('\n') {
                ctx.s.append_left(seo, "\n");
            }
            ctx.s.move_range(sso, seo, 0);
        }
    }

    // 2.2 <script setup> body
    let mut props_destructure_enabled = false;
    for item in &setup_ast.body {
        match item {
            ModuleItem::Stmt(Stmt::Expr(stmt)) => {
                let expr = unwrap_ts_node(&stmt.expr).clone();
                if process_define_props(&mut ctx, &expr, None, false)?
                    || process_define_emits(&mut ctx, &expr, None)?
                    || process_define_options(&mut ctx, &expr)?
                    || process_define_slots(&mut ctx, &expr, None)?
                {
                    let (s, e) = sp(stmt);
                    ctx.s.remove(s + start_offset, e + start_offset);
                } else if process_define_expose(&mut ctx, &expr)? {
                    if let Expr::Call(c) = &expr {
                        if let Callee::Expr(callee) = &c.callee {
                            let (s, e) = sp(&**callee);
                            ctx.s
                                .overwrite(s + start_offset, e + start_offset, "__expose");
                        }
                    }
                } else {
                    process_define_model(&mut ctx, &expr, None)?;
                }
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::Var(v))) if !v.declare => {
                let total = v.decls.len();
                let mut left = total;
                let mut last_non_removed: Option<usize> = None;
                for i in 0..total {
                    let decl = &v.decls[i];
                    let init = match &decl.init {
                        Some(i) => unwrap_ts_node(i).clone(),
                        None => continue,
                    };
                    if process_define_options(&mut ctx, &init)? {
                        return Err(ctx.error(&format!(
                            "{DEFINE_OPTIONS}() has no returning value, it cannot be assigned."
                        )));
                    }
                    let is_define_props =
                        process_define_props(&mut ctx, &init, Some(&decl.name), false)?;
                    if let Some(rest) = ctx.props_destructure_rest_id.clone() {
                        setup_bindings.set(&rest, BindingType::SetupReactiveConst);
                    }
                    let is_define_emits = !is_define_props
                        && process_define_emits(&mut ctx, &init, Some(&decl.name))?;
                    if !is_define_emits {
                        let _ = process_define_slots(&mut ctx, &init, Some(&decl.name))?
                            || process_define_model(&mut ctx, &init, Some(&decl.name))?;
                    }

                    if is_define_props
                        && ctx.props_destructure_rest_id.is_none()
                        && ctx.props_destructure_decl.is_some()
                    {
                        props_destructure_enabled = true;
                        if left == 1 {
                            let (s, e) = sp(v);
                            ctx.s.remove(s + start_offset, e + start_offset);
                        } else {
                            let (mut start, mut end) = sp(decl);
                            start += start_offset;
                            end += start_offset;
                            if i == total - 1 {
                                start = sp(&v.decls[last_non_removed.unwrap()]).1 + start_offset;
                            } else {
                                end = sp(&v.decls[i + 1]).0 + start_offset;
                            }
                            ctx.s.remove(start, end);
                            left -= 1;
                        }
                    } else if is_define_emits {
                        let (s, e) = sp(&init);
                        ctx.s
                            .overwrite(start_offset + s, start_offset + e, "__emit");
                    } else {
                        last_non_removed = Some(i);
                    }
                }
            }
            _ => {}
        }

        // walk declarations to record bindings
        let mut is_all_literal = false;
        let decl = match item {
            ModuleItem::Stmt(Stmt::Decl(d)) if !is_declare(d) => Some(d),
            _ => None,
        };
        if let Some(d) = decl {
            is_all_literal = walk_declaration(
                false,
                d,
                &mut setup_bindings,
                &vue_import_aliases,
                hoist_static,
                ctx.props_destructure_decl.is_some(),
            );
        }
        if hoist_static && is_all_literal {
            hoist_node(&mut ctx, sp_of_item(item), &source, start_offset, end_offset);
        }

        // top-level await
        if let ModuleItem::Stmt(stmt) = item {
            let mut found = Vec::new();
            collect_top_level_awaits(stmt, &mut found);
            if !found.is_empty() {
                has_await = true;
                for a in found {
                    process_await(
                        &mut ctx,
                        a.await_span,
                        a.arg_span,
                        &source,
                        start_offset,
                        a.needs_semi,
                        a.is_statement,
                    );
                }
            }
        }

        // ES module exports are not allowed
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(e))
                if !matches!(
                    &e.decl,
                    Decl::TsInterface(_) | Decl::TsTypeAlias(_) | Decl::TsModule(_)
                ) =>
            {
                return Err(ctx.error_at_item(
                    "<script setup> cannot contain ES module exports. If you are using a previous version of <script setup>, please consult the updated RFC at https://github.com/vuejs/rfcs/pull/227.",
                    sp_of_item(item),
                ));
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(n)) if !n.type_only => {
                return Err(ctx.error_at_item(
                    "<script setup> cannot contain ES module exports. If you are using a previous version of <script setup>, please consult the updated RFC at https://github.com/vuejs/rfcs/pull/227.",
                    sp_of_item(item),
                ));
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportAll(_))
            | ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(_))
            | ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(_)) => {
                return Err(ctx.error_at_item(
                    "<script setup> cannot contain ES module exports. If you are using a previous version of <script setup>, please consult the updated RFC at https://github.com/vuejs/rfcs/pull/227.",
                    sp_of_item(item),
                ));
            }
            _ => {}
        }

        // move type declarations to outer scope
        if ctx.is_ts {
            // `node.type.startsWith('TS')`, minus enums: a bodyless function
            // is Babel's `TSDeclareFunction` — an overload or a `declare`
            let is_ts_decl = |d: &Decl| match d {
                Decl::TsInterface(_) | Decl::TsTypeAlias(_) | Decl::TsModule(_) => true,
                Decl::Fn(f) => f.function.body.is_none(),
                _ => false,
            };
            let is_type_decl = match item {
                ModuleItem::Stmt(Stmt::Decl(Decl::Var(v))) => v.declare,
                ModuleItem::Stmt(Stmt::Decl(d)) => is_ts_decl(d),
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(n)) => n.type_only,
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(e)) => is_ts_decl(&e.decl),
                _ => false,
            };
            if is_type_decl {
                hoist_node(&mut ctx, sp_of_item(item), &source, start_offset, end_offset);
            }
        }
    }
    let _ = props_destructure_enabled;

    // 3. props destructure transform
    if ctx.props_destructure_decl.is_some() {
        super::script::define_props_destructure::transform_destructured_props(&mut ctx)?;
    }

    // 5. remove non-script content
    if script.is_some() {
        let sso = script_start_offset.unwrap();
        let seo = script_end_offset.unwrap();
        if start_offset < sso {
            ctx.s.remove(0, start_offset);
            ctx.s.remove(end_offset, sso);
            ctx.s.remove(seo, source.len());
        } else {
            ctx.s.remove(0, sso);
            ctx.s.remove(seo, start_offset);
            ctx.s.remove(end_offset, source.len());
        }
    } else {
        ctx.s.remove(0, start_offset);
        ctx.s.remove(end_offset, source.len());
    }

    // 6. binding metadata
    if let Some(ast) = &script_ast {
        let analyzed = super::script::analyze_script_bindings::analyze_script_bindings(&ast.body);
        for (k, v) in analyzed.bindings {
            ctx.binding_metadata.bindings.insert(k, v);
        }
        if analyzed.is_script_setup == Some(false) {
            ctx.binding_metadata.is_script_setup = Some(false);
        }
    }
    let imports = ctx.user_imports.clone();
    for (key, b) in &imports {
        if b.is_type {
            continue;
        }
        let ty = if b.imported == "*"
            || (b.imported == "default" && b.source.ends_with(".vue"))
            || b.source == "vue"
        {
            BindingType::SetupConst
        } else {
            BindingType::SetupMaybeRef
        };
        ctx.set_binding(key, ty);
    }
    for (k, v) in &script_bindings.0 {
        ctx.set_binding(k, *v);
    }
    for (k, v) in &setup_bindings.0 {
        ctx.set_binding(k, *v);
    }

    // `v-model` cannot write to a `const` reactive binding, so the compiler
    // demotes it to `let`
    if let Some(children) = sfc
        .template
        .as_ref()
        .filter(|t| t.src.is_none())
        .and_then(|t| t.ast.as_ref())
    {
        let v_model_ids = analyze_template(arena, children).v_model_ids;
        let to_demote: HashSet<String> = v_model_ids
            .into_iter()
            .filter(|id| {
                setup_bindings.0.iter().any(|(k, t)| {
                    k == id && *t == BindingType::SetupReactiveConst
                })
            })
            .collect();
        if !to_demote.is_empty() {
            for item in &setup_ast.body {
                let ModuleItem::Stmt(Stmt::Decl(Decl::Var(v))) = item else {
                    continue;
                };
                if v.kind != VarDeclKind::Const || v.declare {
                    continue;
                }
                let demoted: Vec<String> = v
                    .decls
                    .iter()
                    .filter_map(|d| match &d.name {
                        Pat::Ident(i) if to_demote.contains(&i.id.sym.to_string()) => {
                            Some(i.id.sym.to_string())
                        }
                        _ => None,
                    })
                    .collect();
                if demoted.is_empty() {
                    continue;
                }
                let start = v.span.lo.0 as usize + start_offset;
                ctx.s.overwrite(start, start + "const".len(), "let");
                for id in demoted {
                    setup_bindings.set(&id, BindingType::SetupLet);
                    ctx.set_binding(&id, BindingType::SetupLet);
                }
            }
        }
    }

    ctx.binding_metadata.is_script_setup = Some(true);

    // 7. useCssVars
    if !sfc.css_vars.is_empty() && !options.template_ssr {
        ctx.helper_imports.push(super::css_vars::CSS_VARS_HELPER.to_string());
        ctx.helper_imports.push("unref".to_string());
        let code = super::css_vars::gen_css_vars_code(
            &sfc.css_vars,
            &ctx.binding_metadata,
            &scope_id,
            options.is_prod,
        );
        ctx.s.prepend_left(start_offset, &format!("\n{code}\n"));
    }

    // 8. setup() signature
    let mut args = String::from("__props");
    if ctx.props_type_decl.is_some() {
        args.push_str(": any");
    }
    if ctx.props_decl.is_some() {
        if let Some(rest_id) = ctx.props_destructure_rest_id.clone() {
            let keys: Vec<String> = ctx
                .props_destructured_bindings
                .iter()
                .map(|(k, _)| k.clone())
                .collect();
            let helper = ctx.helper("createPropsRestProxy");
            if let Some((s, e)) = ctx.props_call {
                ctx.s.overwrite(
                    start_offset + s,
                    start_offset + e,
                    &format!(
                        "{helper}(__props, {})",
                        serde_json::to_string(&keys).unwrap()
                    ),
                );
            }
            if let Some(decl) = ctx.props_destructure_decl.clone() {
                let (s, e) = sp(&decl);
                ctx.s
                    .overwrite(start_offset + s, start_offset + e, &rest_id);
            }
        } else if ctx.props_destructure_decl.is_none() {
            if let Some((s, e)) = ctx.props_call {
                ctx.s
                    .overwrite(start_offset + s, start_offset + e, "__props");
            }
        }
    }
    if has_await {
        let any = if ctx.is_ts { ": any" } else { "" };
        ctx.s
            .prepend_left(start_offset, &format!("\nlet __temp{any}, __restore{any}\n"));
    }

    let mut destructure_elements: Vec<String> = Vec::new();
    if ctx.has_define_expose_call || !options.inline_template {
        destructure_elements.push("expose: __expose".to_string());
    }
    if ctx.emit_decl.is_some() {
        destructure_elements.push("emit: __emit".to_string());
    }
    if !destructure_elements.is_empty() {
        args.push_str(&format!(", {{ {} }}", destructure_elements.join(", ")));
    }

    // 9. return statement
    let props_decl = gen_runtime_props(&mut ctx);
    // `ctx.error` throws in JS, so a type that could not be resolved aborts
    if let Some(e) = ctx.errors.first() {
        return Err(e.clone());
    }
    let mut has_inlined_ssr_render_fn = false;
    let returned: String;
    if !options.inline_template || (sfc.template.is_none() && ctx.has_default_export_render) {
        let mut all: Vec<(String, Option<BindingType>)> = Vec::new();
        for (k, v) in &script_bindings.0 {
            all.push((k.clone(), Some(*v)));
        }
        for (k, v) in &setup_bindings.0 {
            match all.iter_mut().find(|(n, _)| n == k) {
                Some(slot) => slot.1 = Some(*v),
                None => all.push((k.clone(), Some(*v))),
            }
        }
        for (key, b) in &imports {
            if !b.is_type && b.is_used_in_template {
                match all.iter_mut().find(|(n, _)| n == key) {
                    Some(slot) => slot.1 = None,
                    None => all.push((key.clone(), None)),
                }
            }
        }
        let mut out = String::from("{ ");
        for (key, ty) in &all {
            let import = imports.iter().find(|(k, _)| k == key).map(|(_, v)| v);
            if ty.is_none() {
                let src = import.map(|i| i.source.clone()).unwrap_or_default();
                if src != "vue" && !src.ends_with(".vue") {
                    out.push_str(&format!("get {key}() {{ return {key} }}, "));
                    continue;
                }
                out.push_str(&format!("{key}, "));
            } else if ctx.binding_metadata.get(key) == Some(BindingType::SetupLet) {
                let set_arg = if key == "v" { "_v" } else { "v" };
                out.push_str(&format!(
                    "get {key}() {{ return {key} }}, set {key}({set_arg}) {{ {key} = {set_arg} }}, "
                ));
            } else {
                out.push_str(&format!("{key}, "));
            }
        }
        if out.ends_with(", ") {
            out.truncate(out.len() - 2);
        }
        out.push_str(" }");
        returned = out;
    } else if let Some(template) = sfc.template.as_ref().filter(|t| t.src.is_none()) {
        if options.template_ssr {
            has_inlined_ssr_render_fn = true;
        }
        let scoped = sfc.styles.iter().any(|s| s.scoped);
        // `compileTemplate` is handed the descriptor's own (untransformed) AST,
        // so its parse errors are not re-reported here
        let (t_arena, children) = super::compile_template::fresh_template_ast(sfc, arena)
            .ok_or("failed to re-parse template")?;
        let _ = template;
        let r = super::compile_template::compile_template_ast(
            t_arena,
            children,
            sfc.source.clone(),
            Vec::new(),
            super::compile_template::TemplateCompileOptions {
                filename: ctx.filename.clone(),
                id: scope_id.clone(),
                scoped,
                is_prod: options.is_prod,
                ssr: options.template_ssr,
                ssr_css_vars: sfc.css_vars.clone(),
                binding_metadata: ctx.binding_metadata.clone(),
                binding_metadata_provided: true,
                expression_plugins: if ctx.is_ts {
                    vec!["typescript".to_string()]
                } else {
                    Vec::new()
                },
                inline: true,
                is_ts: ctx.is_ts,
                ..Default::default()
            },
        );
        if let Some(err) = r.errors.first() {
            let mut msg = err.message.clone();
            if let Some(loc) = &err.loc {
                msg += &format!(
                    "\n\n{}\n{}\n",
                    ctx.filename,
                    crate::core::codeframe::generate_code_frame(
                        &source,
                        loc.start.offset.max(0) as usize,
                        loc.end.offset.max(0) as usize,
                    )
                );
            }
            return Err(msg);
        }
        if !r.preamble.is_empty() {
            ctx.s.prepend(&r.preamble);
        }
        if r.arena.root(r.root).helpers.contains(&crate::core::ast::RuntimeHelper::UNREF) {
            ctx.helper_imports.retain(|h| h != "unref");
        }
        returned = r.code;
    } else {
        returned = "() => {}".to_string();
    }

    if !options.inline_template {
        ctx.s.append_right(
            end_offset,
            &format!(
                "\nconst __returned__ = {returned}\nObject.defineProperty(__returned__, '__isScriptSetup', {{ enumerable: false, value: true }})\nreturn __returned__\n}}\n\n"
            ),
        );
    } else {
        ctx.s
            .append_right(end_offset, &format!("\nreturn {returned}\n}}\n\n"));
    }

    // 10. default export
    let gen_default_as = match &options.gen_default_as {
        Some(v) => format!("const {v} ="),
        None => "export default".to_string(),
    };
    let mut runtime_options = String::new();
    if !ctx.has_default_export_name
        && !ctx.filename.is_empty()
        && ctx.filename != super::parse::DEFAULT_FILENAME
    {
        if let Some(name) = file_base_name(&ctx.filename) {
            runtime_options.push_str(&format!("\n  __name: '{name}',"));
        }
    }
    if has_inlined_ssr_render_fn {
        runtime_options.push_str("\n  __ssrInlineRender: true,");
    }
    if let Some(p) = &props_decl {
        runtime_options.push_str(&format!("\n  props: {p},"));
    }
    let emits_decl = gen_runtime_emits(&mut ctx);
    if let Some(e) = ctx.errors.first() {
        return Err(e.clone());
    }
    if let Some(e) = emits_decl {
        runtime_options.push_str(&format!("\n  emits: {e},"));
    }

    let defined_options = ctx
        .options_runtime_decl
        .clone()
        .map(|d| {
            let (s, e) = sp(&d);
            script_setup.content[s.min(script_setup.content.len())
                ..e.min(script_setup.content.len())]
                .trim()
                .to_string()
        })
        .unwrap_or_default();

    let expose_call = if ctx.has_define_expose_call || options.inline_template {
        String::new()
    } else {
        "  __expose();\n".to_string()
    };

    if ctx.is_ts {
        let def = format!(
            "{}{}",
            if has_default_export {
                format!("\n  ...{NORMAL_SCRIPT_DEFAULT_VAR},")
            } else {
                String::new()
            },
            if !defined_options.is_empty() {
                format!("\n  ...{defined_options},")
            } else {
                String::new()
            }
        );
        let helper = ctx.helper("defineComponent");
        ctx.s.prepend_left(
            start_offset,
            &format!(
                "\n{gen_default_as} /*@__PURE__*/{helper}({{{def}{runtime_options}\n  {}setup({args}) {{\n{expose_call}",
                if has_await { "async " } else { "" }
            ),
        );
        ctx.s.append_right(end_offset, "})");
    } else if has_default_export || !defined_options.is_empty() {
        ctx.s.prepend_left(
            start_offset,
            &format!(
                "\n{gen_default_as} /*@__PURE__*/Object.assign({}{}{{{runtime_options}\n  {}setup({args}) {{\n{expose_call}",
                if has_default_export {
                    format!("{NORMAL_SCRIPT_DEFAULT_VAR}, ")
                } else {
                    String::new()
                },
                if !defined_options.is_empty() {
                    format!("{defined_options}, ")
                } else {
                    String::new()
                },
                if has_await { "async " } else { "" }
            ),
        );
        ctx.s.append_right(end_offset, "})");
    } else {
        ctx.s.prepend_left(
            start_offset,
            &format!(
                "\n{gen_default_as} {{{runtime_options}\n  {}setup({args}) {{\n{expose_call}",
                if has_await { "async " } else { "" }
            ),
        );
        ctx.s.append_right(end_offset, "}");
    }

    // 11. helper imports
    if !ctx.helper_imports.is_empty() {
        let mut seen = HashSet::new();
        let list: Vec<String> = ctx
            .helper_imports
            .iter()
            .filter(|h| seen.insert((*h).clone()))
            .map(|h| format!("{h} as _{h}"))
            .collect();
        ctx.s
            .prepend(&format!("import {{ {} }} from 'vue'\n", list.join(", ")));
    }

    let content = ctx.s.to_string();
    Ok(ScriptBlockResult {
        block: script_setup,
        content,
        bindings: ctx.binding_metadata.clone(),
        imports: ctx.user_imports.clone(),
    })
}

fn sp_of_item(item: &ModuleItem) -> (usize, usize) {
    let s = item.span();
    (s.lo.0 as usize, s.hi.0 as usize)
}

fn is_declare(d: &Decl) -> bool {
    match d {
        Decl::Var(v) => v.declare,
        Decl::Fn(f) => f.declare,
        Decl::Class(c) => c.declare,
        Decl::TsEnum(e) => e.declare,
        Decl::TsInterface(i) => i.declare,
        Decl::TsTypeAlias(t) => t.declare,
        Decl::TsModule(m) => m.declare,
        _ => false,
    }
}

fn file_base_name(filename: &str) -> Option<String> {
    let last = filename.rsplit(['/', '\\']).next()?;
    let dot = last.rfind('.')?;
    let (name, ext) = last.split_at(dot);
    if name.is_empty() || ext.len() < 2 {
        return None;
    }
    Some(name.to_string())
}

fn check_default_export_options(ctx: &mut ScriptCompileContext, props: &[PropOrSpread]) {
    for p in props {
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
                Prop::Shorthand(i) => Some(i.sym.to_string()),
                _ => None,
            };
            match key.as_deref() {
                Some("name") => ctx.has_default_export_name = true,
                Some("render") => ctx.has_default_export_render = true,
                _ => {}
            }
        }
    }
}

fn register_user_import(
    ctx: &mut ScriptCompileContext,
    source: String,
    local: String,
    imported: String,
    is_type: bool,
    is_from_setup: bool,
    template_used_ids: &Option<HashSet<String>>,
    need_template_usage_check: bool,
) {
    // an import counts as used unless a TS template says otherwise; with no
    // usable template there is nothing to narrow it down
    let mut is_used_in_template = need_template_usage_check;
    if let Some(ids) = template_used_ids {
        if ctx.is_ts {
            is_used_in_template = ids.contains(&local);
        }
    }
    let binding = ImportBinding {
        is_type,
        imported,
        local: local.clone(),
        source,
        is_from_setup,
        is_used_in_template,
    };
    match ctx.user_imports.iter_mut().find(|(k, _)| *k == local) {
        Some(slot) => slot.1 = binding,
        None => ctx.user_imports.push((local, binding)),
    }
}

fn hoist_node(
    ctx: &mut ScriptCompileContext,
    (start, end): (usize, usize),
    source: &str,
    start_offset: usize,
    limit: usize,
) {
    let start = start + start_offset;
    let mut end = end + start_offset;
    // `node.trailingComments`: Babel attaches every comment between this node
    // and the next statement to it, so they are hoisted along with it
    let mut scan = end;
    loop {
        while scan < limit {
            match source[scan..].chars().next() {
                Some(c) if c.is_whitespace() => scan += c.len_utf8(),
                _ => break,
            }
        }
        if source[scan.min(limit)..limit].starts_with("//") {
            end = source[scan..limit].find('\n').map_or(limit, |i| scan + i);
        } else if source[scan.min(limit)..limit].starts_with("/*") {
            end = source[scan..limit]
                .find("*/")
                .map_or(limit, |i| scan + i + 2);
        } else {
            break;
        }
        scan = end;
    }
    while end <= source.len() {
        let ch = source[end.min(source.len())..].chars().next();
        match ch {
            Some(c) if c.is_whitespace() => end += c.len_utf8(),
            _ => break,
        }
    }
    ctx.s.move_range(start, end, 0);
}

#[allow(clippy::too_many_arguments)]
fn process_await(
    ctx: &mut ScriptCompileContext,
    (await_start, await_end): (usize, usize),
    (arg_start, arg_end): (usize, usize),
    source: &str,
    start_offset: usize,
    need_semi: bool,
    is_statement: bool,
) {
    let argument_str = &source[(arg_start + start_offset).min(source.len())
        ..(arg_end + start_offset).min(source.len())];
    let contains_nested_await = argument_str.contains("await");
    let helper = ctx.helper("withAsyncContext");
    ctx.s.overwrite(
        await_start + start_offset,
        arg_start + start_offset,
        &format!(
            "{}(\n  ([__temp,__restore] = {helper}({}() => ",
            if need_semi { ";" } else { "" },
            if contains_nested_await { "async " } else { "" }
        ),
    );
    ctx.s.append_left(
        await_end + start_offset,
        &format!(
            ")),\n  {}await __temp,\n  __restore(){}\n)",
            if is_statement { "" } else { "__temp = " },
            if is_statement { "" } else { ",\n  __temp" }
        ),
    );
    let _ = arg_end;
}

pub struct AwaitInfo {
    pub await_span: (usize, usize),
    pub arg_span: (usize, usize),
    pub is_statement: bool,
    pub needs_semi: bool,
}

/// collects top-level `await` expressions (not inside nested functions)
fn collect_top_level_awaits(stmt: &Stmt, out: &mut Vec<AwaitInfo>) {
    struct V<'a> {
        out: &'a mut Vec<AwaitInfo>,
        /// (statement start, index within its block, block depth)
        statement: Option<(usize, usize, usize)>,
        depth: usize,
    }
    use swc_core::ecma::visit::VisitWith as _;
    impl swc_core::ecma::visit::Visit for V<'_> {
        fn visit_function(&mut self, _: &Function) {}
        fn visit_arrow_expr(&mut self, _: &ArrowExpr) {}
        fn visit_class(&mut self, _: &Class) {}
        fn visit_block_stmt(&mut self, n: &BlockStmt) {
            self.depth += 1;
            for (i, s) in n.stmts.iter().enumerate() {
                self.visit_stmt_at(s, i);
            }
            self.depth -= 1;
        }
        fn visit_await_expr(&mut self, n: &AwaitExpr) {
            let await_span = (n.span.lo.0 as usize, n.span.hi.0 as usize);
            let arg = n.arg.span();
            let (is_statement, needs_semi) = match self.statement {
                Some((start, index, depth)) if start == await_span.0 => {
                    (true, depth == 1 || index > 0)
                }
                _ => (false, false),
            };
            self.out.push(AwaitInfo {
                await_span,
                arg_span: (arg.lo.0 as usize, arg.hi.0 as usize),
                is_statement,
                needs_semi,
            });
            n.visit_children_with(self);
        }
    }
    impl V<'_> {
        fn visit_stmt_at(&mut self, s: &Stmt, index: usize) {
            use swc_core::ecma::visit::Visit;
            let saved = self.statement;
            if let Stmt::Expr(e) = s {
                self.statement = Some((e.span.lo.0 as usize, index, self.depth));
            } else {
                self.statement = None;
            }
            self.visit_stmt(s);
            self.statement = saved;
        }
    }
    let mut v = V {
        out,
        statement: None,
        depth: 1,
    };
    v.visit_stmt_at(stmt, 0);
}
