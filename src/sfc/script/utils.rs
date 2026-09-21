//! Port of `compiler-sfc/src/script/utils.ts`.

use swc_core::ecma::ast::*;

pub const UNKNOWN_TYPE: &str = "Unknown";

pub fn atom(s: &swc_core::atoms::Wtf8Atom) -> String {
    s.as_str().map(|s| s.to_string()).unwrap_or_default()
}

pub fn resolve_object_key(key: &PropName) -> Option<String> {
    match key {
        PropName::Str(s) => Some(atom(&s.value)),
        PropName::Num(n) => Some(crate::core::js_value::number_to_string(n.value)),
        PropName::Ident(i) => Some(i.sym.to_string()),
        PropName::BigInt(b) => Some(b.value.to_string()),
        PropName::Computed(_) => None,
    }
}

pub fn concat_strings(strs: Vec<Option<String>>) -> String {
    strs.into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn is_literal_node(node: &Expr) -> bool {
    matches!(node, Expr::Lit(_))
}

/// `isCallOf(node, name)`
pub fn is_call_of(node: Option<&Expr>, test: impl Fn(&str) -> bool) -> bool {
    match node {
        Some(Expr::Call(c)) => match &c.callee {
            Callee::Expr(e) => matches!(&**e, Expr::Ident(i) if test(&i.sym)),
            _ => false,
        },
        _ => false,
    }
}

pub fn as_call<'a>(node: &'a Expr, test: impl Fn(&str) -> bool) -> Option<&'a CallExpr> {
    match node {
        Expr::Call(c) => match &c.callee {
            Callee::Expr(e) => match &**e {
                Expr::Ident(i) if test(&i.sym) => Some(c),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

pub fn to_runtime_type_string(types: &[String]) -> String {
    if types.len() > 1 {
        format!("[{}]", types.join(", "))
    } else {
        types.first().cloned().unwrap_or_default()
    }
}

pub fn unwrap_ts_node(e: &Expr) -> &Expr {
    match e {
        Expr::TsAs(t) => unwrap_ts_node(&t.expr),
        Expr::TsTypeAssertion(t) => unwrap_ts_node(&t.expr),
        Expr::TsNonNull(t) => unwrap_ts_node(&t.expr),
        Expr::TsInstantiation(t) => unwrap_ts_node(&t.expr),
        Expr::TsSatisfies(t) => unwrap_ts_node(&t.expr),
        other => other,
    }
}

const PROP_NAME_ESCAPE_SYMBOLS: &str = " !\"#$%&'()*+,./:;<=>?@[\\]^`{|}~-";

pub fn get_escaped_prop_name(key: &str) -> String {
    if key.chars().any(|c| PROP_NAME_ESCAPE_SYMBOLS.contains(c)) {
        serde_json::to_string(key).unwrap()
    } else {
        key.to_string()
    }
}

pub fn get_imported_name(specifier: &ImportSpecifier) -> String {
    match specifier {
        ImportSpecifier::Named(n) => match &n.imported {
            Some(ModuleExportName::Ident(i)) => i.sym.to_string(),
            Some(ModuleExportName::Str(s)) => atom(&s.value),
            None => n.local.sym.to_string(),
        },
        ImportSpecifier::Namespace(_) => "*".to_string(),
        ImportSpecifier::Default(_) => "default".to_string(),
    }
}
