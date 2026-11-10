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
        let root = std::env::temp_dir().join(format!(
            "sx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        let socket = root.join("saumux/test.sock");
        let log = fs::File::create(root.join("server.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_saumux"))
            .args(["__server", "test"])
            .args(args)
            .env("XDG_RUNTIME_DIR", &root)
            .env("XDG_CONFIG_HOME", root.join("config"))
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
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if ready(&self.screen.screen().contents()) {
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
