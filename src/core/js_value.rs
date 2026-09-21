//! A tiny constant-expression evaluator, standing in for the `new Function`
//! call `stringifyStatic` uses (`evaluateConstant`). Only literal expressions
//! reach it, because `constType` already proved them constant.

use swc_core::ecma::ast::*;

/// swc string atoms are WTF-8; lone surrogates cannot appear in template
/// constants, so a lossy conversion is exact here.
fn wtf8(s: &swc_core::atoms::Wtf8Atom) -> String {
    s.as_str().map(|s| s.to_string()).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Undefined,
    Array(Vec<JsValue>),
    Object(Vec<(String, JsValue)>),
}

impl JsValue {
    pub fn is_truthy(&self) -> bool {
        match self {
            JsValue::Str(s) => !s.is_empty(),
            JsValue::Num(n) => *n != 0.0 && !n.is_nan(),
            JsValue::Bool(b) => *b,
            JsValue::Null | JsValue::Undefined => false,
            _ => true,
        }
    }

    /// `String(value)`
    pub fn to_js_string(&self) -> String {
        match self {
            JsValue::Str(s) => s.clone(),
            JsValue::Num(n) => number_to_string(*n),
            JsValue::Bool(b) => b.to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Array(items) => items
                .iter()
                .map(|i| match i {
                    JsValue::Null | JsValue::Undefined => String::new(),
                    other => other.to_js_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            JsValue::Object(_) => "[object Object]".to_string(),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        match self {
            JsValue::Str(s) => serde_json::Value::String(s.clone()),
            JsValue::Num(n) => serde_json::Number::from_f64(*n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            JsValue::Bool(b) => serde_json::Value::Bool(*b),
            JsValue::Null | JsValue::Undefined => serde_json::Value::Null,
            JsValue::Array(items) => {
                serde_json::Value::Array(items.iter().map(|i| i.to_json()).collect())
            }
            JsValue::Object(entries) => {
                let mut m = serde_json::Map::new();
                for (k, v) in entries {
                    m.insert(k.clone(), v.to_json());
                }
                serde_json::Value::Object(m)
            }
        }
    }
}

/// `toDisplayString`
pub fn to_display_string(v: &JsValue) -> String {
    match v {
        JsValue::Str(s) => s.clone(),
        JsValue::Null | JsValue::Undefined => String::new(),
        JsValue::Array(_) | JsValue::Object(_) => {
            serde_json::to_string_pretty(&v.to_json()).unwrap_or_default()
        }
        other => other.to_js_string(),
    }
}

/// JS `String(number)` for the cases that matter here.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == n.trunc() && n.abs() < 1e21 {
        return format!("{}", n as i64);
    }
    format!("{n}")
}

pub fn eval_constant(src: &str) -> Option<JsValue> {
    let expr = crate::core::jsparse::parse_expression(src, true).ok()?;
    eval_expr(&expr)
}

fn eval_expr(e: &Expr) -> Option<JsValue> {
    match e {
        Expr::Lit(l) => Some(match l {
            Lit::Str(s) => JsValue::Str(wtf8(&s.value)),
            Lit::Bool(b) => JsValue::Bool(b.value),
            Lit::Null(_) => JsValue::Null,
            Lit::Num(n) => JsValue::Num(n.value),
            Lit::BigInt(b) => JsValue::Str(b.value.to_string()),
            Lit::Regex(_) => return None,
            Lit::JSXText(_) => return None,
        }),
        Expr::Ident(i) => match &*i.sym {
            "undefined" => Some(JsValue::Undefined),
            "NaN" => Some(JsValue::Num(f64::NAN)),
            "Infinity" => Some(JsValue::Num(f64::INFINITY)),
            _ => None,
        },
        Expr::Paren(p) => eval_expr(&p.expr),
        Expr::TsAs(t) => eval_expr(&t.expr),
        Expr::TsNonNull(t) => eval_expr(&t.expr),
        Expr::TsSatisfies(t) => eval_expr(&t.expr),
        Expr::TsConstAssertion(t) => eval_expr(&t.expr),
        Expr::TsTypeAssertion(t) => eval_expr(&t.expr),
        Expr::Array(a) => {
            let mut items = Vec::new();
            for el in &a.elems {
                match el {
                    Some(e) if e.spread.is_none() => items.push(eval_expr(&e.expr)?),
                    None => items.push(JsValue::Undefined),
                    _ => return None,
                }
            }
            Some(JsValue::Array(items))
        }
        Expr::Object(o) => {
            let mut entries = Vec::new();
            for p in &o.props {
                match p {
                    PropOrSpread::Prop(prop) => match &**prop {
                        Prop::KeyValue(kv) => {
                            let key = prop_name(&kv.key)?;
                            entries.push((key, eval_expr(&kv.value)?));
                        }
                        Prop::Shorthand(_) => return None,
                        _ => return None,
                    },
                    PropOrSpread::Spread(_) => return None,
                }
            }
            Some(JsValue::Object(entries))
        }
        Expr::Tpl(t) => {
            if !t.exprs.is_empty() {
                return None;
            }
            let s = t
                .quasis
                .iter()
                .map(|q| {
                    q.cooked
                        .as_ref()
                        .map(|c| wtf8(c))
                        .unwrap_or_else(|| q.raw.to_string())
                })
                .collect::<String>();
            Some(JsValue::Str(s))
        }
        Expr::Unary(u) => {
            let v = eval_expr(&u.arg)?;
            Some(match u.op {
                UnaryOp::Minus => JsValue::Num(-to_number(&v)),
                UnaryOp::Plus => JsValue::Num(to_number(&v)),
                UnaryOp::Bang => JsValue::Bool(!v.is_truthy()),
                UnaryOp::Void => JsValue::Undefined,
                _ => return None,
            })
        }
        Expr::Bin(b) => {
            let l = eval_expr(&b.left)?;
            let r = eval_expr(&b.right)?;
            Some(match b.op {
                BinaryOp::Add => match (&l, &r) {
                    (JsValue::Str(_), _) | (_, JsValue::Str(_)) => {
                        JsValue::Str(format!("{}{}", l.to_js_string(), r.to_js_string()))
                    }
                    _ => JsValue::Num(to_number(&l) + to_number(&r)),
                },
                BinaryOp::Sub => JsValue::Num(to_number(&l) - to_number(&r)),
                BinaryOp::Mul => JsValue::Num(to_number(&l) * to_number(&r)),
                BinaryOp::Div => JsValue::Num(to_number(&l) / to_number(&r)),
                BinaryOp::Mod => JsValue::Num(to_number(&l) % to_number(&r)),
                _ => return None,
            })
        }
        Expr::Cond(c) => {
            let test = eval_expr(&c.test)?;
            if test.is_truthy() {
                eval_expr(&c.cons)
            } else {
                eval_expr(&c.alt)
            }
        }
        _ => None,
    }
}

fn prop_name(p: &PropName) -> Option<String> {
    Some(match p {
        PropName::Ident(i) => i.sym.to_string(),
        PropName::Str(s) => wtf8(&s.value),
        PropName::Num(n) => number_to_string(n.value),
        _ => return None,
    })
}

fn to_number(v: &JsValue) -> f64 {
    match v {
        JsValue::Num(n) => *n,
        JsValue::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        JsValue::Null => 0.0,
        JsValue::Str(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

/// `normalizeClass`
pub fn normalize_class(v: &JsValue) -> String {
    let mut res = String::new();
    match v {
        JsValue::Str(s) => res = s.clone(),
        JsValue::Array(items) => {
            for item in items {
                let n = normalize_class(item);
                if !n.is_empty() {
                    res.push_str(&n);
                    res.push(' ');
                }
            }
        }
        JsValue::Object(entries) => {
            for (k, val) in entries {
                if val.is_truthy() {
                    res.push_str(k);
                    res.push(' ');
                }
            }
        }
        _ => {}
    }
    res.trim().to_string()
}

/// `normalizeStyle` + `stringifyStyle`
pub fn stringify_normalized_style(v: &JsValue) -> String {
    match normalize_style(v) {
        Some(JsValue::Str(s)) => s,
        Some(JsValue::Object(entries)) => {
            let mut ret = String::new();
            for (k, val) in entries {
                let is_renderable = matches!(val, JsValue::Str(_) | JsValue::Num(_));
                if is_renderable {
                    let key = if k.starts_with("--") {
                        k.clone()
                    } else {
                        crate::core::transform::hyphenate(&k)
                    };
                    ret.push_str(&format!("{key}:{};", val.to_js_string()));
                }
            }
            ret
        }
        _ => String::new(),
    }
}

fn normalize_style(v: &JsValue) -> Option<JsValue> {
    match v {
        JsValue::Array(items) => {
            let mut res: Vec<(String, JsValue)> = Vec::new();
            for item in items {
                let normalized = match item {
                    JsValue::Str(s) => Some(JsValue::Object(parse_string_style(s))),
                    other => normalize_style(other),
                };
                if let Some(JsValue::Object(entries)) = normalized {
                    for (k, val) in entries {
                        match res.iter_mut().find(|(ek, _)| *ek == k) {
                            Some(slot) => slot.1 = val,
                            None => res.push((k, val)),
                        }
                    }
                }
            }
            Some(JsValue::Object(res))
        }
        JsValue::Str(_) | JsValue::Object(_) => Some(v.clone()),
        _ => None,
    }
}

fn parse_string_style(css: &str) -> Vec<(String, JsValue)> {
    let mut ret = Vec::new();
    for item in css.split(';') {
        if item.is_empty() {
            continue;
        }
        if let Some(idx) = item.find(':') {
            let key = item[..idx].trim().to_string();
            let value = item[idx + 1..].trim().to_string();
            ret.push((key, JsValue::Str(value)));
        }
    }
    ret
}
