use lsp_types::{Diagnostic, DiagnosticSeverity, Range};

#[derive(Clone, Debug, Default)]
pub struct DiagnosticStore {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticStore {
    pub fn set(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics = diagnostics;
    }

    pub fn at_position(&self, line: usize, col: usize) -> Vec<Diagnostic> {
        let exact: Vec<_> = self
            .diagnostics
            .iter()
            .filter(|diagnostic| contains(&diagnostic.range, line, col))
            .cloned()
            .collect();
        if exact.is_empty() {
            self.on_line(line).into_iter().cloned().collect()
        } else {
            exact
        }
    }

    pub fn highlights_position(&self, line: usize, col: usize, line_len: usize) -> bool {
        self.diagnostics.iter().any(|diagnostic| {
            contains(&diagnostic.range, line, col)
                || (is_degenerate(&diagnostic.range)
                    && line_in_range(&diagnostic.range, line)
                    && col < line_len)
        })
    }

    pub fn on_line(&self, line: usize) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| {
                let start = diagnostic.range.start.line as usize;
                let end = diagnostic.range.end.line as usize;
                start <= line && line <= end
            })
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

fn line_in_range(range: &Range, line: usize) -> bool {
    let start = range.start.line as usize;
    let end = range.end.line as usize;
    start <= line && line <= end
}

fn is_degenerate(range: &Range) -> bool {
    range.start.line == range.end.line && range.start.character == range.end.character
}
