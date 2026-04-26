use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use arboard::Clipboard;
use crossterm::{
    event::{self, MouseButton, MouseEvent, MouseEventKind},
    terminal,
};

use crate::{
    buffer::Buffer,
    config::Config,
    input::{Action, Direction, MoveKind, event_to_action},
    lsp::{
        LspClient, LspEvent,
        actions::{CodeActionItem, apply_workspace_edit},
        diagnostics::DiagnosticStore,
    },
    picker::Picker,
    search::SearchState,
    syntax::SyntaxHighlighter,
    view::{OverlayInfo, RenderInfo, View},
};

#[derive(Debug)]
enum Mode {
    Normal,
    Search,
    Picker,
    Menu {
        menu: MenuKind,
        selected: usize,
    },
    CodeActions {
        message: String,
        items: Vec<CodeActionItem>,
        selected: usize,
        loading: bool,
    },
}

#[derive(Clone, Copy, Debug)]
enum MenuKind {
    File,
    Search,
    Navigate,
    Lsp,
    Help,
}

#[derive(Clone, Copy, Debug)]
struct IndentStyle {
    use_tabs: bool,
    width: usize,
}

impl Default for IndentStyle {
    fn default() -> Self {
        Self {
            use_tabs: false,
            width: 4,
        }
    }
}

#[derive(Clone, Copy)]
enum ModeTag {
    Normal,
    Search,
    Picker,
    Menu,
    CodeActions,
}

pub struct App {
    buffer: Buffer,
    view: View,
    search: SearchState,
    picker: Picker,
    syntax: SyntaxHighlighter,
    config: Config,
    lsp: Option<LspClient>,
    diagnostics: DiagnosticStore,
    clipboard: Option<Clipboard>,
    indent: IndentStyle,
    cwd: PathBuf,
    mode: Mode,
    running: bool,
    status: String,
    quit_armed: bool,
}

impl App {
    pub fn new(path: Option<PathBuf>) -> io::Result<Self> {
        let cwd = std::env::current_dir()?;
        let buffer = match path {
            Some(path) => Buffer::open(path)?,
            None => Buffer::empty(None),
        };
        let indent = detect_indent_style(&buffer);
        let config = Config::load();
        let syntax = SyntaxHighlighter::new(config.theme());
        let mut app = Self {
            buffer,
            view: View::new(),
            search: SearchState::default(),
            picker: Picker::new(cwd.clone()),
            syntax,
            config,
            lsp: None,
            diagnostics: DiagnosticStore::default(),
            clipboard: Clipboard::new().ok(),
            indent,
            cwd,
            mode: Mode::Normal,
            running: true,
            status: String::new(),
            quit_armed: false,
        };
        app.start_lsp();
        Ok(app)
    }

    pub fn run(&mut self, out: &mut impl Write) -> io::Result<()> {
        let mut dirty = true;
        while self.running {
            dirty |= self.process_lsp();
            if dirty {
                self.render(out)?;
                dirty = false;
            }

            if event::poll(Duration::from_millis(50))? {
                // Coalesce bursts of input, especially mouse-wheel events. Rendering after every
                // individual scroll event makes the editor feel like it is falling behind the
                // terminal's event stream.
                for _ in 0..128 {
                    let action = event_to_action(event::read()?);
                    self.handle_action(action);
                    dirty = true;
                    if !self.running || !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, out: &mut impl Write) -> io::Result<()> {
        let action_titles: Vec<String> = match &self.mode {
            Mode::CodeActions { items, .. } => {
                items.iter().map(|item| item.title.clone()).collect()
            }
            _ => Vec::new(),
        };
        let overlay = match &self.mode {
            Mode::Normal => OverlayInfo::None,
            Mode::Search => OverlayInfo::Search,
            Mode::Picker => OverlayInfo::Picker(&self.picker),
            Mode::Menu { menu, selected } => OverlayInfo::Menu {
                title: menu.title(),
                items: menu.items(),
                x: menu.x(),
                selected: *selected,
            },
            Mode::CodeActions {
                message,
                selected,
                loading,
                ..
            } => OverlayInfo::CodeActions {
                message,
                items: &action_titles,
                selected: *selected,
                loading: *loading,
            },
        };
        let lsp_status = self
            .lsp
            .as_ref()
            .map(|lsp| format!("LSP:{}", lsp.language_id()))
            .unwrap_or_else(|| "LSP:off".to_string());
        self.view.render(
            out,
            RenderInfo {
                buffer: &self.buffer,
                search: &self.search,
                syntax: &self.syntax,
                diagnostics: &self.diagnostics,
                overlay,
                status_message: &self.status,
                lsp_status: &lsp_status,
            },
        )
    }

    fn handle_action(&mut self, action: Action) {
        if !matches!(&action, Action::None | Action::Quit) {
            self.quit_armed = false;
        }
        let mode = match &self.mode {
            Mode::Normal => ModeTag::Normal,
            Mode::Search => ModeTag::Search,
            Mode::Picker => ModeTag::Picker,
            Mode::Menu { .. } => ModeTag::Menu,
            Mode::CodeActions { .. } => ModeTag::CodeActions,
        };
        match mode {
            ModeTag::Search => self.handle_search_action(action),
            ModeTag::Picker => self.handle_picker_action(action),
            ModeTag::Menu => self.handle_menu_action(action),
            ModeTag::CodeActions => self.handle_code_action_overlay(action),
            ModeTag::Normal => self.handle_normal_action(action),
        }
    }

    fn handle_normal_action(&mut self, action: Action) {
        match action {
            Action::Save => self.save(),
            Action::Quit => self.quit(),
            Action::Find => {
                self.search.open(&self.buffer);
                self.mode = Mode::Search;
            }
            Action::FindNext => {
                self.search.recompute(&self.buffer);
                self.search.next(&mut self.buffer);
            }
            Action::FindPrevious => {
                self.search.recompute(&self.buffer);
                self.search.previous(&mut self.buffer);
            }
            Action::Picker => {
                self.picker.open();
                self.mode = Mode::Picker;
            }
            Action::ShowCodeActions => self.open_code_actions(),
            Action::Undo => self.mutate(|buffer| {
                buffer.undo();
            }),
            Action::Redo => self.mutate(|buffer| {
                buffer.redo();
            }),
            Action::Cut => self.cut(),
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::SelectAll => self.buffer.select_all(),
            Action::Move { kind, selecting } => self.move_cursor(kind, selecting),
            Action::Insert(ch) => self.mutate(|buffer| buffer.insert_str(&ch.to_string())),
            Action::Tab => self.insert_indent(),
            Action::Enter => self.insert_newline_with_indent(),
            Action::Backspace => self.mutate(|buffer| buffer.delete_backward()),
            Action::Delete => self.mutate(|buffer| buffer.delete_forward()),
            Action::Mouse(mouse) => self.handle_mouse(mouse),
            Action::Escape | Action::Create | Action::None => {}
        }
    }

    fn handle_search_action(&mut self, action: Action) {
        match action {
            Action::Escape | Action::Enter => {
                self.search.close();
                self.mode = Mode::Normal;
            }
            Action::FindNext => self.search.next(&mut self.buffer),
            Action::FindPrevious => self.search.previous(&mut self.buffer),
            Action::Backspace => self.search.pop(&self.buffer),
            Action::Insert(ch) => {
                self.search.push(ch, &self.buffer);
                self.search.jump_to_current(&mut self.buffer);
            }
            Action::Paste => {
                if let Some(text) = self.clipboard_text() {
                    for ch in text.chars().filter(|ch| *ch != '\n' && *ch != '\r') {
                        self.search.push(ch, &self.buffer);
                    }
                    self.search.jump_to_current(&mut self.buffer);
                }
            }
            Action::Mouse(mouse) => self.handle_mouse(mouse),
            _ => self.handle_normal_action(action),
        }
    }

    fn handle_picker_action(&mut self, action: Action) {
        match action {
            Action::Escape => {
                self.picker.close();
                self.mode = Mode::Normal;
            }
            Action::Backspace => self.picker.pop(),
            Action::Insert(ch) => self.picker.push(ch),
            Action::Move {
                kind: MoveKind::Char(Direction::Up),
                ..
            } => self.picker.move_selection(-1),
            Action::Move {
                kind: MoveKind::Char(Direction::Down),
                ..
            } => self.picker.move_selection(1),
            Action::Move {
                kind: MoveKind::PageUp,
                ..
            } => self.picker.page(-1),
            Action::Move {
                kind: MoveKind::PageDown,
                ..
            } => self.picker.page(1),
            Action::Enter => {
                if let Some(path) = self.picker.selected_path() {
                    self.open_path(path);
                    self.picker.close();
                    self.mode = Mode::Normal;
                }
            }
            Action::Create => {
                if let Some(path) = self.picker.create_path() {
                    self.create_path(path);
                    self.picker.close();
                    self.mode = Mode::Normal;
                }
            }
            Action::Paste => {
                if let Some(text) = self.clipboard_text() {
                    for ch in text.chars().filter(|ch| *ch != '\n' && *ch != '\r') {
                        self.picker.push(ch);
                    }
                }
            }
            Action::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => self.picker.move_selection(-3),
                MouseEventKind::ScrollDown => self.picker.move_selection(3),
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_menu_action(&mut self, action: Action) {
        match action {
            Action::Escape => self.mode = Mode::Normal,
            Action::Move {
                kind: MoveKind::Char(Direction::Up),
                ..
            } => self.move_menu_selection(-1),
            Action::Move {
                kind: MoveKind::Char(Direction::Down),
                ..
            } => self.move_menu_selection(1),
            Action::Enter => self.activate_selected_menu_item(),
            Action::Mouse(mouse) => self.handle_menu_mouse(mouse),
            _ => {}
        }
    }

    fn handle_code_action_overlay(&mut self, action: Action) {
        match action {
            Action::Escape => self.mode = Mode::Normal,
            Action::Move {
                kind: MoveKind::Char(Direction::Up),
                ..
            } => self.move_code_action_selection(-1),
            Action::Move {
                kind: MoveKind::Char(Direction::Down),
                ..
            } => self.move_code_action_selection(1),
            Action::Enter => self.apply_selected_code_action(),
            Action::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => self.move_code_action_selection(-3),
                MouseEventKind::ScrollDown => self.move_code_action_selection(3),
                _ => {}
            },
            _ => {}
        }
    }

    fn save(&mut self) {
        let result = if self.buffer.path().is_some() {
            self.buffer.save()
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no file path; use Ctrl+P and Ctrl+Enter to create one",
            ))
        };
        match result {
            Ok(()) => {
                self.status = "Saved".into();
                if let Some(lsp) = &mut self.lsp {
                    lsp.notify_did_change(&self.buffer);
                }
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn quit(&mut self) {
        if self.buffer.is_modified() && !self.quit_armed {
            self.quit_armed = true;
            self.status = "Unsaved changes; press Ctrl+Q again to quit without saving".into();
            return;
        }
        self.running = false;
    }

    fn cut(&mut self) {
        if let Some(text) = self.buffer.cut_selection() {
            self.set_clipboard_text(text);
            self.after_edit();
        }
    }

    fn copy(&mut self) {
        if let Some(text) = self.buffer.selected_text() {
            self.set_clipboard_text(text);
            self.status = "Copied".into();
        }
    }

    fn paste(&mut self) {
        if let Some(text) = self.clipboard_text() {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            self.mutate(|buffer| buffer.insert_str(&text));
        }
    }

    fn clipboard_text(&mut self) -> Option<String> {
        self.clipboard.as_mut()?.get_text().ok()
    }

    fn set_clipboard_text(&mut self, text: String) {
        if let Some(clipboard) = &mut self.clipboard {
            if clipboard.set_text(text).is_ok() {
                self.status = "Clipboard updated".into();
            }
        } else {
            self.status = "System clipboard unavailable".into();
        }
    }

    fn mutate(&mut self, f: impl FnOnce(&mut Buffer)) {
        f(&mut self.buffer);
        self.after_edit();
    }

    fn insert_indent(&mut self) {
        let indent = if self.indent.use_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.indent.width)
        };
        self.mutate(|buffer| buffer.insert_str(&indent));
    }

    fn insert_newline_with_indent(&mut self) {
        let (line, _) = self.buffer.line_col();
        let line_text = self.buffer.line_text(line);
        let leading_indent: String = line_text
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .collect();
        let inserted = format!("\n{leading_indent}");
        self.mutate(|buffer| buffer.insert_str(&inserted));
    }

    fn after_edit(&mut self) {
        self.search.recompute(&self.buffer);
        if let Some(lsp) = &mut self.lsp {
            lsp.notify_did_change(&self.buffer);
        }
    }

    fn move_cursor(&mut self, kind: MoveKind, selecting: bool) {
        let (line, col) = self.buffer.line_col();
        let target = match kind {
            MoveKind::Char(Direction::Left) => {
                self.view.clear_preferred_col();
                self.buffer.prev_char_boundary(self.buffer.cursor())
            }
            MoveKind::Char(Direction::Right) => {
                self.view.clear_preferred_col();
                self.buffer.next_char_boundary(self.buffer.cursor())
            }
            MoveKind::Word(Direction::Left) => {
                self.view.clear_preferred_col();
                self.buffer.prev_word_boundary(self.buffer.cursor())
            }
            MoveKind::Word(Direction::Right) => {
                self.view.clear_preferred_col();
                self.buffer.next_word_boundary(self.buffer.cursor())
            }
            MoveKind::Char(Direction::Up) => {
                let preferred = self.view.take_preferred_col(&self.buffer);
                self.buffer
                    .offset_for_line_col(line.saturating_sub(1), preferred)
            }
            MoveKind::Char(Direction::Down) => {
                let preferred = self.view.take_preferred_col(&self.buffer);
                self.buffer
                    .offset_for_line_col((line + 1).min(self.buffer.line_count() - 1), preferred)
            }
            MoveKind::LineStart => {
                self.view.clear_preferred_col();
                self.buffer.line_start(line)
            }
            MoveKind::LineEnd => {
                self.view.clear_preferred_col();
                self.buffer.line_end(line)
            }
            MoveKind::PageUp => {
                let preferred = self.view.take_preferred_col(&self.buffer);
                let line = line.saturating_sub(20);
                self.buffer.offset_for_line_col(line, preferred)
            }
            MoveKind::PageDown => {
                let preferred = self.view.take_preferred_col(&self.buffer);
                let line = (line + 20).min(self.buffer.line_count() - 1);
                self.buffer.offset_for_line_col(line, preferred)
            }
            MoveKind::Word(Direction::Up) | MoveKind::Word(Direction::Down) => self.buffer.cursor(),
        };
        self.buffer.set_cursor(target, selecting);
        if !matches!(
            kind,
            MoveKind::Char(Direction::Up | Direction::Down) | MoveKind::PageUp | MoveKind::PageDown
        ) {
            self.view.clear_preferred_col();
        }
        let _ = col;
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && let Some(menu) = menu_at_column(mouse.column, mouse.row)
        {
            self.mode = Mode::Menu { menu, selected: 0 };
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_view(-3),
            MouseEventKind::ScrollDown => self.scroll_view(3),
            MouseEventKind::Down(MouseButton::Left) => {
                let offset = self.offset_for_mouse(mouse.column, mouse.row);
                self.buffer.set_cursor(offset, false);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let offset = self.offset_for_mouse(mouse.column, mouse.row);
                self.buffer.set_cursor(offset, true);
            }
            _ => {}
        }
    }

    fn move_menu_selection(&mut self, delta: isize) {
        if let Mode::Menu { menu, selected } = &mut self.mode {
            let len = menu.items().len();
            *selected = (*selected as isize + delta).clamp(0, len as isize - 1) as usize;
        }
    }

    fn scroll_view(&mut self, delta: isize) {
        let visible = terminal::size()
            .map(|(_, height)| height.saturating_sub(2) as usize)
            .unwrap_or(22)
            .max(1);
        let max_top = self.buffer.line_count().saturating_sub(1);
        self.view.top_line = if delta.is_negative() {
            self.view.top_line.saturating_sub(delta.unsigned_abs())
        } else {
            (self.view.top_line + delta as usize).min(max_top)
        };

        let (line, col) = self.buffer.line_col();
        if line < self.view.top_line {
            let target = self.buffer.offset_for_line_col(self.view.top_line, col);
            self.buffer.set_cursor(target, false);
        } else if line >= self.view.top_line + visible {
            let bottom = (self.view.top_line + visible - 1).min(self.buffer.line_count() - 1);
            let target = self.buffer.offset_for_line_col(bottom, col);
            self.buffer.set_cursor(target, false);
        }
    }

    fn handle_menu_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        if let Some(menu) = menu_at_column(mouse.column, mouse.row) {
            self.mode = Mode::Menu { menu, selected: 0 };
            return;
        }
        let Mode::Menu { menu, .. } = self.mode else {
            return;
        };
        let row = mouse.row as usize;
        let column = mouse.column;
        let width = menu.width() as u16;
        let x = menu.x();
        let in_menu_x = x <= column && column < x.saturating_add(width);
        let first_item_row = 2usize;
        let last_item_row = first_item_row + menu.items().len();
        if in_menu_x && row >= first_item_row && row < last_item_row {
            let selected = row - first_item_row;
            self.mode = Mode::Menu { menu, selected };
            self.activate_selected_menu_item();
        } else {
            self.mode = Mode::Normal;
        }
    }

    fn activate_selected_menu_item(&mut self) {
        let (menu, selected) = match self.mode {
            Mode::Menu { menu, selected } => (menu, selected),
            _ => return,
        };
        self.mode = Mode::Normal;
        match (menu, selected) {
            (MenuKind::File, 0) => self.save(),
            (MenuKind::File, 1) => {
                self.picker.open();
                self.mode = Mode::Picker;
            }
            (MenuKind::File, 2) => self.quit(),
            (MenuKind::Search, 0) => {
                self.search.open(&self.buffer);
                self.mode = Mode::Search;
            }
            (MenuKind::Search, 1) => {
                self.search.recompute(&self.buffer);
                self.search.next(&mut self.buffer);
            }
            (MenuKind::Search, 2) => {
                self.search.recompute(&self.buffer);
                self.search.previous(&mut self.buffer);
            }
            (MenuKind::Navigate, 0) => self.buffer.set_cursor(0, false),
            (MenuKind::Navigate, 1) => self.buffer.set_cursor(self.buffer.len(), false),
            (MenuKind::Lsp, 0) => self.open_code_actions(),
            (MenuKind::Lsp, 1) => self.start_lsp(),
            (MenuKind::Help, 0) => {
                self.status = "Keys: Ctrl+S save · Ctrl+P picker · Ctrl+F find · Ctrl+. code actions · Ctrl+Q quit".into();
            }
            _ => {}
        }
    }

    fn offset_for_mouse(&self, column: u16, row: u16) -> usize {
        let gutter = self.gutter_width() as usize;
        let text_row = row.saturating_sub(1) as usize;
        let line = (self.view.top_line + text_row).min(self.buffer.line_count().saturating_sub(1));
        let col = (column as usize).saturating_sub(gutter) + self.view.left_col;
        self.buffer.offset_for_line_col(line, col)
    }

    fn gutter_width(&self) -> u16 {
        (self.buffer.line_count().max(1).to_string().len() + 3) as u16
    }

    fn open_path(&mut self, path: PathBuf) {
        if self.buffer.is_modified() {
            self.status = "Save or quit current file before opening another".into();
            return;
        }
        match Buffer::open(&path) {
            Ok(buffer) => {
                self.indent = detect_indent_style(&buffer);
                self.buffer = buffer;
                self.view = View::new();
                self.diagnostics = DiagnosticStore::default();
                self.search.recompute(&self.buffer);
                self.status = format!("Opened {}", path.display());
                self.start_lsp();
            }
            Err(err) => self.status = format!("Open failed: {err}"),
        }
    }

    fn create_path(&mut self, path: PathBuf) {
        if self.buffer.is_modified() {
            self.status = "Save or quit current file before creating another".into();
            return;
        }
        let mut buffer = Buffer::empty(Some(path.clone()));
        match buffer.save() {
            Ok(()) => {
                self.indent = detect_indent_style(&buffer);
                self.buffer = buffer;
                self.view = View::new();
                self.diagnostics = DiagnosticStore::default();
                self.status = format!("Created {}", path.display());
                self.start_lsp();
            }
            Err(err) => self.status = format!("Create failed: {err}"),
        }
    }

    fn start_lsp(&mut self) {
        self.lsp = None;
        let Some(path) = self.buffer.path() else {
            self.status = "No file path; LSP off".into();
            return;
        };
        let Some(config) = self.config.lsp_for_path(path) else {
            return;
        };
        match LspClient::spawn(config, &self.buffer, &self.cwd) {
            Ok(client) => self.lsp = Some(client),
            Err(err) => self.status = format!("LSP unavailable: {err}"),
        }
    }

    fn process_lsp(&mut self) -> bool {
        let Some(lsp) = &mut self.lsp else {
            return false;
        };
        let events = lsp.poll(&self.buffer);
        let changed = !events.is_empty();
        for event in events {
            match event {
                LspEvent::Diagnostics(diagnostics) => self.diagnostics.set(diagnostics),
                LspEvent::CodeActions(items) => {
                    if let Mode::CodeActions {
                        items: existing,
                        loading,
                        ..
                    } = &mut self.mode
                    {
                        *existing = items;
                        *loading = false;
                    }
                }
                LspEvent::Status(status) => self.status = status,
            }
        }
        changed
    }

    fn open_code_actions(&mut self) {
        let (line, col) = self.buffer.line_col();
        let diagnostics = self.diagnostics.at_position(line, col);
        let message = diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "No diagnostic at cursor; requesting contextual actions".into());
        self.mode = Mode::CodeActions {
            message,
            items: Vec::new(),
            selected: 0,
            loading: self.lsp.is_some(),
        };
        if let Some(lsp) = &mut self.lsp {
            lsp.request_code_actions(&self.buffer, diagnostics);
        } else if let Mode::CodeActions { loading, .. } = &mut self.mode {
            *loading = false;
        }
    }

    fn move_code_action_selection(&mut self, delta: isize) {
        if let Mode::CodeActions {
            items, selected, ..
        } = &mut self.mode
        {
            if items.is_empty() {
                *selected = 0;
            } else {
                *selected =
                    (*selected as isize + delta).clamp(0, items.len() as isize - 1) as usize;
            }
        }
    }

    fn apply_selected_code_action(&mut self) {
        let selected = match &self.mode {
            Mode::CodeActions {
                items,
                selected,
                loading,
                ..
            } if !*loading => items.get(*selected).cloned(),
            _ => None,
        };
        let Some(action) = selected else {
            return;
        };
        if let Some(edit) = &action.edit {
            let count = apply_workspace_edit(&mut self.buffer, edit);
            self.after_edit();
            self.status = format!("Applied {count} edit(s)");
        }
        if let Some(command) = &action.command
            && let Some(lsp) = &mut self.lsp
        {
            lsp.execute_command(command);
        }
        self.mode = Mode::Normal;
    }
}

fn detect_indent_style(buffer: &Buffer) -> IndentStyle {
    let mut tab_lines = 0usize;
    let mut space_runs = Vec::new();

    for line_idx in 0..buffer.line_count() {
        let line = buffer.line_text(line_idx);
        if line.trim().is_empty() {
            continue;
        }
        let leading_spaces = line.chars().take_while(|ch| *ch == ' ').count();
        let leading_tabs = line.chars().take_while(|ch| *ch == '\t').count();
        if leading_tabs > 0 {
            tab_lines += 1;
        } else if leading_spaces > 0 {
            space_runs.push(leading_spaces);
        }
    }

    if tab_lines > space_runs.len() {
        return IndentStyle {
            use_tabs: true,
            width: 4,
        };
    }

    let mut best = (0usize, 4usize);
    for width in [2usize, 4, 8] {
        let score = space_runs
            .iter()
            .map(|spaces| usize::from(*spaces == width) * 2 + usize::from(*spaces % width == 0))
            .sum();
        if score > best.0 || (score == best.0 && width == 4) {
            best = (score, width);
        }
    }

    IndentStyle {
        use_tabs: false,
        width: best.1,
    }
}

impl MenuKind {
    fn title(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Search => "Search",
            Self::Navigate => "Navigate",
            Self::Lsp => "LSP",
            Self::Help => "Help",
        }
    }

    fn items(self) -> &'static [&'static str] {
        match self {
            Self::File => &["Save   Ctrl+S", "Open…  Ctrl+P", "Quit   Ctrl+Q"],
            Self::Search => &["Find   Ctrl+F", "Next   Ctrl+G", "Prev   Ctrl+Shift+G"],
            Self::Navigate => &["Top", "Bottom"],
            Self::Lsp => &["Code actions  Ctrl+.", "Restart LSP"],
            Self::Help => &["Show shortcuts"],
        }
    }

    fn x(self) -> u16 {
        match self {
            Self::File => 1,
            Self::Search => 8,
            Self::Navigate => 17,
            Self::Lsp => 28,
            Self::Help => 34,
        }
    }

    fn width(self) -> usize {
        self.items()
            .iter()
            .map(|item| item.chars().count())
            .max()
            .unwrap_or(0)
            .max(self.title().chars().count())
            + 4
    }
}

fn menu_at_column(column: u16, row: u16) -> Option<MenuKind> {
    if row != 0 {
        return None;
    }
    let menus = [
        (MenuKind::File, 1, 7),
        (MenuKind::Search, 8, 16),
        (MenuKind::Navigate, 17, 27),
        (MenuKind::Lsp, 28, 33),
        (MenuKind::Help, 34, 40),
    ];
    menus
        .into_iter()
        .find_map(|(menu, start, end)| (start <= column && column < end).then_some(menu))
}

#[allow(dead_code)]
fn display_path(path: &Path) -> String {
    path.strip_prefix(std::env::current_dir().unwrap_or_default())
        .unwrap_or(path)
        .display()
        .to_string()
}
