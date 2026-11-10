use crate::{
    commands::{self, COMMANDS, MENUS},
    config::Config,
    geometry::{Rect, tile},
    input,
    render::{Grid, Style},
    session::ServerEvent,
    window::Window,
};
use anyhow::{Context, Result, bail};
use base64::Engine;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::SyncSender,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct MenuEntry {
    pub label: String,
    pub command: String,
    pub shortcut: String,
}
pub struct Menu {
    pub index: usize,
    pub selected: usize,
}
#[derive(Clone, Copy)]
enum DragKind {
    Move,
    Resize {
        left: bool,
        top: bool,
        right: bool,
        bottom: bool,
    },
}
struct Drag {
    window: u64,
    owner: u64,
    kind: DragKind,
    anchor: (u16, u16),
    original: Rect,
    preview: Rect,
    keyboard: bool,
}
struct Selection {
    window: u64,
    start: (u16, u16),
    end: (u16, u16),
    keyboard: bool,
    dragging: bool,
}
struct ThemePicker {
    selected: usize,
}
#[derive(Serialize, Deserialize)]
struct Layout {
    windows: Vec<LayoutWindow>,
}
#[derive(Serialize, Deserialize)]
struct LayoutWindow {
    rect: Rect,
    command: Vec<String>,
    cwd: PathBuf,
    workspace: usize,
    shelved: bool,
    color: bool,
}

pub struct App {
    pub name: String,
    pub windows: Vec<Window>,
    pub cols: u16,
    pub rows: u16,
    pub workspace: usize,
    pub config: Config,
    pub running: bool,
    pub detach: Option<u64>,
    pub clipboard_output: Option<(u64, String)>,
    pub status: String,
    pub menu: Option<Menu>,
    pub help: bool,
    pub confirmation: bool,
    pub list: Option<usize>,
    theme_picker: Option<ThemePicker>,
    pointers: HashMap<u64, (u16, u16)>,
    next_id: u64,
    drag: Option<Drag>,
    selection: Option<Selection>,
    clipboard: String,
    last_title_click: Option<(u64, u64, Instant)>,
    compact_restore: HashMap<u64, Rect>,
    events: SyncSender<ServerEvent>,
}

impl App {
    pub fn new(name: String, config: Config, events: SyncSender<ServerEvent>) -> Self {
        Self {
            name,
            windows: vec![],
            cols: 80,
            rows: 24,
            workspace: 0,
            config,
            running: true,
            detach: None,
            clipboard_output: None,
            status: String::new(),
            menu: None,
            help: false,
            confirmation: false,
            list: None,
            theme_picker: None,
            pointers: HashMap::new(),
            next_id: 1,
            drag: None,
            selection: None,
            clipboard: String::new(),
            last_title_click: None,
            compact_restore: HashMap::new(),
            events,
        }
    }
    pub fn active(&self) -> Option<&Window> {
        self.windows
            .iter()
            .rev()
            .find(|w| w.workspace == self.workspace && !w.shelved)
    }
    fn active_mut(&mut self) -> Option<&mut Window> {
        self.windows
            .iter_mut()
            .rev()
            .find(|w| w.workspace == self.workspace && !w.shelved)
    }
    pub fn active_id(&self) -> Option<u64> {
        self.active().map(|w| w.id)
    }
    pub fn compact(&self) -> bool {
        self.cols < 60 || self.rows < 16
    }
    pub fn shell() -> Vec<String> {
        vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())]
    }
    pub fn new_window(&mut self, command: Vec<String>, cwd: Option<PathBuf>) -> Result<u64> {
        if self.windows.len() >= 64 {
            bail!("a session can have at most 64 windows");
        }
        if command.is_empty() {
            bail!("missing program");
        }
        let offset = (self.windows.len() % 6) as u16;
        let rect = Rect {
            x: 2 + offset * 3,
            y: 2 + offset,
            w: self.cols.saturating_sub(12).min(100),
            h: self.rows.saturating_sub(6).min(30),
        }
        .fit(self.cols, self.rows);
        let id = self.next_id;
        self.next_id += 1;
        let cwd = cwd.unwrap_or_else(|| {
            self.active()
                .map(current_cwd)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
        });
        let mut window = Window::spawn(id, rect, command, cwd, &self.name, self.events.clone())?;
        window.workspace = self.workspace;
        window.color = self.config.look.content == "color"
            || window.is_editor()
            || window.command.get(1).is_some_and(|s| s == "__files");
        if self.compact() {
            self.compact_restore.insert(id, rect);
            window.resize(Rect::desktop(self.cols, self.rows))?;
        }
        self.windows.push(window);
        Ok(id)
    }
    fn focus(&mut self, id: u64) {
        if let Some(i) = self.windows.iter().position(|w| w.id == id) {
            let mut w = self.windows.remove(i);
            w.shelved = false;
            w.activity = false;
            self.workspace = w.workspace;
            self.windows.push(w);
            self.selection = None;
        }
    }
    pub fn remove(&mut self, id: u64) {
        self.windows.retain(|w| w.id != id);
        self.compact_restore.remove(&id);
        if self.drag.as_ref().is_some_and(|d| d.window == id) {
            self.drag = None;
        }
        if self.selection.as_ref().is_some_and(|s| s.window == id) {
            self.selection = None;
        }
        if self.windows.is_empty() {
            self.running = false;
        }
    }
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        let was_compact = self.compact();
        self.cols = cols;
        self.rows = rows;
        let compact = self.compact();
        for w in &mut self.windows {
            if compact && !was_compact {
                self.compact_restore.insert(w.id, w.rect);
            }
            let rect = if compact || w.restore.is_some() {
                Rect::desktop(cols, rows)
            } else if was_compact {
                self.compact_restore
                    .remove(&w.id)
                    .unwrap_or(w.rect)
                    .fit(cols, rows)
            } else {
                Rect {
                    x: w.rect.x.min(cols.saturating_sub(4)),
                    y: w.rect.y.clamp(1, rows.saturating_sub(3).max(1)),
                    ..w.rect
                }
            };
            w.resize(rect)?;
        }
        self.drag = None;
        self.selection = None;
        Ok(())
    }
    pub fn disconnected(&mut self, client: u64) {
        self.pointers.remove(&client);
        if self.drag.as_ref().is_some_and(|d| d.owner == client) {
            self.drag = None;
        }
    }
    pub fn menu_entries(&self, index: usize) -> Vec<MenuEntry> {
        let mut entries: Vec<_> = COMMANDS
            .iter()
            .filter(|c| c.menu == index)
            .map(|c| MenuEntry {
                label: if c.name == "theme.set" {
                    format!("Colour theme… ({})", self.config.look.theme.label())
                } else {
                    c.label.into()
                },
                command: c.name.into(),
                shortcut: commands::shortcut(c.name, &self.config),
            })
            .collect();
        if let Some(launchers) = self.config.menu.get(&MENUS[index].to_lowercase()) {
            entries.extend(launchers.iter().enumerate().map(|(i, l)| MenuEntry {
                label: l.label.clone(),
                command: format!("launcher.{index}.{i}"),
                shortcut: String::new(),
            }));
        }
        entries
    }
    pub fn command(&mut self, name: &str, args: &[String], client: u64) -> Result<Value> {
        if let Some(suffix) = name.strip_prefix("launcher.") {
            let (menu, index) = suffix.split_once('.').context("invalid launcher")?;
            let menu = menu.parse::<usize>()?;
            let index = index.parse::<usize>()?;
            let launcher = self
                .config
                .menu
                .get(
                    MENUS
                        .get(menu)
                        .context("invalid menu")?
                        .to_lowercase()
                        .as_str(),
                )
                .and_then(|l| l.get(index))
                .context("unknown launcher")?
                .clone();
            self.new_window(vec!["/bin/sh".into(), "-lc".into(), launcher.run], None)?;
            return Ok(json!(null));
        }
        match name {
            "theme.set"=>{
                if let Some(name)=args.first() {
                    if args.len()!=1 {bail!("theme.set expects one theme name");}
                    self.apply_theme(name.parse()?)?;
                    self.theme_picker=None;
                } else {self.theme_picker=Some(ThemePicker {selected:crate::theme::ThemeId::ALL.iter().position(|&id|id==self.config.look.theme).unwrap_or(0)});}
            }
            "theme.next"=>{
                let ids=crate::theme::ThemeId::ALL;
                let index=ids.iter().position(|&id|id==self.config.look.theme).unwrap_or(0);
                self.apply_theme(ids[(index+1)%ids.len()])?;
            }
            "theme.list"=>return Ok(json!(crate::theme::ThemeId::ALL.map(|id|json!({"id":id.id(),"label":id.label(),"description":id.description(),"active":id==self.config.look.theme})))),
            "browser.open"|"editor.new"|"editor.open" => {
                let kind=if name=="browser.open"||(name=="editor.open"&&args.is_empty()) {"__files"} else {"__edit"};
                let mut paths=args.to_vec();
                if let Some(path)=paths.first_mut() {
                    let requested=PathBuf::from(&*path);
                    let cwd=self.active().map(current_cwd).unwrap_or(std::env::current_dir()?);
                    let absolute=if requested.is_absolute() {requested} else {cwd.join(requested)};
                    if kind=="__edit" {crate::editor::Document::open(Some(&absolute))?;}
                    else {std::fs::read_dir(&absolute).with_context(||format!("opening {}",absolute.display()))?;}
                    *path=absolute.to_str().context("this path is not UTF-8")?.into();
                }
                let command=crate::browser::launch_args(kind,&paths)?;
                let id=self.new_window(command,None)?;
                return Ok(json!({"id":id}));
            }
            "editor.save"|"editor.save_as"=>{
                let w=self.active_mut().context("activate an editor window first")?;
                if !w.is_editor() {bail!("activate an editor window first");}
                w.send(vec![if name=="editor.save" {19} else {15}])?;
            }
            "menu.open" => { self.menu=Some(Menu {index:0,selected:0}); }
            "window.new" => {
                let mut command=vec![]; let mut title=None; let mut cwd=None; let mut i=0;
                while i<args.len() {
                    match args[i].as_str() {
                        "--title"|"--cwd" => { let option=&args[i]; i+=1; let value=args.get(i).context("missing option value")?.clone(); if option=="--title" { title=Some(value); } else { cwd=Some(PathBuf::from(value)); } }
                        "--" => { command=args[i+1..].to_vec(); break; }
                        _ => { command=args[i..].to_vec(); break; }
                    } i+=1;
                }
                let id=self.new_window(if command.is_empty() { Self::shell() } else { command },cwd)?;
                if let Some(title)=title { self.windows.last_mut().unwrap().title=title; }
                return Ok(json!({"id":id}));
            }
            "window.next" => {
                let ids:Vec<_>=self.windows.iter().filter(|w|w.workspace==self.workspace&&!w.shelved).map(|w|w.id).collect();
                if let Some(&id)=ids.first() { self.focus(id); }
            }
            "window.focus" => { let id=args.first().context("window.focus needs an ID")?.parse()?; if !self.windows.iter().any(|w|w.id==id) { bail!("unknown window {id}"); } self.focus(id); }
            "window.close" => {
                if self.active().is_some_and(|w|w.is_editor()) {self.active_mut().unwrap().send(vec![17])?;}
                else if let Some(id)=self.active_id() {self.remove(id);}
            }
            "window.zoom" => {
                if self.compact() { return Ok(json!(null)); }
                let desktop=Rect::desktop(self.cols,self.rows);
                if let Some(w)=self.active_mut() { let rect=if let Some(rect)=w.restore.take() { rect } else { w.restore=Some(w.rect); desktop }; w.resize(rect)?; }
            }
            "window.shelve" => { if let Some(w)=self.active_mut() { w.shelved=true; } self.selection=None; }
            "layout.shelve" => { for w in &mut self.windows { if w.workspace==self.workspace { w.shelved=true; } } self.selection=None; }
            "layout.tile" => {
                if self.compact() { bail!("enlarge the host terminal to arrange windows"); }
                let count=self.windows.iter().filter(|w|w.workspace==self.workspace&&!w.shelved).count();
                let rects=tile(count,self.cols,self.rows);
                if rects.iter().any(|r|r.w<4||r.h<3) { bail!("not enough room to tile these windows"); }
                for (w,rect) in self.windows.iter_mut().filter(|w|w.workspace==self.workspace&&!w.shelved).zip(rects) { w.restore=None; w.resize(rect)?; }
                self.selection=None;
            }
            "layout.cascade" => {
                if self.compact() { bail!("enlarge the host terminal to arrange windows"); }
                for (i,w) in self.windows.iter_mut().filter(|w|w.workspace==self.workspace&&!w.shelved).enumerate() {
                    let offset=(i%8) as u16;
                    w.restore=None; w.resize(Rect { x:offset*3,y:1+offset,w:self.cols.saturating_sub(24),h:self.rows.saturating_sub(10) }.fit(self.cols,self.rows))?;
                } self.selection=None;
            }
            "window.move"|"window.resize" => {
                if self.compact() { bail!("enlarge the host terminal to arrange windows"); }
                if let Some(w)=self.active() { self.drag=Some(Drag { window:w.id,owner:client,kind:if name=="window.move" { DragKind::Move } else { DragKind::Resize { left:false,top:false,right:true,bottom:true } },anchor:(0,0),original:w.rect,preview:w.rect,keyboard:true }); }
            }
            "window.list" => { self.list=Some(0); }
            "window.color" => { if let Some(w)=self.active_mut() { w.color = !w.color; } }
            "workspace.next" => { self.workspace=(self.workspace+1)%4; self.selection=None; }
            "workspace.previous" => { self.workspace=(self.workspace+3)%4; self.selection=None; }
            "window.workspace" => { let next=(self.workspace+1)%4; if let Some(w)=self.active_mut() { w.workspace=next; } self.selection=None; }
            "selection.start" => {
                if let Some(w)=self.active() { let (row,col)=w.parser.screen().cursor_position(); self.selection=Some(Selection { window:w.id,start:(col,row),end:(col,row),keyboard:true,dragging:false }); }
            }
            "selection.copy" => { self.copy(client); }
            "selection.paste" => { self.paste(self.clipboard.clone())?; }
            "scroll.up"|"scroll.down" => { if let Some(w)=self.active_mut() { let amount=w.rect.inner().1 as i32-1; w.scroll(if name=="scroll.up" {amount} else {-amount}); } self.selection=None; }
            "session.detach" => { if client==0 { bail!("detach must be invoked by an attached client"); } self.detach=Some(client); }
            "session.end" => { if args.first().is_some_and(|s|s=="--confirm") { self.running=false; } else { self.confirmation=true; } }
            "config.reload" => {
                let config=Config::load()?;
                for (key,value) in &config.keys { if !value.is_empty()&&!COMMANDS.iter().any(|c|c.name==value) { bail!("unknown command {value} for key {key}"); } }
                crate::session::write_theme(&self.name,config.look.theme)?;
                self.config=config; self.status="Configuration reloaded".into();
            }
            "layout.save" => {
                let path=args.first().map(PathBuf::from).unwrap_or_else(||Config::path().with_file_name("layout.json"));
                self.save_layout(&path)?; self.status=format!("Saved {}",path.display()); return Ok(json!({"path":path}));
            }
            "help.show" => { self.help=true; }
            "list" => return Ok(json!(self.windows.iter().map(|w|json!({"id":w.id,"title":w.title,"rect":w.rect,"pid":w.pid(),"command":w.command,"workspace":w.workspace+1,"shelved":w.shelved,"active":self.active_id()==Some(w.id)})).collect::<Vec<_>>())),
            "commands" => return Ok(json!(COMMANDS.iter().map(|c|json!({"name":c.name,"label":c.label,"shortcut":commands::shortcut(c.name,&self.config)})).collect::<Vec<_>>())),
            _ => bail!("unknown command: {name}"),
        }
        Ok(json!(null))
    }
    fn apply_theme(&mut self, id: crate::theme::ThemeId) -> Result<()> {
        crate::session::write_theme(&self.name, id)?;
        self.config.look.theme = id;
        self.status = format!("{} theme", id.label());
        Ok(())
    }
    pub fn save_layout(&self, path: &std::path::Path) -> Result<()> {
        let layout = Layout {
            windows: self
                .windows
                .iter()
                .map(|w| LayoutWindow {
                    rect: w.restore.unwrap_or(w.rect),
                    command: w.command.clone(),
                    cwd: current_cwd(w),
                    workspace: w.workspace,
                    shelved: w.shelved,
                    color: w.color,
                })
                .collect(),
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("tmp-{}", std::process::id()));
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(serde_json::to_string_pretty(&layout)?.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
    pub fn load_layout(&mut self, path: &std::path::Path) -> Result<()> {
        let layout: Layout = serde_json::from_slice(&std::fs::read(path)?)?;
        if layout.windows.len() > 64 {
            bail!("layout exceeds 64 windows");
        }
        for entry in layout.windows {
            self.new_window(entry.command, Some(entry.cwd))?;
            let w = self.windows.last_mut().unwrap();
            w.resize(Rect {
                x: entry.rect.x.min(499),
                y: entry.rect.y.clamp(1, 199),
                w: entry.rect.w.clamp(4, 500),
                h: entry.rect.h.clamp(3, 200),
            })?;
            w.workspace = entry.workspace % 4;
            w.shelved = entry.shelved;
            w.color = entry.color;
        }
        Ok(())
    }
    fn copy(&mut self, client: u64) {
        let Some(s) = &self.selection else {
            return;
        };
        let Some(w) = self.windows.iter().find(|w| w.id == s.window) else {
            return;
        };
        let (start, end) = ordered(s.start, s.end);
        let (cols, _) = w.rect.inner();
        let mut text = String::new();
        for y in start.1..=end.1 {
            let left = if y == start.1 { start.0 } else { 0 };
            let right = if y == end.1 { end.0 } else { cols - 1 };
            let mut row = String::new();
            for x in left..=right {
                if let Some(cell) = w.parser.screen().cell(y, x)
                    && !cell.is_wide_continuation()
                {
                    row.push_str(if cell.contents().is_empty() {
                        " "
                    } else {
                        cell.contents()
                    });
                }
            }
            text.push_str(row.trim_end());
            if y != end.1 {
                text.push('\n');
            }
        }
        // Keep OSC 52 small enough for common host terminal limits.
        while text.len() > 75000 {
            text.pop();
        }
        self.clipboard = text;
        self.clipboard_output = Some((
            client,
            format!(
                "\x1b]52;c;{}\x07",
                base64::engine::general_purpose::STANDARD.encode(self.clipboard.as_bytes())
            ),
        ));
        self.status = format!("Copied {} bytes", self.clipboard.len());
    }
    fn paste(&mut self, text: String) -> Result<()> {
        self.selection = None;
        if let Some(w) = self.active_mut() {
            let text = text.replace("\x1b[201~", "");
            let bytes = if w.parser.screen().bracketed_paste() {
                format!("\x1b[200~{text}\x1b[201~").into_bytes()
            } else {
                text.into_bytes()
            };
            w.send(bytes)?;
        }
        Ok(())
    }
    pub fn event(&mut self, event: Event, client: u64) -> Result<()> {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.key(key, client)?,
            Event::Mouse(mouse) => self.mouse(mouse, client)?,
            Event::Paste(text) if self.theme_picker.is_none() => self.paste(text)?,
            Event::FocusLost => {
                self.pointers.remove(&client);
                if self.drag.as_ref().is_some_and(|d| d.owner == client) {
                    self.drag = None;
                }
                if self.selection.as_ref().is_some_and(|s| s.dragging) {
                    self.selection = None;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn key(&mut self, key: KeyEvent, client: u64) -> Result<()> {
        if self.confirmation {
            match key.code {
                KeyCode::Char('y' | 'Y') => self.running = false,
                KeyCode::Esc | KeyCode::Char('n' | 'N') => self.confirmation = false,
                _ => {}
            }
            return Ok(());
        }
        if self.help {
            self.help = false;
            return Ok(());
        }
        if let Some(picker) = &mut self.theme_picker {
            let count = crate::theme::ThemeId::ALL.len();
            let selected = picker.selected;
            match key.code {
                KeyCode::Esc => self.theme_picker = None,
                KeyCode::Up => picker.selected = (selected + count - 1) % count,
                KeyCode::Down => picker.selected = (selected + 1) % count,
                KeyCode::Home => picker.selected = 0,
                KeyCode::End => picker.selected = count - 1,
                KeyCode::Enter => {
                    self.apply_theme(crate::theme::ThemeId::ALL[selected])?;
                    self.theme_picker = None;
                }
                _ => {}
            }
            return Ok(());
        }
        if let Some(selected) = self.list {
            let count = self.windows.len();
            match key.code {
                KeyCode::Esc => self.list = None,
                KeyCode::Up => self.list = Some(selected.saturating_sub(1)),
                KeyCode::Down => self.list = Some((selected + 1).min(count.saturating_sub(1))),
                KeyCode::Enter => {
                    let id = self.windows.get(selected).map(|w| w.id);
                    self.list = None;
                    if let Some(id) = id {
                        self.focus(id);
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        if self.drag.is_some() {
            match key.code {
                KeyCode::Esc => self.drag = None,
                KeyCode::Enter => self.finish_drag()?,
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
                    let d = self.drag.as_mut().unwrap();
                    let (dx, dy) = match key.code {
                        KeyCode::Up => (0, -1),
                        KeyCode::Down => (0, 1),
                        KeyCode::Left => (-1, 0),
                        _ => (1, 0),
                    };
                    d.preview = adjust(d.preview, d.kind, dx, dy, self.cols, self.rows);
                }
                _ => {}
            }
            return Ok(());
        }
        if self.selection.as_ref().is_some_and(|s| s.keyboard) {
            match key.code {
                KeyCode::Esc => self.selection = None,
                KeyCode::Enter => {
                    self.copy(client);
                    if let Some(s) = self.selection.as_mut() {
                        s.keyboard = false;
                    }
                }
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
                    let (cols, rows) = self.active().map(|w| w.rect.inner()).unwrap_or((1, 1));
                    let s = self.selection.as_mut().unwrap();
                    match key.code {
                        KeyCode::Up => s.end.1 = s.end.1.saturating_sub(1),
                        KeyCode::Down => s.end.1 = (s.end.1 + 1).min(rows - 1),
                        KeyCode::Left => s.end.0 = s.end.0.saturating_sub(1),
                        _ => s.end.0 = (s.end.0 + 1).min(cols - 1),
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        if let Some(menu) = &self.menu {
            let index = menu.index;
            let selected = menu.selected;
            let entries = self.menu_entries(index);
            match key.code {
                KeyCode::Esc | KeyCode::F(10) => self.menu = None,
                KeyCode::Left => {
                    self.menu = Some(Menu {
                        index: (index + MENUS.len() - 1) % MENUS.len(),
                        selected: 0,
                    })
                }
                KeyCode::Right => {
                    self.menu = Some(Menu {
                        index: (index + 1) % MENUS.len(),
                        selected: 0,
                    })
                }
                KeyCode::Up => {
                    self.menu.as_mut().unwrap().selected =
                        (selected + entries.len() - 1) % entries.len()
                }
                KeyCode::Down => {
                    self.menu.as_mut().unwrap().selected = (selected + 1) % entries.len()
                }
                KeyCode::Enter => {
                    let command = entries[selected].command.clone();
                    self.menu = None;
                    self.command(&command, &[], client)?;
                }
                KeyCode::Char(c) => {
                    if let Some(entry) = entries
                        .iter()
                        .find(|e| e.label.to_lowercase().starts_with(c.to_ascii_lowercase()))
                    {
                        let command = entry.command.clone();
                        self.menu = None;
                        self.command(&command, &[], client)?;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        let name = input::key_name(key);
        if let Some(command) = commands::bound_command(&name, &self.config) {
            self.command(&command, &[], client)?;
            return Ok(());
        }
        if key.modifiers == KeyModifiers::ALT
            && let KeyCode::Char(c @ '1'..='9') = key.code
        {
            let id = (c as u8 - b'0') as u64;
            if let Some(id) = self
                .windows
                .iter()
                .find(|w| w.workspace == self.workspace && w.id == id)
                .map(|w| w.id)
            {
                self.focus(id);
            }
            return Ok(());
        }
        self.selection = None;
        self.status.clear();
        if let Some(w) = self.active_mut() {
            w.send(input::encode_key(
                key,
                w.parser.screen().application_cursor(),
            ))?;
        }
        Ok(())
    }
    fn finish_drag(&mut self) -> Result<()> {
        if let Some(d) = self.drag.take()
            && let Some(w) = self.windows.iter_mut().find(|w| w.id == d.window)
        {
            w.restore = None;
            w.resize(d.preview)?;
        }
        self.selection = None;
        Ok(())
    }
    fn mouse(&mut self, event: MouseEvent, client: u64) -> Result<()> {
        let (x, y) = (event.column, event.row);
        self.pointers.insert(client, (x, y));
        if x >= self.cols || y >= self.rows {
            return Ok(());
        }
        let left = event.kind == MouseEventKind::Down(MouseButton::Left);
        if self.confirmation {
            if left {
                let (bx, by) = self.dialog_origin(48, 6);
                if y == by + 4 && x >= bx + 2 && x < bx + 14 {
                    self.running = false;
                } else if y == by + 4 {
                    self.confirmation = false;
                }
            }
            return Ok(());
        }
        if self.help {
            if left {
                self.help = false;
            }
            return Ok(());
        }
        if let Some(picker) = &self.theme_picker {
            let selected = picker.selected;
            let r = self.theme_picker_geometry();
            let visible = r.h.saturating_sub(3) as usize;
            let offset = selected.saturating_sub(visible.saturating_sub(1));
            match event.kind {
                MouseEventKind::ScrollUp => {
                    self.theme_picker.as_mut().unwrap().selected = selected.saturating_sub(1)
                }
                MouseEventKind::ScrollDown => {
                    self.theme_picker.as_mut().unwrap().selected =
                        (selected + 1).min(crate::theme::ThemeId::ALL.len() - 1)
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if x > r.x
                        && x < r.x + r.w.saturating_sub(1)
                        && y > r.y
                        && y < r.y + r.h.saturating_sub(2)
                    {
                        let index = offset + (y - r.y - 1) as usize;
                        if let Some(&id) = crate::theme::ThemeId::ALL.get(index) {
                            self.apply_theme(id)?;
                            self.theme_picker = None;
                        }
                    } else {
                        self.theme_picker = None;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        if self.list.is_some() {
            let (bx, by) = self.dialog_origin(
                48,
                (self.windows.len() + 3).min(self.rows.saturating_sub(2) as usize) as u16,
            );
            if left {
                if x >= bx && x < bx + 48 && y > by && (y - by - 1) < self.windows.len() as u16 {
                    let id = self.windows[(y - by - 1) as usize].id;
                    self.list = None;
                    self.focus(id);
                } else {
                    self.list = None;
                }
            }
            return Ok(());
        }
        if let Some(d) = self
            .drag
            .as_ref()
            .filter(|d| d.owner == client && !d.keyboard)
        {
            match event.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    let preview = adjust(
                        d.original,
                        d.kind,
                        x as i32 - d.anchor.0 as i32,
                        y as i32 - d.anchor.1 as i32,
                        self.cols,
                        self.rows,
                    );
                    self.drag.as_mut().unwrap().preview = preview;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.finish_drag()?;
                }
                _ => {}
            }
            return Ok(());
        }
        if self.selection.as_ref().is_some_and(|s| s.dragging) {
            let id = self.selection.as_ref().unwrap().window;
            if let Some(w) = self.windows.iter().find(|w| w.id == id)
                && matches!(
                    event.kind,
                    MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
                )
            {
                self.selection.as_mut().unwrap().end = (
                    x.saturating_sub(w.rect.x + 1).min(w.rect.inner().0 - 1),
                    y.saturating_sub(w.rect.y + 1).min(w.rect.inner().1 - 1),
                );
                if matches!(event.kind, MouseEventKind::Up(_)) {
                    self.selection.as_mut().unwrap().dragging = false;
                    self.copy(client);
                }
            }
            return Ok(());
        }
        if y == 0 {
            if left && let Some(index) = menu_at(x) {
                self.menu = if self.menu.as_ref().is_some_and(|m| m.index == index) {
                    None
                } else {
                    Some(Menu { index, selected: 0 })
                };
            }
            return Ok(());
        }
        if let Some(menu) = &self.menu {
            let index = menu.index;
            let entries = self.menu_entries(index);
            let (mx, my, mw, mh) = self.menu_geometry(index);
            if matches!(
                event.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            ) {
                let selected = menu.selected;
                self.menu.as_mut().unwrap().selected = if event.kind == MouseEventKind::ScrollUp {
                    selected.saturating_sub(1)
                } else {
                    (selected + 1).min(entries.len() - 1)
                };
                return Ok(());
            }
            if x >= mx && x < mx.saturating_add(mw) && y > my && y < my + mh - 1 {
                let visible = mh.saturating_sub(2) as usize;
                let offset = menu.selected.saturating_sub(visible.saturating_sub(1));
                let selected = offset + (y - my - 1) as usize;
                if selected < entries.len() {
                    self.menu.as_mut().unwrap().selected = selected;
                    if left {
                        let command = entries[selected].command.clone();
                        self.menu = None;
                        self.command(&command, &[], client)?;
                    }
                }
            } else if left {
                self.menu = None;
            }
            return Ok(());
        }
        if y == self.rows.saturating_sub(1) {
            if left && let Some(command) = self.keybar_at(x) {
                self.command(command, &[], client)?;
            }
            return Ok(());
        }
        let mut sx = 0;
        for w in self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.shelved)
        {
            let width = tab_width(w);
            if y == self.rows.saturating_sub(2) && x >= sx && x < sx + width {
                let id = w.id;
                if left {
                    self.focus(id);
                }
                return Ok(());
            }
            sx += width;
        }
        let active = self.active_id();
        let hit = self
            .windows
            .iter()
            .rev()
            .find(|w| {
                w.workspace == self.workspace
                    && !w.shelved
                    && (!self.compact() || Some(w.id) == active)
                    && w.rect.contains(x, y)
            })
            .map(|w| w.id);
        let Some(id) = hit else {
            if event.kind == MouseEventKind::Down(MouseButton::Right) {
                self.list = Some(0);
            }
            return Ok(());
        };
        if matches!(event.kind, MouseEventKind::Down(_)) && self.active_id() != Some(id) {
            self.focus(id);
        }
        let w = self.windows.iter().find(|w| w.id == id).unwrap();
        let r = w.rect;
        if y == r.y {
            if left {
                if x <= r.x + 3 {
                    self.command("window.close", &[], client)?;
                } else if x >= r.x + r.w - 4 {
                    self.command("window.zoom", &[], client)?;
                } else if self.last_title_click.is_some_and(|(wid, cid, t)| {
                    wid == id && cid == client && t.elapsed().as_millis() < 400
                }) {
                    self.last_title_click = None;
                    self.command("window.zoom", &[], client)?;
                } else if !self.compact() {
                    self.last_title_click = Some((id, client, Instant::now()));
                    self.drag = Some(Drag {
                        window: id,
                        owner: client,
                        kind: DragKind::Move,
                        anchor: (x, y),
                        original: r,
                        preview: r,
                        keyboard: false,
                    });
                }
            } else if event.kind == MouseEventKind::Down(MouseButton::Middle) {
                self.command("window.shelve", &[], client)?;
            }
            return Ok(());
        }
        let right = x == r.x + r.w - 1;
        let bottom = y == r.y + r.h - 1;
        // The right frame is the scroll bar; its bottom corner resizes.
        if right && !bottom {
            let w = self.windows.iter_mut().find(|w| w.id == id).unwrap();
            if left || matches!(event.kind, MouseEventKind::Drag(MouseButton::Left)) {
                if y == r.y + 1 {
                    w.scroll(1);
                } else if y == r.y + r.h - 2 {
                    w.scroll(-1);
                } else {
                    let track = r.h.saturating_sub(4).max(1) as usize;
                    let offset = w
                        .parser
                        .screen()
                        .scrollback_len()
                        .saturating_mul((r.y + r.h - 3).saturating_sub(y) as usize)
                        / track;
                    w.parser.screen_mut().set_scrollback(offset);
                }
                self.selection = None;
            }
            return Ok(());
        }
        if x == r.x || bottom {
            if left && !self.compact() {
                self.drag = Some(Drag {
                    window: id,
                    owner: client,
                    kind: DragKind::Resize {
                        left: x == r.x,
                        top: false,
                        right,
                        bottom,
                    },
                    anchor: (x, y),
                    original: r,
                    preview: r,
                    keyboard: false,
                });
            }
            return Ok(());
        }
        let local = (x - r.x - 1, y - r.y - 1);
        let program_mouse =
            w.parser.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None;
        if program_mouse && !event.modifiers.contains(KeyModifiers::ALT) {
            let bytes = input::encode_mouse(event, local.0, local.1, w.parser.screen());
            if !bytes.is_empty() {
                self.windows
                    .iter_mut()
                    .find(|w| w.id == id)
                    .unwrap()
                    .send(bytes)?;
            }
        } else {
            match event.kind {
                MouseEventKind::ScrollUp => {
                    self.windows
                        .iter_mut()
                        .find(|w| w.id == id)
                        .unwrap()
                        .scroll(3);
                    self.selection = None;
                }
                MouseEventKind::ScrollDown => {
                    self.windows
                        .iter_mut()
                        .find(|w| w.id == id)
                        .unwrap()
                        .scroll(-3);
                    self.selection = None;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.selection = Some(Selection {
                        window: id,
                        start: local,
                        end: local,
                        keyboard: false,
                        dragging: true,
                    })
                }
                MouseEventKind::Down(MouseButton::Middle) => self.paste(self.clipboard.clone())?,
                _ => {}
            }
        }
        Ok(())
    }
    fn keybar(&self) -> Vec<(u16, String, &'static str)> {
        let mut x = 0;
        [
            ("Help", "help.show"),
            ("Next", "window.next"),
            ("Tile", "layout.tile"),
            ("Cascade", "layout.cascade"),
            ("Shelf", "layout.shelve"),
            ("Menu", "menu.open"),
        ]
        .into_iter()
        .map(|(label, command)| {
            let key = commands::shortcut(command, &self.config);
            let label = if key.is_empty() {
                format!(" {label} ")
            } else {
                format!(" {key} {label} ")
            };
            let start = x;
            x += label.len() as u16;
            (start, label, command)
        })
        .collect()
    }
    fn keybar_at(&self, x: u16) -> Option<&'static str> {
        self.keybar()
            .into_iter()
            .find(|(start, label, _)| x >= *start && x < *start + label.len() as u16)
            .map(|(_, _, command)| command)
    }
    fn menu_geometry(&self, index: usize) -> (u16, u16, u16, u16) {
        let entries = self.menu_entries(index);
        let width = entries
            .iter()
            .map(|e| e.label.chars().count() + e.shortcut.len() + 5)
            .max()
            .unwrap_or(20) as u16;
        let width = width.min(self.cols);
        let x = menu_start(index).min(self.cols.saturating_sub(width));
        (
            x,
            1,
            width,
            (entries.len() as u16 + 2).min(self.rows.saturating_sub(2)),
        )
    }
    fn dialog_origin(&self, w: u16, h: u16) -> (u16, u16) {
        (
            self.cols.saturating_sub(w) / 2,
            self.rows.saturating_sub(h) / 2,
        )
    }
    fn theme_picker_geometry(&self) -> Rect {
        let w = self.cols.min(62);
        let h = self
            .rows
            .saturating_sub(2)
            .min(crate::theme::ThemeId::ALL.len() as u16 + 3);
        let (x, y) = self.dialog_origin(w, h);
        Rect { x, y, w, h }
    }
    pub fn compose(&self, client: u64, ascii: bool, console: bool) -> Grid {
        let mut grid = Grid::new(self.cols, self.rows);
        let theme = crate::theme::Theme::named(self.config.look.theme);
        let normal = theme.frame.clone();
        let inverse = theme.menu.clone();
        grid.fill(
            0,
            1,
            self.cols,
            self.rows.saturating_sub(2),
            if ascii { "." } else { "░" },
            theme.desktop.clone(),
        );
        let active = self.active_id();
        for w in self.windows.iter().filter(|w| {
            w.workspace == self.workspace && !w.shelved && (!self.compact() || Some(w.id) == active)
        }) {
            if self.drag.as_ref().is_some_and(|d| d.window == w.id) {
                continue;
            }
            let r = w.rect;
            if self.config.look.shadow && !self.compact() {
                grid.fill(
                    r.x.saturating_add(1),
                    r.y.saturating_add(1),
                    r.w,
                    r.h,
                    if ascii { "#" } else { " " },
                    theme.shadow.clone(),
                );
            }
            frame(&mut grid, r, ascii, normal.clone());
            let title_style = if active == Some(w.id) {
                theme.title.clone()
            } else {
                theme.inactive_title.clone()
            };
            grid.fill(
                r.x + 1,
                r.y,
                r.w.saturating_sub(2),
                1,
                if active == Some(w.id) {
                    " "
                } else if ascii {
                    "-"
                } else {
                    "─"
                },
                title_style.clone(),
            );
            grid.text(
                r.x + 1,
                r.y,
                if ascii { "[x]" } else { "[×]" },
                3,
                title_style.clone(),
            );
            grid.text(
                r.x + r.w.saturating_sub(4),
                r.y,
                if ascii {
                    if w.restore.is_some() { "[-]" } else { "[+]" }
                } else if w.restore.is_some() {
                    "[↙]"
                } else {
                    "[↑]"
                },
                3,
                title_style.clone(),
            );
            let title = format!(" {} ", w.title);
            let available = r.w.saturating_sub(10);
            let title_width = unicode_width::UnicodeWidthStr::width(title.as_str())
                .min(available as usize) as u16;
            grid.text(
                r.x + 5 + available.saturating_sub(title_width) / 2,
                r.y,
                &title,
                available,
                title_style,
            );
            let (cols, rows) = r.inner();
            for y in 0..rows {
                for x in 0..cols {
                    if r.y + 1 + y >= self.rows.saturating_sub(1) || r.x + 1 + x >= self.cols {
                        continue;
                    }
                    if let Some(cell) = w.parser.screen().cell(y, x) {
                        if cell.is_wide_continuation() {
                            continue;
                        }
                        let mut style = Style::program(cell, w.color);
                        if self
                            .selection
                            .as_ref()
                            .is_some_and(|s| s.window == w.id && selected(s, (x, y)))
                        {
                            style = theme.title.clone();
                        }
                        let clipped = cell.is_wide() && (x + 1 >= cols || r.x + 2 + x >= self.cols);
                        grid.put(
                            r.x + 1 + x,
                            r.y + 1 + y,
                            if clipped || cell.contents().is_empty() {
                                " "
                            } else {
                                cell.contents()
                            },
                            if cell.is_wide() && !clipped { 2 } else { 1 },
                            style,
                        );
                    }
                }
            }
            let bar = r.x + r.w - 1;
            grid.fill(
                bar,
                r.y + 1,
                1,
                r.h.saturating_sub(2),
                if ascii { ":" } else { "░" },
                theme.scroll.clone(),
            );
            grid.text(
                bar,
                r.y + 1,
                if ascii { "^" } else { "▲" },
                1,
                theme.scroll.clone(),
            );
            grid.text(
                bar,
                r.y + r.h - 2,
                if ascii { "v" } else { "▼" },
                1,
                theme.scroll.clone(),
            );
            let max = w.parser.screen().scrollback_len().max(1);
            let track = r.h.saturating_sub(5);
            let thumb = r.y
                + 2
                + track
                    .saturating_sub((track as usize * w.parser.screen().scrollback() / max) as u16);
            if r.h > 4 {
                grid.text(
                    bar,
                    thumb,
                    if ascii { "#" } else { "█" },
                    1,
                    theme.thumb.clone(),
                );
            }
            grid.text(
                bar,
                r.y + r.h - 1,
                if ascii { "+" } else { "◢" },
                1,
                theme.scroll.clone(),
            );
        }
        if let Some(d) = &self.drag {
            outline(&mut grid, d.preview, ascii, theme.thumb.clone());
        }
        let mut sx = 0;
        for w in self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.shelved)
        {
            let width = tab_width(w);
            let tab_style = if w.activity {
                theme.activity.clone()
            } else {
                theme.title.clone()
            };
            let title = format!("[{}{}]", if w.activity { "*" } else { "" }, w.title);
            grid.fill(
                sx,
                self.rows.saturating_sub(2),
                width,
                1,
                " ",
                tab_style.clone(),
            );
            grid.text(
                sx,
                self.rows.saturating_sub(2),
                &title,
                width,
                tab_style.clone(),
            );
            sx += width;
        }
        grid.fill(0, 0, self.cols, 1, " ", inverse.clone());
        for (index, label) in MENUS.iter().enumerate() {
            grid.text(
                menu_start(index),
                0,
                &format!(" {label} "),
                label.len() as u16 + 2,
                if self.menu.as_ref().is_some_and(|m| m.index == index) {
                    theme.selected.clone()
                } else {
                    theme.menu.clone()
                },
            );
        }
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // localtime_r follows the server's local timezone.
        let clock = local_clock(seconds);
        let right = format!(" {}  {}/4  {} ", self.name, self.workspace + 1, clock);
        let width = unicode_width::UnicodeWidthStr::width(right.as_str()) as u16;
        if self.cols > menu_start(MENUS.len()) + width {
            grid.text(self.cols - width, 0, &right, width, inverse.clone());
        }
        grid.fill(
            0,
            self.rows.saturating_sub(1),
            self.cols,
            1,
            " ",
            inverse.clone(),
        );
        let keybar = self.keybar();
        let keybar_width = keybar
            .last()
            .map(|(x, label, _)| x + label.len() as u16)
            .unwrap_or(0);
        for (x, label, _) in &keybar {
            grid.text(
                *x,
                self.rows.saturating_sub(1),
                label,
                label.len() as u16,
                inverse.clone(),
            );
        }
        for (x, label, _) in &keybar {
            if let Some(key) = label
                .trim_start()
                .split(' ')
                .next()
                .filter(|key| key.starts_with('F') || key.contains('+'))
            {
                grid.text(
                    *x + 1,
                    self.rows.saturating_sub(1),
                    key,
                    key.len() as u16,
                    theme.key.clone(),
                );
            }
        }
        let context = if self.confirmation {
            "Y end session · N cancel".into()
        } else if self.drag.is_some() {
            "Arrows move · Enter apply · Esc cancel".into()
        } else if self.selection.as_ref().is_some_and(|s| s.keyboard) {
            "Arrows select · Enter copy · Esc cancel".into()
        } else if !self.status.is_empty() {
            self.status.clone()
        } else {
            self.hover_help(client)
        };
        let context_width = unicode_width::UnicodeWidthStr::width(context.as_str()) as u16;
        if self.cols > keybar_width + context_width {
            grid.text(
                self.cols - context_width - 1,
                self.rows.saturating_sub(1),
                &context,
                context_width,
                inverse.clone(),
            );
        }
        if let Some(menu) = &self.menu {
            let (x, y, w, h) = self.menu_geometry(menu.index);
            let visible = h.saturating_sub(2) as usize;
            let offset = menu.selected.saturating_sub(visible.saturating_sub(1));
            grid.fill(x + 1, y + 1, w, h, " ", theme.shadow.clone());
            frame(&mut grid, Rect { x, y, w, h }, ascii, theme.menu.clone());
            for (i, e) in self
                .menu_entries(menu.index)
                .iter()
                .skip(offset)
                .take(visible)
                .enumerate()
            {
                let style = if i + offset == menu.selected {
                    theme.selected.clone()
                } else {
                    theme.menu.clone()
                };
                grid.fill(
                    x + 1,
                    y + 1 + i as u16,
                    w.saturating_sub(2),
                    1,
                    " ",
                    style.clone(),
                );
                grid.text(
                    x + 2,
                    y + 1 + i as u16,
                    &e.label,
                    w.saturating_sub(4 + e.shortcut.len() as u16),
                    style.clone(),
                );
                if let Some(c) = e.label.chars().next() {
                    grid.text(
                        x + 2,
                        y + 1 + i as u16,
                        &c.to_string(),
                        1,
                        if i + offset == menu.selected {
                            theme.thumb.clone()
                        } else {
                            theme.hotkey.clone()
                        },
                    );
                }
                grid.text(
                    x + w.saturating_sub(2 + e.shortcut.len() as u16),
                    y + 1 + i as u16,
                    &e.shortcut,
                    e.shortcut.len() as u16,
                    if i + offset == menu.selected {
                        theme.thumb.clone()
                    } else {
                        theme.shortcut.clone()
                    },
                );
            }
        }
        if let Some(index) = self.list {
            let h = (self.windows.len() as u16 + 2).min(self.rows.saturating_sub(2));
            let width = 48.min(self.cols);
            let (x, y) = self.dialog_origin(width, h);
            frame(
                &mut grid,
                Rect { x, y, w: width, h },
                ascii,
                theme.dialog.clone(),
            );
            for (i, w) in self
                .windows
                .iter()
                .take(h.saturating_sub(2) as usize)
                .enumerate()
            {
                let style = if index == i {
                    theme.selected.clone()
                } else {
                    theme.dialog.clone()
                };
                grid.fill(
                    x + 1,
                    y + 1 + i as u16,
                    width.saturating_sub(2),
                    1,
                    " ",
                    style.clone(),
                );
                grid.text(
                    x + 2,
                    y + 1 + i as u16,
                    &format!(
                        "{}  {}{}  (workspace {})",
                        w.id,
                        if w.shelved { "↓ " } else { "" },
                        w.title,
                        w.workspace + 1
                    ),
                    width.saturating_sub(4),
                    style,
                );
            }
        }
        if self.help {
            let lines = [
                "saumux · a text-mode desktop",
                "",
                "F10 opens menus; arrows and Enter choose.",
                "Drag a title to move, an edge to resize.",
                "Alt+F5 / Alt+F8: arrows, then Enter.",
                "F6 next · F7 tile · F8 cascade · F9 shelf",
                "Alt+n shell · Alt+d detach · Alt+F4 close",
                "Shift+PgUp/PgDn scroll · Alt+Space select",
                "Drag to copy · middle-click to paste.",
                "Alt+drag selects inside mouse-aware apps.",
                "Shift+drag uses the host terminal selection.",
                "Sessions survive terminal / SSH disconnects.",
                "",
                "Click or press any key to return.",
            ];
            self.dialog(&mut grid, &lines, ascii);
        }
        if self.confirmation {
            let width = 48.min(self.cols);
            let (x, y) = self.dialog_origin(width, 6);
            frame(
                &mut grid,
                Rect {
                    x,
                    y,
                    w: width,
                    h: 6,
                },
                ascii,
                theme.dialog.clone(),
            );
            grid.text(
                x + 2,
                y + 1,
                "End session and stop every program?",
                width.saturating_sub(4),
                theme.dialog.clone(),
            );
            grid.text(
                x + 2,
                y + 4,
                "[Y] End       [N] Cancel",
                width.saturating_sub(4),
                theme.warning.clone(),
            );
        }
        if let Some(picker) = &self.theme_picker {
            let r = self.theme_picker_geometry();
            grid.fill(r.x + 1, r.y + 1, r.w, r.h, " ", theme.shadow.clone());
            frame(&mut grid, r, ascii, theme.dialog.clone());
            grid.text(
                r.x + 2,
                r.y,
                " Colour themes ",
                r.w.saturating_sub(4),
                theme.dialog.clone(),
            );
            let visible = r.h.saturating_sub(3) as usize;
            let offset = picker.selected.saturating_sub(visible.saturating_sub(1));
            for (row, id) in crate::theme::ThemeId::ALL
                .iter()
                .enumerate()
                .skip(offset)
                .take(visible)
            {
                let y = r.y + 1 + (row - offset) as u16;
                let style = if row == picker.selected {
                    theme.selected.clone()
                } else {
                    theme.dialog.clone()
                };
                grid.fill(r.x + 1, y, r.w.saturating_sub(2), 1, " ", style.clone());
                let label = format!(
                    "{} {:10} {}",
                    if *id == self.config.look.theme {
                        "*"
                    } else {
                        " "
                    },
                    id.label(),
                    id.description()
                );
                grid.text(r.x + 2, y, &label, r.w.saturating_sub(10), style);
                if r.w > 12 {
                    grid.text(
                        r.x + r.w - 7,
                        y,
                        " Aa ",
                        4,
                        crate::theme::Theme::named(*id).editor,
                    );
                }
            }
            grid.text(
                r.x + 2,
                r.y + r.h.saturating_sub(2),
                "Click / Enter applies · Esc cancels",
                r.w.saturating_sub(4),
                theme.dialog.clone(),
            );
        }
        let modal = self.menu.is_some()
            || self.theme_picker.is_some()
            || self.help
            || self.confirmation
            || self.list.is_some()
            || self.drag.is_some()
            || self.selection.is_some();
        if !modal
            && let Some(w) = self.active()
            && !w.parser.screen().hide_cursor()
            && w.parser.screen().scrollback() == 0
        {
            let (row, col) = w.parser.screen().cursor_position();
            let x = w.rect.x + 1 + col;
            let y = w.rect.y + 1 + row;
            if x < self.cols && y < self.rows.saturating_sub(1) {
                grid.cursor = Some((x, y));
            }
        }
        if let Some(&(x, y)) = self.pointers.get(&client)
            && (console || self.clickable(x, y))
        {
            let i = y as usize * self.cols as usize + x as usize;
            if console && grid.cells.get(i).is_some_and(|c| c.width != 1) {
                grid.put(x, y, " ", 1, inverse.clone());
            } else if let Some(cell) = grid.cells.get_mut(i) {
                cell.style.inverse = !cell.style.inverse;
            }
        }
        grid
    }
    fn dialog(&self, grid: &mut Grid, lines: &[&str], ascii: bool) {
        let width =
            (lines.iter().map(|l| l.chars().count()).max().unwrap_or(20) as u16 + 4).min(self.cols);
        let height = (lines.len() as u16 + 2).min(self.rows.saturating_sub(2));
        let (x, y) = self.dialog_origin(width, height);
        frame(
            grid,
            Rect {
                x,
                y,
                w: width,
                h: height,
            },
            ascii,
            crate::theme::Theme::named(self.config.look.theme).dialog,
        );
        for (i, line) in lines
            .iter()
            .take(height.saturating_sub(2) as usize)
            .enumerate()
        {
            grid.text(
                x + 2,
                y + 1 + i as u16,
                line,
                width.saturating_sub(4),
                crate::theme::Theme::named(self.config.look.theme).dialog,
            );
        }
    }
    fn clickable(&self, x: u16, y: u16) -> bool {
        if x >= self.cols || y >= self.rows {
            return false;
        }
        if y == 0 {
            return menu_at(x).is_some();
        }
        if y == self.rows - 1 {
            return self.keybar_at(x).is_some();
        }
        if self.menu.is_some()
            || self.list.is_some()
            || self.help
            || self.confirmation
            || self.theme_picker.is_some()
        {
            return true;
        }
        self.windows
            .iter()
            .rev()
            .filter(|w| w.workspace == self.workspace && !w.shelved)
            .find(|w| w.rect.contains(x, y))
            .is_some_and(|w| {
                y == w.rect.y
                    || y == w.rect.y + w.rect.h - 1
                    || x == w.rect.x
                    || x == w.rect.x + w.rect.w - 1
            })
    }
    fn hover_help(&self, client: u64) -> String {
        let Some(&(x, y)) = self.pointers.get(&client) else {
            return "F10 menus".into();
        };
        if y == 0 {
            return "Click to open menu".into();
        }
        if y == self.rows.saturating_sub(1) {
            return "Click a function key".into();
        }
        if let Some(w) = self
            .windows
            .iter()
            .rev()
            .find(|w| w.workspace == self.workspace && !w.shelved && w.rect.contains(x, y))
        {
            if y == w.rect.y {
                return "Drag title · double-click zoom".into();
            }
            if x == w.rect.x + w.rect.w - 1 {
                return "Scroll · bottom corner resizes".into();
            }
            return "Drag to select · middle-click paste".into();
        }
        "Right-click for window list".into()
    }
}

fn menu_start(index: usize) -> u16 {
    MENUS.iter().take(index).map(|s| s.len() as u16 + 2).sum()
}
fn menu_at(x: u16) -> Option<usize> {
    (0..MENUS.len()).find(|&i| x >= menu_start(i) && x < menu_start(i + 1))
}
fn tab_width(w: &Window) -> u16 {
    (unicode_width::UnicodeWidthStr::width(w.title.as_str()) as u16 + 3 + u16::from(w.activity))
        .min(24)
}
fn ordered(a: (u16, u16), b: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    if (a.1, a.0) <= (b.1, b.0) {
        (a, b)
    } else {
        (b, a)
    }
}
fn selected(s: &Selection, p: (u16, u16)) -> bool {
    let (a, b) = ordered(s.start, s.end);
    (p.1, p.0) >= (a.1, a.0) && (p.1, p.0) <= (b.1, b.0)
}
fn adjust(r: Rect, kind: DragKind, dx: i32, dy: i32, cols: u16, rows: u16) -> Rect {
    let mut x = r.x as i32;
    let mut y = r.y as i32;
    let mut w = r.w as i32;
    let mut h = r.h as i32;
    match kind {
        DragKind::Move => {
            x += dx;
            y += dy;
        }
        DragKind::Resize {
            left,
            top,
            right,
            bottom,
        } => {
            if right {
                w = (w + dx).max(12);
            }
            if bottom {
                h = (h + dy).max(5);
            }
            if left {
                let delta = dx.min(w - 12).max(-x);
                x += delta;
                w -= delta;
            }
            if top {
                let delta = dy.min(h - 5).max(1 - y);
                y += delta;
                h -= delta;
            }
        }
    }
    Rect {
        x: x.max(0) as u16,
        y: y.max(1) as u16,
        w: w.min(500) as u16,
        h: h.min(200) as u16,
    }
    .fit(cols, rows)
}
fn current_cwd(w: &Window) -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Some(pid) = w.pid()
        && let Ok(cwd) = std::fs::read_link(format!("/proc/{pid}/cwd"))
    {
        return cwd;
    }
    #[cfg(target_os = "macos")]
    if let Some(pid) = w.pid() {
        let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
        let size = std::mem::size_of::<libc::proc_vnodepathinfo>();
        // SAFETY: proc_pidinfo receives valid storage and its exact byte size.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                info.as_mut_ptr().cast(),
                size as i32,
            )
        };
        if written == size as i32 {
            let info = unsafe { info.assume_init() };
            let path: Vec<u8> = info
                .pvi_cdir
                .vip_path
                .iter()
                .flatten()
                .map(|&c| c as u8)
                .take_while(|&b| b != 0)
                .collect();
            if !path.is_empty() {
                use std::os::unix::ffi::OsStringExt;
                return PathBuf::from(std::ffi::OsString::from_vec(path));
            }
        }
    }
    w.cwd.clone()
}
fn local_clock(seconds: u64) -> String {
    // Infer the platform's time representation from localtime_r; libc's
    // explicit time_t alias is deprecated on musl during its time64 transition.
    let Ok(seconds) = seconds.try_into() else {
        return "--:--".into();
    };
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers refer to valid storage of their specified types.
    if unsafe { libc::localtime_r(&seconds, tm.as_mut_ptr()) }.is_null() {
        return "--:--".into();
    }
    let tm = unsafe { tm.assume_init() };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}
fn frame(grid: &mut Grid, r: Rect, ascii: bool, style: Style) {
    grid.fill(r.x, r.y, r.w, r.h, " ", style.clone());
    outline(grid, r, ascii, style);
}
fn outline(grid: &mut Grid, r: Rect, ascii: bool, style: Style) {
    if r.w < 2 || r.h < 2 {
        return;
    }
    let (tl, tr, bl, br, h, v) = if ascii {
        ("+", "+", "+", "+", "-", "|")
    } else {
        ("┌", "┐", "└", "┘", "─", "│")
    };
    grid.fill(r.x + 1, r.y, r.w - 2, 1, h, style.clone());
    grid.fill(r.x + 1, r.y + r.h - 1, r.w - 2, 1, h, style.clone());
    grid.fill(r.x, r.y + 1, 1, r.h - 2, v, style.clone());
    grid.fill(r.x + r.w - 1, r.y + 1, 1, r.h - 2, v, style.clone());
    for (x, y, c) in [
        (r.x, r.y, tl),
        (r.x + r.w - 1, r.y, tr),
        (r.x, r.y + r.h - 1, bl),
        (r.x + r.w - 1, r.y + r.h - 1, br),
    ] {
        grid.put(x, y, c, 1, style.clone());
    }
}
