use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct LspServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub language_id: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    lsp_by_extension: HashMap<String, LspServerConfig>,
    theme: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    theme: Option<String>,
    #[serde(default)]
    lsp: HashMap<String, FileLspServerConfig>,
}

#[derive(Debug, Deserialize)]
struct FileLspServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    language_id: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let mut lsp_by_extension = default_lsp_configs();
        let mut theme = None;

        if let Some(file_config) = load_file_config() {
            theme = file_config.theme;
            for (ext, server) in file_config.lsp {
                let ext = ext.trim_start_matches('.').to_string();
                let language_id = server
                    .language_id
                    .or_else(|| {
                        lsp_by_extension
                            .get(&ext)
                            .map(|config| config.language_id.clone())
                    })
                    .unwrap_or_else(|| ext.clone());
                lsp_by_extension.insert(
                    ext,
                    LspServerConfig {
                        command: server.command,
                        args: server.args,
                        language_id,
                    },
                );
            }
        }

        Self {
            lsp_by_extension,
            theme,
        }
    }

    pub fn lsp_for_path(&self, path: &Path) -> Option<LspServerConfig> {
        let ext = path.extension()?.to_str()?;
        self.lsp_by_extension.get(ext).cloned()
    }

    pub fn theme(&self) -> Option<&str> {
        self.theme.as_deref()
    }
}

fn default_lsp_configs() -> HashMap<String, LspServerConfig> {
    let mut map = HashMap::new();
    add(&mut map, &["rs"], "rust-analyzer", &[], "rust");
    add(
        &mut map,
        &["py"],
        "pyright-langserver",
        &["--stdio"],
        "python",
    );
    add(
        &mut map,
        &["js", "jsx"],
        "typescript-language-server",
        &["--stdio"],
        "javascript",
    );
    add(
        &mut map,
        &["ts", "tsx"],
        "typescript-language-server",
        &["--stdio"],
        "typescript",
    );
    add(&mut map, &["go"], "gopls", &[], "go");
    map
}

fn add(
    map: &mut HashMap<String, LspServerConfig>,
    exts: &[&str],
    command: &str,
    args: &[&str],
    language_id: &str,
) {
    for ext in exts {
        map.insert(
            (*ext).to_string(),
            LspServerConfig {
                command: command.to_string(),
                args: args.iter().map(|arg| (*arg).to_string()).collect(),
                language_id: language_id.to_string(),
            },
        );
    }
}

fn load_file_config() -> Option<FileConfig> {
    let path = config_path()?;
    let text = fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

fn config_path() -> Option<PathBuf> {
    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty())
    {
        return Some(PathBuf::from(xdg_config_home).join("medit.toml"));
    }
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("medit.toml"))
}
