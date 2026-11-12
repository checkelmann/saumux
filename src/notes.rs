//! Markdown notebook. Notes stay ordinary UTF-8 files and open in the protected
//! bundled editor, while this window provides a quick notebook and preview.
use crate::{
    builtin::{self, Surface},
    editor::Editor,
    render::Grid,
};
use anyhow::{Context, Result};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use std::{fs, path::PathBuf};

const ACTIONS: &[(&str, &str)] = &[
    (" Edit ", "edit"),
    (" New ", "new"),
    (" Search ", "search"),
    (" Delete ", "delete"),
    (" Refresh ", "refresh"),
    (" Close ", "close"),
];
const DELETE_ACTIONS: &[(&str, &str)] =
    &[(" Delete permanently ", "confirm"), (" Cancel ", "cancel")];
const EDIT_ACTIONS: &[(&str, &str)] = &[
    (" Save ", "save"),
    (" Undo ", "undo"),
    (" Redo ", "redo"),
    (" Preview ", "preview"),
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
    editor: Option<Editor>,
    editing: bool,
    leave_prompt: bool,
    close_after_leave: bool,
    delete_prompt: Option<PathBuf>,
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
            message: "Enter / E edit · N new · / search · Del delete".into(),
            editor: None,
            editing: false,
            leave_prompt: false,
            close_after_leave: false,
            delete_prompt: None,
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
    fn request_delete(&mut self) {
        if self.editing {
            return;
        }
        if let Some(path) = self.selected_path().cloned() {
            self.message = format!(
                "Delete {}? · Enter delete permanently · Esc cancel",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            self.delete_prompt = Some(path);
        } else {
            self.message = "No note selected".into();
        }
    }
    fn cancel_delete(&mut self) {
        self.delete_prompt = None;
        self.message = "Deletion cancelled".into();
    }
    fn confirm_delete(&mut self) {
        let Some(path) = self.delete_prompt.take() else {
            return;
        };
        match fs::remove_file(&path) {
            Ok(()) => {
                self.message = match self.refresh() {
                    Ok(()) => "Note deleted".into(),
                    Err(error) => format!("Note deleted; refresh failed: {error:#}"),
                };
            }
            Err(error) => self.message = format!("Delete failed: {error}"),
        }
    }
    fn begin_edit(&mut self) -> Result<()> {
        let Some(path) = self.selected_path().cloned() else {
            self.message = "Create a note first".into();
            return Ok(());
        };
        let doc = match crate::editor::Document::open(Some(&path)) {
            Ok(doc) => doc,
            Err(error) => {
                self.message = format!("Cannot edit note: {error:#}");
                return Ok(());
            }
        };
        self.editor = Some(Editor::new(doc));
        self.editing = true;
        self.leave_prompt = false;
        self.message = "Markdown edit · Ctrl+S saves · Ctrl+Z undo · Esc returns to preview".into();
        Ok(())
    }
    fn request_leave(&mut self) {
        self.close_after_leave = false;
        if self.editor.as_ref().is_some_and(Editor::inline_dirty) {
            self.leave_prompt = true;
            self.message = "Unsaved note · S save · D discard · Esc keep editing".into();
        } else {
            self.leave_edit();
        }
    }
    fn leave_edit(&mut self) {
        self.editor = None;
        self.editing = false;
        self.leave_prompt = false;
        self.message = "Preview · Enter or E edits · click the note to edit".into();
    }
    fn save_inline(&mut self, leave: bool) {
        let result = self
            .editor
            .as_mut()
            .context("no note is being edited")
            .and_then(Editor::inline_save);
        match result {
            Ok(()) => {
                let _ = self.refresh();
                self.message = "Saved".into();
                if leave {
                    self.leave_edit();
                }
            }
            Err(error) => self.message = format!("Save failed: {error:#}"),
        }
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
        self.begin_edit()
    }
    fn draw(&mut self, width: u16, height: u16) -> Grid {
        let theme = crate::theme::current();
        let mut grid = Grid::new(width, height);
        grid.fill(0, 0, width, height, " ", theme.editor.clone());
        builtin::toolbar(
            &mut grid,
            0,
            if self.delete_prompt.is_some() {
                DELETE_ACTIONS
            } else if self.editing {
                EDIT_ACTIONS
            } else {
                ACTIONS
            },
        );
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
        if let Some(path) = self.selected_path().cloned() {
            let pane_x = split + 2;
            let pane_width = width.saturating_sub(pane_x + 1);
            let pane_height = height.saturating_sub(3);
            if self.editing {
                if let Some(editor) = &mut self.editor {
                    grid.fill(
                        pane_x,
                        1,
                        pane_width,
                        pane_height,
                        " ",
                        theme.editor.clone(),
                    );
                    editor.draw_inline(&mut grid, pane_x, 1, pane_width, pane_height);
                }
            } else {
                let text = fs::read_to_string(&path)
                    .unwrap_or_else(|e| format!("Could not read note: {e}"));
                crate::notes_markdown::draw(
                    &mut grid,
                    pane_x,
                    1,
                    pane_width,
                    pane_height,
                    &text,
                    self.preview_top,
                );
            }
            grid.text(
                pane_x,
                height.saturating_sub(2),
                &path.file_name().unwrap_or_default().to_string_lossy(),
                pane_width,
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
        // Freeze selection and editing while confirming the captured file path.
        if notebook.delete_prompt.is_some() {
            match event {
                Event::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                    KeyCode::Enter => notebook.confirm_delete(),
                    KeyCode::Esc => notebook.cancel_delete(),
                    KeyCode::Char('q' | 'c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        break;
                    }
                    _ => {}
                },
                Event::Mouse(m)
                    if m.kind == MouseEventKind::Down(MouseButton::Left) && m.row == 0 =>
                {
                    match builtin::action_at(m.column, DELETE_ACTIONS) {
                        Some("confirm") => notebook.confirm_delete(),
                        Some("cancel") => notebook.cancel_delete(),
                        _ => {}
                    }
                }
                _ => {}
            }
            continue;
        }
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
                if notebook.leave_prompt {
                    match k.code {
                        KeyCode::Esc => {
                            notebook.leave_prompt = false;
                            notebook.close_after_leave = false;
                            notebook.message = "Keep editing · Ctrl+S saves".into();
                        }
                        KeyCode::Char('s' | 'S') => {
                            notebook.save_inline(true);
                            if notebook.close_after_leave && !notebook.leave_prompt {
                                break;
                            }
                        }
                        KeyCode::Char('d' | 'D') => notebook.leave_edit(),
                        _ => {}
                    }
                    if notebook.close_after_leave && !notebook.leave_prompt {
                        break;
                    }
                    continue;
                }
                if notebook.editing {
                    match k.code {
                        KeyCode::Esc => notebook.request_leave(),
                        KeyCode::Char('q') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            if notebook.editor.as_ref().is_some_and(Editor::inline_dirty) {
                                notebook.request_leave();
                                notebook.close_after_leave = true;
                                notebook.message =
                                    "Close Notes? · S save · D discard · Esc cancel".into();
                            } else {
                                break;
                            }
                        }
                        KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            notebook.save_inline(false);
                        }
                        KeyCode::Char('f' | 'o') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            notebook.message =
                                "Use / in preview to search notes · each note keeps its own file"
                                    .into();
                        }
                        _ => {
                            if let Some(editor) = &mut notebook.editor
                                && let Err(error) = editor.inline_key(k)
                            {
                                notebook.message = format!("Edit error: {error:#}");
                            }
                        }
                    }
                    continue;
                }
                match k.code {
                    KeyCode::Esc => {
                        break;
                    }
                    KeyCode::Char('q') if k.modifiers.contains(KeyModifiers::CONTROL) => break,
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
                    KeyCode::Enter | KeyCode::Char('e' | 'E') => notebook.begin_edit()?,
                    KeyCode::Delete => notebook.request_delete(),
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
            Event::Paste(text) if notebook.editing => {
                if let Some(editor) = &mut notebook.editor
                    && let Err(error) = editor.inline_paste(&text)
                {
                    notebook.message = format!("Edit error: {error:#}");
                }
            }
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) if m.row == 0 => {
                    let actions = if notebook.editing {
                        EDIT_ACTIONS
                    } else {
                        ACTIONS
                    };
                    if let Some(a) = builtin::action_at(m.column, actions) {
                        match a {
                            "edit" => notebook.begin_edit()?,
                            "delete" => notebook.request_delete(),
                            "save" => notebook.save_inline(false),
                            "undo" | "redo" => {
                                if let Some(editor) = &mut notebook.editor {
                                    let key = if a == "undo" { 'z' } else { 'y' };
                                    if let Err(error) = editor.inline_key(KeyEvent::new(
                                        KeyCode::Char(key),
                                        KeyModifiers::CONTROL,
                                    )) {
                                        notebook.message = format!("Edit error: {error:#}");
                                    }
                                }
                            }
                            "preview" => notebook.request_leave(),
                            "new" => prompt = Some(("new".into(), String::new())),
                            "search" => prompt = Some(("search".into(), String::new())),
                            "refresh" => notebook.refresh()?,
                            "close" => {
                                if notebook.editing {
                                    notebook.request_leave();
                                } else {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                MouseEventKind::Down(MouseButton::Left) if m.column < 32 && m.row > 1 => {
                    if notebook.editing {
                        if notebook.editor.as_ref().is_some_and(Editor::inline_dirty) {
                            notebook.request_leave();
                        } else {
                            notebook.leave_edit();
                            notebook.selected = (notebook.top + m.row as usize - 2)
                                .min(notebook.notes.len().saturating_sub(1));
                        }
                    } else {
                        notebook.selected = (notebook.top + m.row as usize - 2)
                            .min(notebook.notes.len().saturating_sub(1));
                    }
                }
                MouseEventKind::Down(MouseButton::Left) if m.column >= 32 && m.row > 0 => {
                    let pane_x = w.saturating_sub(12).clamp(1, 32) + 2;
                    if m.column < pane_x {
                        continue;
                    }
                    if !notebook.editing {
                        notebook.begin_edit()?;
                    }
                    if let Some(editor) = &mut notebook.editor {
                        editor.inline_click(
                            m.column.saturating_sub(pane_x),
                            m.row - 1,
                            w.saturating_sub(pane_x + 1),
                            h.saturating_sub(3),
                        );
                    }
                }
                MouseEventKind::ScrollUp if m.column < 32 => {
                    notebook.selected = notebook.selected.saturating_sub(3)
                }
                MouseEventKind::ScrollDown if m.column < 32 => {
                    notebook.selected =
                        (notebook.selected + 3).min(notebook.notes.len().saturating_sub(1))
                }
                MouseEventKind::ScrollUp if notebook.editing && m.column >= 32 => {
                    if let Some(editor) = &mut notebook.editor {
                        editor.inline_scroll(-3);
                    }
                }
                MouseEventKind::ScrollDown if notebook.editing && m.column >= 32 => {
                    if let Some(editor) = &mut notebook.editor {
                        editor.inline_scroll(3);
                    }
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
