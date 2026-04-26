# medit

`medit` is a small personal, modeless terminal text editor written in Rust.

It is intentionally not a Vim/Emacs replacement and not a TUI framework. It is a single-purpose editor with full-screen redraws, a gap buffer, bespoke overlays, and LSP handled on plain OS threads.

## Build

```sh
cargo build --release
```

Run:

```sh
cargo run -- path/to/file.rs
# or
./target/release/medit path/to/file.rs
```

## Keys

| Action | Binding |
|---|---|
| Save | `Ctrl+S` |
| Quit | `Ctrl+Q` (press twice with unsaved changes) |
| Find / incremental search | `Ctrl+F` |
| Find next / previous | `Ctrl+G` / `Ctrl+Shift+G` |
| Open / create file picker | `Ctrl+P` |
| Create typed picker path | `Ctrl+Enter` or `Alt+Enter` |
| Code actions / diagnostic at cursor | `Ctrl+.` |
| Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Cut / copy / paste | `Ctrl+X` / `Ctrl+C` / `Ctrl+V` |
| Select all | `Ctrl+A` |
| Word movement | `Ctrl+Left` / `Ctrl+Right` |
| Selection | `Shift+Arrows`, mouse drag |

## Features implemented

- Gap-buffer text storage with line index and undo/redo
- Basic editing, cursor movement, selection, save, and guarded quit
- System clipboard integration via `arboard`
- Incremental search with visible match highlighting
- `Ctrl+P` project file picker using `.gitignore`-aware walking and `nucleo` fuzzy matching
- Open existing file with Enter; create the typed path with `Ctrl+Enter` / `Alt+Enter`
- Line number gutter, status line, modified indicator, cursor position, language label
- `syntect` syntax highlighting for common languages and bundled TextMate/Sublime grammars
- Theme loading via `$XDG_CONFIG_HOME/medit.toml`
- Mouse click, drag selection, and wheel scrolling
- LSP transport without async: child process + stdin/stdout threads + main-loop polling
- LSP `initialize`, `didOpen`, full-buffer `didChange`
- Diagnostic gutter marks and underlines
- `Ctrl+.` code action overlay with workspace-edit application for current-file edits

## Configuration

`medit` reads configuration from:

```text
$XDG_CONFIG_HOME/medit.toml
```

If `XDG_CONFIG_HOME` is unset, it falls back to:

```text
~/.config/medit.toml
```

Example:

```toml
# Bundled syntect theme name or a .tmTheme file path.
theme = "InspiredGitHub"

[lsp.rs]
command = "/nix/store/...-rust-analyzer/bin/rust-analyzer"
args = []
language_id = "rust"

[lsp.py]
command = "uv"
args = ["run", "pyright-langserver", "--stdio"]
language_id = "python"
```

By default, `medit` uses syntect's `base16-ocean.dark` theme.

## LSP

Default language-server commands:

| Extension | Command |
|---|---|
| `rs` | `rust-analyzer` |
| `py` | `pyright-langserver --stdio` |
| `js`, `jsx`, `ts`, `tsx` | `typescript-language-server --stdio` |
| `go` | `gopls` |

Override or add language-server commands in `medit.toml` using `[lsp.<extension>]` tables.

LSP support is intentionally v1-scoped: diagnostics and code actions only. Hover, goto definition, completion, splits, git gutter, and project-wide search are deferred.
