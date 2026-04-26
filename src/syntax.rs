use std::path::{Path, PathBuf};

use crossterm::style::Color;
use syntect::{
    easy::HighlightLines,
    highlighting::{Color as SynColor, HighlightState, Style, Theme, ThemeSet},
    parsing::{ParseState, SyntaxSet},
};

#[derive(Clone, Debug)]
pub struct StyledSegment {
    pub text: String,
    pub fg: Option<Color>,
}

pub struct SyntaxHighlighter {
    ps: SyntaxSet,
    theme: Theme,
    theme_name: String,
}

#[derive(Debug, Default)]
pub struct SyntaxCache {
    version: Option<i32>,
    path: Option<PathBuf>,
    line_count: usize,
    highlighted: Vec<Vec<StyledSegment>>,
    parsed_bytes: usize,
    state: Option<(HighlightState, ParseState)>,
}

impl SyntaxCache {
    pub fn ensure_highlighted(
        &mut self,
        highlighter: &SyntaxHighlighter,
        path: Option<&Path>,
        version: i32,
        line_count: usize,
        through_line: usize,
        get_text: impl FnOnce() -> String,
    ) {
        if !self.matches(path, version, line_count) {
            self.reset(path, version, line_count);
        }

        let requested = through_line.saturating_add(1).min(line_count);
        if requested <= self.highlighted.len() {
            return;
        }

        let text = get_text();
        let syntax = highlighter.syntax_for_path(path);
        let mut h = if let Some((highlight_state, parse_state)) = self.state.take() {
            HighlightLines::from_state(&highlighter.theme, highlight_state, parse_state)
        } else {
            HighlightLines::new(syntax, &highlighter.theme)
        };
        let mut byte_offset = self.parsed_bytes.min(text.len());

        while self.highlighted.len() < requested {
            let line_idx = self.highlighted.len();
            let (line, next_offset) = line_at(&text, byte_offset);
            let append_newline = line_idx + 1 < line_count;
            let segments = highlight_with_state(&mut h, &highlighter.ps, line, append_newline);
            self.highlighted.push(segments);
            byte_offset = next_offset;
        }

        self.parsed_bytes = byte_offset;
        self.state = Some(h.state());
    }

    pub fn line(&self, line_idx: usize) -> &[StyledSegment] {
        self.highlighted
            .get(line_idx)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn matches(&self, path: Option<&Path>, version: i32, line_count: usize) -> bool {
        self.version == Some(version)
            && self.path.as_deref() == path
            && self.line_count == line_count
    }

    fn reset(&mut self, path: Option<&Path>, version: i32, line_count: usize) {
        self.version = Some(version);
        self.path = path.map(Path::to_path_buf);
        self.line_count = line_count;
        self.highlighted.clear();
        self.parsed_bytes = 0;
        self.state = None;
    }
}

impl SyntaxHighlighter {
    pub fn new(configured_theme: Option<&str>) -> Self {
        let ps = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();
        let (theme, theme_name) = configured_theme
            .and_then(|theme| load_theme(theme, &ts))
            .unwrap_or_else(|| {
                let name = "base16-ocean.dark";
                let theme = ts
                    .themes
                    .get(name)
                    .cloned()
                    .or_else(|| ts.themes.values().next().cloned())
                    .expect("syntect ships at least one default theme");
                (theme, name.to_string())
            });
        Self {
            ps,
            theme,
            theme_name,
        }
    }

    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }

    fn syntax_for_path(&self, path: Option<&Path>) -> &syntect::parsing::SyntaxReference {
        path.and_then(|path| self.ps.find_syntax_for_file(path).ok().flatten())
            .unwrap_or_else(|| self.ps.find_syntax_plain_text())
    }
}

fn line_at(text: &str, byte_offset: usize) -> (&str, usize) {
    let rest = text.get(byte_offset..).unwrap_or_default();
    match rest.find('\n') {
        Some(newline) => (&rest[..newline], byte_offset + newline + 1),
        None => (rest, text.len()),
    }
}

fn highlight_with_state(
    h: &mut HighlightLines<'_>,
    ps: &SyntaxSet,
    line: &str,
    append_newline: bool,
) -> Vec<StyledSegment> {
    let mut line_with_newline;
    let input = if append_newline {
        line_with_newline = String::with_capacity(line.len() + 1);
        line_with_newline.push_str(line);
        line_with_newline.push('\n');
        line_with_newline.as_str()
    } else {
        line
    };

    match h.highlight_line(input, ps) {
        Ok(ranges) => trim_highlighted_newline(ranges, line.len()),
        Err(_) => vec![StyledSegment {
            text: line.to_string(),
            fg: None,
        }],
    }
}

fn trim_highlighted_newline(ranges: Vec<(Style, &str)>, line_len: usize) -> Vec<StyledSegment> {
    let mut remaining = line_len;
    let mut segments = Vec::new();

    for (style, text) in ranges {
        if remaining == 0 {
            break;
        }

        let take = text.len().min(remaining);
        if let Some(text) = text.get(..take)
            && !text.is_empty()
        {
            segments.push(style_segment(style, text));
        }
        remaining -= take;
    }

    segments
}

fn load_theme(value: &str, ts: &ThemeSet) -> Option<(Theme, String)> {
    let path = Path::new(value);
    if path.exists() {
        let theme = ThemeSet::get_theme(path).ok()?;
        return Some((theme, value.to_string()));
    }
    ts.themes
        .get(value)
        .cloned()
        .map(|theme| (theme, value.to_string()))
}

fn style_segment(style: Style, text: &str) -> StyledSegment {
    StyledSegment {
        text: text.to_string(),
        fg: syn_to_term(style.foreground),
    }
}

fn syn_to_term(color: SynColor) -> Option<Color> {
    if color.a == 0 {
        None
    } else {
        Some(Color::Rgb {
            r: color.r,
            g: color.g,
            b: color.b,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_multiline_blocks_with_prior_line_state() {
        let highlighter = SyntaxHighlighter::new(None);
        let lines = [
            "fn main() {",
            "    /* comment",
            "       still comment */ let x = 1;",
            "}",
        ];

        let text = lines.join("\n");
        let mut cache = SyntaxCache::default();
        cache.ensure_highlighted(
            &highlighter,
            Some(Path::new("test.rs")),
            0,
            lines.len(),
            2,
            || text.clone(),
        );
        let comment_start = fg_for(cache.line(1), "comment");
        let continued_comment = fg_for(cache.line(2), "still");
        let code_after_comment = fg_for(cache.line(2), "let");

        assert_eq!(continued_comment, comment_start);
        assert_ne!(continued_comment, code_after_comment);
    }

    #[test]
    fn strips_parser_newlines_from_rendered_segments() {
        let highlighter = SyntaxHighlighter::new(None);
        let lines = ["// comment", "let x = 1;"];

        let text = lines.join("\n");
        let mut cache = SyntaxCache::default();
        cache.ensure_highlighted(
            &highlighter,
            Some(Path::new("test.rs")),
            0,
            lines.len(),
            0,
            || text.clone(),
        );
        let rendered: String = cache
            .line(0)
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();

        assert_eq!(rendered, lines[0]);
        assert!(!rendered.contains('\n'));
    }

    fn fg_for(segments: &[StyledSegment], needle: &str) -> Option<Color> {
        segments
            .iter()
            .find(|segment| segment.text.contains(needle))
            .and_then(|segment| segment.fg)
    }
}
