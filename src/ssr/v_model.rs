//! `ssrTransformModel`.

use crate::core::ast::*;
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::TransformContext;
use crate::core::utils::{find_prop, has_dynamic_key_v_bind};

use super::ssr_helper;

pub struct SsrModelResult {
    pub props: Vec<NodeId>,
    pub ssr_tag_parts: Vec<NodeId>,
}

fn find_value_binding(node: NodeId, ctx: &mut TransformContext) -> NodeId {
    match find_prop(&ctx.a, node, "value", false, false) {
        Some(p) => match ctx.a.node(p) {
            Node::Directive(d) => d.exp.unwrap(),
            Node::Attribute(a) => {
                let content = a.value.as_ref().map(|v| v.content.clone()).unwrap_or_default();
                ctx.a.simple_exp(content, true)
            }
            _ => ctx.a.simple_exp("null", false),
        },
        None => ctx.a.simple_exp("null", false),
    }
}

fn check_duplicated_value(node: NodeId, ctx: &mut TransformContext) {
    if let Some(value) = find_prop(&ctx.a, node, "value", false, false) {
        let loc = ctx.a.loc(value).clone();
        let e = create_dom_compiler_error(DomErrorCode::X_V_MODEL_UNNECESSARY_VALUE, Some(loc));
        ctx.on_error(e);
    }
}

pub fn ssr_transform_model(
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
) -> SsrModelResult {
    let model = match ctx.a.dir(dir).exp {
        Some(e) => e,
        None => ctx.a.simple_exp("", false),
    };
    if ctx.a.el(node).tag_type != ElementType::Element {
        let r = crate::core::transforms::v_model::transform_model(dir, node, ctx);
        return SsrModelResult {
            props: r.props,
            ssr_tag_parts: Vec::new(),
        };
    }

    let tag = ctx.a.el(node).tag.clone();
    let mut res = SsrModelResult {
        props: Vec::new(),
        ssr_tag_parts: Vec::new(),
    };
    match tag.as_str() {
        "input" => {
            let type_prop = find_prop(&ctx.a, node, "type", false, false);
            match type_prop {
                Some(type_prop) => {
                    let value = find_value_binding(node, ctx);
                    if ctx.a.is(type_prop, NodeType::Directive) {
                        let exp = ctx.a.dir(type_prop).exp.unwrap();
                        let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_DYNAMIC_MODEL);
                        let call = ctx
                            .a
                            .create_call_expression(helper, vec![exp, model, value]);
                        res.ssr_tag_parts = vec![call];
                    } else if let Some(v) = ctx.a.attr(type_prop).value.clone() {
                        match v.content.as_str() {
                            "radio" => {
                                let helper = ssr_helper(ctx, RuntimeHelper::SSR_LOOSE_EQUAL);
                                let call = ctx
                                    .a
                                    .create_call_expression(helper, vec![model, value]);
                                res.props =
                                    vec![ctx.a.create_object_property_str("checked", call)];
                            }
                            "checkbox" => {
                                let true_value_binding =
                                    find_prop(&ctx.a, node, "true-value", false, false);
                                match true_value_binding {
                                    Some(b) => {
                                        let true_value = match ctx.a.node(b) {
                                            Node::Attribute(a) => {
                                                let c = a
                                                    .value
                                                    .as_ref()
                                                    .map(|v| v.content.clone())
                                                    .unwrap_or_default();
                                                let s =
                                                    serde_json::to_string(&c).unwrap();
                                                ctx.a.string(s)
                                            }
                                            _ => ctx.a.dir(b).exp.unwrap(),
                                        };
                                        let helper =
                                            ssr_helper(ctx, RuntimeHelper::SSR_LOOSE_EQUAL);
                                        let call = ctx.a.create_call_expression(
                                            helper,
                                            vec![model, true_value],
                                        );
                                        res.props = vec![
                                            ctx.a.create_object_property_str("checked", call),
                                        ];
                                    }
                                    None => {
                                        let is_array = ctx.a.string("Array.isArray");
                                        let test = ctx
                                            .a
                                            .create_call_expression(is_array, vec![model]);
                                        let helper =
                                            ssr_helper(ctx, RuntimeHelper::SSR_LOOSE_CONTAIN);
                                        let consequent = ctx
                                            .a
                                            .create_call_expression(helper, vec![model, value]);
                                        let cond = ctx.a.create_conditional_expression(
                                            test, consequent, model, true,
                                        );
                                        res.props = vec![
                                            ctx.a.create_object_property_str("checked", cond),
                                        ];
                                    }
                                }
                            }
                            "file" => {
                                let loc = ctx.a.loc(dir).clone();
                                let e = create_dom_compiler_error(
                                    DomErrorCode::X_V_MODEL_ON_FILE_INPUT_ELEMENT,
                                    Some(loc),
                                );
                                ctx.on_error(e);
                            }
                            _ => {
                                check_duplicated_value(node, ctx);
                                res.props =
                                    vec![ctx.a.create_object_property_str("value", model)];
                            }
                        }
                    }
                }
                None => {
                    if !has_dynamic_key_v_bind(&ctx.a, node) {
                        check_duplicated_value(node, ctx);
                        res.props = vec![ctx.a.create_object_property_str("value", model)];
                    }
                }
            }
        }
        "textarea" => {
            check_duplicated_value(node, ctx);
            let loc = ctx.a.loc(model).clone();
            let interp = ctx.a.create_interpolation(model, loc);
            ctx.a.el_mut(node).children = vec![interp];
        }
        "select" => {
            let children = ctx.a.el(node).children.clone();
            process_select_children(&children, model, ctx);
        }
        _ => {
            let loc = ctx.a.loc(dir).clone();
            let e = create_dom_compiler_error(
                DomErrorCode::X_V_MODEL_ON_INVALID_ELEMENT,
                Some(loc),
            );
            ctx.on_error(e);
        }
    }
    res
}

fn process_select_children(children: &[NodeId], model: NodeId, ctx: &mut TransformContext) {
    for child in children {
        match ctx.a.node_type(*child) {
            NodeType::Element => process_option(*child, model, ctx),
            NodeType::For => {
                let c = ctx.a.for_node(*child).children.clone();
                process_select_children(&c, model, ctx);
            }
            NodeType::If => {
                for b in ctx.a.if_node(*child).branches.clone() {
                    let c = ctx.a.branch(b).children.clone();
                    process_select_children(&c, model, ctx);
                }
            }
            _ => {}
        }
    }
}

fn process_option(node: NodeId, model: NodeId, ctx: &mut TransformContext) {
    let tag = ctx.a.el(node).tag.clone();
    if tag == "option" {
        let has_selected = ctx.a.el(node).props.iter().any(|p| match ctx.a.node(*p) {
            Node::Attribute(a) => a.name == "selected",
            Node::Directive(d) => d.name == "selected",
            _ => false,
        });
        if has_selected {
            return;
        }
        let include = ssr_helper(ctx, RuntimeHelper::SSR_INCLUDE_BOOLEAN_ATTR);
        let value = find_value_binding(node, ctx);
        let is_array = ctx.a.string("Array.isArray");
        let test = ctx.a.create_call_expression(is_array, vec![model]);
        let contain = ssr_helper(ctx, RuntimeHelper::SSR_LOOSE_CONTAIN);
        let consequent = ctx.a.create_call_expression(contain, vec![model, value]);
        let equal = ssr_helper(ctx, RuntimeHelper::SSR_LOOSE_EQUAL);
        let alternate = ctx.a.create_call_expression(equal, vec![model, value]);
        let inner = ctx
            .a
            .create_conditional_expression(test, consequent, alternate, true);
        let call = ctx.a.create_call_expression(include, vec![inner]);
        let yes = ctx.a.simple_exp(" selected", true);
        let no = ctx.a.simple_exp("", true);
        let cond = ctx.a.create_conditional_expression(call, yes, no, false);
        let lit = ctx.a.el(node).ssr_codegen_node.unwrap();
        match ctx.a.node_mut(lit) {
            Node::TemplateLiteral(e) => e.push(cond),
            _ => unreachable!(),
        }
    } else if tag == "optgroup" {
        let children = ctx.a.el(node).children.clone();
        process_select_children(&children, model, ctx);
    }
}
