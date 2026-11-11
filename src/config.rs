use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub theme: crate::theme::ThemeId,
    pub ink: String,
    pub paper: String,
    pub content: String,
    pub shadow: bool,
}
impl Default for Look {
    fn default() -> Self {
        Self {
            theme: crate::theme::ThemeId::Borland,
            ink: "theme".into(),
            paper: "theme".into(),
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
    pub agents: Vec<crate::agents::AgentConfig>,
    pub notifications: Notifications,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Notifications {
    pub detect_prompts: bool,
    pub prompt_delay_ms: u64,
}
impl Default for Notifications {
    fn default() -> Self {
        Self {
            detect_prompts: true,
            prompt_delay_ms: 900,
        }
    }
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
        crate::agents::validate(&config.agents)?;
        if !(100..=10000).contains(&config.notifications.prompt_delay_ms) {
            bail!("notifications.prompt_delay_ms must be between 100 and 10000");
        }
        if !["mono", "color"].contains(&config.look.content.as_str()) {
            bail!("look.content must be mono or color");
        }
        for color in [&config.look.ink, &config.look.paper] {
            if color != "default" && color != "theme" && parse_color(color).is_none() {
                bail!("invalid colour {color}: use theme, default or #RRGGBB");
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

    /// Persist an interactive theme choice as the default for future sessions,
    /// preserving the rest of the user's TOML file and comments.
    pub fn save_default_theme(theme: crate::theme::ThemeId) -> Result<()> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        use std::{fs, io::Write};

        // Do not replace a malformed or semantically invalid user configuration.
        Self::load()?;
        let path = Self::path();
        let old = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let mut lines: Vec<String> = old.lines().map(str::to_owned).collect();
        let mut in_look = false;
        let mut found_look = false;
        let mut found_theme = false;
        let mut index = 0;
        while index < lines.len() {
            let trimmed = lines[index].trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_look = trimmed == "[look]";
                found_look |= in_look;
            }
            let top_level_key = !trimmed.starts_with('[') && trimmed.starts_with("look.theme");
            let direct_theme = in_look
                && trimmed
                    .split_once('=')
                    .is_some_and(|(key, _)| key.trim() == "theme");
            if top_level_key || direct_theme {
                lines[index] = replace_theme_assignment(&lines[index], theme);
                found_theme = true;
            }
            index += 1;
        }
        if !found_theme {
            if found_look {
                let section = lines
                    .iter()
                    .position(|line| line.trim() == "[look]")
                    .unwrap();
                lines.insert(section + 1, format!("theme = \"{}\"", theme.id()));
            } else {
                if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
                    lines.push(String::new());
                }
                lines.push("[look]".into());
                lines.push(format!("theme = \"{}\"", theme.id()));
            }
        }
        let contents = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };
        let parent = path.parent().context("configuration path has no parent")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(error)
                }
            })?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temp = parent.join(format!(
            ".config.toml-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            if let Ok(metadata) = fs::metadata(&path) {
                file.set_permissions(metadata.permissions())?;
            }
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &path).with_context(|| format!("saving {}", path.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

fn replace_theme_assignment(line: &str, theme: crate::theme::ThemeId) -> String {
    let comment = line.find('#').map(|i| &line[i..]).unwrap_or("");
    let assignment = line.split('#').next().unwrap_or(line);
    let left = assignment
        .split_once('=')
        .map(|(left, _)| left.trim_end())
        .unwrap_or("theme");
    format!("{left} = \"{}\"{}", theme.id(), comment)
}
pub fn parse_color(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
