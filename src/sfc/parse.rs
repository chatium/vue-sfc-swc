//! Port of `compiler-sfc/src/parse.ts`.

use crate::core::ast::*;
use crate::core::errors::CompilerError;
use crate::core::parser::{ParseMode, base_parse};
use crate::dom::parser_options::dom_parser_options;

use super::css_vars::parse_css_vars;

#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    True,
    Str(String),
}

impl AttrValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttrValue::Str(s) => Some(s),
            AttrValue::True => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SfcBlock {
    pub block_type: String,
    pub content: String,
    pub attrs: Vec<(String, AttrValue)>,
    pub loc: SourceLocation,
    pub lang: Option<String>,
    pub src: Option<String>,
    // style-only
    pub scoped: bool,
    pub module: Option<AttrValue>,
    // script-only
    pub setup: Option<AttrValue>,
    // template-only
    /// the template block's children, in the parse arena
    pub ast: Option<Vec<NodeId>>,
}

impl SfcBlock {
    pub fn attr(&self, name: &str) -> Option<&AttrValue> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SfcDescriptor {
    pub filename: String,
    pub source: String,
    pub template: Option<SfcBlock>,
    pub script: Option<SfcBlock>,
    pub script_setup: Option<SfcBlock>,
    pub styles: Vec<SfcBlock>,
    pub custom_blocks: Vec<SfcBlock>,
    pub css_vars: Vec<String>,
    pub slotted: bool,
}

#[derive(Debug, Clone)]
pub struct SfcError {
    pub message: String,
    pub code: Option<i32>,
    pub loc: Option<SourceLocation>,
}

impl From<CompilerError> for SfcError {
    fn from(e: CompilerError) -> Self {
        SfcError {
            message: e.message,
            code: Some(e.code),
            loc: e.loc,
        }
    }
}

pub struct SfcParseResult {
    pub descriptor: SfcDescriptor,
    pub errors: Vec<SfcError>,
    /// owns the template AST nodes referenced by `descriptor.template.ast`
    pub arena: Arena,
}

#[derive(Debug, Clone)]
pub struct SfcParseOptions {
    pub filename: String,
    pub source_map: bool,
    pub ignore_empty: bool,
}

pub const DEFAULT_FILENAME: &str = "anonymous.vue";

impl Default for SfcParseOptions {
    fn default() -> Self {
        SfcParseOptions {
            filename: DEFAULT_FILENAME.to_string(),
            source_map: true,
            ignore_empty: true,
        }
    }
}

fn has_src(a: &Arena, node: &ElementNode) -> bool {
    node.props
        .iter()
        .any(|p| matches!(a.node(*p), Node::Attribute(attr) if attr.name == "src"))
}

fn is_empty(a: &Arena, node: &ElementNode) -> bool {
    node.children.iter().all(|c| match a.node(*c) {
        Node::Text(t) => t.content.trim().is_empty(),
        _ => false,
    })
}

fn create_block(a: &Arena, node: &ElementNode, source: &[u16]) -> SfcBlock {
    let block_type = node.tag.clone();
    let loc = node.inner_loc.clone().unwrap_or_else(loc_stub);
    let content = slice_utf16(source, loc.start.offset.max(0) as usize, loc.end.offset.max(0) as usize);
    let mut block = SfcBlock {
        block_type: block_type.clone(),
        content,
        attrs: Vec::new(),
        loc,
        lang: None,
        src: None,
        scoped: false,
        module: None,
        setup: None,
        ast: None,
    };
    for p in &node.props {
        if let Node::Attribute(attr) = a.node(*p) {
            let name = attr.name.clone();
            let value = match &attr.value {
                Some(v) if !v.content.is_empty() => AttrValue::Str(v.content.clone()),
                _ => AttrValue::True,
            };
            block.attrs.push((name.clone(), value.clone()));
            if name == "lang" {
                block.lang = attr.value.as_ref().map(|v| v.content.clone());
            } else if name == "src" {
                block.src = attr.value.as_ref().map(|v| v.content.clone());
            } else if block_type == "style" {
                if name == "scoped" {
                    block.scoped = true;
                } else if name == "module" {
                    block.module = Some(value);
                }
            } else if block_type == "script" && name == "setup" {
                block.setup = Some(value);
            }
        }
    }
    block
}

fn slice_utf16(source: &[u16], start: usize, end: usize) -> String {
    let end = end.min(source.len());
    if start >= end {
        return String::new();
    }
    String::from_utf16_lossy(&source[start..end])
}

fn duplicate_block_error(node: &ElementNode, is_script_setup: bool) -> SfcError {
    SfcError {
        message: format!(
            "Single file component can contain only one <{}{}> element",
            node.tag,
            if is_script_setup { " setup" } else { "" }
        ),
        code: None,
        loc: Some(node.loc.clone()),
    }
}

pub fn parse(source: &str, options: SfcParseOptions) -> SfcParseResult {
    let units: Vec<u16> = source.encode_utf16().collect();
    let mut descriptor = SfcDescriptor {
        filename: options.filename.clone(),
        source: source.to_string(),
        ..Default::default()
    };

    let mut parser_options = dom_parser_options();
    parser_options.parse_mode = ParseMode::Sfc;
    parser_options.prefix_identifiers = true;

    let parsed = base_parse(source, parser_options);
    let arena = &parsed.arena;
    let mut errors: Vec<SfcError> = parsed.errors.into_iter().map(SfcError::from).collect();

    let root_children = arena.root(parsed.root).children.clone();
    for child in &root_children {
        let node = match arena.node(*child) {
            Node::Element(e) => e.as_ref(),
            _ => continue,
        };
        if options.ignore_empty
            && node.tag != "template"
            && is_empty(arena, node)
            && !has_src(arena, node)
        {
            continue;
        }
        match node.tag.as_str() {
            "template" => {
                if descriptor.template.is_none() {
                    let mut block = create_block(arena, node, &units);
                    if block.attr("src").is_none() {
                        block.ast = Some(node.children.clone());
                    }
                    if block.attr("functional").is_some() {
                        let loc = node
                            .props
                            .iter()
                            .find(|p| {
                                matches!(arena.node(**p), Node::Attribute(a) if a.name == "functional")
                            })
                            .map(|p| arena.loc(*p).clone());
                        errors.push(SfcError {
                            message: "<template functional> is no longer supported in Vue 3, since \
functional components no longer have significant performance difference from stateful ones. \
Just use a normal <template> instead."
                                .to_string(),
                            code: None,
                            loc,
                        });
                    }
                    descriptor.template = Some(block);
                } else {
                    errors.push(duplicate_block_error(node, false));
                }
            }
            "script" => {
                let block = create_block(arena, node, &units);
                let is_setup = block.attr("setup").is_some();
                if is_setup && descriptor.script_setup.is_none() {
                    descriptor.script_setup = Some(block);
                } else if !is_setup && descriptor.script.is_none() {
                    descriptor.script = Some(block);
                } else {
                    errors.push(duplicate_block_error(node, is_setup));
                }
            }
            "style" => {
                let block = create_block(arena, node, &units);
                if block.attr("vars").is_some() {
                    errors.push(SfcError {
                        message: "<style vars> has been replaced by a new proposal: \
https://github.com/vuejs/rfcs/pull/231"
                            .to_string(),
                        code: None,
                        loc: None,
                    });
                }
                descriptor.styles.push(block);
            }
            _ => descriptor
                .custom_blocks
                .push(create_block(arena, node, &units)),
        }
    }

    if descriptor.template.is_none()
        && descriptor.script.is_none()
        && descriptor.script_setup.is_none()
    {
        errors.push(SfcError {
            message: format!(
                "At least one <template> or <script> is required in a single file component. {}",
                descriptor.filename
            ),
            code: None,
            loc: None,
        });
    }

    if descriptor.script_setup.is_some() {
        if descriptor
            .script_setup
            .as_ref()
            .is_some_and(|s| s.src.is_some())
        {
            errors.push(SfcError {
                message: "<script setup> cannot use the \"src\" attribute because its syntax \
will be ambiguous outside of the component."
                    .to_string(),
                code: None,
                loc: None,
            });
            descriptor.script_setup = None;
        }
        if descriptor.script.as_ref().is_some_and(|s| s.src.is_some()) {
            errors.push(SfcError {
                message: "<script> cannot use the \"src\" attribute when <script setup> is \
also present because they must be processed together."
                    .to_string(),
                code: None,
                loc: None,
            });
            descriptor.script = None;
        }
    }

    descriptor.css_vars = parse_css_vars(&descriptor);
    descriptor.slotted = descriptor
        .styles
        .iter()
        .any(|s| s.scoped && (s.content.contains("::v-slotted(") || s.content.contains(":slotted(")));

    SfcParseResult {
        descriptor,
        errors,
        arena: parsed.arena,
    }
}
