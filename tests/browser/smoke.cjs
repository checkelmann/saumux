/* Real Chromium, real session, isolated runtime/config, no external services. */
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const { once } = require('node:events');
const binary = path.resolve(process.argv[2] || 'target/debug/saumux');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sx-web-'));
const env = { ...process.env, XDG_RUNTIME_DIR: root, XDG_CONFIG_HOME: path.join(root,'config'), SHELL: '/bin/sh' };
// Never inherit a user's active session selection into fixtures.
delete env.SAUMUX; delete env.SAUMUX_WINDOW;
const ctl = (...args) => JSON.parse(execFileSync(binary, ['ctl','--session','browser-test',...args], { env, encoding:'utf8' }) || 'null');
let server, browser;
async function run() {
  server = spawn(binary, ['web','--bind','127.0.0.1','--port','0','--session','browser-test','--','/bin/sh'], { env, stdio:['ignore','pipe','pipe'] });
  let log = '';
  server.stderr.on('data', data => { log += data; });
  const url = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('web startup timeout: '+log)), 10000);
    server.stdout.on('data', data => {
      log += data;
      const match = log.match(/saumux web: (http:\/\/[^ ]+) /);
      if (match) { clearTimeout(timer); resolve(match[1]); }
    });
    server.once('exit', code => { clearTimeout(timer); reject(new Error('web exited '+code+': '+log)); });
  });
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const failures = [];
  page.on('pageerror', e => failures.push(e.message));
  const external = [];
  page.on('request', r => { if (!r.url().startsWith(url) && !r.url().startsWith('data:')) external.push(r.url()); });
  await page.goto(url);
  const hasText = text => page.waitForFunction(text => document.querySelector('.xterm-rows')?.textContent.includes(text), text);
  await hasText('Projects');
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type("printf 'BROWSER_READY\\n'");
  await page.keyboard.press('Enter');
  await hasText('BROWSER_READY');
  assert.equal(ctl('list').length, 1);
  const original = ctl('list')[0];
  // Toolbar opens the actual compositor's theme picker; choose with browser keys.
  await page.getByRole('button', { name:'Themes', exact:true }).click();
  await hasText('Colour themes');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => !document.querySelector('.xterm-rows')?.textContent.includes('Colour themes'));
  assert.equal(ctl('theme.list').find(t=>t.active).id, 'amber');
  // Mouse events use terminal cell coordinates and preserve drag ordering.
  const cell = async (x,y) => {
    const box = await page.locator('.xterm-screen').boundingBox();
    const dimensions = await page.evaluate(() => ({ cols: document.querySelectorAll('.xterm-rows > div')[0]?.textContent.length, rows: document.querySelectorAll('.xterm-rows > div').length }));
    // Width is measured by xterm itself; use a rendered row's cell span geometry.
    const charWidth = await page.locator('.xterm-rows > div').first().locator('span').first().evaluate(e=>e.getBoundingClientRect().width/[...e.textContent].length);
    return { x: box.x + (x+.5)*charWidth, y: box.y+(y+.5)*box.height/dimensions.rows };
  };
  const from = await cell(original.rect.x+10,original.rect.y);
  const to = await cell(original.rect.x+14,original.rect.y+2);
  await page.mouse.move(from.x,from.y); await page.mouse.down();
  await page.mouse.move(to.x,to.y,{steps:5}); await page.mouse.up();
  await page.waitForFunction(() => document.getElementById('status').textContent !== 'Connecting…');
  const movedBy = original.rect.x+4;
  const deadline = Date.now()+5000;
  while (ctl('list')[0].rect.x !== movedBy) {
    assert.ok(Date.now()<deadline, 'mouse drag did not move the window');
    await new Promise(resolve=>setTimeout(resolve,20));
  }
  // Paste through the fallback works on ordinary HTTP without clipboard API permissions.
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable:true }));
  await page.getByRole('button', { name:'Paste', exact:true }).click();
  // Force the plain-HTTP clipboard fallback rather than skipping it.
  await page.locator('#paste-dialog').waitFor({state:'visible'});
  {
    await page.locator('#paste-text').fill("printf 'PASTE_READY\\n'\n");
    await page.getByRole('button', { name:'Send text', exact:true }).click();
    await hasText('PASTE_READY');
  }
  await page.getByRole('button', { name:'Projects', exact:true }).click();
  await hasText('Saved projects'); await page.keyboard.press('Escape');
  await page.getByRole('button', { name:'Attention', exact:true }).click();
  await hasText('Agent attention'); await page.keyboard.press('Escape');
  await page.setViewportSize({width:1000,height:650});
  await hasText('Projects');
  await page.getByRole('button', { name:'Reconnect', exact:true }).click();
  await hasText('BROWSER_READY');
  assert.equal(ctl('list')[0].pid,original.pid);
  // A browser refresh and closing the listener do not end the session.
  await page.reload(); await hasText('BROWSER_READY');
  assert.equal(ctl('list')[0].pid,original.pid);
  const recorder = "import os,tty; tty.setraw(0); print('KEY_RECORDER_READY',flush=True)\nwhile True:\n b=os.read(0,1); print('\\rBYTE_'+b.hex(),flush=True)";
  ctl('window.new','--','python3','-u','-c',recorder);
  await hasText('KEY_RECORDER_READY');
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.press('Shift+Enter');
  await hasText('BYTE_75');
  assert.ok(!(await page.locator('.xterm-rows').textContent()).includes('BYTE_0d'), 'Shift+Enter became submit');
  await page.keyboard.press('Control+j'); await hasText('BYTE_0a');
  await page.keyboard.press('Enter'); await hasText('BYTE_0d');
  if (process.env.SAUMUX_TEST_SCREENSHOT) await page.screenshot({path:process.env.SAUMUX_TEST_SCREENSHOT});
  assert.deepEqual(failures, []);
  assert.deepEqual(external, []);
  console.log('Browser desktop: keyboard, mouse drag, themes, projects/attention, resize, reconnect and offline assets passed.');
}
run().catch(error => { console.error(error); process.exitCode=1; }).finally(async () => {
  if (browser) await browser.close();
  if (server && server.exitCode===null) { server.kill('SIGTERM'); await once(server,'exit'); }
  try { ctl('session.end','--confirm'); } catch {}
  fs.rmSync(root,{recursive:true,force:true});
});
