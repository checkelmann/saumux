# saumux

A Borland-inspired text-mode desktop and terminal multiplexer for **Linux and macOS**.
Windows overlap, programs run in their own PTYs, and commands are discoverable
through a global menu bar. Sessions keep running when a terminal or SSH connection
closes.

This is an early implementation of the November 2026 specification. It includes
the core multiplexer plus a bundled file browser and editor, configuration,
shelving, workspaces, layout files and multiple clients. See [implementation notes](docs/implementation.md) for limits
and validation still needed before treating it as a daily driver.

## Build and run

Install stable Rust (1.88 or newer), then:

```sh
cargo build --release --locked
./target/release/saumux
```

Or install the executable on your PATH:

```sh
cargo install --path . --locked
saumux
```

On macOS, use Terminal, iTerm2 or another terminal with mouse reporting. On Linux,
use a terminal emulator, SSH, or a virtual console. A running gpm service supplies
mouse input on a Linux virtual console; keyboard operation does not require it.

Optional terminfo installation:

```sh
tic -x -o ~/.terminfo terminfo/saumux.terminfo
```

Programs receive `TERM=saumux-256color` when that entry is installed, otherwise
`TERM=screen-256color`. The latter must exist on the machine running the programs.

## Using the desktop

Click **System → New shell**, or press **Alt+n**, to open a window. Click a window
to focus it, drag its title to move it, and drag its left or bottom edge to resize
it. The bottom-right corner resizes in both directions. The right edge is a
scroll bar. Title-bar buttons close and zoom the window.

| Action | Default shortcut |
| --- | --- |
| Open menus | F10, then arrows and Enter |
| New shell / detach | Alt+n / Alt+d |
| Next window / focus window 1–9 | F6 / Alt+1–9 |
| Move / resize | Alt+F5 / Alt+F8, arrows, Enter; Esc cancels |
| Close / zoom | Alt+F4 / Alt+F10 |
| Tile / cascade / shelve all | F7 / F8 / F9 |
| Shelve active window / window list | Alt+F9 / Alt+0 |
| Scroll history | Shift+PgUp / Shift+PgDn |
| Select using the keyboard | Alt+Space, arrows, Enter to copy |
| Copy / paste internal clipboard | Alt+c / Shift+Insert |
| Previous / next workspace | Alt+Left / Alt+Right |
| Colour theme picker | Alt+t |
| Help | F1 |

Every function-key label in the bottom bar is clickable. Menus also contain
commands without default shortcuts. Four workspaces are available through View;
a window can be moved to the next workspace through the same menu.

Drag in program content to select and copy text. Copy also emits OSC 52 to the
attached terminal; host clipboard support and permission depend on that terminal.
Middle-click pastes the internal buffer. In a mouse-aware program, content clicks
and drags go to that program; use **Alt+drag** for saumux selection. **Shift+drag**
uses the host terminal's selection where supported.

Shelved programs continue running. Their tabs appear at the bottom of the desktop;
`*` marks output or a bell. Restore them with a click or the window list.

Reserved keys can conflict with programs such as htop and mc. Rebind or unbind
them in the configuration. macOS or the host terminal may also intercept function
keys or Alt combinations; the menu remains accessible with the mouse.

## Live colour themes

Open **View → Colour theme…** or press **Alt+t** to choose a palette. Use arrows
and Enter, or click a theme. The picker includes a colour swatch for each option;
Esc closes it without changing anything.

| Theme | Appearance |
| --- | --- |
| Borland | Classic blue, silver menus, cyan titles and yellow editor text |
| Norton | Blue panels with cyan menus and yellow highlights |
| Amber | Warm amber text on black |
| Phosphor | Green CRT text on black |
| Midnight | Dark charcoal with violet and cyan accents |
| Paper | Light paper with dark text and blue accents |

Changes apply to the desktop, menus, frames, default terminal colours, and all
open bundled editors/browsers within about 100 ms. Running programs keep their
state, unsaved documents stay open, and every attached client sees the same
session theme. Program-specified colours are preserved.

```sh
saumux ctl theme.list
saumux ctl theme.set amber
saumux ctl theme.next
```

The choice belongs to the current session and survives detach/reattach. To choose
a default for new sessions, set `theme = "midnight"` in `[look]` in the configuration.
**System → Reload configuration** also applies the configured theme to a running
session. Selecting a theme does not rewrite the configuration file.

`look.ink = "theme"` and `look.paper = "theme"` follow the palette for default
program output. You can override either with `"default"` for the host terminal's
colour or an explicit `"#RRGGBB"`; explicit overrides remain in place when changing
themes. The interface uses the shared theme roles. `content = "mono"` remains an
optional setting for program output.

## File browser and editor

Open **File → Browse files** (**Alt+b**) or **File → New text document** (**Alt+e**).
**File → Open text file** opens the browser as a file picker. Both components are
bundled in the saumux binary and run in regular PTY windows, with the same move,
resize, shelf and detach behavior as shells.

The browser lists directories first. Use arrows and Enter, or double-click, to
open a folder or edit a file in a separate window. Backspace goes to the parent;
`.` toggles hidden files; Ctrl+r refreshes; Ctrl+l changes location. Its toolbar
also provides these actions by mouse. Ctrl+q closes the browser. The browser
currently handles navigation and opening files; it has no delete, rename or copy
operations.

The editor supports UTF-8 text, mouse cursor placement, scrolling, tabs, search,
undo/redo, Save and Save As. Its toolbar is clickable, and save actions are also
available in the desktop File menu.

| Editor action | Shortcut |
| --- | --- |
| Save / Save As | Ctrl+s / Ctrl+o |
| Find (case-sensitive, wraps) | Ctrl+f |
| Undo / redo | Ctrl+z / Ctrl+y |
| Close | Ctrl+q, or the window close button |
| First / last line | Ctrl+Home / Ctrl+End |

Path/search fields accept Enter and Esc; Ctrl+u clears the field. Use Alt+drag and
Alt+c for saumux text copying, and Shift+Insert for its clipboard paste. There is
no native editor text-selection model or syntax highlighting in this first version.

Unsaved documents show `*` in their title. Closing asks whether to save, discard,
or cancel. Save As asks before replacing another file. Saves use a temporary file
and atomic rename, preserve existing permissions and LF/CRLF line endings, and
refuse to overwrite a file changed on disk since it was loaded. Binary and
non-UTF-8 files are rejected. The initial limit is 2 MiB / 100,000 lines per file.

```sh
saumux ctl browser.open /path/to/project
saumux ctl editor.new
saumux ctl editor.open /path/to/notes.txt
```

## Sessions and automation

```sh
saumux new work
saumux new logs -- tail -f /var/log/system.log
saumux ls
saumux attach work
saumux ctl --session work window.new --title Logs -- journalctl -f
saumux ctl --session work layout.tile
saumux ctl --session work list --json
saumux ctl --session work commands
saumux ctl --session work layout.save ./work.json
saumux --layout ./work.json --session restored
saumux ctl --session work session.end --confirm
```

`saumux` attaches when exactly one session exists and starts `default` when none
exists. With several sessions, choose one explicitly. Inside a window, `ctl` uses
its session automatically through `SAUMUX`. Window IDs appear in the window list
and JSON listing; `saumux ctl window.focus 2` restores and focuses window 2.

**System → Detach** disconnects the current client. **End session** asks for
confirmation and stops all its programs. Closing the last window ends the server.
An unexpected client exit leaves programs running. Several clients share focus;
the smallest attached terminal determines desktop size. Larger terminals have
empty space outside it.

Save arrangement defaults to `~/.config/saumux/layout.json`. Layouts restart
commands in their recorded directories; they do not checkpoint program state.

Sockets live in `$XDG_RUNTIME_DIR/saumux`, or the platform temporary directory's
`saumux-<uid>` directory when XDG_RUNTIME_DIR is absent (including typical macOS
setups). The directory is private, sockets have mode 0600, and the server checks
peer credentials. Startup diagnostics are kept in `<session>.log` beside sockets.
Remote access uses SSH.

## Configuration

Create `~/.config/saumux/config.toml`, or `$XDG_CONFIG_HOME/saumux/config.toml`.
**System → Reload configuration** applies changes to the running session. Invalid
configuration leaves the existing configuration active.

```toml
# Optional startup commands, each run through /bin/sh -lc.
start = ["$SHELL"]

[look]
theme = "borland"     # borland, norton, amber, phosphor, midnight, paper
ink = "theme"         # theme default; "default" uses the host; or "#RRGGBB"
paper = "theme"       # theme default; "default" uses the host; or "#RRGGBB"
content = "color"     # preserve program colours; optional "mono" maps attributes
shadow = true

[keys]
"F2" = "menu.open"    # moves the menu shortcut from F10 to F2
"F10" = ""            # passes F10 through to the active program
"F6" = "window.next"
"Alt+F4" = "window.close"

[[menu.system]]
label = "Processes"
run = "htop"

[[menu.file]]
label = "Editor"
run = "vim"
```

Launcher entries can be added to `system`, `file`, `edit`, `view`, `window` and
`help`. Key names use `Ctrl+`, `Alt+`, `Shift+` in that order, function keys such
as `F6`, `PgUp`, `PgDn`, `Space`, and crossterm names such as `Insert`, `Left`,
`Enter`. Use `ctl commands` for action names. Rebinding a command replaces its
default shortcut; both menus and the key bar show the effective binding.

## Development

For agents and maintainers, start with [the handoff](https://github.com/checkelmann/saumux/blob/main/docs/HANDOFF.md) and
[implementation notes](docs/implementation.md).

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/client_pty.py target/debug/saumux
```

The integration tests start real PTYs and servers. Python 3 is needed for the
input/resize fixtures; release helpers require Python 3.11 or newer. vim and htop tests run when those programs are installed.
CI runs on both Ubuntu and macOS. All dependencies are embedded in the executable;
no saumux daemon, emulator library or service needs to be installed separately.

MIT licensed. Rust dependencies retain their own licenses.

## Releases

GitHub Actions builds and tests native Intel and ARM variants for Linux and
macOS. Linux release executables use musl for static linking. Each archive
contains `saumux`, documentation, the license and the optional terminfo entry.
Downloads are accompanied by `SHA256SUMS`.

v0.1.0 is already published. Live theme switching is currently on `main` and
requires a newer build; it is not in the v0.1.0 downloads.

To publish the next version, update `version` in Cargo.toml, run `cargo check` to update
Cargo.lock, commit and push those changes, then push the matching version tag:

```sh
git tag -a v0.1.1 -m "saumux 0.1.1"
git push origin v0.1.1
```

The release workflow rejects a tag that does not match Cargo.toml. All four builds
must pass tests and packaging before publication. Tags such as `v0.2.0-beta.1`
produce prereleases when Cargo.toml contains the same prerelease version. A tag
push is the publication trigger; ordinary main-branch pushes only run checks and
upload preview build artifacts. Never reuse a published version tag.
