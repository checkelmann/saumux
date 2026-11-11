//! Agent/project workflows, kept outside the core window manager.
use crate::{
    agents::{self, Agent, Identity},
    app::MenuEntry,
    builtin::Prompt,
    config::Config,
    geometry::Rect,
    projects::{Project, Projects},
    render::Grid,
    theme::Theme,
    window::Window,
};
use anyhow::{Context, Result, bail};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub const AGENTS_MENU: usize = 5;
pub const PROJECTS_MENU: usize = 6;
#[derive(Clone)]
struct Choice {
    label: String,
    command: String,
}
enum Modal {
    List {
        title: String,
        subtitle: String,
        choices: Vec<Choice>,
        selected: usize,
    },
    Directory {
        prompt: Prompt,
    },
    Remove {
        project: Project,
    },
}
pub enum Action {
    None,
    Launch {
        command: Vec<String>,
        cwd: PathBuf,
        agent: Option<Identity>,
    },
    Focus(u64),
}
pub struct Support {
    pub agents: Vec<Agent>,
    pub projects: Projects,
    modal: Option<Modal>,
}
impl Support {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            agents: agents::discover(&config.agents),
            projects: Projects::load()?,
            modal: None,
        })
    }
    pub fn active(&self) -> bool {
        self.modal.is_some()
    }
    pub fn error(&mut self, message: String) {
        self.modal = Some(Modal::List {
            title: "Unable to complete action".into(),
            subtitle: message,
            choices: vec![Choice {
                label: "OK".into(),
                command: "agent.dismiss".into(),
            }],
            selected: 0,
        });
    }
    pub fn entries(&self, index: usize, windows: &[Window]) -> Vec<MenuEntry> {
        let entry = |label: String, command: String| MenuEntry {
            label,
            command,
            shortcut: String::new(),
        };
        if index == AGENTS_MENU {
            let mut entries = self
                .agents
                .iter()
                .map(|agent| {
                    entry(
                        format!("Start {}", agent.name),
                        format!("agent.start.{}", agent.id),
                    )
                })
                .collect::<Vec<_>>();
            if entries.is_empty() {
                entries.push(entry(
                    "No agents found — rescan".into(),
                    "agent.rescan".into(),
                ));
            }
            for w in windows
                .iter()
                .filter(|w| w.agent.as_ref().is_some_and(|r| r.notice.is_some()))
            {
                entries.push(entry(
                    format!(
                        "! {} — {}",
                        w.agent.as_ref().unwrap().identity.name,
                        w.title
                    ),
                    format!("agent.focus.{}", w.id),
                ));
            }
            entries
        } else if index == PROJECTS_MENU {
            self.projects
                .projects
                .iter()
                .map(|p| {
                    entry(
                        format!("{}  — {}", p.name, p.path.display()),
                        format!("project.open.{}", p.id),
                    )
                })
                .collect()
        } else {
            vec![]
        }
    }
    pub fn pending(windows: &[Window]) -> usize {
        windows
            .iter()
            .filter(|w| w.agent.as_ref().is_some_and(|a| a.notice.is_some()))
            .count()
    }
    pub fn banner(windows: &[Window], config: &Config) -> String {
        let count = Self::pending(windows);
        let shortcut = crate::commands::shortcut("agent.attention", config);
        format!(
            " !{count} agents {} ",
            if shortcut.is_empty() {
                "click"
            } else {
                &shortcut
            }
        )
    }
    pub fn poll(&mut self, windows: &mut [Window], config: &Config) -> bool {
        let mut changed = false;
        for w in windows {
            if let Some(agent) = &mut w.agent {
                changed |= agent.poll(
                    w.parser.screen(),
                    Duration::from_millis(config.notifications.prompt_delay_ms),
                    config.notifications.detect_prompts,
                );
            }
        }
        changed
    }
    pub fn command(
        &mut self,
        name: &str,
        args: &[String],
        cwd: &Path,
        active: Option<u64>,
        windows: &mut [Window],
        config: &Config,
    ) -> Result<(Action, Value)> {
        if let Some(id) = name.strip_prefix("agent.start.") {
            return self.command("agent.start", &[id.into()], cwd, active, windows, config);
        }
        if let Some(id) = name.strip_prefix("project.open.") {
            return self.command("project.open", &[id.into()], cwd, active, windows, config);
        }
        if let Some(id) = name.strip_prefix("agent.focus.") {
            return Ok((Action::Focus(id.parse()?), json!(null)));
        }
        if let Some(suffix) = name.strip_prefix("project.action.") {
            let (id, action) = suffix.split_once('.').context("invalid project action")?;
            let project = self.projects.get(id)?.clone();
            match action {
                "shell" => {
                    return Ok((
                        Action::Launch {
                            command: crate::app::App::shell(),
                            cwd: project.path,
                            agent: None,
                        },
                        json!(null),
                    ));
                }
                "files" => {
                    return Ok((
                        Action::Launch {
                            command: crate::browser::launch_args("__files", &[])?,
                            cwd: project.path,
                            agent: None,
                        },
                        json!(null),
                    ));
                }
                "editor" => {
                    return Ok((
                        Action::Launch {
                            command: crate::browser::launch_args("__edit", &[])?,
                            cwd: project.path,
                            agent: None,
                        },
                        json!(null),
                    ));
                }
                "remove" => {
                    self.modal = Some(Modal::Remove { project });
                    return Ok((Action::None, json!(null)));
                }
                agent => {
                    return Ok((
                        self.launch(
                            agent
                                .strip_prefix("agent:")
                                .context("invalid project action")?,
                            &project.path,
                            Some(project.name),
                        )?,
                        json!(null),
                    ));
                }
            }
        }
        match name {
            "agent.dismiss" => Ok((Action::None, json!(null))),
            "agent.list" => Ok((Action::None, json!(self.agents))),
            "agent.rescan" => {
                self.agents = agents::discover(&config.agents);
                Ok((Action::None, json!({"found":self.agents.len()})))
            }
            "agent.start" => {
                if args.is_empty() {
                    self.modal = Some(Modal::List {
                        title: "Start coding agent".into(),
                        subtitle: cwd.display().to_string(),
                        choices: self
                            .agents
                            .iter()
                            .map(|a| Choice {
                                label: a.name.clone(),
                                command: format!("agent.start.{}", a.id),
                            })
                            .collect(),
                        selected: 0,
                    });
                    return Ok((Action::None, json!(null)));
                }
                let id = &args[0];
                let project = if args.len() == 3 && args[1] == "--project" {
                    Some(self.projects.get(&args[2])?.clone())
                } else if args.len() == 1 {
                    None
                } else {
                    bail!("usage: agent.start <id> [--project <project ID or name>]")
                };
                let action = if let Some(project) = project {
                    self.launch(id, &project.path, Some(project.name))?
                } else {
                    self.launch(id, cwd, None)?
                };
                Ok((action, json!(null)))
            }
            "agent.attention" => {
                let choices = windows
                    .iter()
                    .filter_map(|w| {
                        w.agent.as_ref().and_then(|a| {
                            a.notice.as_ref().map(|n| Choice {
                                label: format!(
                                    "{} · {}: {}",
                                    a.identity.name,
                                    a.identity.project.as_deref().unwrap_or("no project"),
                                    n.message
                                ),
                                command: format!("agent.focus.{}", w.id),
                            })
                        })
                    })
                    .collect();
                self.modal = Some(Modal::List {
                    title: "Agent attention".into(),
                    subtitle: "Enter / click to focus; typing in an agent clears its alert".into(),
                    choices,
                    selected: 0,
                });
                Ok((Action::None, json!(null)))
            }
            "agent.ack" => {
                let id = if let Some(id) = args.first() {
                    id.parse()?
                } else {
                    active.context("no active window")?
                };
                let agent = windows
                    .iter_mut()
                    .find(|w| w.id == id)
                    .and_then(|w| w.agent.as_mut())
                    .context("window is not an agent")?;
                agent.acknowledge();
                Ok((Action::None, json!(null)))
            }
            "agent.notify" => {
                let state = args
                    .first()
                    .context("agent.notify needs waiting, attention, done or working")?;
                let index = args
                    .iter()
                    .position(|a| a == "--window")
                    .context("agent.notify needs --window <id> (automatic inside agent windows)")?;
                let id: u64 = args
                    .get(index + 1)
                    .context("--window needs an ID")?
                    .parse()?;
                let agent = windows
                    .iter_mut()
                    .find(|w| w.id == id)
                    .and_then(|w| w.agent.as_mut())
                    .context("window is not an agent")?;
                let message = args.get(1..index).unwrap_or_default().join(" ");
                match state.as_str() {
                    "waiting" => agent.notify(if message.is_empty() {
                        "Agent is waiting for your input".into()
                    } else {
                        message
                    }),
                    "attention" => agent.notify(if message.is_empty() {
                        "Agent needs attention".into()
                    } else {
                        message
                    }),
                    "done" => agent.notify(if message.is_empty() {
                        "Agent finished a turn".into()
                    } else {
                        message
                    }),
                    "working" => agent.acknowledge(),
                    _ => bail!("unknown agent notification state {state}"),
                }
                Ok((Action::None, json!(null)))
            }
            "project.list" => {
                self.projects = Projects::load()?;
                Ok((Action::None, json!(self.projects.projects)))
            }
            "project.refresh" => {
                self.projects = Projects::load()?;
                Ok((
                    Action::None,
                    json!({"projects":self.projects.projects.len()}),
                ))
            }
            "project.add" => {
                if args.is_empty() {
                    self.modal = Some(Modal::Directory {
                        prompt: Prompt::new("Directory", cwd.display().to_string()),
                    });
                    return Ok((Action::None, json!(null)));
                }
                let path = PathBuf::from(&args[0]);
                let path = if path.is_absolute() {
                    path
                } else {
                    cwd.join(path)
                };
                let label = if args.len() == 3 && args[1] == "--name" {
                    Some(args[2].clone())
                } else if args.len() == 1 {
                    None
                } else {
                    bail!("usage: project.add <directory> [--name <label>]")
                };
                let project = self.projects.add(&path, label)?;
                Ok((Action::None, json!(project)))
            }
            "project.save_current" => {
                let project = self.projects.add(cwd, None)?;
                Ok((Action::None, json!(project)))
            }
            "project.remove" => {
                let project = self
                    .projects
                    .get(
                        args.first()
                            .context("project.remove needs a project ID or name")?,
                    )?
                    .clone();
                self.projects.remove(project.id)?;
                Ok((Action::None, json!(null)))
            }
            "project.open" => {
                self.projects = Projects::load()?;
                if let Some(id) = args.first() {
                    let project = self.projects.get(id)?.clone();
                    let mut choices = self
                        .agents
                        .iter()
                        .map(|a| Choice {
                            label: format!("Start {}", a.name),
                            command: format!("project.action.{}.agent:{}", project.id, a.id),
                        })
                        .collect::<Vec<_>>();
                    for (label, action) in [
                        ("New shell here", "shell"),
                        ("Browse files here", "files"),
                        ("New text document here", "editor"),
                        ("Remove from saved projects…", "remove"),
                    ] {
                        choices.push(Choice {
                            label: label.into(),
                            command: format!("project.action.{}.{action}", project.id),
                        });
                    }
                    self.modal = Some(Modal::List {
                        title: format!("Project: {}", project.name),
                        subtitle: project.path.display().to_string(),
                        choices,
                        selected: 0,
                    });
                } else {
                    self.modal = Some(Modal::List {
                        title: "Saved projects".into(),
                        subtitle: "Choose a project to launch an agent, shell or browser".into(),
                        choices: self
                            .projects
                            .projects
                            .iter()
                            .map(|p| Choice {
                                label: format!("{} — {}", p.name, p.path.display()),
                                command: format!("project.open.{}", p.id),
                            })
                            .collect(),
                        selected: 0,
                    });
                }
                Ok((Action::None, json!(null)))
            }
            _ => bail!("unknown support command {name}"),
        }
    }
    fn launch(&self, id: &str, cwd: &Path, project: Option<String>) -> Result<Action> {
        let agent = self
            .agents
            .iter()
            .find(|a| a.id == id)
            .context("agent is not installed; rescan agents or check PATH")?;
        if !cwd.is_dir() {
            bail!("project directory is unavailable: {}", cwd.display());
        }
        Ok(Action::Launch {
            command: agent.command.clone(),
            cwd: cwd.into(),
            agent: Some(Identity {
                id: agent.id.clone(),
                name: agent.name.clone(),
                project,
            }),
        })
    }
    fn rect(cols: u16, rows: u16) -> Rect {
        let w = cols.min(76);
        let h = rows.saturating_sub(2).min(18);
        Rect {
            x: cols.saturating_sub(w) / 2,
            y: rows.saturating_sub(h) / 2,
            w,
            h,
        }
    }
    pub fn key(&mut self, key: KeyEvent, cols: u16, rows: u16) -> Result<Option<String>> {
        let Some(modal) = self.modal.as_mut() else {
            return Ok(None);
        };
        match modal {
            Modal::List {
                choices, selected, ..
            } => match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Down => *selected = (*selected + 1).min(choices.len().saturating_sub(1)),
                KeyCode::Home => *selected = 0,
                KeyCode::End => *selected = choices.len().saturating_sub(1),
                KeyCode::PageUp => {
                    *selected =
                        selected.saturating_sub(Self::rect(cols, rows).h.saturating_sub(4) as usize)
                }
                KeyCode::PageDown => {
                    *selected = (*selected + Self::rect(cols, rows).h.saturating_sub(4) as usize)
                        .min(choices.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    let command = choices.get(*selected).map(|c| c.command.clone());
                    self.modal = None;
                    return Ok(command);
                }
                _ => {}
            },
            Modal::Directory { prompt } => match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter => {
                    let value = prompt.text.clone();
                    self.modal = None;
                    return Ok(Some(format!("project.add.path:{value}")));
                }
                _ => prompt.edit(key),
            },
            Modal::Remove { project } => match key.code {
                KeyCode::Char('y' | 'Y') => {
                    let id = project.id;
                    self.modal = None;
                    return Ok(Some(format!("project.remove:{id}")));
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => self.modal = None,
                _ => {}
            },
        }
        Ok(None)
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(Modal::Directory { prompt }) = &mut self.modal {
            let text = text.replace(['\r', '\n'], "");
            let byte = prompt
                .text
                .char_indices()
                .nth(prompt.cursor)
                .map(|(i, _)| i)
                .unwrap_or(prompt.text.len());
            prompt.text.insert_str(byte, &text);
            prompt.cursor += text.chars().count();
        }
    }
    pub fn mouse(&mut self, event: MouseEvent, cols: u16, rows: u16) -> Option<String> {
        let r = Self::rect(cols, rows);
        let modal = self.modal.as_mut()?;
        if let Modal::List {
            choices, selected, ..
        } = modal
        {
            let visible = r.h.saturating_sub(4) as usize;
            let offset = selected.saturating_sub(visible.saturating_sub(1));
            match event.kind {
                MouseEventKind::ScrollUp => *selected = selected.saturating_sub(1),
                MouseEventKind::ScrollDown => {
                    *selected = (*selected + 1).min(choices.len().saturating_sub(1))
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if event.column > r.x
                        && event.column < r.x + r.w.saturating_sub(1)
                        && event.row >= r.y + 2
                        && event.row < r.y + r.h.saturating_sub(2)
                    {
                        let index = offset + (event.row - r.y - 2) as usize;
                        let command = choices.get(index).map(|c| c.command.clone());
                        self.modal = None;
                        return command;
                    }
                    self.modal = None;
                }
                _ => {}
            }
        } else if event.kind == MouseEventKind::Down(MouseButton::Left)
            && event.row == r.y + r.h.saturating_sub(2)
        {
            match modal {
                Modal::Directory { prompt } => {
                    if event.column < r.x + 14 {
                        let path = prompt.text.clone();
                        self.modal = None;
                        return Some(format!("project.add.path:{path}"));
                    }
                    self.modal = None;
                }
                Modal::Remove { project } => {
                    if event.column < r.x + 14 {
                        let id = project.id;
                        self.modal = None;
                        return Some(format!("project.remove:{id}"));
                    }
                    self.modal = None;
                }
                _ => {}
            }
        }
        None
    }
    pub fn draw(&self, grid: &mut Grid, theme: &Theme, ascii: bool) {
        let Some(modal) = &self.modal else {
            return;
        };
        let r = Self::rect(grid.cols, grid.rows);
        grid.fill(r.x + 1, r.y + 1, r.w, r.h, " ", theme.shadow.clone());
        crate::app::frame(grid, r, ascii, theme.dialog.clone());
        match modal {
            Modal::List {
                title,
                subtitle,
                choices,
                selected,
            } => {
                grid.text(
                    r.x + 2,
                    r.y,
                    &format!(" {title} "),
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
                grid.text(
                    r.x + 2,
                    r.y + 1,
                    subtitle,
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
                let visible = r.h.saturating_sub(4) as usize;
                let offset = selected.saturating_sub(visible.saturating_sub(1));
                for (index, choice) in choices.iter().enumerate().skip(offset).take(visible) {
                    let y = r.y + 2 + (index - offset) as u16;
                    let style = if index == *selected {
                        theme.selected.clone()
                    } else {
                        theme.dialog.clone()
                    };
                    grid.fill(r.x + 1, y, r.w.saturating_sub(2), 1, " ", style.clone());
                    grid.text(r.x + 2, y, &choice.label, r.w.saturating_sub(4), style);
                }
                if choices.is_empty() {
                    grid.text(
                        r.x + 2,
                        r.y + 2,
                        "No entries. Esc returns.",
                        r.w.saturating_sub(4),
                        theme.dialog.clone(),
                    );
                }
                grid.text(
                    r.x + 2,
                    r.y + r.h.saturating_sub(2),
                    "Click / Enter chooses · Esc cancels",
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
            }
            Modal::Directory { prompt } => {
                grid.text(
                    r.x + 2,
                    r.y,
                    " Save project directory ",
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
                grid.text(
                    r.x + 2,
                    r.y + 1,
                    "Directory: Ctrl+U clears the field",
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
                let chars: Vec<_> = prompt.text.chars().collect();
                let cursor = crate::builtin::column(&chars, prompt.cursor);
                let width = r.w.saturating_sub(4);
                let left = cursor.saturating_sub(width.saturating_sub(1) as usize);
                let text: String = chars
                    .iter()
                    .skip(crate::builtin::index_at(&chars, left))
                    .collect();
                grid.fill(r.x + 2, r.y + 3, width, 1, " ", theme.field.clone());
                grid.text(r.x + 2, r.y + 3, &text, width, theme.field.clone());
                if width > 0 {
                    grid.cursor = Some((r.x + 2 + (cursor - left) as u16, r.y + 3));
                }
                grid.text(
                    r.x + 2,
                    r.y + r.h.saturating_sub(2),
                    "[Enter] Save   [Esc] Cancel",
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
            }
            Modal::Remove { project } => {
                grid.text(
                    r.x + 2,
                    r.y,
                    " Remove saved project? ",
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
                grid.text(
                    r.x + 2,
                    r.y + 2,
                    &project.name,
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
                grid.text(
                    r.x + 2,
                    r.y + 3,
                    "Only removes the bookmark; keeps all files.",
                    r.w.saturating_sub(4),
                    theme.dialog.clone(),
                );
                grid.text(
                    r.x + 2,
                    r.y + r.h.saturating_sub(2),
                    "[Y] Remove    [N] Cancel",
                    r.w.saturating_sub(4),
                    theme.warning.clone(),
                );
            }
        }
    }
}
