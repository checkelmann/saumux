# Bundled browser dependencies

These upstream MIT-licensed distribution files are embedded in the executable.
The browser makes no CDN requests; Node/npm are not runtime or Rust-build dependencies.

- `@xterm/xterm` **6.0.0**: `lib/xterm.js`, `css/xterm.css`, `LICENSE`.
- `@xterm/addon-fit` **0.11.0**: `lib/addon-fit.js`, `LICENSE`.

Upstream: https://github.com/xtermjs/xterm.js

To refresh, use `npm pack --ignore-scripts @xterm/xterm@<version>` and
`npm pack --ignore-scripts @xterm/addon-fit@<version>` in a temporary directory,
then copy only the listed files and retain their licenses. Check both versions'
compatibility, browser tests and `THIRD_PARTY_NOTICES.md` before committing an update.
Source maps are intentionally omitted.
