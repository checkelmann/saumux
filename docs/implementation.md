# Implementation notes

For release state, continuation guidance and the current backlog, see
[the agent handoff](https://github.com/checkelmann/saumux/blob/main/docs/HANDOFF.md).

The target platforms are Linux **and macOS**, following the updated requirement.
Windows is outside scope. gpm applies only to Linux virtual consoles.

## Structure

- `browser.rs`, `editor.rs`, `builtin.rs`: bundled terminal applications with
  shared retro rendering, file navigation, text editing and save dialogs.
- `apps.rs`: live system monitor, local/UTC digital clock and private notes
  storage. Notes delegates to the shared editor engine with a distinct title.
- `window.rs`: one portable-pty per program, vt100 cell buffer, 10,000 lines of
  scrollback, OSC titles, bell tracking, terminal-query replies and SIGWINCH.
- `app.rs`: stacking/focus, geometry, workspaces, menus, outline dragging,
  selection, shelf, layouts, hit testing and composition.
- `commands.rs`: shared command catalog, effective bindings and menu labels.
- `agents.rs`: executable discovery, agent identity, explicit visual notices and
  conservative quiet-screen prompt detection. Discovery never executes a CLI.
- `projects.rs`: shared project bookmarks, canonical paths, bounded/validated
  JSON storage, nonblocking writer lock and mode-0600 atomic saves.
- `support.rs`: Agents/Projects menu entries, launch/attention/project dialogs
  and control commands; returns launch/focus actions to the window manager.
- `render.rs`: cell grid and ANSI differences, Unicode width/overlap handling,
  the Borland palette, optional monochrome content mapping and program colours.
- `session.rs`: detached server, private Unix sockets, peer authentication,
  bounded queues and per-client rendering state.
- `client.rs`: raw mode, alternate screen, terminal events and synchronized
  output; restores terminal state on detach, error, panic and termination signals.
- `web.rs`, `web/`: opt-in HTTP/WebSocket client bridge and embedded xterm.js
  frontend, using the same Unix-socket protocol and compositor as the TUI.
- `gpm.rs`: Linux console mouse adapter using the local gpm wire protocol.
- `protocol.rs`: versioned, length-prefixed JSON packets, bounded at 16 MiB.

PTY readers and input writers run on separate threads. The server serializes
window-manager events and composes at most 60 times a second. It retains the last
cell grid for each client and emits changed cells. During movement/resizing only
the preview outline changes; PTY size changes on release/Enter. Slow clients can
be disconnected when their bounded output queue fills; their programs continue.

The terminal parser is the MIT-licensed `vt100` crate, backed by `vte`.
`portable-pty` is MIT licensed and provides the platform PTYs. `crossterm` is MIT
licensed and handles host terminal input. Dependency sources and licenses are
recorded in Cargo.lock and each upstream package.

## Appearance

The original two-colour requirement was removed. `theme.rs` defines semantic
roles for six 16-colour palettes: Borland, Norton, Amber, Phosphor, Midnight and
Paper. They cover desktop surfaces, menus, focus, buttons, scroll bars and bundled
applications. View → Colour theme (Alt+t), `theme.set` and `theme.next` change the
current session immediately; `look.theme` chooses the default for new sessions.
The compositor sends
standard/bright ANSI sequences for these colours; the terminal palette determines
the exact shades. Program colour output is preserved by default. Default ink and
paper follow the active theme; explicit host/hex overrides take precedence.

The server publishes the selected theme to a mode-0600 `<session>.theme` sidecar
in the private socket directory, using an atomic rename. Bundled applications
inherit `SAUMUX_THEME_FILE` and check it at most once every 100 ms. Their normal
idle redraw loop picks up changes without injected keystrokes, restarting PTYs,
or losing editing state. Theme changes are shared by all clients and survive
client disconnects. Session shutdown removes the sidecar; configuration files are
not rewritten by the picker.

## Decisions and bounds

- Four workspaces, at most 64 windows, host desktop limited to 500×200 cells.
- Minimum content buffer is 2×1 cells; tiny hosts clip the interface safely.
  Below 60×16 only the active window is shown, zoomed; the menu/key bars remain.
- Host shrinking preserves window size, moving its title into reach. The small
  terminal fallback temporarily changes PTY sizes; leaving it restores geometry.
- Tile is a one-off rectangular layout with no desktop gaps. It rejects layouts
  where even a frame would not fit.
- Program colours are preserved per window when enabled. In mono mode, nondefault
  foreground/background colours map to bold/underline, retaining inverse video.
- Selection is linear, bounded to one window. Copy trims trailing spaces on each
  row. Clipboard exports are capped at 75,000 UTF-8 bytes for OSC 52 limits.
- Configuration adds program launchers to the fixed menu groups. Reload changes
  appearance and bindings immediately; `look.content` is a default for new windows.
- Working directories come from `/proc/<pid>/cwd` on Linux and `proc_pidinfo` on
  macOS, falling back to the initial directory when unavailable.
- The original terminal-program-only scope was expanded to include the bundled
  browser and editor. They run as PTY programs from the same executable. Editor
  close requests go through its unsaved-changes prompt; confirmed session shutdown
  still stops every program. No plugins or graphics protocols. The later web
  frontend explicitly adds an opt-in network listener.

## Additional bundled apps

The shared catalog exposes `monitor.open`, `notes.open` and `clock.open` in Apps.
Their internal PTY entry points are `__monitor`, `__notes` and `__clock`. Existing
Agents/Projects menu indexes stay unchanged; Apps precedes Help. Browser clients
receive the same desktop menu and app output automatically. Monitor/clock open in
compact default windows, with the ordinary zoom/resize/shelf/layout controls.

System Monitor uses the MIT-licensed sysinfo 0.37 library with only its `system`
feature. It refreshes memory and CPU counters about once per second, waiting for
the minimum CPU interval before reporting usage. It does not spawn top/htop,
collect process command lines or manage processes. A capped 60-sample history,
per-core meters, OS/hostname, swap, load averages and uptime are displayed. Pause
stops automatic samples; Refresh still takes an eligible manual sample. Resume
resets the CPU baseline. Totals reflect the OS view rather than a promised
container/cgroup quota view. Only rows that fit are shown; there is no process list.

Clock uses reentrant libc localtime/gmtime functions (with an inferred timestamp
type for musl compatibility). It draws large digit glyphs when space permits and
compact text otherwise. Local/UTC and 12/24-hour choices are runtime-only. It has
no alarms, timers or background sounds. All app surfaces check live theme roles
through the existing sidecar/cache, even when the monitor is paused.

Notes prepares `$XDG_DATA_HOME/saumux/notes.txt` or `~/.local/share/saumux/notes.txt`
with mode 0600, creating its data directory with 0700. Relative/empty XDG data
paths fall back to HOME. Notes uses `editor::run_named` and therefore retains the
same UTF-8, 2 MiB limit, undo/search, atomic writes, permission preservation,
external-change checks and save/discard/cancel behavior. It does not autosave.
A second launch in the same session focuses/restores the existing Notes window.
Other sessions open the shared file independently; concurrent saves get the
normal external-change protection. Save As switches the open document, while a
fresh Notes window still defaults to the scratchpad path.

`Window::is_builtin` validates both executable identity and the internal entry
point. `is_editor` covers both `__edit` and `__notes`; keep this contract so desktop
close sends Ctrl+Q to protect dirty notes. Built-in colour roles are preserved
even if the default program-output setting is mono. Layouts record their PTY
commands/working directories, not unsaved text or runtime widget options.

Tests isolate XDG_DATA_HOME as well as runtime/config paths. They cover real
monitor sampling, menu launch, mouse pause/resume, clock zone/format and theme
changes, tiny-host resize, Notes persistence/private permissions/reopen/focus,
unsaved close and concurrent-change rejection. Real Chromium also exercises the
apps and verifies the paused monitor's content changes palette without restarting.

## Modified Enter keys

The shared input encoder keeps plain Enter as CR and legacy Alt+Enter as ESC CR,
but encodes Shift/Control-modified Enter using CSI-u (`CSI 13;<modifier>u`). This
prevents Shift+Enter from silently submitting Claude prompts in both browser and
TUI clients. Ctrl+J remains LF. This does not implement or advertise the full
kitty keyboard protocol to programs; it preserves modified Enter input only.

The TUI pushes the host's DISAMBIGUATE_ESCAPE_CODES keyboard enhancement flag and
pops it during restoration. A process-wide atomic guard prevents the panic hook
and terminal guard from popping the host's keyboard stack twice. Unsupported
hosts may still send Shift+Enter identically to Enter; the server cannot recover
that distinction. Claude's Ctrl+J newline is the portable fallback. Browser DOM
input supplies Shift directly. No Claude settings or host terminal config files
are edited. Tests record exact PTY bytes rather than launching a real provider.

## Agents, projects and attention

Each agent remains a normal PTY window with optional `agents::Run` metadata.
Launch actions use literal argument vectors and an explicit CWD. Built-in IDs are
codex, claude, gemini, aider, opencode, goose, amp and cursor. Configured agents
replace built-in IDs. Rescanning checks server PATH executable permissions, not
login state, version output or provider availability. Attached clients cannot
replace a running server's inherited PATH; restart a new session to change it.

Projects are shared between sessions in the configuration directory. Mutations
lock and reload the latest file, merge the change, then atomically rename a
mode-0600 temporary file; the desktop never blocks waiting for a writer lock.
Display lists refresh explicitly or when opening the project chooser. Bookmarks
are capped at 128, agent/windows at the existing 64-window bound. Removing a
bookmark never changes files or terminates windows. Missing directories remain
bookmarked and launching reports an error rather than silently choosing a CWD.

The server polls tagged agent windows for prompts after a configurable quiet
period. It examines the last ten nonempty lines of the live viewport, even if the
user is reading scrollback. Strong English prompt phrases produce a notice marked
`possible`; ordinary inactivity does not. This is deliberately best effort,
not a provider state protocol. Acknowledgement suppresses repeated identical
prompt text until the prompt disappears. Disabling detection clears guessed
notices and keeps explicit ones.

OSC 9 text / OSC 777 notify and BEL parser callbacks set explicit visual notices.
Numeric OSC 9 messages are ignored as progress. No host bell or notification OSC
is emitted. `agent.notify` supports waiting/attention/done/working and requires a
window ID, supplied by the CLI from `SAUMUX_WINDOW` for hooks. `SAUMUX_BIN` names
the launching executable; `SAUMUX` chooses the session. These variables are
inherited by child agent processes. No provider configuration is written.

Notices survive focus, shelving, workspace changes and client disconnects. Actual
forwarded key/paste/application-button input, explicit acknowledgement or a
working notification clears them. Titles, shelf tabs and persistent badges use
the shared theme's activity role; attention navigation crosses workspace boundaries.
Layouts retain agent identity/project labels but create fresh transient state.

## Browser frontend

`saumux web` binds to 0.0.0.0:8080 by default, with configurable IP/port and session.
The user explicitly requested no authentication for this iteration. The listener
shares one existing session or starts one, independent of any terminal. It binds
before launching programs so an occupied port does not create an unwanted session.
Network access is now an accepted scope expansion; SSH is still supported.

Axum/Tokio serve embedded static assets and upgrade `/ws`. A browser sends one
size hello, then bounded structured keyboard/mouse/paste/resize/focus events.
The bridge converts them to crossterm events and sends the existing length-prefixed
Unix-socket protocol. It accepts no arbitrary control packet or alternate socket
path from the browser. The session server still owns all windows, menus, PTYs,
attention and differential composition. Output goes unchanged through xterm.js;
no second desktop implementation exists. All assets are bundled for offline use.

There are at most 64 web connections, a 10-second hello timeout, a 1 MiB browser
message limit, 15-second write timeouts and bounded browser input/render queues.
WebSocket pings run every 15 seconds; missing replies for over 45 seconds detach
the abandoned client. Packet reads remain pinned across heartbeat sends.
Protocol packets retain the existing 16 MiB limit. Forwarding selects entire
input/output loops, avoiding cancellation of a partially read framed packet.
Closing either direction closes the local connection, detaching only that client.
Ctrl+C/SIGTERM stops the HTTP process, leaving the detached server/programs alive.

The server rejects cross-origin browser upgrades while allowing direct clients
without an Origin header; this is not authentication. No credentials, login,
TLS, provider approval changes or automatic firewall changes are implemented.
Anyone able to reach the listener can operate the selected session as its owner.
A reverse proxy can supply HTTPS and must preserve the Host/Origin relationship
and forward WebSocket upgrades for `/ws`; serve at the origin root.

The xterm.js 6.0.0 distribution and Fit addon 0.11.0 are vendored with MIT notices.
No npm step is required for Rust builds or release use. Playwright is a test-only
pinned dependency under `tests/browser`. Release archives include third-party
notices. Browser clipboard reads always require local user action; OSC 52 sets a
copy buffer exported via the toolbar. Plain HTTP uses a paste dialog/copy fallback.
Browser/OS-reserved keys and clipboard permissions remain host limitations. This
frontend targets desktop browsers with keyboard/mouse; touch gestures/mobile
virtual keyboards have not been validated.

## Validation and remaining release work

Automated checks exercise shell input/output, vim alternate-screen editing,
htop on Linux, overlap and wide glyphs, keyboard/mouse input translation,
outline-resize commit/SIGWINCH, clipboard export and bracketed paste, tiling,
shelf activity, detach/reattach, smallest-client sizing, saving/reloading layouts,
browser-to-editor opening, UTF-8 Save As/reopen, and
unsaved editor close/cancel/save behavior, live theme changes across open
components and clients, theme-picker mouse/keyboard operation, configuration
reload, and detach/reattach theme persistence. Fake-agent integration fixtures
exercise project launch CWDs with spaces/metacharacters, project dialogs and
bookmark-only removal, background hook routing, prompt/OSC detection, visual-only
alerts, cross-workspace/shelf restoration and acknowledgement. They do not start
real coding agents or make provider API requests.
The separate controlling-PTY test checks the real client and exact terminal
settings restoration after detach and SIGTERM. CI covers Linux and macOS. Web transport integration tests check unauthenticated
static assets, mouse/keyboard/clipboard input, shared TUI themes, reconnect and
listener shutdown survival, cross-origin rejection and malformed input isolation.
A real Chromium fixture checks the DOM renderer, dragging, live themes, projects/
attention dialogs, HTTP paste fallback, viewport resize, refresh/reconnect and
absence of external asset requests.

This is not yet a claim of production-grade xterm compatibility. Before a stable
release, exercise long working sessions with vim, htop and mc; benchmark actual
100 ms SSH links; test tmux nesting across host terminals; and check gpm on a real
Linux console. gpm packet decoding has automated coverage; hardware delivery
cannot be validated on the macOS host or in an ordinary container. Emoji width
still depends on the host terminal agreeing with Unicode cell widths.

Session survival means client/SSH disconnect survival, not server crashes, host
reboots or power failures. Layout files restart programs and do not restore their
internal state. Nested saumux prints a notice; its bindings may be intercepted by
the outer desktop. Application clipboard requests are not forwarded automatically;
saumux selections are the explicit source of host clipboard writes.

Protocol and mouse behavior were checked against upstream documentation:

- https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
- https://docs.rs/vt100/0.18.1/vt100/
- https://docs.rs/portable-pty/0.9.0/portable_pty/
- https://docs.rs/crossterm/0.29.0/crossterm/
- https://github.com/telmich/gpm/blob/master/src/headers/gpm.h

## Build and release automation

`build.yml` is shared by main/PR checks and version-tag releases. It tests and
packages x86_64/aarch64 Linux-musl and Intel/Apple Silicon macOS executables on
native runners. `release.yml` validates the tag against Cargo.toml, waits for all
four builds and the reusable Chromium check, verifies SHA-256 digests and publishes
a GitHub release. A draft is created first so incomplete uploads are not published. If publication fails after
a draft is created, inspect/remove that draft before rerunning; published version
tags are immutable by convention.
