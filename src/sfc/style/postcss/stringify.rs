//! Port of `postcss@8.5.28/lib/stringifier.js`.

use std::collections::HashMap;

use super::node::*;

fn default_raw(detect: &str) -> String {
    match detect {
        "after" => "\n",
        "beforeClose" => "\n",
        "beforeComment" => "\n",
        "beforeDecl" => "\n",
        "beforeOpen" => " ",
        "beforeRule" => "\n",
        "colon" => ": ",
        "commentLeft" => " ",
        "commentRight" => " ",
        "emptyBody" => "",
        "indent" => "    ",
        _ => "",
    }
    .to_string()
}

/// escapes `</style` and `<!--` sequences, as postcss 8.5 does
fn escape_html_in_css(s: &str) -> String {
    if !s.contains('<') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let rest: String = chars[i + 1..].iter().collect();
            let lower = rest.to_lowercase();
            let style_match = if lower.starts_with("/style") {
                Some(6)
            } else if lower.starts_with("style") {
                Some(5)
            } else {
                None
            };
            if let Some(len) = style_match {
                // \b — the next char must not be a word character
                let after = chars.get(i + 1 + len).copied();
                let is_boundary = match after {
                    Some(c) => !(c.is_alphanumeric() || c == '_'),
                    None => true,
                };
                if is_boundary {
                    out.push_str("\\3c ");
                    out.extend(&chars[i + 1..i + 1 + len]);
                    i += 1 + len;
                    continue;
                }
            }
            if rest.starts_with("!--") {
                out.push_str("\\3c !--");
                i += 4;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub struct Stringifier<'a> {
    tree: &'a CssTree,
    out: String,
    cache: HashMap<String, Option<String>>,
}

pub fn stringify(tree: &CssTree) -> String {
    let mut s = Stringifier {
        tree,
        out: String::new(),
        cache: HashMap::new(),
    };
    s.root(tree.root);
    s.out
}

impl<'a> Stringifier<'a> {
    fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn root(&mut self, node: usize) {
        self.body(node);
        if let Some(after) = self.tree.get(node).raws.after.clone() {
            let escaped = escape_html_in_css(&after);
            self.push(&escaped);
        }
    }

    fn body(&mut self, node: usize) {
        let children = self.tree.children(node);
        let mut last = children.len() as i64 - 1;
        while last > 0 {
            if self.tree.get(children[last as usize]).kind != CssKind::Comment {
                break;
            }
            last -= 1;
        }
        let semicolon = self
            .raw(node, Some("semicolon"), "semicolon")
            .map(|v| v == "true")
            .unwrap_or(false);
        for (i, child) in children.iter().enumerate() {
            let mut child_semicolon = last != i as i64 || semicolon;
            if !child_semicolon
                && i < children.len() - 1
                && ((self.tree.get(*child).kind == CssKind::AtRule
                    && self.tree.get(*child).nodes.is_none())
                    || (self.tree.get(*child).kind == CssKind::Decl
                        && is_custom_property(self.tree.get(*child))))
            {
                child_semicolon = true;
            }
            let before = self.raw(*child, Some("before"), "before").unwrap_or_default();
            if !before.is_empty() {
                let escaped = escape_html_in_css(&before);
                self.push(&escaped);
            }
            self.stringify(*child, child_semicolon);
        }
    }

    fn stringify(&mut self, node: usize, semicolon: bool) {
        match self.tree.get(node).kind {
            CssKind::Root => self.root(node),
            CssKind::Rule => self.rule(node),
            CssKind::AtRule => self.atrule(node, semicolon),
            CssKind::Decl => self.decl(node, semicolon),
            CssKind::Comment => self.comment(node),
        }
    }

    fn rule(&mut self, node: usize) {
        let start = self.raw_value(node, "selector");
        self.block(node, &start);
        if let Some(own) = self.tree.get(node).raws.own_semicolon.clone() {
            let escaped = escape_html_in_css(&own);
            self.push(&escaped);
        }
    }

    fn atrule(&mut self, node: usize, semicolon: bool) {
        let start = self.atrule_start(node);
        if self.tree.get(node).nodes.is_some() {
            self.block(node, &start);
        } else {
            let between = self.tree.get(node).raws.between.clone().unwrap_or_default();
            let end = format!("{between}{}", if semicolon { ";" } else { "" });
            let escaped = escape_html_in_css(&format!("{start}{end}"));
            self.push(&escaped);
        }
    }

    fn atrule_start(&mut self, node: usize) -> String {
        let name = format!("@{}", self.tree.get(node).name);
        let params = if self.tree.get(node).params.is_empty() {
            String::new()
        } else {
            self.raw_value(node, "params")
        };
        let after_name = match self.tree.get(node).raws.after_name.clone() {
            None => {
                if params.is_empty() {
                    String::new()
                } else {
                    " ".to_string()
                }
            }
            Some(a) => {
                if a.is_empty()
                    && !params.is_empty()
                    && !is_at_name_end(params.chars().next().unwrap())
                {
                    " ".to_string()
                } else {
                    a
                }
            }
        };
        format!("{name}{after_name}{params}")
    }

    fn block(&mut self, node: usize, start: &str) {
        let between = self
            .raw(node, Some("between"), "beforeOpen")
            .unwrap_or_default();
        let escaped = escape_html_in_css(&format!("{start}{between}"));
        self.push(&format!("{escaped}{{"));

        let has_nodes = self
            .tree
            .get(node)
            .nodes
            .as_ref()
            .map(|n| !n.is_empty())
            .unwrap_or(false);
        let after = if has_nodes {
            self.body(node);
            self.raw(node, Some("after"), "after")
        } else {
            self.raw(node, Some("after"), "emptyBody")
        }
        .unwrap_or_default();
        if !after.is_empty() {
            let escaped = escape_html_in_css(&after);
            self.push(&escaped);
        }
        self.push("}");
    }

    fn decl(&mut self, node: usize, semicolon: bool) {
        let between = self
            .raw(node, Some("between"), "colon")
            .unwrap_or_else(|| default_raw("colon"));
        let value = self.raw_value(node, "value");
        let n = self.tree.get(node);
        let mut string = format!("{}{between}{value}", n.prop);
        if n.important {
            string.push_str(
                &n.raws
                    .important
                    .clone()
                    .unwrap_or_else(|| " !important".to_string()),
            );
        }
        if semicolon {
            string.push(';');
        }
        let escaped = escape_html_in_css(&string);
        self.push(&escaped);
    }

    fn comment(&mut self, node: usize) {
        let left = self
            .raw(node, Some("left"), "commentLeft")
            .unwrap_or_else(|| default_raw("commentLeft"));
        let right = self
            .raw(node, Some("right"), "commentRight")
            .unwrap_or_else(|| default_raw("commentRight"));
        let text = self.tree.get(node).text.clone();
        let escaped = escape_html_in_css(&format!("/*{left}{text}{right}*/"));
        self.push(&escaped);
    }

    fn raw_value(&self, node: usize, prop: &str) -> String {
        let n = self.tree.get(node);
        let (value, raw) = match prop {
            "value" => (n.value.clone(), n.raws.value.clone()),
            "selector" => (n.selector.clone(), n.raws.selector.clone()),
            "params" => (n.params.clone(), n.raws.params.clone()),
            _ => (String::new(), None),
        };
        match raw {
            Some((raw, raw_value)) if raw_value == value => raw,
            _ => value,
        }
    }

    fn own_raw(&self, node: usize, own: &str) -> Option<String> {
        let n = self.tree.get(node);
        match own {
            "before" => n.raws.before.clone(),
            "after" => n.raws.after.clone(),
            "between" => n.raws.between.clone(),
            "left" => n.raws.left.clone(),
            "right" => n.raws.right.clone(),
            "semicolon" => n.raws.semicolon.map(|b| b.to_string()),
            _ => None,
        }
    }

    fn raw(&mut self, node: usize, own: Option<&str>, detect: &str) -> Option<String> {
        if let Some(own) = own {
            if let Some(v) = self.own_raw(node, own) {
                return Some(v);
            }
        }
        let parent = self.tree.get(node).parent;

        if detect == "before" {
            match parent {
                None => return Some(String::new()),
                Some(p) => {
                    if self.tree.get(p).kind == CssKind::Root
                        && self.tree.children(p).first() == Some(&node)
                    {
                        return Some(String::new());
                    }
                }
            }
        }

        if parent.is_none() {
            return Some(default_raw(detect));
        }

        if let Some(cached) = self.cache.get(detect) {
            return cached.clone();
        }

        let value = if detect == "before" || detect == "after" {
            Some(self.before_after(node, detect))
        } else {
            self.detect_raw(detect, own)
        };

        let value = Some(value.unwrap_or_else(|| default_raw(detect)));
        self.cache.insert(detect.to_string(), value.clone());
        value
    }

    fn detect_raw(&mut self, detect: &str, own: Option<&str>) -> Option<String> {
        let root = self.tree.root;
        match detect {
            "beforeClose" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    let n = self.tree.get(id);
                    if n.nodes.as_ref().map(|v| !v.is_empty()).unwrap_or(false) {
                        if let Some(after) = &n.raws.after {
                            let mut v = after.clone();
                            if v.contains('\n') {
                                v = strip_after_last_newline(&v);
                            }
                            value = Some(v);
                            break;
                        }
                    }
                }
                value.map(|v| strip_non_space(&v))
            }
            "beforeComment" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    if self.tree.get(id).kind == CssKind::Comment {
                        if let Some(before) = &self.tree.get(id).raws.before {
                            let mut v = before.clone();
                            if v.contains('\n') {
                                v = strip_after_last_newline(&v);
                            }
                            value = Some(v);
                            break;
                        }
                    }
                }
                match value {
                    None => Some(
                        self.detect_raw("beforeDecl", None)
                            .unwrap_or_else(|| default_raw("beforeDecl")),
                    ),
                    Some(v) => Some(strip_non_space(&v)),
                }
            }
            "beforeDecl" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    if self.tree.get(id).kind == CssKind::Decl {
                        if let Some(before) = &self.tree.get(id).raws.before {
                            let mut v = before.clone();
                            if v.contains('\n') {
                                v = strip_after_last_newline(&v);
                            }
                            value = Some(v);
                            break;
                        }
                    }
                }
                match value {
                    None => Some(
                        self.detect_raw("beforeRule", None)
                            .unwrap_or_else(|| default_raw("beforeRule")),
                    ),
                    Some(v) => Some(strip_non_space(&v)),
                }
            }
            "beforeOpen" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    if self.tree.get(id).kind != CssKind::Decl {
                        if let Some(between) = &self.tree.get(id).raws.between {
                            value = Some(between.clone());
                            break;
                        }
                    }
                }
                value
            }
            "beforeRule" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    let n = self.tree.get(id);
                    let is_first_root_child =
                        n.parent == Some(root) && self.tree.children(root).first() == Some(&id);
                    if n.nodes.is_some() && !is_first_root_child {
                        if let Some(before) = &n.raws.before {
                            let mut v = before.clone();
                            if v.contains('\n') {
                                v = strip_after_last_newline(&v);
                            }
                            value = Some(v);
                            break;
                        }
                    }
                }
                value.map(|v| strip_non_space(&v))
            }
            "colon" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    if self.tree.get(id).kind == CssKind::Decl {
                        if let Some(between) = &self.tree.get(id).raws.between {
                            value = Some(
                                between
                                    .chars()
                                    .filter(|c| c.is_whitespace() || *c == ':')
                                    .collect(),
                            );
                            break;
                        }
                    }
                }
                value
            }
            "emptyBody" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    let n = self.tree.get(id);
                    if n.nodes.as_ref().map(|v| v.is_empty()).unwrap_or(false) {
                        if let Some(after) = &n.raws.after {
                            value = Some(after.clone());
                            break;
                        }
                    }
                }
                value
            }
            "indent" => {
                if let Some(i) = &self.tree.get(root).raws.indent {
                    return Some(i.clone());
                }
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    let p = self.tree.get(id).parent;
                    if let Some(p) = p {
                        let pp = self.tree.get(p).parent;
                        if p != root && pp == Some(root) {
                            if let Some(before) = &self.tree.get(id).raws.before {
                                let last = before.rsplit('\n').next().unwrap_or("");
                                value = Some(strip_non_space(last));
                                break;
                            }
                        }
                    }
                }
                value
            }
            "semicolon" => {
                let mut value: Option<String> = None;
                for id in self.tree.walk_ids(root) {
                    let n = self.tree.get(id);
                    if let Some(children) = &n.nodes {
                        if !children.is_empty()
                            && self.tree.get(*children.last().unwrap()).kind == CssKind::Decl
                        {
                            if let Some(s) = n.raws.semicolon {
                                value = Some(s.to_string());
                                break;
                            }
                        }
                    }
                }
                value
            }
            _ => {
                let mut value: Option<String> = None;
                if let Some(own) = own {
                    for id in self.tree.walk_ids(root) {
                        if let Some(v) = self.own_raw(id, own) {
                            value = Some(v);
                            break;
                        }
                    }
                }
                value
            }
        }
    }

    fn before_after(&mut self, node: usize, detect: &str) -> String {
        let kind = self.tree.get(node).kind;
        let mut value = if kind == CssKind::Decl {
            self.raw(node, None, "beforeDecl")
        } else if kind == CssKind::Comment {
            self.raw(node, None, "beforeComment")
        } else if detect == "before" {
            self.raw(node, None, "beforeRule")
        } else {
            self.raw(node, None, "beforeClose")
        }
        .unwrap_or_default();

        let depth = self.tree.depth(node);
        if value.contains('\n') {
            let indent = self
                .raw(node, None, "indent")
                .unwrap_or_else(|| default_raw("indent"));
            if !indent.is_empty() {
                for _ in 0..depth {
                    value.push_str(&indent);
                }
            }
        }
        value
    }
}

fn is_custom_property(node: &CssNode) -> bool {
    if !node.prop.starts_with("--") {
        return false;
    }
    match &node.raws.before {
        None => true,
        Some(b) => !b.chars().last().map(|c| !c.is_whitespace()).unwrap_or(false),
    }
}

/// `/[\t\n\f\r "#'()/;[\\\]{}]/`
fn is_at_name_end(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{c}' | '\r' | ' ' | '"' | '#' | '\'' | '(' | ')' | '/' | ';' | '['
            | '\\' | ']' | '{' | '}'
    )
}

fn strip_after_last_newline(s: &str) -> String {
    match s.rfind('\n') {
        Some(i) => s[..=i].to_string(),
        None => s.to_string(),
    }
}

fn strip_non_space(s: &str) -> String {
    s.chars().filter(|c| c.is_whitespace()).collect()
}
