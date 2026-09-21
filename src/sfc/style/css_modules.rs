//! `postcss-modules` equivalent: the local-by-default + scope behaviour Vue
//! relies on (class/id renaming, `:global`/`:local`, keyframes localization).

use super::postcss::node::{CssKind, CssTree};
use super::selector::{SelKind, Selector, parse as parse_selector};

/// `string-hash`
fn string_hash(s: &str) -> u32 {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut hash: u32 = 5381;
    let mut i = units.len();
    while i > 0 {
        i -= 1;
        hash = hash.wrapping_mul(33) ^ (units[i] as u32);
    }
    hash
}

fn to_base36(mut n: u32) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// `generateScopedNameDefault`
fn generate_scoped_name(name: &str, css: &str) -> String {
    let needle = format!(".{name}");
    let line_number = match css.find(&needle) {
        Some(i) => css[..i].split(['\r', '\n']).count(),
        None => 1,
    };
    let hash: String = to_base36(string_hash(css)).chars().take(5).collect();
    format!("_{name}_{hash}_{line_number}")
}

fn is_keyframes(name: &str) -> bool {
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

fn strip_vendor(prop: &str) -> &str {
    if let Some(rest) = prop.strip_prefix('-') {
        if let Some(i) = rest.find('-') {
            return &rest[i + 1..];
        }
    }
    prop
}

pub struct ModulesResult {
    pub exports: Vec<(String, String)>,
    pub error: Option<String>,
}

pub fn apply(tree: &mut CssTree, original_css: &str) -> ModulesResult {
    let mut exports: Vec<(String, String)> = Vec::new();
    let mut keyframes: Vec<(String, String)> = Vec::new();
    let mut error = None;

    let export = |name: &str, scoped: &str, exports: &mut Vec<(String, String)>| {
        if !exports.iter().any(|(k, v)| k == name && v == scoped) {
            exports.push((name.to_string(), scoped.to_string()));
        }
    };

    for id in tree.walk_ids(tree.root) {
        match tree.get(id).kind {
            CssKind::AtRule => {
                let name = tree.get(id).name.clone();
                // `postcss-modules-values` reads the imported file; this port
                // has no filesystem, so the import cannot be resolved
                if name == "value" {
                    let params = tree.get(id).params.clone();
                    if let Some(i) = params.find(" from ") {
                        let from = params[i + 6..].trim().trim_matches(['"', '\''].as_ref());
                        error = Some(format!(
                            "Unable to resolve `@value ... from '{from}'`: imports from other \
                             files are not supported."
                        ));
                    }
                }
                if is_keyframes(&name) {
                    let params = tree.get(id).params.trim().to_string();
                    if !params.is_empty() {
                        let scoped = generate_scoped_name(&params, original_css);
                        export(&params, &scoped, &mut exports);
                        keyframes.push((params, scoped.clone()));
                        tree.get_mut(id).params = scoped;
                        tree.get_mut(id).raws.params = None;
                    }
                }
            }
            CssKind::Rule => {
                // selectors inside @keyframes are keyframe stops, not selectors
                if let Some(parent) = tree.get(id).parent {
                    if tree.get(parent).kind == CssKind::AtRule
                        && is_keyframes(&tree.get(parent).name)
                    {
                        continue;
                    }
                }
                let selector = tree.get(id).selector.clone();
                let mut root = parse_selector(&selector);
                for sel in root.selectors.iter_mut() {
                    localize_selector(sel, original_css, &mut exports, &mut error);
                }
                tree.get_mut(id).selector = root.to_string();
                tree.get_mut(id).raws.selector = None;
            }
            CssKind::Decl => {
                let prop = tree.get(id).prop.clone();
                if prop == "composes" || prop == "compose-with" {
                    error = Some(
                        "referenced class name \"composes\" in composes not found".to_string(),
                    );
                }
            }
            _ => {}
        }
    }

    if !keyframes.is_empty() {
        for id in tree.walk_ids(tree.root) {
            if tree.get(id).kind != CssKind::Decl {
                continue;
            }
            let prop = strip_vendor(&tree.get(id).prop).to_string();
            if prop != "animation" && prop != "animation-name" {
                continue;
            }
            let value = tree.get(id).value.clone();
            let new = value
                .split(',')
                .map(|part| {
                    part.split_whitespace()
                        .map(|w| {
                            keyframes
                                .iter()
                                .find(|(k, _)| k == w)
                                .map(|(_, v)| v.clone())
                                .unwrap_or_else(|| w.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join(",");
            if new != value {
                tree.get_mut(id).value = new;
                tree.get_mut(id).raws.value = None;
            }
        }
    }

    ModulesResult { exports, error }
}

fn localize_selector(
    selector: &mut Selector,
    css: &str,
    exports: &mut Vec<(String, String)>,
    _error: &mut Option<String>,
) {
    let mut global_mode = false;
    let mut i = 0usize;
    while i < selector.nodes.len() {
        let kind = selector.nodes[i].kind;
        let value = selector.nodes[i].value.clone();
        match kind {
            SelKind::Pseudo if value == ":global" || value == ":local" => {
                let is_global = value == ":global";
                if selector.nodes[i].nodes.is_empty() {
                    // `:global` / `:local` switch mode for the rest; the
                    // separating combinator goes with them
                    global_mode = is_global;
                    selector.nodes.remove(i);
                    if i < selector.nodes.len()
                        && selector.nodes[i].kind == SelKind::Combinator
                        && !selector.nodes[i].value.is_empty()
                        && selector.nodes[i].value.chars().all(|c| c.is_whitespace())
                    {
                        selector.nodes.remove(i);
                    }
                    if let Some(first) = selector.nodes.get_mut(i) {
                        first.spaces_before.clear();
                    }
                    continue;
                }
                let spaces_before = selector.nodes[i].spaces_before.clone();
                let spaces_after = selector.nodes[i].spaces_after.clone();
                let mut inner = selector.nodes[i].nodes[0].clone();
                if !is_global {
                    localize_selector(&mut inner, css, exports, _error);
                }
                let count = inner.nodes.len();
                for (k, mut n) in inner.nodes.into_iter().enumerate() {
                    if k == 0 {
                        n.spaces_before = format!("{spaces_before}{}", n.spaces_before);
                    }
                    if k == count - 1 {
                        n.spaces_after = format!("{}{spaces_after}", n.spaces_after);
                    }
                    selector.nodes.insert(i + 1 + k, n);
                }
                selector.nodes.remove(i);
                i += count;
                continue;
            }
            SelKind::Class | SelKind::Id if !global_mode => {
                let scoped = generate_scoped_name(&value, css);
                if !exports.iter().any(|(k, _)| k == &value) {
                    exports.push((value.clone(), scoped.clone()));
                }
                let prefix = if kind == SelKind::Class { "." } else { "#" };
                selector.nodes[i].value = scoped.clone();
                selector.nodes[i].rendered = format!("{prefix}{scoped}");
            }
            SelKind::Pseudo if !selector.nodes[i].nodes.is_empty() => {
                let mut inner = std::mem::take(&mut selector.nodes[i].nodes);
                for s in inner.iter_mut() {
                    localize_selector(s, css, exports, _error);
                }
                selector.nodes[i].nodes = inner;
            }
            _ => {}
        }
        i += 1;
    }
}
