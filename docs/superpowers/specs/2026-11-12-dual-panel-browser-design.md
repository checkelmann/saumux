# Dual-Panel File Browser Design

**Date:** 2026-11-12
**Status:** Approved

## Summary

Replace the existing single-panel file browser (`src/browser.rs`) with a Norton Commander-style dual-panel browser. Both panels show directory listings side-by-side in a fixed 50/50 split. File operations (copy, move, delete, mkdir, rename) work between panels. Multi-file marking is supported. A floating preview overlay shows text content or image metadata.

## Module Structure

The existing `src/browser.rs` is converted into a module:

```
src/browser/
├── mod.rs      — DualBrowser struct + public run() entry point
├── panel.rs    — Single panel logic (existing Browser struct renamed Panel, + marked set)
├── ops.rs      — File operations: copy, move, delete, mkdir, rename
└── preview.rs  — Floating preview overlay
```

The public API (`browser::run()`, `browser::launch_args()`) is unchanged so `src/main.rs` and `src/app.rs` require no changes.

## Layout

```
[Tab | Copy | Move | Delete | MkDir | Rename | Preview | Hidden | Close]
/home/user/docs              │ /home/user/downloads
D folder/                    │ D other/
> selected.txt     1.2 KB   │ * marked.txt       4.2 KB
  another.txt      2.4 KB   │   image.png        3.5 MB
─────────────────────────────┴──────────────────────────
4 entries · hidden off       │ 1 marked · 3 entries
[message bar]
```

- `>` — cursor position in the active panel
- `*` — marked file (selected for batch operations)
- `D` — directory prefix (unchanged)
- The vertical divider is a single `│` character drawn at `width / 2`
- Each panel occupies exactly half the terminal width; the split is fixed

## Panel (`panel.rs`)

Extracted from the existing `Browser` struct with minimal changes:

- `cwd: PathBuf`
- `entries: Vec<Entry>`
- `selected: usize`
- `top: usize`
- `hidden: bool`
- `marked: HashSet<usize>` — NEW: indices of marked entries
- `last_click: Option<(usize, Instant)>` — double-click detection

Methods: `new()`, `refresh()`, `navigate()`, `up()`, `draw(width, height, active)` — the `active` flag controls whether to show `>` or a plain highlight for the cursor row.

The `draw()` method returns a `Grid` for its half-width column only; `DualBrowser` composites both halves.

**Marking:** Insert or Space toggles the mark on the current entry and advances the cursor. `Ctrl+A` marks all non-`..` entries. Marked entries display `*` in the first column; the status bar shows `N marked` when any are marked.

## Dual Browser (`mod.rs`)

```rust
struct DualBrowser {
    left: Panel,
    right: Panel,
    active: Side,       // enum Side { Left, Right }
    message: String,
    prompt: Option<(PromptKind, Prompt)>,  // location, rename, mkdir
    op: Option<OpConfirm>,                 // pending destructive operation
    preview: Option<PreviewOverlay>,
}
```

`run()` owns the `Surface`, the event loop, and routes input to whichever panel is active (or to a prompt/overlay if one is open).

**Active panel:** Tab switches focus. Mouse clicks on either panel half set focus to that half. The toolbar and message bar span the full width.

**Input routing:**
1. If `preview` is `Some` → route to preview (arrow keys scroll, Escape closes)
2. Else if `op` is `Some` → route to op confirmation (Enter confirms, Escape cancels)
3. Else if `prompt` is `Some` → route to prompt (Enter submits, Escape cancels)
4. Else → route to active panel + handle global shortcuts

## File Operations (`ops.rs`)

Each operation follows the same flow:
1. **Collect sources** — the marked set of the active panel; falls back to the cursor entry if the marked set is empty. The `..` entry is never included.
2. **Show confirmation prompt** — a one-line `Prompt` at the bottom. For copy/move the default text is the other panel's `cwd`. For delete/mkdir/rename the prompt text is a description.
3. **Execute on Enter** — runs synchronously (no background thread). Progress is shown in the message bar as each file is processed.
4. **Refresh both panels** on completion.

| Operation | Ctrl key | Toolbar label | Confirmation text |
|-----------|----------|---------------|-------------------|
| Copy      | Ctrl+C   | Copy          | `Copy N files to: <other-panel-cwd>` |
| Move      | Ctrl+X   | Move          | `Move N files to: <other-panel-cwd>` |
| Delete    | Ctrl+D   | Delete        | `Delete N files? [Enter/Esc]` |
| New dir   | Ctrl+N   | MkDir         | `New directory name:` |
| Rename    | Ctrl+R   | Rename        | `Rename to:` (single file only) |

Copy/Move use `std::fs::copy` + `std::fs::remove_file` (move = copy + delete source). Directories are copied/moved recursively. Delete uses `std::fs::remove_file` / `std::fs::remove_dir_all`. All errors are caught per-file and reported in the message bar; processing continues with remaining files.

## Preview Overlay (`preview.rs`)

A centered floating `Grid` drawn over both panels. Width = `min(terminal_width - 4, 80)`, height = `terminal_height - 4`. Dismissed with Escape or Ctrl+P.

**Text files** (UTF-8 or valid text): first 500 lines loaded, displayed with line numbers, scrollable with Up/Down/PgUp/PgDn.

**Images** (detected by magic bytes: PNG `\x89PNG`, JPEG `\xff\xd8`, GIF `GIF8`, BMP `BM`, WebP `RIFF...WEBP`): metadata card showing format, pixel dimensions (parsed from header), and file size. No external dependencies.

**Binary / unknown**: hex dump of the first 256 bytes, 16 bytes per row.

## Key Bindings

| Key | Action |
|-----|--------|
| Tab | Switch active panel |
| Insert / Space | Mark/unmark current file, advance cursor |
| Ctrl+A | Mark all (non-`..`) in active panel |
| Ctrl+C | Copy to other panel |
| Ctrl+X | Move to other panel |
| Ctrl+D | Delete |
| Ctrl+N | New directory |
| Ctrl+R | Rename (single file) |
| Ctrl+P | Open/close preview overlay |
| Enter | Open (directory: navigate; file: open in editor) |
| Backspace | Go up one directory |
| Ctrl+L | Location prompt |
| `.` | Toggle hidden files |
| Ctrl+Q | Close browser |
| Up/Down/PgUp/PgDn/Home/End | Navigate list |
| Mouse click | Set active panel + move cursor |
| Mouse double-click | Open entry |
| Mouse scroll | Scroll list |

## Tests

- Existing `directories_first_hidden_toggle_and_navigation` test moves to `panel.rs` and continues to pass unchanged.
- New tests in `ops.rs`: copy file, move file, delete file, mkdir — using `tempfile` or `std::env::temp_dir()` (matching existing test style).
- New test in `panel.rs`: mark/unmark toggle, mark-all, `..` excluded from mark-all.
- New test in `preview.rs`: image format detection from magic bytes for each supported type.

## Out of Scope

- Resizable panels
- Background file operation threads / progress bars
- Sixel / Kitty / iTerm2 inline image rendering
- File permissions editing
- Symlink creation
- Sorting options beyond the existing dirs-first + alphabetical
