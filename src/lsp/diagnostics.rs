use lsp_types::{Diagnostic, DiagnosticSeverity, Range};

use crate::buffer::Buffer;

#[derive(Clone, Debug, Default)]
pub struct DiagnosticStore {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticStore {
    pub fn set(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics = diagnostics;
    }

    pub fn at_position(&self, buffer: &Buffer, line: usize, byte_col: usize) -> Vec<Diagnostic> {
        let utf16_col = buffer.utf16_col_for_byte_col(line, byte_col);
        let exact: Vec<_> = self
            .diagnostics
            .iter()
            .filter(|diagnostic| contains(&diagnostic.range, line, utf16_col))
            .cloned()
            .collect();
        if exact.is_empty() {
            self.on_line(line).into_iter().cloned().collect()
        } else {
            exact
        }
    }

    pub fn highlights_position(&self, line: usize, utf16_col: usize, has_text: bool) -> bool {
        self.diagnostics.iter().any(|diagnostic| {
            contains(&diagnostic.range, line, utf16_col)
                || (is_degenerate(&diagnostic.range)
                    && line_in_range(&diagnostic.range, line)
                    && has_text)
        })
    }

    pub fn on_line(&self, line: usize) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| touches_line(&diagnostic.range, line))
            .collect()
    }

    pub fn has_error_on_line(&self, line: usize) -> bool {
        self.on_line(line)
            .into_iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR) || d.severity.is_none())
    }

    pub fn has_warning_on_line(&self, line: usize) -> bool {
        self.on_line(line)
            .into_iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::WARNING))
    }
}

fn contains(range: &Range, line: usize, col: usize) -> bool {
    let start_line = range.start.line as usize;
    let start_col = range.start.character as usize;
    let end_line = range.end.line as usize;
    let end_col = range.end.character as usize;
    if line < start_line || line > end_line {
        return false;
    }
    if line == start_line && col < start_col {
        return false;
    }
    if line == end_line && col >= end_col {
        return false;
    }
    true
}

fn touches_line(range: &Range, line: usize) -> bool {
    let start = range.start.line as usize;
    let end = range.end.line as usize;
    if line < start || line > end {
        return false;
    }
    !(start < end && line == end && range.end.character == 0)
}

fn line_in_range(range: &Range, line: usize) -> bool {
    let start = range.start.line as usize;
    let end = range.end.line as usize;
    start <= line && line <= end
}

fn is_degenerate(range: &Range) -> bool {
    range.start.line == range.end.line && range.start.character == range.end.character
}

#[cfg(test)]
mod tests {
    use lsp_types::{Diagnostic, Position};

    use super::*;

    fn diagnostic(range: Range) -> Diagnostic {
        Diagnostic::new_simple(range, "test".into())
    }

    #[test]
    fn matches_utf16_ranges_against_byte_columns() {
        let mut buffer = Buffer::empty(None);
        buffer.insert_str("a😀broken");
        let mut store = DiagnosticStore::default();
        store.set(vec![
            diagnostic(Range::new(Position::new(0, 3), Position::new(0, 9))),
            diagnostic(Range::new(Position::new(0, 10), Position::new(0, 11))),
        ]);

        assert!(!store.highlights_position(0, 1, true));
        assert!(store.highlights_position(0, 3, true));
        assert_eq!(store.at_position(&buffer, 0, 1).len(), 2);
        assert_eq!(store.at_position(&buffer, 0, 5).len(), 1);
    }

    #[test]
    fn excludes_empty_end_line_of_multiline_range() {
        let mut store = DiagnosticStore::default();
        store.set(vec![diagnostic(Range::new(
            Position::new(0, 2),
            Position::new(1, 0),
        ))]);

        assert_eq!(store.on_line(0).len(), 1);
        assert!(store.on_line(1).is_empty());
    }
}
