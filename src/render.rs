use crate::config::{Look, parse_color};
use std::fmt::Write;
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub inverse: bool,
    pub bold: bool,
    pub underline: bool,
    pub fg: Option<vt100::Color>,
    pub bg: Option<vt100::Color>,
}
impl Style {
    #[cfg(test)]
    pub fn inverse() -> Self {
        Self {
            inverse: true,
            ..Self::default()
        }
    }
    pub fn program(cell: &vt100::Cell, color: bool) -> Self {
        let fg = cell.fgcolor();
        let bg = cell.bgcolor();
        Self {
            inverse: cell.inverse(),
            bold: cell.bold() || (!color && fg != vt100::Color::Default),
            underline: cell.underline() || (!color && bg != vt100::Color::Default),
            fg: color.then_some(fg),
            bg: color.then_some(bg),
        }
    }
    fn ansi(&self, look: &Look) -> String {
        let mut s = String::from("\x1b[0");
        if self.bold {
            s.push_str(";1");
        }
        if self.underline {
            s.push_str(";4");
        }
        if self.inverse {
            s.push_str(";7");
        }
        for (code, color, configured) in [(38, self.fg, &look.ink), (48, self.bg, &look.paper)] {
            match color.unwrap_or(vt100::Color::Default) {
                vt100::Color::Default => {
                    if let Some((r, g, b)) = parse_color(configured) {
                        let _ = write!(s, ";{code};2;{r};{g};{b}");
                    }
                }
                vt100::Color::Idx(n) => {
                    if n < 16 {
                        let base = if code == 38 {
                            if n < 8 { 30 } else { 90 }
                        } else if n < 8 {
                            40
                        } else {
                            100
                        };
                        let _ = write!(s, ";{}", base + n % 8);
                    } else {
                        let _ = write!(s, ";{code};5;{n}");
                    }
                }
                vt100::Color::Rgb(r, g, b) => {
                    let _ = write!(s, ";{code};2;{r};{g};{b}");
                }
            }
        }
        s.push('m');
        s
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub width: u8,
    pub style: Style,
}
impl Default for Cell {
    fn default() -> Self {
        Self {
            text: " ".into(),
            width: 1,
            style: Style::default(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub cursor: Option<(u16, u16)>,
}
impl Grid {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); cols as usize * rows as usize],
            cursor: None,
        }
    }
    pub fn put(&mut self, x: u16, y: u16, text: &str, width: u8, style: Style) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        let i = y as usize * self.cols as usize + x as usize;
        // An overlap may cut either half of an underlying wide glyph.
        if self.cells[i].width == 0 && x > 0 {
            self.cells[i - 1] = Cell::default();
        }
        if self.cells[i].width == 2 && x + 1 < self.cols {
            self.cells[i + 1] = Cell::default();
        }
        if width == 2 && x + 1 < self.cols {
            if self.cells[i + 1].width == 2 && x + 2 < self.cols {
                self.cells[i + 2] = Cell::default();
            }
            self.cells[i + 1] = Cell {
                text: String::new(),
                width: 0,
                style: style.clone(),
            };
        }
        self.cells[i] = if width == 2 && x + 1 >= self.cols {
            Cell {
                style,
                ..Cell::default()
            }
        } else {
            Cell {
                text: text.into(),
                width,
                style,
            }
        };
    }
    pub fn text(&mut self, mut x: u16, y: u16, text: &str, limit: u16, style: Style) {
        let end = x.saturating_add(limit).min(self.cols);
        for c in text.chars().filter(|c| !c.is_control()) {
            let width = c.width().unwrap_or(0) as u16;
            if width == 0 {
                continue;
            }
            if x + width > end {
                break;
            }
            self.put(x, y, &c.to_string(), width as u8, style.clone());
            x += width;
        }
    }
    pub fn fill(&mut self, x: u16, y: u16, w: u16, h: u16, text: &str, style: Style) {
        for row in y..y.saturating_add(h).min(self.rows) {
            for col in x..x.saturating_add(w).min(self.cols) {
                self.put(col, row, text, 1, style.clone());
            }
        }
    }
    pub fn diff(&self, previous: Option<&Self>, look: &Look) -> Vec<u8> {
        let mut out = String::from("\x1b[?25l");
        let compatible = previous.filter(|p| p.cols == self.cols && p.rows == self.rows);
        let mut style = None;
        let mut position = None;
        for y in 0..self.rows {
            for x in 0..self.cols {
                let i = y as usize * self.cols as usize + x as usize;
                let cell = &self.cells[i];
                if cell.width == 0 {
                    continue;
                }
                // If the second half changed, redraw the whole glyph.
                let changed = compatible.is_none_or(|p| {
                    p.cells[i] != *cell || (cell.width == 2 && p.cells[i + 1] != self.cells[i + 1])
                });
                if !changed {
                    continue;
                }
                if position != Some((x, y)) {
                    let _ = write!(out, "\x1b[{};{}H", y + 1, x + 1);
                }
                if style.as_ref() != Some(&cell.style) {
                    out.push_str(&cell.style.ansi(look));
                    style = Some(cell.style.clone());
                }
                out.push_str(&cell.text);
                position = Some((x + cell.width as u16, y));
            }
        }
        out.push_str("\x1b[0m");
        if let Some((x, y)) = self.cursor.filter(|&(x, y)| x < self.cols && y < self.rows) {
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", y + 1, x + 1);
        }
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retro_roles_survive_terminal_encoding_and_emulation() {
        let theme = crate::theme::Theme::default();
        let mut grid = Grid::new(8, 2);
        grid.put(0, 0, "D", 1, theme.desktop);
        grid.put(1, 0, "M", 1, theme.menu);
        grid.put(2, 0, "E", 1, theme.editor);
        grid.put(3, 0, "T", 1, theme.title);
        grid.put(4, 0, "K", 1, theme.hotkey);
        let mut parser = vt100::Parser::new(2.try_into().unwrap(), 8.try_into().unwrap(), 0);
        parser.process(&grid.diff(None, &Look::default()));
        for (x, fg, bg) in [(0, 6, 4), (1, 0, 7), (2, 14, 4), (3, 0, 6), (4, 1, 7)] {
            let cell = parser.screen().cell(0, x).unwrap();
            assert_eq!(cell.fgcolor(), vt100::Color::Idx(fg));
            assert_eq!(cell.bgcolor(), vt100::Color::Idx(bg));
        }
    }
    #[test]
    fn overlap_erases_the_whole_wide_character() {
        let mut grid = Grid::new(8, 2);
        grid.put(2, 0, "界", 2, Style::default());
        grid.put(3, 0, "│", 1, Style::default());
        assert_eq!(grid.cells[2].text, " ");
        assert_eq!(grid.cells[3].text, "│");
        grid.put(7, 1, "界", 2, Style::default());
        assert_eq!(grid.cells[15].text, " ");
    }
    #[test]
    fn differential_output_reconstructs_screen() {
        let mut parser = vt100::Parser::new(3.try_into().unwrap(), 12.try_into().unwrap(), 0);
        let mut first = Grid::new(12, 3);
        first.text(1, 0, "hello 界", 10, Style::inverse());
        parser.process(&first.diff(None, &Look::default()));
        let mut second = first.clone();
        second.put(8, 0, "X", 1, Style::default());
        second.text(0, 2, "bottom", 12, Style::default());
        parser.process(&second.diff(Some(&first), &Look::default()));
        for y in 0..3 {
            for x in 0..12 {
                let cell = &second.cells[y * 12 + x];
                if cell.width != 0 {
                    assert_eq!(
                        parser
                            .screen()
                            .cell(y as u16, x as u16)
                            .unwrap()
                            .contents()
                            .trim_end(),
                        cell.text.trim_end()
                    );
                }
            }
        }
    }
}
