pub struct Command {
    pub name: &'static str,
    pub label: &'static str,
    pub menu: usize,
    pub shortcut: &'static str,
}
pub const MENUS: &[&str] = &["System", "File", "Edit", "View", "Window", "Help"];
pub const COMMANDS: &[Command] = &[
    Command {
        name: "browser.open",
        label: "Browse files…",
        menu: 1,
        shortcut: "Alt+b",
    },
    Command {
        name: "editor.new",
        label: "New text document",
        menu: 1,
        shortcut: "Alt+e",
    },
    Command {
        name: "editor.open",
        label: "Open text file…",
        menu: 1,
        shortcut: "",
    },
    Command {
        name: "editor.save",
        label: "Save document",
        menu: 1,
        shortcut: "",
    },
    Command {
        name: "editor.save_as",
        label: "Save document as…",
        menu: 1,
        shortcut: "",
    },
    Command {
        name: "menu.open",
        label: "Open menus",
        menu: 5,
        shortcut: "F10",
    },
    Command {
        name: "window.new",
        label: "New shell",
        menu: 0,
        shortcut: "Alt+n",
    },
    Command {
        name: "session.detach",
        label: "Detach",
        menu: 0,
        shortcut: "Alt+d",
    },
    Command {
        name: "config.reload",
        label: "Reload configuration",
        menu: 0,
        shortcut: "",
    },
    Command {
        name: "session.end",
        label: "End session…",
        menu: 0,
        shortcut: "",
    },
    Command {
        name: "layout.save",
        label: "Save arrangement",
        menu: 1,
        shortcut: "",
    },
    Command {
        name: "window.close",
        label: "Close window",
        menu: 1,
        shortcut: "Alt+F4",
    },
    Command {
        name: "selection.start",
        label: "Select text",
        menu: 2,
        shortcut: "Alt+Space",
    },
    Command {
        name: "selection.copy",
        label: "Copy selection",
        menu: 2,
        shortcut: "Alt+c",
    },
    Command {
        name: "selection.paste",
        label: "Paste",
        menu: 2,
        shortcut: "Shift+Insert",
    },
    Command {
        name: "scroll.up",
        label: "Scroll up",
        menu: 2,
        shortcut: "Shift+PgUp",
    },
    Command {
        name: "scroll.down",
        label: "Scroll down",
        menu: 2,
        shortcut: "Shift+PgDn",
    },
    Command {
        name: "window.color",
        label: "Toggle program colours",
        menu: 3,
        shortcut: "",
    },
    Command {
        name: "workspace.next",
        label: "Next workspace",
        menu: 3,
        shortcut: "Alt+Right",
    },
    Command {
        name: "workspace.previous",
        label: "Previous workspace",
        menu: 3,
        shortcut: "Alt+Left",
    },
    Command {
        name: "window.workspace",
        label: "Move to next workspace",
        menu: 3,
        shortcut: "",
    },
    Command {
        name: "window.next",
        label: "Next window",
        menu: 4,
        shortcut: "F6",
    },
    Command {
        name: "window.list",
        label: "Window list",
        menu: 4,
        shortcut: "Alt+0",
    },
    Command {
        name: "window.move",
        label: "Move window",
        menu: 4,
        shortcut: "Alt+F5",
    },
    Command {
        name: "window.resize",
        label: "Resize window",
        menu: 4,
        shortcut: "Alt+F8",
    },
    Command {
        name: "window.zoom",
        label: "Zoom / restore",
        menu: 4,
        shortcut: "Alt+F10",
    },
    Command {
        name: "layout.tile",
        label: "Tile",
        menu: 4,
        shortcut: "F7",
    },
    Command {
        name: "layout.cascade",
        label: "Cascade",
        menu: 4,
        shortcut: "F8",
    },
    Command {
        name: "window.shelve",
        label: "Shelve window",
        menu: 4,
        shortcut: "Alt+F9",
    },
    Command {
        name: "layout.shelve",
        label: "Shelve all",
        menu: 4,
        shortcut: "F9",
    },
    Command {
        name: "help.show",
        label: "Help",
        menu: 5,
        shortcut: "F1",
    },
];

pub fn bound_command(key: &str, config: &crate::config::Config) -> Option<String> {
    if let Some(command) = config.keys.get(key) {
        return (!command.is_empty()).then(|| command.clone());
    }
    COMMANDS
        .iter()
        .find(|c| {
            c.shortcut == key && !key.is_empty() && !config.keys.values().any(|v| v == c.name)
        })
        .map(|c| c.name.to_string())
}
pub fn shortcut(command: &str, config: &crate::config::Config) -> String {
    config
        .keys
        .iter()
        .find(|(_, v)| *v == command)
        .map(|(k, _)| k.clone())
        .or_else(|| {
            COMMANDS
                .iter()
                .find(|c| c.name == command)
                .filter(|c| !config.keys.contains_key(c.shortcut))
                .map(|c| c.shortcut.into())
        })
        .unwrap_or_default()
}
