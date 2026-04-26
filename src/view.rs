use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
};

use crate::{
    buffer::Buffer,
    lsp::diagnostics::DiagnosticStore,
    picker::Picker,
    search::SearchState,
    syntax::{StyledSegment, SyntaxCache, SyntaxHighlighter},
};

const GUTTER_FG: Color = Color::DarkGrey;
const FRAME_FG: Color = Color::DarkGrey;
const TITLE_FG: Color = Color::White;
const MUTED_FG: Color = Color::DarkGrey;
const FIND_BG: Color = Color::DarkYellow;
const FIND_FG: Color = Color::Black;

#[derive(Debug, Default)]
pub struct View {
    pub top_line: usize,
    pub left_col: usize,
    preferred_col: Option<usize>,
    syntax_cache: SyntaxCache,
}

pub enum OverlayInfo<'a> {
    None,
    Search,
    Picker(&'a Picker),
    Menu {
        title: &'a str,
        items: &'a [&'a str],
        x: u16,
        selected: usize,
    },
    CodeActions {
        message: &'a str,
        items: &'a [String],
        selected: usize,
        loading: bool,
    },
}

pub struct RenderInfo<'a> {
    pub buffer: &'a Buffer,
    pub search: &'a SearchState,
    pub syntax: &'a SyntaxHighlighter,
    pub diagnostics: &'a DiagnosticStore,
    pub overlay: OverlayInfo<'a>,
    pub status_message: &'a str,
    pub lsp_status: &'a str,
}

impl View {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_preferred_col(&mut self, buffer: &Buffer) -> usize {
        *self
            .preferred_col
            .get_or_insert_with(|| buffer.line_col().1)
    }

    pub fn clear_preferred_col(&mut self) {
        self.preferred_col = None;
    }

    pub fn ensure_cursor_visible(&mut self, buffer: &Buffer, height: u16, width: u16) {
        let (line, col) = buffer.line_col();
        let text_rows = text_height(height);
        if line < self.top_line {
            self.top_line = line;
        } else if text_rows > 0 && line >= self.top_line + text_rows {
            self.top_line = line.saturating_sub(text_rows - 1);
        }

        let gutter = gutter_width(buffer) as usize;
        let text_cols = (width as usize).saturating_sub(gutter + 1).max(1);
        if col < self.left_col {
            self.left_col = col;
        } else if col >= self.left_col + text_cols {
            self.left_col = col.saturating_sub(text_cols - 1);
        }
    }

    pub fn render(&mut self, out: &mut impl Write, info: RenderInfo<'_>) -> io::Result<()> {
        let (width, height) = terminal::size()?;
        self.ensure_cursor_visible(info.buffer, height, width);

        queue!(
            out,
            BeginSynchronizedUpdate,
            Hide,
            ResetColor,
            Clear(ClearType::All)
        )?;

        draw_menu(out, width)?;

        let gutter = gutter_width(info.buffer);
        let rows = text_height(height);
        if rows > 0 {
            let last_visible = self.top_line.saturating_add(rows - 1);
            self.syntax_cache.ensure_highlighted(
                info.syntax,
                info.buffer.path(),
                info.buffer.version(),
                info.buffer.line_count(),
                last_visible,
                || info.buffer.text(),
            );
        }
        for screen_row in 0..rows {
            let line_idx = self.top_line + screen_row;
            let y = editor_y(screen_row);
            queue!(out, MoveTo(0, y), ResetColor)?;
            draw_gutter(out, info.buffer, info.diagnostics, line_idx, gutter)?;
            if line_idx < info.buffer.line_count() {
                let segments = self.syntax_cache.line(line_idx);
                draw_line(
                    out,
                    &info,
                    segments,
                    line_idx,
                    gutter,
                    width,
                    self.left_col,
                    self.top_line,
                    1,
                )?;
            }
            queue!(out, ResetColor, SetAttribute(Attribute::Reset))?;
        }

        let mut overlay_cursor = None;
        match &info.overlay {
            OverlayInfo::None => draw_status(out, &info, width, height.saturating_sub(1))?,
            OverlayInfo::Search => {
                overlay_cursor = Some(draw_search(
                    out,
                    info.search,
                    width,
                    height.saturating_sub(1),
                )?);
            }
            OverlayInfo::Picker(picker) => {
                draw_status(out, &info, width, height.saturating_sub(1))?;
                draw_picker(out, picker, width, height)?;
            }
            OverlayInfo::Menu {
                title,
                items,
                x,
                selected,
            } => {
                draw_status(out, &info, width, height.saturating_sub(1))?;
                draw_menu_dropdown(out, title, items, *x, *selected)?;
            }
            OverlayInfo::CodeActions {
                message,
                items,
                selected,
                loading,
            } => {
                draw_status(out, &info, width, height.saturating_sub(1))?;
                draw_code_actions(out, message, items, *selected, *loading, width, height)?;
            }
        }

        if let Some((x, y)) = overlay_cursor {
            queue!(out, MoveTo(x, y), Show, EndSynchronizedUpdate)?;
            return out.flush();
        }

        let (line, col) = info.buffer.line_col();
        let cursor_y = editor_y(line.saturating_sub(self.top_line));
        let cursor_line = info.buffer.line_text(line);
        let cursor_prefix = cursor_line.get(..col.min(cursor_line.len())).unwrap_or("");
        let cursor_x =
            (gutter as usize + visual_col(cursor_prefix).saturating_sub(self.left_col)) as u16;
        if cursor_y < height.saturating_sub(1) && cursor_x < width {
            queue!(out, MoveTo(cursor_x, cursor_y), Show)?;
        } else {
            queue!(out, MoveTo(0, height.saturating_sub(1)), Show)?;
        }
        queue!(out, EndSynchronizedUpdate)?;
        out.flush()
    }
}

fn draw_menu(out: &mut impl Write, width: u16) -> io::Result<()> {
    queue!(
        out,
        MoveTo(0, 0),
        ResetColor,
        SetAttribute(Attribute::Reverse),
        Print(pad("", width as usize))
    )?;

    let items = [
        (" File ", "F"),
        (" Search ", "S"),
        (" Navigate ", "N"),
        (" LSP ", "L"),
        (" Help ", "H"),
    ];
    let mut x = 1u16;
    for (label, hotkey) in items {
        queue!(out, MoveTo(x, 0), Print(label))?;
        if let Some(offset) = label.find(hotkey) {
            queue!(
                out,
                MoveTo(x + offset as u16, 0),
                SetAttribute(Attribute::Underlined),
                Print(hotkey),
                SetAttribute(Attribute::NoUnderline),
                SetAttribute(Attribute::Reverse)
            )?;
        }
        x = x.saturating_add(label.len() as u16 + 1);
    }
    queue!(out, ResetColor, SetAttribute(Attribute::Reset))
}

fn draw_gutter(
    out: &mut impl Write,
    buffer: &Buffer,
    diagnostics: &DiagnosticStore,
    line_idx: usize,
    gutter: u16,
) -> io::Result<()> {
    let digits = (gutter as usize).saturating_sub(3);
    let marker = if diagnostics.has_error_on_line(line_idx) {
        ("■", Color::Red)
    } else if diagnostics.has_warning_on_line(line_idx) {
        ("■", Color::Yellow)
    } else {
        ("│", FRAME_FG)
    };
    queue!(
        out,
        ResetColor,
        SetForegroundColor(marker.1),
        Print(marker.0),
        ResetColor
    )?;
    if line_idx < buffer.line_count() {
        queue!(
            out,
            SetForegroundColor(GUTTER_FG),
            Print(format!("{:>width$} ", line_idx + 1, width = digits)),
            ResetColor
        )?;
    } else {
        queue!(out, Print(" ".repeat(gutter as usize - 1)))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_line(
    out: &mut impl Write,
    info: &RenderInfo<'_>,
    segments: &[StyledSegment],
    line_idx: usize,
    gutter: u16,
    width: u16,
    left_col: usize,
    top_line: usize,
    y_offset: u16,
) -> io::Result<()> {
    let line_start = info.buffer.line_start(line_idx);
    let line_len = info.buffer.line_end(line_idx).saturating_sub(line_start);
    let mut byte_col = 0usize;
    let mut visual = 0usize;
    let max_cols = width as usize;

    for segment in segments {
        draw_segment(
            out,
            info,
            segment,
            line_idx,
            line_start,
            line_len,
            &mut byte_col,
            &mut visual,
            gutter as usize,
            max_cols,
            left_col,
            top_line,
            y_offset,
        )?;
        if gutter as usize + visual.saturating_sub(left_col) >= max_cols {
            break;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_segment(
    out: &mut impl Write,
    info: &RenderInfo<'_>,
    segment: &StyledSegment,
    line_idx: usize,
    line_start: usize,
    line_len: usize,
    byte_col: &mut usize,
    visual: &mut usize,
    gutter: usize,
    max_cols: usize,
    left_col: usize,
    top_line: usize,
    y_offset: u16,
) -> io::Result<()> {
    for ch in segment.text.chars() {
        let abs = line_start + *byte_col;
        let display = if ch == '\t' {
            "    ".to_string()
        } else {
            ch.to_string()
        };
        let char_width = display.chars().count().max(1);
        if *visual + char_width > left_col {
            let screen_x = gutter + visual.saturating_sub(left_col);
            if screen_x < max_cols {
                let fg = segment.fg;
                let selected = info
                    .buffer
                    .selected_range()
                    .is_some_and(|range| range.start <= abs && abs < range.end);
                let search_hit = search_color(info.search, abs).is_some();
                if info
                    .diagnostics
                    .highlights_position(line_idx, *byte_col, line_len)
                {
                    queue!(
                        out,
                        SetAttribute(Attribute::Underlined),
                        SetForegroundColor(Color::Red)
                    )?;
                } else {
                    queue!(out, SetAttribute(Attribute::NoUnderline))?;
                    if let Some(fg) = fg {
                        queue!(out, SetForegroundColor(fg))?;
                    }
                }
                if selected || search_hit {
                    queue!(out, SetAttribute(Attribute::Reverse))?;
                }
                queue!(
                    out,
                    MoveTo(
                        screen_x as u16,
                        y_offset + line_idx.saturating_sub(top_line) as u16
                    ),
                    Print(display),
                    ResetColor,
                    SetAttribute(Attribute::Reset)
                )?;
            }
        }
        *visual += char_width;
        *byte_col += ch.len_utf8();
    }
    Ok(())
}

fn draw_status(
    out: &mut impl Write,
    info: &RenderInfo<'_>,
    width: u16,
    row: u16,
) -> io::Result<()> {
    let (line, col) = info.buffer.line_col();
    let modified = if info.buffer.is_modified() {
        "●"
    } else {
        "·"
    };
    let left = format!(
        " {} {}  {}  Ln {}, Col {}  {} ",
        modified,
        info.buffer.file_name(),
        info.buffer.language_name(),
        line + 1,
        col + 1,
        info.lsp_status
    );
    let right = if info.status_message.is_empty() {
        format!("theme: {} ", info.syntax.theme_name())
    } else {
        format!("{} ", info.status_message)
    };
    let mut text = left;
    let width_usize = width as usize;
    if text.len() + right.len() < width_usize {
        text.push_str(&" ".repeat(width_usize - text.len() - right.len()));
        text.push_str(&right);
    }
    text.truncate(width_usize);
    queue!(
        out,
        MoveTo(0, row),
        ResetColor,
        SetAttribute(Attribute::Reverse),
        Print(pad(&text, width_usize)),
        ResetColor,
        SetAttribute(Attribute::Reset)
    )
}

fn draw_search(
    out: &mut impl Write,
    search: &SearchState,
    width: u16,
    row: u16,
) -> io::Result<(u16, u16)> {
    let prefix = " Find: ";
    let text = format!(
        "{}{}  [{}/{}]  Enter/Esc closes, Ctrl+G next, Ctrl+Shift+G previous ",
        prefix,
        search.query,
        search.current_index(),
        search.count()
    );
    let cursor_x = (prefix.chars().count() + search.query.chars().count())
        .min(width.saturating_sub(1) as usize) as u16;
    queue!(
        out,
        MoveTo(0, row),
        ResetColor,
        SetBackgroundColor(FIND_BG),
        SetForegroundColor(FIND_FG),
        Print(pad(&text, width as usize)),
        ResetColor,
        SetAttribute(Attribute::Reset)
    )?;
    Ok((cursor_x, row))
}

fn draw_menu_dropdown(
    out: &mut impl Write,
    title: &str,
    items: &[&str],
    x: u16,
    selected: usize,
) -> io::Result<()> {
    let width = items
        .iter()
        .map(|item| item.chars().count())
        .max()
        .unwrap_or(0)
        .max(title.chars().count())
        + 4;
    let height = items.len() + 2;
    draw_box(out, x as usize, 1, width, height, title)?;
    for (idx, item) in items.iter().enumerate() {
        queue!(out, MoveTo(x + 2, 2 + idx as u16))?;
        queue!(out, ResetColor)?;
        if idx == selected {
            queue!(out, SetAttribute(Attribute::Reverse))?;
        }
        queue!(
            out,
            Print(pad(item, width.saturating_sub(4))),
            ResetColor,
            SetAttribute(Attribute::Reset)
        )?;
    }
    Ok(())
}

fn draw_picker(out: &mut impl Write, picker: &Picker, width: u16, height: u16) -> io::Result<()> {
    let box_w = (width as usize * 4 / 5).clamp(34, width as usize);
    let box_h = (height as usize * 3 / 5).clamp(9, (height as usize).saturating_sub(2).max(9));
    let x = (width as usize - box_w) / 2;
    let y = (height as usize - box_h) / 2;
    draw_box(out, x, y, box_w, box_h, " Open / Create ")?;

    let title = format!(" CWD: {} ", picker.cwd().display());
    queue!(
        out,
        MoveTo((x + 2) as u16, (y + 1) as u16),
        ResetColor,
        SetForegroundColor(TITLE_FG),
        SetAttribute(Attribute::Bold),
        Print(pad(&title, box_w.saturating_sub(4))),
        SetAttribute(Attribute::Reset),
        MoveTo((x + 2) as u16, (y + 2) as u16),
        SetAttribute(Attribute::Reverse),
        Print(pad(&format!("> {}", picker.query), box_w.saturating_sub(4))),
        ResetColor,
        SetAttribute(Attribute::Reset)
    )?;

    let visible = box_h.saturating_sub(6);
    let skip = picker.selected().saturating_sub(visible.saturating_sub(1));
    for (screen_idx, (row, path)) in picker.results().skip(skip).take(visible).enumerate() {
        let selected = row == picker.selected();
        queue!(out, MoveTo((x + 2) as u16, (y + 4 + screen_idx) as u16))?;
        queue!(out, ResetColor)?;
        if selected {
            queue!(out, SetAttribute(Attribute::Reverse))?;
        }
        queue!(
            out,
            Print(pad(
                &format!(" {}", path.display()),
                box_w.saturating_sub(4)
            )),
            ResetColor
        )?;
    }
    let help = " Enter opens match · Ctrl/Alt+Enter creates typed path · Esc closes ";
    queue!(
        out,
        MoveTo((x + 2) as u16, (y + box_h - 2) as u16),
        ResetColor,
        SetForegroundColor(MUTED_FG),
        Print(pad(help, box_w.saturating_sub(4))),
        ResetColor
    )
}

fn draw_code_actions(
    out: &mut impl Write,
    message: &str,
    items: &[String],
    selected: usize,
    loading: bool,
    width: u16,
    height: u16,
) -> io::Result<()> {
    let box_w = (width as usize * 3 / 5).clamp(34, width as usize);
    let box_h = (items.len() + 6).clamp(7, height as usize / 2).max(7);
    let x = (width as usize - box_w) / 2;
    let y = (height as usize - box_h) / 2;
    draw_box(out, x, y, box_w, box_h, " Code Actions ")?;
    queue!(
        out,
        MoveTo((x + 2) as u16, (y + 1) as u16),
        ResetColor,
        SetForegroundColor(TITLE_FG),
        SetAttribute(Attribute::Bold),
        Print(pad(message, box_w.saturating_sub(4))),
        ResetColor,
        SetAttribute(Attribute::Reset)
    )?;
    if loading {
        queue!(
            out,
            MoveTo((x + 2) as u16, (y + 3) as u16),
            ResetColor,
            Print(pad(" Loading…", box_w.saturating_sub(4))),
            ResetColor
        )?;
    } else if items.is_empty() {
        queue!(
            out,
            MoveTo((x + 2) as u16, (y + 3) as u16),
            ResetColor,
            Print(pad(" No actions available", box_w.saturating_sub(4))),
            ResetColor
        )?;
    } else {
        for (idx, item) in items.iter().take(box_h.saturating_sub(5)).enumerate() {
            queue!(out, MoveTo((x + 2) as u16, (y + 3 + idx) as u16))?;
            queue!(out, ResetColor)?;
            if idx == selected {
                queue!(out, SetAttribute(Attribute::Reverse))?;
            }
            queue!(
                out,
                Print(pad(&format!(" {item}"), box_w.saturating_sub(4))),
                ResetColor
            )?;
        }
    }
    Ok(())
}

fn draw_box(
    out: &mut impl Write,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    title: &str,
) -> io::Result<()> {
    if width < 4 || height < 3 {
        return Ok(());
    }

    queue!(
        out,
        MoveTo(x as u16, y as u16),
        ResetColor,
        SetForegroundColor(FRAME_FG),
        Print("┌"),
        Print("─".repeat(width - 2)),
        Print("┐"),
        MoveTo((x + 2) as u16, y as u16),
        SetForegroundColor(TITLE_FG),
        SetAttribute(Attribute::Bold),
        Print(title),
        SetAttribute(Attribute::Reset)
    )?;
    for row in 1..height - 1 {
        queue!(
            out,
            MoveTo(x as u16, (y + row) as u16),
            ResetColor,
            SetForegroundColor(FRAME_FG),
            Print("│"),
            ResetColor,
            Print(" ".repeat(width - 2)),
            SetForegroundColor(FRAME_FG),
            Print("│")
        )?;
    }
    queue!(
        out,
        MoveTo(x as u16, (y + height - 1) as u16),
        ResetColor,
        SetForegroundColor(FRAME_FG),
        Print("└"),
        Print("─".repeat(width - 2)),
        Print("┘"),
        ResetColor
    )
}

fn gutter_width(buffer: &Buffer) -> u16 {
    let digits = buffer.line_count().max(1).to_string().len();
    (digits + 3) as u16
}

fn text_height(height: u16) -> usize {
    height.saturating_sub(2) as usize
}

fn editor_y(screen_row: usize) -> u16 {
    screen_row.saturating_add(1) as u16
}

fn visual_col(s: &str) -> usize {
    s.chars().map(|ch| if ch == '\t' { 4 } else { 1 }).sum()
}

fn search_color(search: &SearchState, abs: usize) -> Option<Color> {
    for range in search.visible_matches() {
        if range.start <= abs && abs < range.end {
            return if search
                .current_match()
                .map(|current| current.start == range.start && current.end == range.end)
                .unwrap_or(false)
            {
                Some(Color::Yellow)
            } else {
                Some(Color::DarkYellow)
            };
        }
    }
    None
}

fn pad(text: &str, width: usize) -> String {
    let mut out: String = text.chars().take(width).collect();
    let len = out.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}
