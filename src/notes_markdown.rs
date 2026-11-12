//! CommonMark preview rendered into themed terminal cells. No HTML execution or
//! remote resource loading; image destinations remain visible as text.
use crate::{
    render::{Grid, Style},
    theme,
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone)]
struct Span {
    text: String,
    style: Style,
}
type Line = Vec<Span>;

#[derive(Default)]
struct Table {
    align: Vec<Alignment>,
    rows: Vec<Vec<Line>>,
    row: Vec<Line>,
    cell: Line,
}

struct Preview {
    lines: Vec<Line>,
    line: Line,
    styles: Vec<Style>,
    lists: Vec<Option<u64>>,
    quote: usize,
    code: bool,
    destinations: Vec<String>,
    table: Option<Table>,
    width: usize,
}
impl Preview {
    fn style(&self) -> Style {
        self.styles.last().unwrap().clone()
    }
    fn text(&mut self, text: &str) {
        let style = self.style();
        if let Some(table) = &mut self.table {
            table.cell.push(Span {
                text: text.replace('\n', " "),
                style,
            });
            return;
        }
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                if self.code && self.line.is_empty() {
                    self.lines.push(vec![]);
                } else {
                    self.flush();
                }
            }
            if self.line.is_empty() && !part.is_empty() {
                let prefix = format!(
                    "{}{}",
                    "│ ".repeat(self.quote),
                    "  ".repeat(self.lists.len().saturating_sub(1))
                );
                if !prefix.is_empty() {
                    self.line.push(Span {
                        text: prefix,
                        style: theme::current().monitor_muted,
                    });
                }
            }
            if !part.is_empty() {
                self.line.push(Span {
                    text: part.into(),
                    style: style.clone(),
                });
            }
        }
    }
    fn flush(&mut self) {
        if !self.line.is_empty() {
            self.lines.push(std::mem::take(&mut self.line));
        }
    }
    fn blank(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(vec![]);
        }
    }
    fn render_table(&mut self) {
        let table = self.table.take().unwrap();
        let cols = table.align.len().max(1);
        let available = self.width.saturating_sub(cols.saturating_sub(1) * 3);
        let col_width = (available / cols).max(1);
        for (index, row) in table.rows.iter().enumerate() {
            let cells: Vec<_> = row.iter().map(|cell| wrap(cell, col_width)).collect();
            let height = cells.iter().map(Vec::len).max().unwrap_or(1);
            for y in 0..height {
                let mut line = vec![];
                for col in 0..cols {
                    if col > 0 {
                        line.push(Span {
                            text: " │ ".into(),
                            style: theme::current().monitor_muted,
                        });
                    }
                    let content = cells
                        .get(col)
                        .and_then(|c| c.get(y))
                        .cloned()
                        .unwrap_or_default();
                    let used: usize = content.iter().map(|s| s.text.width()).sum();
                    let padding = col_width.saturating_sub(used);
                    let left = match table.align.get(col) {
                        Some(Alignment::Right) => padding,
                        Some(Alignment::Center) => padding / 2,
                        _ => 0,
                    };
                    line.push(Span {
                        text: " ".repeat(left),
                        style: self.style(),
                    });
                    line.extend(content);
                    line.push(Span {
                        text: " ".repeat(padding - left),
                        style: self.style(),
                    });
                }
                self.lines.push(line);
            }
            if index == 0 {
                self.lines.push(vec![Span {
                    text: (0..cols)
                        .map(|_| "─".repeat(col_width))
                        .collect::<Vec<_>>()
                        .join("─┼─"),
                    style: theme::current().monitor_muted,
                }]);
            }
        }
        self.blank();
    }
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => {
                let mut style = self.style();
                match &tag {
                    Tag::Paragraph => {}
                    Tag::Heading { .. } => {
                        self.blank();
                        style = theme::current().monitor_accent;
                        style.bold = true;
                    }
                    Tag::Strong | Tag::TableHead => style.bold = true,
                    Tag::Emphasis => style.italic = true,
                    Tag::Strikethrough => style.strikethrough = true,
                    Tag::BlockQuote(_) => {
                        self.flush();
                        self.quote += 1;
                    }
                    Tag::List(start) => {
                        self.flush();
                        self.lists.push(*start);
                    }
                    Tag::Item => {
                        self.flush();
                        let bullet = match self.lists.last_mut() {
                            Some(Some(n)) => {
                                let value = format!("{n}. ");
                                *n = n.saturating_add(1);
                                value
                            }
                            _ => "• ".into(),
                        };
                        self.text(&bullet);
                    }
                    Tag::CodeBlock(kind) => {
                        self.blank();
                        if let CodeBlockKind::Fenced(language) = kind
                            && !language.is_empty()
                        {
                            self.text(&format!("[{language}]"));
                            self.flush();
                        }
                        self.code = true;
                        style = theme::current().hotkey;
                    }
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                        self.destinations.push(dest_url.to_string());
                        if matches!(tag, Tag::Image { .. }) {
                            self.text("[Image: ");
                        }
                        style.underline = true;
                    }
                    Tag::FootnoteDefinition(label) => {
                        self.blank();
                        self.text(&format!("[{label}] "));
                    }
                    Tag::Table(align) => {
                        self.blank();
                        self.table = Some(Table {
                            align: align.clone(),
                            ..Table::default()
                        });
                    }
                    _ => {}
                }
                self.styles.push(style);
            }
            Event::End(tag) => {
                match tag {
                    TagEnd::Paragraph => {
                        if self.table.is_none() {
                            if self.lists.is_empty() {
                                self.blank();
                            } else {
                                self.flush();
                            }
                        }
                    }
                    TagEnd::Heading(_) | TagEnd::FootnoteDefinition | TagEnd::HtmlBlock => {
                        self.blank()
                    }
                    TagEnd::CodeBlock => {
                        self.code = false;
                        self.blank();
                    }
                    TagEnd::BlockQuote(_) => {
                        self.flush();
                        self.quote = self.quote.saturating_sub(1);
                    }
                    TagEnd::List(_) => {
                        self.flush();
                        self.lists.pop();
                        if self.lists.is_empty() {
                            self.blank();
                        }
                    }
                    TagEnd::Item => self.flush(),
                    TagEnd::Link | TagEnd::Image => {
                        if tag == TagEnd::Image {
                            self.text("]");
                        }
                        if let Some(dest) = self.destinations.pop() {
                            self.text(&format!(" ({dest})"));
                        }
                    }
                    TagEnd::TableCell => {
                        if let Some(table) = &mut self.table {
                            table.row.push(std::mem::take(&mut table.cell));
                        }
                    }
                    TagEnd::TableHead | TagEnd::TableRow => {
                        if let Some(table) = &mut self.table {
                            table.rows.push(std::mem::take(&mut table.row));
                        }
                    }
                    TagEnd::Table => self.render_table(),
                    _ => {}
                }
                self.styles.pop();
            }
            Event::Text(text) => {
                if self.code {
                    self.text(&text.replace('\t', "    "));
                } else {
                    self.text(&text);
                }
            }
            Event::Code(text) => {
                let mut style = theme::current().hotkey;
                style.bold = self.style().bold;
                self.styles.push(style);
                self.text(&text);
                self.styles.pop();
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.blank();
                self.text(&"─".repeat(self.width));
                self.blank();
            }
            Event::TaskListMarker(done) => self.text(if done { "☑ " } else { "☐ " }),
            Event::FootnoteReference(label) => self.text(&format!("[{label}]")),
            // Raw HTML stays literal; terminal previews never execute markup.
            Event::Html(text) | Event::InlineHtml(text) => self.text(&text),
            Event::InlineMath(text) | Event::DisplayMath(text) => self.text(&text),
        }
    }
}

// Wrap styled text in terminal cells, retaining combining marks with their base.
fn wrap(line: &Line, width: usize) -> Vec<Line> {
    let mut out = vec![];
    let mut current: Line = vec![];
    let mut used = 0;
    for span in line {
        for word in span.text.split_inclusive(char::is_whitespace) {
            let word_width = word.trim_end().width();
            if used > 0 && word_width <= width && used + word_width > width {
                out.push(std::mem::take(&mut current));
                used = 0;
            }
            for c in word.chars().filter(|c| !c.is_control()) {
                let cells = c.width().unwrap_or(0);
                if used == width && c.is_whitespace() {
                    continue;
                }
                if cells > width {
                    continue;
                }
                if used + cells > width {
                    out.push(std::mem::take(&mut current));
                    used = 0;
                }
                if cells == 0 && current.is_empty() {
                    continue;
                }
                if let Some(last) = current.last_mut()
                    && last.style == span.style
                {
                    last.text.push(c);
                } else {
                    current.push(Span {
                        text: c.to_string(),
                        style: span.style.clone(),
                    });
                }
                used += cells;
            }
        }
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

fn layout(text: &str, width: usize) -> Vec<Line> {
    let mut preview = Preview {
        lines: vec![],
        line: vec![],
        styles: vec![theme::current().editor],
        lists: vec![],
        quote: 0,
        code: false,
        destinations: vec![],
        table: None,
        width: width.max(1),
    };
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    for event in Parser::new_ext(text, options) {
        preview.event(event);
    }
    preview.flush();
    preview
        .lines
        .iter()
        .flat_map(|line| wrap(line, width.max(1)))
        .collect()
}

pub fn draw(grid: &mut Grid, x: u16, y: u16, width: u16, height: u16, text: &str, top: usize) {
    if width == 0 || height == 0 {
        return;
    }
    for (row, line) in layout(text, width as usize)
        .iter()
        .skip(top)
        .take(height as usize)
        .enumerate()
    {
        let mut column = x;
        for span in line {
            // vt100 preserves combining marks but has no SGR-9 cell attribute.
            // Carry deletion strokes through the PTY as combining overlays.
            let text = if span.style.strikethrough {
                span.text
                    .chars()
                    .flat_map(|c| {
                        std::iter::once(c).chain(
                            (!c.is_whitespace() && c.width().unwrap_or(0) > 0).then_some('\u{336}'),
                        )
                    })
                    .collect::<String>()
            } else {
                span.text.clone()
            };
            // builtin::line_at attaches combining marks and clips whole wide glyphs.
            crate::builtin::line_at(
                grid,
                column,
                y + row as u16,
                &text.chars().collect::<Vec<_>>(),
                0,
                width.saturating_sub(column - x),
            );
            // line_at uses the editor style; apply this run's semantic style.
            let end = (column + span.text.width() as u16).min(x + width);
            for col in column..end {
                if let Some(cell) = grid
                    .cells
                    .get_mut((y as usize + row) * grid.cols as usize + col as usize)
                {
                    cell.style = span.style.clone();
                }
            }
            column = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plain(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn commonmark_and_extensions_keep_text_and_style() {
        let lines = layout(
            "# Heading\n\n**bold** *italic* ~~gone~~ `code` [link](https://example.org)\n\n3. ordered\n   - nested\n\n- [x] done\n\n> quote\n\n```rs\nlet x = 1;\n```\n\n![alt](pic.png)\n\nref[^a]\n\n[^a]: footnote",
            120,
        );
        let text = plain(&lines);
        for expected in [
            "Heading",
            "bold italic gone code link (https://example.org)",
            "3. ordered",
            "  • nested",
            "☑ done",
            "│ quote",
            "[rs]",
            "let x = 1;",
            "[Image: alt] (pic.png)",
            "[a] footnote",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        for (text, matches) in [("bold", 0), ("italic", 1), ("gone", 2)] {
            let span = lines.iter().flatten().find(|s| s.text == text).unwrap();
            assert!(match matches {
                0 => span.style.bold,
                1 => span.style.italic,
                _ => span.style.strikethrough,
            });
        }
        assert!(!text.contains("**"));
    }
    #[test]
    fn tables_align_and_unicode_wraps_within_cells() {
        let text = plain(&layout(
            "| Name | Count |\n| :--- | ---: |\n| 界界 | 7 |\n| cafe\u{301} | 12 |",
            25,
        ));
        assert!(text.contains("│"));
        assert!(text.contains("┼"));
        assert!(text.contains("界界"));
        assert!(text.contains("cafe\u{301}"));
        for width in [1, 2, 5, 25] {
            for line in layout("hello 界界 e\u{301} words and **styled text**", width) {
                assert!(line.iter().map(|s| s.text.width()).sum::<usize>() <= width);
            }
        }
    }
    #[test]
    fn draw_scrolls_rendered_rows_and_handles_tiny_sizes() {
        let mut grid = Grid::new(12, 4);
        draw(&mut grid, 1, 1, 8, 2, "**abcdefghijk**", 1);
        assert_eq!(grid.cells[13].text, "i");
        assert!(grid.cells[13].style.bold);
        for width in 0..4 {
            for height in 0..4 {
                draw(
                    &mut Grid::new(width, height),
                    0,
                    0,
                    width,
                    height,
                    "界\n\n| a | b |\n|---|---|\n| c | d |",
                    0,
                );
            }
        }
    }
}
