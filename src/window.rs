use crate::geometry::Rect;
use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    num::NonZeroU16,
    path::PathBuf,
    sync::mpsc::SyncSender,
    thread,
};

#[derive(Default)]
pub struct Callbacks {
    pub title: Option<String>,
    pub bell: bool,
    pub replies: Vec<u8>,
    pub notification: Option<String>,
}
impl vt100::Callbacks for Callbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(
            String::from_utf8_lossy(title)
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect(),
        );
    }
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }
    fn visual_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let pieces = if params.first().is_some_and(|p| *p == b"9") {
            if params
                .get(1)
                .is_some_and(|p| !p.is_empty() && p.iter().all(u8::is_ascii_digit))
            {
                return;
            }
            &params[1..]
        } else if params.first().is_some_and(|p| *p == b"777")
            && params.get(1).is_some_and(|p| *p == b"notify")
        {
            &params[2..]
        } else {
            return;
        };
        self.notification = Some(
            pieces
                .iter()
                .map(|p| String::from_utf8_lossy(p))
                .collect::<Vec<_>>()
                .join(": ")
                .chars()
                .filter(|c| !c.is_control())
                .take(240)
                .collect(),
        );
    }
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let n = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let reply = match (i1, n, c) {
            (None, 5, 'n') => "\x1b[0n".into(),
            (None, 6, 'n') => {
                let (r, c) = screen.cursor_position();
                format!("\x1b[{};{}R", r + 1, c + 1)
            }
            (None, 0, 'c') => "\x1b[?1;2c".into(),
            (Some(b'>'), 0, 'c') => "\x1b[>0;1;0c".into(),
            (None, 18, 't') => {
                let (r, c) = screen.size();
                format!("\x1b[8;{r};{c}t")
            }
            _ => String::new(),
        };
        self.replies.extend_from_slice(reply.as_bytes());
    }
}

pub struct Window {
    pub id: u64,
    pub title: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub rect: Rect,
    pub restore: Option<Rect>,
    pub workspace: usize,
    pub shelved: bool,
    pub activity: bool,
    pub color: bool,
    pub parser: vt100::Parser<Callbacks>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    pub input: SyncSender<Vec<u8>>,
    pub agent: Option<crate::agents::Run>,
}

impl Window {
    pub fn is_editor(&self) -> bool {
        self.command.get(1).is_some_and(|s| s == "__edit")
            && std::env::current_exe().is_ok_and(|exe| {
                self.command
                    .first()
                    .is_some_and(|s| std::path::Path::new(s) == exe)
            })
    }
    pub fn spawn(
        id: u64,
        rect: Rect,
        command: Vec<String>,
        cwd: PathBuf,
        session: &str,
        events: SyncSender<crate::session::ServerEvent>,
    ) -> Result<Self> {
        let (cols, rows) = rect.inner();
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            ..PtySize::default()
        })?;
        let mut cmd = CommandBuilder::new(&command[0]);
        cmd.args(&command[1..]);
        cmd.cwd(&cwd);
        static TERM: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
        let term = TERM.get_or_init(|| {
            let installed = std::process::Command::new("infocmp")
                .arg("saumux-256color")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if installed {
                "saumux-256color"
            } else {
                "screen-256color"
            }
        });
        cmd.env("TERM", *term);
        cmd.env("COLORTERM", "truecolor");
        cmd.env("SAUMUX", session);
        cmd.env("SAUMUX_WINDOW", id.to_string());
        cmd.env("SAUMUX_BIN", std::env::current_exe()?);
        cmd.env("SAUMUX_THEME_FILE", crate::session::theme_path(session)?);
        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("starting {}", command[0]))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut input_writer = pair.master.take_writer()?;
        let (input, receiver) = std::sync::mpsc::sync_channel::<Vec<u8>>(128);
        // A stalled application cannot block the server's event loop.
        thread::spawn(move || {
            while let Ok(bytes) = receiver.recv() {
                if input_writer.write_all(&bytes).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        if events
                            .send(crate::session::ServerEvent::Output(
                                id,
                                buffer[..n].to_vec(),
                            ))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let _ = events.send(crate::session::ServerEvent::Exited(id));
        });
        Ok(Self {
            id,
            title: command[0].clone(),
            command,
            cwd,
            rect,
            restore: None,
            workspace: 0,
            shelved: false,
            activity: false,
            color: false,
            parser: vt100::Parser::new_with_callbacks(
                NonZeroU16::new(rows).unwrap(),
                NonZeroU16::new(cols).unwrap(),
                10000,
                Callbacks::default(),
            ),
            master: pair.master,
            child,
            input,
            agent: None,
        })
    }
    pub fn resize(&mut self, rect: Rect) -> Result<()> {
        let (cols, rows) = rect.inner();
        let current = self.parser.screen().size();
        if (current.1.get(), current.0.get()) != (cols, rows) {
            self.master.resize(PtySize {
                rows,
                cols,
                ..PtySize::default()
            })?;
            self.parser
                .set_size(rows.try_into().unwrap(), cols.try_into().unwrap());
        }
        self.rect = rect;
        Ok(())
    }
    pub fn output(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        let callbacks = self.parser.callbacks_mut();
        if let Some(title) = callbacks.title.take() {
            self.title = title;
        }
        let bell = callbacks.bell;
        if self.shelved || bell {
            self.activity = true;
        }
        callbacks.bell = false;
        if let Some(agent) = &mut self.agent {
            agent.output(bell, std::mem::take(&mut callbacks.notification));
        } else {
            callbacks.notification = None;
        }
        let replies = std::mem::take(&mut callbacks.replies);
        if !replies.is_empty() {
            let _ = self.input.try_send(replies);
        }
    }
    pub fn send(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.parser.screen_mut().set_scrollback(0);
        self.input
            .try_send(bytes)
            .context("program input queue is full or closed")?;
        Ok(())
    }
    pub fn scroll(&mut self, delta: i32) {
        let offset = self.parser.screen().scrollback();
        self.parser
            .screen_mut()
            .set_scrollback(offset.saturating_add_signed(delta as isize));
    }
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        // Signal the PTY's foreground job as well as the shell, then reap it.
        #[cfg(unix)]
        if let Some(fd) = self.master.as_raw_fd() {
            // SAFETY: fd is owned by master; a positive group denotes a PTY job.
            let group = unsafe { libc::tcgetpgrp(fd) };
            if group > 0 {
                unsafe {
                    libc::kill(-group, libc::SIGHUP);
                }
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notifications_ignore_progress_and_capture_osc777_and_bell() {
        let mut parser = vt100::Parser::new_with_callbacks(
            5.try_into().unwrap(),
            40.try_into().unwrap(),
            0,
            Callbacks::default(),
        );
        parser.process(b"\x1b]9;4;1;30\x07");
        assert!(parser.callbacks().notification.is_none());
        assert!(!parser.callbacks().bell);
        parser.process(b"\x1b]777;notify;Agent;Input requested\x1b\\");
        assert_eq!(
            parser.callbacks().notification.as_deref(),
            Some("Agent: Input requested")
        );
        parser.process(b"\x07");
        assert!(parser.callbacks().bell);
    }
    #[test]
    fn emulator_handles_titles_queries_and_alternate_screen() {
        let mut parser = vt100::Parser::new_with_callbacks(
            5.try_into().unwrap(),
            20.try_into().unwrap(),
            100,
            Callbacks::default(),
        );
        parser.process(b"shell\x1b]2;editor\x07\x1b[?1049h\x1b[2J\x1b[Hvim\x1b[6n");
        assert_eq!(parser.callbacks().title.as_deref(), Some("editor"));
        assert!(parser.screen().alternate_screen());
        assert_eq!(parser.callbacks().replies, b"\x1b[1;4R");
        parser.process(b"\x1b[?1049l");
        assert!(parser.screen().contents().starts_with("shell"));
    }
}
