//! An opt-in, unauthenticated HTTP/WebSocket client for one local session.
//! The desktop is still composed by the ordinary session server.
use crate::{
    protocol::{self, Request, Response},
    session,
};
use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response as HttpResponse},
    routing::get,
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::Semaphore,
    time::timeout,
};

const INPUT_LIMIT: usize = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(15);
const WEB_HELP: &str = "Usage: saumux web [--bind <address>] [--port <port>] [--session <name>] [--layout <file>] [-- program args…]\n\nDefaults: --bind 0.0.0.0 --port 8080. No authentication.\nStarts a session, or shares an existing one. Ctrl+C stops HTTP, leaving programs running.\n";

struct Options {
    address: SocketAddr,
    session: Option<String>,
    layout: Option<PathBuf>,
    program: Vec<String>,
}
impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut bind: IpAddr = Ipv4Addr::UNSPECIFIED.into();
        let mut port = 8080;
        let mut name = None;
        let mut layout = None;
        let mut program = vec![];
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--" {
                program = args[i + 1..].to_vec();
                break;
            }
            let value = args
                .get(i + 1)
                .with_context(|| format!("{} needs a value", args[i]))?;
            match args[i].as_str() {
                "--bind" => {
                    bind = value
                        .parse()
                        .context("--bind needs an IPv4 or IPv6 address")?
                }
                "--port" => {
                    port = value
                        .parse()
                        .context("--port needs a number from 0 to 65535")?
                }
                "--session" => name = Some(value.clone()),
                "--layout" => layout = Some(PathBuf::from(value)),
                option => bail!("unknown web option {option}; try saumux web --help"),
            }
            i += 2;
        }
        Ok(Self {
            address: SocketAddr::new(bind, port),
            session: name,
            layout,
            program,
        })
    }
}
struct WebState {
    socket: PathBuf,
    connections: Arc<Semaphore>,
}

pub fn run(args: &[String]) -> Result<()> {
    if args
        .first()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        print!("{WEB_HELP}");
        return Ok(());
    }
    let options = Options::parse(args)?;
    // Bind before starting a program: a port conflict must not leave a new session.
    let listener = std::net::TcpListener::bind(options.address)
        .with_context(|| format!("listening on {}", options.address))?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let name = match options.session {
        Some(name) => name,
        None => match session::list()?.as_slice() {
            [] => "default".into(),
            [name] => name.clone(),
            names => bail!(
                "several sessions exist: {}; use --session <name>",
                names.join(", ")
            ),
        },
    };
    let socket = session::socket_path(&name)?;
    if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
        if options.layout.is_some() || !options.program.is_empty() {
            bail!("session {name} already exists; omit --layout/program to share it");
        }
    } else {
        drop(session::start(
            &name,
            &options.program,
            options.layout.as_deref(),
        )?);
    }
    println!("saumux web: http://{address}/ · session {name} · no authentication");
    if address.ip().is_unspecified() {
        println!(
            "Open http://<this-computer-address>:{}/ from another PC.",
            address.port()
        );
    }
    println!("Ctrl+C stops the web listener; session programs keep running.");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let state = Arc::new(WebState {
            socket,
            connections: Arc::new(Semaphore::new(64)),
        });
        let app = Router::new()
            .route("/ws", get(upgrade))
            .fallback(get(asset))
            .with_state(state);
        let server = axum::serve(tokio::net::TcpListener::from_std(listener)?, app);
        tokio::select! {
            result = server.into_future() => result.context("serving web client"),
            _ = shutdown() => Ok(()),
        }
    })
}
async fn shutdown() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing SIGTERM handler");
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
}
async fn asset(uri: axum::http::Uri) -> HttpResponse {
    let (content_type, body) = match uri.path() {
        "/" => (
            "text/html; charset=utf-8",
            include_str!("../web/index.html"),
        ),
        "/app.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/app.js"),
        ),
        "/style.css" => ("text/css; charset=utf-8", include_str!("../web/style.css")),
        "/vendor/xterm.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/vendor/xterm.js"),
        ),
        "/vendor/addon-fit.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/vendor/addon-fit.js"),
        ),
        "/vendor/xterm.css" => (
            "text/css; charset=utf-8",
            include_str!("../web/vendor/xterm.css"),
        ),
        "/vendor/xterm.LICENSE" => (
            "text/plain; charset=utf-8",
            include_str!("../web/vendor/xterm.LICENSE"),
        ),
        "/vendor/addon-fit.LICENSE" => (
            "text/plain; charset=utf-8",
            include_str!("../web/vendor/addon-fit.LICENSE"),
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([
        (header::CONTENT_TYPE, content_type),
        (header::CACHE_CONTROL, "no-store"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"),
    ], body).into_response()
}
async fn upgrade(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> HttpResponse {
    // This is not authentication. Prevent unrelated sites from driving a browser
    // connection implicitly; direct network clients still have unrestricted access.
    if let Some(origin) = headers.get(header::ORIGIN) {
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        let origin = origin.to_str().unwrap_or("");
        if origin != format!("http://{host}") && origin != format!("https://{host}") {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    let Ok(permit) = state.connections.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(INPUT_LIMIT)
        .max_frame_size(INPUT_LIMIT)
        .on_upgrade(move |mut browser| async move {
            let _permit = permit;
            if let Err(error) = bridge(&mut browser, &state.socket).await {
                let response =
                    serde_json::to_string(&Response::Error(format!("{error:#}"))).unwrap();
                let _ = timeout(IO_TIMEOUT, browser.send(Message::Text(response.into()))).await;
            }
            let _ = timeout(IO_TIMEOUT, browser.close()).await;
        })
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Hello {
        cols: u16,
        rows: u16,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Key {
        key: String,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        shift: bool,
    },
    Mouse {
        kind: String,
        button: u8,
        x: u16,
        y: u16,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        shift: bool,
    },
    Paste {
        text: String,
    },
    Focus {
        focused: bool,
    },
    Detach,
}
fn modifiers(ctrl: bool, alt: bool, shift: bool) -> KeyModifiers {
    let mut flags = KeyModifiers::NONE;
    if ctrl {
        flags |= KeyModifiers::CONTROL;
    }
    if alt {
        flags |= KeyModifiers::ALT;
    }
    if shift {
        flags |= KeyModifiers::SHIFT;
    }
    flags
}
fn size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(1, 500), rows.clamp(1, 200))
}
impl Input {
    fn request(self) -> Result<Option<Request>> {
        let event = match self {
            Self::Hello { .. } => bail!("hello must be the first message only"),
            Self::Detach => return Ok(None),
            Self::Resize { cols, rows } => {
                let (cols, rows) = size(cols, rows);
                Event::Resize(cols, rows)
            }
            Self::Focus { focused } => {
                if focused {
                    Event::FocusGained
                } else {
                    Event::FocusLost
                }
            }
            Self::Paste { text } => Event::Paste(text),
            Self::Key {
                key,
                ctrl,
                alt,
                shift,
            } => {
                let code = match key.as_str() {
                    "Enter" => KeyCode::Enter,
                    "Escape" => KeyCode::Esc,
                    "Backspace" => KeyCode::Backspace,
                    "Tab" if shift => KeyCode::BackTab,
                    "Tab" => KeyCode::Tab,
                    "ArrowUp" => KeyCode::Up,
                    "ArrowDown" => KeyCode::Down,
                    "ArrowLeft" => KeyCode::Left,
                    "ArrowRight" => KeyCode::Right,
                    "Home" => KeyCode::Home,
                    "End" => KeyCode::End,
                    "PageUp" => KeyCode::PageUp,
                    "PageDown" => KeyCode::PageDown,
                    "Insert" => KeyCode::Insert,
                    "Delete" => KeyCode::Delete,
                    k if k.starts_with('F')
                        && k[1..].parse::<u8>().is_ok_and(|f| (1..=24).contains(&f)) =>
                    {
                        KeyCode::F(k[1..].parse()?)
                    }
                    _ if key.chars().count() == 1 => KeyCode::Char(key.chars().next().unwrap()),
                    _ => bail!("unsupported key"),
                };
                Event::Key(KeyEvent::new(code, modifiers(ctrl, alt, shift)))
            }
            Self::Mouse {
                kind,
                button,
                x,
                y,
                ctrl,
                alt,
                shift,
            } => {
                let button = match button {
                    0 => MouseButton::Left,
                    1 => MouseButton::Middle,
                    2 => MouseButton::Right,
                    _ => bail!("invalid mouse button"),
                };
                let kind = match kind.as_str() {
                    "down" => MouseEventKind::Down(button),
                    "up" => MouseEventKind::Up(button),
                    "drag" => MouseEventKind::Drag(button),
                    "move" => MouseEventKind::Moved,
                    "wheel_up" => MouseEventKind::ScrollUp,
                    "wheel_down" => MouseEventKind::ScrollDown,
                    _ => bail!("invalid mouse event"),
                };
                Event::Mouse(MouseEvent {
                    kind,
                    column: x.min(499),
                    row: y.min(199),
                    modifiers: modifiers(ctrl, alt, shift),
                })
            }
        };
        Ok(Some(Request::Input(event)))
    }
}
async fn send_request(writer: &mut (impl AsyncWriteExt + Unpin), request: &Request) -> Result<()> {
    let bytes = serde_json::to_vec(request)?;
    timeout(IO_TIMEOUT, async {
        writer.write_u32(bytes.len() as u32).await?;
        writer.write_all(&bytes).await
    })
    .await??;
    Ok(())
}
async fn receive_response(reader: &mut (impl AsyncReadExt + Unpin)) -> Result<Response> {
    let length = reader.read_u32().await? as usize;
    if length > protocol::MAX_PACKET {
        bail!("session packet too large");
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
async fn bridge(browser: &mut WebSocket, socket: &std::path::Path) -> Result<()> {
    let hello = timeout(Duration::from_secs(10), browser.recv())
        .await?
        .context("connection closed before hello")??;
    let Message::Text(hello) = hello else {
        bail!("expected hello");
    };
    let Input::Hello { cols, rows } = serde_json::from_str(&hello)? else {
        bail!("expected hello");
    };
    let (cols, rows) = size(cols, rows);
    let mut session = UnixStream::connect(socket)
        .await
        .context("connecting to session; start a new web listener if the session ended")?;
    send_request(
        &mut session,
        &Request::Hello {
            version: 1,
            cols,
            rows,
            utf8: true,
            console: false,
        },
    )
    .await?;
    let (mut reader, mut writer) = session.split();
    let (mut outgoing, mut incoming) = browser.split();
    let started = std::time::Instant::now();
    let last_received = AtomicU64::new(0);
    let to_browser = async {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        loop {
            // Keep the framed read pinned while sending heartbeat pings. Dropping
            // and restarting read_exact here could discard half a packet.
            let response = receive_response(&mut reader);
            tokio::pin!(response);
            let response = loop {
                tokio::select! {
                    result = &mut response => break result?,
                    _ = heartbeat.tick() => {
                        if started.elapsed().as_secs().saturating_sub(last_received.load(Ordering::Relaxed)) > 45 {
                            bail!("browser heartbeat timed out");
                        }
                        timeout(IO_TIMEOUT, outgoing.send(Message::Ping(Vec::new().into()))).await??;
                    }
                }
            };
            let detached = matches!(response, Response::Detached | Response::Error(_));
            let message = Message::Text(serde_json::to_string(&response)?.into());
            timeout(IO_TIMEOUT, outgoing.send(message)).await??;
            if detached {
                return Ok::<(), anyhow::Error>(());
            }
        }
    };
    let to_session = async {
        while let Some(message) = incoming.next().await {
            last_received.store(started.elapsed().as_secs(), Ordering::Relaxed);
            match message? {
                Message::Text(text) => {
                    let input: Input =
                        serde_json::from_str(&text).context("invalid browser input")?;
                    let Some(request) = input.request()? else {
                        return Ok(());
                    };
                    send_request(&mut writer, &request).await?;
                }
                Message::Close(_) => return Ok(()),
                Message::Ping(_) | Message::Pong(_) => {}
                _ => bail!("expected text input"),
            }
        }
        Ok::<(), anyhow::Error>(())
    };
    // Select whole forwarding loops, not a cancel-unsafe partial packet read.
    // Ending either direction drops both halves and detaches only this client.
    tokio::select! { result = to_browser => result, result = to_session => result }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_structured_input_preserve_terminal_semantics() {
        let opts = Options::parse(&[]).unwrap();
        assert_eq!(opts.address, "0.0.0.0:8080".parse().unwrap());
        let input: Input =
            serde_json::from_str(r#"{"type":"key","key":"F10","alt":true}"#).unwrap();
        assert!(matches!(
            input.request().unwrap(),
            Some(Request::Input(Event::Key(KeyEvent {
                code: KeyCode::F(10),
                modifiers: KeyModifiers::ALT,
                ..
            })))
        ));
        let input: Input = serde_json::from_str(
            r#"{"type":"mouse","kind":"drag","button":0,"x":65535,"y":65535}"#,
        )
        .unwrap();
        assert!(matches!(
            input.request().unwrap(),
            Some(Request::Input(Event::Mouse(MouseEvent {
                column: 499,
                row: 199,
                kind: MouseEventKind::Drag(MouseButton::Left),
                ..
            })))
        ));
        assert!(
            serde_json::from_str::<Input>(r#"{"type":"control","command":"session.end"}"#).is_err()
        );
    }
}
