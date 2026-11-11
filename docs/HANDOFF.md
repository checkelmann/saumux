# Agent handoff

Last verified: **2026-11-11**. This file is the continuation entry point, not a
verbatim conversation transcript. Read it together with the README and implementation
notes; inspect the current source and Git history before making changes.

## Product direction and accepted scope changes

saumux is a terminal multiplexer presented as a text-mode desktop. Free,
overlapping windows are the base model; tiling is a one-off arrangement command.
Commands should be discoverable through menus and mouse controls, with keyboard
routes as well. The server owns programs and survives client/SSH disconnects.

The user explicitly changed three original requirements:

- Both Linux and macOS are first-class targets, rather than Linux only.
- Bundled file browser and editor components are now wanted. They remain PTY
  programs from the same executable, integrated into the desktop's File menu.
- The monochrome requirement was removed in favor of a colourful Borland/Turbo
  Vision style, subsequently extended with live selectable themes.

Windows support, graphics protocols, a plugin language and direct network access
remain outside the current scope. Remote access uses SSH. Do not confuse layout
saving with checkpointing program state.

## Repository, release and development state

- Public repository: https://github.com/checkelmann/saumux
- Default branch: `main`.
- Published release: https://github.com/checkelmann/saumux/releases/tag/v0.1.0
- `v0.1.0` points to `14e81f20265f28dd1b4f968ff47e1887f02028db`. Its four binary
  archives and SHA256SUMS were downloaded and verified after publication.
- Live theme switching was added later in commit
  `170733f5f6ce1e5cfb56033893e4535275b6a85e` on `main`. It is **not included in
  the published v0.1.0 archives**.
- Cargo.toml still declares version `0.1.0` on this development snapshot. Before
  the next requested release, choose/bump its version and update Cargo.lock.
  Never move the existing v0.1.0 tag to include newer code.
- The local `target/release/saumux` was rebuilt with live theme support. Build
  artifacts are ignored by Git and must be rebuilt in a fresh checkout.

The initial release workflow completed successfully:
https://github.com/checkelmann/saumux/actions/runs/42853382887

The live-theme CI run was queued at the previous conversation handoff. It has
since completed **successfully on all four targets**, verified on 2026-11-11:
https://github.com/checkelmann/saumux/actions/runs/42888997606

Do not assume these links establish the status of later commits; query their
current checks. At the previous handoff the user explicitly asked not to wait for
queued runners. Report runner-capacity delays rather than delaying a handoff only
for a queued job, unless the user asks otherwise.

## What is implemented

- Detached server/client, private Unix sockets, peer credentials, named sessions,
  `ls`, attach/detach, shared focus and smallest-client desktop sizing.
- Multiple PTYs and vt100 emulators, overlap, focus, moving/resizing by outline,
  zoom, close, differential rendering, menus, clickable key bar, tile/cascade.
- Scrollback, window-bounded copying, internal clipboard, OSC 52 export,
  bracketed paste and application mouse-coordinate translation.
- Shelf/activity indicators, four workspaces, TOML configuration/reload,
  external `ctl` commands and saved/reloaded arrangements.
- File browser: directory navigation, hidden toggle, refresh, location entry,
  mouse/keyboard opening of text files in a separate editor window.
- Editor: UTF-8, search, undo/redo, cursor navigation, scrolling, Save/Save As,
  atomic writes, external-change detection, preserved line endings/permissions,
  and save/discard/cancel on close.
- Themes: Borland, Norton, Amber, Phosphor, Midnight and Paper. View → Colour
  theme / Alt+t, `theme.set`, `theme.next`, `theme.list`; all attached clients and
  already-open bundled applications update without restarting or losing state.

The README is the authoritative quick reference for shortcuts and CLI examples.
The implementation notes map modules to responsibilities and list resource bounds.

## Important implementation contracts

- `commands.rs` drives menu entries and configurable keyboard bindings. Add
  interactive actions there rather than inventing a separate prefix/mode system.
- `app.rs` owns window-manager state and composition; `render.rs` owns cells,
  widths, attributes and ANSI differences. Preserve wide-character clipping and
  whole-glyph cleanup when overlapping either half of a wide character.
- `window.rs` owns PTYs, parser callbacks and terminal query replies. Input and
  output use separate threads/queues so a stalled application does not block the
  desktop. Resize the PTY only when a drag/keyboard resize is committed.
- `browser.rs`, `editor.rs` and `builtin.rs` run under the same binary's internal
  `__files` / `__edit` entry points. Avoid introducing external editor/browser
  dependencies as a replacement for the bundled components.
- **Editor close is special:** `window.close` sends Ctrl+Q to a bundled editor so
  it can protect dirty buffers. Replacing this with unconditional window removal
  would cause data loss. Confirmed session shutdown still stops all programs.
- Live palettes are semantic roles in `theme.rs`. `look.theme` picks the startup
  palette; `ink = "theme"` and `paper = "theme"` follow it for default program
  output. Explicit host/hex overrides and program-specified colours take precedence.
- Runtime theme choice is per session, survives detach, and does not rewrite the
  user's config. Configuration reload reapplies the configured theme.
- The server atomically publishes `<session>.theme` with mode 0600 inside its
  private runtime directory. `SAUMUX_THEME_FILE` tells bundled applications where
  to read it, at most every 100 ms through a thread-local cache. Session shutdown
  removes it. Preserve this cross-process update path when changing theme roles.
- Session persistence covers client disconnects, not server crashes or reboots.
  Layouts restart recorded commands/directories; they do not save dirty editor
  buffers or arbitrary program state.

## Build, test and safely try changes

Use stable Rust. Cargo declares a minimum of 1.88, but the minimum toolchain has
not been separately validated. Development validation also needs Python 3;
release helpers specifically require Python **3.11+** (`tomllib`). Install vim and
htop to exercise their optional smoke tests; otherwise those test functions skip.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
python3 tests/client_pty.py target/release/saumux
python3 -m unittest discover -s tests -p 'release_*.py'
```

Use a new, uniquely named session when trying a new executable:

```sh
./target/release/saumux new agent-test
```

Rebuilding does not replace an already-running server's code. A plain `saumux`
may attach to an older session; that is not evidence that the new build is broken.
Do not kill existing sessions just to upgrade or test. Integration fixtures use
isolated temporary runtime/config directories and clean up their own processes.

Current checks include real PTY programs, editor/browser operations and saving,
mouse translation, SIGWINCH, clipboard/paste, layouts, multiple clients, live
recolouring with unchanged process IDs/dirty buffers, picker mouse/keyboard
selection, configuration reload and theme persistence across detach. The separate
Python controlling-PTY test checks exact terminal-settings restoration.

Three CI issues already fixed must not be reintroduced:

1. A Unix socket path can exist between `bind` and `listen`. Test startup waits
   for a successful connection, not just path existence.
2. Vim's welcome banner varies by build/window height. The test waits for the
   buffer and verifies actual inserted text, not the optional `VIM` banner.
3. Redraw timing varies by runner. Wait for the expected screen state instead of
   assuming a short sleep/read gap means an input event has been rendered.

Also retain the inferred clock argument type for `localtime_r`: naming
`libc::time_t` directly triggers a deprecation warning on the musl targets.
Current stable Rust can diagnose warnings missed by an older local toolchain;
keep CI's `-D warnings` checks.

## Known gaps and useful next work

These are a backlog, not new user-authorized tasks. Prioritize the user's next
request and verify a suspected bug before changing behavior.

- Daily-use terminal compatibility testing with vim, htop and mc; actual 100 ms
  SSH benchmarking; tmux-inside/around-saumux mouse/key testing; gpm on real Linux
  console hardware. Automated smoke tests do not establish full xterm compatibility.
- Original product research is still open: hands-on comparison with twin, vtm
  and XtermWM, and checking project/package-name collisions beyond creating this
  GitHub repository. Do not present the original search notes as usage experience.
- Shelf tabs overflow the visible row; the window list needs scrolling when its
  entries exceed its height. Check matching mouse hit regions while improving it.
- Mouse resizing covers left/bottom edges and the bottom-right corner. Top/right
  resizing and scroll-bar ownership need design work if extending this behavior.
- Tiny-terminal fallback keeps both menu and key bars, differing from the initial
  menu-only proposal. Four workspaces, 64 windows, 500×200 desktop cells and 10,000
  scrollback lines are current fixed bounds, not promises of arbitrary scale.
- Configuration adds launchers to fixed menu groups rather than freely arranging
  built-in menus. Command values are validated; key spelling/normalization could
  be improved. Host applications may intercept function/Alt shortcuts.
- Browser has no file copy/move/rename/delete operations. Editor has no native
  selection model, syntax highlighting, autosave or crash recovery; it uses the
  desktop clipboard. Its current file limit is 2 MiB / 100,000 lines, with bounded
  snapshot undo history.
- Mono program-colour mapping is intentionally simple and can lose distinctions
  such as red/green diffs. Host terminal capabilities are mostly requested
  optimistically; mouse-hover traffic/capability fallbacks merit real-host testing.
- `app.rs` is large; separating command dispatch, modal UI and composition could
  make future changes easier without changing the interaction model.
- No installer/Homebrew/Linux package distribution or macOS signing/notarization
  is set up. The release currently provides tar archives and checksums.

## Release workflow

- `ci.yml`: main pushes, PRs and manual check runs; calls `build.yml`.
- `build.yml`: native x86_64/aarch64 Linux-musl and Intel/Apple Silicon macOS,
  strict checks, tests, optimized executable, PTY lifecycle check, packaging helper
  tests, tar archives and build artifacts. Linux binaries are statically linked.
- `release.yml`: a `v*` tag push validates against Cargo.toml, runs all four builds,
  verifies archive hashes, creates a draft with generated notes, uploads assets
  and publishes it. Prerelease version tags produce prereleases.
- `scripts/release.py`: validates tag/version matching and packages the binary,
  README, LICENSE, implementation notes and terminfo with SHA-256 sidecars.

For the next requested release, update Cargo.toml/Cargo.lock, commit/push, then
create and push the matching new annotated version tag. v0.1.0 is already taken.
If publication fails after draft creation, inspect that draft before rerunning;
do not move a published tag to work around an error. Build caches and runner
capacity affect duration, especially on macOS; they do not justify skipping tests.
