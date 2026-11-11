//! Cell-grid presentation for the system monitor, independent of live sampling.
use crate::{
    builtin,
    render::{Grid, Style},
    theme::Theme,
};

pub const ACTIONS: &[(&str, &str)] = &[
    (" Pause  ", "pause"),
    (" Refresh ", "refresh"),
    (" Close ", "close"),
];
const PAUSED_ACTIONS: &[(&str, &str)] = &[
    (" Resume ", "pause"),
    (" Refresh ", "refresh"),
    (" Close ", "close"),
];
#[derive(Default)]
pub struct Snapshot<'a> {
    pub host: &'a str,
    pub os: &'a str,
    pub cpu: Option<f32>,
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub cores: &'a [f32],
    pub cpu_count: usize,
    pub history: &'a [f32],
    pub paused: bool,
    pub samples: u64,
    pub uptime: u64,
    pub load: [f64; 3],
    pub core_offset: usize,
}
struct Layout {
    x: u16,
    w: u16,
    chart_height: u16,
    core_y: u16,
    rows: usize,
    cols: usize,
}
impl Layout {
    fn new(width: u16, height: u16) -> Self {
        let w = width.saturating_sub(4).min(110);
        let chart_height = height.saturating_sub(17).clamp(2, 10);
        let core_y = 12 + chart_height;
        let rows = height.saturating_sub(core_y + 1) as usize;
        let cols = if w >= 96 {
            3
        } else if w >= 48 {
            2
        } else {
            1
        };
        Self {
            x: width.saturating_sub(w) / 2,
            w,
            chart_height,
            core_y,
            rows,
            cols,
        }
    }
}
pub fn core_capacity(width: u16, height: u16) -> (usize, usize) {
    let layout = Layout::new(width, height);
    ((layout.rows * layout.cols).max(1), layout.cols)
}
fn text(grid: &mut Grid, x: u16, y: u16, s: &str, w: u16, style: &Style) {
    grid.text(x, y, s, w, style.clone());
}
fn section(grid: &mut Grid, x: u16, y: u16, w: u16, label: &str, theme: &Theme) {
    grid.text(x, y, &"─".repeat(w as usize), w, theme.monitor_grid.clone());
    text(grid, x, y, &format!(" {label} "), w, &theme.monitor_muted);
}
pub fn draw(width: u16, height: u16, s: &Snapshot<'_>, theme: &Theme) -> Grid {
    let mut grid = Grid::new(width, height);
    grid.fill(0, 0, width, height, " ", theme.monitor_text.clone());
    builtin::toolbar(
        &mut grid,
        0,
        if s.paused { PAUSED_ACTIONS } else { ACTIONS },
    );
    let l = Layout::new(width, height);
    let x = l.x;
    let w = l.w;
    let state = if s.paused { "Paused" } else { "Live · 1s" };
    let title = format!("System Monitor · {}", s.host);
    let reserve = state.chars().count() as u16 + 2;
    text(
        &mut grid,
        x,
        1,
        &title,
        w.saturating_sub(reserve),
        &theme.monitor_text,
    );
    if w > reserve {
        text(
            &mut grid,
            x + w - reserve + 2,
            1,
            state,
            reserve - 2,
            &theme.monitor_accent,
        );
    }
    let meta = format!("{} · {} logical CPUs", s.os, s.cpu_count);
    text(&mut grid, x, 2, &meta, w, &theme.monitor_muted);
    if height < 10 || width < 30 {
        text(
            &mut grid,
            x,
            4,
            &format!(
                "CPU {}",
                s.cpu
                    .map(|v| format!("{v:.1}%"))
                    .unwrap_or_else(|| "sampling…".into())
            ),
            w,
            &theme.monitor_accent,
        );
        text(
            &mut grid,
            x,
            5,
            &format!("RAM {} / {}", bytes(s.used), bytes(s.total)),
            w,
            &theme.monitor_text,
        );
    } else {
        let cpu_detail = format!("{} logical CPUs", s.cpu_count);
        let ram_detail = format!("{} / {}", bytes(s.used), bytes(s.total));
        let swap_detail = if s.swap_total == 0 {
            "Not configured".into()
        } else {
            format!("{} / {}", bytes(s.swap_used), bytes(s.swap_total))
        };
        let detail_width = [cpu_detail.len(), ram_detail.len(), swap_detail.len()]
            .into_iter()
            .max()
            .unwrap_or(0) as u16;
        let bar_width = w.saturating_sub(15 + detail_width).clamp(2, 24);
        for (row, label, value, detail) in [
            (4, "CPU", s.cpu, cpu_detail),
            (5, "RAM", Some(percent(s.used, s.total)), ram_detail),
            (
                6,
                "Swap",
                (s.swap_total > 0).then(|| percent(s.swap_used, s.swap_total)),
                swap_detail,
            ),
        ] {
            text(&mut grid, x, row, label, 5, &theme.monitor_muted);
            let value_text = value
                .map(|v| format!("{v:5.1}%"))
                .unwrap_or_else(|| "    --".into());
            text(&mut grid, x + 5, row, &value_text, 7, &theme.monitor_text);
            if w >= 45 {
                meter(
                    &mut grid,
                    x + 13,
                    row,
                    bar_width,
                    value.unwrap_or(0.0),
                    theme,
                );
                text(
                    &mut grid,
                    x + 15 + bar_width,
                    row,
                    &detail,
                    w.saturating_sub(15 + bar_width),
                    &theme.monitor_muted,
                );
            } else {
                text(
                    &mut grid,
                    x + 13,
                    row,
                    &detail,
                    w.saturating_sub(13),
                    &theme.monitor_muted,
                );
            }
        }
        let context = format!(
            "Load 1/5/15m  {:.2} / {:.2} / {:.2} · Up {}d {:02}h {:02}m",
            s.load[0],
            s.load[1],
            s.load[2],
            s.uptime / 86400,
            s.uptime / 3600 % 24,
            s.uptime / 60 % 60
        );
        text(&mut grid, x, 7, &context, w, &theme.monitor_muted);
        section(&mut grid, x, 8, w, "CPU history · 60 samples", theme);
        if height >= 16 && w >= 24 {
            chart(&mut grid, x, 9, w, l.chart_height, s.history, theme);
            let label_y = 9 + l.chart_height;
            text(
                &mut grid,
                x + 5,
                label_y,
                "59 samples ago",
                w.saturating_sub(5),
                &theme.monitor_muted,
            );
            text(
                &mut grid,
                x + w.saturating_sub(3),
                label_y,
                "now",
                3,
                &theme.monitor_muted,
            );
        } else {
            text(
                &mut grid,
                x,
                9,
                if s.history.is_empty() {
                    "Waiting for first sample…"
                } else {
                    "Enlarge window to see the chart"
                },
                w,
                &theme.monitor_muted,
            );
        }
        if l.rows > 0 && height >= 16 {
            let count = s.cores.len();
            let capacity = l.rows * l.cols;
            let offset = s.core_offset.min(count.saturating_sub(capacity));
            let shown = count.saturating_sub(offset).min(capacity);
            let heading = if shown > 0 {
                format!(
                    "Logical CPUs · {}–{} of {count}",
                    offset + 1,
                    offset + shown
                )
            } else {
                "Logical CPUs · sampling…".into()
            };
            section(&mut grid, x, l.core_y - 1, w, &heading, theme);
            let col_width = w / l.cols as u16;
            for (i, usage) in s.cores.iter().enumerate().skip(offset).take(capacity) {
                let position = i - offset;
                let cx = x + (position % l.cols) as u16 * col_width;
                let cy = l.core_y + (position / l.cols) as u16;
                text(
                    &mut grid,
                    cx,
                    cy,
                    &format!("{:>3}", i + 1),
                    4,
                    &theme.monitor_muted,
                );
                let meter_width = col_width.saturating_sub(13).min(18);
                if meter_width > 0 {
                    meter(&mut grid, cx + 5, cy, meter_width, *usage, theme);
                }
                text(
                    &mut grid,
                    cx + 5 + meter_width,
                    cy,
                    &format!(" {usage:5.1}%"),
                    7,
                    &theme.monitor_text,
                );
            }
        }
    }
    let prefix = format!(
        "{} · sample {}",
        if s.paused { "Paused" } else { "Live" },
        s.samples
    );
    let toggle = if s.paused {
        "Space resume"
    } else {
        "Space pause"
    };
    let hints = [
        format!("{toggle} · PgUp/PgDn cores · Ctrl+Q close"),
        format!("{toggle} · ↑↓ cores · ^Q close"),
        format!("{toggle} · ^Q close"),
        "^Q close".into(),
        String::new(),
    ];
    let hint = hints
        .iter()
        .find(|hint| prefix.chars().count() + 2 + hint.chars().count() <= w as usize)
        .map(String::as_str)
        .unwrap_or("");
    let footer = if hint.is_empty() {
        prefix
    } else {
        format!("{prefix}  {hint}")
    };
    // No bright full-width status strip competing with the data.
    grid.fill(
        0,
        height.saturating_sub(1),
        width,
        1,
        " ",
        theme.monitor_grid.clone(),
    );
    text(
        &mut grid,
        x,
        height.saturating_sub(1),
        &footer,
        width.saturating_sub(x),
        &theme.monitor_muted,
    );
    grid
}
fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", value as f64 / 1073741824.0)
    } else {
        format!("{:.0} MiB", value as f64 / 1048576.0)
    }
}
fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as f32
    }
}
fn meter(grid: &mut Grid, x: u16, y: u16, width: u16, value: f32, theme: &Theme) {
    text(
        grid,
        x,
        y,
        &"─".repeat(width as usize),
        width,
        &theme.monitor_grid,
    );
    let filled = (value.clamp(0.0, 100.0) / 100.0 * width as f32).round() as u16;
    text(
        grid,
        x,
        y,
        &"━".repeat(filled as usize),
        width,
        &theme.monitor_accent,
    );
}
fn chart(grid: &mut Grid, x: u16, y: u16, width: u16, height: u16, history: &[f32], theme: &Theme) {
    let plot_x = x + 5;
    let w = width.saturating_sub(5);
    if w == 0 || height == 0 {
        return;
    }
    for (row, label) in [(0, "100%"), (height / 2, " 50%"), (height - 1, "  0%")] {
        text(grid, x, y + row, label, 4, &theme.monitor_muted);
        for col in (0..w).step_by(4) {
            text(grid, plot_x + col, y + row, "·", 1, &theme.monitor_grid);
        }
    }
    if history.is_empty() {
        text(
            grid,
            plot_x + 2,
            y + height / 2,
            "Waiting for first sample…",
            w.saturating_sub(2),
            &theme.monitor_muted,
        );
        return;
    }
    let mut pixels = vec![0u8; w as usize * height as usize];
    let samples = &history[history.len().saturating_sub(60)..];
    let px_width = w as i32 * 2;
    let px_height = height as i32 * 4;
    let point = |i: usize, value: f32| {
        (
            (59 - (samples.len() - 1 - i)) as i32 * (px_width - 1) / 59,
            ((100.0 - value.clamp(0.0, 100.0)) / 100.0 * (px_height - 1) as f32).round() as i32,
        )
    };
    let mut previous = None;
    for (i, value) in samples.iter().enumerate() {
        let p = point(i, *value);
        let (mut ax, mut ay) = previous.unwrap_or(p);
        let (bx, by) = p;
        let dx = (bx - ax).abs();
        let dy = -(by - ay).abs();
        let sx = if ax < bx { 1 } else { -1 };
        let sy = if ay < by { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            let bits = [[0, 1, 2, 6], [3, 4, 5, 7]];
            let cell = (ay / 4) as usize * w as usize + (ax / 2) as usize;
            pixels[cell] |= 1 << bits[(ax % 2) as usize][(ay % 4) as usize];
            if (ax, ay) == (bx, by) {
                break;
            }
            let e = 2 * error;
            if e >= dy {
                error += dy;
                ax += sx;
            }
            if e <= dx {
                error += dx;
                ay += sy;
            }
        }
        previous = Some(p);
    }
    for row in 0..height {
        for col in 0..w {
            let bits = pixels[row as usize * w as usize + col as usize];
            if bits != 0 {
                grid.put(
                    plot_x + col,
                    y + row,
                    &char::from_u32(0x2800 + bits as u32).unwrap().to_string(),
                    1,
                    theme.monitor_accent.clone(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture<'a>(cores: &'a [f32], history: &'a [f32]) -> Snapshot<'a> {
        Snapshot {
            host: "demo",
            os: "Linux",
            cpu: Some(25.0),
            used: 12 << 30,
            total: 16 << 30,
            swap_total: 0,
            cores,
            cpu_count: cores.len(),
            history,
            samples: 60,
            ..Snapshot::default()
        }
    }
    fn contents(grid: &Grid) -> String {
        grid.cells
            .chunks(grid.cols as usize)
            .map(|row| row.iter().map(|c| c.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn chart_layouts_and_core_pages_are_bounded_and_themed() {
        let cores = vec![42.0; 128];
        let history = vec![25.0; 60];
        let mut s = fixture(&cores, &history);
        for id in crate::theme::ThemeId::ALL {
            let theme = Theme::named(id);
            let grid = draw(90, 30, &s, &theme);
            let text = contents(&grid);
            assert!(text.contains("100%") && text.contains("50%") && text.contains("0%"));
            assert!(text.contains("59 samples ago") && text.contains("now"));
            assert!(!text.contains('░') && !text.contains('█'));
            assert!(grid.cells.iter().any(|c| {
                c.text
                    .chars()
                    .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
            }));
            assert_eq!(grid.cells[90 + 2].style.bg, theme.monitor_text.bg);
        }
        s.core_offset = 120;
        assert!(contents(&draw(66, 22, &s, &Theme::default())).contains("121–128 of 128"));
        for (value, expected_row) in [(0.0, 15), (100.0, 9)] {
            let history = vec![value; 60];
            let snapshot = fixture(&cores, &history);
            let grid = draw(70, 24, &snapshot, &Theme::default());
            let rows: Vec<_> = grid
                .cells
                .chunks(70)
                .enumerate()
                .filter_map(|(row, cells)| {
                    cells
                        .iter()
                        .any(|cell| {
                            cell.text
                                .chars()
                                .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                        })
                        .then_some(row)
                })
                .collect();
            assert_eq!(rows, vec![expected_row], "chart scale for {value}%");
        }
        for (w, h) in [(1, 1), (29, 9), (48, 14), (66, 16), (110, 40)] {
            draw(w, h, &s, &Theme::default());
        }
        let waiting = draw(66, 22, &Snapshot::default(), &Theme::default());
        assert!(contents(&waiting).contains("Waiting for first sample"));
        assert_eq!(percent(5, 0), 0.0);
    }
}
