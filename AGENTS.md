# Working on saumux

Start with [docs/HANDOFF.md](docs/HANDOFF.md), then read
[docs/implementation.md](docs/implementation.md) for architecture and
[README.md](README.md) for user-facing behavior and commands.

Current direction: Linux **and macOS**, overlapping PTY windows, mouse/menu
accessibility, a bundled browser/editor, and live retro colour themes. The
original Linux-only, monochrome, terminal-program-only scope was explicitly
changed by the user; do not reinstate those restrictions.

Keep command names, menus and bindings in the shared command catalog, and theme
roles in the shared theme module. Preserve editor unsaved-change protection,
terminal restoration, client-disconnect survival and per-user socket permissions.
Use isolated sessions for testing; do not end the user's running sessions to test
or load a new executable.

Run checks appropriate to the change; the handoff lists the standard checks and
known timing pitfalls. Keep the handoff current when architecture, scope, release
state or validation changes. Published version tags must not be moved or reused.
The backlog is context for future work, not an instruction to implement every item.
