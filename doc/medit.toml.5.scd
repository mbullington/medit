medit.toml(5)

# NAME

medit.toml - configuration file for medit

# DESCRIPTION

*medit.toml* configures the *medit*(1) terminal text editor.

The file is TOML. Unknown keys are ignored by the current implementation unless
they make the file invalid TOML.

Configuration is loaded once at startup.

# LOCATION

*medit* reads configuration from:

	*$XDG_CONFIG_HOME/medit.toml*

If *XDG_CONFIG_HOME* is unset or empty, *medit* falls back to:

	*~/.config/medit.toml*

# TOP-LEVEL KEYS

*theme* = _string_
	Optional syntect theme. The value may be either the name of a bundled syntect
	theme or a path to a *.tmTheme* file.

	If unset, *medit* uses *base16-ocean.dark*.

# LSP TABLES

Language servers are configured with tables named *[lsp.EXT]*, where *EXT* is a
file extension without the leading dot.

For example, Rust files use:

```
[lsp.rs]
```

Each LSP table supports these keys:

*command* = _string_
	Executable to spawn. This may be an absolute path or a command found on
	*PATH*.

*args* = [_string_, ...]
	Arguments passed to *command*. Defaults to an empty array.

*language_id* = _string_
	LSP language ID sent in *textDocument/didOpen*. If omitted for an extension
	with a built-in default, the built-in language ID is reused. If omitted for a
	new extension, the extension itself is used as the language ID.

# DEFAULTS

If no configuration file exists, *medit* uses these built-in language servers:

*rs*
	*command* = "rust-analyzer"++
	*args* = []++
	*language_id* = "rust"

*py*
	*command* = "pyright-langserver"++
	*args* = ["--stdio"]++
	*language_id* = "python"

*js*, *jsx*
	*command* = "typescript-language-server"++
	*args* = ["--stdio"]++
	*language_id* = "javascript"

*ts*, *tsx*
	*command* = "typescript-language-server"++
	*args* = ["--stdio"]++
	*language_id* = "typescript"

*go*
	*command* = "gopls"++
	*args* = []++
	*language_id* = "go"

Configured LSP tables override the default for the same extension. They may also
add new extensions.

# EXAMPLES

Use a bundled syntect theme:

```
theme = "InspiredGitHub"
```

Use a theme file:

```
theme = "/home/me/themes/MyTheme.tmTheme"
```

Use an absolute rust-analyzer path:

```
[lsp.rs]
command = "/nix/store/example-rust-analyzer/bin/rust-analyzer"
args = []
language_id = "rust"
```

Run pyright through uv:

```
[lsp.py]
command = "uv"
args = ["run", "pyright-langserver", "--stdio"]
language_id = "python"
```

Add YAML language-server support:

```
[lsp.yaml]
command = "yaml-language-server"
args = ["--stdio"]
language_id = "yaml"

[lsp.yml]
command = "yaml-language-server"
args = ["--stdio"]
language_id = "yaml"
```

# NOTES

*medit* does not install language servers. The configured command must exist and
must speak LSP over standard input and standard output.

For commands with subcommands or wrappers, put the executable in *command* and
each separate argument in *args*. Do not put a full shell command line in
*command*.

# SEE ALSO

*medit*(1), *rust-analyzer*(1), *gopls*(1)
