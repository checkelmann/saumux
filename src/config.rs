use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub ink: String,
    pub paper: String,
    pub content: String,
    pub shadow: bool,
}
impl Default for Look {
    fn default() -> Self {
        Self {
            ink: "#aaaaaa".into(),
            paper: "#0000aa".into(),
            content: "color".into(),
            shadow: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Launcher {
    pub label: String,
    pub run: String,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub look: Look,
    pub keys: BTreeMap<String, String>,
    pub menu: BTreeMap<String, Vec<Launcher>>,
    pub start: Vec<String>,
}
impl Config {
    pub fn path() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
            .join("saumux/config.toml")
    }
    pub fn load() -> Result<Self> {
        let path = Self::path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let config: Self =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if !["mono", "color"].contains(&config.look.content.as_str()) {
            bail!("look.content must be mono or color");
        }
        for color in [&config.look.ink, &config.look.paper] {
            if color != "default" && parse_color(color).is_none() {
                bail!("invalid colour {color}: use default or #RRGGBB");
            }
        }
        for (key, command) in &config.keys {
            if !command.is_empty() && !crate::commands::COMMANDS.iter().any(|c| c.name == command) {
                bail!("unknown command {command} for key {key}");
            }
        }
        for name in config.menu.keys() {
            if !crate::commands::MENUS
                .iter()
                .any(|m| m.to_lowercase() == *name)
            {
                bail!("unknown menu {name}");
            }
        }
        Ok(config)
    }
}
pub fn parse_color(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
