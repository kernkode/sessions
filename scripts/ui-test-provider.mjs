// UI test: edits a provider from the editor and saves it.
//   node scripts/ui-test-provider.mjs <id> <timeout-ms>
const PORT = process.env.CDP_PORT ?? "9222";
const ID = process.argv[2] ?? "gorouter";
const TIMEOUT = process.argv[3] ?? "300000";

const [page] = await fetch(`http://localhost:${PORT}/json/list`).then((r) => r.json());
const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
const send = (method, params = {}) => {
  const id = ++nextId;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((res) => pending.set(id, res));
};
ws.addEventListener("message", (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) {
    pending.get(m.id)(m.result);
    pending.delete(m.id);
  }
});
await new Promise((r) => ws.addEventListener("open", r));
await send("Runtime.enable");

const evaluate = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.exceptionDetails)
    throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result?.value;
};

const script = `(async () => {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const setValue = (el, v) => {
    const d = Object.getOwnPropertyDescriptor(el.constructor.prototype, 'value').set;
    d.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const s = window.__store.getState();
  s.setDialog('settings');
  await wait(350);
  [...document.querySelectorAll('.tab')].find((x) => x.textContent === 'Proveedores').click();
  await wait(350);
  const card = [...document.querySelectorAll('.card')].find((c) => c.textContent.includes(${JSON.stringify(ID)}));
  if (!card) return { error: 'no encontré la tarjeta del proveedor' };
  card.querySelector('.icon-btn').click();
  await wait(450);
  const title = document.querySelector('.dialog-head').textContent.trim();
  // The advanced tab holds the timeout field.
  [...document.querySelectorAll('.dialog .tab')].find((x) => x.textContent.trim() === 'Avanzado').click();
  await wait(400);
  const inputs = [...document.querySelectorAll('.dialog input')];
  const field = inputs.find((i) => i.placeholder === '600000');
  if (!field) return { error: 'no encontré el campo de timeout' };
  setValue(field, ${JSON.stringify(TIMEOUT)});
  await wait(250);
  const save = [...document.querySelectorAll('.dialog-foot .btn')].find((b) => b.textContent.includes('Guardar'));
  save.click();
  await wait(1200);
  const st = window.__store.getState();
  const p = st.config.providers.find((x) => x.id === ${JSON.stringify(ID)});
  return {
    title,
    notice: st.notice,
    timeout_in_state: p?.timeout_ms ?? null,
    editor_open: Boolean(document.querySelector('.dialog-head')?.textContent.includes('Editar')),
  };
})()`;

console.log(JSON.stringify(await evaluate(script), null, 1));
ws.close();
