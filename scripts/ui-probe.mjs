// CDP probe: inspects the real UI inside WebView2.
//   node scripts/ui-probe.mjs [js-expression]
//   node scripts/ui-probe.mjs --type "text\r"
// Requires the app launched with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
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
const logs = [];

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
    return;
  }
  if (m.method === "Runtime.consoleAPICalled") {
    logs.push(`[${m.params.type}] ${m.params.args.map((a) => a.value ?? a.description ?? "").join(" ")}`);
  }
  if (m.method === "Runtime.exceptionThrown") {
    logs.push(
      `[error] ${m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text}`,
    );
  }
});

await new Promise((res, rej) => {
  ws.addEventListener("open", res);
  ws.addEventListener("error", rej);
});

await send("Runtime.enable");
await send("Log.enable");

// Typing mode: `node scripts/ui-probe.mjs --type "text\r"` types into the window.
if (process.argv[2] === "--type") {
  const text = process.argv[3] ?? "";
  for (const ch of text) {
    if (ch === "\r" || ch === "\n") {
      await send("Input.dispatchKeyEvent", {
        type: "keyDown",
        key: "Enter",
        code: "Enter",
        windowsVirtualKeyCode: 13,
        text: "\r",
      });
      await send("Input.dispatchKeyEvent", {
        type: "keyUp",
        key: "Enter",
        code: "Enter",
        windowsVirtualKeyCode: 13,
      });
    } else {
      await send("Input.dispatchKeyEvent", { type: "char", text: ch });
    }
  }
  console.log(`tecleado: ${JSON.stringify(text)}`);
  ws.close();
  process.exit(0);
}

const expression = process.argv[2] ?? "document.body.innerText";
const r = await send("Runtime.evaluate", {
  expression,
  returnByValue: true,
  awaitPromise: true,
});

if (r.exceptionDetails) {
  console.log(
    "EXCEPCIÓN:",
    JSON.stringify(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text),
  );
} else {
  const v = r.result?.value;
  console.log(typeof v === "string" ? v : JSON.stringify(v, null, 2));
}

if (logs.length) {
  console.log("\n--- consola ---");
  console.log(logs.join("\n"));
}
ws.close();
