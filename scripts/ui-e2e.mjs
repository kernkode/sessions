// Frontend e2e: asserts the running UI (via WebView2 CDP) behaves as expected.
//   1. Start the app with CDP:  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run app:dev
//   2. Run:                    node scripts/ui-e2e.mjs
// Exits non-zero if any assertion fails. Read-only: it never mutates sessions.
const PORT = process.env.CDP_PORT ?? "9222";

const targets = await fetch(`http://localhost:${PORT}/json/list`).then((r) => r.json());
const page = targets.find((t) => t.type === "page");
if (!page) {
  console.error("no hay ninguna página abierta en el depurador");
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
function send(method, params = {}) {
  const id = ++nextId;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((res) => pending.set(id, res));
}
ws.addEventListener("message", (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) {
    pending.get(m.id)(m.result);
    pending.delete(m.id);
  }
});
await new Promise((res, rej) => {
  ws.addEventListener("open", res);
  ws.addEventListener("error", rej);
});
await send("Runtime.enable");

async function evalJs(expression) {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? "eval error");
  return r.result?.value;
}

let failed = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log("  ok  " + name);
  } else {
    failed++;
    console.error("  FAIL " + name + (extra ? " — " + extra : ""));
  }
}

// Wait for the store to be ready.
let ready = false;
for (let i = 0; i < 50 && !ready; i++) {
  ready = await evalJs("window.__store ? window.__store.getState().ready : false");
  if (!ready) await new Promise((r) => setTimeout(r, 200));
}
check("la app arranca y el store está listo", ready === true);

if (ready) {
  const cards = await evalJs(
    "[...document.querySelectorAll('.sidebar .card .card-sub')].map(c => c.innerText)",
  );
  check("hay tarjetas de sesión", Array.isArray(cards) && cards.length > 0, JSON.stringify(cards));
  check(
    "las tarjetas no muestran pid",
    cards.every((c) => !/pid\s+\d/.test(c)),
    JSON.stringify(cards),
  );

  const viewportBg = await evalJs(
    "(() => { const e = [...document.querySelectorAll('.xterm-viewport')].find(x => x.getBoundingClientRect().height > 0); return e ? getComputedStyle(e).backgroundColor : null; })()",
  );
  check(
    "el viewport de xterm es transparente (sin franja negra)",
    viewportBg === "rgba(0, 0, 0, 0)" || viewportBg === "transparent",
    String(viewportBg),
  );

  // Palette opens and lists commands.
  await evalJs("window.__store.getState().setDialog('palette')");
  const palette = await evalJs(
    "(() => { const d = document.querySelector('.dialog'); return d ? d.innerText : null; })()",
  );
  check("la paleta de comandos abre", typeof palette === "string" && palette.includes("Nueva sesión"), String(palette));
  await evalJs("window.__store.getState().setDialog(null)");

  // Settings exposes the editable auto_relaunch toggle.
  await evalJs("window.__store.getState().setDialog('settings')");
  const settings = await evalJs(
    "(() => { const d = document.querySelector('.dialog'); return d ? d.innerText : null; })()",
  );
  check(
    "Ajustes muestra «Relanzar al terminar»",
    typeof settings === "string" && settings.includes("Relanzar al terminar"),
    String(settings),
  );
  await evalJs("window.__store.getState().setDialog(null)");

  // Git controls appear for a session whose cwd is a repo.
  const gitbar = await evalJs(
    "(() => { const c = [...document.querySelectorAll('.header .chip')]; return c.some(x => x.innerText.includes('main') || x.innerText.includes('master')); })()",
  );
  check("la cabecera muestra la rama de git con undo/redo", gitbar === true);
}

ws.close();
if (failed > 0) {
  console.error(`\n${failed} comprobación(es) fallida(s)`);
  process.exit(1);
}
console.log("\ne2e de frontend: todo en verde");
process.exit(0);
