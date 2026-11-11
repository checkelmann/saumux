#[path = "../src/protocol.rs"]
mod protocol;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use protocol::{Request, Response};
use std::{
    fs,
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Session {
    child: Child,
    root: PathBuf,
    socket: PathBuf,
}
impl Session {
    fn start(program: &[&str]) -> Self {
        let mut args = vec!["--"];
        args.extend_from_slice(program);
        Self::start_server(&args)
    }
    fn start_server(args: &[&str]) -> Self {
        Self::start_configured(args, "")
    }
    fn start_configured(args: &[&str], config: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("config/saumux")).unwrap();
        fs::write(root.join("config/saumux/config.toml"), config).unwrap();
        let socket = root.join("saumux/test.sock");
        let log = fs::File::create(root.join("server.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_saumux"))
            .args(["__server", "test"])
            .args(args)
            .env("XDG_RUNTIME_DIR", &root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("SHELL", "/bin/sh")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut s = Self {
            child,
            root,
            socket,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        // bind creates the path before listen finishes, so existence alone is
        // not a readiness signal (observable under parallel PTY test load).
        while UnixStream::connect(&s.socket).is_err() {
            assert!(
                s.child.try_wait().unwrap().is_none(),
                "{}",
                fs::read_to_string(s.root.join("server.log")).unwrap()
            );
            assert!(Instant::now() < deadline, "server startup timed out");
            thread::sleep(Duration::from_millis(10));
        }
        s
    }
    fn ctl(&self, command: &str, args: &[&str]) -> serde_json::Value {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        protocol::send(
            &mut stream,
            &Request::Control {
                command: command.into(),
                args: args.iter().map(|s| s.to_string()).collect(),
            },
        )
        .unwrap();
        match protocol::receive(&mut stream).unwrap() {
            Response::Result(v) => v,
            other => panic!("{other:?}"),
        }
    }
    fn attach(&self, cols: u16, rows: u16) -> Client {
        let mut writer = UnixStream::connect(&self.socket).unwrap();
        protocol::send(
            &mut writer,
            &Request::Hello {
                version: 1,
                cols,
                rows,
                utf8: true,
                console: false,
            },
        )
        .unwrap();
        let mut reader = writer.try_clone().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(message) = protocol::receive::<Response>(&mut reader) {
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        Client {
            writer,
            rx,
            screen: vt100::Parser::new(rows.try_into().unwrap(), cols.try_into().unwrap(), 0),
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "session child {:?}, log: {}",
                self.child.try_wait(),
                fs::read_to_string(self.root.join("server.log")).unwrap_or_default()
            );
        }
        if self.child.try_wait().ok().flatten().is_none() {
            if let Ok(mut socket) = UnixStream::connect(&self.socket) {
                let _ = protocol::send(
                    &mut socket,
                    &Request::Control {
                        command: "session.end".into(),
                        args: vec!["--confirm".into()],
                    },
                );
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Client {
    writer: UnixStream,
    rx: mpsc::Receiver<Response>,
    screen: vt100::Parser,
}
impl Client {
    fn event(&mut self, event: Event) {
        protocol::send(&mut self.writer, &Request::Input(event)).unwrap();
    }
    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.event(Event::Key(KeyEvent::new(code, modifiers)));
    }
    fn text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }
    fn until(&mut self, needle: &str) {
        self.until_screen(needle, |contents| contents.contains(needle));
    }
    fn until_screen(&mut self, description: &str, ready: impl Fn(&str) -> bool) {
        self.until_render(description, |screen| ready(&screen.contents()));
    }
    fn until_render(&mut self, description: &str, ready: impl Fn(&vt100::Screen) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if ready(self.screen.screen()) {
                return;
            }
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Response::Render(data)) => self.screen.process(data.as_bytes()),
                Ok(Response::Error(e)) => panic!("{e}"),
                _ => {}
            }
            assert!(
                Instant::now() < deadline,
                "waiting for {description:?}, screen:\n{}",
                self.screen.screen().contents()
            );
        }
    }
    fn drain(&mut self) {
        while let Ok(message) = self.rx.recv_timeout(Duration::from_millis(50)) {
            if let Response::Render(data) = message {
                self.screen.process(data.as_bytes());
            }
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }
}

#[test]
fn shell_output_mouse_arrangement_and_reconnect() {
    let s = Session::start(&["/bin/sh"]);
    let mut client = s.attach(100, 30);
    client.until("System");
    client.text("printf '\\033]2;Shell one\\007READY\\n'");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("READY");
    client.until("Shell one");
    let first = s.ctl("list", &[]);
    assert_eq!(first.as_array().unwrap().len(), 1);
    let r = first[0]["rect"].clone();
    let (x, y) = (
        r["x"].as_u64().unwrap() as u16 + 10,
        r["y"].as_u64().unwrap() as u16,
    );
    for (kind, column, row) in [
        (MouseEventKind::Down(MouseButton::Left), x, y),
        (MouseEventKind::Drag(MouseButton::Left), x + 4, y + 2),
        (MouseEventKind::Up(MouseButton::Left), x + 4, y + 2),
    ] {
        client.event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
    client.drain();
    let moved = s.ctl("list", &[]);
    assert_eq!(
        moved[0]["rect"]["x"].as_u64().unwrap(),
        r["x"].as_u64().unwrap() + 4
    );
    s.ctl("window.new", &["--title", "Second", "--", "/bin/sh"]);
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 2);
    s.ctl("layout.tile", &[]);
    let tiled = s.ctl("list", &[]);
    assert_eq!(tiled[0]["rect"]["w"], 50);
    assert_eq!(tiled[1]["rect"]["w"], 50);
    s.ctl("window.shelve", &[]);
    client.until("Second]");
    drop(client);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 2);
    let mut reattached = s.attach(100, 30);
    reattached.until("READY");
    reattached.key(KeyCode::Char('d'), KeyModifiers::ALT);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(
            reattached.rx.recv_timeout(Duration::from_millis(50)),
            Ok(Response::Detached)
        ) {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 2);
}

#[test]
fn menus_keyboard_resize_and_multi_client_minimum() {
    let s = Session::start(&["/bin/sh"]);
    let mut large = s.attach(120, 40);
    large.until("System");
    large.key(KeyCode::F(10), KeyModifiers::NONE);
    large.until("New shell");
    large.key(KeyCode::Enter, KeyModifiers::NONE);
    large.drain();
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 2);
    large.key(KeyCode::F(8), KeyModifiers::ALT);
    large.key(KeyCode::Right, KeyModifiers::NONE);
    large.key(KeyCode::Down, KeyModifiers::NONE);
    large.key(KeyCode::Enter, KeyModifiers::NONE);
    large.drain();
    let before = s.ctl("list", &[]);
    assert!(before[1]["rect"]["w"].as_u64().unwrap() > 0);
    let mut small = s.attach(70, 20);
    small.until("System");
    s.ctl("layout.tile", &[]);
    let tiled = s.ctl("list", &[]);
    assert_eq!(tiled[0]["rect"]["w"], 35);
    drop(small);
    thread::sleep(Duration::from_millis(100));
    s.ctl("layout.tile", &[]);
    assert_eq!(s.ctl("list", &[])[0]["rect"]["w"], 60);
}

#[test]
fn vim_alternate_screen_and_program_exit() {
    if Command::new("vim")
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .is_err()
    {
        return;
    }
    let s = Session::start(&["vim", "-Nu", "NONE", "-n"]);
    let mut client = s.attach(100, 30);
    // The welcome banner depends on Vim's build and the window height. An
    // empty-buffer row is stable; inserted text below verifies actual editing.
    client.until("~");
    client.text("isaumux editor smoke");
    client.key(KeyCode::Esc, KeyModifiers::NONE);
    client.until("saumux editor smoke");
    client.text(":q!");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.socket.exists() {
        assert!(
            Instant::now() < deadline,
            "server did not exit after its last window closed"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn saved_layout_restores_programs_and_geometry() {
    let s = Session::start(&["/bin/sh"]);
    s.ctl("window.new", &["--", "/bin/sh"]);
    s.ctl("layout.tile", &[]);
    let path = s.root.join("layout.json");
    s.ctl("layout.save", &[path.to_str().unwrap()]);
    let layout: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(layout["windows"].as_array().unwrap().len(), 2);
    assert_eq!(layout["windows"][0]["command"][0], "/bin/sh");
    assert_eq!(layout["windows"][0]["rect"]["w"], 40);
    let restored = Session::start_server(&["--layout", path.to_str().unwrap()]);
    let list = restored.ctl("list", &[]);
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["rect"], layout["windows"][0]["rect"]);
    assert_eq!(list[1]["command"], layout["windows"][1]["command"]);
}

#[test]
fn scrollback_selection_internal_paste_and_osc52() {
    let s = Session::start(&[
        "python3",
        "-c",
        r"import os,tty,time; tty.setraw(0); os.write(1,b'COPYME\x1b[?2004h'); data=os.read(0,128); os.write(1,b'\r\nPASTED='+data.hex().encode()); time.sleep(30)",
    ]);
    let mut client = s.attach(100, 30);
    client.until("COPYME");
    let list = s.ctl("list", &[]);
    let r = &list[0]["rect"];
    let x = r["x"].as_u64().unwrap() as u16 + 1;
    let y = r["y"].as_u64().unwrap() as u16 + 1;
    for (kind, column) in [
        (MouseEventKind::Down(MouseButton::Left), x),
        (MouseEventKind::Drag(MouseButton::Left), x + 5),
        (MouseEventKind::Up(MouseButton::Left), x + 5),
    ] {
        client.event(Event::Mouse(MouseEvent {
            kind,
            column,
            row: y,
            modifiers: KeyModifiers::NONE,
        }));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Response::Render(data)) = client.rx.recv_timeout(Duration::from_millis(100)) {
            client.screen.process(data.as_bytes());
            if data.contains("\x1b]52;c;Q09QWU1F\x07") {
                break;
            }
        }
        assert!(Instant::now() < deadline, "missing OSC 52 clipboard export");
    }
    client.key(KeyCode::Insert, KeyModifiers::SHIFT);
    client.until("PASTED=1b5b3230307e434f50594d451b5b3230317e");
}

#[test]
fn application_mouse_receives_local_coordinates() {
    let s = Session::start(&[
        "python3",
        "-c",
        r"import os,tty,time; tty.setraw(0); os.write(1,b'\x1b[?1002h\x1b[?1006hMOUSE_READY'); data=os.read(0,128); os.write(1,b'\r\nRECEIVED='+data.hex().encode()); time.sleep(30)",
    ]);
    let mut client = s.attach(100, 30);
    client.until("MOUSE_READY");
    let list = s.ctl("list", &[]);
    let r = &list[0]["rect"];
    client.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: r["x"].as_u64().unwrap() as u16 + 3,
        row: r["y"].as_u64().unwrap() as u16 + 4,
        modifiers: KeyModifiers::NONE,
    }));
    client.until("RECEIVED=1b5b3c303b333b344d");
}

#[test]
fn window_resize_delivers_sigwinch_and_new_pty_dimensions() {
    let s = Session::start(&[
        "python3",
        "-c",
        "import os,signal,time; signal.signal(signal.SIGWINCH,lambda *_: print('SIZE:%dx%d' % (os.get_terminal_size().columns,os.get_terminal_size().lines),flush=True)); print('RESIZE_READY',flush=True); time.sleep(30)",
    ]);
    let mut client = s.attach(100, 30);
    client.until("RESIZE_READY");
    let list = s.ctl("list", &[]);
    let r = &list[0]["rect"];
    let cols = r["w"].as_u64().unwrap() - 1;
    let rows = r["h"].as_u64().unwrap() - 1;
    client.key(KeyCode::F(8), KeyModifiers::ALT);
    client.key(KeyCode::Right, KeyModifiers::NONE);
    client.key(KeyCode::Down, KeyModifiers::NONE);
    // A resize preview must not notify the child.
    client.drain();
    assert_eq!(s.ctl("list", &[])[0]["rect"], *r);
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until(&format!("SIZE:{cols}x{rows}"));
}

#[test]
fn htop_can_start_and_exit_inside_a_window() {
    if Command::new("htop")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        return;
    }
    let s = Session::start(&["htop"]);
    let mut client = s.attach(100, 30);
    client.until("Tasks:");
    client.key(KeyCode::Char('q'), KeyModifiers::NONE);
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.socket.exists() {
        assert!(Instant::now() < deadline, "htop did not exit");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn configuration_reload_updates_bindings_and_rejects_invalid_values() {
    let s = Session::start(&["/bin/sh"]);
    let mut client = s.attach(100, 30);
    client.until("System");
    let directory = s.root.join("config/saumux");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("config.toml"),
        "[look]\nink = '#112233'\n[keys]\n'F2' = 'menu.open'\n'F10' = ''\n",
    )
    .unwrap();
    s.ctl("config.reload", &[]);
    let commands = s.ctl("commands", &[]);
    let menu = commands
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "menu.open")
        .unwrap();
    assert_eq!(menu["shortcut"], "F2");
    client.key(KeyCode::F(2), KeyModifiers::NONE);
    client.until("New shell");
    client.key(KeyCode::Esc, KeyModifiers::NONE);
    fs::write(
        directory.join("config.toml"),
        "[look]\ncontent = 'rainbow'\n",
    )
    .unwrap();
    let mut stream = UnixStream::connect(&s.socket).unwrap();
    protocol::send(
        &mut stream,
        &Request::Control {
            command: "config.reload".into(),
            args: vec![],
        },
    )
    .unwrap();
    assert!(matches!(
        protocol::receive(&mut stream).unwrap(),
        Response::Error(_)
    ));
    let commands = s.ctl("commands", &[]);
    assert_eq!(
        commands
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "menu.open")
            .unwrap()["shortcut"],
        "F2"
    );
}

#[test]
fn compact_desktop_and_tiny_hosts_do_not_lose_programs() {
    let s = Session::start(&["/bin/sh"]);
    let mut client = s.attach(100, 30);
    client.until("System");
    s.ctl("window.new", &["--", "/bin/sh"]);
    let original = s.ctl("list", &[]);
    client.event(Event::Resize(40, 12));
    client.drain();
    let compact = s.ctl("list", &[]);
    assert_eq!(compact[0]["rect"]["w"], 40);
    assert_eq!(compact[1]["rect"]["w"], 40);
    client.event(Event::Resize(1, 1));
    client.drain();
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 2);
    client.event(Event::Resize(100, 30));
    client.drain();
    let restored = s.ctl("list", &[]);
    assert_eq!(restored[0]["rect"], original[0]["rect"]);
    assert_eq!(restored[1]["rect"], original[1]["rect"]);
}

#[test]
fn scrollback_keeps_lines_that_leave_the_live_screen() {
    let s = Session::start(&[
        "/bin/sh",
        "-c",
        "i=0; while [ $i -lt 80 ]; do echo HISTORY-$i; i=$((i+1)); done; sleep 30",
    ]);
    let mut client = s.attach(100, 30);
    client.until("HISTORY-79");
    client.key(KeyCode::PageUp, KeyModifiers::SHIFT);
    client.until_screen("scrollback frame", |contents| {
        !contents.contains("HISTORY-79") && contents.contains("HISTORY-")
    });
    client.key(KeyCode::PageDown, KeyModifiers::SHIFT);
    client.until("HISTORY-79");
}

#[test]
fn control_cli_preserves_program_options_after_the_separator() {
    let s = Session::start(&["/bin/sh"]);
    let output = Command::new(env!("CARGO_BIN_EXE_saumux"))
        .args([
            "ctl",
            "--session",
            "test",
            "window.new",
            "--",
            "/bin/sh",
            "-c",
            "sleep 30",
            "--json",
        ])
        .env("XDG_RUNTIME_DIR", &s.root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let list = s.ctl("list", &[]);
    assert_eq!(list[1]["command"][3], "--json");
}

#[test]
fn bundled_editor_saves_and_protects_unsaved_changes_on_window_close() {
    let s = Session::start(&["/bin/sh"]);
    let path = s.root.join("notes.txt");
    fs::write(&path, "original\n").unwrap();
    let mut client = s.attach(100, 30);
    client.until("System");
    s.ctl("editor.open", &[path.to_str().unwrap()]);
    client.until("original");
    client.until("Ctrl+S Save");
    client.key(KeyCode::End, KeyModifiers::NONE);
    client.text(" updated");
    client.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    client.until("Saved");
    assert_eq!(fs::read_to_string(&path).unwrap(), "original updated\n");
    client.text("!");
    client.key(KeyCode::F(4), KeyModifiers::ALT);
    client.until("Save changes?");
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 2);
    client.key(KeyCode::Esc, KeyModifiers::NONE);
    client.drain();
    assert_eq!(fs::read_to_string(&path).unwrap(), "original updated\n");
    client.key(KeyCode::F(4), KeyModifiers::ALT);
    client.until("Save changes?");
    client.key(KeyCode::Char('s'), KeyModifiers::NONE);
    let deadline = Instant::now() + Duration::from_secs(3);
    while s.ctl("list", &[]).as_array().unwrap().len() != 1 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), "original updated!\n");
}

#[test]
fn bundled_editor_new_document_save_as_and_reopen() {
    let s = Session::start(&["/bin/sh"]);
    let path = s.root.join("new.txt");
    let mut client = s.attach(100, 30);
    client.until("System");
    s.ctl("editor.new", &[]);
    client.until("Untitled");
    client.until("Ctrl+S Save");
    client.event(Event::Paste("Grüße\n界\n".into()));
    client.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    client.until("Save as (Enter / Esc)");
    client.event(Event::Paste(path.to_str().unwrap().into()));
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("Saved");
    assert_eq!(fs::read_to_string(&path).unwrap(), "Grüße\n界\n");
    client.key(KeyCode::F(4), KeyModifiers::ALT);
    client.drain();
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 1);
    s.ctl("editor.open", &[path.to_str().unwrap()]);
    client.until("Grüße");
    client.until("界");
}

#[test]
fn bundled_browser_mouse_opens_a_file_in_a_separate_editor_window() {
    let s = Session::start(&["/bin/sh"]);
    let root = s.root.join("browse");
    fs::create_dir_all(root.join("folder")).unwrap();
    fs::write(root.join("browse-note.txt"), "browser content").unwrap();
    let mut client = s.attach(100, 30);
    client.until("System");
    s.ctl("browser.open", &[root.to_str().unwrap()]);
    client.until("browse-note.txt");
    let list = s.ctl("list", &[]);
    let r = &list[1]["rect"];
    let column = r["x"].as_u64().unwrap() as u16 + 5;
    let row = r["y"].as_u64().unwrap() as u16 + 5;
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        client.event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
    client.until("browser content");
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 3);
}

#[test]
fn live_themes_repaint_existing_components_and_all_clients_without_restarting() {
    use vt100::Color;
    let s = Session::start(&["/bin/sh"]);
    let mut client = s.attach(100, 30);
    client.until("System");
    s.ctl("editor.new", &[]);
    client.until("Ctrl+S Save");
    client.text("unsaved theme text");
    client.until("unsaved theme text");
    let editor = s.ctl("list", &[])[1].clone();
    let ex = editor["rect"]["x"].as_u64().unwrap() as u16 + 1;
    let ey = editor["rect"]["y"].as_u64().unwrap() as u16 + 2;
    let mut second = s.attach(100, 30);
    second.until("unsaved theme text");
    let cell_is = |screen: &vt100::Screen, x, y, fg, bg| {
        screen
            .cell(y, x)
            .is_some_and(|c| c.fgcolor() == Color::Idx(fg) && c.bgcolor() == Color::Idx(bg))
    };
    s.ctl("theme.set", &["amber"]);
    client.until_render("amber desktop and editor", |screen| {
        cell_is(screen, 0, 0, 0, 3) && cell_is(screen, ex, ey, 14, 0)
    });
    second.until_render("second client's amber editor", |screen| {
        cell_is(screen, ex, ey, 14, 0)
    });
    assert!(
        client
            .screen
            .screen()
            .contents()
            .contains("unsaved theme text")
    );
    s.ctl("browser.open", &[s.root.to_str().unwrap()]);
    client.until("entries");
    let windows = s.ctl("list", &[]);
    let browser = &windows[2];
    let bx = browser["rect"]["x"].as_u64().unwrap() as u16 + 1;
    let by = browser["rect"]["y"].as_u64().unwrap() as u16 + 2;
    s.ctl("theme.set", &["phosphor"]);
    client.until_render("green browser and background editor", |screen| {
        cell_is(screen, bx, by, 10, 0) && cell_is(screen, ex, ey, 10, 0)
    });
    let after = s.ctl("list", &[]);
    assert_eq!(after[1]["pid"], editor["pid"]);
    assert_eq!(after[2]["pid"], browser["pid"]);
    // Changing the theme preserves the editor's dirty buffer and close prompt.
    s.ctl("window.focus", &[editor["id"].to_string().as_str()]);
    client.until("unsaved theme text");
    client.key(KeyCode::F(4), KeyModifiers::ALT);
    client.until("Save changes?");
}

#[test]
fn theme_picker_keyboard_mouse_and_configuration_reload() {
    let s = Session::start(&["/bin/sh"]);
    let mut client = s.attach(100, 30);
    client.until("System");
    client.key(KeyCode::Char('t'), KeyModifiers::ALT);
    client.until("Colour themes");
    client.key(KeyCode::Esc, KeyModifiers::NONE);
    client.drain();
    let current = |list: &serde_json::Value| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["active"] == true)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(current(&s.ctl("theme.list", &[])), "borland");
    client.key(KeyCode::Char('t'), KeyModifiers::ALT);
    client.until("Colour themes");
    client.key(KeyCode::Down, KeyModifiers::NONE);
    client.key(KeyCode::Down, KeyModifiers::NONE);
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until_render("amber menu bar", |screen| {
        screen.cell(0, 0).unwrap().bgcolor() == vt100::Color::Idx(3)
    });
    assert_eq!(current(&s.ctl("theme.list", &[])), "amber");
    client.key(KeyCode::Char('t'), KeyModifiers::ALT);
    client.until("Colour themes");
    // At 100x30 the 62x9 picker starts at (19,10); Paper is row six.
    client.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 22,
        row: 16,
        modifiers: KeyModifiers::NONE,
    }));
    client.until_render("paper desktop", |screen| {
        screen.cell(1, 0).unwrap().bgcolor() == vt100::Color::Idx(7)
    });
    assert_eq!(current(&s.ctl("theme.list", &[])), "paper");
    s.ctl("theme.next", &[]);
    assert_eq!(current(&s.ctl("theme.list", &[])), "borland");
    let directory = s.root.join("config/saumux");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("config.toml"),
        "[look]\ntheme = 'midnight'\n",
    )
    .unwrap();
    s.ctl("config.reload", &[]);
    client.until_render("midnight bar after reload", |screen| {
        screen.cell(0, 0).unwrap().bgcolor() == vt100::Color::Idx(8)
    });
    assert_eq!(current(&s.ctl("theme.list", &[])), "midnight");
    let mut socket = UnixStream::connect(&s.socket).unwrap();
    protocol::send(
        &mut socket,
        &Request::Control {
            command: "theme.set".into(),
            args: vec!["not-a-theme".into()],
        },
    )
    .unwrap();
    assert!(matches!(
        protocol::receive(&mut socket).unwrap(),
        Response::Error(_)
    ));
    assert_eq!(current(&s.ctl("theme.list", &[])), "midnight");
    drop(client);
    let mut reattached = s.attach(100, 30);
    reattached.until_render("theme survives detach", |screen| {
        screen.cell(0, 0).unwrap().bgcolor() == vt100::Color::Idx(8)
    });
}

fn fake_agent_config(script: &str) -> String {
    format!(
        "[notifications]\nprompt_delay_ms = 100\n[[agents]]\nid = 'test'\nname = 'Test agent'\ncommand = ['python3', '-u', '-c', {}]\n",
        serde_json::to_string(script).unwrap()
    )
}

#[test]
fn project_agent_launch_attention_hooks_and_acknowledgement() {
    let script = "import os,tty,time; tty.setraw(0); open('agent-started','w').write(os.getcwd()); print('AGENT_READY '+os.getcwd(),flush=True); os.read(0,1); print('INPUT_RECEIVED',flush=True); time.sleep(60)";
    let s = Session::start_configured(&["--", "/bin/sh"], &fake_agent_config(script));
    // Discovery inspects executables without starting the agent.
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 1);
    assert!(
        s.ctl("agent.list", &[])
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == "test")
    );
    let project_dir = s.root.join("project with spaces; literal");
    fs::create_dir_all(&project_dir).unwrap();
    fs::write(project_dir.join("keep.txt"), "keep me").unwrap();
    let path = project_dir.to_str().unwrap();
    let project = s.ctl("project.add", &[path, "--name", "Example"]);
    let project_id = project["id"].to_string();
    assert_eq!(s.ctl("project.add", &[path])["id"], project["id"]);
    assert_eq!(s.ctl("project.list", &[]).as_array().unwrap().len(), 1);
    let mut client = s.attach(120, 36);
    client.until("Projects");
    // Launch from the saved-project picker, using only keyboard navigation.
    client.key(KeyCode::Char('p'), KeyModifiers::ALT);
    client.until("Saved projects");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("Project: Example");
    client.until("Start Test agent");
    // Test agent is alphabetically last among installed built-in agents.
    let agents = s.ctl("agent.list", &[]);
    let index = agents
        .as_array()
        .unwrap()
        .iter()
        .position(|a| a["id"] == "test")
        .unwrap();
    for _ in 0..index {
        client.key(KeyCode::Down, KeyModifiers::NONE);
    }
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("AGENT_READY");
    assert_eq!(
        fs::read_to_string(project_dir.join("agent-started")).unwrap(),
        fs::canonicalize(&project_dir).unwrap().to_str().unwrap()
    );
    let list = s.ctl("list", &[]);
    let agent = list
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["agent"]["id"] == "test")
        .unwrap();
    let id = agent["id"].to_string();
    assert_eq!(agent["agent"]["project"], "Example");
    assert_eq!(
        fs::canonicalize(project_dir.clone())
            .unwrap()
            .to_string_lossy(),
        project["path"].as_str().unwrap()
    );
    // A hook invoked inside a pane targets its own window, even in the background.
    s.ctl("window.workspace", &[]);
    let output = Command::new(env!("CARGO_BIN_EXE_saumux"))
        .args(["ctl", "agent.notify", "waiting", "Permission needed"])
        .env("XDG_RUNTIME_DIR", &s.root)
        .env("SAUMUX", "test")
        .env("SAUMUX_WINDOW", &id)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    client.until("!1 agents Alt+a");
    client.key(KeyCode::Char('a'), KeyModifiers::ALT);
    client.until("Agent attention");
    client.until("Permission needed");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("AGENT_READY");
    assert_eq!(
        s.ctl("list", &[])
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"].as_u64() == id.parse().ok())
            .unwrap()["agent"]["needs_attention"],
        true
    );
    client.text("y");
    client.until("INPUT_RECEIVED");
    let list = s.ctl("list", &[]);
    assert_eq!(
        list.as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"].as_u64() == id.parse().ok())
            .unwrap()["agent"]["needs_attention"],
        false
    );
    s.ctl("agent.notify", &["waiting", "--window", &id]);
    s.ctl("window.shelve", &[]);
    client.until("!1 agents Alt+a");
    client.key(KeyCode::Char('a'), KeyModifiers::ALT);
    client.until("Agent attention");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("INPUT_RECEIVED");
    assert_eq!(
        s.ctl("list", &[])
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"].as_u64() == id.parse().ok())
            .unwrap()["shelved"],
        false
    );
    s.ctl("agent.ack", &[&id]);
    s.ctl("project.remove", &[&project_id]);
    assert!(project_dir.join("keep.txt").is_file());
    assert!(s.ctl("project.list", &[]).as_array().unwrap().is_empty());
}

#[test]
fn agent_prompt_detection_and_osc_notifications_are_visual_only() {
    let script = "import os,tty,time; tty.setraw(0); print('Would you like to continue? [y/n]',flush=True); os.read(0,1); os.write(1,b'\\x1b[2J\\x1b[HWorking\\r\\n'); time.sleep(0.3); os.write(1,b'\\x1b]9;Review changes please\\x07'); time.sleep(60)";
    let s = Session::start_configured(&["--", "/bin/sh"], &fake_agent_config(script));
    let mut client = s.attach(110, 32);
    client.until("Projects");
    let id = s.ctl("agent.start", &["test"])["id"].to_string();
    client.until("!1 agents Alt+a");
    let list = s.ctl("list", &[]);
    let agent = list
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"].as_u64() == id.parse().ok())
        .unwrap();
    assert_eq!(agent["agent"]["notice"]["possible"], true);
    client.key(KeyCode::Char('a'), KeyModifiers::ALT);
    client.until("May need input:");
    client.key(KeyCode::Esc, KeyModifiers::NONE);
    client.text("y");
    client.until("Working");
    // OSC 9 ends with BEL, but neither sequence nor a standalone bell reaches the host.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Response::Render(data)) = client.rx.recv_timeout(Duration::from_millis(100)) {
            assert!(
                !data.contains('\x07'),
                "agent bell escaped into the host terminal"
            );
            client.screen.process(data.as_bytes());
        }
        let list = s.ctl("list", &[]);
        let notice = &list
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"].as_u64() == id.parse().ok())
            .unwrap()["agent"]["notice"];
        if notice["message"] == "Review changes please" {
            assert_eq!(notice["possible"], false);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "OSC notification was not detected"
        );
    }
    s.ctl("agent.notify", &["working", "--window", &id]);
    assert_eq!(
        s.ctl("list", &[])
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"].as_u64() == id.parse().ok())
            .unwrap()["agent"]["needs_attention"],
        false
    );
}

#[test]
fn project_directory_dialog_errors_and_mouse_removal_preserve_files() {
    let s = Session::start(&["/bin/sh"]);
    let directory = s.root.join("saved through UI");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("source.txt"), "untouched").unwrap();
    let mut client = s.attach(100, 30);
    client.until("Projects");
    s.ctl("project.add", &[]);
    client.until("Save project directory");
    client.key(KeyCode::Char('u'), KeyModifiers::CONTROL);
    client.event(Event::Paste(
        s.root.join("missing").to_string_lossy().into_owned(),
    ));
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("Unable to complete action");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    s.ctl("project.add", &[]);
    client.until("Save project directory");
    client.key(KeyCode::Char('u'), KeyModifiers::CONTROL);
    client.event(Event::Paste(directory.to_string_lossy().into_owned()));
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("Saved project saved through UI");
    client.key(KeyCode::Char('p'), KeyModifiers::ALT);
    client.until("Saved projects");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("Project: saved through UI");
    client.key(KeyCode::End, KeyModifiers::NONE);
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("Remove saved project?");
    client.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 14,
        row: 22,
        modifiers: KeyModifiers::NONE,
    }));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !s.ctl("project.list", &[]).as_array().unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "project removal did not complete"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::read_to_string(directory.join("source.txt")).unwrap(),
        "untouched"
    );
}

struct WebListener {
    child: Child,
    address: String,
}
impl WebListener {
    fn start(s: &Session) -> Self {
        use std::io::{BufRead, BufReader};
        let mut child = Command::new(env!("CARGO_BIN_EXE_saumux"))
            .args([
                "web",
                "--session",
                "test",
                "--bind",
                "127.0.0.1",
                "--port",
                "0",
            ])
            .env("XDG_RUNTIME_DIR", &s.root)
            .env("XDG_CONFIG_HOME", s.root.join("config"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if line.starts_with("saumux web: ") {
                    let _ = tx.send(line);
                }
            }
        });
        let line = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let address = line
            .split_whitespace()
            .nth(2)
            .unwrap()
            .trim_end_matches('/')
            .strip_prefix("http://")
            .unwrap()
            .to_string();
        Self { child, address }
    }
    fn connect(
        &self,
    ) -> tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>> {
        let (mut socket, _) = tungstenite::connect(format!("ws://{}/ws", self.address)).unwrap();
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = socket.get_mut() {
            tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        }
        socket
    }
}
impl Drop for WebListener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn browser_send(socket: &mut impl std::io::Write, path: &str, address: &str) {
    write!(
        socket,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
}
fn web_send(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    value: serde_json::Value,
) {
    socket
        .send(tungstenite::Message::Text(value.to_string().into()))
        .unwrap();
}
fn web_until(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    parser: &mut vt100::Parser,
    needle: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let tungstenite::Message::Text(text) = socket.read().unwrap() {
            match serde_json::from_str::<Response>(&text).unwrap() {
                Response::Render(data) => parser.process(data.as_bytes()),
                Response::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
        if parser.screen().contents().contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "web waiting for {needle}: {}",
            parser.screen().contents()
        );
    }
}

#[test]
fn web_client_shares_desktop_input_mouse_themes_and_survives_disconnect() {
    use serde_json::json;
    use std::io::Read;
    let s = Session::start(&["/bin/sh"]);
    let web = WebListener::start(&s);
    let pid = s.ctl("list", &[])[0]["pid"].clone();
    // All frontend assets are local, served without credentials.
    for path in [
        "/",
        "/app.js",
        "/vendor/xterm.js",
        "/vendor/addon-fit.js",
        "/vendor/xterm.css",
    ] {
        let mut tcp = std::net::TcpStream::connect(&web.address).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        browser_send(&mut tcp, path, &web.address);
        let mut response = String::new();
        tcp.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(!response.contains("Set-Cookie:"));
    }
    let mut browser = web.connect();
    web_send(&mut browser, json!({"type":"hello","cols":100,"rows":30}));
    let mut screen = vt100::Parser::new(30.try_into().unwrap(), 100.try_into().unwrap(), 0);
    web_until(&mut browser, &mut screen, "Projects");
    web_send(
        &mut browser,
        json!({"type":"paste","text":"printf 'WEB_READY\\n'\n"}),
    );
    web_until(&mut browser, &mut screen, "WEB_READY");
    let before = s.ctl("list", &[])[0]["rect"].clone();
    let x = before["x"].as_u64().unwrap() + 10;
    let y = before["y"].as_u64().unwrap();
    for (kind, x, y) in [("down", x, y), ("drag", x + 3, y + 2), ("up", x + 3, y + 2)] {
        web_send(
            &mut browser,
            json!({"type":"mouse","kind":kind,"button":0,"x":x,"y":y}),
        );
    }
    web_send(&mut browser, json!({"type":"key","key":"t","alt":true}));
    web_until(&mut browser, &mut screen, "Colour themes");
    assert_eq!(
        s.ctl("list", &[])[0]["rect"]["x"],
        before["x"].as_u64().unwrap() + 3
    );
    web_send(&mut browser, json!({"type":"key","key":"Escape"}));
    s.ctl("theme.set", &["amber"]);
    let mut tui = s.attach(100, 30);
    tui.until_render("shared amber", |s| {
        s.cell(0, 0).unwrap().bgcolor() == vt100::Color::Idx(3)
    });
    web_send(&mut browser, json!({"type":"detach"}));
    drop(browser);
    let mut second = web.connect();
    web_send(&mut second, json!({"type":"hello","cols":100,"rows":30}));
    web_until(&mut second, &mut screen, "WEB_READY");
    assert_eq!(s.ctl("list", &[])[0]["pid"], pid);
    drop(second);
    drop(web);
    assert_eq!(s.ctl("list", &[])[0]["pid"], pid);
}

#[test]
fn web_rejects_cross_origin_and_malformed_events_without_losing_session() {
    use serde_json::json;
    use tungstenite::client::IntoClientRequest;
    let s = Session::start(&["/bin/sh"]);
    let web = WebListener::start(&s);
    let mut request = format!("ws://{}/ws", web.address)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", "http://unrelated.example".parse().unwrap());
    assert!(
        matches!(tungstenite::connect(request),Err(tungstenite::Error::Http(response)) if response.status()==403)
    );
    let mut browser = web.connect();
    web_send(&mut browser, json!({"type":"hello","cols":100,"rows":30}));
    web_send(
        &mut browser,
        json!({"type":"control","command":"session.end"}),
    );
    loop {
        if let tungstenite::Message::Text(text) = browser.read().unwrap()
            && matches!(
                serde_json::from_str::<Response>(&text).unwrap(),
                Response::Error(_)
            )
        {
            break;
        }
    }
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 1);
}

#[test]
fn modified_enter_reaches_program_distinctly_from_submit_in_tui_and_web() {
    use serde_json::json;
    // Byte recorder, not a real agent: prove a newline gesture never arrives as CR.
    let script = "import os,tty; tty.setraw(0); print('KEY_RECORDER_READY',flush=True)\nwhile True:\n b=os.read(0,1); print('\\rBYTE_'+b.hex(),flush=True)";
    let s = Session::start(&["python3", "-u", "-c", script]);
    let mut client = s.attach(100, 30);
    client.until("KEY_RECORDER_READY");
    client.key(KeyCode::Enter, KeyModifiers::SHIFT);
    client.until("BYTE_75"); // final 'u' in CSI 13;2u
    assert!(!client.screen.screen().contents().contains("BYTE_0d"));
    client.key(KeyCode::Char('j'), KeyModifiers::CONTROL);
    client.until("BYTE_0a");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("BYTE_0d");
    // Use a fresh recorder so no earlier CR can conceal a web regression.
    s.ctl("window.new", &["--", "python3", "-u", "-c", script]);
    let web = WebListener::start(&s);
    let mut browser = web.connect();
    web_send(&mut browser, json!({"type":"hello","cols":100,"rows":30}));
    let mut screen = vt100::Parser::new(30.try_into().unwrap(), 100.try_into().unwrap(), 0);
    web_until(&mut browser, &mut screen, "KEY_RECORDER_READY");
    web_send(
        &mut browser,
        json!({"type":"key","key":"Enter","shift":true}),
    );
    web_until(&mut browser, &mut screen, "BYTE_75");
    assert!(!screen.screen().contents().contains("BYTE_0d"));
    web_send(&mut browser, json!({"type":"key","key":"Enter"}));
    web_until(&mut browser, &mut screen, "BYTE_0d");
}

#[test]
fn notes_persist_protect_unsaved_changes_and_reuse_the_open_scratchpad() {
    use std::os::unix::fs::PermissionsExt;
    let s = Session::start(&["/bin/sh"]);
    let mut client = s.attach(110, 32);
    client.until("Apps");
    let id = s.ctl("notes.open", &[])["id"].as_u64().unwrap();
    client.until("NOTES · 1");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("Ctrl+S Save");
    let editor_id = s.ctl("list", &[])[2]["id"].as_u64().unwrap();
    client.event(Event::Paste("Remember the release\nGrüße 界".into()));
    client.until("Remember the release");
    // Choosing Notes again restores the notebook and preserves the open editor.
    s.ctl("window.workspace", &[]);
    assert_eq!(s.ctl("notes.open", &[])["id"], id);
    s.ctl("window.focus", &[&editor_id.to_string()]);
    client.until("Grüße 界");
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 3);
    s.ctl("window.close", &[]);
    client.until("Save changes?");
    client.key(KeyCode::Esc, KeyModifiers::NONE);
    client.until_screen("close cancelled", |text| !text.contains("Save changes?"));
    client.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    client.until("Saved");
    let path = s.root.join("data/saumux/notes.txt");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Remember the release\nGrüße 界"
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    s.ctl("window.focus", &[&editor_id.to_string()]);
    s.ctl("window.close", &[]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.ctl("list", &[]).as_array().unwrap().len() != 2 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    s.ctl("window.focus", &[&id.to_string()]);
    s.ctl("window.close", &[]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.ctl("list", &[]).as_array().unwrap().len() != 1 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let reopened = s.ctl("notes.open", &[])["id"].as_u64().unwrap();
    assert_ne!(id, reopened);
    client.until("NOTES · 1");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    // Wait for the newly opened editor to finish loading the file.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !s
        .ctl("list", &[])
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["title"].as_str().is_some_and(|t| t.ends_with("— Editor")))
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    client.until("Remember the release");
    client.until("Grüße 界");
    // A conflicting edit from another session/editor must not overwrite the file.
    fs::write(&path, "external change").unwrap();
    client.text("!");
    client.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    client.until("file changed on disk");
    assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
}

#[test]
fn apps_menu_monitor_clock_pause_controls_themes_and_small_windows() {
    let s = Session::start(&["/bin/sh"]);
    let mut client = s.attach(110, 32);
    client.until("Apps");
    // Apps follows Projects; preserve the established indexes of other menus.
    client.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 54,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    client.until("System Monitor");
    client.key(KeyCode::Enter, KeyModifiers::NONE);
    client.until("CPU history");
    client.until_screen("a live CPU sample", |text| {
        text.split("sample ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n > 0)
    });
    let windows = s.ctl("list", &[]);
    let monitor = windows
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["command"][1] == "__monitor")
        .unwrap();
    let monitor_id = monitor["id"].to_string();
    let monitor_pid = monitor["pid"].clone();
    client.key(KeyCode::Char(' '), KeyModifiers::NONE);
    client.until("Paused");
    // Refresh/resume is also available through the app's own mouse toolbar.
    let r = monitor["rect"].clone();
    client.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: r["x"].as_u64().unwrap() as u16 + 3,
        row: r["y"].as_u64().unwrap() as u16 + 1,
        modifiers: KeyModifiers::NONE,
    }));
    client.until("Live · 1s");
    let clock_id = s.ctl("clock.open", &[])["id"].to_string();
    let clock_rect = s
        .ctl("list", &[])
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"].as_u64() == clock_id.parse().ok())
        .unwrap()["rect"]
        .clone();
    client.until("Local time (host)");
    client.key(KeyCode::Char('u'), KeyModifiers::NONE);
    client.until("UTC");
    client.key(KeyCode::Char('t'), KeyModifiers::NONE);
    client.until_screen("12-hour clock", |text| {
        text.contains("UTC AM") || text.contains("UTC PM")
    });
    s.ctl("theme.set", &["phosphor"]);
    client.until_render("clock content follows live theme", |screen| {
        screen
            .cell(
                clock_rect["y"].as_u64().unwrap() as u16 + 1,
                clock_rect["x"].as_u64().unwrap() as u16 + 2,
            )
            .unwrap()
            .bgcolor()
            == vt100::Color::Idx(2)
    });
    // Closing a read-only app leaves the shell and other widget running.
    client.key(KeyCode::Char('q'), KeyModifiers::CONTROL);
    let deadline = Instant::now() + Duration::from_secs(5);
    while s
        .ctl("list", &[])
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"].as_u64() == clock_id.parse().ok())
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    s.ctl("window.focus", &[&monitor_id]);
    client.until_render("monitor content follows live theme", |screen| {
        screen
            .cell(
                r["y"].as_u64().unwrap() as u16 + 2,
                r["x"].as_u64().unwrap() as u16 + 2,
            )
            .unwrap()
            .fgcolor()
            == vt100::Color::Idx(10)
    });
    let windows = s.ctl("list", &[]);
    assert_eq!(
        windows
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"].as_u64() == monitor_id.parse().ok())
            .unwrap()["pid"],
        monitor_pid
    );
    client.event(Event::Resize(1, 1));
    client.drain();
    client.event(Event::Resize(110, 32));
    client.until("CPU history");
    assert_eq!(s.ctl("list", &[]).as_array().unwrap().len(), 2);
}
