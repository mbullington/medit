use std::path::Path;

use crossterm::style::Color;
use syntect::{
    easy::HighlightLines,
    highlighting::{Color as SynColor, Style, Theme, ThemeSet},
    parsing::SyntaxSet,
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

    pub fn highlight_line(&self, path: Option<&Path>, line: &str) -> Vec<StyledSegment> {
        let syntax = path
            .and_then(|path| self.ps.find_syntax_for_file(path).ok().flatten())
            .unwrap_or_else(|| self.ps.find_syntax_plain_text());
        let mut h = HighlightLines::new(syntax, &self.theme);
        match h.highlight_line(line, &self.ps) {
            Ok(ranges) => ranges
                .into_iter()
                .map(|(style, text)| style_segment(style, text))
                .collect(),
            Err(_) => vec![StyledSegment {
                text: line.to_string(),
                fg: None,
            }],
        }
    }
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
