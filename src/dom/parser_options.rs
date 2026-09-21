//! Port of `compiler-dom/src/parserOptions.ts`.

use crate::core::ast::{Arena, ElementNode, Namespace, Node, RuntimeHelper};
use crate::core::parser::{ParseMode, ParserOptions};

use super::tags::{is_html_tag, is_math_ml_tag, is_svg_tag, is_void_tag};

fn is_native_tag(tag: &str) -> bool {
    is_html_tag(tag) || is_svg_tag(tag) || is_math_ml_tag(tag)
}

fn is_pre_tag(tag: &str) -> bool {
    tag == "pre"
}

fn is_ignore_newline_tag(tag: &str) -> bool {
    tag == "pre" || tag == "textarea"
}

fn is_built_in_component(tag: &str) -> Option<RuntimeHelper> {
    match tag {
        "Transition" | "transition" => Some(RuntimeHelper::TRANSITION),
        "TransitionGroup" | "transition-group" => Some(RuntimeHelper::TRANSITION_GROUP),
        _ => None,
    }
}

/// `/^m(?:[ions]|text)$/`
fn is_math_text_integration_point(tag: &str) -> bool {
    matches!(tag, "mi" | "mo" | "mn" | "ms" | "mtext")
}

fn get_namespace(
    a: &Arena,
    tag: &str,
    parent: Option<&ElementNode>,
    root_ns: Namespace,
) -> Namespace {
    let mut ns = parent.map(|p| p.ns).unwrap_or(root_ns);
    if let Some(parent) = parent {
        if ns == Namespace::MathMl {
            if parent.tag == "annotation-xml" {
                if tag == "svg" {
                    return Namespace::Svg;
                }
                let html_encoding = parent.props.iter().any(|p| match a.node(*p) {
                    Node::Attribute(attr) => {
                        attr.name == "encoding"
                            && attr.value.as_ref().is_some_and(|v| {
                                v.content == "text/html" || v.content == "application/xhtml+xml"
                            })
                    }
                    _ => false,
                });
                if html_encoding {
                    ns = Namespace::Html;
                }
            } else if is_math_text_integration_point(&parent.tag)
                && tag != "mglyph"
                && tag != "malignmark"
            {
                ns = Namespace::Html;
            }
        } else if ns == Namespace::Svg
            && (parent.tag == "foreignObject" || parent.tag == "desc" || parent.tag == "title")
        {
            ns = Namespace::Html;
        }
    }
    if ns == Namespace::Html {
        if tag == "svg" {
            return Namespace::Svg;
        }
        if tag == "math" {
            return Namespace::MathMl;
        }
    }
    ns
}

pub fn dom_parser_options() -> ParserOptions {
    ParserOptions {
        parse_mode: ParseMode::Html,
        is_void_tag,
        is_native_tag: Some(is_native_tag),
        is_pre_tag,
        is_ignore_newline_tag,
        is_built_in_component: Some(is_built_in_component),
        get_namespace,
        ..Default::default()
    }
}
