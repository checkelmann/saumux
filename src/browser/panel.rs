use crate::render::Grid;
use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

pub(super) struct Entry {
    pub name: std::ffi::OsString,
    pub path: PathBuf,
    pub directory: bool,
    pub size: Option<u64>,
}

pub(super) struct Panel {
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub top: usize,
    pub hidden: bool,
    pub marked: HashSet<usize>,
    pub last_click: Option<(usize, Instant)>,
}

impl Panel {
    pub fn new(path: &Path) -> Result<Self> {
        let cwd = fs::canonicalize(path).with_context(|| format!("opening {}", path.display()))?;
        let mut panel = Self {
            cwd,
            entries: vec![],
            selected: 0,
            top: 0,
            hidden: false,
            marked: HashSet::new(),
            last_click: None,
        };
        panel.refresh()?;
        Ok(panel)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let selected_path = self.entries.get(self.selected).map(|e| e.path.clone());
        let marked_paths: HashSet<PathBuf> = self
            .marked
            .iter()
            .filter_map(|&i| self.entries.get(i).map(|e| e.path.clone()))
            .collect();

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

        self.selected = selected_path
            .and_then(|path| entries.iter().position(|e| e.path == path))
            .unwrap_or(self.selected)
            .min(entries.len().saturating_sub(1));

        self.marked = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| marked_paths.contains(&e.path))
            .map(|(i, _)| i)
            .collect();

        self.entries = entries;
        self.top = self.top.min(self.selected);
        Ok(())
    }

    pub fn navigate(&mut self, path: &Path) -> Result<()> {
        let mut next = Self::new(path)?;
        next.hidden = self.hidden;
        next.refresh()?;
        *self = next;
        Ok(())
    }

    pub fn up(&mut self) -> Result<()> {
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            let previous = self.cwd.clone();
            self.navigate(&parent)?;
            if let Some(index) = self.entries.iter().position(|e| e.path == previous) {
                self.selected = index;
            }
        }
        Ok(())
    }

    pub fn toggle_hidden(&mut self) -> Result<()> {
        self.hidden = !self.hidden;
        self.refresh()
    }

    pub fn toggle_mark(&mut self) {
        if self
            .entries
            .get(self.selected)
            .is_some_and(|e| e.name != "..")
        {
            if !self.marked.remove(&self.selected) {
                self.marked.insert(self.selected);
            }
            self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
        }
    }

    pub fn mark_all(&mut self) {
        let all: HashSet<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name != "..")
            .map(|(i, _)| i)
            .collect();
        if self.marked == all {
            self.marked.clear();
        } else {
            self.marked = all;
        }
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// Marked entries, falling back to cursor if nothing is marked. Never includes `..`.
    pub fn marked_paths(&self) -> Vec<PathBuf> {
        if !self.marked.is_empty() {
            self.marked
                .iter()
                .filter_map(|&i| self.entries.get(i))
                .filter(|e| e.name != "..")
                .map(|e| e.path.clone())
                .collect()
        } else if let Some(e) = self.entries.get(self.selected) {
            if e.name != ".." {
                vec![e.path.clone()]
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }

    /// Open the selected entry. Returns `true` when a file was sent to the editor.
    pub fn open(&mut self) -> Result<bool> {
        let Some(entry) = self.entries.get(self.selected) else {
            return Ok(false);
        };
        let path = entry.path.clone();
        if entry.directory {
            self.navigate(&path)?;
            return Ok(false);
        }
        crate::editor::Document::open(Some(&path))?;
        let session =
            std::env::var("SAUMUX").context("the file browser runs inside a saumux session")?;
        let path_str = path.to_str().context("this file name is not UTF-8")?;
        crate::session::control(&session, "editor.open".into(), vec![path_str.into()])?;
        Ok(true)
    }

    pub fn draw(&mut self, width: u16, height: u16, active: bool) -> Grid {
        let visible = height.saturating_sub(2).max(1) as usize;
        if self.selected < self.top {
            self.top = self.selected;
        }
        if self.selected >= self.top + visible {
            self.top = self.selected + 1 - visible;
        }
        let theme = crate::theme::current();
        let mut grid = Grid::new(width, height);
        grid.fill(0, 0, width, height, " ", theme.text.clone());

        // Row 0: current path
        grid.text(
            0,
            0,
            &self.cwd.display().to_string(),
            width,
            theme.text.clone(),
        );

        // Rows 1..height-2: file list
        for (i, entry) in self.entries.iter().skip(self.top).take(visible).enumerate() {
            let idx = self.top + i;
            let is_selected = idx == self.selected;
            let is_marked = self.marked.contains(&idx);

            let row_style = if is_selected {
                if active {
                    theme.title.clone()
                } else {
                    theme.selected.clone()
                }
            } else {
                theme.text.clone()
            };

            let y = i as u16 + 1;
            grid.fill(0, y, width, 1, " ", row_style.clone());

            let prefix = if is_selected && active {
                '>'
            } else if is_marked {
                '*'
            } else if entry.directory {
                'D'
            } else {
                ' '
            };

            let name = format!(
                "{} {}{}",
                prefix,
                entry.name.to_string_lossy(),
                if entry.directory { "/" } else { "" }
            );

            let size_str = entry.size.map(super::format_size).unwrap_or_default();
            let reserve: u16 = if width > 24 { 10 } else { 0 };

            let name_style = if is_selected {
                row_style.clone()
            } else if entry.directory {
                theme.directory.clone()
            } else {
                theme.text.clone()
            };

            grid.text(0, y, &name, width.saturating_sub(reserve), name_style);

            if reserve > 0 && !entry.directory && !size_str.is_empty() {
                let col = width.saturating_sub(size_str.len() as u16 + 1);
                grid.text(col, y, &size_str, size_str.len() as u16, row_style);
            }
        }

        if self.entries.is_empty() {
            grid.text(
                1,
                1,
                "Empty directory",
                width.saturating_sub(2),
                theme.text.clone(),
            );
        }

        // Row height-1: status bar
        let status = if !self.marked.is_empty() {
            format!(
                " {} entries · {} marked · hidden {}",
                self.entries.len(),
                self.marked.len(),
                if self.hidden { "on" } else { "off" }
            )
        } else {
            format!(
                " {} entries · hidden {}",
                self.entries.len(),
                if self.hidden { "on" } else { "off" }
            )
        };
        let sy = height.saturating_sub(1);
        grid.fill(0, sy, width, 1, " ", theme.status.clone());
        grid.text(0, sy, &status, width, theme.status.clone());

        grid
    }
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
        let mut p = Panel::new(&root).unwrap();
        assert_eq!(p.entries[1].name, "folder");
        assert!(!p.entries.iter().any(|e| e.name == ".hidden"));
        p.toggle_hidden().unwrap();
        assert!(p.entries.iter().any(|e| e.name == ".hidden"));
        p.navigate(&root.join("folder")).unwrap();
        p.up().unwrap();
        assert_eq!(p.cwd, fs::canonicalize(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mark_toggle_and_mark_all() {
        let root = std::env::temp_dir().join(format!("sx-browser-mark-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), b"x").unwrap();
        fs::write(root.join("b.txt"), b"x").unwrap();
        let mut p = Panel::new(&root).unwrap();
        // entries: [..(0), a.txt(1), b.txt(2)]
        p.selected = 1;
        p.toggle_mark();
        assert!(p.marked.contains(&1));
        assert_eq!(p.selected, 2);

        p.toggle_mark();
        assert!(p.marked.contains(&1));
        assert!(p.marked.contains(&2));

        // mark_all when all already marked → clear
        p.mark_all();
        assert!(p.marked.is_empty());

        // mark_all when nothing marked → marks all non-..
        p.mark_all();
        assert!(!p.marked.contains(&0));
        assert!(p.marked.contains(&1));
        assert!(p.marked.contains(&2));

        // marked_paths excludes ..
        let paths = p.marked_paths();
        assert!(paths.iter().all(|path| !path.ends_with("..")));

        fs::remove_dir_all(root).unwrap();
    }
}
