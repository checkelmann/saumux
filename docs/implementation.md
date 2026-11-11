# Implementation notes

For release state, continuation guidance and the current backlog, see
[the agent handoff](https://github.com/checkelmann/saumux/blob/main/docs/HANDOFF.md).

The target platforms are Linux **and macOS**, following the updated requirement.
Windows is outside scope. gpm applies only to Linux virtual consoles.

## Structure

- `browser.rs`, `editor.rs`, `builtin.rs`: bundled terminal applications with
  shared retro rendering, file navigation, text editing and save dialogs.
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
  still stops every program. No plugins, graphics protocols or network listener.

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
settings restoration after detach and SIGTERM. CI covers Linux and macOS.

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
four builds, verifies SHA-256 digests and publishes a GitHub release. A draft is
created first so incomplete uploads are not published. If publication fails after
a draft is created, inspect/remove that draft before rerunning; published version
tags are immutable by convention.
