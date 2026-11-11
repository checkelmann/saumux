//! Borland / Turbo Vision inspired roles, using the classic 16-colour palette.
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
            hotkey: colors(1, 7),    // red menu mnemonic
            shortcut: colors(8, 7),
            status: colors(0, 6),
            key: colors(1, 7),
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
}
impl ThemeId {
    pub const ALL: [Self; 6] = [
        Self::Borland,
        Self::Norton,
        Self::Amber,
        Self::Phosphor,
        Self::Midnight,
        Self::Paper,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Borland => "borland",
            Self::Norton => "norton",
            Self::Amber => "amber",
            Self::Phosphor => "phosphor",
            Self::Midnight => "midnight",
            Self::Paper => "paper",
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
        }
    }
}
impl std::str::FromStr for ThemeId {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|t|t.id()==value.trim().to_lowercase()).ok_or_else(||anyhow::anyhow!("unknown theme {value}; choose borland, norton, amber, phosphor, midnight or paper"))
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
                hotkey: colors(1, 6),
                shortcut: colors(4, 6),
                status: colors(0, 6),
                key: colors(1, 6),
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
                inactive_title: colors(3, 0),
                title: colors(0, 14),
                menu: colors(0, 3),
                selected: colors(0, 14),
                hotkey: colors(4, 3),
                shortcut: colors(0, 3),
                status: colors(0, 3),
                key: colors(4, 3),
                scroll: colors(3, 0),
                thumb: colors(14, 0),
                activity: colors(0, 14),
                dialog: colors(0, 3),
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
                hotkey: colors(15, 2),
                shortcut: colors(0, 2),
                status: colors(0, 2),
                key: colors(15, 2),
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
                inactive_title: colors(8, 7),
                title: colors(15, 4),
                menu: colors(0, 7),
                selected: colors(15, 4),
                hotkey: colors(1, 7),
                shortcut: colors(8, 7),
                status: colors(15, 4),
                key: colors(1, 7),
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
        };
        let (fg, muted, accent, grid, bg) = match id {
            ThemeId::Borland | ThemeId::Norton => (15, 7, 11, 8, 0),
            ThemeId::Amber => (14, 3, 14, 8, 0),
            ThemeId::Phosphor => (10, 2, 10, 8, 0),
            ThemeId::Midnight => (15, 7, 13, 8, 0),
            ThemeId::Paper => (0, 8, 4, 8, 15),
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
