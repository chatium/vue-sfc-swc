//! Port of `postcss@8.5.28/lib/tokenize.js`. Offsets are UTF-16 code units,
//! matching the JS original.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: String,
    pub value: String,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl Token {
    pub fn pos(&self) -> Option<usize> {
        self.end.or(self.start)
    }
}

const SINGLE_QUOTE: u16 = b'\'' as u16;
const DOUBLE_QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const NEWLINE: u16 = b'\n' as u16;
const SPACE: u16 = b' ' as u16;
const FEED: u16 = 0x0c;
const TAB: u16 = b'\t' as u16;
const CR: u16 = b'\r' as u16;
const OPEN_SQUARE: u16 = b'[' as u16;
const CLOSE_SQUARE: u16 = b']' as u16;
const OPEN_PARENTHESES: u16 = b'(' as u16;
const CLOSE_PARENTHESES: u16 = b')' as u16;
const OPEN_CURLY: u16 = b'{' as u16;
const CLOSE_CURLY: u16 = b'}' as u16;
const SEMICOLON: u16 = b';' as u16;
const ASTERISK: u16 = b'*' as u16;
const COLON: u16 = b':' as u16;
const AT: u16 = b'@' as u16;

/// `/[\t\n\f\r "#'()/;[\\\]{}]/`
fn is_at_end(c: u16) -> bool {
    matches!(
        c,
        TAB | NEWLINE
            | FEED
            | CR
            | SPACE
            | DOUBLE_QUOTE
            | 0x23
            | SINGLE_QUOTE
            | OPEN_PARENTHESES
            | CLOSE_PARENTHESES
            | SLASH
            | SEMICOLON
            | OPEN_SQUARE
            | BACKSLASH
            | CLOSE_SQUARE
            | OPEN_CURLY
            | CLOSE_CURLY
    )
}

/// `/[\t\n\f\r !"#'():;@[\\\]{}]|\/(?=\*)/`
fn is_word_end(css: &[u16], i: usize) -> bool {
    let c = css[i];
    if matches!(
        c,
        TAB | NEWLINE
            | FEED
            | CR
            | SPACE
            | 0x21
            | DOUBLE_QUOTE
            | 0x23
            | SINGLE_QUOTE
            | OPEN_PARENTHESES
            | CLOSE_PARENTHESES
            | COLON
            | SEMICOLON
            | AT
            | OPEN_SQUARE
            | BACKSLASH
            | CLOSE_SQUARE
            | OPEN_CURLY
            | CLOSE_CURLY
    ) {
        return true;
    }
    c == SLASH && css.get(i + 1) == Some(&ASTERISK)
}

fn is_hex(c: u16) -> bool {
    let c = c as u32;
    (0x30..=0x39).contains(&c)
        || (0x41..=0x46).contains(&c)
        || (0x61..=0x66).contains(&c)
}

/// `/.[\r\n"'(/\\]/`
fn has_bad_bracket(s: &[u16]) -> bool {
    for i in 1..s.len() {
        if matches!(
            s[i],
            CR | NEWLINE | DOUBLE_QUOTE | SINGLE_QUOTE | OPEN_PARENTHESES | SLASH | BACKSLASH
        ) {
            return true;
        }
    }
    false
}

pub struct Tokenizer {
    css: Vec<u16>,
    pos: usize,
    buffer: Vec<Token>,
    returned: Vec<Token>,
    last_bad_paren: i64,
    pub error: Option<super::parse::CssSyntaxError>,
}

impl Tokenizer {
    pub fn new(css: &str) -> Self {
        Tokenizer {
            css: css.encode_utf16().collect(),
            pos: 0,
            buffer: Vec::new(),
            returned: Vec::new(),
            last_bad_paren: -1,
            error: None,
        }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn end_of_file(&self) -> bool {
        self.returned.is_empty() && self.pos >= self.css.len()
    }

    pub fn back(&mut self, token: Token) {
        self.returned.push(token);
    }

    fn slice(&self, from: usize, to: usize) -> String {
        let to = to.min(self.css.len());
        if from >= to {
            return String::new();
        }
        String::from_utf16_lossy(&self.css[from..to])
    }

    fn index_of(&self, needle: u16, from: usize) -> i64 {
        if from >= self.css.len() {
            return -1;
        }
        self.css[from..]
            .iter()
            .position(|c| *c == needle)
            .map(|i| (i + from) as i64)
            .unwrap_or(-1)
    }

    fn index_of_comment_end(&self, from: usize) -> i64 {
        let mut i = from;
        while i + 1 < self.css.len() {
            if self.css[i] == ASTERISK && self.css[i + 1] == SLASH {
                return i as i64;
            }
            i += 1;
        }
        -1
    }

    pub fn next_token(&mut self) -> Option<Token> {
        if let Some(t) = self.returned.pop() {
            return Some(t);
        }
        if self.pos >= self.css.len() {
            return None;
        }
        let length = self.css.len();
        let mut code = self.css[self.pos];
        let current_token;

        match code {
            NEWLINE | SPACE | TAB | CR | FEED => {
                let mut next = self.pos;
                loop {
                    next += 1;
                    code = *self.css.get(next).unwrap_or(&0);
                    if !matches!(code, SPACE | NEWLINE | TAB | CR | FEED) {
                        break;
                    }
                }
                current_token = Token {
                    kind: "space".into(),
                    value: self.slice(self.pos, next),
                    start: None,
                    end: None,
                };
                self.pos = next - 1;
            }
            OPEN_SQUARE | CLOSE_SQUARE | OPEN_CURLY | CLOSE_CURLY | COLON | SEMICOLON
            | CLOSE_PARENTHESES => {
                let ch = String::from_utf16_lossy(&[code]);
                current_token = Token {
                    kind: ch.clone(),
                    value: ch,
                    start: Some(self.pos),
                    end: None,
                };
            }
            OPEN_PARENTHESES => {
                let prev = self.buffer.pop().map(|t| t.value).unwrap_or_default();
                let n = *self.css.get(self.pos + 1).unwrap_or(&0);
                if prev == "url"
                    && n != SINGLE_QUOTE
                    && n != DOUBLE_QUOTE
                    && n != SPACE
                    && n != NEWLINE
                    && n != TAB
                    && n != FEED
                    && n != CR
                {
                    let mut next = self.pos as i64;
                    loop {
                        let mut escaped = false;
                        next = self.index_of(CLOSE_PARENTHESES, (next + 1) as usize);
                        if next == -1 {
                            self.error = Some(super::parse::CssSyntaxError::new("Unclosed bracket", self.pos));
                            next = self.pos as i64;
                            break;
                        }
                        let mut escape_pos = next;
                        while escape_pos > 0
                            && self.css.get((escape_pos - 1) as usize) == Some(&BACKSLASH)
                        {
                            escape_pos -= 1;
                            escaped = !escaped;
                        }
                        if !escaped {
                            break;
                        }
                    }
                    current_token = Token {
                        kind: "brackets".into(),
                        value: self.slice(self.pos, (next + 1) as usize),
                        start: Some(self.pos),
                        end: Some(next as usize),
                    };
                    self.pos = next as usize;
                } else if (self.pos as i64) <= self.last_bad_paren {
                    current_token = Token {
                        kind: "(".into(),
                        value: "(".into(),
                        start: Some(self.pos),
                        end: None,
                    };
                } else {
                    let next = self.index_of(CLOSE_PARENTHESES, self.pos + 1);
                    let content = self.slice(self.pos, (next + 1).max(0) as usize);
                    let content_u: Vec<u16> = content.encode_utf16().collect();
                    if next == -1 || has_bad_bracket(&content_u) {
                        self.last_bad_paren = if next == -1 { length as i64 } else { next };
                        current_token = Token {
                            kind: "(".into(),
                            value: "(".into(),
                            start: Some(self.pos),
                            end: None,
                        };
                    } else {
                        current_token = Token {
                            kind: "brackets".into(),
                            value: content,
                            start: Some(self.pos),
                            end: Some(next as usize),
                        };
                        self.pos = next as usize;
                    }
                }
            }
            SINGLE_QUOTE | DOUBLE_QUOTE => {
                let quote = code;
                let mut next = self.pos as i64;
                loop {
                    let mut escaped = false;
                    next = self.index_of(quote, (next + 1) as usize);
                    if next == -1 {
                        self.error = Some(super::parse::CssSyntaxError::new("Unclosed string", self.pos));
                        next = self.pos as i64 + 1;
                        break;
                    }
                    let mut escape_pos = next;
                    while escape_pos > 0
                        && self.css.get((escape_pos - 1) as usize) == Some(&BACKSLASH)
                    {
                        escape_pos -= 1;
                        escaped = !escaped;
                    }
                    if !escaped {
                        break;
                    }
                }
                current_token = Token {
                    kind: "string".into(),
                    value: self.slice(self.pos, (next + 1) as usize),
                    start: Some(self.pos),
                    end: Some(next as usize),
                };
                self.pos = next as usize;
            }
            AT => {
                let mut next = self.pos + 1;
                while next < length && !is_at_end(self.css[next]) {
                    next += 1;
                }
                let next = if next >= length { length - 1 } else { next - 1 };
                current_token = Token {
                    kind: "at-word".into(),
                    value: self.slice(self.pos, next + 1),
                    start: Some(self.pos),
                    end: Some(next),
                };
                self.pos = next;
            }
            BACKSLASH => {
                let mut next = self.pos;
                let mut escape = true;
                while self.css.get(next + 1) == Some(&BACKSLASH) {
                    next += 1;
                    escape = !escape;
                }
                code = *self.css.get(next + 1).unwrap_or(&0);
                if escape
                    && code != SLASH
                    && code != SPACE
                    && code != NEWLINE
                    && code != TAB
                    && code != CR
                    && code != FEED
                {
                    next += 1;
                    if self.css.get(next).map(|c| is_hex(*c)).unwrap_or(false) {
                        while self.css.get(next + 1).map(|c| is_hex(*c)).unwrap_or(false) {
                            next += 1;
                        }
                        if self.css.get(next + 1) == Some(&SPACE) {
                            next += 1;
                        }
                    }
                }
                current_token = Token {
                    kind: "word".into(),
                    value: self.slice(self.pos, next + 1),
                    start: Some(self.pos),
                    end: Some(next),
                };
                self.pos = next;
            }
            _ => {
                if code == SLASH && self.css.get(self.pos + 1) == Some(&ASTERISK) {
                    let mut next = self.index_of_comment_end(self.pos + 2) + 1;
                    if next == 0 {
                        self.error = Some(super::parse::CssSyntaxError::new("Unclosed comment", self.pos));
                        next = length as i64;
                    }
                    current_token = Token {
                        kind: "comment".into(),
                        value: self.slice(self.pos, (next + 1) as usize),
                        start: Some(self.pos),
                        end: Some(next as usize),
                    };
                    self.pos = next as usize;
                } else {
                    let mut next = self.pos + 1;
                    while next < length && !is_word_end(&self.css, next) {
                        next += 1;
                    }
                    let next = if next >= length { length - 1 } else { next - 1 };
                    let t = Token {
                        kind: "word".into(),
                        value: self.slice(self.pos, next + 1),
                        start: Some(self.pos),
                        end: Some(next),
                    };
                    self.buffer.push(t.clone());
                    current_token = t;
                    self.pos = next;
                }
            }
        }

        self.pos += 1;
        Some(current_token)
    }
}
