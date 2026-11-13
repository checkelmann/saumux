/* README captures: real compositor/apps, sample files, isolated session. */
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('node:child_process');
const { once } = require('node:events');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const binary = path.resolve(process.argv[2] || 'target/release/saumux');
const output = path.resolve(__dirname, '../../docs/screenshots');
// Both supported platforms provide /tmp; keep visible demo paths short on macOS.
const root = fs.mkdtempSync('/tmp/saumux-demo-');
const project = path.join(root, 'demo-project');
const env = { ...process.env, XDG_RUNTIME_DIR: root,
  XDG_CONFIG_HOME: path.join(root, 'config'), XDG_DATA_HOME: path.join(root, 'data'),
  SHELL: '/bin/sh', PS1: 'demo$ ' };
delete env.SAUMUX; delete env.SAUMUX_WINDOW;
const session = 'readme';
const ctl = (...args) => JSON.parse(execFileSync(binary, ['ctl', '--session', session, ...args],
  { env, encoding: 'utf8', timeout: 10000 }) || 'null');
let listener, browser;

async function run() {
  fs.mkdirSync(output, { recursive: true });
  fs.mkdirSync(path.join(project, 'src'), { recursive: true });
  fs.mkdirSync(path.join(project, 'docs'));
  fs.mkdirSync(path.join(project, 'archive'));
  fs.mkdirSync(path.join(root, 'data/saumux'), { recursive: true, mode: 0o700 });
  fs.writeFileSync(path.join(project, 'README.md'), `# A workspace that stays with you

Open a shell. Browse your files. Keep a notebook.
Everything lives in its own movable PTY window.

## Desktop essentials

- Drag a title bar to move a window.
- Minimize to the shelf; restore with a click.
- Switch between four workspaces.
- Detach and return without stopping programs.

## Make it yours

Choose a live colour theme from the View menu.
The desktop and bundled apps update together.

Save this file with Ctrl+S.
Find text with Ctrl+F. Undo with Ctrl+Z.
`);
  fs.writeFileSync(path.join(project, 'config.toml'), '[look]\ntheme = "borland"\nshadow = true\n');
  fs.writeFileSync(path.join(project, 'src/main.rs'), 'fn main() {\n    println!("Hello from saumux!");\n}\n');
  fs.writeFileSync(path.join(project, 'docs/workflow.md'), '# Workflow\n\nBuild, review, detach, return.\n');
  fs.writeFileSync(path.join(root, 'data/saumux/Desktop field guide.md'), `# Desktop field guide

Your terminal, arranged your way. **One session**, many windows.

## A place for everything

- **Shells** for the work in progress
- **Files + editor** for quick changes
- **Notes** for ideas worth keeping

> Disconnect when you need to. Your programs keep running.

## Today's checklist

- [x] Open the project workspace
- [x] Pick a favourite colour theme
- [ ] Write down the next idea

## A few useful keys

| Action | Shortcut |
| --- | --- |
| New shell | Alt+n |
| Colour themes | Alt+t |
| Next window | F6 |
| Detach | Alt+d |

Notes are ordinary Markdown files. Press **Enter** to edit.
`);
  fs.writeFileSync(path.join(root, 'data/saumux/Ideas.md'), '# Ideas\n\n- Keep tools close at hand.\n- Make time to explore.\n');
  fs.writeFileSync(path.join(root, 'data/saumux/Project journal.md'), '# Project journal\n\nA fresh workspace, ready for the next task.\n');

  const window = (command, x, y, w, h, workspace = 0) => ({ command, rect: { x, y, w, h },
    cwd: project, workspace, shelved: false, color: true });
  const layout = path.join(root, 'layout.json');
  fs.writeFileSync(layout, JSON.stringify({ windows: [
    window([binary, '__monitor'], 2, 2, 80, 29),
    window(['/bin/sh', '-c', "printf '\033]0;Project shell\007'; printf 'demo$ ls\\n'; ls; printf '\\ndemo$ cat config.toml\\n'; cat config.toml; exec /bin/sh"], 65, 24, 73, 14),
    window([binary, '__clock'], 85, 3, 52, 15),
    window([binary, '__files', '.'], 2, 2, 133, 35, 1),
    window([binary, '__edit', 'README.md'], 46, 13, 91, 26, 1),
    window([binary, '__notes'], 3, 2, 134, 36, 2),
  ] }));

  listener = spawn(binary, ['web', '--bind', '127.0.0.1', '--port', '0', '--session', session,
    '--layout', layout], { env, cwd: project, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  const url = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Startup timeout: ' + log)), 10000);
    listener.stderr.on('data', data => { log += data; });
    listener.stdout.on('data', data => {
      log += data;
      const match = log.match(/saumux web: (http:\/\/[^ ]+) /);
      if (match) { clearTimeout(timer); resolve(match[1]); }
    });
    listener.once('exit', code => { clearTimeout(timer); reject(new Error(`Listener exited ${code}: ${log}`)); });
  });
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => {
    if (!request.url().startsWith(url) && !request.url().startsWith('data:')) errors.push(request.url());
  });
  const hasText = async text => {
    try {
      await page.waitForFunction(text => document.querySelector('.xterm-rows')?.textContent.includes(text), text);
    } catch (error) {
      console.error('Waiting for:', text, '\n', await page.locator('.xterm-rows').innerText());
      throw error;
    }
  };
  await page.goto(url);
  await hasText('Projects');
  await page.evaluate(() => document.fonts.ready);
  ctl('window.focus', '2');
  await hasText('CPU history');
  console.log('Desktop ready; collecting monitor history');
  // Collect real monitor history; do not synthesize CPU readings.
  await page.waitForFunction(() => /sample (?:[8-9]|[1-9][0-9]+)/.test(document.querySelector('.xterm-rows')?.textContent));
  const capture = async name => {
    await page.mouse.move(1, 1);
    // Allow the theme sidecar's 100 ms polling and terminal paint to settle.
    await page.waitForTimeout(400);
    await page.screenshot({ path: path.join(output, name + '.png') });
    console.log('Captured ' + name);
  };
  await hasText('demo$');
  await hasText('Local time (host)');
  await capture('desktop');
  ctl('workspace.next');
  ctl('window.focus', '5');
  await hasText('A workspace that stays with you');
  await hasText('Rename');
  await capture('files-editor');
  ctl('workspace.next');
  ctl('window.focus', '6');
  await hasText("Today's checklist");
  await hasText('Project journal');
  await capture('notes');
  ctl('workspace.previous');
  ctl('workspace.previous');
  ctl('window.focus', '2');
  await hasText('CPU history');
  // Verify live switching retains every PTY process.
  const pids = ctl('list').map(w => w.pid);
  for (const theme of ['matrix', 'atari', 'ocean']) {
    ctl('theme.set', theme);
    await hasText(ctl('theme.list').find(t => t.active).label + ' theme');
    assert.deepEqual(ctl('list').map(w => w.pid), pids);
    await capture('theme-' + theme);
  }
  assert.deepEqual(errors, []);
}

run().catch(error => { console.error(error); process.exitCode = 1; }).finally(async () => {
  if (browser) await browser.close();
  if (listener && listener.exitCode === null) {
    listener.kill('SIGTERM');
    await once(listener, 'exit');
  }
  if (fs.existsSync(path.join(root, 'saumux', session + '.sock'))) {
    try { ctl('session.end', '--confirm'); } catch (error) { console.error(error.message); process.exitCode = 1; }
  }
  fs.rmSync(root, { recursive: true, force: true });
});
