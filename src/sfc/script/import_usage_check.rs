//! Port of `compiler-sfc/src/script/importUsageCheck.ts`.

use std::collections::HashSet;

use crate::core::ast::*;
use crate::core::js_walk::{KnownIds, Walker};
use crate::core::transform::{camelize, capitalize};
use crate::core::utils::is_built_in_directive;
use crate::dom::tags::{is_html_tag, is_math_ml_tag, is_svg_tag};

pub struct TemplateAnalysis {
    pub used_ids: HashSet<String>,
    pub v_model_ids: HashSet<String>,
}

fn is_native_tag(tag: &str) -> bool {
    is_html_tag(tag) || is_svg_tag(tag) || is_math_ml_tag(tag)
}

fn is_built_in_component(tag: &str) -> bool {
    matches!(
        tag,
        "Transition" | "transition" | "TransitionGroup" | "transition-group"
    )
}

pub fn analyze_template(a: &Arena, children: &[NodeId]) -> TemplateAnalysis {
    let mut res = TemplateAnalysis {
        used_ids: HashSet::new(),
        v_model_ids: HashSet::new(),
    };
    for c in children {
        walk(a, *c, &mut res);
    }
    res
}

fn walk(a: &Arena, node: NodeId, res: &mut TemplateAnalysis) {
    match a.node(node) {
        Node::Element(el) => {
            let mut tag = el.tag.clone();
            if tag.contains('.') {
                tag = tag.split('.').next().unwrap().trim().to_string();
            }
            if !is_native_tag(&tag) && !is_built_in_component(&tag) {
                res.used_ids.insert(camelize(&tag));
                res.used_ids.insert(capitalize(&camelize(&tag)));
            }
            for p in &el.props {
                match a.node(*p) {
                    Node::Directive(d) => {
                        if !is_built_in_directive(&d.name) {
                            res.used_ids
                                .insert(format!("v{}", capitalize(&camelize(&d.name))));
                        }
                        if d.name == "model" {
                            if let Some(exp) = d.exp {
                                if let Node::SimpleExpression(e) = a.node(exp) {
                                    let s = e.content.trim();
                                    if crate::core::utils::is_simple_identifier(s)
                                        && s != "undefined"
                                    {
                                        res.v_model_ids.insert(s.to_string());
                                    }
                                }
                            }
                        }
                        if let Some(arg) = d.arg {
                            if matches!(a.node(arg), Node::SimpleExpression(e) if !e.is_static) {
                                extract_identifiers(a, arg, &mut res.used_ids);
                            }
                        }
                        if d.name == "for" {
                            if let Some(fp) = &d.for_parse_result {
                                extract_identifiers(a, fp.source, &mut res.used_ids);
                            }
                        } else if let Some(exp) = d.exp {
                            extract_identifiers(a, exp, &mut res.used_ids);
                        } else if d.name == "bind" && d.exp.is_none() {
                            if let Some(arg) = d.arg {
                                if let Node::SimpleExpression(e) = a.node(arg) {
                                    res.used_ids.insert(camelize(&e.content));
                                }
                            }
                        }
                    }
                    Node::Attribute(attr) => {
                        if attr.name == "ref" {
                            if let Some(v) = &attr.value {
                                if !v.content.is_empty() {
                                    res.used_ids.insert(v.content.clone());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            for c in &el.children {
                walk(a, *c, res);
            }
        }
        Node::Interpolation(i) => extract_identifiers(a, i.content, &mut res.used_ids),
        _ => {}
    }
}

fn extract_identifiers(a: &Arena, node: NodeId, ids: &mut HashSet<String>) {
    let exp = match a.node(node) {
        Node::SimpleExpression(e) => e,
        _ => return,
    };
    match &exp.ast {
        ExpAst::Null => {
            ids.insert(exp.content.clone());
        }
        ExpAst::Expr(e) => {
            let mut known = KnownIds::default();
            let mut walker = Walker::new(&mut known, false);
            walker.walk_expr(e);
            for r in walker.finish() {
                ids.insert(r.name);
            }
        }
        ExpAst::Program(p) => {
            let mut known = KnownIds::default();
            let mut walker = Walker::new(&mut known, false);
            walker.walk_program(p);
            for r in walker.finish() {
                ids.insert(r.name);
            }
        }
        _ => {}
    }
}
