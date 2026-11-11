//! Markdown notebook. Notes stay ordinary UTF-8 files and open in the protected
//! bundled editor, while this window provides a quick notebook and preview.
use crate::{
    builtin::{self, Surface},
    render::Grid,
};
use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::{fs, path::PathBuf};

const ACTIONS: &[(&str, &str)] = &[
    (" Open ", "open"),
    (" New ", "new"),
    (" Search ", "search"),
    (" Refresh ", "refresh"),
    (" Close ", "close"),
];

fn root() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(p) if !p.is_empty() && PathBuf::from(&p).is_absolute() => PathBuf::from(p),
        _ => PathBuf::from(std::env::var_os("HOME").context("HOME is required for Notes")?)
            .join(".local/share"),
    };
    let dir = base.join("saumux");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    Ok(dir)
}

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

fn files(dir: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut out = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name().is_some_and(|n| {
                    n == "notes.txt" || p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md"))
                })
        })
        .collect::<Vec<_>>();
    out.sort_by_key(|p| {
        (
            p.file_name().is_some_and(|n| n != "notes.txt"),
            p.file_name()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
        )
    });
    Ok(out)
}

struct Notebook {
    dir: PathBuf,
    notes: Vec<PathBuf>,
    selected: usize,
    top: usize,
    preview_top: usize,
    filter: Option<String>,
    message: String,
}
impl Notebook {
    fn new() -> Result<Self> {
        let dir = root()?;
        let mut this = Self {
            dir,
            notes: vec![],
            selected: 0,
            top: 0,
            preview_top: 0,
            filter: None,
            message: "Enter open · N new Markdown note · / search".into(),
        };
        this.refresh()?;
        Ok(this)
    }
    fn refresh(&mut self) -> Result<()> {
        self.notes = files(&self.dir)?;
        if let Some(q) = &self.filter {
            self.notes.retain(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&q.to_lowercase())
                    || fs::read_to_string(p)
                        .is_ok_and(|s| s.to_lowercase().contains(&q.to_lowercase()))
            });
        }
        self.selected = self.selected.min(self.notes.len().saturating_sub(1));
        self.preview_top = 0;
        Ok(())
    }
    fn selected_path(&self) -> Option<&PathBuf> {
        self.notes.get(self.selected)
    }
    fn open(&mut self) -> Result<()> {
        let Some(path) = self.selected_path() else {
            self.message = "Create a note first".into();
            return Ok(());
        };
        let session = std::env::var("SAUMUX").context("Notes runs inside a saumux session")?;
        crate::session::control(
            &session,
            "editor.open".into(),
            vec![path.to_str().context("note path is not UTF-8")?.into()],
        )?;
        self.message = "Opened in editor · changes appear in preview after Refresh".into();
        Ok(())
    }
    fn create(&mut self, name: &str) -> Result<()> {
        let clean: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .take(64)
            .collect();
        let stem = clean.trim().trim_matches(['.', ' ']);
        if stem.is_empty() {
            self.message = "Choose a note title".into();
            return Ok(());
        }
        let mut path = self.dir.join(format!("{stem}.md"));
        let mut n = 2;
        while path.exists() {
            path = self.dir.join(format!("{stem} {n}.md"));
            n += 1;
        }
        let title = path.file_stem().unwrap().to_string_lossy();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        use std::io::Write;
        writeln!(file, "# {title}\n")?;
        self.filter = None;
        self.refresh()?;
        self.selected = self.notes.iter().position(|p| p == &path).unwrap_or(0);
        self.message = "Markdown note created".into();
        self.open()
    }
    fn draw(&mut self, width: u16, height: u16) -> Grid {
        let theme = crate::theme::current();
        let mut grid = Grid::new(width, height);
        grid.fill(0, 0, width, height, " ", theme.editor.clone());
        builtin::toolbar(&mut grid, 0, ACTIONS);
        let split = width.saturating_sub(12).clamp(1, 32);
        grid.fill(
            split,
            1,
            1,
            height.saturating_sub(3),
            "│",
            theme.status.clone(),
        );
        let list_rows = height.saturating_sub(4) as usize;
        if self.selected < self.top {
            self.top = self.selected;
        }
        if self.selected >= self.top + list_rows.max(1) {
            self.top = self.selected + 1 - list_rows.max(1);
        }
        grid.text(
            1,
            1,
            &format!("NOTES · {}", self.notes.len()),
            split.saturating_sub(2),
            theme.monitor_accent.clone(),
        );
        for (i, path) in self.notes.iter().skip(self.top).take(list_rows).enumerate() {
            let y = i as u16 + 2;
            let title = path.file_stem().unwrap_or_default().to_string_lossy();
            let style = if self.top + i == self.selected {
                theme.selected.clone()
            } else {
                theme.editor.clone()
            };
            grid.fill(1, y, split.saturating_sub(2), 1, " ", style.clone());
            grid.text(
                1,
                y,
                &format!(
                    "{} {}",
                    if self.top + i == self.selected {
                        "›"
                    } else {
                        " "
                    },
                    title
                ),
                split.saturating_sub(2),
                style,
            );
        }
        if let Some(path) = self.selected_path() {
            let text =
                fs::read_to_string(path).unwrap_or_else(|e| format!("Could not read note: {e}"));
            markdown_preview(
                &mut grid,
                split + 2,
                1,
                width.saturating_sub(split + 3),
                height.saturating_sub(3),
                &text,
                self.preview_top,
            );
            grid.text(
                split + 2,
                height.saturating_sub(2),
                &path.file_name().unwrap_or_default().to_string_lossy(),
                width.saturating_sub(split + 3),
                theme.monitor_muted.clone(),
            );
        } else {
            grid.text(
                split + 3,
                3,
                "Your notes live here",
                width.saturating_sub(split + 4),
                theme.monitor_accent.clone(),
            );
            grid.text(
                split + 3,
                5,
                "Create a Markdown note with N, then edit it in saumux.",
                width.saturating_sub(split + 4),
                theme.monitor_muted.clone(),
            );
        }
        builtin::bar(&mut grid, height.saturating_sub(1), &self.message);
        grid
    }
}

fn markdown_preview(
    grid: &mut Grid,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    text: &str,
    top: usize,
) {
    let theme = crate::theme::current();
    for (row, line) in text.lines().skip(top).enumerate() {
        if row >= height as usize {
            break;
        }
        let (rendered, style) = if let Some(h) = line.strip_prefix("### ") {
            (h.to_string(), theme.hotkey.clone())
        } else if let Some(h) = line.strip_prefix("## ") {
            (h.to_string(), theme.monitor_accent.clone())
        } else if let Some(h) = line.strip_prefix("# ") {
            (h.to_string(), theme.title.clone())
        } else if let Some(s) = line.strip_prefix("- [ ] ") {
            (format!("☐ {s}"), theme.warning.clone())
        } else if let Some(s) = line
            .strip_prefix("- [x] ")
            .or_else(|| line.strip_prefix("- [X] "))
        {
            (format!("☑ {s}"), theme.hotkey.clone())
        } else if let Some(s) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            (format!("• {s}"), theme.editor.clone())
        } else if let Some(s) = line.strip_prefix("> ") {
            (s.to_string(), theme.monitor_muted.clone())
        } else {
            (line.to_string(), theme.editor.clone())
        };
        grid.text(x, y + row as u16, &rendered, width, style);
    }
}

pub fn run() -> Result<()> {
    let mut notebook = Notebook::new()?;
    let mut surface = Surface::enter()?;
    let mut prompt: Option<(String, String)> = None;
    loop {
        let (w, h) = surface.size()?;
        let title = notebook
            .selected_path()
            .and_then(|p| p.file_name())
            .map(|p| format!("{} — Notes", p.to_string_lossy()))
            .unwrap_or_else(|| "Notes — saumux".into());
        surface.draw(notebook.draw(w, h), &title)?;
        let Some(event) = surface.event()? else {
            continue;
        };
        match event {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                if let Some((kind, mut value)) = prompt.take() {
                    match k.code {
                        KeyCode::Esc => {
                            notebook.message = "Cancelled".into();
                            continue;
                        }
                        KeyCode::Enter => {
                            if kind == "new" {
                                notebook.create(&value)?;
                            } else {
                                notebook.filter = (!value.is_empty()).then_some(value);
                                notebook.refresh()?;
                            }
                            continue;
                        }
                        KeyCode::Backspace => {
                            value.pop();
                        }
                        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                            value.push(c)
                        }
                        _ => {}
                    }
                    notebook.message = format!(
                        "{}: {}_  · Enter confirm · Esc cancel",
                        if kind == "new" { "New note" } else { "Search" },
                        value
                    );
                    prompt = Some((kind, value));
                    continue;
                }
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q')
                        if k.code == KeyCode::Esc
                            || k.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        break;
                    }
                    KeyCode::Up => notebook.selected = notebook.selected.saturating_sub(1),
                    KeyCode::Down => {
                        notebook.selected =
                            (notebook.selected + 1).min(notebook.notes.len().saturating_sub(1))
                    }
                    KeyCode::PageUp => {
                        notebook.preview_top = notebook.preview_top.saturating_sub(10)
                    }
                    KeyCode::PageDown => {
                        notebook.preview_top = notebook.preview_top.saturating_add(10)
                    }
                    KeyCode::Home => notebook.preview_top = 0,
                    KeyCode::Enter => notebook.open()?,
                    KeyCode::Char('n' | 'N') => prompt = Some(("new".into(), String::new())),
                    KeyCode::Char('/') | KeyCode::Char('f')
                        if !k.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        prompt =
                            Some(("search".into(), notebook.filter.clone().unwrap_or_default()))
                    }
                    KeyCode::Char('r' | 'R') => notebook.refresh()?,
                    KeyCode::Char('c' | 'C') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        break;
                    }
                    _ => {}
                }
            }
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) if m.row == 0 => {
                    if let Some(a) = builtin::action_at(m.column, ACTIONS) {
                        match a {
                            "open" => notebook.open()?,
                            "new" => prompt = Some(("new".into(), String::new())),
                            "search" => prompt = Some(("search".into(), String::new())),
                            "refresh" => notebook.refresh()?,
                            "close" => break,
                            _ => {}
                        }
                    }
                }
                MouseEventKind::Down(MouseButton::Left) if m.column < 32 && m.row > 1 => {
                    notebook.selected = (notebook.top + m.row as usize - 2)
                        .min(notebook.notes.len().saturating_sub(1))
                }
                MouseEventKind::ScrollUp if m.column < 32 => {
                    notebook.selected = notebook.selected.saturating_sub(3)
                }
                MouseEventKind::ScrollDown if m.column < 32 => {
                    notebook.selected =
                        (notebook.selected + 3).min(notebook.notes.len().saturating_sub(1))
                }
                MouseEventKind::ScrollUp => {
                    notebook.preview_top = notebook.preview_top.saturating_sub(3)
                }
                MouseEventKind::ScrollDown => {
                    notebook.preview_top = notebook.preview_top.saturating_add(3)
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}
