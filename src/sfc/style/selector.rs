//! A focused port of `postcss-selector-parser`: enough of the node model,
//! parser and stringifier for `pluginScoped` to round-trip selectors exactly.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelKind {
    Tag,
    Class,
    Id,
    Universal,
    Attribute,
    Pseudo,
    Combinator,
    Str,
    Nesting,
    Comment,
}

#[derive(Debug, Clone)]
pub struct SelNode {
    pub kind: SelKind,
    /// the node's value without its prefix (`foo` for `.foo`)
    pub value: String,
    /// the exact rendered text of the value part (`.foo`, `[a="b"]`, `:is`)
    pub rendered: String,
    pub spaces_before: String,
    pub spaces_after: String,
    /// pseudo arguments
    pub nodes: Vec<Selector>,
}

impl SelNode {
    pub fn new(kind: SelKind, value: &str, rendered: &str) -> Self {
        SelNode {
            kind,
            value: value.to_string(),
            rendered: rendered.to_string(),
            spaces_before: String::new(),
            spaces_after: String::new(),
            nodes: Vec::new(),
        }
    }

    pub fn combinator(value: &str) -> Self {
        SelNode::new(SelKind::Combinator, value, value)
    }

    pub fn attribute(name: &str) -> Self {
        SelNode::new(SelKind::Attribute, name, &format!("[{name}]"))
    }

    pub fn to_string(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.spaces_before);
        out.push_str(&self.rendered);
        if self.kind == SelKind::Pseudo && !self.nodes.is_empty() {
            out.push('(');
            out.push_str(
                &self
                    .nodes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push(')');
        }
        out.push_str(&self.spaces_after);
        out
    }
}

#[derive(Debug, Clone, Default)]
pub struct Selector {
    pub nodes: Vec<SelNode>,
}

impl Selector {
    pub fn to_string(&self) -> String {
        self.nodes.iter().map(|n| n.to_string()).collect()
    }
    pub fn index_of(&self, node: &SelNode) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| std::ptr::eq(n as *const _, node as *const _))
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelRoot {
    pub selectors: Vec<Selector>,
    pub trailing_comma: bool,
}

impl SelRoot {
    pub fn to_string(&self) -> String {
        let s = self
            .selectors
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if self.trailing_comma {
            format!("{s},")
        } else {
            s
        }
    }
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

/// `convertWhitespaceNodesToSpace` for a pure-whitespace run
fn descendant_combinator(space: &str) -> SelNode {
    let mut node = SelNode::combinator(" ");
    if space.ends_with(' ') {
        node.spaces_before = space[..space.len() - 1].to_string();
    } else if space.starts_with(' ') {
        node.spaces_after = space[1..].to_string();
    } else {
        node.rendered = space.to_string();
    }
    node
}

pub fn parse(input: &str) -> SelRoot {
    let chars: Vec<char> = input.chars().collect();
    let mut root = SelRoot::default();
    let mut current = Selector::default();
    let mut pending_spaces = String::new();
    let mut i = 0usize;

    macro_rules! push_node {
        ($node:expr) => {{
            let mut n = $node;
            if !pending_spaces.is_empty() {
                n.spaces_before = format!("{}{}", pending_spaces, n.spaces_before);
                pending_spaces.clear();
            }
            current.nodes.push(n);
        }};
    }

    while i < chars.len() {
        let c = chars[i];
        if is_ws(c) {
            let start = i;
            while i < chars.len() && is_ws(chars[i]) {
                i += 1;
            }
            let ws: String = chars[start..i].iter().collect();
            let all_comments = current
                .nodes
                .iter()
                .all(|n| n.kind == SelKind::Comment);
            if current.nodes.is_empty() || all_comments {
                pending_spaces.push_str(&ws);
            } else if i >= chars.len() || chars[i] == ',' {
                current.nodes.last_mut().unwrap().spaces_after = ws;
            } else if is_combinator_start(&chars, i) {
                pending_spaces.push_str(&ws);
            } else {
                push_node!(descendant_combinator(&ws));
            }
            continue;
        }

        match c {
            ',' => {
                root.selectors.push(std::mem::take(&mut current));
                i += 1;
                if chars[i..].iter().all(|c| is_ws(*c)) && i >= chars.len() {
                    root.trailing_comma = true;
                }
                continue;
            }
            '>' | '+' | '~' => {
                let start = i;
                while i < chars.len() && matches!(chars[i], '>' | '+' | '~') {
                    i += 1;
                }
                let value: String = chars[start..i].iter().collect();
                let mut node = SelNode::combinator(&value);
                // trailing whitespace belongs to the combinator
                let ws_start = i;
                while i < chars.len() && is_ws(chars[i]) {
                    i += 1;
                }
                if i > ws_start {
                    node.spaces_after = chars[ws_start..i].iter().collect();
                }
                push_node!(node);
                continue;
            }
            '/' if starts_with(&chars, i, "/deep/") => {
                let mut node = SelNode::combinator("/deep/");
                i += 6;
                let ws_start = i;
                while i < chars.len() && is_ws(chars[i]) {
                    i += 1;
                }
                if i > ws_start {
                    node.spaces_after = chars[ws_start..i].iter().collect();
                }
                push_node!(node);
                continue;
            }
            '/' if starts_with(&chars, i, "/*") => {
                let start = i;
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(chars.len());
                let text: String = chars[start..i].iter().collect();
                push_node!(SelNode::new(SelKind::Comment, &text, &text));
                continue;
            }
            '.' => {
                i += 1;
                let name = read_ident(&chars, &mut i);
                push_node!(SelNode::new(SelKind::Class, &name, &format!(".{name}")));
                continue;
            }
            '#' => {
                i += 1;
                let name = read_ident(&chars, &mut i);
                push_node!(SelNode::new(SelKind::Id, &name, &format!("#{name}")));
                continue;
            }
            '*' => {
                // `*|a` is a namespaced tag, not a universal selector
                if chars.get(i + 1) == Some(&'|') && chars.get(i + 2).is_some_and(|c| *c != '=') {
                    let start = i;
                    i += 2;
                    let name = read_ident(&chars, &mut i);
                    let text: String = chars[start..i].iter().collect();
                    let kind = if name == "*" {
                        SelKind::Universal
                    } else {
                        SelKind::Tag
                    };
                    push_node!(SelNode::new(kind, &name, &text));
                    continue;
                }
                i += 1;
                push_node!(SelNode::new(SelKind::Universal, "*", "*"));
                continue;
            }
            '&' => {
                i += 1;
                push_node!(SelNode::new(SelKind::Nesting, "&", "&"));
                continue;
            }
            '[' => {
                let start = i;
                i += 1;
                let mut quote: Option<char> = None;
                while i < chars.len() {
                    let ch = chars[i];
                    match quote {
                        Some(q) => {
                            if ch == '\\' {
                                i += 1;
                            } else if ch == q {
                                quote = None;
                            }
                        }
                        None => {
                            if ch == '\'' || ch == '"' {
                                quote = Some(ch);
                            } else if ch == ']' {
                                i += 1;
                                break;
                            }
                        }
                    }
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let inner: String = text[1..text.len().saturating_sub(1)].to_string();
                push_node!(SelNode::new(SelKind::Attribute, &inner, &text));
                continue;
            }
            '\'' | '"' => {
                let start = i;
                let q = c;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == q {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                push_node!(SelNode::new(SelKind::Str, &text, &text));
                continue;
            }
            ':' => {
                let start = i;
                i += 1;
                if i < chars.len() && chars[i] == ':' {
                    i += 1;
                }
                let _ = read_ident(&chars, &mut i);
                let name: String = chars[start..i].iter().collect();
                let mut node = SelNode::new(SelKind::Pseudo, &name, &name);
                if i < chars.len() && chars[i] == '(' {
                    let arg_start = i + 1;
                    let mut depth = 1;
                    let mut j = arg_start;
                    let mut quote: Option<char> = None;
                    while j < chars.len() {
                        let ch = chars[j];
                        match quote {
                            Some(q) => {
                                if ch == '\\' {
                                    j += 1;
                                } else if ch == q {
                                    quote = None;
                                }
                            }
                            None => {
                                if ch == '\'' || ch == '"' {
                                    quote = Some(ch);
                                } else if ch == '(' {
                                    depth += 1;
                                } else if ch == ')' {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                            }
                        }
                        j += 1;
                    }
                    let inner: String = chars[arg_start..j.min(chars.len())].iter().collect();
                    let inner_root = parse(&inner);
                    node.nodes = inner_root.selectors;
                    i = (j + 1).min(chars.len());
                }
                push_node!(node);
                continue;
            }
            _ => {
                let name = read_ident(&chars, &mut i);
                if name.is_empty() {
                    // unknown char: keep it as a tag node so output round-trips
                    let text: String = chars[i..i + 1].iter().collect();
                    i += 1;
                    push_node!(SelNode::new(SelKind::Tag, &text, &text));
                } else {
                    push_node!(SelNode::new(SelKind::Tag, &name, &name));
                }
                continue;
            }
        }
    }

    if !pending_spaces.is_empty() && current.nodes.is_empty() {
        // whitespace-only selector
        current
            .nodes
            .push(SelNode::new(SelKind::Tag, "", &pending_spaces));
    }
    root.selectors.push(current);
    root
}

fn starts_with(chars: &[char], i: usize, s: &str) -> bool {
    let target: Vec<char> = s.chars().collect();
    if i + target.len() > chars.len() {
        return false;
    }
    chars[i..i + target.len()] == target[..]
}

fn is_combinator_start(chars: &[char], i: usize) -> bool {
    if i >= chars.len() {
        return false;
    }
    matches!(chars[i], '>' | '+' | '~') || starts_with(chars, i, "/deep/")
}

fn read_ident(chars: &[char], i: &mut usize) -> String {
    let start = *i;
    while *i < chars.len() {
        let c = chars[*i];
        if c == '\\' {
            *i += 2;
            continue;
        }
        if c.is_alphanumeric() || c == '-' || c == '_' || (c as u32) > 127 || c == '%' {
            *i += 1;
        } else if c == '|' {
            // namespace separator
            *i += 1;
        } else {
            break;
        }
    }
    chars[start..*i].iter().collect()
}
