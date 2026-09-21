//! Port of `compiler-sfc/src/rewriteDefault.ts`.

use swc_core::common::Spanned;
use swc_core::ecma::ast::*;

use super::magic_string::MagicString;

pub fn rewrite_default(input: &str, as_: &str, ts: bool) -> String {
    let program = match crate::core::jsparse::parse_module(input, ts) {
        Ok(p) => p,
        Err(_) => return input.to_string(),
    };
    let mut s = MagicString::new(input);
    rewrite_default_ast(&program.body, &mut s, as_);
    s.to_string()
}

pub fn has_default_export(body: &[ModuleItem]) -> bool {
    body.iter().any(|item| match item {
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(_))
        | ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(_)) => true,
        ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(n)) => {
            n.specifiers.iter().any(|s| match s {
                ExportSpecifier::Named(named) => {
                    export_name(named.exported.as_ref().unwrap_or(&named.orig)) == "default"
                }
                ExportSpecifier::Default(_) => true,
                ExportSpecifier::Namespace(ns) => module_export_name(&ns.name) == "default",
            })
        }
        _ => false,
    })
}

fn export_name(n: &ModuleExportName) -> String {
    module_export_name(n)
}

fn module_export_name(n: &ModuleExportName) -> String {
    match n {
        ModuleExportName::Ident(i) => i.sym.to_string(),
        ModuleExportName::Str(s) => s.value.as_str().unwrap_or_default().to_string(),
    }
}

pub fn rewrite_default_ast(body: &[ModuleItem], s: &mut MagicString, as_: &str) {
    if !has_default_export(body) {
        s.append(&format!("\nconst {as_} = {{}}"));
        return;
    }

    for item in body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(node)) => {
                let node_start = node.span().lo.0 as usize;
                match &node.decl {
                    DefaultDecl::Class(c) if c.ident.is_some() => {
                        let ident = c.ident.as_ref().unwrap();
                        let start = if !c.class.decorators.is_empty() {
                            c.class.decorators.last().unwrap().span().hi.0 as usize
                        } else {
                            node_start
                        };
                        s.overwrite(start, ident.span.lo.0 as usize, " class ");
                        s.append(&format!("\nconst {as_} = {}", ident.sym));
                    }
                    other => {
                        let decl_start = match other {
                            DefaultDecl::Class(c) => c.class.span.lo.0 as usize,
                            DefaultDecl::Fn(f) => f.function.span.lo.0 as usize,
                            DefaultDecl::TsInterfaceDecl(t) => t.span.lo.0 as usize,
                        };
                        s.overwrite(node_start, decl_start, &format!("const {as_} = "));
                    }
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(node)) => {
                let start = node.span.lo.0 as usize;
                let decl_start = node.expr.span().lo.0 as usize;
                s.overwrite(start, decl_start, &format!("const {as_} = "));
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(node)) => {
                for specifier in &node.specifiers {
                    let named = match specifier {
                        ExportSpecifier::Named(n) => n,
                        _ => continue,
                    };
                    let exported = named.exported.as_ref().unwrap_or(&named.orig);
                    if module_export_name(exported) != "default" {
                        continue;
                    }
                    let local_name = module_export_name(&named.orig);
                    let spec_start = named.span.lo.0 as usize;
                    let spec_end = named.span.hi.0 as usize;
                    let node_end = node.span.hi.0 as usize;
                    if let Some(src) = &node.src {
                        let source = src.value.as_str().unwrap_or_default();
                        if local_name == "default" {
                            s.prepend(&format!(
                                "import {{ default as __VUE_DEFAULT__ }} from '{source}'\n"
                            ));
                            let end = specifier_end(
                                s,
                                named.orig.span().hi.0 as usize,
                                node_end,
                            );
                            s.remove(spec_start, end);
                            s.append(&format!("\nconst {as_} = __VUE_DEFAULT__"));
                            continue;
                        } else {
                            let local_src = s.slice_original(
                                named.orig.span().lo.0 as usize,
                                named.orig.span().hi.0 as usize,
                            );
                            s.prepend(&format!(
                                "import {{ {local_src} as __VUE_DEFAULT__ }} from '{source}'\n"
                            ));
                            let end =
                                specifier_end(s, exported.span().hi.0 as usize, node_end);
                            s.remove(spec_start, end);
                            s.append(&format!("\nconst {as_} = __VUE_DEFAULT__"));
                            continue;
                        }
                    }
                    let end = specifier_end(s, spec_end, node_end);
                    s.remove(spec_start, end);
                    s.append(&format!("\nconst {as_} = {local_name}"));
                }
            }
            _ => {}
        }
    }
}

fn specifier_end(s: &MagicString, end: usize, node_end: usize) -> usize {
    let mut end = end;
    let old_end = end;
    let mut has_commas = false;
    let original = s.original();
    while end < node_end {
        let ch = original[end..].chars().next().unwrap_or('\0');
        if ch.is_whitespace() {
            end += ch.len_utf8();
        } else if ch == ',' {
            end += 1;
            has_commas = true;
            break;
        } else if ch == '}' {
            break;
        } else {
            break;
        }
    }
    if has_commas { end } else { old_end }
}
