//! Shared terminal surface for the bundled applications. They remain ordinary
//! PTY programs, using the same window manager and emulator as every other app.
use crate::{config::Look, render::Grid};
use anyhow::Result;
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event,
    },
    execute,
    terminal::{self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    io::{self, Write},
    time::Duration,
};
use unicode_width::UnicodeWidthChar;

pub struct Surface {
    previous: Option<Grid>,
    title: String,
}
impl Surface {
    pub fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let surface = Self {
            previous: None,
            title: String::new(),
        };
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            DisableLineWrap,
            Hide,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        // Drag reports suffice; avoid traffic from every hover movement.
        io::stdout().write_all(b"\x1b[?1003l\x1b[?1002h")?;
        Ok(surface)
    }
    pub fn size(&self) -> Result<(u16, u16)> {
        let (w, h) = terminal::size()?;
        Ok((w.max(1), h.max(1)))
    }
    pub fn draw(&mut self, grid: Grid, title: &str) -> Result<()> {
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(240)
            .collect();
        if self.previous.as_ref() == Some(&grid) && self.title == title {
            return Ok(());
        }
        let mut stdout = io::stdout().lock();
        write!(stdout, "\x1b]2;{title}\x07")?;
        let look = Look {
            theme: crate::theme::current_id(),
            ..Look::default()
        };
        stdout.write_all(&grid.diff(self.previous.as_ref(), &look))?;
        stdout.flush()?;
        self.previous = Some(grid);
        self.title = title;
        Ok(())
    }
    pub fn event(&self) -> Result<Option<Event>> {
        if event::poll(Duration::from_millis(100))? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            EnableLineWrap,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

pub fn bar(grid: &mut Grid, y: u16, text: &str) {
    grid.fill(0, y, grid.cols, 1, " ", crate::theme::current().status);
    grid.text(0, y, text, grid.cols, crate::theme::current().status);
}
pub fn toolbar(grid: &mut Grid, y: u16, actions: &[(&str, &str)]) {
    let theme = crate::theme::current();
    grid.fill(0, y, grid.cols, 1, " ", theme.menu.clone());
    let mut x = 0;
    for (label, action) in actions {
        grid.text(x, y, label, label.len() as u16, theme.menu.clone());
        if !action.is_empty() {
            let offset = label.chars().position(|c| c.is_alphanumeric()).unwrap_or(0) as u16;
            if let Some(c) = label.chars().nth(offset as usize) {
                grid.text(x + offset, y, &c.to_string(), 1, theme.hotkey.clone());
            }
        }
        x += label.len() as u16;
    }
}
pub fn action_at<'a>(x: u16, actions: &[(&str, &'a str)]) -> Option<&'a str> {
    let mut start = 0;
    for (label, action) in actions {
        let end = start + label.len() as u16;
        if x >= start && x < end {
            return (!action.is_empty()).then_some(*action);
        }
        start = end;
    }
    None
}
// Render tabs, wide glyphs and combining marks while clipping in whole cells.
// Returning character indices lets the editor use the identical geometry for clicks.
pub fn line(grid: &mut Grid, y: u16, text: &[char], left: usize, width: u16) {
    line_at(grid, 0, y, text, left, width);
}
pub fn line_at(grid: &mut Grid, origin_x: u16, y: u16, text: &[char], left: usize, width: u16) {
    let theme = crate::theme::current();
    let mut column = 0usize;
    let mut previous = None;
    for &c in text {
        let cells = if c == '\t' {
            4 - column % 4
        } else {
            c.width().unwrap_or(0)
        };
        if cells == 0 {
            if !c.is_control()
                && let Some(x) = previous
            {
                let i = y as usize * grid.cols as usize + x as usize;
                if let Some(cell) = grid.cells.get_mut(i) {
                    cell.text.push(c);
                }
            }
            continue;
        }
        if column >= left + width as usize {
            break;
        }
        if column >= left && column + cells <= left + width as usize {
            let x = origin_x + (column - left) as u16;
            if c == '\t' {
                grid.fill(x, y, cells as u16, 1, " ", theme.editor.clone());
                previous = None;
            } else {
                grid.put(
                    x,
                    y,
                    &if c.is_control() {
                        "�".into()
                    } else {
                        c.to_string()
                    },
                    cells.min(2) as u8,
                    theme.editor.clone(),
                );
                previous = Some(x);
            }
        } else {
            previous = None;
        }
        column += cells;
    }
}
pub fn column(chars: &[char], index: usize) -> usize {
    chars.iter().take(index).fold(0, |n, &c| {
        n + if c == '\t' {
            4 - n % 4
        } else {
            c.width().unwrap_or(0)
        }
    })
}
pub fn index_at(chars: &[char], target: usize) -> usize {
    let mut col = 0;
    for (index, &c) in chars.iter().enumerate() {
        let width = if c == '\t' {
            4 - col % 4
        } else {
            c.width().unwrap_or(0)
        };
        if col + width > target {
            return index;
        }
        col += width;
    }
    chars.len()
}

pub struct Prompt {
    pub label: String,
    pub text: String,
    pub cursor: usize,
}
impl Prompt {
    pub fn new(label: &str, text: String) -> Self {
        let cursor = text.chars().count();
        Self {
            label: label.into(),
            text,
            cursor,
        }
    }
    pub fn edit(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let mut chars: Vec<char> = self.text.chars().collect();
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                chars.clear();
                self.cursor = 0;
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => self.cursor = 0,
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = chars.len()
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                chars.truncate(self.cursor)
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < chars.len() => {
                chars.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = chars.len(),
            _ => {}
        }
        self.text = chars.into_iter().collect();
    }
    pub fn draw(&self, grid: &mut Grid) {
        let theme = crate::theme::current();
        let y = grid.rows.saturating_sub(1);
        let prefix = format!("{}: ", self.label);
        let start = prefix.chars().count() as u16;
        bar(grid, y, &prefix);
        let chars: Vec<_> = self.text.chars().collect();
        let cursor = column(&chars, self.cursor);
        let width = grid.cols.saturating_sub(start).max(1);
        let left = cursor.saturating_sub(width.saturating_sub(1) as usize);
        let mut field = Grid::new(width, 1);
        line(&mut field, 0, &chars, left, width);
        for (x, cell) in field.cells.iter().enumerate() {
            if cell.width > 0 {
                grid.put(
                    start + x as u16,
                    y,
                    &cell.text,
                    cell.width,
                    theme.field.clone(),
                );
            }
        }
        if start < grid.cols {
            grid.cursor = Some((start + (cursor - left) as u16, y));
        }
    }
}
