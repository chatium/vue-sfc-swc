//! Serializes the AST into the same JSON shape `JSON.stringify` produces for
//! Vue's own AST, so conformance tests can diff the two directly.

use serde_json::{Map, Value, json};

use super::ast::*;

fn loc(l: &SourceLocation) -> Value {
    json!({
        "start": { "column": l.start.column, "line": l.start.line, "offset": l.start.offset },
        "end": { "column": l.end.column, "line": l.end.line, "offset": l.end.offset },
        "source": l.source,
    })
}

fn nodes(v: &[Node]) -> Value {
    Value::Array(v.iter().map(node).collect())
}

fn opt(o: &Option<Node>) -> Option<Value> {
    o.as_ref().map(node)
}

fn insert_opt(m: &mut Map<String, Value>, key: &str, v: Option<Value>) {
    if let Some(v) = v {
        m.insert(key.to_string(), v);
    }
}

pub fn root(r: &RootNode) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!(0));
    m.insert("source".into(), json!(r.source));
    m.insert("children".into(), nodes(&r.children));
    m.insert("helpers".into(), json!({}));
    m.insert("components".into(), json!(r.components));
    m.insert("directives".into(), json!(r.directives));
    m.insert(
        "hoists".into(),
        Value::Array(
            r.hoists
                .iter()
                .map(|h| h.as_ref().map(node).unwrap_or(Value::Null))
                .collect(),
        ),
    );
    m.insert("imports".into(), json!([]));
    m.insert(
        "cached".into(),
        Value::Array(
            r.cached
                .iter()
                .map(|h| h.as_ref().map(node).unwrap_or(Value::Null))
                .collect(),
        ),
    );
    m.insert("temps".into(), json!(r.temps));
    insert_opt(&mut m, "codegenNode", opt(&r.codegen_node));
    m.insert("loc".into(), loc(&r.loc));
    Value::Object(m)
}

pub fn node(n: &Node) -> Value {
    match n {
        Node::Root(r) => root(r),
        Node::Element(e) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(1));
            m.insert("tag".into(), json!(e.tag));
            m.insert("ns".into(), json!(e.ns as u8));
            m.insert("tagType".into(), json!(e.tag_type as u8));
            m.insert("props".into(), nodes(&e.props));
            m.insert("children".into(), nodes(&e.children));
            m.insert("loc".into(), loc(&e.loc));
            if e.is_self_closing {
                m.insert("isSelfClosing".into(), json!(true));
            }
            if let Some(il) = &e.inner_loc {
                m.insert("innerLoc".into(), loc(il));
            }
            insert_opt(&mut m, "codegenNode", opt(&e.codegen_node));
            Value::Object(m)
        }
        Node::Text(t) => json!({ "type": 2, "content": t.content, "loc": loc(&t.loc) }),
        Node::Comment(c) => json!({ "type": 3, "content": c.content, "loc": loc(&c.loc) }),
        Node::SimpleExpression(e) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(4));
            m.insert("loc".into(), loc(&e.loc));
            m.insert("content".into(), json!(e.content));
            m.insert("isStatic".into(), json!(e.is_static));
            m.insert("constType".into(), json!(e.const_type as u8));
            Value::Object(m)
        }
        Node::Interpolation(i) => {
            json!({ "type": 5, "loc": loc(&i.loc), "content": node(&i.content) })
        }
        Node::Attribute(a) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(6));
            m.insert("name".into(), json!(a.name));
            m.insert("nameLoc".into(), loc(&a.name_loc));
            if let Some(v) = &a.value {
                m.insert(
                    "value".into(),
                    json!({ "type": 2, "content": v.content, "loc": loc(&v.loc) }),
                );
            }
            m.insert("loc".into(), loc(&a.loc));
            Value::Object(m)
        }
        Node::Directive(d) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(7));
            m.insert("name".into(), json!(d.name));
            if let Some(r) = &d.raw_name {
                m.insert("rawName".into(), json!(r));
            }
            insert_opt(&mut m, "exp", opt(&d.exp));
            insert_opt(&mut m, "arg", opt(&d.arg));
            m.insert("modifiers".into(), nodes(&d.modifiers));
            m.insert("loc".into(), loc(&d.loc));
            if let Some(f) = &d.for_parse_result {
                let mut fm = Map::new();
                fm.insert("source".into(), node(&f.source));
                insert_opt(&mut fm, "value", opt(&f.value));
                insert_opt(&mut fm, "key", opt(&f.key));
                insert_opt(&mut fm, "index", opt(&f.index));
                fm.insert("finalized".into(), json!(f.finalized));
                m.insert("forParseResult".into(), Value::Object(fm));
            }
            Value::Object(m)
        }
        Node::CompoundExpression(c) => {
            json!({ "type": 8, "loc": loc(&c.loc), "children": nodes(&c.children) })
        }
        Node::If(i) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(9));
            m.insert("loc".into(), loc(&i.loc));
            m.insert("branches".into(), nodes(&i.branches));
            insert_opt(&mut m, "codegenNode", opt(&i.codegen_node));
            Value::Object(m)
        }
        Node::IfBranch(b) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(10));
            m.insert("loc".into(), loc(&b.loc));
            insert_opt(&mut m, "condition", opt(&b.condition));
            m.insert("children".into(), nodes(&b.children));
            insert_opt(&mut m, "userKey", opt(&b.user_key));
            if b.is_template_if {
                m.insert("isTemplateIf".into(), json!(true));
            }
            Value::Object(m)
        }
        Node::For(f) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(11));
            m.insert("loc".into(), loc(&f.loc));
            m.insert("source".into(), node(&f.source));
            insert_opt(&mut m, "valueAlias", opt(&f.value_alias));
            insert_opt(&mut m, "keyAlias", opt(&f.key_alias));
            insert_opt(&mut m, "objectIndexAlias", opt(&f.object_index_alias));
            m.insert("children".into(), nodes(&f.children));
            insert_opt(&mut m, "codegenNode", opt(&f.codegen_node));
            Value::Object(m)
        }
        Node::TextCall(t) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(12));
            m.insert("loc".into(), loc(&t.loc));
            m.insert("content".into(), node(&t.content));
            insert_opt(&mut m, "codegenNode", opt(&t.codegen_node));
            Value::Object(m)
        }
        Node::VNodeCall(v) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(13));
            m.insert("tag".into(), node(&v.tag));
            insert_opt(&mut m, "props", opt(&v.props));
            insert_opt(&mut m, "children", opt(&v.children));
            if let Some(p) = v.patch_flag {
                m.insert("patchFlag".into(), json!(p));
            }
            insert_opt(&mut m, "dynamicProps", opt(&v.dynamic_props));
            insert_opt(&mut m, "directives", opt(&v.directives));
            m.insert("isBlock".into(), json!(v.is_block));
            m.insert("disableTracking".into(), json!(v.disable_tracking));
            m.insert("isComponent".into(), json!(v.is_component));
            m.insert("loc".into(), loc(&v.loc));
            Value::Object(m)
        }
        Node::CallExpression(c) => json!({
            "type": 14,
            "loc": loc(&c.loc),
            "callee": node(&c.callee),
            "arguments": nodes(&c.arguments),
        }),
        Node::ObjectExpression(o) => {
            json!({ "type": 15, "loc": loc(&o.loc), "properties": nodes(&o.properties) })
        }
        Node::Property(p) => json!({
            "type": 16,
            "loc": loc(&p.loc),
            "key": node(&p.key),
            "value": node(&p.value),
        }),
        Node::ArrayExpression(a) => {
            json!({ "type": 17, "loc": loc(&a.loc), "elements": nodes(&a.elements) })
        }
        Node::FunctionExpression(f) => {
            let mut m = Map::new();
            m.insert("type".into(), json!(18));
            insert_opt(&mut m, "params", opt(&f.params));
            insert_opt(&mut m, "returns", opt(&f.returns));
            insert_opt(&mut m, "body", opt(&f.body));
            m.insert("newline".into(), json!(f.newline));
            m.insert("isSlot".into(), json!(f.is_slot));
            m.insert("loc".into(), loc(&f.loc));
            Value::Object(m)
        }
        Node::ConditionalExpression(c) => json!({
            "type": 19,
            "test": node(&c.test),
            "consequent": node(&c.consequent),
            "alternate": node(&c.alternate),
            "newline": c.newline,
            "loc": loc(&c.loc),
        }),
        Node::CacheExpression(c) => json!({
            "type": 20,
            "index": c.index,
            "value": node(&c.value),
            "needPauseTracking": c.need_pause_tracking,
            "inVOnce": c.in_v_once,
            "needArraySpread": c.need_array_spread,
            "loc": loc(&c.loc),
        }),
        Node::Str(s) => json!(s),
        Node::Sym(h) => json!(format!("Symbol({})", h.name())),
        Node::Nodes(v) => nodes(v),
        Node::None => Value::Null,
    }
}
