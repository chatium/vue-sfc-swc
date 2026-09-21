//! Port of `compiler-sfc/src/script/analyzeScriptBindings.ts`.

use swc_core::ecma::ast::*;

use crate::core::options::{BindingMetadata, BindingType};

use super::utils::{atom, resolve_object_key};

pub fn analyze_script_bindings(body: &[ModuleItem]) -> BindingMetadata {
    for item in body {
        if let ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(e)) = item {
            if let Expr::Object(o) = &*e.expr {
                return analyze_bindings_from_options(o);
            }
        }
    }
    BindingMetadata::default()
}

fn analyze_bindings_from_options(node: &ObjectLit) -> BindingMetadata {
    let mut bindings = BindingMetadata {
        is_script_setup: Some(false),
        ..Default::default()
    };
    for p in &node.props {
        let prop = match p {
            PropOrSpread::Prop(p) => &**p,
            _ => continue,
        };
        match prop {
            Prop::KeyValue(kv) => {
                let name = match &kv.key {
                    PropName::Ident(i) => i.sym.to_string(),
                    _ => continue,
                };
                if name == "props" {
                    for key in object_or_array_keys(&kv.value) {
                        bindings.bindings.insert(key, BindingType::Props);
                    }
                } else if name == "inject" {
                    for key in object_or_array_keys(&kv.value) {
                        bindings.bindings.insert(key, BindingType::Options);
                    }
                } else if (name == "computed" || name == "methods")
                    && matches!(&*kv.value, Expr::Object(_))
                {
                    for key in object_or_array_keys(&kv.value) {
                        bindings.bindings.insert(key, BindingType::Options);
                    }
                }
            }
            Prop::Method(m) => {
                let name = match &m.key {
                    PropName::Ident(i) => i.sym.to_string(),
                    _ => continue,
                };
                if name != "setup" && name != "data" {
                    continue;
                }
                if let Some(body) = &m.function.body {
                    for stmt in &body.stmts {
                        if let Stmt::Return(r) = stmt {
                            if let Some(arg) = &r.arg {
                                if let Expr::Object(o) = &**arg {
                                    for key in object_expression_keys(o) {
                                        bindings.bindings.insert(
                                            key,
                                            if name == "setup" {
                                                BindingType::SetupMaybeRef
                                            } else {
                                                BindingType::Data
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    bindings
}

fn object_expression_keys(node: &ObjectLit) -> Vec<String> {
    let mut keys = Vec::new();
    for p in &node.props {
        if let PropOrSpread::Prop(prop) = p {
            let key = match &**prop {
                Prop::KeyValue(kv) => resolve_object_key(&kv.key),
                Prop::Method(m) => resolve_object_key(&m.key),
                Prop::Getter(g) => resolve_object_key(&g.key),
                Prop::Setter(s) => resolve_object_key(&s.key),
                Prop::Shorthand(i) => Some(i.sym.to_string()),
                Prop::Assign(_) => None,
            };
            if let Some(k) = key {
                keys.push(k);
            }
        }
    }
    keys
}

fn array_expression_keys(node: &ArrayLit) -> Vec<String> {
    node.elems
        .iter()
        .flatten()
        .filter_map(|e| match &*e.expr {
            Expr::Lit(Lit::Str(s)) => Some(atom(&s.value)),
            _ => None,
        })
        .collect()
}

pub fn object_or_array_keys(value: &Expr) -> Vec<String> {
    match value {
        Expr::Array(a) => array_expression_keys(a),
        Expr::Object(o) => object_expression_keys(o),
        _ => Vec::new(),
    }
}
