//! Configuration for the Markdown → Typst converter.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TypstConfig {
    pub table: TableConfig,
    pub blockquote: BlockquoteConfig,
    pub hr: HrConfig,
    pub image: ImageConfig,
    pub code: CodeConfig,
    pub page: PageConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TableConfig {
    pub header_bold: bool,
    pub stroke: String,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self {
            header_bold: true,
            stroke: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BlockquoteConfig {
    pub function: String,
}

impl Default for BlockquoteConfig {
    fn default() -> Self {
        Self {
            function: "quote".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HrConfig {
    pub style: String,
}

impl Default for HrConfig {
    fn default() -> Self {
        Self {
            style: "#line(length: 100%)".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ImageConfig {
    pub use_figure: bool,
    pub width: String,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            use_figure: true,
            width: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CodeConfig {
    pub block_function: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PageConfig {
    pub paper: String,
}

/// Default config file path: $XDG_CONFIG_HOME/mdtyp/config.toml
pub fn default_config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs_fallback().join(".config")
        });
    base.join("mdtyp").join("config.toml")
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Load config from a TOML file. Returns defaults if no file exists.
pub fn load_config(path: Option<&Path>) -> TypstConfig {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => {
            let default = default_config_path();
            if !default.exists() {
                return TypstConfig::default();
            }
            default
        }
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("warning: failed to parse config {}: {}", path.display(), e);
            TypstConfig::default()
        }),
        Err(_) => TypstConfig::default(),
    }
}
