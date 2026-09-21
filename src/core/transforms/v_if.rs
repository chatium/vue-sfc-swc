//! Port of `compiler-core/src/transforms/vIf.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::patch_flags as pf;
use crate::core::transform::{
    ExitFn, TransformContext, convert_to_block, create_vnode_call, traverse_node,
};
use crate::core::utils::{
    find_dir, find_prop, get_memoed_vnode_call, inject_prop, is_comment_or_whitespace,
};

use super::transform_expression::process_expression;

pub fn transform_if(node: NodeId, dir: NodeId, ctx: &mut TransformContext) -> Vec<ExitFn> {
    let name = ctx.a.dir(dir).name.clone();
    let exp = ctx.a.dir(dir).exp;
    let empty_exp = exp
        .map(|e| {
            matches!(ctx.a.node(e), Node::SimpleExpression(s) if s.content.trim().is_empty())
        })
        .unwrap_or(true);
    if name != "else" && empty_exp {
        let loc = match exp {
            Some(e) => ctx.a.loc(e).clone(),
            None => ctx.a.loc(node).clone(),
        };
        let dir_loc = ctx.a.loc(dir).clone();
        ctx.error(ErrorCode::X_V_IF_NO_EXPRESSION, Some(dir_loc));
        let e = ctx
            .a
            .create_simple_expression("true", false, loc, ConstantType::NotConstant);
        ctx.a.dir_mut(dir).exp = Some(e);
    }

    if ctx.opts.prefix_identifiers {
        if let Some(exp) = ctx.a.dir(dir).exp {
            let processed = process_expression(exp, ctx, false, false, None);
            ctx.a.dir_mut(dir).exp = Some(processed);
        }
    }

    if name == "if" {
        let branch = create_if_branch(node, dir, ctx);
        let loc = ctx.a.loc(node).clone();
        let if_node = ctx.a.add(Node::If(Box::new(IfNode {
            branches: vec![branch],
            codegen_node: None,
            loc,
        })));
        ctx.replace_node(if_node);
        let key = sibling_key(if_node, ctx);
        vec![ExitFn::IfRoot {
            if_node,
            branch,
            key,
        }]
    } else {
        process_else(node, dir, &name, ctx);
        Vec::new()
    }
}

/// `#1587` — the branch key depends on preceding sibling `v-if` chains.
fn sibling_key(if_node: NodeId, ctx: &TransformContext) -> usize {
    let parent = match ctx.parent {
        Some(p) => p,
        None => return 0,
    };
    let siblings = ctx.a.children_of(parent);
    // `while (i-- >= 0)` starts one before the node itself
    let mut i = siblings
        .iter()
        .position(|c| *c == if_node)
        .map(|i| i as i64)
        .unwrap_or(-1)
        - 1;
    let mut key = 0usize;
    while i >= 0 {
        let sibling = siblings[i as usize];
        if ctx.a.is(sibling, NodeType::If) {
            key += ctx.a.if_node(sibling).branches.len();
        }
        i -= 1;
    }
    key
}

fn process_else(node: NodeId, dir: NodeId, name: &str, ctx: &mut TransformContext) {
    let parent = match ctx.parent {
        Some(p) => p,
        None => return,
    };
    let mut comments: Vec<NodeId> = Vec::new();
    let mut i = ctx
        .a
        .children_of(parent)
        .iter()
        .position(|c| *c == node)
        .map(|i| i as i64)
        .unwrap_or(-1);
    loop {
        i -= 1;
        if i < -1 {
            break;
        }
        let sibling = if i >= 0 {
            ctx.a.children_of(parent).get(i as usize).copied()
        } else {
            None
        };
        if let Some(sibling) = sibling {
            if is_comment_or_whitespace(&ctx.a, sibling) {
                ctx.remove_node(Some(sibling));
                if ctx.a.is(sibling, NodeType::Comment) {
                    comments.insert(0, sibling);
                }
                continue;
            }
            if ctx.a.is(sibling, NodeType::If) {
                let last_branch = *ctx.a.if_node(sibling).branches.last().unwrap();
                if (name == "else-if" || name == "else")
                    && ctx.a.branch(last_branch).condition.is_none()
                {
                    let loc = ctx.a.loc(node).clone();
                    ctx.error(ErrorCode::X_V_ELSE_NO_ADJACENT_IF, Some(loc));
                }
                ctx.remove_node(None);
                let branch = create_if_branch(node, dir, ctx);
                let parent_is_transition = ctx.a.is(parent, NodeType::Element)
                    && (ctx.a.el(parent).tag == "transition"
                        || ctx.a.el(parent).tag == "Transition");
                if !comments.is_empty() && !parent_is_transition {
                    let mut children = comments.clone();
                    children.extend(ctx.a.branch(branch).children.clone());
                    ctx.a.branch_mut(branch).children = children;
                }

                let key = ctx.a.branch(branch).user_key;
                if let Some(key) = key {
                    let existing: Vec<Option<NodeId>> = ctx
                        .a
                        .if_node(sibling)
                        .branches
                        .iter()
                        .map(|b| ctx.a.branch(*b).user_key)
                        .collect();
                    for other in existing {
                        if is_same_key(&ctx.a, other, key) {
                            let loc = ctx.a.loc(key).clone();
                            ctx.error(ErrorCode::X_V_IF_SAME_KEY, Some(loc));
                        }
                    }
                }

                ctx.a.if_node_mut(sibling).branches.push(branch);
                let key_index = sibling_key(sibling, ctx);

                // the branch was removed from the tree, so traverse it here
                let saved_parent = ctx.parent;
                let saved_index = ctx.child_index;
                traverse_node(branch, ctx);
                ctx.parent = saved_parent;
                ctx.child_index = saved_index;

                exit_if_branch(sibling, branch, key_index, ctx);
                ctx.current_node = None;
            } else {
                let loc = ctx.a.loc(node).clone();
                ctx.error(ErrorCode::X_V_ELSE_NO_ADJACENT_IF, Some(loc));
            }
        } else {
            let loc = ctx.a.loc(node).clone();
            ctx.error(ErrorCode::X_V_ELSE_NO_ADJACENT_IF, Some(loc));
        }
        break;
    }
}

fn create_if_branch(node: NodeId, dir: NodeId, ctx: &mut TransformContext) -> NodeId {
    let is_template_if = ctx.a.el(node).tag_type == ElementType::Template;
    let condition = if ctx.a.dir(dir).name == "else" {
        None
    } else {
        ctx.a.dir(dir).exp
    };
    let children = if is_template_if && find_dir(&ctx.a, node, "for", false).is_none() {
        ctx.a.el(node).children.clone()
    } else {
        vec![node]
    };
    let user_key = find_prop(&ctx.a, node, "key", false, false);
    let loc = ctx.a.loc(node).clone();
    ctx.a.add(Node::IfBranch(Box::new(IfBranchNode {
        condition,
        children,
        user_key,
        is_template_if,
        loc,
    })))
}

pub fn exit_if_root(if_node: NodeId, branch: NodeId, key: usize, ctx: &mut TransformContext) {
    let codegen = create_codegen_node_for_branch(branch, key, ctx);
    ctx.a.if_node_mut(if_node).codegen_node = Some(codegen);
}

fn exit_if_branch(if_node: NodeId, branch: NodeId, key: usize, ctx: &mut TransformContext) {
    let branches_len = ctx.a.if_node(if_node).branches.len();
    let codegen = create_codegen_node_for_branch(branch, key + branches_len - 1, ctx);
    let root_codegen = ctx.a.if_node(if_node).codegen_node.unwrap();
    let parent_condition = get_parent_condition(&ctx.a, root_codegen);
    ctx.a.cond_mut(parent_condition).alternate = codegen;
}

fn create_codegen_node_for_branch(
    branch: NodeId,
    key_index: usize,
    ctx: &mut TransformContext,
) -> NodeId {
    let condition = ctx.a.branch(branch).condition;
    match condition {
        Some(condition) => {
            let consequent = create_children_codegen_node(branch, key_index, ctx);
            let helper = ctx.helper_node(RuntimeHelper::CREATE_COMMENT);
            let a1 = ctx.a.string("\"v-if\"");
            let a2 = ctx.a.string("true");
            let alternate = ctx.a.create_call_expression(helper, vec![a1, a2]);
            ctx.a
                .create_conditional_expression(condition, consequent, alternate, true)
        }
        None => create_children_codegen_node(branch, key_index, ctx),
    }
}

fn create_children_codegen_node(
    branch: NodeId,
    key_index: usize,
    ctx: &mut TransformContext,
) -> NodeId {
    let key_value = ctx.a.create_simple_expression(
        key_index.to_string(),
        false,
        loc_stub(),
        ConstantType::CanCache,
    );
    let key_property = ctx.a.create_object_property_str("key", key_value);
    let children = ctx.a.branch(branch).children.clone();
    let first_child = children.first().copied();
    let need_fragment_wrapper = children.len() != 1
        || first_child
            .map(|c| !ctx.a.is(c, NodeType::Element))
            .unwrap_or(true);
    if need_fragment_wrapper {
        if children.len() == 1 && ctx.a.is(children[0], NodeType::For) {
            let vnode_call = ctx.a.for_node(children[0]).codegen_node.unwrap();
            let merge = ctx.a.sym(RuntimeHelper::MERGE_PROPS);
            if inject_prop(&mut ctx.a, vnode_call, key_property, merge) {
                ctx.helper(RuntimeHelper::MERGE_PROPS);
            }
            vnode_call
        } else {
            let mut patch_flag = pf::STABLE_FRAGMENT;
            if !ctx.a.branch(branch).is_template_if
                && children
                    .iter()
                    .filter(|c| !ctx.a.is(**c, NodeType::Comment))
                    .count()
                    == 1
            {
                patch_flag |= pf::DEV_ROOT_FRAGMENT;
            }
            let tag = ctx.helper_node(RuntimeHelper::FRAGMENT);
            let props = ctx.a.create_object_expression(vec![key_property]);
            let children_ref = ctx.a.children_ref(branch);
            let loc = ctx.a.branch(branch).loc.clone();
            create_vnode_call(
                Some(ctx),
                tag,
                Some(props),
                Some(children_ref),
                Some(patch_flag),
                None,
                None,
                true,
                false,
                false,
                loc,
            )
        }
    } else {
        let ret = ctx.a.el(children[0]).codegen_node.unwrap();
        let vnode_call = get_memoed_vnode_call(&ctx.a, ret);
        if ctx.a.is(vnode_call, NodeType::VNodeCall) {
            convert_to_block(vnode_call, ctx);
        }
        let merge = ctx.a.sym(RuntimeHelper::MERGE_PROPS);
        if inject_prop(&mut ctx.a, vnode_call, key_property, merge) {
            ctx.helper(RuntimeHelper::MERGE_PROPS);
        }
        ret
    }
}

fn is_same_key(a: &Arena, x: Option<NodeId>, y: NodeId) -> bool {
    let x = match x {
        Some(x) => x,
        None => return false,
    };
    if a.node_type(x) != a.node_type(y) {
        return false;
    }
    match (a.node(x), a.node(y)) {
        (Node::Attribute(ax), Node::Attribute(ay)) => {
            match (&ax.value, &ay.value) {
                (Some(vx), Some(vy)) => vx.content == vy.content,
                _ => false,
            }
        }
        (Node::Directive(dx), Node::Directive(dy)) => {
            let (ex, ey) = match (dx.exp, dy.exp) {
                (Some(ex), Some(ey)) => (ex, ey),
                _ => return false,
            };
            if a.node_type(ex) != a.node_type(ey) {
                return false;
            }
            match (a.node(ex), a.node(ey)) {
                (Node::SimpleExpression(sx), Node::SimpleExpression(sy)) => {
                    sx.is_static == sy.is_static && sx.content == sy.content
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn get_parent_condition(a: &Arena, node: NodeId) -> NodeId {
    let mut node = node;
    loop {
        match a.node_type(node) {
            NodeType::JsConditionalExpression => {
                let alt = a.cond(node).alternate;
                if a.is(alt, NodeType::JsConditionalExpression) {
                    node = alt;
                } else {
                    return node;
                }
            }
            NodeType::JsCacheExpression => {
                node = a.cache(node).value;
            }
            _ => return node,
        }
    }
}
