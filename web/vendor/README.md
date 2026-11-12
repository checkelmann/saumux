# Bundled browser dependencies

These upstream MIT-licensed distribution files are embedded in the executable.
The browser makes no CDN requests; Node/npm are not runtime or Rust-build dependencies.

- `@xterm/xterm` **6.0.0**: `lib/xterm.js`, `css/xterm.css`, `LICENSE`.
- `@xterm/addon-fit` **0.11.0**: `lib/addon-fit.js`, `LICENSE`.
- Nerd Fonts Symbols **3.5.1**: `SymbolsNerdFontMono-Regular.ttf`,
  `SymbolsNerdFont.LICENSE`.
  The web client uses it as a fallback for Powerline/private-use prompt symbols.

Upstream: https://github.com/xtermjs/xterm.js

To refresh, use `npm pack --ignore-scripts @xterm/xterm@<version>` and
`npm pack --ignore-scripts @xterm/addon-fit@<version>` in a temporary directory,
then copy only the listed files and retain their licenses. The Nerd Fonts release
asset is `NerdFontsSymbolsOnly.zip`; keep its symbol font version in sync with
`THIRD_PARTY_NOTICES.md`. Check versions, browser checks and notices before
committing an update.
Source maps are intentionally omitted.
