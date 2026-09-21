//! Port of `compiler-sfc/src/template/transformSrcset.ts`.

use crate::core::ast::*;
use crate::core::transform::TransformContext;

use super::utils::*;

struct ImageCandidate {
    url: String,
    descriptor: String,
}

pub fn transform_srcset(node: NodeId, ctx: &mut TransformContext) {
    if !ctx.a.is(node, NodeType::Element) {
        return;
    }
    let tag = ctx.a.el(node).tag.clone();
    if (tag != "img" && tag != "source") || ctx.a.el(node).props.is_empty() {
        return;
    }
    let options = ctx.opts.asset_url_options.clone();
    let props = ctx.a.el(node).props.clone();
    for (index, attr) in props.iter().enumerate() {
        let (value, loc) = match ctx.a.node(*attr) {
            Node::Attribute(a) if a.name == "srcset" => match &a.value {
                Some(v) if !v.content.is_empty() => (v.content.clone(), a.loc.clone()),
                _ => continue,
            },
            _ => continue,
        };

        let mut image_candidates: Vec<ImageCandidate> = value
            .split(',')
            .map(|s| {
                let normalized = s
                    .replace("\\t", " ")
                    .replace("\\n", " ")
                    .replace("\\f", " ")
                    .replace("\\r", " ");
                let trimmed = normalized.trim();
                let mut parts = trimmed.splitn(2, ' ');
                let url = parts.next().unwrap_or("").to_string();
                let descriptor = parts.next().unwrap_or("").to_string();
                ImageCandidate { url, descriptor }
            })
            .collect();

        // re-merge data urls split on their internal comma
        let mut i = 0;
        while i < image_candidates.len() {
            if is_data_url(&image_candidates[i].url) && i + 1 < image_candidates.len() {
                let url = image_candidates[i].url.clone();
                image_candidates[i + 1].url = format!("{url},{}", image_candidates[i + 1].url);
                image_candidates.remove(i);
            } else {
                i += 1;
            }
        }

        let should_process = |url: &str| -> bool {
            !url.is_empty()
                && !is_external_url(url)
                && !is_data_url(url)
                && (options.include_absolute || is_relative_url(url))
        };

        if !image_candidates.iter().any(|c| should_process(&c.url)) {
            continue;
        }

        if let Some(base) = &options.base {
            let mut set: Vec<String> = Vec::new();
            let mut need_import_transform = false;
            for candidate in image_candidates.iter_mut() {
                let descriptor = if candidate.descriptor.is_empty() {
                    String::new()
                } else {
                    format!(" {}", candidate.descriptor)
                };
                if candidate.url.starts_with('.') {
                    candidate.url = posix_join(base, &candidate.url);
                    set.push(format!("{}{descriptor}", candidate.url));
                } else if should_process(&candidate.url) {
                    need_import_transform = true;
                } else {
                    set.push(format!("{}{descriptor}", candidate.url));
                }
            }
            if !need_import_transform {
                let joined = set.join(", ");
                if let Node::Attribute(a) = ctx.a.node_mut(*attr) {
                    if let Some(v) = a.value.as_mut() {
                        v.content = joined;
                    }
                }
                continue;
            }
        }

        let compound = ctx.a.create_compound_expression(Vec::new(), loc.clone());
        let total = image_candidates.len();
        for (i, candidate) in image_candidates.iter().enumerate() {
            if should_process(&candidate.url) {
                let (path, hash) = parse_url(&candidate.url);
                let source = path.clone().or_else(|| hash.clone());
                if let Some(source) = source {
                    let normalized = normalize_decoded_import_path(&source);
                    let mut exp = match ctx.imports.iter().position(|i| i.path == normalized) {
                        Some(existing) => ctx.a.create_simple_expression(
                            format!("_imports_{existing}"),
                            false,
                            loc.clone(),
                            ConstantType::CanStringify,
                        ),
                        None => {
                            let e = ctx.a.create_simple_expression(
                                format!("_imports_{}", ctx.imports.len()),
                                false,
                                loc.clone(),
                                ConstantType::CanStringify,
                            );
                            ctx.imports.push(ImportItem {
                                exp: e,
                                path: normalized,
                            });
                            e
                        }
                    };
                    if path.is_some() && hash.is_some() {
                        let content = format!(
                            "{} + '{}'",
                            ctx.a.exp(exp).content,
                            hash.clone().unwrap()
                        );
                        exp = ctx.a.create_simple_expression(
                            content,
                            false,
                            loc.clone(),
                            ConstantType::CanStringify,
                        );
                    }
                    ctx.a.compound_mut(compound).children.push(exp);
                }
            } else {
                let exp = ctx.a.create_simple_expression(
                    format!("\"{}\"", candidate.url),
                    false,
                    loc.clone(),
                    ConstantType::CanStringify,
                );
                ctx.a.compound_mut(compound).children.push(exp);
            }
            let is_not_last = total - 1 > i;
            let text = if !candidate.descriptor.is_empty() && is_not_last {
                Some(format!(" + ' {}, ' + ", candidate.descriptor))
            } else if !candidate.descriptor.is_empty() {
                Some(format!(" + ' {}'", candidate.descriptor))
            } else if is_not_last {
                Some(" + ', ' + ".to_string())
            } else {
                None
            };
            if let Some(t) = text {
                let n = ctx.a.string(t);
                ctx.a.compound_mut(compound).children.push(n);
            }
        }

        let exp = if ctx.opts.hoist_static {
            let h = ctx.hoist(compound);
            ctx.a.exp_mut(h).const_type = ConstantType::CanStringify;
            h
        } else {
            compound
        };

        let arg = ctx
            .a
            .create_simple_expression("srcset", true, loc.clone(), ConstantType::NotConstant);
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
