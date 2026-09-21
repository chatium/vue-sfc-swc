//! Port of `compiler-sfc/src/template/transformAssetUrl.ts`.

use std::collections::HashMap;

use crate::core::ast::*;
use crate::core::transform::TransformContext;

use super::utils::*;

#[derive(Debug, Clone)]
pub struct AssetUrlOptions {
    pub base: Option<String>,
    pub include_absolute: bool,
    pub tags: HashMap<String, Vec<String>>,
}

fn resource_url_tag_config() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    m.insert("video".into(), vec!["src".into(), "poster".into()]);
    m.insert("source".into(), vec!["src".into()]);
    m.insert("img".into(), vec!["src".into()]);
    m
}

impl Default for AssetUrlOptions {
    fn default() -> Self {
        let mut tags = resource_url_tag_config();
        tags.insert("image".into(), vec!["xlink:href".into(), "href".into()]);
        tags.insert("use".into(), vec!["xlink:href".into(), "href".into()]);
        AssetUrlOptions {
            base: None,
            include_absolute: false,
            tags,
        }
    }
}

fn can_transform_hash_import(tag: &str, attr_name: &str) -> bool {
    resource_url_tag_config()
        .get(tag)
        .map(|v| v.iter().any(|a| a == attr_name))
        .unwrap_or(false)
}

pub fn transform_asset_url(node: NodeId, ctx: &mut TransformContext) {
    if !ctx.a.is(node, NodeType::Element) {
        return;
    }
    if ctx.a.el(node).props.is_empty() {
        return;
    }
    let options = ctx.opts.asset_url_options.clone();
    let tag = ctx.a.el(node).tag.clone();
    let attrs = options.tags.get(&tag).cloned().unwrap_or_default();
    let wildcard = options.tags.get("*").cloned().unwrap_or_default();
    if attrs.is_empty() && wildcard.is_empty() {
        return;
    }
    let mut asset_attrs = attrs;
    asset_attrs.extend(wildcard);

    let props = ctx.a.el(node).props.clone();
    for (index, attr) in props.iter().enumerate() {
        let (name, value, loc) = match ctx.a.node(*attr) {
            Node::Attribute(a) => match &a.value {
                Some(v) => (a.name.clone(), v.content.clone(), a.loc.clone()),
                None => continue,
            },
            _ => continue,
        };
        if !asset_attrs.contains(&name) {
            continue;
        }

        let url_value = value;
        let is_hash_only = url_value.starts_with('#');
        if is_external_url(&url_value)
            || is_data_url(&url_value)
            || url_value == "#"
            || (is_hash_only && !can_transform_hash_import(&tag, &name))
            || (!options.include_absolute && !is_relative_url(&url_value))
        {
            continue;
        }

        let (path, hash) = parse_url(&url_value);
        if let Some(base) = &options.base {
            if url_value.starts_with('.') {
                let (base_path, _) = parse_url(base);
                let base_path = base_path.unwrap_or_else(|| "/".to_string());
                let joined = posix_join(
                    &base_path,
                    &format!(
                        "{}{}",
                        path.clone().unwrap_or_default(),
                        hash.clone().unwrap_or_default()
                    ),
                );
                if let Node::Attribute(a) = ctx.a.node_mut(*attr) {
                    if let Some(v) = a.value.as_mut() {
                        v.content = joined;
                    }
                }
                continue;
            }
        }

        let exp = get_imports_expression_exp(path, hash, loc.clone(), ctx);
        let arg = ctx
            .a
            .create_simple_expression(name, true, loc.clone(), ConstantType::NotConstant);
        let dir = ctx.a.add(Node::Directive(Box::new(DirectiveNode {
            name: "bind".into(),
            raw_name: None,
            arg: Some(arg),
            exp: Some(exp),
            modifiers: Vec::new(),
            for_parse_result: None,
            loc,
        })));
        ctx.a.el_mut(node).props[index] = dir;
    }
}

fn resolve_or_register_import(
    source: &str,
    loc: SourceLocation,
    ctx: &mut TransformContext,
) -> (String, NodeId) {
    let normalized = normalize_decoded_import_path(source);
    if let Some(i) = ctx.imports.iter().position(|i| i.path == normalized) {
        return (format!("_imports_{i}"), ctx.imports[i].exp);
    }
    let name = format!("_imports_{}", ctx.imports.len());
    let exp = ctx.a.create_simple_expression(
        name.clone(),
        false,
        loc,
        ConstantType::CanStringify,
    );
    ctx.imports.push(ImportItem {
        exp,
        path: normalized,
    });
    (name, exp)
}

fn get_imports_expression_exp(
    path: Option<String>,
    hash: Option<String>,
    loc: SourceLocation,
    ctx: &mut TransformContext,
) -> NodeId {
    match (path, hash) {
        (None, None) => {
            ctx.a
                .create_simple_expression("''", false, loc, ConstantType::CanStringify)
        }
        (None, Some(hash)) => resolve_or_register_import(&hash, loc, ctx).1,
        (Some(path), None) => resolve_or_register_import(&path, loc, ctx).1,
        (Some(path), Some(hash)) => {
            let (name, _) = resolve_or_register_import(&path, loc.clone(), ctx);
            let hash_exp = format!("{name} + '{hash}'");
            let final_exp = ctx.a.create_simple_expression(
                hash_exp.clone(),
                false,
                loc.clone(),
                ConstantType::CanStringify,
            );
            if !ctx.opts.hoist_static {
                return final_exp;
            }
            let existing = ctx.hoists.iter().position(|h| {
                h.map(|h| {
                    matches!(ctx.a.node(h), Node::SimpleExpression(e)
                        if !e.is_static && e.content == hash_exp)
                })
                .unwrap_or(false)
            });
            if let Some(i) = existing {
                return ctx.a.create_simple_expression(
                    format!("_hoisted_{}", i + 1),
                    false,
                    loc,
                    ConstantType::CanStringify,
                );
            }
            ctx.hoist(final_exp)
        }
    }
}
