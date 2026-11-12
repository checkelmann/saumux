use crate::{
    app::App,
    config::Config,
    protocol::{self, Request, Response},
    render::Grid,
};
use anyhow::{Context, Result, bail};
use crossterm::event::Event;
use std::{
    collections::HashMap,
    fs, io,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, SyncSender},
    thread,
    time::{Duration, Instant},
};

pub enum ServerEvent {
    Connected {
        id: u64,
        writer: SyncSender<Response>,
        cols: u16,
        rows: u16,
        utf8: bool,
        console: bool,
    },
    Input(u64, Event),
    Disconnected(u64),
    Output(u64, Vec<u8>),
    Exited(u64),
    Control(String, Vec<String>, UnixStream),
}
struct Client {
    writer: SyncSender<Response>,
    cols: u16,
    rows: u16,
    utf8: bool,
    console: bool,
    previous: Option<Grid>,
    look: Option<crate::config::Look>,
}
struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("theme"));
    }
}
pub fn theme_path(name: &str) -> Result<PathBuf> {
    Ok(socket_path(name)?.with_extension("theme"))
}
pub fn write_theme(name: &str, theme: crate::theme::ThemeId) -> Result<()> {
    use std::io::Write;
    let path = theme_path(name)?;
    let temp = path.with_extension(format!("theme-tmp-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        writeln!(file, "{}", theme.id())?;
        fs::rename(&temp, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

pub fn runtime_dir() -> Result<PathBuf> {
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("saumux-{uid}")));
    let path = if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
        root.join("saumux")
    } else {
        root
    };
    match fs::DirBuilder::new().mode(0o700).create(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
    }
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_dir() || meta.uid() != uid || meta.permissions().mode() & 0o077 != 0 {
        bail!(
            "{} must be a directory owned by you with mode 0700",
            path.display()
        );
    }
    Ok(path)
}
pub fn socket_path(name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 40
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        bail!("session names must be 1–40 letters, digits, underscores or hyphens");
    }
    Ok(runtime_dir()?.join(format!("{name}.sock")))
}
pub fn list() -> Result<Vec<String>> {
    let mut names = vec![];
    for entry in fs::read_dir(runtime_dir()?)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "sock")
            && UnixStream::connect(&path).is_ok()
            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
        {
            names.push(name.into());
        }
    }
    names.sort();
    Ok(names)
}
pub fn start(name: &str, program: &[String], layout: Option<&Path>) -> Result<UnixStream> {
    let path = socket_path(name)?;
    if UnixStream::connect(&path).is_ok() {
        bail!("session {name} already exists");
    }
    if path.exists() {
        fs::remove_file(&path).context("removing stale session socket")?;
    }
    let log_path = path.with_extension("log");
    let log = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&log_path)?;
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("__server").arg(name);
    if let Some(layout) = layout {
        command.arg("--layout").arg(layout);
    }
    command
        .arg("--")
        .args(program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    // SAFETY: setsid is async-signal-safe and touches no Rust state after fork.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().context("starting session server")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(stream) = UnixStream::connect(&path) {
            return Ok(stream);
        }
        if let Some(status) = child.try_wait()? {
            bail!(
                "server exited ({status}): {}",
                fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("server did not start; see {}", log_path.display());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
pub fn control(name: &str, command: String, args: Vec<String>) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(socket_path(name)?)
        .with_context(|| format!("connecting to session {name}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    protocol::send(&mut stream, &Request::Control { command, args })?;
    match protocol::receive(&mut stream)? {
        Response::Result(value) => Ok(value),
        Response::Error(message) => bail!("{message}"),
        _ => bail!("unexpected response from server"),
    }
}
fn peer_allowed(stream: &UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "linux")]
    {
        let mut cred = std::mem::MaybeUninit::<libc::ucred>::uninit();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: getsockopt writes at most len bytes to this storage on success.
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                cred.as_mut_ptr().cast(),
                &mut len,
            ) == 0
                && cred.assume_init().uid == libc::geteuid()
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut uid = 0;
        let mut gid = 0;
        // SAFETY: getpeereid writes to valid uid and gid pointers.
        unsafe {
            libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid()
        }
    }
}
fn serve_connection(mut stream: UnixStream, id: u64, events: SyncSender<ServerEvent>) {
    if !peer_allowed(&stream) {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    match protocol::receive::<Request>(&mut stream) {
        Ok(Request::Control { command, args }) => {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let _ = events.send(ServerEvent::Control(command, args, stream));
        }
        Ok(Request::Hello {
            version: 1,
            cols,
            rows,
            utf8,
            console,
        }) => {
            let _ = stream.set_read_timeout(None);
            let Ok(mut writer) = stream.try_clone() else {
                return;
            };
            let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
            let (tx, rx) = mpsc::sync_channel(8);
            if events
                .send(ServerEvent::Connected {
                    id,
                    writer: tx,
                    cols,
                    rows,
                    utf8,
                    console,
                })
                .is_err()
            {
                return;
            }
            let write_events = events.clone();
            thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    if protocol::send(&mut writer, &message).is_err() {
                        break;
                    }
                }
                let _ = writer.shutdown(std::net::Shutdown::Both);
                let _ = write_events.send(ServerEvent::Disconnected(id));
            });
            while let Ok(Request::Input(event)) = protocol::receive(&mut stream) {
                if events.send(ServerEvent::Input(id, event)).is_err() {
                    break;
                }
            }
            let _ = events.send(ServerEvent::Disconnected(id));
        }
        Ok(_) => {
            let _ = protocol::send(
                &mut stream,
                &Response::Error("unsupported protocol; use a matching saumux binary".into()),
            );
        }
        Err(_) => {}
    }
}
pub fn server(name: String, program: Vec<String>, layout: Option<PathBuf>) -> Result<()> {
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [libc::SIGTERM, libc::SIGINT] {
        signal_hook::flag::register(signal, stopped.clone())?;
    }
    let path = socket_path(&name)?;
    let listener =
        UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
    let _guard = SocketGuard(path.clone());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let (events, rx) = mpsc::sync_channel(256);
    let mut app = App::new(name, Config::load()?, events.clone())?;
    write_theme(&app.name, app.config.look.theme)?;
    if let Some(layout) = layout {
        app.load_layout(&layout)?;
    } else if !program.is_empty() {
        app.new_window(program, None)?;
    } else if !app.config.start.is_empty() {
        for program in app.config.start.clone() {
            app.new_window(vec!["/bin/sh".into(), "-lc".into(), program], None)?;
        }
    } else {
        app.new_window(App::shell(), None)?;
    }
    thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            match stream {
                Ok(stream) => {
                    let events = events.clone();
                    thread::spawn(move || serve_connection(stream, i as u64 + 1, events));
                }
                Err(_) => break,
            }
        }
    });
    let mut clients: HashMap<u64, Client> = HashMap::new();
    let mut dirty = true;
    let mut last_render = Instant::now() - Duration::from_secs(1);
    let mut last_clock = Instant::now();
    while app.running {
        if stopped.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(16)) {
            handle_event(event, &mut app, &mut clients, &mut dirty);
            // Bound each batch so continuous program output cannot starve input.
            for _ in 0..127 {
                match rx.try_recv() {
                    Ok(event) => handle_event(event, &mut app, &mut clients, &mut dirty),
                    Err(_) => break,
                }
            }
        }
        dirty |= app.poll_agents();
        if let Some(id) = app.detach.take() {
            if let Some(client) = clients.remove(&id) {
                let _ = client.writer.try_send(Response::Detached);
            }
            app.disconnected(id);
            resize_to_clients(&mut app, &clients);
            dirty = true;
        }
        if let Some((id, output)) = app.clipboard_output.take()
            && let Some(client) = clients.get(&id)
        {
            let _ = client.writer.try_send(Response::Render(output));
        }
        if last_clock.elapsed() >= Duration::from_secs(1) {
            dirty = true;
            last_clock = Instant::now();
        }
        if dirty && last_render.elapsed() >= Duration::from_nanos(1_000_000_000 / 60) {
            let mut failed = vec![];
            for (&id, client) in &mut clients {
                let grid = app.compose(id, !client.utf8, client.console);
                if client.look.as_ref() != Some(&app.config.look) {
                    client.previous = None;
                }
                let mut bytes = Vec::new();
                if client
                    .previous
                    .as_ref()
                    .is_none_or(|p| p.cols != grid.cols || p.rows != grid.rows)
                {
                    bytes.extend_from_slice(b"\x1b[0m\x1b[2J");
                }
                bytes.extend(grid.diff(client.previous.as_ref(), &app.config.look));
                if client
                    .writer
                    .try_send(Response::Render(
                        String::from_utf8(bytes).expect("renderer emits UTF-8"),
                    ))
                    .is_err()
                {
                    failed.push(id);
                } else {
                    client.previous = Some(grid);
                    client.look = Some(app.config.look.clone());
                }
            }
            for id in failed {
                clients.remove(&id);
                app.disconnected(id);
            }
            dirty = false;
            last_render = Instant::now();
        }
    }
    for client in clients.values() {
        let _ = client.writer.try_send(Response::Detached);
    }
    let expected = clients.len();
    clients.clear();
    // Let socket writers send the final detach before the process exits.
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut disconnected = std::collections::HashSet::new();
    while disconnected.len() < expected && Instant::now() < deadline {
        if let Ok(ServerEvent::Disconnected(id)) = rx.recv_timeout(Duration::from_millis(10)) {
            disconnected.insert(id);
        }
    }
    drop(app);
    Ok(())
}
fn valid_size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(1, 500), rows.clamp(1, 200))
}
fn resize_to_clients(app: &mut App, clients: &HashMap<u64, Client>) {
    if clients.is_empty() {
        return;
    }
    let cols = clients.values().map(|c| c.cols).min().unwrap();
    let rows = clients.values().map(|c| c.rows).min().unwrap();
    if (app.cols, app.rows) != (cols, rows)
        && let Err(e) = app.resize(cols, rows)
    {
        app.status = e.to_string();
    }
}
fn handle_event(
    event: ServerEvent,
    app: &mut App,
    clients: &mut HashMap<u64, Client>,
    dirty: &mut bool,
) {
    *dirty = true;
    match event {
        ServerEvent::Connected {
            id,
            writer,
            cols,
            rows,
            utf8,
            console,
        } => {
            let (cols, rows) = valid_size(cols, rows);
            app.rescan_agents();
            clients.insert(
                id,
                Client {
                    writer,
                    cols,
                    rows,
                    utf8,
                    console,
                    previous: None,
                    look: None,
                },
            );
            resize_to_clients(app, clients);
        }
        ServerEvent::Disconnected(id) => {
            clients.remove(&id);
            app.disconnected(id);
            resize_to_clients(app, clients);
        }
        ServerEvent::Input(id, event) => {
            if !clients.contains_key(&id) {
                return;
            }
            if let Event::Resize(cols, rows) = event {
                let (cols, rows) = valid_size(cols, rows);
                let client = clients.get_mut(&id).unwrap();
                client.cols = cols;
                client.rows = rows;
                client.previous = None;
                let _ = client
                    .writer
                    .try_send(Response::Render("\x1b[0m\x1b[2J".into()));
                resize_to_clients(app, clients);
            } else if let Err(e) = app.event(event, id) {
                app.status = e.to_string();
            }
        }
        ServerEvent::Output(id, bytes) => {
            if let Some(w) = app.windows.iter_mut().find(|w| w.id == id) {
                w.output(&bytes);
            }
        }
        ServerEvent::Exited(id) => app.remove(id),
        ServerEvent::Control(command, args, mut reply) => {
            let response = match app.command(&command, &args, 0) {
                Ok(value) => Response::Result(value),
                Err(e) => Response::Error(e.to_string()),
            };
            let _ = protocol::send(&mut reply, &response);
        }
    }
}
