//! Port of `compiler-core/src/tokenizer.ts` + `compiler-core/src/parser.ts`.
//!
//! The two files are one state machine in practice (the parser callbacks mutate
//! tokenizer state and vice versa), so they are one struct here. All offsets are
//! UTF-16 code-unit indices, exactly like the JS original.

use std::sync::Arc;

use super::ast::*;
use super::entities::{DecodingMode, EntityDecoder, decode_with_trie, from_code_point};
use super::errors::{CompilerError, ErrorCode, create_compiler_error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    Base,
    Html,
    Sfc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitespaceStrategy {
    Preserve,
    Condense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum State {
    Text = 1,
    InterpolationOpen,
    Interpolation,
    InterpolationClose,
    BeforeTagName,
    InTagName,
    InSelfClosingTag,
    BeforeClosingTagName,
    InClosingTagName,
    AfterClosingTagName,
    BeforeAttrName,
    InAttrName,
    InDirName,
    InDirArg,
    InDirDynamicArg,
    InDirModifier,
    AfterAttrName,
    BeforeAttrValue,
    InAttrValueDq,
    InAttrValueSq,
    InAttrValueNq,
    BeforeDeclaration,
    InDeclaration,
    InProcessingInstruction,
    BeforeComment,
    CdataSequence,
    InSpecialComment,
    InCommentLike,
    BeforeSpecialS,
    BeforeSpecialT,
    SpecialStartSequence,
    InRcdata,
    InEntity,
    InSfcRootTagName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteType {
    NoValue = 0,
    Unquoted = 1,
    Single = 2,
    Double = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeqKind {
    None,
    Cdata,
    CdataEnd,
    CommentEnd,
    ScriptEnd,
    StyleEnd,
    TitleEnd,
    TextareaEnd,
    Custom,
}

const SEQ_CDATA: &[u16] = &[0x43, 0x44, 0x41, 0x54, 0x41, 0x5b];
const SEQ_CDATA_END: &[u16] = &[0x5d, 0x5d, 0x3e];
const SEQ_COMMENT_END: &[u16] = &[0x2d, 0x2d, 0x3e];
const SEQ_SCRIPT_END: &[u16] = &[0x3c, 0x2f, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74];
const SEQ_STYLE_END: &[u16] = &[0x3c, 0x2f, 0x73, 0x74, 0x79, 0x6c, 0x65];
const SEQ_TITLE_END: &[u16] = &[0x3c, 0x2f, 0x74, 0x69, 0x74, 0x6c, 0x65];
const SEQ_TEXTAREA_END: &[u16] = &[0x3c, 0x2f, 116, 101, 120, 116, 97, 114, 101, 97];

mod cc {
    pub const TAB: u32 = 0x9;
    pub const NEWLINE: u32 = 0xa;
    pub const FORM_FEED: u32 = 0xc;
    pub const CARRIAGE_RETURN: u32 = 0xd;
    pub const SPACE: u32 = 0x20;
    pub const EXCLAMATION_MARK: u32 = 0x21;
    pub const NUMBER: u32 = 0x23;
    pub const AMP: u32 = 0x26;
    pub const SINGLE_QUOTE: u32 = 0x27;
    pub const DOUBLE_QUOTE: u32 = 0x22;
    pub const GRAVE_ACCENT: u32 = 96;
    pub const DASH: u32 = 0x2d;
    pub const SLASH: u32 = 0x2f;
    pub const SEMI: u32 = 0x3b;
    pub const LT: u32 = 0x3c;
    pub const EQ: u32 = 0x3d;
    pub const GT: u32 = 0x3e;
    pub const QUESTIONMARK: u32 = 0x3f;
    pub const UPPER_A: u32 = 0x41;
    pub const LOWER_A: u32 = 0x61;
    pub const UPPER_Z: u32 = 0x5a;
    pub const LOWER_Z: u32 = 0x7a;
    pub const LOWER_V: u32 = 0x76;
    pub const DOT: u32 = 0x2e;
    pub const COLON: u32 = 0x3a;
    pub const AT: u32 = 0x40;
    pub const LEFT_SQUARE: u32 = 91;
    pub const RIGHT_SQUARE: u32 = 93;
}

pub fn is_whitespace(c: u32) -> bool {
    c == cc::SPACE
        || c == cc::NEWLINE
        || c == cc::TAB
        || c == cc::FORM_FEED
        || c == cc::CARRIAGE_RETURN
}

fn is_tag_start_char(c: u32) -> bool {
    (cc::LOWER_A..=cc::LOWER_Z).contains(&c) || (cc::UPPER_A..=cc::UPPER_Z).contains(&c)
}

fn is_end_of_tag_section(c: u32) -> bool {
    c == cc::SLASH || c == cc::GT || is_whitespace(c)
}

fn to_char_codes(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

type TagPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct ParserOptions {
    pub parse_mode: ParseMode,
    pub ns: Namespace,
    pub delimiters: (String, String),
    pub get_namespace: fn(&Arena, &str, Option<&ElementNode>, Namespace) -> Namespace,
    pub is_void_tag: fn(&str) -> bool,
    pub is_pre_tag: fn(&str) -> bool,
    pub is_ignore_newline_tag: fn(&str) -> bool,
    pub is_custom_element: Option<TagPredicate>,
    pub comments: bool,
    pub prefix_identifiers: bool,
    pub whitespace: WhitespaceStrategy,
    pub is_native_tag: Option<fn(&str) -> bool>,
    pub is_built_in_component: Option<fn(&str) -> Option<RuntimeHelper>>,
    pub expression_plugins: Vec<String>,
}

fn no(_: &str) -> bool {
    false
}
fn default_get_namespace(
    _: &Arena,
    _: &str,
    _: Option<&ElementNode>,
    ns: Namespace,
) -> Namespace {
    ns
}

impl Default for ParserOptions {
    fn default() -> Self {
        ParserOptions {
            parse_mode: ParseMode::Base,
            ns: Namespace::Html,
            delimiters: ("{{".into(), "}}".into()),
            get_namespace: default_get_namespace,
            is_void_tag: no,
            is_pre_tag: no,
            is_ignore_newline_tag: no,
            is_custom_element: None,
            comments: true,
            prefix_identifiers: false,
            whitespace: WhitespaceStrategy::Condense,
            is_native_tag: None,
            is_built_in_component: None,
            expression_plugins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpParseMode {
    Normal,
    Params,
    Statements,
    Skip,
}

struct OpenElement {
    el: ElementNode,
    id: u64,
}

pub struct Parser {
    // --- tokenizer state ---
    state: State,
    buffer: Vec<u16>,
    section_start: i64,
    index: usize,
    entity_start: usize,
    base_state: State,
    in_rcdata: bool,
    in_xml: bool,
    in_v_pre_tok: bool,
    newlines: Vec<usize>,
    mode: ParseMode,
    delimiter_open: Vec<u16>,
    delimiter_close: Vec<u16>,
    delimiter_index: i64,
    current_sequence: Vec<u16>,
    current_seq_kind: SeqKind,
    sequence_index: usize,
    entity_decoder: EntityDecoder,

    // --- parser state ---
    options: ParserOptions,
    arena: Arena,
    root_children: Vec<NodeId>,
    stack: Vec<OpenElement>,
    next_open_id: u64,
    current_open_tag: Option<OpenElement>,
    current_prop: Option<Node>,
    current_attr_value: Vec<u16>,
    current_attr_start_index: i64,
    current_attr_end_index: i64,
    in_pre: usize,
    in_v_pre: bool,
    current_v_pre_boundary: Option<u64>,
    pub errors: Vec<CompilerError>,
}

impl Parser {
    fn new(input: &str, options: ParserOptions) -> Self {
        let delimiter_open = to_char_codes(&options.delimiters.0);
        let delimiter_close = to_char_codes(&options.delimiters.1);
        let mode = options.parse_mode;
        let in_xml = options.ns == Namespace::Svg || options.ns == Namespace::MathMl;
        Parser {
            state: State::Text,
            buffer: input.encode_utf16().collect(),
            section_start: 0,
            index: 0,
            entity_start: 0,
            base_state: State::Text,
            in_rcdata: false,
            in_xml,
            in_v_pre_tok: false,
            newlines: Vec::new(),
            mode,
            delimiter_open,
            delimiter_close,
            delimiter_index: -1,
            current_sequence: Vec::new(),
            current_seq_kind: SeqKind::None,
            sequence_index: 0,
            entity_decoder: EntityDecoder::new(),
            options,
            arena: Arena::new(),
            root_children: Vec::new(),
            stack: Vec::new(),
            next_open_id: 1,
            current_open_tag: None,
            current_prop: None,
            current_attr_value: Vec::new(),
            current_attr_start_index: -1,
            current_attr_end_index: -1,
            in_pre: 0,
            in_v_pre: false,
            current_v_pre_boundary: None,
            errors: Vec::new(),
        }
    }

    // --- shared helpers ---

    fn char_at(&self, i: usize) -> u32 {
        self.buffer.get(i).map(|c| *c as u32).unwrap_or(u32::MAX)
    }

    fn peek(&self) -> u32 {
        self.char_at(self.index + 1)
    }

    fn in_sfc_root(&self) -> bool {
        self.mode == ParseMode::Sfc && self.stack.is_empty()
    }

    fn get_slice(&self, start: usize, end: usize) -> String {
        let end = end.min(self.buffer.len());
        if start >= end {
            return String::new();
        }
        String::from_utf16_lossy(&self.buffer[start..end])
    }

    pub fn get_pos(&self, index: usize) -> Position {
        let mut line = 1i64;
        let mut column = index as i64 + 1;
        let length = self.newlines.len();
        let mut j: i64 = -1;
        if length > 100 {
            let mut l: i64 = -1;
            let mut r: i64 = length as i64;
            while l + 1 < r {
                let m = ((l + r) as usize) >> 1;
                if self.newlines[m] < index {
                    l = m as i64;
                } else {
                    r = m as i64;
                }
            }
            j = l;
        } else {
            for i in (0..length).rev() {
                if index > self.newlines[i] {
                    j = i as i64;
                    break;
                }
            }
        }
        if j >= 0 {
            line = j + 2;
            column = (index - self.newlines[j as usize]) as i64;
        }
        Position {
            column,
            line,
            offset: index as i64,
        }
    }

    /// `getPos` for the negative offsets the JS parser can produce on
    /// truncated input (`sectionStart === -1`).
    fn get_pos_signed(&self, index: i64) -> Position {
        if index < 0 {
            Position {
                column: index + 1,
                line: 1,
                offset: index,
            }
        } else {
            self.get_pos(index as usize)
        }
    }

    /// `String.prototype.slice` semantics for a possibly negative start.
    fn get_slice_signed(&self, start: i64, end: usize) -> String {
        let start = if start < 0 {
            (self.buffer.len() as i64 + start).max(0) as usize
        } else {
            start as usize
        };
        self.get_slice(start, end)
    }

    fn get_loc(&self, start: usize, end: usize) -> SourceLocation {
        SourceLocation {
            start: self.get_pos(start),
            end: self.get_pos(end),
            source: self.get_slice(start, end),
        }
    }

    /// `getLoc(start)` — end/source filled in later by `set_loc_end`.
    fn get_loc_open(&self, start: usize) -> SourceLocation {
        SourceLocation {
            start: self.get_pos(start),
            end: Position::default(),
            source: String::new(),
        }
    }

    fn set_loc_end(&self, loc: &mut SourceLocation, end: usize) {
        loc.end = self.get_pos(end);
        loc.source = self.get_slice(loc.start.offset.max(0) as usize, end);
    }

    fn emit_error(&mut self, code: ErrorCode, index: usize) {
        let loc = self.get_loc(index, index);
        self.errors
            .push(create_compiler_error(code, Some(loc), None));
    }

    fn emit_error_msg(&mut self, code: ErrorCode, index: usize, message: &str) {
        let loc = self.get_loc(index, index);
        self.errors
            .push(create_compiler_error(code, Some(loc), Some(message)));
    }

    // --- tokenizer ---

    fn state_text(&mut self, c: u32) {
        if c == cc::LT {
            if self.index as i64 > self.section_start {
                let (s, e) = (self.section_start as usize, self.index);
                self.on_text_slice(s, e);
            }
            self.state = State::BeforeTagName;
            self.section_start = self.index as i64;
        } else if c == cc::AMP {
            self.start_entity();
        } else if !self.in_v_pre_tok && c == self.delimiter_open[0] as u32 {
            self.state = State::InterpolationOpen;
            self.delimiter_index = 0;
            self.state_interpolation_open(c);
        }
    }

    fn state_interpolation_open(&mut self, c: u32) {
        if c == self.delimiter_open[self.delimiter_index as usize] as u32 {
            if self.delimiter_index as usize == self.delimiter_open.len() - 1 {
                let start = self.index + 1 - self.delimiter_open.len();
                if start as i64 > self.section_start {
                    let s = self.section_start as usize;
                    self.on_text_slice(s, start);
                }
                self.state = State::Interpolation;
                self.section_start = start as i64;
            } else {
                self.delimiter_index += 1;
            }
        } else if self.in_rcdata {
            self.state = State::InRcdata;
            self.state_in_rcdata(c);
        } else {
            self.state = State::Text;
            self.state_text(c);
        }
    }

    fn state_interpolation(&mut self, c: u32) {
        if c == self.delimiter_close[0] as u32 {
            self.state = State::InterpolationClose;
            self.delimiter_index = 0;
            self.state_interpolation_close(c);
        }
    }

    fn state_interpolation_close(&mut self, c: u32) {
        if c == self.delimiter_close[self.delimiter_index as usize] as u32 {
            if self.delimiter_index as usize == self.delimiter_close.len() - 1 {
                let (s, e) = (self.section_start as usize, self.index + 1);
                self.on_interpolation(s, e);
                self.state = if self.in_rcdata {
                    State::InRcdata
                } else {
                    State::Text
                };
                self.section_start = self.index as i64 + 1;
            } else {
                self.delimiter_index += 1;
            }
        } else {
            self.state = State::Interpolation;
            self.state_interpolation(c);
        }
    }

    fn state_special_start_sequence(&mut self, c: u32) {
        let is_end = self.sequence_index == self.current_sequence.len();
        let is_match = if is_end {
            is_end_of_tag_section(c)
        } else {
            (c | 0x20) == self.current_sequence[self.sequence_index] as u32
        };
        if !is_match {
            self.in_rcdata = false;
        } else if !is_end {
            self.sequence_index += 1;
            return;
        }
        self.sequence_index = 0;
        self.state = State::InTagName;
        self.state_in_tag_name(c);
    }

    fn state_in_rcdata(&mut self, c: u32) {
        if self.sequence_index == self.current_sequence.len() {
            if c == cc::GT || is_whitespace(c) {
                let end_of_text = self.index - self.current_sequence.len();
                if self.section_start < end_of_text as i64 {
                    let actual_index = self.index;
                    self.index = end_of_text;
                    let s = self.section_start as usize;
                    self.on_text_slice(s, end_of_text);
                    self.index = actual_index;
                }
                self.section_start = end_of_text as i64 + 2;
                self.state_in_closing_tag_name(c);
                self.in_rcdata = false;
                return;
            }
            self.sequence_index = 0;
        }
        if (c | 0x20) == self.current_sequence[self.sequence_index] as u32 {
            self.sequence_index += 1;
        } else if self.sequence_index == 0 {
            if self.current_seq_kind == SeqKind::TitleEnd
                || (self.current_seq_kind == SeqKind::TextareaEnd && !self.in_sfc_root())
            {
                if c == cc::AMP {
                    self.start_entity();
                } else if !self.in_v_pre_tok && c == self.delimiter_open[0] as u32 {
                    self.state = State::InterpolationOpen;
                    self.delimiter_index = 0;
                    self.state_interpolation_open(c);
                }
            } else if self.fast_forward_to(cc::LT) {
                self.sequence_index = 1;
            }
        } else {
            self.sequence_index = usize::from(c == cc::LT);
        }
    }

    fn state_cdata_sequence(&mut self, c: u32) {
        if c == SEQ_CDATA[self.sequence_index] as u32 {
            self.sequence_index += 1;
            if self.sequence_index == SEQ_CDATA.len() {
                self.state = State::InCommentLike;
                self.current_sequence = SEQ_CDATA_END.to_vec();
                self.current_seq_kind = SeqKind::CdataEnd;
                self.sequence_index = 0;
                self.section_start = self.index as i64 + 1;
            }
        } else {
            self.sequence_index = 0;
            self.state = State::InDeclaration;
            self.state_in_declaration(c);
        }
    }

    fn fast_forward_to(&mut self, c: u32) -> bool {
        loop {
            self.index += 1;
            if self.index >= self.buffer.len() {
                break;
            }
            let cc_ = self.buffer[self.index] as u32;
            if cc_ == cc::NEWLINE {
                self.newlines.push(self.index);
            }
            if cc_ == c {
                return true;
            }
        }
        self.index = self.buffer.len().saturating_sub(1);
        false
    }

    fn state_in_comment_like(&mut self, c: u32) {
        if c == self.current_sequence[self.sequence_index] as u32 {
            self.sequence_index += 1;
            if self.sequence_index == self.current_sequence.len() {
                let (s, e) = (self.section_start as usize, self.index - 2);
                if self.current_seq_kind == SeqKind::CdataEnd {
                    self.on_cdata(s, e);
                } else {
                    self.on_comment(s, e);
                }
                self.sequence_index = 0;
                self.section_start = self.index as i64 + 1;
                self.state = State::Text;
            }
        } else if self.sequence_index == 0 {
            let first = self.current_sequence[0] as u32;
            if self.fast_forward_to(first) {
                self.sequence_index = 1;
            }
        } else if c != self.current_sequence[self.sequence_index - 1] as u32 {
            self.sequence_index = 0;
        }
    }

    fn start_special(&mut self, sequence: &'static [u16], kind: SeqKind, offset: usize) {
        self.enter_rcdata(sequence.to_vec(), kind, offset);
        self.state = State::SpecialStartSequence;
    }

    fn enter_rcdata(&mut self, sequence: Vec<u16>, kind: SeqKind, offset: usize) {
        self.in_rcdata = true;
        self.current_sequence = sequence;
        self.current_seq_kind = kind;
        self.sequence_index = offset;
    }

    fn state_before_tag_name(&mut self, c: u32) {
        if c == cc::EXCLAMATION_MARK {
            self.state = State::BeforeDeclaration;
            self.section_start = self.index as i64 + 1;
        } else if c == cc::QUESTIONMARK {
            self.state = State::InProcessingInstruction;
            self.section_start = self.index as i64 + 1;
        } else if is_tag_start_char(c) {
            self.section_start = self.index as i64;
            if self.mode == ParseMode::Base {
                self.state = State::InTagName;
            } else if self.in_sfc_root() {
                self.state = State::InSfcRootTagName;
            } else if !self.in_xml {
                if c == 116 {
                    self.state = State::BeforeSpecialT;
                } else {
                    self.state = if c == 115 {
                        State::BeforeSpecialS
                    } else {
                        State::InTagName
                    };
                }
            } else {
                self.state = State::InTagName;
            }
        } else if c == cc::SLASH {
            self.state = State::BeforeClosingTagName;
        } else {
            self.state = State::Text;
            self.state_text(c);
        }
    }

    fn state_in_tag_name(&mut self, c: u32) {
        if is_end_of_tag_section(c) {
            self.handle_tag_name(c);
        }
    }

    fn state_in_sfc_root_tag_name(&mut self, c: u32) {
        if is_end_of_tag_section(c) {
            let tag = self.get_slice(self.section_start as usize, self.index);
            if tag != "template" {
                let seq = to_char_codes(&format!("</{tag}"));
                self.enter_rcdata(seq, SeqKind::Custom, 0);
            }
            self.handle_tag_name(c);
        }
    }

    fn handle_tag_name(&mut self, c: u32) {
        let (s, e) = (self.section_start as usize, self.index);
        self.on_open_tag_name(s, e);
        self.section_start = -1;
        self.state = State::BeforeAttrName;
        self.state_before_attr_name(c);
    }

    fn state_before_closing_tag_name(&mut self, c: u32) {
        if is_whitespace(c) {
        } else if c == cc::GT {
            let i = self.index;
            self.emit_error(ErrorCode::MISSING_END_TAG_NAME, i);
            self.state = State::Text;
            self.section_start = self.index as i64 + 1;
        } else {
            self.state = if is_tag_start_char(c) {
                State::InClosingTagName
            } else {
                State::InSpecialComment
            };
            self.section_start = self.index as i64;
        }
    }

    fn state_in_closing_tag_name(&mut self, c: u32) {
        if c == cc::GT || is_whitespace(c) {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_close_tag_cb(s, e);
            self.section_start = -1;
            self.state = State::AfterClosingTagName;
            self.state_after_closing_tag_name(c);
        }
    }

    fn state_after_closing_tag_name(&mut self, c: u32) {
        if c == cc::GT {
            self.state = State::Text;
            self.section_start = self.index as i64 + 1;
        }
    }

    fn state_before_attr_name(&mut self, c: u32) {
        if c == cc::GT {
            let i = self.index;
            self.end_open_tag(i);
            self.state = if self.in_rcdata {
                State::InRcdata
            } else {
                State::Text
            };
            self.section_start = self.index as i64 + 1;
        } else if c == cc::SLASH {
            self.state = State::InSelfClosingTag;
            if self.peek() != cc::GT {
                let i = self.index;
                self.emit_error(ErrorCode::UNEXPECTED_SOLIDUS_IN_TAG, i);
            }
        } else if c == cc::LT && self.peek() == cc::SLASH {
            let i = self.index;
            self.end_open_tag(i);
            self.state = State::BeforeTagName;
            self.section_start = self.index as i64;
        } else if !is_whitespace(c) {
            if c == cc::EQ {
                let i = self.index;
                self.emit_error(ErrorCode::UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME, i);
            }
            self.handle_attr_start(c);
        }
    }

    fn handle_attr_start(&mut self, c: u32) {
        if c == cc::LOWER_V && self.peek() == cc::DASH {
            self.state = State::InDirName;
            self.section_start = self.index as i64;
        } else if c == cc::DOT || c == cc::COLON || c == cc::AT || c == cc::NUMBER {
            let i = self.index;
            self.on_dir_name(i, i + 1);
            self.state = State::InDirArg;
            self.section_start = self.index as i64 + 1;
        } else {
            self.state = State::InAttrName;
            self.section_start = self.index as i64;
        }
    }

    fn state_in_self_closing_tag(&mut self, c: u32) {
        if c == cc::GT {
            let i = self.index;
            self.on_self_closing_tag(i);
            self.state = State::Text;
            self.section_start = self.index as i64 + 1;
            self.in_rcdata = false;
        } else if !is_whitespace(c) {
            self.state = State::BeforeAttrName;
            self.state_before_attr_name(c);
        }
    }

    fn state_in_attr_name(&mut self, c: u32) {
        if c == cc::EQ || is_end_of_tag_section(c) {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_attrib_name(s, e);
            self.handle_attr_name_end(c);
        } else if c == cc::DOUBLE_QUOTE || c == cc::SINGLE_QUOTE || c == cc::LT {
            let i = self.index;
            self.emit_error(ErrorCode::UNEXPECTED_CHARACTER_IN_ATTRIBUTE_NAME, i);
        }
    }

    fn state_in_dir_name(&mut self, c: u32) {
        if c == cc::EQ || is_end_of_tag_section(c) {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_dir_name(s, e);
            self.handle_attr_name_end(c);
        } else if c == cc::COLON {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_dir_name(s, e);
            self.state = State::InDirArg;
            self.section_start = self.index as i64 + 1;
        } else if c == cc::DOT {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_dir_name(s, e);
            self.state = State::InDirModifier;
            self.section_start = self.index as i64 + 1;
        }
    }

    fn state_in_dir_arg(&mut self, c: u32) {
        if c == cc::EQ || is_end_of_tag_section(c) {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_dir_arg(s, e);
            self.handle_attr_name_end(c);
        } else if c == cc::LEFT_SQUARE {
            self.state = State::InDirDynamicArg;
        } else if c == cc::DOT {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_dir_arg(s, e);
            self.state = State::InDirModifier;
            self.section_start = self.index as i64 + 1;
        }
    }

    fn state_in_dynamic_dir_arg(&mut self, c: u32) {
        if c == cc::RIGHT_SQUARE {
            self.state = State::InDirArg;
        } else if c == cc::EQ || is_end_of_tag_section(c) {
            let (s, e) = (self.section_start as usize, self.index + 1);
            self.on_dir_arg(s, e);
            self.handle_attr_name_end(c);
            let i = self.index;
            self.emit_error(ErrorCode::X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END, i);
        }
    }

    fn state_in_dir_modifier(&mut self, c: u32) {
        if c == cc::EQ || is_end_of_tag_section(c) {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_dir_modifier(s, e);
            self.handle_attr_name_end(c);
        } else if c == cc::DOT {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_dir_modifier(s, e);
            self.section_start = self.index as i64 + 1;
        }
    }

    fn handle_attr_name_end(&mut self, c: u32) {
        self.section_start = self.index as i64;
        self.state = State::AfterAttrName;
        let i = self.index;
        self.on_attrib_name_end(i);
        self.state_after_attr_name(c);
    }

    fn state_after_attr_name(&mut self, c: u32) {
        if c == cc::EQ {
            self.state = State::BeforeAttrValue;
        } else if c == cc::SLASH || c == cc::GT {
            let s = self.section_start as usize;
            self.on_attrib_end(QuoteType::NoValue, s);
            self.section_start = -1;
            self.state = State::BeforeAttrName;
            self.state_before_attr_name(c);
        } else if !is_whitespace(c) {
            let s = self.section_start as usize;
            self.on_attrib_end(QuoteType::NoValue, s);
            self.handle_attr_start(c);
        }
    }

    fn state_before_attr_value(&mut self, c: u32) {
        if c == cc::DOUBLE_QUOTE {
            self.state = State::InAttrValueDq;
            self.section_start = self.index as i64 + 1;
        } else if c == cc::SINGLE_QUOTE {
            self.state = State::InAttrValueSq;
            self.section_start = self.index as i64 + 1;
        } else if !is_whitespace(c) {
            self.section_start = self.index as i64;
            self.state = State::InAttrValueNq;
            self.state_in_attr_value_no_quotes(c);
        }
    }

    fn handle_in_attr_value(&mut self, c: u32, quote: u32) {
        if c == quote {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_attrib_data(s, e);
            self.section_start = -1;
            let qt = if quote == cc::DOUBLE_QUOTE {
                QuoteType::Double
            } else {
                QuoteType::Single
            };
            let end = self.index + 1;
            self.on_attrib_end(qt, end);
            self.state = State::BeforeAttrName;
        } else if c == cc::AMP {
            self.start_entity();
        }
    }

    fn state_in_attr_value_no_quotes(&mut self, c: u32) {
        if is_whitespace(c) || c == cc::GT {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_attrib_data(s, e);
            self.section_start = -1;
            let end = self.index;
            self.on_attrib_end(QuoteType::Unquoted, end);
            self.state = State::BeforeAttrName;
            self.state_before_attr_name(c);
        } else if c == cc::DOUBLE_QUOTE
            || c == cc::SINGLE_QUOTE
            || c == cc::LT
            || c == cc::EQ
            || c == cc::GRAVE_ACCENT
        {
            let i = self.index;
            self.emit_error(
                ErrorCode::UNEXPECTED_CHARACTER_IN_UNQUOTED_ATTRIBUTE_VALUE,
                i,
            );
        } else if c == cc::AMP {
            self.start_entity();
        }
    }

    fn state_before_declaration(&mut self, c: u32) {
        if c == cc::LEFT_SQUARE {
            self.state = State::CdataSequence;
            self.sequence_index = 0;
        } else {
            self.state = if c == cc::DASH {
                State::BeforeComment
            } else {
                State::InDeclaration
            };
        }
    }

    fn state_in_declaration(&mut self, c: u32) {
        if c == cc::GT || self.fast_forward_to(cc::GT) {
            self.state = State::Text;
            self.section_start = self.index as i64 + 1;
        }
    }

    fn state_in_processing_instruction(&mut self, c: u32) {
        if c == cc::GT || self.fast_forward_to(cc::GT) {
            let s = self.section_start as usize;
            self.on_processing_instruction(s);
            self.state = State::Text;
            self.section_start = self.index as i64 + 1;
        }
    }

    fn state_before_comment(&mut self, c: u32) {
        if c == cc::DASH {
            self.state = State::InCommentLike;
            self.current_sequence = SEQ_COMMENT_END.to_vec();
            self.current_seq_kind = SeqKind::CommentEnd;
            self.sequence_index = 2;
            self.section_start = self.index as i64 + 1;
        } else {
            self.state = State::InDeclaration;
        }
    }

    fn state_in_special_comment(&mut self, c: u32) {
        if c == cc::GT || self.fast_forward_to(cc::GT) {
            let (s, e) = (self.section_start as usize, self.index);
            self.on_comment(s, e);
            self.state = State::Text;
            self.section_start = self.index as i64 + 1;
        }
    }

    fn state_before_special_s(&mut self, c: u32) {
        if c == SEQ_SCRIPT_END[3] as u32 {
            self.start_special(SEQ_SCRIPT_END, SeqKind::ScriptEnd, 4);
        } else if c == SEQ_STYLE_END[3] as u32 {
            self.start_special(SEQ_STYLE_END, SeqKind::StyleEnd, 4);
        } else {
            self.state = State::InTagName;
            self.state_in_tag_name(c);
        }
    }

    fn state_before_special_t(&mut self, c: u32) {
        if c == SEQ_TITLE_END[3] as u32 {
            self.start_special(SEQ_TITLE_END, SeqKind::TitleEnd, 4);
        } else if c == SEQ_TEXTAREA_END[3] as u32 {
            self.start_special(SEQ_TEXTAREA_END, SeqKind::TextareaEnd, 4);
        } else {
            self.state = State::InTagName;
            self.state_in_tag_name(c);
        }
    }

    fn start_entity(&mut self) {
        self.base_state = self.state;
        self.state = State::InEntity;
        self.entity_start = self.index;
        let mode = if self.base_state == State::Text || self.base_state == State::InRcdata {
            DecodingMode::Legacy
        } else {
            DecodingMode::Attribute
        };
        self.entity_decoder.start_entity(mode);
    }

    fn state_in_entity(&mut self) {
        let buffer = std::mem::take(&mut self.buffer);
        let length = self.entity_decoder.write(&buffer, self.index);
        self.buffer = buffer;
        let emitted: Vec<(u32, usize)> = self.entity_decoder.emitted.drain(..).collect();
        for (cp, consumed) in emitted {
            self.emit_code_point(cp, consumed);
        }
        if length >= 0 {
            self.state = self.base_state;
            if length == 0 {
                self.index = self.entity_start;
            }
        } else {
            self.index = self.buffer.len().saturating_sub(1);
        }
    }

    fn emit_code_point(&mut self, cp: u32, consumed: usize) {
        let ch = String::from_utf16_lossy(&from_code_point(cp));
        if self.base_state != State::Text && self.base_state != State::InRcdata {
            if self.section_start < self.entity_start as i64 {
                let (s, e) = (self.section_start as usize, self.entity_start);
                self.on_attrib_data(s, e);
            }
            self.section_start = (self.entity_start + consumed) as i64;
            self.index = self.section_start as usize - 1;
            let (s, e) = (self.entity_start, self.section_start as usize);
            self.on_attrib_entity(&ch, s, e);
        } else {
            if self.section_start < self.entity_start as i64 {
                let (s, e) = (self.section_start as usize, self.entity_start);
                self.on_text_slice(s, e);
            }
            self.section_start = (self.entity_start + consumed) as i64;
            self.index = self.section_start as usize - 1;
            let (s, e) = (self.entity_start, self.section_start as usize);
            self.on_text(ch, s, e);
        }
    }

    fn tokenize(&mut self) {
        while self.index < self.buffer.len() {
            let c = self.buffer[self.index] as u32;
            if c == cc::NEWLINE && self.state != State::InEntity {
                self.newlines.push(self.index);
            }
            match self.state {
                State::Text => self.state_text(c),
                State::InterpolationOpen => self.state_interpolation_open(c),
                State::Interpolation => self.state_interpolation(c),
                State::InterpolationClose => self.state_interpolation_close(c),
                State::SpecialStartSequence => self.state_special_start_sequence(c),
                State::InRcdata => self.state_in_rcdata(c),
                State::CdataSequence => self.state_cdata_sequence(c),
                State::InAttrValueDq => self.handle_in_attr_value(c, cc::DOUBLE_QUOTE),
                State::InAttrName => self.state_in_attr_name(c),
                State::InDirName => self.state_in_dir_name(c),
                State::InDirArg => self.state_in_dir_arg(c),
                State::InDirDynamicArg => self.state_in_dynamic_dir_arg(c),
                State::InDirModifier => self.state_in_dir_modifier(c),
                State::InCommentLike => self.state_in_comment_like(c),
                State::InSpecialComment => self.state_in_special_comment(c),
                State::BeforeAttrName => self.state_before_attr_name(c),
                State::InTagName => self.state_in_tag_name(c),
                State::InSfcRootTagName => self.state_in_sfc_root_tag_name(c),
                State::InClosingTagName => self.state_in_closing_tag_name(c),
                State::BeforeTagName => self.state_before_tag_name(c),
                State::AfterAttrName => self.state_after_attr_name(c),
                State::InAttrValueSq => self.handle_in_attr_value(c, cc::SINGLE_QUOTE),
                State::BeforeAttrValue => self.state_before_attr_value(c),
                State::BeforeClosingTagName => self.state_before_closing_tag_name(c),
                State::AfterClosingTagName => self.state_after_closing_tag_name(c),
                State::BeforeSpecialS => self.state_before_special_s(c),
                State::BeforeSpecialT => self.state_before_special_t(c),
                State::InAttrValueNq => self.state_in_attr_value_no_quotes(c),
                State::InSelfClosingTag => self.state_in_self_closing_tag(c),
                State::InDeclaration => self.state_in_declaration(c),
                State::BeforeDeclaration => self.state_before_declaration(c),
                State::BeforeComment => self.state_before_comment(c),
                State::InProcessingInstruction => self.state_in_processing_instruction(c),
                State::InEntity => self.state_in_entity(),
            }
            self.index += 1;
        }
        self.cleanup();
        self.finish();
    }

    fn cleanup(&mut self) {
        if self.section_start != self.index as i64 {
            if self.state == State::Text
                || (self.state == State::InRcdata && self.sequence_index == 0)
            {
                let (s, e) = (self.section_start as usize, self.index);
                self.on_text_slice(s, e);
                self.section_start = self.index as i64;
            } else if self.state == State::InAttrValueDq
                || self.state == State::InAttrValueSq
                || self.state == State::InAttrValueNq
            {
                let (s, e) = (self.section_start as usize, self.index);
                self.on_attrib_data(s, e);
                self.section_start = self.index as i64;
            }
        }
    }

    fn finish(&mut self) {
        if self.state == State::InEntity {
            let consumed = self.entity_decoder.end();
            let _ = consumed;
            let emitted: Vec<(u32, usize)> = self.entity_decoder.emitted.drain(..).collect();
            for (cp, c) in emitted {
                self.emit_code_point(cp, c);
            }
            self.state = self.base_state;
        }
        self.handle_trailing_data();
        self.on_end();
    }

    fn handle_trailing_data(&mut self) {
        let end_index = self.buffer.len();
        if self.section_start >= end_index as i64 {
            return;
        }
        let s = self.section_start.max(0) as usize;
        if self.state == State::InCommentLike {
            if self.current_seq_kind == SeqKind::CdataEnd {
                self.on_cdata(s, end_index);
            } else {
                self.on_comment(s, end_index);
            }
        } else if matches!(
            self.state,
            State::InTagName
                | State::BeforeAttrName
                | State::BeforeAttrValue
                | State::AfterAttrName
                | State::InAttrName
                | State::InDirName
                | State::InDirArg
                | State::InDirDynamicArg
                | State::InDirModifier
                | State::InAttrValueSq
                | State::InAttrValueDq
                | State::InAttrValueNq
                | State::InClosingTagName
        ) {
            // tag is ignored
        } else if self.section_start < 0 {
            let content = self.get_slice_signed(self.section_start, end_index);
            let start_pos = self.get_pos_signed(self.section_start);
            self.on_text_at(content, start_pos, end_index);
        } else {
            self.on_text_slice(s, end_index);
        }
    }

    // --- parser callbacks ---

    fn on_text_slice(&mut self, start: usize, end: usize) {
        let content = self.get_slice(start, end);
        self.on_text(content, start, end);
    }

    fn on_text(&mut self, content: String, start: usize, end: usize) {
        let last = self.current_children().last().copied();
        let merge = matches!(last, Some(id) if self.arena.is(id, NodeType::Text));
        if merge {
            let id = last.unwrap();
            let start_off = self.arena.text(id).loc.start.offset;
            let end_pos = self.get_pos(end);
            let src = self.get_slice(start_off.max(0) as usize, end);
            let t = self.arena.text_mut(id);
            t.content.push_str(&content);
            t.loc.end = end_pos;
            t.loc.source = src;
        } else {
            let loc = self.get_loc(start, end);
            let id = self.arena.add(Node::Text(Box::new(TextNode { content, loc })));
            self.current_children_mut().push(id);
        }
    }

    fn on_text_at(&mut self, content: String, start_pos: Position, end: usize) {
        let last = self.current_children().last().copied();
        let merge = matches!(last, Some(id) if self.arena.is(id, NodeType::Text));
        if merge {
            let id = last.unwrap();
            let start_off = self.arena.text(id).loc.start.offset;
            let end_pos = self.get_pos(end);
            let src = self.get_slice(start_off.max(0) as usize, end);
            let t = self.arena.text_mut(id);
            t.content.push_str(&content);
            t.loc.end = end_pos;
            t.loc.source = src;
        } else {
            let loc = SourceLocation {
                start: start_pos,
                end: self.get_pos(end),
                source: content.clone(),
            };
            let id = self.arena.add(Node::Text(Box::new(TextNode { content, loc })));
            self.current_children_mut().push(id);
        }
    }

    fn current_children(&self) -> &Vec<NodeId> {
        match self.stack.first() {
            Some(top) => &top.el.children,
            None => &self.root_children,
        }
    }

    fn current_children_mut(&mut self) -> &mut Vec<NodeId> {
        match self.stack.first_mut() {
            Some(top) => &mut top.el.children,
            None => &mut self.root_children,
        }
    }

    fn add_node(&mut self, node: NodeId) {
        self.current_children_mut().push(node);
    }

    fn on_interpolation(&mut self, start: usize, end: usize) {
        if self.in_v_pre {
            let s = self.get_slice(start, end);
            return self.on_text(s, start, end);
        }
        let mut inner_start = start + self.delimiter_open.len();
        let mut inner_end = end - self.delimiter_close.len();
        while is_whitespace(self.char_at(inner_start)) {
            inner_start += 1;
        }
        while inner_end > 0 && is_whitespace(self.char_at(inner_end - 1)) {
            inner_end -= 1;
        }
        let mut exp = self.get_slice(inner_start, inner_end);
        if exp.contains('&') {
            let units: Vec<u16> = exp.encode_utf16().collect();
            exp = String::from_utf16_lossy(&decode_with_trie(&units, DecodingMode::Legacy));
        }
        let inner_loc = self.get_loc(inner_start, inner_end);
        let content = self.create_exp(
            exp,
            false,
            inner_loc,
            ConstantType::NotConstant,
            ExpParseMode::Normal,
        );
        let loc = self.get_loc(start, end);
        let id = self.arena.create_interpolation(content, loc);
        self.add_node(id);
    }

    fn on_open_tag_name(&mut self, start: usize, end: usize) {
        let name = self.get_slice(start, end);
        let ns = (self.options.get_namespace)(
            &self.arena,
            &name,
            self.stack.first().map(|o| &o.el),
            self.options.ns,
        );
        let loc = self.get_loc_open(start - 1);
        let id = self.next_open_id;
        self.next_open_id += 1;
        self.current_open_tag = Some(OpenElement {
            el: ElementNode {
                tag: name,
                ns,
                tag_type: ElementType::Element,
                props: Vec::new(),
                children: Vec::new(),
                is_self_closing: false,
                inner_loc: None,
                codegen_node: None,
                ssr_codegen_node: None,
                loc,
            },
            id,
        });
    }

    fn end_open_tag(&mut self, end: usize) {
        let mut open = match self.current_open_tag.take() {
            Some(o) => o,
            None => return,
        };
        if self.in_sfc_root() {
            open.el.inner_loc = Some(self.get_loc(end + 1, end + 1));
        }
        let tag = open.el.tag.clone();
        let ns = open.el.ns;
        if ns == Namespace::Html && (self.options.is_pre_tag)(&tag) {
            self.in_pre += 1;
        }
        if (self.options.is_void_tag)(&tag) {
            let sfc_root = self.in_sfc_root();
            self.on_close_tag(open, end, false, sfc_root);
        } else {
            self.stack.insert(0, open);
            if ns == Namespace::Svg || ns == Namespace::MathMl {
                self.in_xml = true;
            }
        }
    }

    fn on_self_closing_tag(&mut self, end: usize) {
        let name = match &self.current_open_tag {
            Some(o) => o.el.tag.clone(),
            None => return,
        };
        if let Some(o) = self.current_open_tag.as_mut() {
            o.el.is_self_closing = true;
        }
        self.end_open_tag(end);
        if self.stack.first().map(|o| o.el.tag.as_str()) == Some(name.as_str()) {
            let el = self.stack.remove(0);
            let sfc_root = self.in_sfc_root();
            self.on_close_tag(el, end, false, sfc_root);
        }
    }

    fn on_close_tag_cb(&mut self, start: usize, end: usize) {
        let name = self.get_slice(start, end);
        if (self.options.is_void_tag)(&name) {
            return;
        }
        let lower = name.to_lowercase();
        let mut found = false;
        let mut found_index = 0usize;
        for (i, o) in self.stack.iter().enumerate() {
            if o.el.tag.to_lowercase() == lower {
                found = true;
                found_index = i;
                break;
            }
        }
        if found {
            if found_index > 0 {
                let off = self.stack[0].el.loc.start.offset.max(0) as usize;
                self.emit_error(ErrorCode::X_MISSING_END_TAG, off);
            }
            for j in 0..=found_index {
                let el = self.stack.remove(0);
                let sfc_root = self.in_sfc_root();
                self.on_close_tag(el, end, j < found_index, sfc_root);
            }
        } else {
            let idx = self.back_track(start, cc::LT);
            self.emit_error(ErrorCode::X_INVALID_END_TAG, idx);
        }
    }

    fn on_attrib_name(&mut self, start: usize, end: usize) {
        let name = self.get_slice(start, end);
        let name_loc = self.get_loc(start, end);
        let loc = self.get_loc_open(start);
        self.current_prop = Some(Node::Attribute(Box::new(AttributeNode {
            name,
            name_loc,
            value: None,
            loc,
        })));
    }

    fn on_dir_name(&mut self, start: usize, end: usize) {
        let raw = self.get_slice(start, end);
        let name = if raw == "." || raw == ":" {
            "bind".to_string()
        } else if raw == "@" {
            "on".to_string()
        } else if raw == "#" {
            "slot".to_string()
        } else {
            raw.chars().skip(2).collect::<String>()
        };

        if !self.in_v_pre && name.is_empty() {
            self.emit_error(ErrorCode::X_MISSING_DIRECTIVE_NAME, start);
        }

        if self.in_v_pre || name.is_empty() {
            let name_loc = self.get_loc(start, end);
            let loc = self.get_loc_open(start);
            self.current_prop = Some(Node::Attribute(Box::new(AttributeNode {
                name: raw,
                name_loc,
                value: None,
                loc,
            })));
        } else {
            let loc = self.get_loc_open(start);
            let modifiers = if raw == "." {
                vec![self.arena.simple_exp("prop", false)]
            } else {
                Vec::new()
            };
            let is_pre = name == "pre";
            self.current_prop = Some(Node::Directive(Box::new(DirectiveNode {
                name,
                raw_name: Some(raw),
                exp: None,
                arg: None,
                modifiers,
                for_parse_result: None,
                loc,
            })));
            if is_pre {
                self.in_v_pre = true;
                self.in_v_pre_tok = true;
                self.current_v_pre_boundary = self.current_open_tag.as_ref().map(|o| o.id);
                let props: Vec<NodeId> = self
                    .current_open_tag
                    .as_ref()
                    .map(|o| o.el.props.clone())
                    .unwrap_or_default();
                for id in props {
                    if self.arena.is(id, NodeType::Directive) {
                        self.dir_to_attr(id);
                    }
                }
            }
        }
    }

    fn on_dir_arg(&mut self, start: usize, end: usize) {
        if start == end {
            return;
        }
        let arg = self.get_slice(start, end);
        let is_v_pre = matches!(&self.current_prop, Some(Node::Directive(d)) if d.name == "pre");
        if self.in_v_pre && !is_v_pre {
            let mut loc = match self.current_prop.as_mut() {
                Some(Node::Attribute(a)) => {
                    a.name.push_str(&arg);
                    std::mem::replace(&mut a.name_loc, loc_stub())
                }
                _ => return,
            };
            self.set_loc_end(&mut loc, end);
            if let Some(Node::Attribute(a)) = self.current_prop.as_mut() {
                a.name_loc = loc;
            }
        } else {
            let is_static = !arg.starts_with('[');
            let content = if is_static {
                arg.clone()
            } else {
                let chars: Vec<char> = arg.chars().collect();
                chars[1..chars.len().saturating_sub(1)].iter().collect()
            };
            let loc = self.get_loc(start, end);
            let const_type = if is_static {
                ConstantType::CanStringify
            } else {
                ConstantType::NotConstant
            };
            let exp = self.create_exp(content, is_static, loc, const_type, ExpParseMode::Normal);
            if let Some(Node::Directive(d)) = self.current_prop.as_mut() {
                d.arg = Some(exp);
            }
        }
    }

    fn on_dir_modifier(&mut self, start: usize, end: usize) {
        let m = self.get_slice(start, end);
        let is_v_pre = matches!(&self.current_prop, Some(Node::Directive(d)) if d.name == "pre");
        if self.in_v_pre && !is_v_pre {
            let mut loc = match self.current_prop.as_mut() {
                Some(Node::Attribute(a)) => {
                    a.name.push('.');
                    a.name.push_str(&m);
                    std::mem::replace(&mut a.name_loc, loc_stub())
                }
                _ => return,
            };
            self.set_loc_end(&mut loc, end);
            if let Some(Node::Attribute(a)) = self.current_prop.as_mut() {
                a.name_loc = loc;
            }
        } else if matches!(&self.current_prop, Some(Node::Directive(d)) if d.name == "slot") {
            let arg = match &self.current_prop {
                Some(Node::Directive(d)) => match d.arg {
                    Some(a) => a,
                    None => return,
                },
                _ => return,
            };
            if self.arena.is(arg, NodeType::SimpleExpression) {
                let e = self.arena.exp_mut(arg);
                e.content.push('.');
                e.content.push_str(&m);
                let mut loc = std::mem::replace(&mut e.loc, loc_stub());
                self.set_loc_end(&mut loc, end);
                self.arena.exp_mut(arg).loc = loc;
            }
        } else {
            let loc = self.get_loc(start, end);
            let exp = self
                .arena
                .create_simple_expression(m, true, loc, ConstantType::NotConstant);
            if let Some(Node::Directive(d)) = self.current_prop.as_mut() {
                d.modifiers.push(exp);
            }
        }
    }

    fn on_attrib_data(&mut self, start: usize, end: usize) {
        let end = end.min(self.buffer.len());
        if start < end {
            self.current_attr_value
                .extend_from_slice(&self.buffer[start..end]);
        }
        if self.current_attr_start_index < 0 {
            self.current_attr_start_index = start as i64;
        }
        self.current_attr_end_index = end as i64;
    }

    fn on_attrib_entity(&mut self, ch: &str, start: usize, end: usize) {
        self.current_attr_value.extend(ch.encode_utf16());
        if self.current_attr_start_index < 0 {
            self.current_attr_start_index = start as i64;
        }
        self.current_attr_end_index = end as i64;
    }

    fn on_attrib_name_end(&mut self, end: usize) {
        let start = match &self.current_prop {
            Some(p) => p.loc().start.offset.max(0) as usize,
            None => return,
        };
        let name = self.get_slice(start, end);
        if let Some(Node::Directive(d)) = self.current_prop.as_mut() {
            d.raw_name = Some(name.clone());
        }
        let props: Vec<NodeId> = self
            .current_open_tag
            .as_ref()
            .map(|o| o.el.props.clone())
            .unwrap_or_default();
        let dup = props.iter().any(|id| match self.arena.node(*id) {
            Node::Directive(d) => d.raw_name.as_deref() == Some(name.as_str()),
            Node::Attribute(a) => a.name == name,
            _ => false,
        });
        if dup {
            self.emit_error(ErrorCode::DUPLICATE_ATTRIBUTE, start);
        }
    }

    fn on_attrib_end(&mut self, quote: QuoteType, end: usize) {
        if self.current_open_tag.is_some() && self.current_prop.is_some() {
            let mut prop = self.current_prop.take().unwrap();
            {
                let start_off = prop.loc().start.offset.max(0) as usize;
                let pos = self.get_pos(end);
                let src = self.get_slice(start_off, end);
                let loc: &mut SourceLocation = match &mut prop {
                    Node::Attribute(a) => &mut a.loc,
                    Node::Directive(d) => &mut d.loc,
                    _ => unreachable!(),
                };
                loc.end = pos;
                loc.source = src;
            }

            if quote != QuoteType::NoValue {
                let mut attr_value = String::from_utf16_lossy(&self.current_attr_value);
                let is_attr = matches!(prop, Node::Attribute(_));
                if is_attr {
                    let name = match &prop {
                        Node::Attribute(a) => a.name.clone(),
                        _ => unreachable!(),
                    };
                    if name == "class" {
                        attr_value = condense(&attr_value).trim().to_string();
                    }
                    if quote == QuoteType::Unquoted && attr_value.is_empty() {
                        self.emit_error(ErrorCode::MISSING_ATTRIBUTE_VALUE, end);
                    }
                    let value_loc = if quote == QuoteType::Unquoted {
                        self.get_loc(
                            self.current_attr_start_index.max(0) as usize,
                            self.current_attr_end_index.max(0) as usize,
                        )
                    } else {
                        self.get_loc(
                            (self.current_attr_start_index - 1).max(0) as usize,
                            (self.current_attr_end_index + 1).max(0) as usize,
                        )
                    };
                    if let Node::Attribute(a) = &mut prop {
                        a.value = Some(TextNode {
                            content: attr_value.clone(),
                            loc: value_loc,
                        });
                    }
                    let is_root_template_lang = self.in_sfc_root()
                        && self.current_open_tag.as_ref().unwrap().el.tag == "template"
                        && name == "lang"
                        && !attr_value.is_empty()
                        && attr_value != "html";
                    if is_root_template_lang {
                        self.enter_rcdata(to_char_codes("</template"), SeqKind::Custom, 0);
                    }
                } else {
                    let dir_name = match &prop {
                        Node::Directive(d) => d.name.clone(),
                        _ => unreachable!(),
                    };
                    let mut exp_parse_mode = ExpParseMode::Normal;
                    if dir_name == "for" {
                        exp_parse_mode = ExpParseMode::Skip;
                    } else if dir_name == "slot" {
                        exp_parse_mode = ExpParseMode::Params;
                    } else if dir_name == "on" && attr_value.contains(';') {
                        exp_parse_mode = ExpParseMode::Statements;
                    }
                    let loc = self.get_loc(
                        self.current_attr_start_index.max(0) as usize,
                        self.current_attr_end_index.max(0) as usize,
                    );
                    let exp = self.create_exp(
                        attr_value.clone(),
                        false,
                        loc,
                        ConstantType::NotConstant,
                        exp_parse_mode,
                    );
                    let for_result = if dir_name == "for" {
                        self.parse_for_expression(exp)
                    } else {
                        None
                    };
                    if let Node::Directive(d) = &mut prop {
                        d.exp = Some(exp);
                        d.for_parse_result = for_result;
                    }
                }
            }

            let skip = matches!(&prop, Node::Directive(d) if d.name == "pre");
            if !skip {
                let id = self.arena.add(prop);
                if let Some(o) = self.current_open_tag.as_mut() {
                    o.el.props.push(id);
                }
            }
        }
        self.current_attr_value.clear();
        self.current_attr_start_index = -1;
        self.current_attr_end_index = -1;
    }

    fn on_comment(&mut self, start: usize, end: usize) {
        if self.options.comments {
            let content = self.get_slice(start, end);
            let loc = self.get_loc(start.saturating_sub(4), end + 3);
            let id = self
                .arena
                .add(Node::Comment(Box::new(CommentNode { content, loc })));
            self.add_node(id);
        }
    }

    fn on_cdata(&mut self, start: usize, end: usize) {
        let ns = self
            .stack
            .first()
            .map(|o| o.el.ns)
            .unwrap_or(self.options.ns);
        if ns != Namespace::Html {
            let s = self.get_slice(start, end);
            self.on_text(s, start, end);
        } else {
            self.emit_error(ErrorCode::CDATA_IN_HTML_CONTENT, start.saturating_sub(9));
        }
    }

    fn on_processing_instruction(&mut self, start: usize) {
        let ns = self
            .stack
            .first()
            .map(|o| o.el.ns)
            .unwrap_or(self.options.ns);
        if ns == Namespace::Html {
            self.emit_error(
                ErrorCode::UNEXPECTED_QUESTION_MARK_INSTEAD_OF_TAG_NAME,
                start.saturating_sub(1),
            );
        }
    }

    fn on_end(&mut self) {
        let end = self.buffer.len();
        match self.state {
            State::BeforeTagName | State::BeforeClosingTagName => {
                self.emit_error(ErrorCode::EOF_BEFORE_TAG_NAME, end)
            }
            State::Interpolation | State::InterpolationClose => {
                let s = self.section_start.max(0) as usize;
                self.emit_error(ErrorCode::X_MISSING_INTERPOLATION_END, s)
            }
            State::InCommentLike => {
                if self.current_seq_kind == SeqKind::CdataEnd {
                    self.emit_error(ErrorCode::EOF_IN_CDATA, end)
                } else {
                    self.emit_error(ErrorCode::EOF_IN_COMMENT, end)
                }
            }
            State::InTagName
            | State::InSelfClosingTag
            | State::InClosingTagName
            | State::BeforeAttrName
            | State::InAttrName
            | State::InDirName
            | State::InDirArg
            | State::InDirDynamicArg
            | State::InDirModifier
            | State::AfterAttrName
            | State::BeforeAttrValue
            | State::InAttrValueDq
            | State::InAttrValueSq
            | State::InAttrValueNq => self.emit_error(ErrorCode::EOF_IN_TAG, end),
            _ => {}
        }
        while !self.stack.is_empty() {
            let el = self.stack.remove(0);
            let off = el.el.loc.start.offset.max(0) as usize;
            // JS `onend` does not shift the stack, so `inSFCRoot` is never true here.
            self.on_close_tag(el, end.saturating_sub(1), false, false);
            self.emit_error(ErrorCode::X_MISSING_END_TAG, off);
        }
    }

    fn on_close_tag(&mut self, open: OpenElement, end: usize, is_implied: bool, sfc_root: bool) {
        let OpenElement { mut el, id } = open;

        if is_implied {
            let idx = self.back_track(end, cc::LT);
            self.set_loc_end(&mut el.loc, idx);
        } else {
            let idx = self.look_ahead(end, cc::GT) + 1;
            self.set_loc_end(&mut el.loc, idx);
        }

        if sfc_root {
            let inner_end = if let Some(last) = el.children.last() {
                self.arena.loc(*last).end
            } else {
                el.inner_loc.as_ref().unwrap().start
            };
            if let Some(inner) = el.inner_loc.as_mut() {
                inner.end = inner_end;
            }
            let (s, e) = (
                el.inner_loc.as_ref().unwrap().start.offset.max(0) as usize,
                el.inner_loc.as_ref().unwrap().end.offset.max(0) as usize,
            );
            let src = self.get_slice(s, e);
            if let Some(inner) = el.inner_loc.as_mut() {
                inner.source = src;
            }
        }

        if !self.in_v_pre {
            if el.tag == "slot" {
                el.tag_type = ElementType::Slot;
            } else if self.is_fragment_template(&el) {
                el.tag_type = ElementType::Template;
            } else if self.is_component(&el) {
                el.tag_type = ElementType::Component;
            }
        }

        if !self.in_rcdata {
            let children = std::mem::take(&mut el.children);
            el.children = self.condense_whitespace(children);
        }

        if el.ns == Namespace::Html && (self.options.is_ignore_newline_tag)(&el.tag) {
            if let Some(first) = el.children.first().copied() {
                if self.arena.is(first, NodeType::Text) {
                    let t = self.arena.text_mut(first);
                    if let Some(rest) = t.content.strip_prefix("\r\n") {
                        t.content = rest.to_string();
                    } else if let Some(rest) = t.content.strip_prefix('\n') {
                        t.content = rest.to_string();
                    }
                }
            }
        }

        if el.ns == Namespace::Html && (self.options.is_pre_tag)(&el.tag) {
            self.in_pre -= 1;
        }
        if self.current_v_pre_boundary == Some(id) {
            self.in_v_pre = false;
            self.in_v_pre_tok = false;
            self.current_v_pre_boundary = None;
        }
        let parent_ns = self
            .stack
            .first()
            .map(|o| o.el.ns)
            .unwrap_or(self.options.ns);
        if self.in_xml && parent_ns == Namespace::Html {
            self.in_xml = false;
        }

        let node_id = self.arena.add(Node::Element(Box::new(el)));
        self.add_node(node_id);
    }

    /// `dirToAttr` — converts an already-parsed directive prop into a plain
    /// attribute (used when `v-pre` is seen after other props).
    fn dir_to_attr(&mut self, id: NodeId) {
        let d = match std::mem::take(self.arena.node_mut(id)) {
            Node::Directive(d) => *d,
            other => {
                *self.arena.node_mut(id) = other;
                return;
            }
        };
        let raw_name = d.raw_name.clone().unwrap_or_default();
        let name_start = d.loc.start.offset;
        let name_loc = SourceLocation {
            start: d.loc.start,
            end: Position {
                offset: name_start + utf16_len(&raw_name) as i64,
                line: d.loc.start.line,
                column: d.loc.start.column + utf16_len(&raw_name) as i64,
            },
            source: self.get_slice(
                name_start.max(0) as usize,
                (name_start + utf16_len(&raw_name) as i64).max(0) as usize,
            ),
        };
        let mut attr = AttributeNode {
            name: raw_name,
            name_loc,
            value: None,
            loc: d.loc.clone(),
        };
        if let Some(exp) = d.exp {
            let e = self.arena.exp(exp);
            let mut loc = e.loc.clone();
            let content = e.content.clone();
            if loc.end.offset < d.loc.end.offset {
                loc.start.offset -= 1;
                loc.start.column -= 1;
                loc.end.offset += 1;
                loc.end.column += 1;
            }
            attr.value = Some(TextNode { content, loc });
        }
        *self.arena.node_mut(id) = Node::Attribute(Box::new(attr));
    }

    fn look_ahead(&self, index: usize, c: u32) -> usize {
        let mut i = index;
        while self.char_at(i) != c && i + 1 < self.buffer.len() {
            i += 1;
        }
        i
    }

    fn back_track(&self, index: usize, c: u32) -> usize {
        let mut i = index as i64;
        while i >= 0 && self.char_at(i as usize) != c {
            i -= 1;
        }
        i.max(0) as usize
    }

    fn is_fragment_template(&self, el: &ElementNode) -> bool {
        if el.tag == "template" {
            for p in &el.props {
                if let Node::Directive(d) = self.arena.node(*p) {
                    if matches!(d.name.as_str(), "if" | "else" | "else-if" | "for" | "slot") {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn is_component(&self, el: &ElementNode) -> bool {
        if let Some(f) = &self.options.is_custom_element {
            if f(&el.tag) {
                return false;
            }
        }
        let tag = &el.tag;
        if tag == "component"
            || tag
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false)
            || super::utils::is_core_component(tag).is_some()
            || self
                .options
                .is_built_in_component
                .map(|f| f(tag).is_some())
                .unwrap_or(false)
            || self.options.is_native_tag.map(|f| !f(tag)).unwrap_or(false)
        {
            return true;
        }
        for p in &el.props {
            if let Node::Attribute(a) = self.arena.node(*p) {
                if a.name == "is" {
                    if let Some(v) = &a.value {
                        if v.content.starts_with("vue:") {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn condense_whitespace(&mut self, nodes: Vec<NodeId>) -> Vec<NodeId> {
        let should_condense = self.options.whitespace != WhitespaceStrategy::Preserve;
        let mut removed = vec![false; nodes.len()];
        for i in 0..nodes.len() {
            let id = nodes[i];
            if !self.arena.is(id, NodeType::Text) {
                continue;
            }
            if self.in_pre == 0 {
                let all_ws = self
                    .arena
                    .text(id)
                    .content
                    .chars()
                    .all(|c| is_whitespace(c as u32));
                if all_ws {
                    let prev = if i > 0 {
                        Some(self.arena.node_type(nodes[i - 1]))
                    } else {
                        None
                    };
                    let next = nodes.get(i + 1).map(|n| self.arena.node_type(*n));
                    let content = &self.arena.text(id).content;
                    let has_newline = content.contains('\n') || content.contains('\r');
                    let remove = prev.is_none()
                        || next.is_none()
                        || (should_condense
                            && ((prev == Some(NodeType::Comment)
                                && (next == Some(NodeType::Comment)
                                    || next == Some(NodeType::Element)))
                                || (prev == Some(NodeType::Element)
                                    && (next == Some(NodeType::Comment)
                                        || (next == Some(NodeType::Element) && has_newline)))));
                    if remove {
                        removed[i] = true;
                    } else {
                        self.arena.text_mut(id).content = " ".to_string();
                    }
                } else if should_condense {
                    let c = condense(&self.arena.text(id).content);
                    self.arena.text_mut(id).content = c;
                }
            } else {
                let c = self.arena.text(id).content.replace("\r\n", "\n");
                self.arena.text_mut(id).content = c;
            }
        }
        if removed.iter().any(|r| *r) {
            nodes
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !removed[*i])
                .map(|(_, n)| n)
                .collect()
        } else {
            nodes
        }
    }

    fn create_exp(
        &mut self,
        content: String,
        is_static: bool,
        loc: SourceLocation,
        const_type: ConstantType,
        parse_mode: ExpParseMode,
    ) -> NodeId {
        let exp = self
            .arena
            .create_simple_expression(content.clone(), is_static, loc.clone(), const_type);
        if !is_static
            && self.options.prefix_identifiers
            && parse_mode != ExpParseMode::Skip
            && !content.trim().is_empty()
        {
            if super::utils::is_simple_identifier(&content) {
                self.arena.exp_mut(exp).ast = ExpAst::Null;
                return exp;
            }
            let ts = true;
            let parsed = match parse_mode {
                ExpParseMode::Statements => {
                    super::jsparse::parse_program(&format!(" {content} "), ts)
                        .map(|p| ExpAst::Program(Box::new(p)))
                }
                ExpParseMode::Params => {
                    super::jsparse::parse_expression(&format!("({content})=>{{}}"), ts)
                        .map(|e| ExpAst::Expr(Box::new(e)))
                }
                _ => super::jsparse::parse_expression(&format!("({content})"), ts)
                    .map(|e| ExpAst::Expr(Box::new(e))),
            };
            match parsed {
                Ok(ast) => self.arena.exp_mut(exp).ast = ast,
                Err(msg) => {
                    self.arena.exp_mut(exp).ast = ExpAst::Failed;
                    let off = loc.start.offset.max(0) as usize;
                    self.emit_error_msg(ErrorCode::X_INVALID_EXPRESSION, off, &msg);
                }
            }
        }
        exp
    }

    fn parse_for_expression(&mut self, input: NodeId) -> Option<ForParseResult> {
        let loc = self.arena.exp(input).loc.clone();
        let exp = self.arena.exp(input).content.clone();
        let (lhs, rhs) = super::utils::match_for_alias(&exp)?;

        let make = |content: String, offset: usize, as_param: bool, me: &mut Self| -> NodeId {
            let start = loc.start.offset.max(0) as usize + offset;
            let end = start + content.encode_utf16().count();
            let l = me.get_loc(start, end);
            me.create_exp(
                content,
                false,
                l,
                ConstantType::NotConstant,
                if as_param {
                    ExpParseMode::Params
                } else {
                    ExpParseMode::Normal
                },
            )
        };

        let rhs_trimmed = rhs.trim().to_string();
        let source_offset = utf16_index_of(&exp, &rhs, utf16_len(&lhs)).unwrap_or(0);
        let source = make(rhs_trimmed, source_offset, false, self);

        let mut result = ForParseResult {
            source,
            value: None,
            key: None,
            index: None,
            finalized: false,
        };

        let mut value_content = strip_parens(lhs.trim()).trim().to_string();
        let trimmed_offset = utf16_index_of(&lhs, &value_content, 0).unwrap_or(0);

        if let Some((key_raw, index_raw)) = super::utils::match_for_iterator(&value_content) {
            value_content = super::utils::strip_for_iterator(&value_content)
                .trim()
                .to_string();
            let key_content = key_raw.trim().to_string();
            let mut key_offset = 0usize;
            if !key_content.is_empty() {
                key_offset = utf16_index_of(
                    &exp,
                    &key_content,
                    trimmed_offset + utf16_len(&value_content),
                )
                .unwrap_or(0);
                result.key = Some(make(key_content.clone(), key_offset, true, self));
            }
            if let Some(index_raw) = index_raw {
                let index_content = index_raw.trim().to_string();
                if !index_content.is_empty() {
                    let from = if result.key.is_some() {
                        key_offset + utf16_len(&key_content)
                    } else {
                        trimmed_offset + utf16_len(&value_content)
                    };
                    let off = utf16_index_of(&exp, &index_content, from).unwrap_or(0);
                    result.index = Some(make(index_content, off, true, self));
                }
            }
        }

        if !value_content.is_empty() {
            result.value = Some(make(value_content, trimmed_offset, true, self));
        }

        Some(result)
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `String.prototype.indexOf` in UTF-16 index space.
fn utf16_index_of(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let h: Vec<u16> = haystack.encode_utf16().collect();
    let n: Vec<u16> = needle.encode_utf16().collect();
    if n.is_empty() {
        return Some(from.min(h.len()));
    }
    if n.len() > h.len() {
        return None;
    }
    (from.min(h.len())..=h.len().saturating_sub(n.len())).find(|&i| h[i..i + n.len()] == n[..])
}

fn strip_parens(s: &str) -> String {
    let mut out = s.to_string();
    if out.starts_with('(') {
        out.remove(0);
    }
    if out.ends_with(')') {
        out.pop();
    }
    out
}

fn condense(s: &str) -> String {
    let mut ret = String::new();
    let mut prev_ws = false;
    for ch in s.chars() {
        if is_whitespace(ch as u32) {
            if !prev_ws {
                ret.push(' ');
                prev_ws = true;
            }
        } else {
            ret.push(ch);
            prev_ws = false;
        }
    }
    ret
}

fn unused_dir_to_attr(dir: Node) -> Node {
    let d = match dir {
        Node::Directive(d) => *d,
        other => return other,
    };
    let raw_name = d.raw_name.clone().unwrap_or_default();
    let name_start = d.loc.start.offset;
    let name_loc = SourceLocation {
        start: d.loc.start,
        end: Position {
            offset: name_start + utf16_len(&raw_name) as i64,
            line: d.loc.start.line,
            column: d.loc.start.column + utf16_len(&raw_name) as i64,
        },
        source: raw_name.clone(),
    };
    let attr = AttributeNode {
        name: raw_name,
        name_loc,
        value: None,
        loc: d.loc.clone(),
    };
    // NOTE: the JS version copies `dir.exp` into the attribute value; it needs
    // arena access, so `Parser::dir_to_attr_with_exp` does that part.
    Node::Attribute(Box::new(attr))
}

pub struct ParseResult {
    pub arena: Arena,
    pub root: NodeId,
    pub errors: Vec<CompilerError>,
}

pub fn base_parse(input: &str, options: ParserOptions) -> ParseResult {
    let mut p = Parser::new(input, options);
    p.tokenize();
    let children = std::mem::take(&mut p.root_children);
    let children = p.condense_whitespace(children);
    let loc = p.get_loc(0, p.buffer.len());
    let root = p.arena.create_root(children, input.to_string());
    p.arena.root_mut(root).loc = loc;
    ParseResult {
        arena: p.arena,
        root,
        errors: std::mem::take(&mut p.errors),
    }
}
