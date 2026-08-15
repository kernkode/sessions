// Reloads the UI capturing errors from before the app code runs.
const PORT = process.env.CDP_PORT ?? "9222";
const [page] = await fetch(`http://localhost:${PORT}/json/list`).then((r) => r.json());
const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
const events = [];

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
  } else if (m.method === "Runtime.exceptionThrown") {
    const d = m.params.exceptionDetails;
    events.push(`EXC ${d.text} :: ${d.exception?.description ?? JSON.stringify(d.exception?.value)}`);
  } else if (m.method === "Runtime.consoleAPICalled" && m.params.type === "error") {
    events.push(`CONSOLE ${m.params.args.map((a) => a.description ?? a.value).join(" ")}`);
  }
});

await new Promise((r) => ws.addEventListener("open", r));
await send("Runtime.enable");
await send("Page.enable");
await send("Page.addScriptToEvaluateOnNewDocument", {
  source: `window.__errs=[];
    addEventListener('unhandledrejection',e=>window.__errs.push('REJ '+(e.reason&&(e.reason.stack||e.reason.message||String(e.reason)))));
    addEventListener('error',e=>window.__errs.push('ERR '+e.message));`,
});
await send("Page.reload", { ignoreCache: false });
await new Promise((r) => setTimeout(r, 6000));
const r = await send("Runtime.evaluate", {
  expression: "JSON.stringify(window.__errs ?? null)",
  returnByValue: true,
});
console.log("errores capturados:", r.result?.value);
console.log(events.length ? events.join("\n") : "(sin eventos de excepción)");
ws.close();
