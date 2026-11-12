/* The server owns the UI. This client only renders ANSI and forwards cell input. */
(() => {
  "use strict";
  const host = document.getElementById("terminal");
  const status = document.getElementById("status");
  const term = new Terminal({
    cols: 80, rows: 24, fontSize: 15,
    fontFamily: '"Saumux Symbols", "SFMono-Regular", Menlo, Consolas, "Liberation Mono", monospace',
    cursorBlink: false, scrollback: 0, allowProposedApi: false,
    macOptionIsMeta: true, minimumContrastRatio: 1,
    // Match the ordinary ANSI palette used by the TUI's semantic theme roles.
    theme: { foreground: "#aaaaaa", background: "#000000", cursor: "#ffffff",
      black: "#000000", red: "#aa0000", green: "#00aa00", yellow: "#aa5500",
      blue: "#0000aa", magenta: "#aa00aa", cyan: "#00aaaa", white: "#aaaaaa",
      brightBlack: "#555555", brightRed: "#ff5555", brightGreen: "#55ff55", brightYellow: "#ffff55",
      brightBlue: "#5555ff", brightMagenta: "#ff55ff", brightCyan: "#55ffff", brightWhite: "#ffffff" }
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(host);
  // Force-load the private-use symbol fallback even when the active prompt has
  // not drawn one yet, then repaint any glyphs rendered during the font request.
  document.fonts.load('15px "Saumux Symbols"', "\uE0A0\uE0B0\uE0B1")
    .then(() => term.refresh(0, term.rows - 1)).catch(() => {});
  let socket;
  let generation = 0;
  let connected = false;
  let clipboard = "";
  let pressed = null;
  let latestMotion = null;
  let motionFrame = 0;
  let renderQueue = 0;
  const MAX_PASTE = 900000;
  const mods = e => ({ ctrl: e.ctrlKey, alt: e.altKey || e.metaKey, shift: e.shiftKey });
  function note(text) { status.textContent = text; status.title = text; }
  function send(message) {
    if (socket?.readyState === WebSocket.OPEN && connected) {
      if (socket.bufferedAmount > 2 * 1024 * 1024) { note("Connection is slow; reconnect to resume."); socket.close(); return; }
      const payload = JSON.stringify(message);
      if (new TextEncoder().encode(payload).length > 1024 * 1024) { note("Input is too large for one message."); return; }
      socket.send(payload);
    }
  }
  function paste(text) {
    if (new TextEncoder().encode(text).length > MAX_PASTE) { note("Paste is too large (maximum 900 kB)."); return; }
    send({ type: "paste", text });
  }
  function dimensions() {
    const proposed = fit.proposeDimensions();
    if (!proposed) return;
    const cols = Math.max(1, Math.min(500, proposed.cols));
    const rows = Math.max(1, Math.min(200, proposed.rows));
    if (term.cols !== cols || term.rows !== rows) term.resize(cols, rows);
  }
  new ResizeObserver(dimensions).observe(host);
  term.onResize(({ cols, rows }) => send({ type: "resize", cols, rows }));
  function connect() {
    const current = ++generation;
    connected = false;
    pressed = null;
    socket?.close();
    dimensions();
    term.reset();
    renderQueue = 0;
    note("Connecting…");
    const ws = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`);
    socket = ws;
    ws.onopen = () => {
      if (current !== generation) return;
      connected = true;
      ws.send(JSON.stringify({ type: "hello", cols: term.cols, rows: term.rows }));
      note("Connected");
      term.focus();
    };
    ws.onmessage = event => {
      if (current !== generation) return;
      let message;
      try { message = JSON.parse(event.data); } catch { note("Invalid server response"); ws.close(); return; }
      if (typeof message.Render === "string") {
        // Bound xterm's asynchronous write queue even if the tab is suspended.
        renderQueue += message.Render.length;
        if (renderQueue > 4 * 1024 * 1024) { note("Rendering fell behind; reconnect to resume."); ws.close(); return; }
        term.write(message.Render, () => { if (current === generation) renderQueue -= message.Render.length; });
      } else if (message === "Detached") {
        connected = false;
        note("Detached · programs keep running · Reconnect to return");
        ws.close();
      } else if (typeof message.Error === "string") {
        connected = false;
        note(message.Error);
        ws.close();
      }
    };
    ws.onerror = () => { if (current === generation) note("Connection failed · Reconnect to retry"); };
    ws.onclose = () => {
      if (current !== generation) return;
      if (connected) note("Disconnected · Reconnect to return");
      connected = false;
    };
  }
  const namedKeys = new Set(["Enter", "Escape", "Backspace", "Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown", "Insert", "Delete"]);
  term.attachCustomKeyEventHandler(e => {
    if (e.type !== "keydown") return false;
    if (e.isComposing || ["Dead", "Process", "Unidentified"].includes(e.key) || e.getModifierState("AltGraph")) return true;
    // Let the browser dispatch its trusted clipboard events.
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "v") return true;
    if ((e.ctrlKey && e.shiftKey || e.metaKey) && e.key.toLowerCase() === "c") {
      if (!term.hasSelection()) copyClipboard();
      return false;
    }
    if (namedKeys.has(e.key) || /^F([1-9]|1[0-9]|2[0-4])$/.test(e.key) || [...e.key].length === 1) {
      e.preventDefault();
      send({ type: "key", key: e.key, ...mods(e) });
    }
    return false;
  });
  // Composition/AltGr input reaches onData after xterm's input-method handling.
  term.onData(text => paste(text));
  host.addEventListener("paste", e => {
    const text = e.clipboardData?.getData("text/plain");
    if (text !== undefined) { e.preventDefault(); e.stopImmediatePropagation(); paste(text); }
  }, true);
  const screen = host.querySelector(".xterm-screen");
  function cell(e) {
    const r = screen.getBoundingClientRect();
    return { x: Math.max(0, Math.min(term.cols - 1, Math.floor((e.clientX - r.left) / (r.width / term.cols)))),
      y: Math.max(0, Math.min(term.rows - 1, Math.floor((e.clientY - r.top) / (r.height / term.rows)))) };
  }
  function mouse(e, kind, button = 0) { send({ type: "mouse", kind, button, ...cell(e), ...mods(e) }); }
  screen.addEventListener("pointerdown", e => {
    if (e.shiftKey || e.button > 2 || e.pointerType !== "mouse") return;
    e.preventDefault();
    term.focus();
    pressed = e.button;
    screen.setPointerCapture(e.pointerId);
    mouse(e, "down", pressed);
  }, true);
  screen.addEventListener("pointermove", e => {
    if (e.shiftKey || e.pointerType !== "mouse") return;
    latestMotion = { event: e, kind: pressed === null ? "move" : "drag", button: pressed ?? 0 };
    if (!motionFrame) motionFrame = requestAnimationFrame(flushMotion);
  });
  function flushMotion() {
    motionFrame = 0;
    if (latestMotion) { const m = latestMotion; latestMotion = null; mouse(m.event, m.kind, m.button); }
  }
  function release(e) {
    if (pressed === null) return;
    flushMotion();
    mouse(e, "up", pressed);
    pressed = null;
    if (screen.hasPointerCapture(e.pointerId)) screen.releasePointerCapture(e.pointerId);
  }
  screen.addEventListener("pointerup", release);
  screen.addEventListener("pointercancel", release);
  screen.addEventListener("contextmenu", e => { if (!e.shiftKey) e.preventDefault(); });
  screen.addEventListener("wheel", e => {
    if (e.shiftKey) return;
    e.preventDefault();
    const lines = Math.min(8, Math.max(1, Math.ceil(Math.abs(e.deltaY) / (e.deltaMode ? 1 : 40))));
    for (let i = 0; i < lines; i++) mouse(e, e.deltaY < 0 ? "wheel_up" : "wheel_down");
  }, { passive: false });
  window.addEventListener("blur", () => { pressed = null; latestMotion = null; send({ type: "focus", focused: false }); });
  window.addEventListener("focus", () => send({ type: "focus", focused: true }));
  // OSC 52 copies are local to the browser. Never expose clipboard reads to programs.
  term.parser.registerOscHandler(52, data => {
    const index = data.indexOf(";");
    if (index < 0 || data.length > 110000 || data.slice(index + 1) === "?") return true;
    try {
      const binary = atob(data.slice(index + 1));
      clipboard = new TextDecoder().decode(Uint8Array.from(binary, c => c.charCodeAt(0)));
      note("Selection ready · click Copy to copy to this computer");
    } catch { note("Could not decode copied selection"); }
    return true;
  });
  async function copyClipboard() {
    const text = term.hasSelection() ? term.getSelection() : clipboard;
    if (!text) { note("Select text in a window first (or use Alt+Space)"); return; }
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
      else {
        const field = document.createElement("textarea");
        field.value = text; document.body.append(field); field.select();
        const copied = document.execCommand("copy"); field.remove();
        if (!copied) throw new Error("Copy was blocked by the browser");
      }
      note("Copied to this computer");
    } catch { note("Clipboard blocked · use Shift+drag and your browser’s Copy command"); }
    term.focus();
  }
  document.getElementById("copy").onclick = copyClipboard;
  document.getElementById("paste").onclick = async () => {
    try {
      if (!navigator.clipboard?.readText) throw new Error("Clipboard API unavailable");
      paste(await navigator.clipboard.readText()); term.focus();
    } catch {
      document.getElementById("paste-text").value = "";
      document.getElementById("paste-dialog").returnValue = "";
      document.getElementById("paste-dialog").showModal();
      document.getElementById("paste-text").focus();
    }
  };
  document.getElementById("paste-dialog").addEventListener("close", e => {
    if (e.target.returnValue === "send") paste(document.getElementById("paste-text").value);
    term.focus();
  });
  document.getElementById("reconnect").onclick = connect;
  document.querySelectorAll("[data-key]").forEach(button => {
    button.onclick = () => { send({ type: "key", key: button.dataset.key, alt: button.hasAttribute("data-alt") }); term.focus(); };
  });
  window.addEventListener("pagehide", () => socket?.close());
  connect();
})();
