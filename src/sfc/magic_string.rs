//! Port of the `magic-string` operations `compileScript` relies on.
//! Offsets are byte offsets into the original string.

use std::collections::HashMap;

#[derive(Debug, Clone)]
struct Chunk {
    start: usize,
    end: usize,
    /// only read once `edited`; until then the content is `original[start..end]`
    content: String,
    intro: String,
    outro: String,
    prev: Option<usize>,
    next: Option<usize>,
    edited: bool,
}

#[derive(Debug)]
pub struct MagicString {
    original: String,
    chunks: Vec<Chunk>,
    by_start: HashMap<usize, usize>,
    by_end: HashMap<usize, usize>,
    first: usize,
    intro: String,
    outro: String,
}

impl MagicString {
    pub fn new(original: &str) -> Self {
        let chunk = Chunk {
            start: 0,
            end: original.len(),
            content: String::new(),
            intro: String::new(),
            outro: String::new(),
            prev: None,
            next: None,
            edited: false,
        };
        let mut by_start = HashMap::new();
        let mut by_end = HashMap::new();
        by_start.insert(0, 0usize);
        by_end.insert(original.len(), 0usize);
        MagicString {
            original: original.to_string(),
            chunks: vec![chunk],
            by_start,
            by_end,
            first: 0,
            intro: String::new(),
            outro: String::new(),
        }
    }

    pub fn original(&self) -> &str {
        &self.original
    }

    pub fn slice_original(&self, start: usize, end: usize) -> String {
        self.original[start.min(self.original.len())..end.min(self.original.len())].to_string()
    }

    fn split(&mut self, index: usize) {
        if self.by_start.contains_key(&index) || self.by_end.contains_key(&index) {
            return;
        }
        // find the chunk containing `index`
        let mut cur = Some(self.first);
        while let Some(i) = cur {
            let c = &self.chunks[i];
            if index > c.start && index < c.end {
                self.split_chunk(i, index);
                return;
            }
            cur = self.chunks[i].next;
        }
    }

    fn split_chunk(&mut self, index: usize, split_at: usize) {
        let (end, outro, next, edited) = {
            let c = &mut self.chunks[index];
            (c.end, std::mem::take(&mut c.outro), c.next, c.edited)
        };
        // magic-string keeps edited content on the first half; an unedited
        // half reads its slice of `original`
        let new_chunk = Chunk {
            start: split_at,
            end,
            content: String::new(),
            intro: String::new(),
            outro,
            prev: Some(index),
            next,
            edited,
        };
        let new_index = self.chunks.len();
        self.chunks.push(new_chunk);
        {
            let c = &mut self.chunks[index];
            c.end = split_at;
            c.next = Some(new_index);
        }
        if let Some(n) = next {
            self.chunks[n].prev = Some(new_index);
        }
        self.by_end.insert(split_at, index);
        self.by_start.insert(split_at, new_index);
        self.by_end.insert(end, new_index);
    }

    pub fn append(&mut self, s: &str) {
        self.outro.push_str(s);
    }

    pub fn prepend(&mut self, s: &str) {
        self.intro = format!("{s}{}", self.intro);
    }

    pub fn append_left(&mut self, index: usize, s: &str) {
        self.split(index);
        match self.by_end.get(&index) {
            Some(&c) => self.chunks[c].outro.push_str(s),
            None => self.intro.push_str(s),
        }
    }

    pub fn append_right(&mut self, index: usize, s: &str) {
        self.split(index);
        match self.by_start.get(&index) {
            Some(&c) => {
                let chunk = &mut self.chunks[c];
                chunk.intro = format!("{}{s}", chunk.intro);
            }
            None => self.outro.push_str(s),
        }
    }

    pub fn prepend_left(&mut self, index: usize, s: &str) {
        self.split(index);
        match self.by_end.get(&index) {
            Some(&c) => {
                let chunk = &mut self.chunks[c];
                chunk.outro = format!("{s}{}", chunk.outro);
            }
            None => self.intro = format!("{s}{}", self.intro),
        }
    }

    pub fn prepend_right(&mut self, index: usize, s: &str) {
        self.split(index);
        match self.by_start.get(&index) {
            Some(&c) => self.chunks[c].intro.push_str(s),
            None => self.outro = format!("{s}{}", self.outro),
        }
    }

    pub fn overwrite(&mut self, start: usize, end: usize, content: &str) {
        self.edit_range(start, end, content, false);
    }

    pub fn remove(&mut self, start: usize, end: usize) {
        self.edit_range(start, end, "", true);
    }

    fn edit_range(&mut self, start: usize, end: usize, content: &str, is_remove: bool) {
        if start == end {
            if !is_remove {
                self.append_left(start, content);
            }
            return;
        }
        self.split(start);
        self.split(end);
        let first = match self.by_start.get(&start) {
            Some(&i) => i,
            None => return,
        };
        let mut cur = Some(first);
        let mut is_first = true;
        while let Some(i) = cur {
            if self.chunks[i].start >= end {
                break;
            }
            let next = self.chunks[i].next;
            {
                let c = &mut self.chunks[i];
                if is_first {
                    c.content = content.to_string();
                    c.edited = true;
                    if is_remove {
                        c.intro = String::new();
                        c.outro = String::new();
                    }
                } else {
                    c.content = String::new();
                    c.edited = true;
                    c.intro = String::new();
                    c.outro = String::new();
                }
            }
            is_first = false;
            cur = next;
        }
    }

    /// `magic-string`'s `move(start, end, index)`
    pub fn move_range(&mut self, start: usize, end: usize, index: usize) {
        self.split(start);
        self.split(end);
        self.split(index);

        let first = match self.by_start.get(&start).copied() {
            Some(i) => i,
            None => return,
        };
        let last = match self.by_end.get(&end).copied() {
            Some(i) => i,
            None => return,
        };
        let old_left = self.chunks[first].prev;
        let old_right = self.chunks[last].next;

        let new_right = self.by_start.get(&index).copied();
        let new_left = match new_right {
            Some(r) => self.chunks[r].prev,
            None => Some(self.last_chunk()),
        };

        if let Some(l) = old_left {
            self.chunks[l].next = old_right;
        }
        if let Some(r) = old_right {
            self.chunks[r].prev = old_left;
        }
        if let Some(l) = new_left {
            self.chunks[l].next = Some(first);
        }
        if let Some(r) = new_right {
            self.chunks[r].prev = Some(last);
        }
        if self.chunks[first].prev.is_none() || old_left.is_none() && new_left.is_some() {
            // the moved range was at the head
        }
        self.chunks[first].prev = new_left;
        self.chunks[last].next = new_right;

        if new_left.is_none() {
            self.first = first;
        } else if old_left.is_none() {
            self.first = old_right.unwrap_or(first);
        }
    }

    fn last_chunk(&self) -> usize {
        let mut cur = self.first;
        while let Some(n) = self.chunks[cur].next {
            cur = n;
        }
        cur
    }

    pub fn to_string(&self) -> String {
        let mut out = self.intro.clone();
        let mut cur = Some(self.first);
        while let Some(i) = cur {
            let c = &self.chunks[i];
            out.push_str(&c.intro);
            out.push_str(if c.edited { &c.content } else { &self.original[c.start..c.end] });
            out.push_str(&c.outro);
            cur = c.next;
        }
        out.push_str(&self.outro);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_apply_in_order() {
        let mut s = MagicString::new("hello world");
        s.overwrite(0, 5, "goodbye");
        s.append_left(5, "!");
        s.append_right(6, "[");
        assert_eq!(s.to_string(), "goodbye! [world");
        s.prepend(">");
        s.append("<");
        assert_eq!(s.to_string(), ">goodbye! [world<");
    }

    #[test]
    fn remove_clears_range() {
        let mut s = MagicString::new("abcdef");
        s.remove(1, 4);
        assert_eq!(s.to_string(), "aef");
    }

    #[test]
    fn move_relocates_a_range() {
        let mut s = MagicString::new("abcdefghijkl");
        s.move_range(0, 3, 6);
        assert_eq!(s.to_string(), "defabcghijkl");
    }
}
