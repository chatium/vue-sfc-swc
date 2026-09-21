//! Port of `compiler-sfc/src/style/pluginScoped.ts`.

use std::collections::{HashMap, HashSet};

use super::postcss::node::{CssKind, CssNode, CssTree};
use super::selector::{SelKind, SelNode, SelRoot, Selector, parse as parse_selector};

fn is_keyframes(name: &str) -> bool {
    // /^(?:-\w+-)?keyframes$/
    let n = name.strip_prefix('-').unwrap_or(name);
    if name.starts_with('-') {
        match n.find('-') {
            Some(i) => &n[i + 1..] == "keyframes",
            None => false,
        }
    } else {
        name == "keyframes"
    }
}

fn is_animation_name(prop: &str) -> bool {
    matches!(strip_vendor(prop), "animation-name")
}

fn is_animation(prop: &str) -> bool {
    matches!(strip_vendor(prop), "animation")
}

fn strip_vendor(prop: &str) -> &str {
    if let Some(rest) = prop.strip_prefix('-') {
        if let Some(i) = rest.find('-') {
            return &rest[i + 1..];
        }
    }
    prop
}

pub fn scoped_plugin(tree: &mut CssTree, id: &str) {
    let short_id = id.strip_prefix("data-v-").unwrap_or(id).to_string();
    let mut keyframes: HashMap<String, String> = HashMap::new();
    let mut processed: HashSet<usize> = HashSet::new();
    let mut deep_rules: HashSet<usize> = HashSet::new();

    // postcss re-visits nodes a plugin adds during the walk, so iterate until
    // everything (including wrapped `&` rules) has been processed
    let mut seen_atrules: HashSet<usize> = HashSet::new();
    loop {
        let ids = tree.walk_ids(tree.root);
        let pending: Vec<usize> = ids
            .iter()
            .copied()
            .filter(|i| {
                let k = tree.get(*i).kind;
                (k == CssKind::Rule && !processed.contains(i))
                    || (k == CssKind::AtRule && !seen_atrules.contains(i))
            })
            .collect();
        if pending.is_empty() {
            break;
        }
        for node_id in pending {
            match tree.get(node_id).kind {
                CssKind::Rule => {
                    process_rule(tree, id, node_id, &mut processed, &mut deep_rules);
                }
                CssKind::AtRule => {
                    seen_atrules.insert(node_id);
                    let name = tree.get(node_id).name.clone();
                    let params = tree.get(node_id).params.clone();
                    if is_keyframes(&name) && !params.ends_with(&format!("-{short_id}")) {
                        let new = format!("{params}-{short_id}");
                        keyframes.insert(params, new.clone());
                        tree.get_mut(node_id).params = new;
                        tree.get_mut(node_id).raws.params = None;
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(unreachable_code)]
    for node_id in Vec::<usize>::new() {
        match tree.get(node_id).kind {
            CssKind::Rule => {
                process_rule(tree, id, node_id, &mut processed, &mut deep_rules);
            }
            CssKind::AtRule => {
                let name = tree.get(node_id).name.clone();
                let params = tree.get(node_id).params.clone();
                if is_keyframes(&name) && !params.ends_with(&format!("-{short_id}")) {
                    let new = format!("{params}-{short_id}");
                    keyframes.insert(params, new.clone());
                    tree.get_mut(node_id).params = new;
                    tree.get_mut(node_id).raws.params = None;
                }
            }
            _ => {}
        }
    }

    if !keyframes.is_empty() {
        for node_id in tree.walk_ids(tree.root) {
            if tree.get(node_id).kind != CssKind::Decl {
                continue;
            }
            let prop = tree.get(node_id).prop.clone();
            let value = tree.get(node_id).value.clone();
            if is_animation_name(&prop) {
                let new = value
                    .split(',')
                    .map(|v| {
                        let t = v.trim();
                        keyframes.get(t).cloned().unwrap_or_else(|| t.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                tree.get_mut(node_id).value = new;
                tree.get_mut(node_id).raws.value = None;
            }
            if is_animation(&prop) {
                let new = value
                    .split(',')
                    .map(|v| {
                        let vals: Vec<&str> = v.trim().split_whitespace().collect();
                        match vals.iter().position(|val| keyframes.contains_key(*val)) {
                            Some(i) => {
                                let mut vals: Vec<String> =
                                    vals.iter().map(|s| s.to_string()).collect();
                                vals[i] = keyframes[&vals[i]].clone();
                                vals.join(" ")
                            }
                            None => v.to_string(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                tree.get_mut(node_id).value = new;
                tree.get_mut(node_id).raws.value = None;
            }
        }
    }
}

fn process_rule(
    tree: &mut CssTree,
    id: &str,
    rule: usize,
    processed: &mut HashSet<usize>,
    deep_rules: &mut HashSet<usize>,
) {
    if processed.contains(&rule) {
        return;
    }
    if let Some(parent) = tree.get(rule).parent {
        if tree.get(parent).kind == CssKind::AtRule && is_keyframes(&tree.get(parent).name) {
            // postcss only visits each node once; the worklist needs the same
            processed.insert(rule);
            return;
        }
    }
    processed.insert(rule);

    let mut deep = false;
    let mut parent = tree.get(rule).parent;
    while let Some(p) = parent {
        if tree.get(p).kind == CssKind::Root {
            break;
        }
        if deep_rules.contains(&p) {
            deep = true;
            break;
        }
        parent = tree.get(p).parent;
    }

    let selector = tree.get(rule).selector.clone();
    let mut root = parse_selector(&selector);
    let mut ctx = RewriteCtx {
        id: id.to_string(),
        rule_is_deep: false,
        has_nested_rules: tree
            .children(rule)
            .iter()
            .any(|c| tree.get(*c).kind == CssKind::Rule),
        wrap_nested: false,
    };
    let mut out: Vec<Selector> = Vec::new();
    for selector in std::mem::take(&mut root.selectors) {
        let mut sel = selector;
        let replacement = rewrite_selector(&mut ctx, &mut sel, deep, false);
        match replacement {
            Some(list) => out.extend(list),
            None => out.push(sel),
        }
    }
    root.selectors = out;

    if ctx.rule_is_deep {
        deep_rules.insert(rule);
    }
    if ctx.wrap_nested {
        extract_and_wrap_nodes(tree, rule);
        let atrules: Vec<usize> = tree
            .children(rule)
            .into_iter()
            .filter(|c| tree.get(*c).kind == CssKind::AtRule)
            .collect();
        for at in atrules {
            extract_and_wrap_nodes(tree, at);
        }
    }

    tree.get_mut(rule).selector = root.to_string();
    tree.get_mut(rule).raws.selector = None;
}

struct RewriteCtx {
    id: String,
    rule_is_deep: bool,
    has_nested_rules: bool,
    wrap_nested: bool,
}

fn is_space_combinator(n: &SelNode) -> bool {
    n.kind == SelKind::Combinator && !n.value.is_empty() && n.value.chars().all(|c| c.is_whitespace())
}

fn is_deep_selector(n: &SelNode) -> bool {
    if n.kind == SelKind::Pseudo && (n.value == ":deep" || n.value == "::v-deep") {
        return true;
    }
    n.nodes
        .iter()
        .any(|s| s.nodes.iter().any(is_deep_selector))
}

fn is_deep_container_pseudo(n: &SelNode) -> bool {
    n.kind == SelKind::Pseudo
        && matches!(n.value.as_str(), ":is" | ":where" | ":has" | ":not")
}

fn can_split_deep_container_pseudo(n: &SelNode) -> bool {
    matches!(n.value.as_str(), ":is" | ":where" | ":has")
}

/// Returns `Some(selectors)` when the selector was split into several.
fn rewrite_selector(
    ctx: &mut RewriteCtx,
    selector: &mut Selector,
    deep: bool,
    slotted: bool,
) -> Option<Vec<Selector>> {
    let mut node: Option<usize> = None;
    let mut should_inject = !deep;
    let mut has_nested_deep = false;
    let mut split_result: Option<Vec<Selector>> = None;

    let mut i = 0usize;
    'each: while i < selector.nodes.len() {
        let kind = selector.nodes[i].kind;
        let value = selector.nodes[i].value.clone();

        if kind == SelKind::Combinator && (value == ">>>" || value == "/deep/") {
            let n = &mut selector.nodes[i];
            n.value = " ".into();
            n.rendered = " ".into();
            n.spaces_before.clear();
            n.spaces_after.clear();
            break 'each;
        }

        if kind == SelKind::Pseudo {
            if is_deep_container_pseudo(&selector.nodes[i]) {
                let has_deep_selectors = selector.nodes[i]
                    .nodes
                    .iter()
                    .any(|s| s.nodes.iter().any(is_deep_selector));
                if has_deep_selectors {
                    let has_scope_anchor = node.is_some();
                    let has_mixed = selector.nodes[i]
                        .nodes
                        .iter()
                        .any(|s| !s.nodes.iter().any(is_deep_selector));
                    let has_trailing = i < selector.nodes.len() - 1;
                    if can_split_deep_container_pseudo(&selector.nodes[i])
                        && !deep
                        && !has_scope_anchor
                        && has_mixed
                        && has_trailing
                    {
                        split_result =
                            Some(split_selector_for_nested_deep(ctx, selector, i, deep, slotted));
                        break 'each;
                    }
                    if value == ":not" && !deep && !has_scope_anchor && has_mixed && has_trailing {
                        i += 1;
                        continue 'each;
                    }
                    let inner_deep = deep || has_scope_anchor;
                    let mut inner = std::mem::take(&mut selector.nodes[i].nodes);
                    for s in inner.iter_mut() {
                        if let Some(list) = rewrite_selector(ctx, s, inner_deep, slotted) {
                            // nested splits are not expected here
                            if let Some(first) = list.into_iter().next() {
                                *s = first;
                            }
                        }
                    }
                    selector.nodes[i].nodes = inner;
                    if !has_scope_anchor {
                        node = Some(i);
                        should_inject = false;
                    }
                    has_nested_deep = true;
                }
            }

            if value == ":deep" || value == "::v-deep" {
                ctx.rule_is_deep = true;
                if !selector.nodes[i].nodes.is_empty() {
                    let inner = selector.nodes[i].nodes[0].nodes.clone();
                    let count = inner.len();
                    // replace the pseudo with its inner selector
                    for (k, ss) in inner.into_iter().enumerate() {
                        selector.nodes.insert(i + 1 + k, ss);
                    }
                    // insert a space combinator before if there isn't one
                    let prev_is_space = if i > 0 {
                        is_space_combinator(&selector.nodes[i - 1])
                    } else {
                        false
                    };
                    if !prev_is_space {
                        selector.nodes.insert(i + 1, SelNode::combinator(" "));
                        let _ = count;
                    }
                    selector.nodes.remove(i);
                } else {
                    let prev_is_space = i > 0 && is_space_combinator(&selector.nodes[i - 1]);
                    if prev_is_space {
                        selector.nodes.remove(i - 1);
                        selector.nodes.remove(i - 1);
                    } else {
                        selector.nodes.remove(i);
                    }
                }
                break 'each;
            }

            if value == ":slotted" || value == "::v-slotted" {
                let mut inner = selector.nodes[i].nodes[0].clone();
                rewrite_selector(ctx, &mut inner, deep, true);
                let items = inner.nodes;
                for (k, ss) in items.into_iter().enumerate() {
                    selector.nodes.insert(i + 1 + k, ss);
                }
                selector.nodes.remove(i);
                should_inject = false;
                break 'each;
            }

            if value == ":global" || value == "::v-global" {
                let inner = selector.nodes[i].nodes[0].clone();
                *selector = inner;
                return None;
            }
        }

        if kind == SelKind::Universal {
            let has_prev = i > 0;
            let has_next = i + 1 < selector.nodes.len();
            if !has_prev {
                if has_next {
                    if selector.nodes[i + 1].kind == SelKind::Combinator
                        && selector.nodes[i + 1].value == " "
                    {
                        selector.nodes.remove(i + 1);
                    }
                    selector.nodes.remove(i);
                    continue 'each;
                } else {
                    selector.nodes.insert(i, SelNode::combinator(""));
                    node = Some(i);
                    selector.nodes.remove(i + 1);
                    break 'each;
                }
            }
            if node.is_some() {
                i += 1;
                continue 'each;
            }
        }

        if !has_nested_deep
            && ((kind != SelKind::Pseudo && kind != SelKind::Combinator)
                || (kind == SelKind::Pseudo
                    && (value == ":is" || value == ":where")
                    && node.is_none()))
        {
            node = Some(i);
        }
        i += 1;
    }

    if let Some(list) = split_result {
        return Some(list);
    }

    if ctx.has_nested_rules {
        if !ctx.rule_is_deep {
            ctx.wrap_nested = true;
        }
        should_inject = ctx.rule_is_deep;
    }

    if let Some(idx) = node {
        if !has_nested_deep {
            let n = &selector.nodes[idx];
            if n.kind == SelKind::Pseudo && (n.value == ":is" || n.value == ":where") {
                let mut inner = std::mem::take(&mut selector.nodes[idx].nodes);
                for s in inner.iter_mut() {
                    rewrite_selector(ctx, s, deep, slotted);
                }
                selector.nodes[idx].nodes = inner;
                should_inject = false;
            }
        }
    }

    match node {
        Some(idx) => selector.nodes[idx].spaces_after.clear(),
        None => {
            if let Some(first) = selector.nodes.first_mut() {
                first.spaces_before.clear();
            }
        }
    }

    if should_inject {
        let id_to_add = if slotted {
            format!("{}-s", ctx.id)
        } else {
            ctx.id.clone()
        };
        let attr = SelNode::attribute(&id_to_add);
        match node {
            Some(idx) => selector.nodes.insert(idx + 1, attr),
            None => selector.nodes.insert(0, attr),
        }
    }
    None
}

fn split_selector_for_nested_deep(
    ctx: &mut RewriteCtx,
    selector: &Selector,
    pseudo_index: usize,
    deep: bool,
    slotted: bool,
) -> Vec<Selector> {
    let branches = selector.nodes[pseudo_index].nodes.clone();
    let first_before = selector
        .nodes
        .first()
        .map(|n| n.spaces_before.clone())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (index, branch) in branches.into_iter().enumerate() {
        let mut branch_selector = selector.clone();
        if let Some(first) = branch_selector.nodes.first_mut() {
            first.spaces_before = if index == 0 {
                first_before.clone()
            } else {
                " ".to_string()
            };
        }
        let mut branch_clone = branch;
        if let Some(first) = branch_clone.nodes.first_mut() {
            first.spaces_before.clear();
        }
        branch_selector.nodes[pseudo_index].nodes = vec![branch_clone];
        if let Some(list) = rewrite_selector(ctx, &mut branch_selector, deep, slotted) {
            out.extend(list);
        } else {
            out.push(branch_selector);
        }
    }
    out
}

fn extract_and_wrap_nodes(tree: &mut CssTree, parent: usize) {
    let children = tree.children(parent);
    let nodes: Vec<usize> = children
        .iter()
        .copied()
        .filter(|c| matches!(tree.get(*c).kind, CssKind::Decl | CssKind::Comment))
        .collect();
    if nodes.is_empty() {
        return;
    }
    for n in &nodes {
        tree.remove(*n);
    }
    let mut rule = CssNode::new(CssKind::Rule);
    rule.selector = "&".to_string();
    rule.nodes = Some(Vec::new());
    let rule_id = tree.add(rule);
    for n in nodes {
        tree.push_child(rule_id, n);
    }
    tree.push_child(parent, rule_id);
    // prepend
    if let Some(list) = tree.get_mut(parent).nodes.as_mut() {
        let pos = list.iter().position(|c| *c == rule_id).unwrap();
        let v = list.remove(pos);
        list.insert(0, v);
    }
}

pub fn rewrite_root_selectors(_root: &mut SelRoot) {}
