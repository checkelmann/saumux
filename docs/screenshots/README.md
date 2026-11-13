# README screenshots

These PNGs capture the actual browser frontend and server compositor, using
sample files and notes. The monitor displays real host readings. Matrix, Atari
and Ocean captures change the theme of the same running PTYs.

Regenerate from the repository root:

```sh
cargo build --release --locked
npm ci --prefix tests/browser --ignore-scripts
npm exec --prefix tests/browser -- playwright install chromium
node tests/browser/screenshots.cjs target/release/saumux
```

The script binds an ephemeral port on localhost, isolates runtime/config/data
directories and shuts down only its own session. It uses no provider APIs or
external web assets. The 1280×800 viewport is captured at twice the pixel density
for readable text on high-resolution displays. Clock, hostname and system
readings vary between runs; review the images before committing them.
