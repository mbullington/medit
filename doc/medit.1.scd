medit(1)

# NAME

medit - a small modeless terminal text editor

# SYNOPSIS

*medit* [_file_]

# DESCRIPTION

*medit* is a personal terminal text editor written in Rust. It is modeless:
keys map directly to editor actions, and there is no Vim-style command mode.

If _file_ is provided, *medit* opens it. If the file does not exist, an empty
buffer is opened for that path and the file is created when saved. If no file is
provided, *medit* starts with an unnamed buffer.

The editor uses the terminal alternate screen, raw mode, mouse reporting, and a
synchronous event loop. Syntax highlighting is provided by syntect. Language
server support is provided by spawning external LSP server commands configured
by file extension.

# KEY BINDINGS

*Ctrl+S*
	Save.

*Ctrl+Q*
	Quit. If the buffer has unsaved changes, press twice to quit without saving.

*Ctrl+F*
	Open incremental search in the bottom bar.

*Ctrl+G*
	Find next match.

*Ctrl+Shift+G*
	Find previous match.

*Ctrl+P*
	Open the file picker.

*Ctrl+Enter*, *Alt+Enter*
	Create and open the typed path in the file picker.

*Ctrl+.*
	Show diagnostics and code actions at the cursor.

*Ctrl+Z*
	Undo.

*Ctrl+Shift+Z*
	Redo.

*Ctrl+X*, *Ctrl+C*, *Ctrl+V*
	Cut, copy, and paste using the system clipboard.

*Ctrl+A*
	Select all.

*Ctrl+Left*, *Ctrl+Right*
	Move by word.

*Shift+Arrow keys*
	Select text.

Mouse drag
	Select text.

Mouse wheel
	Scroll the editor, picker, or code-action list.

# FILE PICKER

*Ctrl+P* opens the file picker. The picker walks the current working directory,
respects git ignore files, and fuzzy-filters paths as text is typed.

Press *Enter* to open the highlighted existing file.

Press *Ctrl+Enter* or *Alt+Enter* to create and open the typed path. Parent
directories are created on save.

The picker intentionally ignores the *.git* directory.

# SEARCH

*Ctrl+F* opens incremental search in the bottom bar. Typing updates matches as
you search. Press *Ctrl+G* for the next match and *Ctrl+Shift+G* for the previous
match. Press *Enter* or *Esc* to close the find bar.

# LSP

*medit* does not bundle language servers. It starts language server executables
that are installed on the system and available by the configured command path.

The default LSP commands are:

*rs*
	*rust-analyzer* with language ID *rust*.

*py*
	*pyright-langserver --stdio* with language ID *python*.

*js*, *jsx*
	*typescript-language-server --stdio* with language ID *javascript*.

*ts*, *tsx*
	*typescript-language-server --stdio* with language ID *typescript*.

*go*
	*gopls* with language ID *go*.

LSP support is scoped to diagnostics and code actions. *medit* sends
*initialize*, *initialized*, *textDocument/didOpen*, and full-buffer
*textDocument/didChange* notifications. It renders published diagnostics and
requests code actions with *Ctrl+.*.

Hover, completion, goto definition, and project-wide search are not implemented.

# CONFIGURATION

Configuration is read from *$XDG_CONFIG_HOME/medit.toml*. If *XDG_CONFIG_HOME*
is unset, *medit* falls back to *~/.config/medit.toml*.

See *medit.toml*(5) for the file format.

# FILES

*$XDG_CONFIG_HOME/medit.toml*
	User configuration file.

*~/.config/medit.toml*
	Fallback configuration file when *XDG_CONFIG_HOME* is unset.

# ENVIRONMENT

*XDG_CONFIG_HOME*
	Base directory used to find *medit.toml*.

# EXIT STATUS

*0*
	The editor exited normally.

*>0*
	An error occurred while starting or running the editor.

# SEE ALSO

*medit.toml*(5), *rust-analyzer*(1), *gopls*(1)
