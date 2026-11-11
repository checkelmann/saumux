//! Coding-agent discovery and visual attention state. Discovery never executes a CLI.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub command: Vec<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
}
fn enabled() -> bool {
    true
}
#[derive(Clone, Debug, Serialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub command: Vec<String>,
}
const KNOWN: &[(&str, &str, &str)] = &[
    ("codex", "Codex", "codex"),
    ("claude", "Claude Code", "claude"),
    ("gemini", "Gemini CLI", "gemini"),
    ("aider", "Aider", "aider"),
    ("opencode", "OpenCode", "opencode"),
    ("goose", "Goose", "goose"),
    ("amp", "Amp", "amp"),
    ("cursor", "Cursor Agent", "cursor-agent"),
];
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
}
pub fn validate(configs: &[AgentConfig]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for config in configs {
        if !valid_id(&config.id) || !ids.insert(&config.id) {
            bail!("agent IDs must be unique lowercase letters, digits, - or _");
        }
        if config.name.trim().is_empty()
            || config.name.chars().any(char::is_control)
            || config.name.chars().count() > 80
        {
            bail!(
                "agent {} needs a printable name of at most 80 characters",
                config.id
            );
        }
        if config.command.is_empty() || config.command[0].is_empty() {
            bail!("agent {} needs a command array", config.id);
        }
    }
    Ok(())
}
pub fn discover(configs: &[AgentConfig]) -> Vec<Agent> {
    let paths = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    discover_in(&paths, configs)
}
fn discover_in(paths: &[PathBuf], configs: &[AgentConfig]) -> Vec<Agent> {
    let mut result = vec![];
    for &(id, name, program) in KNOWN {
        if configs.iter().any(|c| c.id == id) {
            continue;
        }
        if let Some(path) = executable(program, paths) {
            result.push(Agent {
                id: id.into(),
                name: name.into(),
                command: vec![path.to_string_lossy().into_owned()],
            });
        }
    }
    for config in configs.iter().filter(|c| c.enabled) {
        if let Some(path) = executable(&config.command[0], paths) {
            let mut command = config.command.clone();
            command[0] = path.to_string_lossy().into_owned();
            result.push(Agent {
                id: config.id.clone(),
                name: config.name.clone(),
                command,
            });
        }
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}
fn executable(name: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let runnable = |path: &Path| {
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if name.contains('/') {
        let path = PathBuf::from(name);
        return runnable(&path)
            .then(|| std::path::absolute(path).ok())
            .flatten();
    }
    paths
        .iter()
        .map(|dir| dir.join(name))
        .find(|path| runnable(path))
        .and_then(|p| std::path::absolute(p).ok())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub project: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Notice {
    pub message: String,
    pub possible: bool,
}
pub struct Run {
    pub identity: Identity,
    pub notice: Option<Notice>,
    last_output: Instant,
    generation: u64,
    inspected: u64,
    last_prompt: Option<String>,
    suppressed: Option<String>,
    explicit: bool,
}
impl Run {
    pub fn new(identity: Identity) -> Self {
        Self {
            identity,
            notice: None,
            last_output: Instant::now(),
            generation: 0,
            inspected: 0,
            last_prompt: None,
            suppressed: None,
            explicit: false,
        }
    }
    pub fn output(&mut self, bell: bool, notification: Option<String>) {
        self.generation = self.generation.wrapping_add(1);
        self.last_output = Instant::now();
        if let Some(message) = notification {
            self.notify(if message.is_empty() {
                "Agent needs attention".into()
            } else {
                message
            });
        } else if bell {
            self.notify("Agent needs attention (terminal bell)".into());
        }
    }
    pub fn notify(&mut self, message: String) {
        self.notice = Some(Notice {
            message: message
                .chars()
                .filter(|c| !c.is_control())
                .take(240)
                .collect(),
            possible: false,
        });
        self.explicit = true;
    }
    pub fn acknowledge(&mut self) {
        self.notice = None;
        self.explicit = false;
        self.inspected = self.generation;
        self.suppressed = self.last_prompt.clone();
    }
    pub fn poll(&mut self, screen: &vt100::Screen, delay: Duration, detect: bool) -> bool {
        if !detect {
            if self.notice.as_ref().is_some_and(|n| n.possible) {
                self.notice = None;
                return true;
            }
            return false;
        }
        if self.generation == self.inspected || self.last_output.elapsed() < delay {
            return false;
        }
        self.inspected = self.generation;
        // Inspect the live viewport, not a user's scrolled-back history.
        let mut live = screen.clone();
        live.set_scrollback(0);
        let prompt = prompt_request(&live.contents());
        let before = self
            .notice
            .as_ref()
            .map(|n| (n.message.clone(), n.possible));
        match prompt {
            Some(prompt) => {
                if self.suppressed.as_ref() != Some(&prompt) && !self.explicit {
                    self.notice = Some(Notice {
                        message: format!("May need input: {prompt}"),
                        possible: true,
                    });
                }
                self.last_prompt = Some(prompt);
            }
            None => {
                self.last_prompt = None;
                self.suppressed = None;
                if !self.explicit {
                    self.notice = None;
                }
            }
        }
        before
            != self
                .notice
                .as_ref()
                .map(|n| (n.message.clone(), n.possible))
    }
}
fn prompt_request(contents: &str) -> Option<String> {
    // Strong request patterns only. Mere output inactivity is never an alert.
    let lines: Vec<_> = contents.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines
        .iter()
        .rev()
        .take(10)
        .rev()
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    for line in tail.lines().rev() {
        let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let lower = normalized.to_lowercase();
        let request = [
            "[y/n]",
            "(y/n)",
            "[y/n/",
            "waiting for your input",
            "waiting for user input",
            "needs your input",
            "press enter to",
            "press return to",
            "would you like to",
            "do you want to",
            "allow this",
            "approve this",
            "yes, proceed",
            "permission required",
            "permission requested",
        ]
        .iter()
        .any(|p| lower.contains(p));
        if request {
            return Some(normalized.chars().take(180).collect());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_uses_path_permissions_and_preserves_arguments_without_execution() {
        let root = std::env::temp_dir().join(format!("sx-discovery-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("codex");
        std::fs::write(&path, "#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(root.join("claude"), "not executable").unwrap();
        let custom = AgentConfig {
            id: "custom".into(),
            name: "Custom".into(),
            command: vec![
                path.to_string_lossy().into_owned(),
                "literal; argument".into(),
            ],
            enabled: true,
        };
        let discovered = discover_in(std::slice::from_ref(&root), &[custom]);
        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[1].command[1], "literal; argument");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn idle_text_is_not_a_prompt_and_acknowledgement_suppresses_repeats() {
        assert!(prompt_request("Completed tests.\nEverything passed.\n›").is_none());
        let mut parser = vt100::Parser::default();
        parser.process(b"Would you like to run this command? [y/n]");
        let mut run = Run::new(Identity {
            id: "fake".into(),
            name: "Fake".into(),
            project: None,
        });
        run.output(false, None);
        assert!(run.poll(parser.screen(), Duration::ZERO, true));
        assert!(run.notice.as_ref().unwrap().possible);
        run.acknowledge();
        run.output(false, None);
        run.poll(parser.screen(), Duration::ZERO, true);
        assert!(run.notice.is_none());
        run.notify("Approval requested".into());
        assert!(!run.notice.as_ref().unwrap().possible);
    }
}
