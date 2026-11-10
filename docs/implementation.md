# Implementation notes

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
roles in a Borland-inspired 16-colour palette for desktop surfaces, menus,
focus, buttons, scroll bars and the bundled applications. The compositor sends
standard/bright ANSI sequences for these colours; the terminal palette determines
the exact shades. Program colour output is preserved by default.

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

## Validation and remaining release work

Automated checks exercise shell input/output, vim alternate-screen editing,
htop on Linux, overlap and wide glyphs, keyboard/mouse input translation,
outline-resize commit/SIGWINCH, clipboard export and bracketed paste, tiling,
shelf activity, detach/reattach, smallest-client sizing, saving/reloading layouts, browser-to-editor opening, UTF-8 Save As/reopen, and
unsaved editor close/cancel/save behavior.
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
