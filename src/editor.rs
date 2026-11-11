use crate::{
    builtin::{self, Prompt, Surface},
    render::Grid,
};
use anyhow::{Context, Result, bail};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_FILE: usize = 2 * 1024 * 1024;
const HISTORY_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
}
impl Snapshot {
    fn weight(&self) -> usize {
        self.lines.iter().map(|l| l.len() * 4 + 24).sum()
    }
}

pub struct Document {
    state: Snapshot,
    saved: Vec<Vec<char>>,
    pub path: Option<PathBuf>,
    baseline: Option<Vec<u8>>,
    crlf: bool,
    bom: bool,
    undo: VecDeque<Snapshot>,
    redo: VecDeque<Snapshot>,
}
impl Document {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let (path, bytes) = if let Some(path) = path {
            let path = absolute(path)?;
            let bytes = match read_file(&path) {
                Ok(bytes) => Some(bytes),
                Err(e) if !path.exists() => {
                    if !path.parent().is_some_and(Path::is_dir) {
                        return Err(e);
                    }
                    None
                }
                Err(e) => return Err(e),
            };
            (Some(path), bytes)
        } else {
            (None, None)
        };
        let text = String::from_utf8(bytes.clone().unwrap_or_default())
            .context("the editor supports UTF-8 text files")?;
        if text.contains('\0') {
            bail!("this appears to be a binary file");
        }
        let bom = text.starts_with('\u{feff}');
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let crlf = text.contains("\r\n") && !text.replace("\r\n", "").contains('\n');
        let text = if crlf {
            text.replace("\r\n", "\n")
        } else {
            text.into()
        };
        let lines: Vec<Vec<char>> = text.split('\n').map(|l| l.chars().collect()).collect();
        if lines.len() > 100_000 {
            bail!("this file has too many lines for the first editor (limit: 100,000)");
        }
        Ok(Self {
            state: Snapshot {
                lines: lines.clone(),
                row: 0,
                col: 0,
            },
            saved: lines,
            path,
            baseline: bytes,
            crlf,
            bom,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        })
    }
    pub fn dirty(&self) -> bool {
        self.state.lines != self.saved
    }
    fn bytes(&self) -> Vec<u8> {
        let text = self
            .state
            .lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(if self.crlf { "\r\n" } else { "\n" });
        let mut bytes = if self.bom {
            vec![0xef, 0xbb, 0xbf]
        } else {
            vec![]
        };
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }
    fn insert(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if self.bytes().len() + text.len() > MAX_FILE {
            bail!("document size limit is 2 MiB");
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.state.lines.len() + text.bytes().filter(|&b| b == b'\n').count() > 100_000 {
            bail!("line limit is 100,000");
        }
        self.remember();
        let tail = self.state.lines[self.state.row].split_off(self.state.col);
        let mut chunks = text.split('\n');
        self.state.lines[self.state.row].extend(chunks.next().unwrap().chars());
        self.state.col = self.state.lines[self.state.row].len();
        for chunk in chunks {
            self.state.row += 1;
            self.state
                .lines
                .insert(self.state.row, chunk.chars().collect());
            self.state.col = chunk.chars().count();
        }
        self.state.lines[self.state.row].extend(tail);
        Ok(())
    }
    fn remember(&mut self) {
        push_history(&mut self.undo, self.state.clone());
        self.redo.clear();
    }
    fn backspace(&mut self) {
        if self.state.col > 0 {
            self.remember();
            self.state.col -= 1;
            self.state.lines[self.state.row].remove(self.state.col);
        } else if self.state.row > 0 {
            self.remember();
            let line = self.state.lines.remove(self.state.row);
            self.state.row -= 1;
            self.state.col = self.state.lines[self.state.row].len();
            self.state.lines[self.state.row].extend(line);
        }
    }
    fn delete(&mut self) {
        if self.state.col < self.state.lines[self.state.row].len() {
            self.remember();
            self.state.lines[self.state.row].remove(self.state.col);
        } else if self.state.row + 1 < self.state.lines.len() {
            self.remember();
            let line = self.state.lines.remove(self.state.row + 1);
            self.state.lines[self.state.row].extend(line);
        }
    }
    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop_back() {
            push_history(&mut self.redo, std::mem::replace(&mut self.state, previous));
        }
    }
    fn redo(&mut self) {
        if let Some(next) = self.redo.pop_back() {
            push_history(&mut self.undo, std::mem::replace(&mut self.state, next));
        }
    }
    fn find(&mut self, needle: &str) -> bool {
        if needle.is_empty() {
            return false;
        }
        let needle: Vec<char> = needle.chars().collect();
        let start = (self.state.row, self.state.col.saturating_add(1));
        for pass in 0..2 {
            let range = if pass == 0 {
                start.0..self.state.lines.len()
            } else {
                0..start.0 + 1
            };
            for row in range {
                let line = &self.state.lines[row];
                let from = if pass == 0 && row == start.0 {
                    start.1.min(line.len())
                } else {
                    0
                };
                if let Some(col) = line
                    .windows(needle.len())
                    .enumerate()
                    .skip(from)
                    .find(|(_, w)| *w == needle)
                    .map(|(i, _)| i)
                {
                    self.state.row = row;
                    self.state.col = col;
                    return true;
                }
            }
        }
        false
    }
    fn save(&mut self, target: &Path, overwrite: bool) -> Result<()> {
        let target = absolute(target)?;
        if self.path.as_ref() == Some(&target) {
            let current = match read_file(&target) {
                Ok(bytes) => Some(bytes),
                Err(_) if !target.exists() => None,
                Err(e) => return Err(e),
            };
            if current != self.baseline {
                bail!("file changed on disk; use Save as to preserve your edits");
            }
        } else if target.exists() && !overwrite {
            bail!("file already exists");
        }
        let bytes = self.bytes();
        atomic_write(&target, &bytes)?;
        self.path = Some(target);
        self.baseline = Some(bytes);
        self.saved = self.state.lines.clone();
        Ok(())
    }
}
fn push_history(history: &mut VecDeque<Snapshot>, snapshot: Snapshot) {
    history.push_back(snapshot);
    while history.len() > 100 || history.iter().map(Snapshot::weight).sum::<usize>() > HISTORY_BYTES
    {
        history.pop_front();
    }
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    if !fs::metadata(path)
        .with_context(|| format!("opening {}", path.display()))?
        .is_file()
    {
        bail!("not a regular file: {}", path.display());
    }
    let file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("not a regular file: {}", path.display());
    }
    if metadata.len() > MAX_FILE as u64 {
        bail!("file is larger than the first editor's 2 MiB limit");
    }
    let mut bytes = vec![];
    file.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE {
        bail!("file grew beyond the 2 MiB limit");
    }
    Ok(bytes)
}
fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.into()
    } else {
        std::env::current_dir()?.join(path)
    };
    if path.exists() {
        return fs::canonicalize(&path).with_context(|| format!("resolving {}", path.display()));
    }
    let parent = path.parent().context("missing parent directory")?;
    Ok(fs::canonicalize(parent)?.join(path.file_name().context("missing file name")?))
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if fs::metadata(path).is_ok_and(|m| m.permissions().readonly()) {
        bail!("file is read-only");
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().context("missing parent directory")?;
    let temp = parent.join(format!(
        ".saumux-save-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o666)
            .open(&temp)?;
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path).with_context(|| format!("saving {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

enum Dialog {
    SaveAs(Prompt),
    Find(Prompt),
    Close,
    Overwrite(PathBuf),
}
pub(crate) struct Editor {
    doc: Document,
    top: usize,
    left: usize,
    message: String,
    dialog: Option<Dialog>,
    close_after_save: bool,
    query: String,
}
const ACTIONS: &[(&str, &str)] = &[
    (" Save  ", "save"),
    (" Save as  ", "save_as"),
    (" Find  ", "find"),
    (" Undo  ", "undo"),
    (" Redo  ", "redo"),
    (" Close ", "close"),
];
const CLOSE_ACTIONS: &[(&str, &str)] = &[
    (" Save changes? ", ""),
    ("[S] Save  ", "save"),
    ("[D] Discard  ", "discard"),
    ("[Esc] Cancel", "cancel"),
];
const OVERWRITE_ACTIONS: &[(&str, &str)] = &[
    (" File exists. ", ""),
    ("[Y] Overwrite  ", "yes"),
    ("[N] Cancel", "cancel"),
];
const PROMPT_ACTIONS: &[(&str, &str)] = &[
    (" [OK] Enter  ", "ok"),
    ("[Cancel] Esc  ", "cancel"),
    ("Ctrl+U clears the field", ""),
];
impl Editor {
    pub(crate) fn new(doc: Document) -> Self {
        Self {
            doc,
            top: 0,
            left: 0,
            message: "Ctrl+S Save · Ctrl+O Save as · Ctrl+Q Close".into(),
            dialog: None,
            close_after_save: false,
            query: String::new(),
        }
    }
    fn visible(&mut self, width: u16, height: u16) {
        let rows = height.saturating_sub(3).max(1) as usize;
        if self.doc.state.row < self.top {
            self.top = self.doc.state.row;
        }
        if self.doc.state.row >= self.top + rows {
            self.top = self.doc.state.row + 1 - rows;
        }
        let col = builtin::column(
            &self.doc.state.lines[self.doc.state.row],
            self.doc.state.col,
        );
        if col < self.left {
            self.left = col;
        }
        if col >= self.left + width as usize {
            self.left = col + 1 - width as usize;
        }
    }
    fn draw(&mut self, width: u16, height: u16) -> Grid {
        self.visible(width, height);
        let theme = crate::theme::current();
        let mut grid = Grid::new(width, height);
        grid.fill(0, 0, width, height, " ", theme.editor.clone());
        builtin::toolbar(&mut grid, 0, ACTIONS);
        for (y, line) in self
            .doc
            .state
            .lines
            .iter()
            .skip(self.top)
            .take(height.saturating_sub(3) as usize)
            .enumerate()
        {
            builtin::line(&mut grid, y as u16 + 1, line, self.left, width);
        }
        let path = self
            .doc
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Untitled".into());
        let info = format!(
            " {}{}  Ln {} Col {}  {}",
            if self.doc.dirty() { "* " } else { "" },
            path,
            self.doc.state.row + 1,
            self.doc.state.col + 1,
            if self.doc.crlf { "CRLF" } else { "LF" }
        );
        builtin::bar(&mut grid, height.saturating_sub(2), &info);
        builtin::bar(&mut grid, height.saturating_sub(1), &self.message);
        let x = builtin::column(
            &self.doc.state.lines[self.doc.state.row],
            self.doc.state.col,
        )
        .saturating_sub(self.left) as u16;
        let y = self.doc.state.row.saturating_sub(self.top) as u16 + 1;
        if x < width && y < height.saturating_sub(2) {
            grid.cursor = Some((x, y));
        }
        match &self.dialog {
            Some(Dialog::SaveAs(p) | Dialog::Find(p)) => {
                builtin::toolbar(&mut grid, height.saturating_sub(2), PROMPT_ACTIONS);
                p.draw(&mut grid);
            }
            Some(Dialog::Close) => {
                builtin::toolbar(&mut grid, height.saturating_sub(1), CLOSE_ACTIONS);
                grid.text(
                    0,
                    height.saturating_sub(1),
                    CLOSE_ACTIONS[0].0,
                    CLOSE_ACTIONS[0].0.len() as u16,
                    theme.warning.clone(),
                );
                grid.cursor = None;
            }
            Some(Dialog::Overwrite(_)) => {
                builtin::toolbar(&mut grid, height.saturating_sub(1), OVERWRITE_ACTIONS);
                grid.text(
                    0,
                    height.saturating_sub(1),
                    OVERWRITE_ACTIONS[0].0,
                    OVERWRITE_ACTIONS[0].0.len() as u16,
                    theme.warning.clone(),
                );
                grid.cursor = None;
            }
            None => {}
        }
        grid
    }
    fn save(&mut self) -> Result<bool> {
        if let Some(path) = self.doc.path.clone() {
            self.doc.save(&path, false)?;
            self.message = "Saved".into();
            Ok(self.close_after_save)
        } else {
            self.save_as();
            Ok(false)
        }
    }
    pub(crate) fn inline_dirty(&self) -> bool {
        self.doc.dirty()
    }
    pub(crate) fn inline_save(&mut self) -> Result<()> {
        let path = self.doc.path.clone().context("note has no file path")?;
        self.doc.save(&path, false)?;
        self.message = "Saved".into();
        Ok(())
    }
    pub(crate) fn inline_paste(&mut self, text: &str) -> Result<()> {
        self.doc.insert(text)
    }
    pub(crate) fn inline_key(&mut self, key: KeyEvent) -> Result<()> {
        self.key(key)?;
        Ok(())
    }
    pub(crate) fn inline_click(&mut self, x: u16, y: u16, width: u16, height: u16) {
        self.inline_visible(width, height);
        self.doc.state.row = (self.top + y as usize).min(self.doc.state.lines.len() - 1);
        self.doc.state.col = builtin::index_at(
            &self.doc.state.lines[self.doc.state.row],
            self.left + x as usize,
        );
    }
    pub(crate) fn inline_scroll(&mut self, delta: isize) {
        self.doc.state.row = self
            .doc
            .state
            .row
            .saturating_add_signed(delta)
            .min(self.doc.state.lines.len().saturating_sub(1));
        self.doc.state.col = self
            .doc
            .state
            .col
            .min(self.doc.state.lines[self.doc.state.row].len());
    }
    pub(crate) fn draw_inline(&mut self, grid: &mut Grid, x: u16, y: u16, width: u16, height: u16) {
        self.inline_visible(width, height);
        for (line_y, line) in self
            .doc
            .state
            .lines
            .iter()
            .skip(self.top)
            .take(height as usize)
            .enumerate()
        {
            builtin::line(grid, y + line_y as u16, line, self.left, width);
        }
        let col = builtin::column(
            &self.doc.state.lines[self.doc.state.row],
            self.doc.state.col,
        )
        .saturating_sub(self.left) as u16;
        let row = self.doc.state.row.saturating_sub(self.top) as u16;
        if col < width && row < height {
            grid.cursor = Some((x + col, y + row));
        }
    }
    fn inline_visible(&mut self, width: u16, height: u16) {
        let rows = height.max(1) as usize;
        if self.doc.state.row < self.top {
            self.top = self.doc.state.row;
        }
        if self.doc.state.row >= self.top + rows {
            self.top = self.doc.state.row + 1 - rows;
        }
        let col = builtin::column(
            &self.doc.state.lines[self.doc.state.row],
            self.doc.state.col,
        );
        if col < self.left {
            self.left = col;
        }
        if col >= self.left + width as usize {
            self.left = col + 1 - width as usize;
        }
    }
    fn save_as(&mut self) {
        let path = self
            .doc
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.dialog = Some(Dialog::SaveAs(Prompt::new("Save as (Enter / Esc)", path)));
    }
    fn action(&mut self, action: &str) -> Result<bool> {
        match action {
            "save" => return self.save(),
            "save_as" => {
                self.close_after_save = false;
                self.save_as();
            }
            "find" => {
                self.dialog = Some(Dialog::Find(Prompt::new(
                    "Find (Enter / Esc)",
                    self.query.clone(),
                )))
            }
            "undo" => self.doc.undo(),
            "redo" => self.doc.redo(),
            "close" => {
                if self.doc.dirty() {
                    self.dialog = Some(Dialog::Close);
                } else {
                    return Ok(true);
                }
            }
            _ => {}
        }
        Ok(false)
    }
    pub(crate) fn key(&mut self, key: KeyEvent) -> Result<bool> {
        if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.dialog = None;
            self.close_after_save = false;
            return self.action("close");
        }
        if let Some(dialog) = self.dialog.take() {
            match dialog {
                Dialog::Close => match key.code {
                    KeyCode::Char('s' | 'S') => {
                        self.close_after_save = true;
                        return self.save();
                    }
                    KeyCode::Char('d' | 'D') => return Ok(true),
                    KeyCode::Esc => self.close_after_save = false,
                    _ => self.dialog = Some(Dialog::Close),
                },
                Dialog::Overwrite(path) => match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        self.doc.save(&path, true)?;
                        self.message = "Saved".into();
                        return Ok(self.close_after_save);
                    }
                    KeyCode::Char('n' | 'N') | KeyCode::Esc => self.close_after_save = false,
                    _ => self.dialog = Some(Dialog::Overwrite(path)),
                },
                Dialog::SaveAs(mut prompt) => match key.code {
                    KeyCode::Esc => self.close_after_save = false,
                    KeyCode::Enter => {
                        if prompt.text.is_empty() {
                            self.dialog = Some(Dialog::SaveAs(prompt));
                            return Ok(false);
                        }
                        let path = absolute(Path::new(&prompt.text))?;
                        if path.exists() && self.doc.path.as_ref() != Some(&path) {
                            self.dialog = Some(Dialog::Overwrite(path));
                        } else {
                            self.doc.save(&path, false)?;
                            self.message = "Saved".into();
                            return Ok(self.close_after_save);
                        }
                    }
                    _ => {
                        prompt.edit(key);
                        self.dialog = Some(Dialog::SaveAs(prompt));
                    }
                },
                Dialog::Find(mut prompt) => match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        self.query = prompt.text;
                        if !self.doc.find(&self.query) {
                            self.message = "No match".into();
                        }
                    }
                    _ => {
                        prompt.edit(key);
                        self.dialog = Some(Dialog::Find(prompt));
                    }
                },
            }
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('s') => return self.action("save"),
                KeyCode::Char('o') => return self.action("save_as"),
                KeyCode::Char('q') => return self.action("close"),
                KeyCode::Char('f') => return self.action("find"),
                KeyCode::Char('z') => return self.action("undo"),
                KeyCode::Char('y') => return self.action("redo"),
                KeyCode::Home => {
                    self.doc.state.row = 0;
                    self.doc.state.col = 0;
                }
                KeyCode::End => {
                    self.doc.state.row = self.doc.state.lines.len() - 1;
                    self.doc.state.col = self.doc.state.lines[self.doc.state.row].len();
                }
                _ => {}
            }
            return Ok(false);
        }
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.doc.insert(&c.to_string())?
            }
            KeyCode::Enter => self.doc.insert("\n")?,
            KeyCode::Tab => self.doc.insert("\t")?,
            KeyCode::Backspace => self.doc.backspace(),
            KeyCode::Delete => self.doc.delete(),
            KeyCode::Left => {
                if self.doc.state.col > 0 {
                    self.doc.state.col -= 1;
                } else if self.doc.state.row > 0 {
                    self.doc.state.row -= 1;
                    self.doc.state.col = self.doc.state.lines[self.doc.state.row].len();
                }
            }
            KeyCode::Right => {
                if self.doc.state.col < self.doc.state.lines[self.doc.state.row].len() {
                    self.doc.state.col += 1;
                } else if self.doc.state.row + 1 < self.doc.state.lines.len() {
                    self.doc.state.row += 1;
                    self.doc.state.col = 0;
                }
            }
            KeyCode::Up => self.doc.state.row = self.doc.state.row.saturating_sub(1),
            KeyCode::Down => {
                self.doc.state.row = (self.doc.state.row + 1).min(self.doc.state.lines.len() - 1)
            }
            KeyCode::PageUp => self.doc.state.row = self.doc.state.row.saturating_sub(10),
            KeyCode::PageDown => {
                self.doc.state.row = (self.doc.state.row + 10).min(self.doc.state.lines.len() - 1)
            }
            KeyCode::Home => self.doc.state.col = 0,
            KeyCode::End => self.doc.state.col = self.doc.state.lines[self.doc.state.row].len(),
            _ => {}
        }
        self.doc.state.col = self
            .doc
            .state
            .col
            .min(self.doc.state.lines[self.doc.state.row].len());
        Ok(false)
    }
}
pub fn run(path: Option<&Path>) -> Result<()> {
    run_named(path, "Editor")
}
pub fn run_named(path: Option<&Path>, label: &str) -> Result<()> {
    let doc = Document::open(path)?;
    let mut editor = Editor::new(doc);
    let mut surface = Surface::enter()?;
    loop {
        let (width, height) = surface.size()?;
        let grid = editor.draw(width, height);
        let name = editor
            .doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        surface.draw(
            grid,
            &format!(
                "{}{} — {label}",
                if editor.doc.dirty() { "* " } else { "" },
                name
            ),
        )?;
        let Some(event) = surface.event()? else {
            continue;
        };
        let result = (|| -> Result<bool> {
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => return editor.key(key),
                Event::Paste(text) => {
                    if let Some(Dialog::SaveAs(p) | Dialog::Find(p)) = &mut editor.dialog {
                        let text = text.replace(['\r', '\n'], "");
                        let byte = p
                            .text
                            .char_indices()
                            .nth(p.cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(p.text.len());
                        p.text.insert_str(byte, &text);
                        p.cursor += text.chars().count();
                    } else if editor.dialog.is_none() {
                        editor.doc.insert(&text)?;
                    }
                }
                Event::Mouse(mouse) => {
                    if editor.dialog.is_some() {
                        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                            let actions = match editor.dialog {
                                Some(Dialog::Close) if mouse.row == height.saturating_sub(1) => {
                                    CLOSE_ACTIONS
                                }
                                Some(Dialog::Overwrite(_))
                                    if mouse.row == height.saturating_sub(1) =>
                                {
                                    OVERWRITE_ACTIONS
                                }
                                Some(Dialog::SaveAs(_) | Dialog::Find(_))
                                    if mouse.row == height.saturating_sub(2) =>
                                {
                                    PROMPT_ACTIONS
                                }
                                _ => return Ok(false),
                            };
                            let Some(action) = builtin::action_at(mouse.column, actions) else {
                                return Ok(false);
                            };
                            let code = match action {
                                "save" => KeyCode::Char('s'),
                                "discard" => KeyCode::Char('d'),
                                "yes" => KeyCode::Char('y'),
                                "ok" => KeyCode::Enter,
                                _ => KeyCode::Esc,
                            };
                            return editor.key(KeyEvent::new(code, KeyModifiers::NONE));
                        }
                    } else {
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) if mouse.row == 0 => {
                                if let Some(action) = builtin::action_at(mouse.column, ACTIONS) {
                                    return editor.action(action);
                                }
                            }
                            MouseEventKind::Down(MouseButton::Left)
                                if mouse.row < height.saturating_sub(2) =>
                            {
                                editor.doc.state.row = (editor.top
                                    + mouse.row.saturating_sub(1) as usize)
                                    .min(editor.doc.state.lines.len() - 1);
                                editor.doc.state.col = builtin::index_at(
                                    &editor.doc.state.lines[editor.doc.state.row],
                                    editor.left + mouse.column as usize,
                                );
                            }
                            MouseEventKind::ScrollUp => {
                                editor.doc.state.row = editor.doc.state.row.saturating_sub(3);
                            }
                            MouseEventKind::ScrollDown => {
                                editor.doc.state.row = (editor.doc.state.row + 3)
                                    .min(editor.doc.state.lines.len() - 1);
                                editor.doc.state.col = editor
                                    .doc
                                    .state
                                    .col
                                    .min(editor.doc.state.lines[editor.doc.state.row].len());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            Ok(false)
        })();
        editor.doc.state.col = editor
            .doc
            .state
            .col
            .min(editor.doc.state.lines[editor.doc.state.row].len());
        match result {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => {
                editor.message = e.to_string();
                editor.close_after_save = false;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_editing_undo_and_newlines() {
        let mut doc = Document::open(None).unwrap();
        doc.insert("Grüße\n界").unwrap();
        assert!(doc.dirty());
        doc.backspace();
        assert_eq!(String::from_utf8(doc.bytes()).unwrap(), "Grüße\n");
        doc.undo();
        assert_eq!(String::from_utf8(doc.bytes()).unwrap(), "Grüße\n界");
        doc.undo();
        assert!(!doc.dirty());
        doc.redo();
        assert!(doc.dirty());
    }
    #[test]
    fn save_preserves_crlf_and_rejects_external_changes() {
        let root = std::env::temp_dir().join(format!("sx-editor-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("notes.txt");
        fs::write(&path, b"one\r\ntwo\r\n").unwrap();
        let mut doc = Document::open(Some(&path)).unwrap();
        doc.insert("X").unwrap();
        doc.save(&path, false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"Xone\r\ntwo\r\n");
        assert!(!doc.dirty());
        fs::write(&path, b"external change").unwrap();
        doc.insert("Y").unwrap();
        assert!(doc.save(&path, false).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        fs::remove_dir_all(root).unwrap();
    }
}
