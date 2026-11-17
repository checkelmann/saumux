//! Keyboard and mouse file chooser for Save As; filesystem writes stay in Document.
use crate::{
    builtin::{self, Prompt},
    render::Grid,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Entry {
    name: String,
    directory: bool,
}
pub struct SaveDialog {
    directory: PathBuf,
    location: Prompt,
    filename: Prompt,
    entries: Vec<Entry>,
    selected: usize,
    top: usize,
    focus: usize,
    error: String,
}
pub enum Outcome {
    Save(PathBuf),
    Cancel,
}
const ACTIONS: &[(&str, &str)] = &[
    (" [Up folder]  ", "up"),
    ("[Save]  ", "save"),
    ("[Cancel]", "cancel"),
];
impl SaveDialog {
    pub fn new(path: Option<&Path>) -> Self {
        let directory = path
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        let filename = path
            .and_then(Path::file_name)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut dialog = Self {
            location: Prompt::new("Folder", directory.display().to_string()),
            directory,
            filename: Prompt::new("Filename", filename),
            entries: vec![],
            selected: 0,
            top: 0,
            focus: 2,
            error: String::new(),
        };
        dialog.refresh();
        dialog
    }
    pub fn set_error(&mut self, error: String) {
        self.error = error;
    }
    fn refresh(&mut self) {
        match fs::read_dir(&self.directory) {
            Ok(entries) => {
                self.entries = entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        Some(Entry {
                            name: entry.file_name().into_string().ok()?,
                            directory: entry.path().is_dir(),
                        })
                    })
                    .collect();
                self.entries
                    .sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
                self.selected = 0;
                self.top = 0;
                self.error.clear();
                self.location = Prompt::new("Folder", self.directory.display().to_string());
            }
            Err(e) => self.error = e.to_string(),
        }
    }
    fn enter_directory(&mut self, path: PathBuf) {
        match fs::canonicalize(&path).and_then(|p| {
            fs::read_dir(&p)?;
            Ok(p)
        }) {
            Ok(path) => {
                self.directory = path;
                self.refresh();
            }
            Err(e) => self.error = e.to_string(),
        }
    }
    fn activate(&mut self) {
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.directory {
                self.enter_directory(self.directory.join(&entry.name));
            } else {
                self.filename = Prompt::new("Filename", entry.name.clone());
                self.focus = 2;
            }
        }
    }
    fn save(&mut self) -> Option<Outcome> {
        if self.filename.text.is_empty() {
            self.error = "Enter a filename".into();
            self.focus = 2;
            return None;
        }
        let path = self.directory.join(&self.filename.text);
        if path.is_dir() {
            self.enter_directory(path);
            self.filename = Prompt::new("Filename", String::new());
            None
        } else {
            Some(Outcome::Save(path))
        }
    }
    fn geometry(width: u16, height: u16) -> (u16, u16, u16, u16) {
        let w = width.min(78);
        let h = height.min(24);
        ((width - w) / 2, (height - h) / 2, w, h)
    }
    pub fn event(&mut self, event: Event, width: u16, height: u16) -> Option<Outcome> {
        match event {
            Event::Key(key) if key.kind != crossterm::event::KeyEventKind::Release => {
                match key.code {
                    KeyCode::Esc => return Some(Outcome::Cancel),
                    KeyCode::Tab | KeyCode::BackTab => {
                        self.focus = (self.focus
                            + if key.code == KeyCode::BackTab
                                || key.modifiers.contains(KeyModifiers::SHIFT)
                            {
                                4
                            } else {
                                1
                            })
                            % 5;
                    }
                    KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                        self.focus = 1;
                        let step = if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                            10
                        } else {
                            1
                        };
                        self.selected = self
                            .selected
                            .saturating_add_signed(
                                if matches!(key.code, KeyCode::Up | KeyCode::PageUp) {
                                    -step
                                } else {
                                    step
                                },
                            )
                            .min(self.entries.len().saturating_sub(1));
                    }
                    KeyCode::Enter => match self.focus {
                        0 => self.enter_directory(self.directory.join(&self.location.text)),
                        1 => self.activate(),
                        4 => return Some(Outcome::Cancel),
                        _ => return self.save(),
                    },
                    KeyCode::Backspace if self.focus == 1 => {
                        if let Some(p) = self.directory.parent() {
                            self.enter_directory(p.to_path_buf());
                        }
                    }
                    _ => {
                        if self.focus == 0 {
                            self.location.edit(key);
                        } else if self.focus == 2 {
                            self.filename.edit(key);
                        }
                    }
                }
            }
            Event::Paste(text) => {
                let prompt = if self.focus == 0 {
                    &mut self.location
                } else if self.focus == 2 {
                    &mut self.filename
                } else {
                    return None;
                };
                for c in text.chars().filter(|c| !c.is_control()) {
                    prompt.edit(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
                }
            }
            Event::Mouse(mouse) => {
                let (x, y, w, h) = Self::geometry(width, height);
                if mouse.column < x || mouse.column >= x + w || mouse.row < y || mouse.row >= y + h
                {
                    return None;
                }
                let col = mouse.column - x;
                let row = mouse.row - y;
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if row == 1 && col > 0 => {
                        match builtin::action_at(col - 1, ACTIONS) {
                            Some("up") => {
                                if let Some(p) = self.directory.parent() {
                                    self.enter_directory(p.to_path_buf());
                                }
                            }
                            Some("save") => return self.save(),
                            Some("cancel") => return Some(Outcome::Cancel),
                            _ => {}
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) if row == 2 => {
                        self.focus = 0;
                        Self::place_cursor(&mut self.location, col, w);
                    }
                    MouseEventKind::Down(MouseButton::Left) if row == h.saturating_sub(3) => {
                        self.focus = 2;
                        Self::place_cursor(&mut self.filename, col, w);
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if row >= 4 && row < h.saturating_sub(4) =>
                    {
                        let selected = self.top + (row - 4) as usize;
                        if selected < self.entries.len() {
                            let activate = self.focus == 1 && self.selected == selected;
                            self.selected = selected;
                            self.focus = 1;
                            if activate {
                                self.activate();
                            } else if !self.entries[selected].directory {
                                self.filename =
                                    Prompt::new("Filename", self.entries[selected].name.clone());
                            }
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        self.selected =
                            (self.selected + 3).min(self.entries.len().saturating_sub(1))
                    }
                    MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(3),
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }
    pub fn draw(&mut self, grid: &mut Grid) {
        let (x, y, w, h) = Self::geometry(grid.cols, grid.rows);
        let theme = crate::theme::current().file_dialog();
        let mut panel = Grid::new(w, h);
        panel.fill(0, 0, w, h, " ", theme.body.clone());
        let inner = w.saturating_sub(2);
        if w >= 2 && h >= 2 {
            for row in 0..h {
                panel.text(0, row, "│", 1, theme.border.clone());
                panel.text(w - 1, row, "│", 1, theme.border.clone());
            }
            for row in [0, h - 1] {
                panel.fill(1, row, inner, 1, "─", theme.border.clone());
            }
            for (cx, cy, glyph) in [
                (0, 0, "┌"),
                (w - 1, 0, "┐"),
                (0, h - 1, "└"),
                (w - 1, h - 1, "┘"),
            ] {
                panel.text(cx, cy, glyph, 1, theme.border.clone());
            }
        }
        panel.text(2, 0, " Save As ", w.saturating_sub(4), theme.title.clone());
        let mut offset = 1;
        for (index, (label, _)) in ACTIONS.iter().enumerate() {
            let focused = (index == 1 && self.focus == 3) || (index == 2 && self.focus == 4);
            panel.text(
                offset,
                1,
                label,
                w.saturating_sub(offset + 1),
                if focused {
                    theme.active.clone()
                } else {
                    theme.button.clone()
                },
            );
            offset += label.len() as u16;
        }
        if h >= 9 {
            let rows = h.saturating_sub(8) as usize;
            if self.selected < self.top {
                self.top = self.selected;
            }
            if self.selected >= self.top + rows {
                self.top = self.selected + 1 - rows;
            }
            panel.fill(1, 3, inner, 1, "─", theme.border.clone());
            panel.text(
                2,
                3,
                " Files and folders ",
                w.saturating_sub(4),
                theme.body.clone(),
            );
            for (i, entry) in self.entries.iter().enumerate().skip(self.top).take(rows) {
                let selected = self.focus == 1 && i == self.selected;
                let row = 4 + (i - self.top) as u16;
                let style = if selected {
                    &theme.active
                } else if entry.directory {
                    &theme.directory
                } else {
                    &theme.body
                };
                panel.fill(1, row, inner, 1, " ", style.clone());
                panel.text(
                    1,
                    row,
                    &format!(
                        "{} {}{}",
                        if selected { ">" } else { " " },
                        entry.name,
                        if entry.directory { "/" } else { "" }
                    ),
                    inner,
                    style.clone(),
                );
            }
            panel.fill(1, h - 4, inner, 1, "─", theme.border.clone());
            self.field(&mut panel, 2, &self.location, 0);
            self.field(&mut panel, h - 3, &self.filename, 2);
            panel.text(
                1,
                h - 2,
                " Tab: next field · Enter: open/save · Esc: cancel",
                inner,
                theme.body.clone(),
            );
            if !self.error.is_empty() {
                panel.text(
                    1,
                    h - 1,
                    &self.error,
                    inner,
                    crate::theme::current().warning,
                );
            }
        } else {
            self.field(&mut panel, h.saturating_sub(1), &self.filename, 2);
        }
        grid.cursor = panel.cursor.map(|(cx, cy)| (x + cx, y + cy));
        grid.blit(&panel, x, y);
    }
    fn place_cursor(prompt: &mut Prompt, col: u16, width: u16) {
        let start = (prompt.label.len() as u16 + 3).min(width.saturating_sub(1));
        let text: Vec<_> = prompt.text.chars().collect();
        let left = builtin::column(&text, prompt.cursor)
            .saturating_sub(width.saturating_sub(start).saturating_sub(2) as usize);
        prompt.cursor = builtin::index_at(&text, left + col.saturating_sub(start) as usize);
    }
    fn field(&self, grid: &mut Grid, y: u16, prompt: &Prompt, focus: usize) {
        let theme = crate::theme::current().file_dialog();
        let label = format!("{}: ", prompt.label);
        let end = grid.cols.saturating_sub(1);
        let start = (label.len() as u16 + 1).min(end);
        let mut label_style = theme.body;
        label_style.bold = self.focus == focus;
        grid.text(1, y, &label, grid.cols.saturating_sub(2), label_style);
        let width = end.saturating_sub(start);
        let style = if self.focus == focus {
            theme.active
        } else {
            theme.field
        };
        grid.fill(start, y, width, 1, " ", style.clone());
        let text: Vec<_> = prompt.text.chars().collect();
        let cursor = builtin::column(&text, prompt.cursor);
        let left = cursor.saturating_sub(width.saturating_sub(1) as usize);
        builtin::styled_line_at(grid, start, y, &text, left, width, |_| style.clone());
        if self.focus == focus && width > 0 {
            grid.cursor = Some((start + (cursor - left) as u16, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(dialog: &mut SaveDialog, code: KeyCode) -> Option<Outcome> {
        dialog.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)), 100, 30)
    }
    #[test]
    fn browse_choose_paste_cancel_and_tiny_rendering() {
        let root = std::env::temp_dir().join(format!("saumux-chooser-{}", std::process::id()));
        fs::create_dir_all(root.join("folder")).unwrap();
        fs::write(root.join("folder/Grüße.rs"), "original").unwrap();
        let mut d = SaveDialog::new(Some(&root.join("untitled.rs")));
        // BackTab enters the directory list; Enter opens the first folder.
        key(&mut d, KeyCode::BackTab);
        key(&mut d, KeyCode::Enter);
        assert_eq!(d.directory, fs::canonicalize(root.join("folder")).unwrap());
        key(&mut d, KeyCode::Enter);
        assert_eq!(d.filename.text, "Grüße.rs");
        let Some(Outcome::Save(path)) = key(&mut d, KeyCode::Enter) else {
            panic!("expected chosen path")
        };
        assert_eq!(path, d.directory.join("Grüße.rs"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        for (w, h) in [(0, 0), (1, 1), (12, 5), (60, 16), (100, 30)] {
            let mut grid = Grid::new(w, h);
            d.draw(&mut grid);
            assert!(grid.cursor.is_none_or(|(x, y)| x < w && y < h));
        }
        d.event(Event::Paste("界\n".into()), 100, 30);
        assert_eq!(d.filename.text, "Grüße.rs界");
        assert!(matches!(key(&mut d, KeyCode::Esc), Some(Outcome::Cancel)));
        d.enter_directory(root.join("missing"));
        assert!(!d.error.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn mouse_save_button_returns_target() {
        let mut d = SaveDialog::new(None);
        d.filename = Prompt::new("Filename", "sample.rs".into());
        let event = Event::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 27,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        // Centred 78x24 dialog at (11,3); Save starts at dialog column 14.
        assert!(matches!(d.event(event, 100, 30), Some(Outcome::Save(_))));
    }
}
