# saumux

A Borland-inspired text-mode desktop and terminal multiplexer for **Linux and macOS**.
Windows overlap, programs run in their own PTYs, and commands are discoverable
through a global menu bar. Sessions keep running when a terminal or SSH connection
closes.

This is an early implementation of the November 2026 specification. It includes
the core multiplexer plus a bundled file browser, editor, system monitor, notes
and clock, configuration,
shelving, workspaces, layout files, multiple clients, coding-agent launchers and saved projects. See [implementation notes](docs/implementation.md) for limits
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

## Browser frontend

```sh
saumux web
```

Starts the web listener at **`0.0.0.0:8080`**. On another PC, open
`http://<saumux-computer-IP>:8080/`. There is **no authentication** in this version:
anyone who can reach this port can control the shared desktop and its programs.
The listener serves plain HTTP; TLS can be supplied by a reverse proxy.

The browser displays the same server-composed character grid, with overlapping
windows, mouse dragging/resizing, menus, function-key bar, themes, bundled
browser/editor, agents, projects and attention indicators. A symbols-font fallback
for Powerline/private-use prompt glyphs is embedded too, so the browser machine
does not need a Nerd Font installed. All web assets are embedded in the executable;
no CDN, Node installation or frontend build is needed to run it.

```sh
saumux web --session work                    # share work, or create it
saumux web --bind 0.0.0.0 --port 9090
saumux web --bind 127.0.0.1 --port 8080        # local access only
saumux web --session logs -- tail -f /path/to/log
saumux web --session restored --layout ./work.json
```

Without `--session`, the listener shares the only running session, starts
`default` if none exists, or asks for a name if several exist. Existing sessions
cannot also be given a startup program/layout. A TUI can attach simultaneously:
`saumux attach work`. Focus and themes are shared; the smallest attached browser
or terminal determines desktop size.

The browser resizes its grid to the viewport. Use the desktop menus by mouse or
F10; a small browser toolbar also provides default shortcuts and Reconnect.
Some browser/OS shortcuts cannot be captured; the desktop menus remain available.
Shift+drag selects through the browser terminal; ordinary dragging uses saumux.
Clipboard paste uses Ctrl+V / Command+V or the toolbar's Paste button. If plain
HTTP or browser permissions prevent clipboard access, Paste opens a text field.
Copying inside saumux fills its internal buffer; click the browser toolbar's Copy
button to export the latest selection to this PC. Clipboard behavior follows the
browser's permissions. Shift+Insert still pastes the internal saumux clipboard.

Closing/reloading a tab or detaching leaves its programs running. Reconnect
attaches again. **Ctrl+C** or SIGTERM stops the foreground web listener and
leaves the session server running. **System → End session** still ends the
session, just as in the TUI. Closing the last window leaves the session open with
the desktop menus available, so you can start another window without restarting
the web listener. The listener exposes one configured session; it has no remote
session picker. Its Unix socket stays private to the owning user.

## Using the desktop

Click **System → New shell**, or press **Alt+n**, to open a window. Click a window
to focus it, drag its title to move it, and drag its left or bottom edge to resize
it. The bottom-right corner resizes in both directions. The right edge is a
scroll bar. Title-bar buttons close, minimize to the shelf (`[_]`) and zoom the
window. Minimized programs keep running; click their shelf tab to restore them.

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
| Saved projects / agent attention | Alt+p / Ctrl+g |
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
| Ocean | Deep blue with seafoam titles and cyan menus |
| Plum | Purple panels with pink titles and silver menus |
| Forest | Black and green with silver menus |
| Burgundy | Wine-red frames with warm cream editor text |
| Ice | Light cyan and white with blue accents |
| Slate | Charcoal and silver with cyan titles |

Changes apply to the desktop, menus, frames, default terminal colours, and all
open bundled editors/browsers within about 100 ms. Running programs keep their
state, unsaved documents stay open, and every attached client sees the same
session theme. Program-specified colours are preserved.

```sh
saumux ctl theme.list
saumux ctl theme.set amber
saumux ctl theme.next
```

The selected theme is saved as `look.theme` in the saumux configuration, so new
sessions inherit it. Each running session still keeps its own choice across
detach/reattach. **System → Reload configuration** applies the configured theme
to the current session. You can also set `theme = "midnight"` in `[look]` by hand.

`look.ink = "theme"` and `look.paper = "theme"` follow the palette for default
program output. You can override either with `"default"` for the host terminal's
colour or an explicit `"#RRGGBB"`; explicit overrides remain in place when changing
themes. The interface uses the shared theme roles. `content = "mono"` remains an
optional setting for program output.

## Coding agents and projects

The **Agents** menu lists installed coding agents directly: Codex, Claude Code,
Gemini CLI, Aider, OpenCode, Goose, Amp and Cursor Agent. Discovery checks executable
files on the server's PATH at startup, on attach, and through **Rescan installed
agents**. It never runs an agent during discovery, installs software or changes
agent permissions. Start an agent from its menu entry in the active window's
working directory; its normal authentication and approval flow still applies.
Agents run in regular PTY windows and survive detach just like shells.

For multiline Claude prompts, **Shift+Enter** is forwarded distinctly from Enter;
plain Enter submits. The browser preserves the modifier directly. The TUI asks
supporting host terminals for disambiguated keyboard events. If your host sends
Shift+Enter as an ordinary Enter, use **Ctrl+J** for a newline instead. Claude also
supports a backslash followed by Enter. See [Claude's terminal guide](https://code.claude.com/docs/en/terminal-config).
An already-running server needs a new session to load an updated input encoder;
rebuilding alone does not replace its code. Keep existing sessions running.

Use **Projects → Add project directory…** to bookmark a directory, or **Save
active directory** to bookmark the active window's working directory. **Alt+p**
opens saved projects; choosing one offers installed agents, a shell, the bundled
browser or a new editor document in that directory. Use arrows/Enter or click.
Removing a project asks for confirmation and removes only the bookmark; files
and running programs are kept.

Bookmarks persist in `~/.config/saumux/projects.json` (or under
`$XDG_CONFIG_HOME/saumux`). Sessions share the file; **Refresh projects** reloads
changes made elsewhere. Paths are canonicalized, duplicates reuse the existing
bookmark, and at most 128 projects can be saved.

```sh
saumux ctl agent.list
saumux ctl agent.rescan
saumux ctl project.add ./my-project --name Work
saumux ctl project.list
saumux ctl agent.start codex --project Work
saumux ctl project.remove Work
```

An agent needing attention gets a coloured `!` on its title/shelf tab and a
persistent clickable `!N` indicator beside the top-row clock, plus a badge in
the key bar. **Ctrl+g** or **Agents → Agent attention…** lists alerts across all
workspaces and restores/focuses the selected agent. The top indicator includes
shelved agents; click it to open the attention list. Notifications do not
automatically open or focus windows. Focus alone keeps its alert; typing,
pasting or an application mouse click
acknowledges it. **Clear active agent alert** also clears it. There is no sound.

**Ctrl+g** uses the Control key on macOS, Linux and the browser frontend; no
Option/Meta terminal setting is required. It reserves Ctrl+g from applications;
rebind or unbind `agent.attention` under `[keys]` if needed.

Attention can be reported explicitly through `ctl`, OSC 9 text notifications,
OSC 777 notifications or a program's terminal bell. A bell becomes a visual
notice and is never sent to the host terminal. Numeric OSC 9 progress messages
are ignored. For agents without notifications, saumux looks for common English
approval/input prompts after output settles (900 ms by default). These guesses
say **May need input**; inactivity alone never triggers an alert. Detection is
best effort and can miss differently worded prompts or flag quoted examples.
Only windows launched as agents are monitored.

For dependable agent-specific events, connect the agent's documented notification
hook to the following commands. Agent windows inherit `SAUMUX`, `SAUMUX_WINDOW`
and `SAUMUX_BIN`, so hooks target their own window even when it is in the
background. Keep the hook's environment intact. From an external script, supply
`--session` and `--window` explicitly.

```sh
"$SAUMUX_BIN" ctl agent.notify waiting "Approval needed"
"$SAUMUX_BIN" ctl agent.notify attention "Please review"
"$SAUMUX_BIN" ctl agent.notify done "Turn finished"
"$SAUMUX_BIN" ctl agent.notify working
saumux ctl --session work agent.notify waiting --window 2
saumux ctl agent.ack 2
```

`working` clears the notice; other states set a persistent visual notice.
`ctl list --json` includes each window's agent identity, project and attention
state; `notice.possible` distinguishes prompt guesses from explicit signals.
Provider hooks are opt-in: saumux does not rewrite their settings or enable
unattended approvals. Saved layouts retain agent identity and launch commands,
but not transient alerts or running conversation state.

Additional agents, launch arguments and notification settings can be configured:

```toml
[notifications]
detect_prompts = true       # false keeps explicit notifications only
prompt_delay_ms = 900       # 100..10000

[[agents]]
id = "custom"
name = "My coding agent"
command = ["my-agent", "--some-option"]
enabled = true
```

Commands are argument arrays, without shell expansion. A configured ID replaces
the built-in entry with that ID; `enabled = false` hides it. Use the IDs returned
by `agent.list` (Cursor Agent's ID is `cursor`). Reload configuration after edits.

## Built-in apps

Open the **Apps** menu for **System Monitor**, **Notes** and **Clock**. They run
inside regular saumux windows: move, resize, zoom, shelve and arrange them as
usual. They share live colour themes and work in both the TUI and web frontend.
All data comes from the machine running saumux, including when viewing remotely.

| App | Features | Controls inside the app |
| --- | --- | --- |
| System Monitor | Aligned CPU/RAM/swap readings, a scaled CPU history chart, load/uptime and compact per-core meters | Space pauses/resumes; R refreshes; arrows, PgUp/PgDn or wheel scroll cores |
| Notes | Markdown notebook with a searchable note list, rendered reading preview, and inline editing | N creates; / searches; Enter/E or click the preview to edit; Ctrl+s saves; Delete removes with confirmation |
| Clock | Large digital clock, weekday/date, local or UTC, 12/24-hour display | U switches local/UTC; T switches 12/24h; clickable toolbar |

System Monitor samples approximately once per second; the first CPU reading
appears after the initial sampling interval. It displays aggregate OS statistics,
not a process manager. The CPU line chart has a fixed 0–100% scale and shows the
latest 60 readings, with the newest at the right. Readings normally arrive once
per second; manual refresh adds readings too. Per-core meters use columns and
can be scrolled with arrows, PgUp/PgDn, Home/End or the mouse wheel. A separate,
high-contrast monitor surface follows the selected theme; its thin meters leave
empty space rather than dense shaded tracks. The Pause button changes to Resume
while readings are frozen. Small windows use a compact summary.
Clock uses the server host's local timezone by default and falls back to compact
text in small windows. Clock display options and monitor pause/history belong
to the running app; reopening starts with defaults. Ctrl+q or Esc closes either
read-only app.

Notes lists Markdown files in `$XDG_DATA_HOME/saumux/`, or
`~/.local/share/saumux/` when the data directory is unset. It also keeps an
existing `notes.txt` scratchpad visible. Press **N** to create a Markdown note,
**/** to search titles and contents, and **Enter** or **E** to edit the selected
note inline; clicking in its preview also enters edit mode. The preview renders
CommonMark with tables, task lists, strikethrough and footnotes.
It supports all six heading levels, bold/italic, inline and fenced code, nested
ordered/unordered lists, quotes, rules and links. Text wraps to the pane width;
PgUp/PgDn and the wheel scroll rendered rows. Links show their destinations and
images show alt text plus their paths. HTML stays literal; images are not loaded
and code blocks have no syntax highlighting. **Ctrl+S** saves,
**Ctrl+Z/Y** undo and redo, and **Esc** returns to preview with a
save/discard/cancel prompt for unsaved edits.

In preview, press **Delete** or click **Delete** to remove the selected file.
The prompt names the file: **Enter** or **Delete permanently** confirms; **Esc**
or **Cancel** keeps it. Deletion is permanent and refreshes the current search.
Return from editing first; unsaved edits retain save/discard/cancel protection.
An empty notebook starts with no files; deleted legacy scratchpads stay deleted.

Inline editing uses the bundled editor's UTF-8 handling, undo and save protection.
New notes are private
(0600) in a private data directory (0700). Reopening Notes in a session focuses
the existing notebook; notes themselves are ordinary files shared between
sessions.

```sh
saumux ctl monitor.open
saumux ctl notes.open
saumux ctl clock.open
```

These command names also work in configurable keyboard bindings; no additional
default shortcuts are reserved. `[[menu.apps]]` can add your own launchers.
New apps require a new session server started with the updated executable;
existing session servers keep their current code.

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
confirmation and stops all its programs. Closing the last window leaves an empty
session running; use **System → New shell** or another launcher to continue.
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
theme = "borland"     # use `saumux ctl theme.list` to list all twelve palettes
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

Launcher entries can be added to `system`, `file`, `edit`, `view`, `window`, `agents`, `projects`, `apps` and
`help`. Key names use `Ctrl+`, `Alt+`, `Shift+` in that order, function keys such
as `F6`, `PgUp`, `PgDn`, `Space`, and crossterm names such as `Insert`, `Left`,
`Enter`. Use `ctl commands` for action names. Rebinding a command replaces its
default shortcut; both menus and the key bar show the effective binding.

## Development

For agents and maintainers, start with [the handoff](https://github.com/checkelmann/saumux/blob/main/docs/HANDOFF.md) and
[implementation notes](docs/implementation.md).

Run `make help` to see the available development targets. Common commands are
`make check` (format, lint and Rust tests), `make build`, `make release`,
`make pty` (terminal restoration check), and `make verify` (the local release
checks). `make run ARGS='new dev'` starts a new development session.

The integration tests start real PTYs and servers. Python 3 is needed for the
input/resize fixtures; release helpers require Python 3.11 or newer. vim and htop tests run when those programs are installed.
CI runs on both Ubuntu and macOS. All dependencies are embedded in the executable;
no separate saumux daemon, emulator library or service needs to be installed.

Optional real-browser check (Node 22+; Chromium is installed only for testing):

```sh
make browser-deps
make browser
```

CI and tag releases also run this check on Linux. The fixture starts its own
isolated session/listener and never connects to a user's running desktop.

MIT licensed. Rust dependencies retain their own licenses. Bundled browser
licenses are recorded in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Releases

GitHub Actions builds and tests native Intel and ARM variants for Linux and
macOS. Linux release executables use musl for static linking. Each archive
contains `saumux`, documentation, the license and the optional terminfo entry.
Downloads are accompanied by `SHA256SUMS`.

v0.1.0 is already published. Live themes, agent/project support, the web frontend
and the additional built-in apps require a newer build from `main`; they are
not in the v0.1.0 downloads.

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
