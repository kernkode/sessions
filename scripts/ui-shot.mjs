// Captures an image of the real window:
//   node scripts/ui-shot.mjs output.png [x y width height scale]
import { writeFileSync } from "node:fs";

const PORT = process.env.CDP_PORT ?? "9222";
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

await new Promise((res) => ws.addEventListener("open", res));

const [, , x, y, w, h, scale] = process.argv;
const params = { format: "png", fromSurface: true };
if (w && h) {
  params.clip = {
    x: Number(x),
    y: Number(y),
    width: Number(w),
    height: Number(h),
    scale: Number(scale ?? 1),
  };
}
const r = await send("Page.captureScreenshot", params);
const out = process.argv[2] ?? "captura.png";
writeFileSync(out, Buffer.from(r.data, "base64"));
console.log(`imagen escrita en ${out}`);
ws.close();
