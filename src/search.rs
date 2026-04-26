use std::ops::Range;

use crate::buffer::Buffer;

#[derive(Debug, Default)]
pub struct SearchState {
    pub active: bool,
    pub query: String,
    matches: Vec<Range<usize>>,
    current: usize,
}

impl SearchState {
    pub fn open(&mut self, buffer: &Buffer) {
        self.active = true;
        self.recompute(buffer);
    }

    pub fn close(&mut self) {
        self.active = false;
    }

    pub fn push(&mut self, ch: char, buffer: &Buffer) {
        self.query.push(ch);
        self.recompute(buffer);
    }

    pub fn pop(&mut self, buffer: &Buffer) {
        self.query.pop();
        self.recompute(buffer);
    }

    pub fn recompute(&mut self, buffer: &Buffer) {
        self.matches.clear();
        self.current = 0;
        if self.query.is_empty() {
            return;
        }
        let text = buffer.text();
        let needle = self.query.to_lowercase();
        let haystack = text.to_lowercase();
        let mut from = 0;
        while let Some(idx) = haystack[from..].find(&needle) {
            let start = from + idx;
            let end = start + self.query.len();
            self.matches.push(start..end);
            from = end.max(start + 1);
        }
        self.current = self
            .matches
            .iter()
            .position(|range| range.start >= buffer.cursor())
            .unwrap_or(0);
    }

    pub fn next(&mut self, buffer: &mut Buffer) {
        if self.matches.is_empty() {
            return;
        }
        self.current = (self.current + 1) % self.matches.len();
        let range = self.matches[self.current].clone();
        buffer.set_cursor(range.start, false);
    }

    pub fn previous(&mut self, buffer: &mut Buffer) {
        if self.matches.is_empty() {
            return;
        }
        self.current = if self.current == 0 {
            self.matches.len() - 1
        } else {
            self.current - 1
        };
        let range = self.matches[self.current].clone();
        buffer.set_cursor(range.start, false);
    }

    pub fn jump_to_current(&mut self, buffer: &mut Buffer) {
        if let Some(range) = self.matches.get(self.current).cloned() {
            buffer.set_cursor(range.start, false);
        }
    }

    pub fn visible_matches(&self) -> &[Range<usize>] {
        &self.matches
    }

    pub fn current_match(&self) -> Option<&Range<usize>> {
        self.matches.get(self.current)
    }

    pub fn count(&self) -> usize {
        self.matches.len()
    }

    pub fn current_index(&self) -> usize {
        if self.matches.is_empty() {
            0
        } else {
            self.current + 1
        }
    }
}
