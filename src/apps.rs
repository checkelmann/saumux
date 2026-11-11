//! Small bundled desktop apps. They run in ordinary PTYs and share live themes.
use crate::{
    builtin::{self, Surface},
    render::Grid,
    theme::Theme,
};
use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::{
    collections::VecDeque,
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::System;

pub fn notes_path() -> Result<PathBuf> {
    let root = match std::env::var_os("XDG_DATA_HOME") {
        Some(path) if !path.is_empty() && PathBuf::from(&path).is_absolute() => PathBuf::from(path),
        _ => PathBuf::from(
            std::env::var_os("HOME").context("HOME or XDG_DATA_HOME is required for Notes")?,
        )
        .join(".local/share"),
    };
    Ok(root.join("saumux/notes.txt"))
}
pub fn prepare_notes() -> Result<PathBuf> {
    let path = notes_path()?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path.parent().unwrap())?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error).context("creating notes file"),
    }
    crate::editor::Document::open(Some(&path))?;
    Ok(path)
}

const MONITOR_ACTIONS: &[(&str, &str)] = crate::monitor_view::ACTIONS;
const CLOCK_ACTIONS: &[(&str, &str)] = &[
    (" Local/UTC ", "zone"),
    (" 12/24h ", "format"),
    (" Close ", "close"),
];
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
struct Monitor {
    system: System,
    cpu: Option<f32>,
    cores: Vec<f32>,
    history: VecDeque<f32>,
    paused: bool,
    last_sample: Instant,
    samples: u64,
    core_offset: usize,
    host: String,
    os: String,
    uptime: u64,
    load: [f64; 3],
}
impl Monitor {
    fn new() -> Self {
        let mut system = System::new();
        system.refresh_memory();
        system.refresh_cpu_usage();
        let mut monitor = Self {
            system,
            cpu: None,
            cores: vec![],
            history: VecDeque::new(),
            paused: false,
            last_sample: Instant::now(),
            samples: 0,
            core_offset: 0,
            host: printable(&System::host_name().unwrap_or_else(|| "Host".into())),
            os: printable(
                &System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
            ),
            uptime: 0,
            load: [0.0; 3],
        };
        monitor.metadata();
        monitor
    }
    fn metadata(&mut self) {
        self.uptime = System::uptime();
        let load = System::load_average();
        self.load = [load.one, load.five, load.fifteen];
    }
    fn refresh(&mut self) {
        if self.last_sample.elapsed() < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
            return;
        }
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        let cpu = self.system.global_cpu_usage().clamp(0.0, 100.0);
        self.cpu = Some(cpu);
        self.cores = self
            .system
            .cpus()
            .iter()
            .map(|c| c.cpu_usage().clamp(0.0, 100.0))
            .collect();
        if self.history.len() == 60 {
            self.history.pop_front();
        }
        self.history.push_back(cpu);
        self.metadata();
        self.samples += 1;
        self.last_sample = Instant::now();
    }
    fn pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            // Discard time spent paused rather than presenting it as one live sample.
            self.system.refresh_cpu_usage();
            self.cpu = None;
            self.last_sample = Instant::now();
        }
    }
    fn draw(&self, width: u16, height: u16, theme: &Theme) -> Grid {
        let history: Vec<_> = self.history.iter().copied().collect();
        crate::monitor_view::draw(
            width,
            height,
            &crate::monitor_view::Snapshot {
                host: &self.host,
                os: &self.os,
                cpu: self.cpu,
                used: self.system.used_memory(),
                total: self.system.total_memory(),
                swap_used: self.system.used_swap(),
                swap_total: self.system.total_swap(),
                cores: &self.cores,
                cpu_count: self.system.cpus().len(),
                history: &history,
                paused: self.paused,
                samples: self.samples,
                uptime: self.uptime,
                load: self.load,
                core_offset: self.core_offset,
            },
            theme,
        )
    }
    fn scroll(&mut self, delta: isize, width: u16, height: u16) {
        let (capacity, _) = crate::monitor_view::core_capacity(width, height);
        self.core_offset = self
            .core_offset
            .saturating_add_signed(delta)
            .min(self.cores.len().saturating_sub(capacity));
    }
}

fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(120).collect()
}
fn background(width: u16, height: u16, theme: &Theme) -> Grid {
    let mut grid = Grid::new(width, height);
    grid.fill(0, 0, width, height, " ", theme.text.clone());
    grid
}
fn close_key(event: &Event) -> bool {
    matches!(event,Event::Key(key) if key.kind!=KeyEventKind::Release && (key.code==KeyCode::Esc || key.code==KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL)))
}
fn click(event: &Event, actions: &[(&str, &str)]) -> Option<String> {
    if let Event::Mouse(mouse) = event
        && mouse.kind == MouseEventKind::Down(MouseButton::Left)
        && mouse.row == 0
    {
        return builtin::action_at(mouse.column, actions).map(str::to_string);
    }
    None
}
pub fn monitor() -> Result<()> {
    let mut monitor = Monitor::new();
    let mut surface = Surface::enter()?;
    loop {
        if !monitor.paused
            && monitor.last_sample.elapsed()
                >= SAMPLE_INTERVAL.max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
        {
            monitor.refresh();
        }
        let (width, height) = surface.size()?;
        surface.draw(
            monitor.draw(width, height, &crate::theme::current()),
            "System Monitor",
        )?;
        let Some(event) = surface.event()? else {
            continue;
        };
        if close_key(&event) {
            break;
        }
        let (capacity, columns) = crate::monitor_view::core_capacity(width, height);
        let action = click(&event, MONITOR_ACTIONS).or_else(|| match &event {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Char(' ') => Some("pause".into()),
                KeyCode::Char('r' | 'R') => Some("refresh".into()),
                KeyCode::Down => {
                    monitor.scroll(columns as isize, width, height);
                    None
                }
                KeyCode::Up => {
                    monitor.scroll(-(columns as isize), width, height);
                    None
                }
                KeyCode::PageDown => {
                    monitor.scroll(capacity as isize, width, height);
                    None
                }
                KeyCode::PageUp => {
                    monitor.scroll(-(capacity as isize), width, height);
                    None
                }
                KeyCode::Home => {
                    monitor.core_offset = 0;
                    None
                }
                KeyCode::End => {
                    monitor.core_offset = monitor.cores.len().saturating_sub(capacity);
                    None
                }
                _ => None,
            },
            _ => None,
        });
        if let Event::Mouse(mouse) = event {
            match mouse.kind {
                MouseEventKind::ScrollDown => monitor.scroll(columns as isize, width, height),
                MouseEventKind::ScrollUp => monitor.scroll(-(columns as isize), width, height),
                _ => {}
            }
        }
        match action.as_deref() {
            Some("close") => break,
            Some("pause") => monitor.pause(),
            Some("refresh") => monitor.refresh(),
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Stamp {
    hour: i32,
    minute: i32,
    second: i32,
    year: i32,
    month: i32,
    day: i32,
    weekday: usize,
}
fn stamp(seconds: u64, utc: bool) -> Result<Stamp> {
    // Infer the platform time representation; naming libc::time_t warns on musl.
    let seconds = seconds.try_into().context("clock timestamp out of range")?;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: timestamp and output pointers are valid; both reentrant functions
    // initialize the output on success and use the server host's timezone.
    let result = unsafe {
        if utc {
            libc::gmtime_r(&seconds, tm.as_mut_ptr())
        } else {
            libc::localtime_r(&seconds, tm.as_mut_ptr())
        }
    };
    if result.is_null() {
        anyhow::bail!("clock conversion failed");
    }
    // SAFETY: the conversion above succeeded and initialized tm.
    let tm = unsafe { tm.assume_init() };
    Ok(Stamp {
        hour: tm.tm_hour,
        minute: tm.tm_min,
        second: tm.tm_sec,
        year: tm.tm_year + 1900,
        month: tm.tm_mon + 1,
        day: tm.tm_mday,
        weekday: tm.tm_wday.clamp(0, 6) as usize,
    })
}
struct Clock {
    utc: bool,
    twelve: bool,
}
impl Clock {
    fn draw(&self, width: u16, height: u16, seconds: u64, theme: &Theme) -> Result<Grid> {
        let now = stamp(seconds, self.utc)?;
        let mut grid = background(width, height, theme);
        builtin::toolbar(&mut grid, 0, CLOCK_ACTIONS);
        let hour = if self.twelve {
            (now.hour + 11) % 12 + 1
        } else {
            now.hour
        };
        let time = format!("{hour:02}:{:02}:{:02}", now.minute, now.second);
        let zone = if self.utc { "UTC" } else { "Local time (host)" };
        let suffix = if self.twelve {
            if now.hour < 12 { " AM" } else { " PM" }
        } else {
            ""
        };
        let large = width >= 33 && height >= 11;
        if large {
            let x = width.saturating_sub(31) / 2;
            for (index, c) in time.chars().enumerate() {
                for (row, line) in digit(c).iter().enumerate() {
                    for (col, filled) in line.chars().enumerate() {
                        if filled == '#' {
                            grid.put(
                                x + index as u16 * 4 + col as u16,
                                2 + row as u16,
                                "█",
                                1,
                                theme.directory.clone(),
                            );
                        }
                    }
                }
            }
        } else {
            center(
                &mut grid,
                2,
                &format!("{time}{suffix}"),
                theme.directory.clone(),
            );
        }
        let y = if large { 8 } else { 4 };
        let weekday = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ][now.weekday];
        center(
            &mut grid,
            y,
            &format!("{weekday}  {:04}-{:02}-{:02}", now.year, now.month, now.day),
            theme.text.clone(),
        );
        center(
            &mut grid,
            y + 1,
            &format!("{zone}{suffix}"),
            theme.text.clone(),
        );
        builtin::bar(
            &mut grid,
            height.saturating_sub(1),
            "U local/UTC · T 12/24h · Ctrl+Q close",
        );
        Ok(grid)
    }
}
fn center(grid: &mut Grid, y: u16, text: &str, style: crate::render::Style) {
    let width = unicode_width::UnicodeWidthStr::width(text) as u16;
    grid.text(
        grid.cols.saturating_sub(width) / 2,
        y,
        text,
        grid.cols,
        style,
    );
}
fn digit(c: char) -> [&'static str; 5] {
    match c {
        '0' => ["###", "# #", "# #", "# #", "###"],
        '1' => [" # ", "## ", " # ", " # ", "###"],
        '2' => ["###", "  #", "###", "#  ", "###"],
        '3' => ["###", "  #", "###", "  #", "###"],
        '4' => ["# #", "# #", "###", "  #", "  #"],
        '5' => ["###", "#  ", "###", "  #", "###"],
        '6' => ["###", "#  ", "###", "# #", "###"],
        '7' => ["###", "  #", "  #", "  #", "  #"],
        '8' => ["###", "# #", "###", "# #", "###"],
        '9' => ["###", "# #", "###", "  #", "###"],
        _ => ["   ", " # ", "   ", " # ", "   "],
    }
}
pub fn clock() -> Result<()> {
    let mut clock = Clock {
        utc: false,
        twelve: false,
    };
    let mut surface = Surface::enter()?;
    loop {
        let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let (width, height) = surface.size()?;
        surface.draw(
            clock.draw(width, height, seconds, &crate::theme::current())?,
            "Clock",
        )?;
        let Some(event) = surface.event()? else {
            continue;
        };
        if close_key(&event) {
            break;
        }
        let action = click(&event, CLOCK_ACTIONS).or_else(|| match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Char('u' | 'U') => Some("zone".into()),
                KeyCode::Char('t' | 'T') => Some("format".into()),
                _ => None,
            },
            _ => None,
        });
        match action.as_deref() {
            Some("close") => break,
            Some("zone") => clock.utc = !clock.utc,
            Some("format") => clock.twelve = !clock.twelve,
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_renders_utc_midnight_and_noon_and_clips_tiny_windows() {
        let clock = Clock {
            utc: true,
            twelve: true,
        };
        let epoch = stamp(0, true).unwrap();
        assert_eq!(
            (epoch.year, epoch.month, epoch.day, epoch.hour),
            (1970, 1, 1, 0)
        );
        for (seconds, suffix) in [(0, "AM"), (43200, "PM")] {
            let grid = clock.draw(26, 10, seconds, &Theme::default()).unwrap();
            let text: String = grid.cells.iter().map(|c| c.text.as_str()).collect();
            assert!(text.contains("12:00:00"));
            assert!(text.contains(suffix));
            assert!(text.contains("UTC"));
        }
        for (w, h) in [(1, 1), (8, 3), (50, 15)] {
            clock.draw(w, h, 0, &Theme::default()).unwrap();
        }
    }
}
