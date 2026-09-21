//! Port of `entities@7.0.1` decode.ts — the exact decoder `@vue/compiler-core`
//! uses. The binary trie is the package's own generated table, embedded
//! verbatim as base64.

use base64::Engine;
use std::sync::LazyLock;

const HTML_DECODE_TREE_B64: &str = include_str!("entities_data.txt");

pub static HTML_DECODE_TREE: LazyLock<Vec<u16>> = LazyLock::new(|| {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(HTML_DECODE_TREE_B64.trim())
        .expect("valid base64 entity trie");
    let even = bytes.len() & !1;
    bytes[..even]
        .chunks_exact(2)
        .map(|c| c[0] as u16 | ((c[1] as u16) << 8))
        .collect()
});

const VALUE_LENGTH: u16 = 49152;
const FLAG13: u16 = 8192;
const BRANCH_LENGTH: u16 = 8064;
const JUMP_TABLE: u16 = 127;

const NUM: u32 = 35;
const SEMI: u32 = 59;
const EQUALS: u32 = 61;
const ZERO: u32 = 48;
const NINE: u32 = 57;
const LOWER_A: u32 = 97;
const LOWER_F: u32 = 102;
const LOWER_X: u32 = 120;
const LOWER_Z: u32 = 122;
const UPPER_A: u32 = 65;
const UPPER_F: u32 = 70;
const UPPER_Z: u32 = 90;
const TO_LOWER_BIT: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodingMode {
    Legacy = 0,
    Strict = 1,
    Attribute = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    EntityStart,
    NumericStart,
    NumericDecimal,
    NumericHex,
    NamedEntity,
}

fn is_number(c: u32) -> bool {
    (ZERO..=NINE).contains(&c)
}
fn is_hex(c: u32) -> bool {
    (UPPER_A..=UPPER_F).contains(&c) || (LOWER_A..=LOWER_F).contains(&c)
}
fn is_ascii_alnum(c: u32) -> bool {
    (UPPER_A..=UPPER_Z).contains(&c) || (LOWER_A..=LOWER_Z).contains(&c) || is_number(c)
}
fn is_entity_in_attribute_invalid_end(c: u32) -> bool {
    c == EQUALS || is_ascii_alnum(c)
}

pub fn replace_code_point(cp: u32) -> u32 {
    if (0xd800..=0xdfff).contains(&cp) || cp > 0x10ffff {
        return 0xfffd;
    }
    match cp {
        0 => 65533,
        128 => 8364,
        130 => 8218,
        131 => 402,
        132 => 8222,
        133 => 8230,
        134 => 8224,
        135 => 8225,
        136 => 710,
        137 => 8240,
        138 => 352,
        139 => 8249,
        140 => 338,
        142 => 381,
        145 => 8216,
        146 => 8217,
        147 => 8220,
        148 => 8221,
        149 => 8226,
        150 => 8211,
        151 => 8212,
        152 => 732,
        153 => 8482,
        154 => 353,
        155 => 8250,
        156 => 339,
        158 => 382,
        159 => 376,
        other => other,
    }
}

/// `String.fromCodePoint` — returns the UTF-16 units, matching JS exactly.
pub fn from_code_point(cp: u32) -> Vec<u16> {
    let mut out = Vec::new();
    if cp > 0xffff {
        let c = cp - 0x10000;
        out.push((((c >> 10) & 1023) | 0xd800) as u16);
        out.push((0xdc00 | (c & 1023)) as u16);
    } else {
        out.push(cp as u16);
    }
    out
}

fn determine_branch(tree: &[u16], current: u16, node_index: usize, char_code: u32) -> i64 {
    let branch_count = ((current & BRANCH_LENGTH) >> 7) as usize;
    let jump_offset = (current & JUMP_TABLE) as u32;

    if branch_count == 0 {
        return if jump_offset != 0 && char_code == jump_offset {
            node_index as i64
        } else {
            -1
        };
    }

    if jump_offset != 0 {
        if char_code < jump_offset {
            return -1;
        }
        let value = (char_code - jump_offset) as usize;
        return if value >= branch_count {
            -1
        } else {
            tree[node_index + value] as i64 - 1
        };
    }

    let packed_key_slots = (branch_count + 1) >> 1;
    let mut lo: i64 = 0;
    let mut hi: i64 = branch_count as i64 - 1;
    while lo <= hi {
        let mid = ((lo + hi) as usize) >> 1;
        let slot = mid >> 1;
        let packed = tree[node_index + slot];
        let mid_key = ((packed >> ((mid & 1) * 8)) & 0xff) as u32;
        match mid_key.cmp(&char_code) {
            std::cmp::Ordering::Less => lo = mid as i64 + 1,
            std::cmp::Ordering::Greater => hi = mid as i64 - 1,
            std::cmp::Ordering::Equal => return tree[node_index + packed_key_slots + mid] as i64,
        }
    }
    -1
}

/// Streaming decoder. `emit` receives `(code_point, consumed)` pairs, exactly
/// like the JS `emitCodePoint` callback.
pub struct EntityDecoder {
    state: State,
    consumed: usize,
    result: usize,
    tree_index: usize,
    excess: usize,
    decode_mode: DecodingMode,
    run_consumed: usize,
    pub emitted: Vec<(u32, usize)>,
}

impl Default for EntityDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityDecoder {
    pub fn new() -> Self {
        EntityDecoder {
            state: State::EntityStart,
            consumed: 1,
            result: 0,
            tree_index: 0,
            excess: 1,
            decode_mode: DecodingMode::Strict,
            run_consumed: 0,
            emitted: Vec::new(),
        }
    }

    pub fn start_entity(&mut self, mode: DecodingMode) {
        self.decode_mode = mode;
        self.state = State::EntityStart;
        self.result = 0;
        self.tree_index = 0;
        self.excess = 1;
        self.consumed = 1;
        self.run_consumed = 0;
        self.emitted.clear();
    }

    /// `input` is UTF-16 code units (JS string semantics).
    pub fn write(&mut self, input: &[u16], offset: usize) -> i64 {
        match self.state {
            State::EntityStart => {
                if input.get(offset).map(|c| *c as u32) == Some(NUM) {
                    self.state = State::NumericStart;
                    self.consumed += 1;
                    self.state_numeric_start(input, offset + 1)
                } else {
                    self.state = State::NamedEntity;
                    self.state_named_entity(input, offset)
                }
            }
            State::NumericStart => self.state_numeric_start(input, offset),
            State::NumericDecimal => self.state_numeric_decimal(input, offset),
            State::NumericHex => self.state_numeric_hex(input, offset),
            State::NamedEntity => self.state_named_entity(input, offset),
        }
    }

    fn state_numeric_start(&mut self, input: &[u16], offset: usize) -> i64 {
        if offset >= input.len() {
            return -1;
        }
        if (input[offset] as u32 | TO_LOWER_BIT) == LOWER_X {
            self.state = State::NumericHex;
            self.consumed += 1;
            return self.state_numeric_hex(input, offset + 1);
        }
        self.state = State::NumericDecimal;
        self.state_numeric_decimal(input, offset)
    }

    fn state_numeric_hex(&mut self, input: &[u16], mut offset: usize) -> i64 {
        while offset < input.len() {
            let c = input[offset] as u32;
            if is_number(c) || is_hex(c) {
                let digit = if c <= NINE {
                    c - ZERO
                } else {
                    (c | TO_LOWER_BIT) - LOWER_A + 10
                };
                self.result = self.result.saturating_mul(16).saturating_add(digit as usize);
                self.consumed += 1;
                offset += 1;
            } else {
                return self.emit_numeric_entity(c, 3);
            }
        }
        -1
    }

    fn state_numeric_decimal(&mut self, input: &[u16], mut offset: usize) -> i64 {
        while offset < input.len() {
            let c = input[offset] as u32;
            if is_number(c) {
                self.result = self.result.saturating_mul(10).saturating_add((c - ZERO) as usize);
                self.consumed += 1;
                offset += 1;
            } else {
                return self.emit_numeric_entity(c, 2);
            }
        }
        -1
    }

    fn emit_numeric_entity(&mut self, last_cp: u32, expected_length: usize) -> i64 {
        if self.consumed <= expected_length {
            return 0;
        }
        if last_cp == SEMI {
            self.consumed += 1;
        } else if self.decode_mode == DecodingMode::Strict {
            return 0;
        }
        let cp = replace_code_point(self.result.min(u32::MAX as usize) as u32);
        let consumed = self.consumed;
        self.emitted.push((cp, consumed));
        self.consumed as i64
    }

    fn state_named_entity(&mut self, input: &[u16], mut offset: usize) -> i64 {
        let tree: &[u16] = &HTML_DECODE_TREE;
        let mut current = tree[self.tree_index];
        let mut value_length = ((current & VALUE_LENGTH) >> 14) as usize;

        while offset < input.len() {
            if value_length == 0 && (current & FLAG13) != 0 {
                let run_length = ((current & BRANCH_LENGTH) >> 7) as usize;
                if self.run_consumed == 0 {
                    let first_char = (current & JUMP_TABLE) as u32;
                    if input[offset] as u32 != first_char {
                        return if self.result == 0 {
                            0
                        } else {
                            self.emit_not_terminated_named_entity()
                        };
                    }
                    offset += 1;
                    self.excess += 1;
                    self.run_consumed += 1;
                }
                while self.run_consumed < run_length {
                    if offset >= input.len() {
                        return -1;
                    }
                    let char_index_in_packed = self.run_consumed - 1;
                    let packed_word = tree[self.tree_index + 1 + (char_index_in_packed >> 1)];
                    let expected_char = if char_index_in_packed % 2 == 0 {
                        packed_word & 0xff
                    } else {
                        (packed_word >> 8) & 0xff
                    };
                    if input[offset] != expected_char {
                        self.run_consumed = 0;
                        return if self.result == 0 {
                            0
                        } else {
                            self.emit_not_terminated_named_entity()
                        };
                    }
                    offset += 1;
                    self.excess += 1;
                    self.run_consumed += 1;
                }
                self.run_consumed = 0;
                self.tree_index += 1 + (run_length >> 1);
                current = tree[self.tree_index];
                value_length = ((current & VALUE_LENGTH) >> 14) as usize;
            }

            if offset >= input.len() {
                break;
            }
            let char_code = input[offset] as u32;

            if char_code == SEMI && value_length != 0 && (current & FLAG13) != 0 {
                let idx = self.tree_index;
                let consumed = self.consumed + self.excess;
                return self.emit_named_entity_data(idx, value_length, consumed);
            }

            let next = determine_branch(
                tree,
                current,
                self.tree_index + value_length.max(1),
                char_code,
            );
            if next < 0 {
                return if self.result == 0
                    || (self.decode_mode == DecodingMode::Attribute
                        && (value_length == 0 || is_entity_in_attribute_invalid_end(char_code)))
                {
                    0
                } else {
                    self.emit_not_terminated_named_entity()
                };
            }
            self.tree_index = next as usize;
            current = tree[self.tree_index];
            value_length = ((current & VALUE_LENGTH) >> 14) as usize;

            if value_length != 0 {
                if char_code == SEMI {
                    let idx = self.tree_index;
                    let consumed = self.consumed + self.excess;
                    return self.emit_named_entity_data(idx, value_length, consumed);
                }
                if self.decode_mode != DecodingMode::Strict && (current & FLAG13) == 0 {
                    self.result = self.tree_index;
                    self.consumed += self.excess;
                    self.excess = 0;
                }
            }

            offset += 1;
            self.excess += 1;
        }
        -1
    }

    fn emit_not_terminated_named_entity(&mut self) -> i64 {
        let result = self.result;
        let value_length = ((HTML_DECODE_TREE[result] & VALUE_LENGTH) >> 14) as usize;
        let consumed = self.consumed;
        self.emit_named_entity_data(result, value_length, consumed);
        self.consumed as i64
    }

    fn emit_named_entity_data(
        &mut self,
        result: usize,
        value_length: usize,
        consumed: usize,
    ) -> i64 {
        let tree: &[u16] = &HTML_DECODE_TREE;
        let cp = if value_length == 1 {
            (tree[result] & !(VALUE_LENGTH | FLAG13)) as u32
        } else {
            tree[result + 1] as u32
        };
        self.emitted.push((cp, consumed));
        if value_length == 3 {
            self.emitted.push((tree[result + 2] as u32, consumed));
        }
        consumed as i64
    }

    pub fn end(&mut self) -> i64 {
        match self.state {
            State::NamedEntity => {
                if self.result != 0
                    && (self.decode_mode != DecodingMode::Attribute
                        || self.result == self.tree_index)
                {
                    self.emit_not_terminated_named_entity()
                } else {
                    0
                }
            }
            State::NumericDecimal => self.emit_numeric_entity(0, 2),
            State::NumericHex => self.emit_numeric_entity(0, 3),
            State::NumericStart => 0,
            State::EntityStart => 0,
        }
    }
}

/// Port of `getDecoder(htmlDecodeTree)` — the whole-string decode used by
/// `decodeHTML`.
pub fn decode_with_trie(input: &[u16], mode: DecodingMode) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    let mut decoder = EntityDecoder::new();
    let mut last_index = 0usize;
    let mut offset = 0usize;
    while let Some(found) = index_of_amp(input, offset) {
        out.extend_from_slice(&input[last_index..found]);
        decoder.start_entity(mode);
        let length = decoder.write(input, found + 1);
        if length < 0 {
            let consumed = decoder.end();
            for (cp, _) in decoder.emitted.drain(..) {
                out.extend(from_code_point(cp));
            }
            last_index = found + consumed as usize;
            break;
        }
        for (cp, _) in decoder.emitted.drain(..) {
            out.extend(from_code_point(cp));
        }
        last_index = found + length as usize;
        offset = if length == 0 {
            last_index + 1
        } else {
            last_index
        };
    }
    out.extend_from_slice(&input[last_index.min(input.len())..]);
    out
}

fn index_of_amp(input: &[u16], from: usize) -> Option<usize> {
    if from >= input.len() {
        return None;
    }
    input[from..].iter().position(|c| *c == 38).map(|i| i + from)
}

pub fn decode_html(s: &str) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(&decode_with_trie(&units, DecodingMode::Legacy))
}
