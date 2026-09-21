//! Port of `postcss@8.5.28/lib/parser.js`.

use super::node::*;
use super::tokenize::{Token, Tokenizer};

pub struct Parser {
    pub tree: CssTree,
    tokenizer: Tokenizer,
    current: usize,
    spaces: String,
    semicolon: bool,
    pub error: Option<String>,
}

fn is_space_or_comment(kind: &str) -> bool {
    kind == "space" || kind == "comment"
}

fn tokens_to_string(tokens: &[Token], from: usize, to: usize) -> String {
    tokens[from..to].iter().map(|t| t.value.clone()).collect()
}

impl Parser {
    pub fn new(css: &str) -> Self {
        let tree = CssTree::new();
        Parser {
            current: tree.root,
            tree,
            tokenizer: Tokenizer::new(css),
            spaces: String::new(),
            semicolon: false,
            error: None,
        }
    }

    pub fn parse(mut self) -> Result<CssTree, String> {
        while !self.tokenizer.end_of_file() {
            let token = match self.tokenizer.next_token() {
                Some(t) => t,
                None => break,
            };
            match token.kind.as_str() {
                "space" => self.spaces.push_str(&token.value),
                ";" => self.free_semicolon(&token),
                "}" => self.end(&token),
                "comment" => self.comment(&token),
                "at-word" => self.atrule(token),
                "{" => self.empty_rule(&token),
                _ => self.other(token),
            }
            if let Some(e) = &self.error {
                return Err(e.clone());
            }
            if let Some(e) = &self.tokenizer.error {
                return Err(e.clone());
            }
        }
        self.end_file();
        if let Some(e) = self.error {
            return Err(e);
        }
        Ok(self.tree)
    }

    fn init(&mut self, node: usize, offset: usize) {
        self.tree.push_child(self.current, node);
        self.tree.get_mut(node).start = offset;
        let spaces = std::mem::take(&mut self.spaces);
        self.tree.get_mut(node).raws.before = Some(spaces);
        if self.tree.get(node).kind != CssKind::Comment {
            self.semicolon = false;
        }
    }

    fn comment(&mut self, token: &Token) {
        let mut node = CssNode::new(CssKind::Comment);
        let text: String = token.value.chars().collect::<Vec<_>>()
            [2..token.value.chars().count() - 2]
            .iter()
            .collect();
        if text.trim().is_empty() {
            node.text = String::new();
            node.raws.left = Some(text);
            node.raws.right = Some(String::new());
        } else {
            let trimmed_start = text.len() - text.trim_start().len();
            let trimmed_end = text.len() - text.trim_end().len();
            node.raws.left = Some(text[..trimmed_start].to_string());
            node.text = text[trimmed_start..text.len() - trimmed_end].to_string();
            node.raws.right = Some(text[text.len() - trimmed_end..].to_string());
        }
        let id = self.tree.add(node);
        self.init(id, token.start.unwrap_or(0));
    }

    fn empty_rule(&mut self, token: &Token) {
        let mut node = CssNode::new(CssKind::Rule);
        node.nodes = Some(Vec::new());
        node.selector = String::new();
        node.raws.between = Some(String::new());
        let id = self.tree.add(node);
        self.init(id, token.start.unwrap_or(0));
        self.current = id;
    }

    fn end(&mut self, _token: &Token) {
        let has_nodes = self
            .tree
            .get(self.current)
            .nodes
            .as_ref()
            .map(|n| !n.is_empty())
            .unwrap_or(false);
        if has_nodes {
            self.tree.get_mut(self.current).raws.semicolon = Some(self.semicolon);
        }
        self.semicolon = false;
        let after = self
            .tree
            .get(self.current)
            .raws
            .after
            .clone()
            .unwrap_or_default();
        let spaces = std::mem::take(&mut self.spaces);
        self.tree.get_mut(self.current).raws.after = Some(format!("{after}{spaces}"));
        match self.tree.get(self.current).parent {
            Some(p) => self.current = p,
            None => self.error = Some("Unexpected }".into()),
        }
    }

    fn end_file(&mut self) {
        if self.tree.get(self.current).parent.is_some() {
            self.error = Some("Unclosed block".into());
        }
        let has_nodes = self
            .tree
            .get(self.current)
            .nodes
            .as_ref()
            .map(|n| !n.is_empty())
            .unwrap_or(false);
        if has_nodes {
            self.tree.get_mut(self.current).raws.semicolon = Some(self.semicolon);
        }
        let after = self
            .tree
            .get(self.current)
            .raws
            .after
            .clone()
            .unwrap_or_default();
        let spaces = std::mem::take(&mut self.spaces);
        self.tree.get_mut(self.current).raws.after = Some(format!("{after}{spaces}"));
    }

    fn free_semicolon(&mut self, token: &Token) {
        self.spaces.push_str(&token.value);
        let children = self.tree.children(self.current);
        if let Some(prev) = children.last().copied() {
            if self.tree.get(prev).kind == CssKind::Rule
                && self.tree.get(prev).raws.own_semicolon.is_none()
            {
                let spaces = std::mem::take(&mut self.spaces);
                self.tree.get_mut(prev).raws.own_semicolon = Some(spaces);
            }
        }
    }

    fn atrule(&mut self, token: Token) {
        let mut node = CssNode::new(CssKind::AtRule);
        node.name = token.value.chars().skip(1).collect();
        if node.name.is_empty() {
            self.error = Some("At-rule without name".into());
        }
        let id = self.tree.add(node);
        self.init(id, token.start.unwrap_or(0));

        let mut last = false;
        let mut open = false;
        let mut params: Vec<Token> = Vec::new();
        let mut brackets: Vec<String> = Vec::new();

        while !self.tokenizer.end_of_file() {
            let token = match self.tokenizer.next_token() {
                Some(t) => t,
                None => break,
            };
            let ty = token.kind.clone();
            if ty == "(" || ty == "[" {
                brackets.push(if ty == "(" { ")".into() } else { "]".into() });
            } else if ty == "{" && !brackets.is_empty() {
                brackets.push("}".into());
            } else if Some(&ty) == brackets.last() {
                brackets.pop();
            }

            if brackets.is_empty() {
                if ty == ";" {
                    self.semicolon = true;
                    break;
                } else if ty == "{" {
                    open = true;
                    break;
                } else if ty == "}" {
                    self.end(&token);
                    break;
                } else {
                    params.push(token);
                }
            } else {
                params.push(token);
            }

            if self.tokenizer.end_of_file() {
                last = true;
                break;
            }
        }

        let between = spaces_and_comments_from_end(&mut params);
        self.tree.get_mut(id).raws.between = Some(between.clone());
        if !params.is_empty() {
            let after_name = spaces_and_comments_from_start(&mut params);
            self.tree.get_mut(id).raws.after_name = Some(after_name);
            self.raw(id, "params", &params, false);
            if last {
                self.spaces = between;
                self.tree.get_mut(id).raws.between = Some(String::new());
            }
        } else {
            self.tree.get_mut(id).raws.after_name = Some(String::new());
            self.tree.get_mut(id).params = String::new();
        }

        if open {
            self.tree.get_mut(id).nodes = Some(Vec::new());
            self.current = id;
        }
    }

    fn other(&mut self, start: Token) {
        let mut end = false;
        let mut colon = false;
        let mut brackets: Vec<String> = Vec::new();
        let custom_property = start.value.starts_with("--");
        let mut tokens: Vec<Token> = Vec::new();
        let mut token = Some(start);

        while let Some(t) = token {
            let ty = t.kind.clone();
            tokens.push(t);

            if ty == "(" || ty == "[" {
                brackets.push(if ty == "(" { ")".into() } else { "]".into() });
            } else if custom_property && colon && ty == "{" {
                brackets.push("}".into());
            } else if brackets.is_empty() {
                if ty == ";" {
                    if colon {
                        self.decl(tokens, custom_property);
                        return;
                    } else {
                        break;
                    }
                } else if ty == "{" {
                    self.rule(tokens);
                    return;
                } else if ty == "}" {
                    let popped = tokens.pop().unwrap();
                    self.tokenizer.back(popped);
                    end = true;
                    break;
                } else if ty == ":" {
                    colon = true;
                }
            } else if Some(&ty) == brackets.last() {
                brackets.pop();
            }

            token = self.tokenizer.next_token();
        }

        if self.tokenizer.end_of_file() {
            end = true;
        }
        if !brackets.is_empty() {
            self.error = Some("Unclosed bracket".into());
            return;
        }

        if end && colon {
            if !custom_property {
                while !tokens.is_empty() {
                    let k = tokens.last().unwrap().kind.clone();
                    if !is_space_or_comment(&k) {
                        break;
                    }
                    let popped = tokens.pop().unwrap();
                    self.tokenizer.back(popped);
                }
            }
            self.decl(tokens, custom_property);
        } else {
            self.error = Some(format!("Unknown word {}", tokens[0].value));
        }
    }

    fn rule(&mut self, mut tokens: Vec<Token>) {
        tokens.pop();
        let mut node = CssNode::new(CssKind::Rule);
        node.nodes = Some(Vec::new());
        let id = self.tree.add(node);
        let offset = tokens[0].start.unwrap_or(0);
        self.init(id, offset);
        let between = spaces_and_comments_from_end(&mut tokens);
        self.tree.get_mut(id).raws.between = Some(between);
        self.raw(id, "selector", &tokens, false);
        self.current = id;
    }

    fn decl(&mut self, mut tokens: Vec<Token>, custom_property: bool) {
        let node = CssNode::new(CssKind::Decl);
        let id = self.tree.add(node);
        let offset = tokens[0].start.unwrap_or(0);
        self.init(id, offset);

        if tokens.last().map(|t| t.kind.as_str()) == Some(";") {
            self.semicolon = true;
            tokens.pop();
        }

        let mut start = 0usize;
        while tokens[start].kind != "word" {
            if start == tokens.len() - 1 {
                self.error = Some(format!("Unknown word {}", tokens[start].value));
                return;
            }
            start += 1;
        }
        let before = self.tree.get(id).raws.before.clone().unwrap_or_default();
        self.tree.get_mut(id).raws.before =
            Some(format!("{before}{}", tokens_to_string(&tokens, 0, start)));

        let prop_start = start;
        while start < tokens.len() {
            let ty = tokens[start].kind.as_str();
            if ty == ":" || ty == "space" || ty == "comment" {
                break;
            }
            start += 1;
        }
        self.tree.get_mut(id).prop = tokens_to_string(&tokens, prop_start, start);

        let between_start = start;
        while start < tokens.len() {
            let t = tokens[start].clone();
            start += 1;
            if t.kind == ":" {
                break;
            }
            if t.kind == "word" && t.value.chars().any(|c| c.is_alphanumeric() || c == '_') {
                self.error = Some(format!("Unknown word {}", t.value));
                return;
            }
        }
        self.tree.get_mut(id).raws.between =
            Some(tokens_to_string(&tokens, between_start, start));

        let prop = self.tree.get(id).prop.clone();
        if prop.starts_with('_') || prop.starts_with('*') {
            let before = self.tree.get(id).raws.before.clone().unwrap_or_default();
            let first = prop.chars().next().unwrap();
            self.tree.get_mut(id).raws.before = Some(format!("{before}{first}"));
            self.tree.get_mut(id).prop = prop.chars().skip(1).collect();
        }

        let first_spaces_start = start;
        while start < tokens.len() {
            let next = tokens[start].kind.as_str();
            if next != "space" && next != "comment" {
                break;
            }
            start += 1;
        }
        let mut first_spaces: Vec<Token> = tokens[first_spaces_start..start].to_vec();
        let mut rest: Vec<Token> = tokens[start..].to_vec();

        let mut i = rest.len();
        while i > 0 {
            i -= 1;
            let token = rest[i].clone();
            let lower = token.value.to_lowercase();
            if lower == "!important" {
                self.tree.get_mut(id).important = true;
                let mut string = string_from(&mut rest, i);
                string = format!("{}{string}", spaces_from_end(&mut rest));
                if string != " !important" {
                    self.tree.get_mut(id).raws.important = Some(string);
                }
                break;
            } else if lower == "important" {
                let mut cache = rest.clone();
                let mut str_ = String::new();
                let mut j = i;
                while j > 0 {
                    let ty = cache[j].kind.clone();
                    if str_.trim_start().starts_with('!') && ty != "space" {
                        break;
                    }
                    let popped = cache.pop().unwrap();
                    str_ = format!("{}{str_}", popped.value);
                    j -= 1;
                }
                if str_.trim_start().starts_with('!') {
                    self.tree.get_mut(id).important = true;
                    self.tree.get_mut(id).raws.important = Some(str_);
                    rest = cache;
                }
            }
            if !is_space_or_comment(&token.kind) {
                break;
            }
        }

        let has_word = rest.iter().any(|t| !is_space_or_comment(&t.kind));
        if has_word {
            let between = self.tree.get(id).raws.between.clone().unwrap_or_default();
            let extra: String = first_spaces.iter().map(|t| t.value.clone()).collect();
            self.tree.get_mut(id).raws.between = Some(format!("{between}{extra}"));
            first_spaces.clear();
        }
        let mut all = first_spaces;
        all.extend(rest);
        self.raw(id, "value", &all, custom_property);
    }

    fn raw(&mut self, node: usize, prop: &str, tokens: &[Token], custom_property: bool) {
        let length = tokens.len();
        let mut value = String::new();
        let mut clean = true;
        for i in 0..length {
            let token = &tokens[i];
            let ty = token.kind.as_str();
            if ty == "space" && i == length - 1 && !custom_property {
                clean = false;
            } else if ty == "comment" {
                let prev = if i > 0 {
                    tokens[i - 1].kind.as_str()
                } else {
                    "empty"
                };
                let next = if i + 1 < length {
                    tokens[i + 1].kind.as_str()
                } else {
                    "empty"
                };
                let safe = |k: &str| k == "empty" || k == "space";
                if !safe(prev) && !safe(next) {
                    if value.ends_with(',') {
                        clean = false;
                    } else {
                        value.push_str(&token.value);
                    }
                } else {
                    clean = false;
                }
            } else {
                value.push_str(&token.value);
            }
        }
        if !clean {
            let raw: String = tokens.iter().map(|t| t.value.clone()).collect();
            let pair = Some((raw, value.clone()));
            match prop {
                "value" => self.tree.get_mut(node).raws.value = pair,
                "selector" => self.tree.get_mut(node).raws.selector = pair,
                "params" => self.tree.get_mut(node).raws.params = pair,
                _ => {}
            }
        }
        match prop {
            "value" => self.tree.get_mut(node).value = value,
            "selector" => self.tree.get_mut(node).selector = value,
            "params" => self.tree.get_mut(node).params = value,
            _ => {}
        }
    }
}

fn spaces_and_comments_from_end(tokens: &mut Vec<Token>) -> String {
    let mut spaces = String::new();
    while let Some(last) = tokens.last() {
        if !is_space_or_comment(&last.kind) {
            break;
        }
        let t = tokens.pop().unwrap();
        spaces = format!("{}{spaces}", t.value);
    }
    spaces
}

fn spaces_and_comments_from_start(tokens: &mut Vec<Token>) -> String {
    let mut spaces = String::new();
    while let Some(first) = tokens.first() {
        if !is_space_or_comment(&first.kind) {
            break;
        }
        let t = tokens.remove(0);
        spaces.push_str(&t.value);
    }
    spaces
}

fn spaces_from_end(tokens: &mut Vec<Token>) -> String {
    let mut spaces = String::new();
    while let Some(last) = tokens.last() {
        if last.kind != "space" {
            break;
        }
        let t = tokens.pop().unwrap();
        spaces = format!("{}{spaces}", t.value);
    }
    spaces
}

fn string_from(tokens: &mut Vec<Token>, from: usize) -> String {
    let result: String = tokens[from..].iter().map(|t| t.value.clone()).collect();
    tokens.truncate(from);
    result
}

pub fn parse(css: &str) -> Result<CssTree, String> {
    Parser::new(css).parse()
}
