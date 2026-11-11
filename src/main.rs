mod agents;
mod app;
mod apps;
mod browser;
mod builtin;
mod client;
mod commands;
mod config;
mod editor;
mod geometry;
mod gpm;
mod input;
mod monitor_view;
mod projects;
mod protocol;
mod render;
mod session;
mod support;
mod theme;
mod web;
mod window;

use anyhow::{Context, Result, bail};
use std::{io::IsTerminal, os::unix::net::UnixStream, path::PathBuf};

const HELP: &str = "saumux — a retro terminal desktop\n\nUsage:\n  saumux                         Start, or attach to the only session\n  saumux new [name] [-- program [args…]]\n  saumux attach <name>\n  saumux ls\n  saumux web [--bind <address>] [--port <port>] [--session <name>]\n  saumux --layout <file> [--session <name>]\n  saumux ctl [--session <name>] <command> [args…]\n\nF10 opens menus. Alt+n starts a shell. Alt+d detaches.\nConfiguration: ~/.config/saumux/config.toml\nUse `saumux ctl commands` to list commands.\n";

fn main() {
    if let Err(error) = run() {
        eprintln!("saumux: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "__monitor") {
        return apps::monitor();
    }
    if args.first().is_some_and(|s| s == "__clock") {
        return apps::clock();
    }
    if args.first().is_some_and(|s| s == "__notes") {
        let path = args.get(1).context("Notes needs its storage path")?;
        return editor::run_named(Some(std::path::Path::new(path)), "Notes");
    }
    if args.first().is_some_and(|s| s == "__edit") {
        return editor::run(args.get(1).map(std::path::Path::new));
    }
    if args.first().is_some_and(|s| s == "__files") {
        return browser::run(std::path::Path::new(
            args.get(1).map(String::as_str).unwrap_or("."),
        ));
    }
    if args.first().is_some_and(|s| s == "--help" || s == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--version" || s == "-V") {
        println!("saumux {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "__server") {
        let name = args.get(1).context("missing session name")?.clone();
        let (layout, _, program) = options(&args[2..])?;
        return session::server(name, program, layout);
    }
    if args.first().is_some_and(|s| s == "ls") {
        for name in session::list()? {
            println!("{name}");
        }
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "web") {
        return web::run(&args[1..]);
    }
    if args.first().is_some_and(|s| s == "ctl") {
        let mut rest = &args[1..];
        let mut name = std::env::var("SAUMUX").ok();
        if rest.first().is_some_and(|s| s == "--session") {
            name = Some(rest.get(1).context("--session needs a name")?.clone());
            rest = &rest[2..];
        }
        let name = choose_session(name)?;
        let command = rest
            .first()
            .context("ctl needs a command; try commands or list")?
            .clone();
        let mut arguments: Vec<String> = rest[1..]
            .iter()
            .filter(|s| command != "list" || s.as_str() != "--json")
            .cloned()
            .collect();
        if command == "project.add" && !arguments.is_empty() {
            arguments[0] = std::path::absolute(&arguments[0])?
                .to_string_lossy()
                .into_owned();
        }
        if command == "agent.notify"
            && !arguments.iter().any(|a| a == "--window")
            && let Ok(window) = std::env::var("SAUMUX_WINDOW")
        {
            arguments.extend(["--window".into(), window]);
        }
        let result = session::control(&name, command, arguments)?;
        if !result.is_null() {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        return Ok(());
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("this command needs an interactive terminal; try --help");
    }
    if std::env::var_os("SAUMUX").is_some() {
        eprintln!(
            "saumux: nested desktop; F10 and mouse events may be handled by the outer session"
        );
    }
    if args.first().is_some_and(|s| s == "attach") {
        let name = args.get(1).context("attach needs a session name")?;
        if args.len() != 2 {
            bail!("usage: saumux attach <name>");
        }
        return client::attach(
            UnixStream::connect(session::socket_path(name)?)
                .with_context(|| format!("attaching to {name}"))?,
        );
    }
    let new = args.first().is_some_and(|s| s == "new");
    let mut rest = if new { &args[1..] } else { &args[..] };
    let positional_name = if new && rest.first().is_some_and(|s| !s.starts_with('-')) {
        let n = rest[0].clone();
        rest = &rest[1..];
        Some(n)
    } else {
        None
    };
    let (layout, flag_name, program) = options(rest)?;
    let name = positional_name.or(flag_name);
    if !new && layout.is_none() && program.is_empty() && name.is_none() {
        let sessions = session::list()?;
        if sessions.len() == 1 {
            return client::attach(UnixStream::connect(session::socket_path(&sessions[0])?)?);
        }
        if sessions.len() > 1 {
            bail!(
                "several sessions exist: {}; use saumux attach <name>",
                sessions.join(", ")
            );
        }
    }
    let name = name.unwrap_or_else(|| {
        if new {
            format!("session-{}", std::process::id())
        } else {
            "default".into()
        }
    });
    client::attach(session::start(&name, &program, layout.as_deref())?)
}
fn choose_session(explicit: Option<String>) -> Result<String> {
    if let Some(name) = explicit {
        return Ok(name);
    }
    let sessions = session::list()?;
    match sessions.as_slice() {
        [name] => Ok(name.clone()),
        [] => bail!("no sessions are running"),
        _ => bail!("several sessions exist; use --session <name>"),
    }
}
fn options(args: &[String]) -> Result<(Option<PathBuf>, Option<String>, Vec<String>)> {
    let mut layout = None;
    let mut name = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--" => return Ok((layout, name, args[i + 1..].to_vec())),
            "--layout" => {
                i += 1;
                layout = Some(PathBuf::from(args.get(i).context("--layout needs a file")?));
            }
            "--session" => {
                i += 1;
                name = Some(args.get(i).context("--session needs a name")?.clone());
            }
            value => bail!("unknown option {value}; put program arguments after --"),
        }
        i += 1;
    }
    Ok((layout, name, vec![]))
}
