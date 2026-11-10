//! Borland / Turbo Vision inspired roles, using the classic 16-colour palette.
//! Keeping roles here gives the desktop and bundled applications one visual language.
use crate::render::Style;
use vt100::Color;

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
