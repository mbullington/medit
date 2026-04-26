use std::{
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
};

const INITIAL_GAP: usize = 4096;

#[derive(Clone, Debug)]
pub struct AppliedEdit {
    pub pos: usize,
    pub deleted: String,
    pub inserted: String,
}

#[derive(Debug)]
pub struct Buffer {
    bytes: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
    line_starts: Vec<usize>,
    selection_anchor: Option<usize>,
    undo: Vec<AppliedEdit>,
    redo: Vec<AppliedEdit>,
    path: Option<PathBuf>,
    modified: bool,
    version: i32,
}

impl Buffer {
    pub fn empty(path: Option<PathBuf>) -> Self {
        let bytes = vec![0; INITIAL_GAP];
        let gap_start = 0;
        let gap_end = bytes.len();
        let mut this = Self {
            bytes,
            gap_start,
            gap_end,
            line_starts: vec![0],
            selection_anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            path,
            modified: false,
            version: 0,
        };
        this.rebuild_line_index();
        this
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(err),
        };
        let gap = INITIAL_GAP.max(text.len() / 8);
        let mut bytes = Vec::with_capacity(text.len() + gap);
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend(std::iter::repeat_n(0, gap));
        let gap_start = text.len();
        let gap_end = gap_start + gap;
        let mut this = Self {
            bytes,
            gap_start,
            gap_end,
            line_starts: vec![0],
            selection_anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            path: Some(path),
            modified: false,
            version: 0,
        };
        this.rebuild_line_index();
        this.set_cursor(0, false);
        Ok(this)
    }

    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no file path set",
            ));
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.text().as_bytes())?;
        self.modified = false;
        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[No Name]".to_string())
    }

    pub fn language_name(&self) -> &'static str {
        match self
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or_default()
        {
            "rs" => "Rust",
            "py" => "Python",
            "js" | "mjs" | "cjs" => "JavaScript",
            "ts" | "tsx" => "TypeScript",
            "go" => "Go",
            "md" | "markdown" => "Markdown",
            "json" => "JSON",
            "toml" => "TOML",
            "yaml" | "yml" => "YAML",
            "html" | "htm" => "HTML",
            "css" => "CSS",
            "sh" | "bash" => "Shell",
            _ => "Plain Text",
        }
    }

    pub fn len(&self) -> usize {
        self.bytes.len() - self.gap_len()
    }

    pub fn cursor(&self) -> usize {
        self.gap_start
    }

    pub fn version(&self) -> i32 {
        self.version
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn text(&self) -> String {
        let mut out = Vec::with_capacity(self.len());
        out.extend_from_slice(&self.bytes[..self.gap_start]);
        out.extend_from_slice(&self.bytes[self.gap_end..]);
        String::from_utf8_lossy(&out).into_owned()
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len().max(1)
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts
            .get(line)
            .copied()
            .unwrap_or_else(|| self.len())
    }

    pub fn line_end(&self, line: usize) -> usize {
        let len = self.len();
        if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1].saturating_sub(1)
        } else {
            len
        }
    }

    pub fn line_text(&self, line: usize) -> String {
        let start = self.line_start(line);
        let end = self.line_end(line);
        self.slice(start..end)
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        let text = self.text();
        let start = range.start.min(text.len());
        let end = range.end.min(text.len()).max(start);
        text.get(start..end)
            .map(ToOwned::to_owned)
            .unwrap_or_default()
    }

    pub fn line_col(&self) -> (usize, usize) {
        self.line_col_for_offset(self.cursor())
    }

    pub fn line_col_for_offset(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        };
        (line, offset.saturating_sub(self.line_start(line)))
    }

    pub fn offset_for_line_col(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        let start = self.line_start(line);
        let end = self.line_end(line);
        (start + col).min(end)
    }

    pub fn selected_range(&self) -> Option<Range<usize>> {
        let anchor = self.selection_anchor?;
        let cursor = self.cursor();
        if anchor == cursor {
            None
        } else if anchor < cursor {
            Some(anchor..cursor)
        } else {
            Some(cursor..anchor)
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selected_range().map(|range| self.slice(range))
    }

    pub fn set_cursor(&mut self, pos: usize, selecting: bool) {
        let old = self.cursor();
        if selecting {
            self.selection_anchor.get_or_insert(old);
        } else {
            self.selection_anchor = None;
        }
        self.move_gap(pos.min(self.len()));
    }

    pub fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.move_gap(self.len());
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let range = self
            .selected_range()
            .unwrap_or_else(|| self.cursor()..self.cursor());
        self.replace_range(range, text);
    }

    pub fn delete_backward(&mut self) {
        if let Some(range) = self.selected_range() {
            self.replace_range(range, "");
            return;
        }
        let cursor = self.cursor();
        if cursor == 0 {
            return;
        }
        let start = self.prev_char_boundary(cursor);
        self.replace_range(start..cursor, "");
    }

    pub fn delete_forward(&mut self) {
        if let Some(range) = self.selected_range() {
            self.replace_range(range, "");
            return;
        }
        let cursor = self.cursor();
        if cursor >= self.len() {
            return;
        }
        let end = self.next_char_boundary(cursor);
        self.replace_range(cursor..end, "");
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let range = self.selected_range()?;
        let deleted = self.slice(range.clone());
        self.replace_range(range, "");
        Some(deleted)
    }

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.apply_replacement_no_record(edit.pos, edit.inserted.len(), &edit.deleted);
        self.redo.push(edit);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.apply_replacement_no_record(edit.pos, edit.deleted.len(), &edit.inserted);
        self.undo.push(edit);
        true
    }

    pub fn prev_char_boundary(&self, pos: usize) -> usize {
        let pos = pos.min(self.len());
        let text = self.text();
        text[..pos]
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    pub fn next_char_boundary(&self, pos: usize) -> usize {
        let pos = pos.min(self.len());
        let text = self.text();
        if pos >= text.len() {
            return text.len();
        }
        text[pos..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| pos + idx)
            .unwrap_or(text.len())
    }

    pub fn prev_word_boundary(&self, pos: usize) -> usize {
        let text = self.text();
        let mut chars: Vec<(usize, char)> = text[..pos.min(text.len())].char_indices().collect();
        while matches!(chars.last(), Some((_, ch)) if ch.is_whitespace()) {
            chars.pop();
        }
        let Some((_, last)) = chars.last().copied() else {
            return 0;
        };
        let want_word = is_word(last);
        while matches!(chars.last(), Some((_, ch)) if is_word(*ch) == want_word && !ch.is_whitespace())
        {
            chars.pop();
        }
        chars
            .last()
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(0)
    }

    pub fn next_word_boundary(&self, pos: usize) -> usize {
        let text = self.text();
        let mut iter = text[pos.min(text.len())..].char_indices().peekable();
        while matches!(iter.peek(), Some((_, ch)) if ch.is_whitespace()) {
            iter.next();
        }
        let Some((_, first)) = iter.peek().copied() else {
            return text.len();
        };
        let want_word = is_word(first);
        while matches!(iter.peek(), Some((_, ch)) if is_word(*ch) == want_word && !ch.is_whitespace())
        {
            iter.next();
        }
        iter.peek()
            .map(|(idx, _)| pos.min(text.len()) + idx)
            .unwrap_or(text.len())
    }

    pub fn replace_range(&mut self, range: Range<usize>, inserted: &str) {
        let start = range.start.min(self.len());
        let end = range.end.min(self.len()).max(start);
        let deleted = self.slice(start..end);
        if deleted.is_empty() && inserted.is_empty() {
            return;
        }
        self.apply_replacement_no_record(start, end - start, inserted);
        self.undo.push(AppliedEdit {
            pos: start,
            deleted,
            inserted: inserted.to_string(),
        });
        self.redo.clear();
    }

    fn apply_replacement_no_record(&mut self, pos: usize, delete_len: usize, inserted: &str) {
        self.selection_anchor = None;
        self.move_gap(pos.min(self.len()));
        self.gap_end = (self.gap_end + delete_len).min(self.bytes.len());
        self.ensure_gap(inserted.len());
        let insert_bytes = inserted.as_bytes();
        let end = self.gap_start + insert_bytes.len();
        self.bytes[self.gap_start..end].copy_from_slice(insert_bytes);
        self.gap_start = end;
        self.modified = true;
        self.version = self.version.saturating_add(1);
        self.rebuild_line_index();
    }

    fn gap_len(&self) -> usize {
        self.gap_end - self.gap_start
    }

    fn ensure_gap(&mut self, needed: usize) {
        if self.gap_len() >= needed {
            return;
        }
        let grow_by = (needed - self.gap_len()).max(INITIAL_GAP);
        self.bytes
            .splice(self.gap_end..self.gap_end, std::iter::repeat_n(0, grow_by));
        self.gap_end += grow_by;
    }

    fn move_gap(&mut self, pos: usize) {
        let pos = pos.min(self.len());
        if pos < self.gap_start {
            let shift = self.gap_start - pos;
            let dest = self.gap_end - shift;
            self.bytes.copy_within(pos..self.gap_start, dest);
            self.gap_start = pos;
            self.gap_end = dest;
        } else if pos > self.gap_start {
            let shift = pos - self.gap_start;
            self.bytes
                .copy_within(self.gap_end..self.gap_end + shift, self.gap_start);
            self.gap_start += shift;
            self.gap_end += shift;
        }
    }

    fn rebuild_line_index(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        let text = self.text();
        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                self.line_starts.push(idx + 1);
            }
        }
        debug_assert_eq!(self.line_starts, recompute_line_starts(text.as_bytes()));
    }
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn recompute_line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_undo() {
        let mut b = Buffer::empty(None);
        b.insert_str("hello\nworld");
        assert_eq!(b.line_count(), 2);
        b.set_cursor(5, false);
        b.insert_str(", there");
        assert_eq!(b.text(), "hello, there\nworld");
        assert!(b.undo());
        assert_eq!(b.text(), "hello\nworld");
        assert!(b.redo());
        assert_eq!(b.text(), "hello, there\nworld");
    }
}
