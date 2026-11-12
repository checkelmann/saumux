mod ops;
mod panel;
mod preview;

use crate::{
    builtin::{self, Prompt, Surface},
    render::Grid,
};
use anyhow::{bail, Result};
use crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use panel::Panel;
use preview::PreviewOverlay;
use std::path::{Path, PathBuf};

const ACTIONS: &[(&str, &str)] = &[
    (" Tab  ", "tab"),
    (" Copy  ", "copy"),
    (" Move  ", "move"),
    (" Delete  ", "delete"),
    (" MkDir  ", "mkdir"),
    (" Rename  ", "rename"),
    (" Preview  ", "preview"),
    (" Hidden  ", "hidden"),
    (" Close ", "close"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side { Left, Right }

#[derive(Clone, Copy)]
enum PromptKind { Location, Rename, Mkdir, Copy, Move, Delete }

struct PendingOp {
    kind: PromptKind,
    sources: Vec<PathBuf>,
    prompt: Prompt,
}

struct DualBrowser {
    left: Panel,
    right: Panel,
    active: Side,
    message: String,
    pending: Option<PendingOp>,
    preview: Option<PreviewOverlay>,
}

impl DualBrowser {
    fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            left: Panel::new(path)?,
            right: Panel::new(path)?,
            active: Side::Left,
            message: "Tab switches panels · Insert marks · Ctrl+C/X/D copy/move/delete · Ctrl+P preview · Ctrl+Q close".into(),
            pending: None,
            preview: None,
        })
    }

    fn active_panel(&self) -> &Panel {
        match self.active { Side::Left => &self.left, Side::Right => &self.right }
    }
    fn active_panel_mut(&mut self) -> &mut Panel {
        match self.active { Side::Left => &mut self.left, Side::Right => &mut self.right }
    }
    fn other_cwd(&self) -> PathBuf {
        match self.active {
            Side::Left => self.right.cwd.clone(),
            Side::Right => self.left.cwd.clone(),
        }
    }

    fn draw(&mut self, width: u16, height: u16) -> Grid {
        let theme = crate::theme::current();
        let mut grid = Grid::new(width, height);
        grid.fill(0, 0, width, height, " ", theme.text.clone());

        // Toolbar row 0
        builtin::toolbar(&mut grid, 0, ACTIONS);

        // Panels: rows 1..height-2
        let panel_height = height.saturating_sub(2);
        let half = width / 2;
        let right_w = width.saturating_sub(half + 1);

        let left_grid = self.left.draw(half, panel_height, self.active == Side::Left);
        let right_grid = self.right.draw(right_w, panel_height, self.active == Side::Right);

        grid.blit(&left_grid, 0, 1);

        // Divider
        for row in 1..height.saturating_sub(1) {
            grid.put(half, row, "│", 1, theme.frame.clone());
        }

        grid.blit(&right_grid, half + 1, 1);

        // Message / prompt bar at height-1
        if let Some(op) = &self.pending {
            op.prompt.draw(&mut grid);
        } else {
            builtin::bar(&mut grid, height.saturating_sub(1), &self.message);
        }

        // Preview overlay on top
        if let Some(pv) = &self.preview {
            pv.draw(&mut grid, width, height);
        }

        grid
    }

    fn start_pending(&mut self, kind: PromptKind) {
        let sources = self.active_panel().marked_paths();
        if matches!(kind, PromptKind::Copy | PromptKind::Move | PromptKind::Delete)
            && sources.is_empty()
        {
            self.message = "Nothing selected".into();
            return;
        }
        if matches!(kind, PromptKind::Rename) && sources.len() != 1 {
            self.message = "Rename works on a single file — unmark extras".into();
            return;
        }

        let (label, default_text) = match kind {
            PromptKind::Copy => (
                format!("Copy {} item(s) to", sources.len()),
                self.other_cwd().display().to_string(),
            ),
            PromptKind::Move => (
                format!("Move {} item(s) to", sources.len()),
                self.other_cwd().display().to_string(),
            ),
            PromptKind::Delete => (
                format!("Delete {} item(s)? Enter confirms", sources.len()),
                String::new(),
            ),
            PromptKind::Mkdir => ("New directory name".into(), String::new()),
            PromptKind::Rename => (
                "Rename to".into(),
                sources
                    .first()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            PromptKind::Location => (
                "Directory".into(),
                self.active_panel().cwd.display().to_string(),
            ),
        };

        self.pending = Some(PendingOp {
            kind,
            sources,
            prompt: Prompt::new(&label, default_text),
        });
    }

    fn submit_pending(&mut self, text: String, sources: Vec<PathBuf>, kind: PromptKind) {
        let result = match kind {
            PromptKind::Copy => {
                let dest = PathBuf::from(&text);
                let (ok, errors) = ops::copy_entries(&sources, &dest);
                self.left.refresh().ok();
                self.right.refresh().ok();
                if errors.is_empty() {
                    Ok(format!("Copied {ok} item(s)"))
                } else {
                    Ok(format!("Copied {ok}, {} error(s): {}", errors.len(), errors[0]))
                }
            }
            PromptKind::Move => {
                let dest = PathBuf::from(&text);
                let (ok, errors) = ops::move_entries(&sources, &dest);
                self.active_panel_mut().clear_marks();
                self.left.refresh().ok();
                self.right.refresh().ok();
                if errors.is_empty() {
                    Ok(format!("Moved {ok} item(s)"))
                } else {
                    Ok(format!("Moved {ok}, {} error(s): {}", errors.len(), errors[0]))
                }
            }
            PromptKind::Delete => {
                let (ok, errors) = ops::delete_entries(&sources);
                self.active_panel_mut().clear_marks();
                self.active_panel_mut().refresh().ok();
                if errors.is_empty() {
                    Ok(format!("Deleted {ok} item(s)"))
                } else {
                    Ok(format!("Deleted {ok}, {} error(s): {}", errors.len(), errors[0]))
                }
            }
            PromptKind::Mkdir => {
                let cwd = self.active_panel().cwd.clone();
                ops::mkdir(&cwd, &text).map(|_| {
                    self.active_panel_mut().refresh().ok();
                    format!("Created directory '{text}'")
                })
            }
            PromptKind::Rename => {
                let src = sources.into_iter().next().unwrap();
                ops::rename(&src, &text).map(|_| {
                    self.active_panel_mut().refresh().ok();
                    format!("Renamed to '{text}'")
                })
            }
            PromptKind::Location => {
                let path = PathBuf::from(&text);
                let path = if path.is_absolute() {
                    path
                } else {
                    self.active_panel().cwd.join(path)
                };
                self.active_panel_mut()
                    .navigate(&path)
                    .map(|_| String::new())
            }
        };
        match result {
            Ok(msg) => self.message = msg,
            Err(e) => self.message = e.to_string(),
        }
    }

    fn toggle_preview(&mut self) {
        if self.preview.is_some() {
            self.preview = None;
            return;
        }
        let panel = self.active_panel();
        if let Some(entry) = panel.entries.get(panel.selected) {
            if !entry.directory {
                self.preview = Some(PreviewOverlay::load(&entry.path.clone()));
                return;
            }
        }
        self.message = "Select a file to preview".into();
    }

    /// Handle a toolbar / keybinding action string. Returns true to close.
    fn action(&mut self, action: &str) -> Result<bool> {
        match action {
            "tab" => {
                self.active = if self.active == Side::Left { Side::Right } else { Side::Left };
                self.message = String::new();
            }
            "copy" => self.start_pending(PromptKind::Copy),
            "move" => self.start_pending(PromptKind::Move),
            "delete" => self.start_pending(PromptKind::Delete),
            "mkdir" => self.start_pending(PromptKind::Mkdir),
            "rename" => self.start_pending(PromptKind::Rename),
            "preview" => self.toggle_preview(),
            "hidden" => {
                self.active_panel_mut().toggle_hidden()?;
            }
            "location" => self.start_pending(PromptKind::Location),
            "close" => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn handle_event(&mut self, event: Event, height: u16, half: u16) -> Result<bool> {
        // --- Preview mode ---
        if let Some(pv) = &mut self.preview {
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Esc => self.preview = None,
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.preview = None
                    }
                    KeyCode::Up => pv.scroll_up(),
                    KeyCode::Down => {
                        let visible = height.saturating_sub(8) as usize;
                        pv.scroll_down(visible);
                    }
                    KeyCode::PageUp => {
                        for _ in 0..10 { pv.scroll_up(); }
                    }
                    KeyCode::PageDown => {
                        let visible = height.saturating_sub(8) as usize;
                        for _ in 0..10 { pv.scroll_down(visible); }
                    }
                    _ => {}
                },
                _ => {}
            }
            return Ok(false);
        }

        // --- Prompt mode ---
        if let Some(op) = self.pending.take() {
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Esc => {
                        self.message = "Cancelled".into();
                    }
                    KeyCode::Enter => {
                        let text = op.prompt.text.clone();
                        let sources = op.sources;
                        let kind = op.kind;
                        self.submit_pending(text, sources, kind);
                    }
                    _ => {
                        let mut op = op;
                        op.prompt.edit(key);
                        self.pending = Some(op);
                    }
                },
                Event::Paste(text) => {
                    let mut op = op;
                    op.prompt.text.push_str(&text.replace(['\r', '\n'], ""));
                    op.prompt.cursor = op.prompt.text.chars().count();
                    self.pending = Some(op);
                }
                _ => { self.pending = Some(op); }
            }
            return Ok(false);
        }

        // --- Normal mode ---
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('q') => return Ok(true),
                        KeyCode::Char('c') => self.start_pending(PromptKind::Copy),
                        KeyCode::Char('x') => self.start_pending(PromptKind::Move),
                        KeyCode::Char('d') => self.start_pending(PromptKind::Delete),
                        KeyCode::Char('n') => self.start_pending(PromptKind::Mkdir),
                        KeyCode::Char('r') => self.start_pending(PromptKind::Rename),
                        KeyCode::Char('p') => self.toggle_preview(),
                        KeyCode::Char('l') => self.start_pending(PromptKind::Location),
                        KeyCode::Char('a') => self.active_panel_mut().mark_all(),
                        KeyCode::F(5) => self.start_pending(PromptKind::Copy),
                        KeyCode::F(6) => self.start_pending(PromptKind::Move),
                        KeyCode::F(8) => self.start_pending(PromptKind::Delete),
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Tab => {
                            self.active = if self.active == Side::Left {
                                Side::Right
                            } else {
                                Side::Left
                            };
                        }
                        KeyCode::Insert | KeyCode::Char(' ') => {
                            self.active_panel_mut().toggle_mark()
                        }
                        KeyCode::Up => {
                            let p = self.active_panel_mut();
                            p.selected = p.selected.saturating_sub(1);
                        }
                        KeyCode::Down => {
                            let p = self.active_panel_mut();
                            p.selected =
                                (p.selected + 1).min(p.entries.len().saturating_sub(1));
                        }
                        KeyCode::PageUp => {
                            let p = self.active_panel_mut();
                            let page = height.saturating_sub(4) as usize;
                            p.selected = p.selected.saturating_sub(page);
                        }
                        KeyCode::PageDown => {
                            let p = self.active_panel_mut();
                            let page = height.saturating_sub(4) as usize;
                            p.selected =
                                (p.selected + page).min(p.entries.len().saturating_sub(1));
                        }
                        KeyCode::Home => self.active_panel_mut().selected = 0,
                        KeyCode::End => {
                            let p = self.active_panel_mut();
                            p.selected = p.entries.len().saturating_sub(1);
                        }
                        KeyCode::Enter => {
                            match self.active_panel_mut().open()? {
                                true => self.message = "Opened in a new editor window".into(),
                                false => {}
                            }
                        }
                        KeyCode::Backspace => self.active_panel_mut().up()?,
                        KeyCode::Char('.') => self.active_panel_mut().toggle_hidden()?,
                        _ => {}
                    }
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) if mouse.row == 0 => {
                    if let Some(action) = builtin::action_at(mouse.column, ACTIONS) {
                        return self.action(action);
                    }
                }
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row >= 1 && mouse.row < height.saturating_sub(1) =>
                {
                    use std::time::{Duration, Instant};
                    // Determine which panel was clicked
                    let (side, col_in_panel) = if mouse.column < half {
                        (Side::Left, mouse.column)
                    } else if mouse.column > half {
                        (Side::Right, mouse.column - half - 1)
                    } else {
                        return Ok(false); // divider
                    };
                    let _ = col_in_panel; // may be used for future column actions
                    self.active = side;
                    let panel_row = mouse.row - 1; // row within panel grid
                    // Row 0 = path header, skip
                    if panel_row == 0 { return Ok(false); }
                    let panel = self.active_panel_mut();
                    let index = panel.top + (panel_row as usize - 1);
                    if index < panel.entries.len() {
                        if panel.last_click.is_some_and(|(prev, t): (usize, Instant)| {
                            prev == index && t.elapsed() < Duration::from_millis(400)
                        }) {
                            panel.last_click = None;
                            match panel.open()? {
                                true => self.message = "Opened in a new editor window".into(),
                                false => {}
                            }
                        } else {
                            panel.selected = index;
                            panel.last_click = Some((index, Instant::now()));
                        }
                    }
                }
                MouseEventKind::ScrollUp => {
                    let p = self.active_panel_mut();
                    p.selected = p.selected.saturating_sub(3);
                }
                MouseEventKind::ScrollDown => {
                    let p = self.active_panel_mut();
                    p.selected = (p.selected + 3).min(p.entries.len().saturating_sub(1));
                }
                _ => {}
            },
            _ => {}
        }
        Ok(false)
    }
}

pub fn run(path: &Path) -> Result<()> {
    let mut browser = DualBrowser::new(path)?;
    let mut surface = Surface::enter()?;
    loop {
        let (width, height) = surface.size()?;
        let half = width / 2;
        let grid = browser.draw(width, height);
        surface.draw(
            grid,
            &format!(
                "Files — {} | {}",
                browser.left.cwd.display(),
                browser.right.cwd.display()
            ),
        )?;
        let Some(event) = surface.event()? else { continue };
        let result = browser.handle_event(event, height, half);
        match result {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => browser.message = e.to_string(),
        }
    }
    Ok(())
}

pub(super) fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} K", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} M", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} G", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
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
