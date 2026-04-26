use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug)]
pub enum MoveKind {
    Char(Direction),
    Word(Direction),
    LineStart,
    LineEnd,
    PageUp,
    PageDown,
}

#[derive(Clone, Debug)]
pub enum Action {
    Save,
    Quit,
    Find,
    FindNext,
    FindPrevious,
    Picker,
    ShowCodeActions,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Move { kind: MoveKind, selecting: bool },
    Insert(char),
    Tab,
    Backspace,
    Delete,
    Enter,
    Create,
    Escape,
    Mouse(MouseEvent),
    None,
}

pub fn event_to_action(event: Event) -> Action {
    match event {
        Event::Key(key) => key_to_action(key),
        Event::Mouse(mouse) => mouse_to_action(mouse),
        Event::Resize(_, _) => Action::None,
        _ => Action::None,
    }
}

fn key_to_action(key: KeyEvent) -> Action {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return Action::None;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    if ctrl {
        match key.code {
            KeyCode::Char('s') | KeyCode::Char('S') => return Action::Save,
            KeyCode::Char('q') | KeyCode::Char('Q') => return Action::Quit,
            KeyCode::Char('f') | KeyCode::Char('F') => return Action::Find,
            KeyCode::Char('g') if shift => return Action::FindPrevious,
            KeyCode::Char('G') => return Action::FindPrevious,
            KeyCode::Char('g') => return Action::FindNext,
            KeyCode::Char('p') | KeyCode::Char('P') => return Action::Picker,
            KeyCode::Char('.') => return Action::ShowCodeActions,
            KeyCode::Char('z') if shift => return Action::Redo,
            KeyCode::Char('Z') => return Action::Redo,
            KeyCode::Char('z') => return Action::Undo,
            KeyCode::Char('x') | KeyCode::Char('X') => return Action::Cut,
            KeyCode::Char('c') | KeyCode::Char('C') => return Action::Copy,
            KeyCode::Char('v') | KeyCode::Char('V') => return Action::Paste,
            KeyCode::Char('a') | KeyCode::Char('A') => return Action::SelectAll,
            KeyCode::Enter => return Action::Create,
            KeyCode::Left => {
                return Action::Move {
                    kind: MoveKind::Word(Direction::Left),
                    selecting: shift,
                };
            }
            KeyCode::Right => {
                return Action::Move {
                    kind: MoveKind::Word(Direction::Right),
                    selecting: shift,
                };
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => Action::Escape,
        KeyCode::Enter if alt => Action::Create,
        KeyCode::Enter => Action::Enter,
        KeyCode::Tab => Action::Tab,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Delete => Action::Delete,
        KeyCode::Left => Action::Move {
            kind: MoveKind::Char(Direction::Left),
            selecting: shift,
        },
        KeyCode::Right => Action::Move {
            kind: MoveKind::Char(Direction::Right),
            selecting: shift,
        },
        KeyCode::Up => Action::Move {
            kind: MoveKind::Char(Direction::Up),
            selecting: shift,
        },
        KeyCode::Down => Action::Move {
            kind: MoveKind::Char(Direction::Down),
            selecting: shift,
        },
        KeyCode::Home => Action::Move {
            kind: MoveKind::LineStart,
            selecting: shift,
        },
        KeyCode::End => Action::Move {
            kind: MoveKind::LineEnd,
            selecting: shift,
        },
        KeyCode::PageUp => Action::Move {
            kind: MoveKind::PageUp,
            selecting: shift,
        },
        KeyCode::PageDown => Action::Move {
            kind: MoveKind::PageDown,
            selecting: shift,
        },
        KeyCode::Char(ch) if !ctrl && !alt => Action::Insert(ch),
        _ => Action::None,
    }
}

fn mouse_to_action(mouse: MouseEvent) -> Action {
    match mouse.kind {
        MouseEventKind::Down(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Up(_)
        | MouseEventKind::ScrollDown
        | MouseEventKind::ScrollUp => Action::Mouse(mouse),
        _ => Action::None,
    }
}
