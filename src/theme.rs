//! Shared retro theme roles, using the classic 16-colour palette.
//! Keeping roles here gives the desktop and bundled applications one visual language.
use crate::render::Style;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};
use vt100::Color;

#[derive(Clone)]
pub struct Theme {
    pub desktop: Style,
    pub frame: Style,
    pub inactive_title: Style,
    pub title: Style,
    pub menu: Style,
    pub selected: Style,
    pub hotkey: Style,
    pub shortcut: Style,
    pub status: Style,
    pub key: Style,
    pub shadow: Style,
    pub scroll: Style,
    pub thumb: Style,
    pub activity: Style,
    pub dialog: Style,
    pub text: Style,
    pub editor: Style,
    pub directory: Style,
    pub field: Style,
    pub warning: Style,
    pub monitor_text: Style,
    pub monitor_muted: Style,
    pub monitor_accent: Style,
    pub monitor_grid: Style,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            desktop: colors(6, 4), // cyan stipple on blue
            frame: colors(15, 4),  // white rules on blue
            inactive_title: colors(7, 4),
            title: colors(0, 6),     // black on cyan, active window
            menu: colors(0, 7),      // Turbo Vision silver menu surfaces
            selected: colors(15, 4), // white on blue, menu selection
            hotkey: mnemonic(7),     // underlined menu mnemonic
            shortcut: colors(0, 7),
            status: colors(0, 6),
            key: mnemonic(7),
            shadow: colors(0, 0),
            scroll: colors(6, 4),
            thumb: colors(14, 4),
            activity: colors(14, 1),
            dialog: colors(0, 7),
            text: colors(7, 4),
            editor: colors(14, 4), // the classic yellow-on-blue editor
            directory: colors(15, 4),
            field: colors(14, 4),
            warning: colors(15, 1),
            monitor_text: colors(15, 0),
            monitor_muted: colors(7, 0),
            monitor_accent: colors(11, 0),
            monitor_grid: colors(8, 0),
        }
    }
}
pub fn colors(fg: u8, bg: u8) -> Style {
    Style {
        fg: Some(Color::Idx(fg)),
        bg: Some(Color::Idx(bg)),
        ..Style::default()
    }
}

// Underlined black keeps menu mnemonics legible even with pale host ANSI accents.
fn mnemonic(bg: u8) -> Style {
    Style {
        underline: true,
        ..colors(0, bg)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeId {
    #[default]
    Borland,
    Norton,
    Amber,
    Phosphor,
    Midnight,
    Paper,
    Ocean,
    Plum,
    Forest,
    Burgundy,
    Ice,
    Slate,
    Matrix,
    Atari,
    Tokyo,
}
impl ThemeId {
    pub const ALL: [Self; 15] = [
        Self::Borland,
        Self::Norton,
        Self::Amber,
        Self::Phosphor,
        Self::Midnight,
        Self::Paper,
        Self::Ocean,
        Self::Plum,
        Self::Forest,
        Self::Burgundy,
        Self::Ice,
        Self::Slate,
        Self::Matrix,
        Self::Atari,
        Self::Tokyo,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Borland => "borland",
            Self::Norton => "norton",
            Self::Amber => "amber",
            Self::Phosphor => "phosphor",
            Self::Midnight => "midnight",
            Self::Paper => "paper",
            Self::Ocean => "ocean",
            Self::Plum => "plum",
            Self::Forest => "forest",
            Self::Burgundy => "burgundy",
            Self::Ice => "ice",
            Self::Slate => "slate",
            Self::Matrix => "matrix",
            Self::Atari => "atari",
            Self::Tokyo => "tokyo",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Borland => "Borland",
            Self::Norton => "Norton",
            Self::Amber => "Amber",
            Self::Phosphor => "Phosphor",
            Self::Midnight => "Midnight",
            Self::Paper => "Paper",
            Self::Ocean => "Ocean",
            Self::Plum => "Plum",
            Self::Forest => "Forest",
            Self::Burgundy => "Burgundy",
            Self::Ice => "Ice",
            Self::Slate => "Slate",
            Self::Matrix => "Matrix",
            Self::Atari => "Atari",
            Self::Tokyo => "Tokyo",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Borland => "Classic blue and silver",
            Self::Norton => "Blue panels, yellow highlights",
            Self::Amber => "Warm amber terminal",
            Self::Phosphor => "Green CRT terminal",
            Self::Midnight => "Dark charcoal and violet",
            Self::Paper => "Light paper and blue ink",
            Self::Ocean => "Deep blue and seafoam",
            Self::Plum => "Purple panels, pink highlights",
            Self::Forest => "Green panels and silver",
            Self::Burgundy => "Wine red and warm cream",
            Self::Ice => "Light cyan and blue ink",
            Self::Slate => "Charcoal panels and cyan",
            Self::Matrix => "Green code rain on black",
            Self::Atari => "Atari ST light grey GEM desktop",
            Self::Tokyo => "Dark modern with magenta and cyan",
        }
    }
}
impl std::str::FromStr for ThemeId {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value_normalized = value.trim().to_lowercase();
        Self::ALL
            .into_iter()
            .find(|t| t.id() == value_normalized)
            .ok_or_else(|| {
                let choices = Self::ALL.map(Self::id).join(", ");
                anyhow::anyhow!("unknown theme {value}; choose {choices}")
            })
    }
}
impl Theme {
    pub fn named(id: ThemeId) -> Self {
        let mut theme = match id {
            ThemeId::Borland => Self::default(),
            ThemeId::Norton => Self {
                desktop: colors(6, 4),
                frame: colors(6, 4),
                inactive_title: colors(7, 4),
                title: colors(0, 14),
                menu: colors(0, 6),
                selected: colors(0, 14),
                hotkey: mnemonic(6),
                shortcut: colors(4, 6),
                status: colors(0, 6),
                key: mnemonic(6),
                scroll: colors(6, 4),
                thumb: colors(14, 4),
                activity: colors(14, 1),
                dialog: colors(0, 6),
                text: colors(7, 4),
                editor: colors(15, 4),
                directory: colors(15, 4),
                field: colors(14, 4),
                ..Self::default()
            },
            ThemeId::Amber => Self {
                desktop: colors(3, 0),
                frame: colors(3, 0),
                inactive_title: colors(11, 0),
                title: colors(0, 14),
                menu: colors(0, 11),
                selected: colors(0, 14),
                hotkey: colors(4, 11),
                shortcut: colors(0, 11),
                status: colors(0, 11),
                key: colors(4, 11),
                scroll: colors(3, 0),
                thumb: colors(14, 0),
                activity: colors(0, 14),
                dialog: colors(0, 11),
                text: colors(14, 0),
                editor: colors(14, 0),
                directory: colors(14, 0),
                field: colors(14, 0),
                ..Self::default()
            },
            ThemeId::Phosphor => Self {
                desktop: colors(2, 0),
                frame: colors(2, 0),
                inactive_title: colors(2, 0),
                title: colors(0, 10),
                menu: colors(0, 2),
                selected: colors(0, 10),
                hotkey: mnemonic(2),
                shortcut: colors(0, 2),
                status: colors(0, 2),
                key: mnemonic(2),
                scroll: colors(2, 0),
                thumb: colors(10, 0),
                activity: colors(0, 10),
                dialog: colors(0, 2),
                text: colors(10, 0),
                editor: colors(10, 0),
                directory: colors(10, 0),
                field: colors(10, 0),
                ..Self::default()
            },
            ThemeId::Midnight => Self {
                desktop: colors(8, 0),
                frame: colors(8, 0),
                inactive_title: colors(7, 0),
                title: colors(15, 5),
                menu: colors(15, 8),
                selected: colors(0, 6),
                hotkey: colors(14, 8),
                shortcut: colors(15, 8),
                status: colors(15, 8),
                key: colors(14, 8),
                scroll: colors(8, 0),
                thumb: colors(13, 0),
                activity: colors(14, 5),
                dialog: colors(15, 8),
                text: colors(7, 0),
                editor: colors(15, 0),
                directory: colors(13, 0),
                field: colors(15, 5),
                ..Self::default()
            },
            ThemeId::Paper => Self {
                desktop: colors(8, 7),
                frame: colors(0, 7),
                inactive_title: colors(0, 7),
                title: colors(15, 4),
                menu: colors(0, 7),
                selected: colors(15, 4),
                hotkey: mnemonic(7),
                shortcut: colors(0, 7),
                status: colors(15, 4),
                key: mnemonic(7),
                scroll: colors(8, 7),
                thumb: colors(4, 7),
                activity: colors(14, 1),
                dialog: colors(0, 7),
                text: colors(0, 7),
                editor: colors(0, 15),
                directory: colors(4, 7),
                field: colors(0, 15),
                ..Self::default()
            },
            ThemeId::Ocean => Self {
                desktop: colors(6, 4),
                frame: colors(6, 4),
                inactive_title: colors(15, 4),
                title: colors(0, 10),
                menu: colors(0, 6),
                selected: colors(15, 4),
                hotkey: colors(4, 6),
                shortcut: colors(0, 6),
                status: colors(0, 10),
                key: colors(4, 6),
                shadow: colors(0, 0),
                scroll: colors(6, 4),
                thumb: colors(10, 4),
                activity: colors(0, 14),
                dialog: colors(0, 6),
                text: colors(15, 4),
                editor: colors(15, 4),
                directory: colors(10, 4),
                field: colors(15, 4),
                warning: colors(15, 1),
                monitor_text: colors(15, 0),
                monitor_muted: colors(7, 0),
                monitor_accent: colors(10, 0),
                monitor_grid: colors(8, 0),
            },
            ThemeId::Plum => Self {
                desktop: colors(13, 5),
                frame: colors(13, 5),
                inactive_title: colors(15, 5),
                title: colors(0, 13),
                menu: colors(0, 7),
                selected: colors(15, 5),
                hotkey: mnemonic(7),
                shortcut: colors(0, 7),
                status: colors(0, 13),
                key: mnemonic(7),
                shadow: colors(0, 0),
                scroll: colors(13, 5),
                thumb: colors(14, 5),
                activity: colors(0, 14),
                dialog: colors(0, 7),
                text: colors(15, 5),
                editor: colors(15, 5),
                directory: colors(14, 5),
                field: colors(15, 5),
                warning: colors(15, 1),
                monitor_text: colors(15, 0),
                monitor_muted: colors(7, 0),
                monitor_accent: colors(14, 0),
                monitor_grid: colors(8, 0),
            },
            ThemeId::Forest => Self {
                desktop: colors(2, 0),
                frame: colors(2, 0),
                inactive_title: colors(7, 0),
                title: colors(0, 2),
                menu: colors(0, 7),
                selected: colors(0, 2),
                hotkey: mnemonic(7),
                shortcut: colors(0, 7),
                status: colors(0, 2),
                key: mnemonic(7),
                shadow: colors(0, 0),
                scroll: colors(2, 0),
                thumb: colors(10, 0),
                activity: colors(0, 14),
                dialog: colors(0, 7),
                text: colors(7, 0),
                editor: colors(15, 0),
                directory: colors(10, 0),
                field: colors(15, 0),
                warning: colors(15, 1),
                monitor_text: colors(15, 0),
                monitor_muted: colors(7, 0),
                monitor_accent: colors(10, 0),
                monitor_grid: colors(8, 0),
            },
            ThemeId::Burgundy => Self {
                desktop: colors(1, 0),
                frame: colors(1, 0),
                inactive_title: colors(7, 0),
                title: colors(15, 1),
                menu: colors(0, 7),
                selected: colors(15, 1),
                hotkey: mnemonic(7),
                shortcut: colors(0, 7),
                status: colors(15, 1),
                key: mnemonic(7),
                shadow: colors(0, 0),
                scroll: colors(1, 0),
                thumb: colors(14, 0),
                activity: colors(0, 14),
                dialog: colors(0, 7),
                text: colors(7, 0),
                editor: colors(14, 0),
                directory: colors(14, 0),
                field: colors(14, 0),
                warning: colors(15, 1),
                monitor_text: colors(15, 0),
                monitor_muted: colors(7, 0),
                monitor_accent: colors(14, 0),
                monitor_grid: colors(8, 0),
            },
            ThemeId::Ice => Self {
                desktop: colors(6, 15),
                frame: colors(6, 15),
                inactive_title: colors(0, 15),
                title: colors(15, 4),
                menu: colors(0, 6),
                selected: colors(15, 4),
                hotkey: colors(4, 6),
                shortcut: colors(0, 6),
                status: colors(15, 4),
                key: colors(4, 6),
                shadow: colors(0, 0),
                scroll: colors(6, 15),
                thumb: colors(4, 15),
                activity: colors(0, 14),
                dialog: colors(0, 6),
                text: colors(0, 15),
                editor: colors(0, 15),
                directory: colors(4, 15),
                field: colors(0, 15),
                warning: colors(15, 1),
                monitor_text: colors(0, 15),
                monitor_muted: colors(8, 15),
                monitor_accent: colors(4, 15),
                monitor_grid: colors(8, 15),
            },
            ThemeId::Slate => Self {
                desktop: colors(8, 0),
                frame: colors(8, 0),
                inactive_title: colors(7, 0),
                title: colors(0, 6),
                menu: colors(15, 8),
                selected: colors(0, 6),
                hotkey: colors(14, 8),
                shortcut: colors(15, 8),
                status: colors(0, 6),
                key: colors(14, 8),
                shadow: colors(0, 0),
                scroll: colors(8, 0),
                thumb: colors(11, 0),
                activity: colors(0, 14),
                dialog: colors(15, 8),
                text: colors(7, 0),
                editor: colors(15, 0),
                directory: colors(11, 0),
                field: colors(15, 0),
                warning: colors(15, 1),
                monitor_text: colors(15, 0),
                monitor_muted: colors(7, 0),
                monitor_accent: colors(11, 0),
                monitor_grid: colors(8, 0),
            },
            ThemeId::Matrix => Self {
                desktop: colors(10, 0),
                frame: colors(10, 0),
                inactive_title: colors(2, 0),  // dark green on black
                title: colors(0, 10),           // black on bright green
                menu: colors(0, 2),             // black on dark green
                selected: colors(0, 10),
                hotkey: mnemonic(2),
                shortcut: colors(0, 2),
                status: colors(0, 10),
                key: mnemonic(2),
                shadow: colors(0, 0),
                scroll: colors(2, 0),
                thumb: colors(10, 0),
                activity: colors(0, 10),
                dialog: colors(0, 2),
                text: colors(10, 0),
                editor: colors(10, 0),
                directory: colors(10, 0),
                field: colors(10, 0),
                warning: colors(15, 1),
                monitor_text: colors(10, 0),
                monitor_muted: colors(2, 0),
                monitor_accent: colors(10, 0),
                monitor_grid: colors(2, 0),
            },
            ThemeId::Atari => Self {
                desktop: colors(8, 7),          // dark-grey stipple on light grey
                frame: colors(0, 7),            // black rules on grey
                inactive_title: colors(0, 7),
                title: colors(15, 4),           // white on blue, like classic GEM
                menu: colors(0, 7),
                selected: colors(15, 4),
                hotkey: mnemonic(7),
                shortcut: colors(0, 7),
                status: colors(15, 4),
                key: mnemonic(7),
                shadow: colors(0, 0),
                scroll: colors(8, 7),
                thumb: colors(0, 7),
                activity: colors(15, 1),
                dialog: colors(0, 7),
                text: colors(0, 7),
                editor: colors(0, 15),          // black on white, GEM document
                directory: colors(4, 7),        // blue on grey
                field: colors(0, 15),
                warning: colors(15, 1),
                monitor_text: colors(15, 0),
                monitor_muted: colors(7, 0),
                monitor_accent: colors(11, 0),
                monitor_grid: colors(8, 0),
            },
            ThemeId::Tokyo => Self {
                desktop: colors(8, 0),
                frame: colors(8, 0),
                inactive_title: colors(7, 0),
                title: colors(0, 13),           // black on bright magenta/pink
                menu: colors(15, 8),            // white on dark grey
                selected: colors(0, 14),        // black on bright cyan
                hotkey: colors(14, 8),          // bright cyan on dark grey
                shortcut: colors(15, 8),
                status: colors(0, 14),
                key: colors(14, 8),
                shadow: colors(0, 0),
                scroll: colors(8, 0),
                thumb: colors(13, 0),           // bright magenta thumb
                activity: colors(0, 13),
                dialog: colors(15, 8),
                text: colors(15, 0),
                editor: colors(15, 0),
                directory: colors(14, 0),       // bright cyan directories
                field: colors(15, 0),
                warning: colors(15, 1),
                monitor_text: colors(15, 0),
                monitor_muted: colors(7, 0),
                monitor_accent: colors(13, 0),  // bright magenta accent
                monitor_grid: colors(8, 0),
            },
        };
        let (fg, muted, accent, grid, bg) = match id {
            ThemeId::Borland | ThemeId::Norton => (15, 7, 11, 8, 0),
            ThemeId::Amber => (14, 11, 14, 8, 0),
            ThemeId::Phosphor => (10, 2, 10, 8, 0),
            ThemeId::Midnight => (15, 7, 13, 8, 0),
            ThemeId::Paper => (0, 8, 4, 8, 15),
            ThemeId::Ocean
            | ThemeId::Plum
            | ThemeId::Forest
            | ThemeId::Burgundy
            | ThemeId::Ice
            | ThemeId::Slate
            | ThemeId::Matrix
            | ThemeId::Atari
            | ThemeId::Tokyo => return theme,
        };
        theme.monitor_text = colors(fg, bg);
        theme.monitor_muted = colors(muted, bg);
        theme.monitor_accent = colors(accent, bg);
        theme.monitor_grid = colors(grid, bg);
        theme
    }
}

// Bundled applications share a private per-session state file. Reading at most
// every 100 ms lets an idle editor repaint without restarting or injecting input.
struct LiveTheme {
    id: ThemeId,
    palette: Theme,
    checked: Option<Instant>,
}
thread_local! {static LIVE:RefCell<LiveTheme>=RefCell::new(LiveTheme {id:ThemeId::Borland,palette:Theme::default(),checked:None});}
pub fn current() -> Theme {
    LIVE.with(|cache| {
        let mut live = cache.borrow_mut();
        if live
            .checked
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(100))
        {
            live.checked = Some(Instant::now());
            if let Some(path) = std::env::var_os("SAUMUX_THEME_FILE")
                && let Ok(contents) = std::fs::read_to_string(path)
                && let Ok(id) = contents.parse::<ThemeId>()
            {
                live.id = id;
                live.palette = Theme::named(id);
            }
        }
        live.palette.clone()
    })
}
pub fn current_id() -> ThemeId {
    current();
    LIVE.with(|cache| cache.borrow().id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_theme_text_has_readable_contrast_in_the_bundled_palette() {
        // Keep this reference tied to the actual browser palette. Host terminals
        // may customize ANSI colours, but our bundled frontend is predictable.
        let palette = [
            ("black", "000000"),
            ("red", "aa0000"),
            ("green", "00aa00"),
            ("yellow", "aa5500"),
            ("blue", "0000aa"),
            ("magenta", "aa00aa"),
            ("cyan", "00aaaa"),
            ("white", "aaaaaa"),
            ("brightBlack", "555555"),
            ("brightRed", "ff5555"),
            ("brightGreen", "55ff55"),
            ("brightYellow", "ffff55"),
            ("brightBlue", "5555ff"),
            ("brightMagenta", "ff55ff"),
            ("brightCyan", "55ffff"),
            ("brightWhite", "ffffff"),
        ];
        let luminance = palette.map(|(name, hex)| {
            assert!(include_str!("../web/app.js").contains(&format!("{name}: \"#{hex}\"")));
            [0.2126, 0.7152, 0.0722]
                .into_iter()
                .enumerate()
                .map(|(i, weight)| {
                    let c =
                        f64::from(u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()) / 255.0;
                    weight
                        * if c <= 0.04045 {
                            c / 12.92
                        } else {
                            ((c + 0.055) / 1.055).powf(2.4)
                        }
                })
                .sum::<f64>()
        });
        for id in ThemeId::ALL {
            let t = Theme::named(id);
            for (name, style) in [
                ("inactive_title", t.inactive_title),
                ("title", t.title),
                ("menu", t.menu),
                ("selected", t.selected),
                ("hotkey", t.hotkey),
                ("shortcut", t.shortcut),
                ("status", t.status),
                ("key", t.key),
                ("activity", t.activity),
                ("dialog", t.dialog),
                ("text", t.text),
                ("editor", t.editor),
                ("directory", t.directory),
                ("field", t.field),
                ("warning", t.warning),
                ("monitor_text", t.monitor_text),
                ("monitor_muted", t.monitor_muted),
                ("monitor_accent", t.monitor_accent),
            ] {
                let (Some(Color::Idx(fg)), Some(Color::Idx(bg))) = (style.fg, style.bg) else {
                    panic!("{} {name}: expected indexed colours", id.id());
                };
                let (a, b) = (luminance[fg as usize], luminance[bg as usize]);
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(
                    ratio >= 4.5,
                    "{} {name}: contrast {ratio:.2}:1 is below 4.5:1",
                    id.id()
                );
            }
        }
    }
}
