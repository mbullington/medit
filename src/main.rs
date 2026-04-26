mod app;
mod buffer;
mod config;
mod input;
mod lsp;
mod picker;
mod search;
mod syntax;
mod view;

use std::{io, path::PathBuf};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use app::App;

fn main() -> io::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .filter(|arg| arg != "-h" && arg != "--help")
        .map(PathBuf::from);

    if std::env::args_os().any(|arg| arg == "-h" || arg == "--help") {
        print_help();
        return Ok(());
    }

    let mut app = App::new(path)?;
    let mut stdout = io::stdout();
    let _terminal = TerminalGuard::enter(&mut stdout)?;
    app.run(&mut stdout)
}

fn print_help() {
    println!("medit - a small modeless terminal text editor");
    println!();
    println!("Usage: medit [file]");
    println!();
    println!("Keys:");
    println!("  Ctrl+S              save");
    println!("  Ctrl+Q              quit (press twice with unsaved changes)");
    println!("  Ctrl+F              incremental search");
    println!("  Ctrl+G / Ctrl+Shift+G next / previous match");
    println!("  Ctrl+P              open/create file picker");
    println!("  Ctrl+Enter/Alt+Enter create typed picker path");
    println!("  Ctrl+.              diagnostic/code actions");
    println!("  Ctrl+Z / Ctrl+Shift+Z undo / redo");
    println!("  Ctrl+X/C/V          cut / copy / paste");
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter(stdout: &mut io::Stdout) -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}
