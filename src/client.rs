use crate::protocol::{self, Request, Response};
use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use std::{
    io::{self, IsTerminal, Write},
    os::unix::net::UnixStream,
    sync::mpsc,
    thread,
    time::Duration,
};

struct TerminalGuard;
// Restoration also runs in the panic hook. Pop the host's keyboard stack only
// once even when both that hook and TerminalGuard::drop execute.
static KEYBOARD_PUSHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            DisableLineWrap,
            Clear(ClearType::All),
            Hide,
            EnableMouseCapture,
            EnableBracketedPaste,
            EnableFocusChange
        )?;
        KEYBOARD_PUSHED.store(true, std::sync::atomic::Ordering::Relaxed);
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        Ok(guard)
    }
    fn restore() {
        if KEYBOARD_PUSHED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = io::stdout().write_all(b"\x1b[?2026l\x1b[0m");
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            DisableFocusChange,
            EnableLineWrap,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        Self::restore();
    }
}

pub fn attach(mut stream: UnixStream) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("attach needs an interactive terminal; use saumux ctl for scripts");
    }
    let (cols, rows) = terminal::size()?;
    let mut gpm = crate::gpm::Gpm::connect();
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        signal_hook::flag::register(signal, stopped.clone())?;
    }
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_CTYPE"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default()
        .to_lowercase();
    let utf8 = locale.contains("utf-8") || locale.contains("utf8");
    protocol::send(
        &mut stream,
        &Request::Hello {
            version: 1,
            cols,
            rows,
            utf8,
            console: gpm.is_some(),
        },
    )?;
    let mut input = stream.try_clone()?;
    let (tx, rx) = mpsc::sync_channel(16);
    thread::spawn(move || {
        loop {
            match protocol::receive(&mut stream) {
                Ok(message) => {
                    if tx.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break;
                }
            }
        }
    });
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        TerminalGuard::restore();
        old_hook(info);
    }));
    let _guard = TerminalGuard::enter().context("entering terminal mode")?;
    let mut stdout = io::stdout().lock();
    loop {
        if stopped.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        for _ in 0..16 {
            match rx.try_recv() {
                Ok(Ok(Response::Render(bytes))) => {
                    stdout.write_all(b"\x1b[?2026h")?;
                    stdout.write_all(bytes.as_bytes())?;
                    stdout.write_all(b"\x1b[?2026l")?;
                }
                Ok(Ok(Response::Detached)) => return Ok(()),
                Ok(Ok(Response::Error(message))) => bail!("{message}"),
                Ok(Err(e)) => return Err(e).context("connection to session lost"),
                Ok(Ok(Response::Result(_))) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => bail!("session server disconnected"),
            }
        }
        stdout.flush()?;
        if let Some(gpm) = &mut gpm {
            for event in gpm.poll() {
                protocol::send(&mut input, &Request::Input(event::Event::Mouse(event)))?;
            }
        }
        if event::poll(Duration::from_millis(8))? {
            protocol::send(&mut input, &Request::Input(event::read()?))?;
        }
    }
}
