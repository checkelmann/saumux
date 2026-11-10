use crate::{
    builtin::{self, Prompt, Surface},
    render::Grid,
};
use anyhow::{Context, Result, bail};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const ACTIONS: &[(&str, &str)] = &[
    (" Up  ", "up"),
    (" Open  ", "open"),
    (" Refresh  ", "refresh"),
    (" Hidden  ", "hidden"),
    (" Location  ", "location"),
    (" Close ", "close"),
];
struct Entry {
    name: std::ffi::OsString,
    path: PathBuf,
    directory: bool,
    size: Option<u64>,
}
struct Browser {
    cwd: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    top: usize,
    hidden: bool,
    message: String,
    prompt: Option<Prompt>,
    last_click: Option<(usize, Instant)>,
}
impl Browser {
    fn new(path: &Path) -> Result<Self> {
        let cwd = fs::canonicalize(path).with_context(|| format!("opening {}", path.display()))?;
        let mut browser = Self {
            cwd,
            entries: vec![],
            selected: 0,
            top: 0,
            hidden: false,
            message: "Enter opens · Backspace up · Ctrl+L location · . hidden · Ctrl+Q close"
                .into(),
            prompt: None,
            last_click: None,
        };
        browser.refresh()?;
        Ok(browser)
    }
    fn refresh(&mut self) -> Result<()> {
        let selected = self.entries.get(self.selected).map(|e| e.path.clone());
        let mut entries = vec![];
        if let Some(parent) = self.cwd.parent() {
            entries.push(Entry {
                name: "..".into(),
                path: parent.into(),
                directory: true,
                size: None,
            });
        }
        let mut files = vec![];
        for entry in fs::read_dir(&self.cwd)? {
            let entry = entry?;
            let name = entry.file_name();
            if !self.hidden && name.to_string_lossy().starts_with('.') {
                continue;
            }
            let metadata = fs::metadata(entry.path()).ok();
            files.push(Entry {
                name,
                path: entry.path(),
                directory: metadata.as_ref().is_some_and(|m| m.is_dir()),
                size: metadata.map(|m| m.len()),
            });
        }
        files.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| {
                    a.name
                        .to_string_lossy()
                        .to_lowercase()
                        .cmp(&b.name.to_string_lossy().to_lowercase())
                })
                .then_with(|| a.name.cmp(&b.name))
        });
        entries.extend(files);
        self.selected = selected
            .and_then(|path| entries.iter().position(|e| e.path == path))
            .unwrap_or(self.selected)
            .min(entries.len().saturating_sub(1));
        self.entries = entries;
        self.top = self.top.min(self.selected);
        Ok(())
    }
    fn navigate(&mut self, path: &Path) -> Result<()> {
        let mut next = Self::new(path)?;
        next.hidden = self.hidden;
        next.refresh()?;
        *self = next;
        Ok(())
    }
    fn open(&mut self) -> Result<()> {
        let Some(entry) = self.entries.get(self.selected) else {
            return Ok(());
        };
        let path = entry.path.clone();
        if entry.directory {
            return self.navigate(&path);
        }
        // Report unsupported files in the browser rather than opening a window
        // which would disappear immediately on a decoding error.
        crate::editor::Document::open(Some(&path))?;
        let session =
            std::env::var("SAUMUX").context("the file browser runs inside a saumux session")?;
        let path = path.to_str().context("this file name is not UTF-8")?;
        crate::session::control(&session, "editor.open".into(), vec![path.into()])?;
        self.message = "Opened in a new editor window".into();
        Ok(())
    }
    fn up(&mut self) -> Result<()> {
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            let previous = self.cwd.clone();
            self.navigate(&parent)?;
            if let Some(index) = self.entries.iter().position(|e| e.path == previous) {
                self.selected = index;
            }
        }
        Ok(())
    }
    fn draw(&mut self, width: u16, height: u16) -> Grid {
        let visible = height.saturating_sub(4).max(1) as usize;
        if self.selected < self.top {
            self.top = self.selected;
        }
        if self.selected >= self.top + visible {
            self.top = self.selected + 1 - visible;
        }
        let theme = crate::theme::Theme::default();
        let mut grid = Grid::new(width, height);
        grid.fill(0, 0, width, height, " ", theme.text.clone());
        builtin::toolbar(&mut grid, 0, ACTIONS);
        grid.text(
            0,
            1,
            &self.cwd.display().to_string(),
            width,
            theme.text.clone(),
        );
        for (i, entry) in self
            .entries
            .iter()
            .skip(self.top)
            .take(height.saturating_sub(4) as usize)
            .enumerate()
        {
            let style = if self.top + i == self.selected {
                theme.title.clone()
            } else {
                theme.text.clone()
            };
            let y = i as u16 + 2;
            grid.fill(0, y, width, 1, " ", style.clone());
            let name = format!(
                "{} {}{}",
                if entry.directory { "D" } else { " " },
                entry.name.to_string_lossy(),
                if entry.directory { "/" } else { "" }
            );
            let size = entry.size.map(|s| s.to_string()).unwrap_or_default();
            let reserve = if width > 24 { 12 } else { 0 };
            grid.text(
                0,
                y,
                &name,
                width.saturating_sub(reserve),
                if self.top + i == self.selected {
                    style.clone()
                } else if entry.directory {
                    theme.directory.clone()
                } else {
                    style.clone()
                },
            );
            if reserve > 0 && !entry.directory {
                grid.text(
                    width.saturating_sub(size.len() as u16 + 1),
                    y,
                    &size,
                    size.len() as u16,
                    style,
                );
            }
        }
        if self.entries.is_empty() {
            grid.text(
                1,
                2,
                "Empty directory",
                width.saturating_sub(2),
                theme.text.clone(),
            );
        }
        builtin::bar(
            &mut grid,
            height.saturating_sub(2),
            &format!(
                " {} entries · hidden {} · Enter / double-click to open",
                self.entries.len(),
                if self.hidden { "shown" } else { "off" }
            ),
        );
        builtin::bar(&mut grid, height.saturating_sub(1), &self.message);
        if let Some(prompt) = &self.prompt {
            builtin::toolbar(
                &mut grid,
                height.saturating_sub(2),
                &[
                    (" [Open] ", ""),
                    (" [Cancel] ", ""),
                    (" Ctrl+U clears the field", ""),
                ],
            );
            prompt.draw(&mut grid);
        }
        grid
    }
    fn action(&mut self, action: &str) -> Result<bool> {
        match action {
            "up" => self.up()?,
            "open" => self.open()?,
            "refresh" => self.refresh()?,
            "hidden" => {
                self.hidden = !self.hidden;
                self.refresh()?;
            }
            "location" => {
                self.prompt = Some(Prompt::new(
                    "Directory (Enter / Esc)",
                    self.cwd.display().to_string(),
                ))
            }
            "close" => return Ok(true),
            _ => {}
        }
        Ok(false)
    }
}
pub fn run(path: &Path) -> Result<()> {
    let mut browser = Browser::new(path)?;
    let mut surface = Surface::enter()?;
    loop {
        let (width, height) = surface.size()?;
        let grid = browser.draw(width, height);
        surface.draw(grid, &format!("Files — {}", browser.cwd.display()))?;
        let Some(event) = surface.event()? else {
            continue;
        };
        let result = (|| -> Result<bool> {
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if let Some(mut prompt) = browser.prompt.take() {
                        match key.code {
                            KeyCode::Esc => {}
                            KeyCode::Enter => {
                                let path = PathBuf::from(&prompt.text);
                                let path = if path.is_absolute() {
                                    path
                                } else {
                                    browser.cwd.join(path)
                                };
                                browser.navigate(&path)?;
                            }
                            _ => {
                                prompt.edit(key);
                                browser.prompt = Some(prompt);
                            }
                        }
                        return Ok(false);
                    }
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('q') => return Ok(true),
                            KeyCode::Char('l') => {
                                browser.action("location")?;
                            }
                            KeyCode::Char('r') => {
                                browser.refresh()?;
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Up => browser.selected = browser.selected.saturating_sub(1),
                            KeyCode::Down => {
                                browser.selected = (browser.selected + 1)
                                    .min(browser.entries.len().saturating_sub(1))
                            }
                            KeyCode::PageUp => {
                                browser.selected = browser
                                    .selected
                                    .saturating_sub(height.saturating_sub(4) as usize)
                            }
                            KeyCode::PageDown => {
                                browser.selected = (browser.selected
                                    + height.saturating_sub(4) as usize)
                                    .min(browser.entries.len().saturating_sub(1))
                            }
                            KeyCode::Home => browser.selected = 0,
                            KeyCode::End => {
                                browser.selected = browser.entries.len().saturating_sub(1)
                            }
                            KeyCode::Enter => browser.open()?,
                            KeyCode::Backspace => browser.up()?,
                            KeyCode::Char('.') => {
                                browser.action("hidden")?;
                            }
                            _ => {}
                        }
                    }
                }
                Event::Paste(text) => {
                    if let Some(prompt) = browser.prompt.as_mut() {
                        prompt.text.push_str(&text.replace(['\r', '\n'], ""));
                        prompt.cursor = prompt.text.chars().count();
                    }
                }
                Event::Mouse(mouse)
                    if browser.prompt.is_some()
                        && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && mouse.row == height.saturating_sub(2) =>
                {
                    if mouse.column < 8 {
                        let prompt = browser.prompt.take().unwrap();
                        let path = PathBuf::from(prompt.text);
                        browser.navigate(&if path.is_absolute() {
                            path
                        } else {
                            browser.cwd.join(path)
                        })?;
                    } else if mouse.column < 18 {
                        browser.prompt = None;
                    }
                }
                Event::Mouse(mouse) if browser.prompt.is_none() => match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if mouse.row == 0 => {
                        let Some(action) = builtin::action_at(mouse.column, ACTIONS) else {
                            return Ok(false);
                        };
                        return browser.action(action);
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if mouse.row >= 2 && mouse.row < height.saturating_sub(2) =>
                    {
                        let index = browser.top + mouse.row as usize - 2;
                        if index < browser.entries.len() {
                            browser.selected = index;
                            if browser.last_click.is_some_and(|(previous, time)| {
                                previous == index && time.elapsed() < Duration::from_millis(400)
                            }) {
                                browser.last_click = None;
                                browser.open()?;
                            } else {
                                browser.last_click = Some((index, Instant::now()));
                            }
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        browser.selected = browser.selected.saturating_sub(3)
                    }
                    MouseEventKind::ScrollDown => {
                        browser.selected =
                            (browser.selected + 3).min(browser.entries.len().saturating_sub(1))
                    }
                    _ => {}
                },
                _ => {}
            }
            Ok(false)
        })();
        match result {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => browser.message = e.to_string(),
        }
    }
    Ok(())
}

pub fn launch_args(kind: &str, args: &[String]) -> Result<Vec<String>> {
    let mut command = vec![
        std::env::current_exe()?.to_string_lossy().into_owned(),
        kind.into(),
    ];
    if args.len() > 1 {
        bail!("expected at most one path");
    }
    command.extend_from_slice(args);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directories_first_hidden_toggle_and_navigation() {
        let root = std::env::temp_dir().join(format!("sx-browser-{}", std::process::id()));
        fs::create_dir_all(root.join("folder")).unwrap();
        fs::write(root.join("a.txt"), b"text").unwrap();
        fs::write(root.join(".hidden"), b"secret").unwrap();
        let mut b = Browser::new(&root).unwrap();
        assert_eq!(b.entries[1].name, "folder");
        assert!(!b.entries.iter().any(|e| e.name == ".hidden"));
        b.action("hidden").unwrap();
        assert!(b.entries.iter().any(|e| e.name == ".hidden"));
        b.navigate(&root.join("folder")).unwrap();
        b.up().unwrap();
        assert_eq!(b.cwd, fs::canonicalize(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
