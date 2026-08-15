// Inspects a scrollback file: which control sequences it contains.
//   node scripts/inspect-scrollback.mjs <file.bin>
import { readFileSync } from "node:fs";

const bytes = readFileSync(process.argv[2]);
const text = bytes.toString("latin1");
const ESC = "\u001b";
const re = new RegExp(`${ESC}\\[[?0-9;]*[A-Za-z]|${ESC}\\][^\\u0007]*\\u0007|${ESC}[=>78]`, "g");
const counts = new Map();
for (const m of text.matchAll(re)) {
  const key = m[0].replaceAll(ESC, "ESC").replace(/[0-9]+/g, "N");
  counts.set(key, (counts.get(key) ?? 0) + 1);
}
console.log("bytes:", bytes.length);
console.log("secuencias:");
[...counts.entries()]
  .sort((a, b) => b[1] - a[1])
  .slice(0, 16)
  .forEach(([k, v]) => console.log(`  ${String(v).padStart(4)}x  ${JSON.stringify(k)}`));
console.log("--- primeros 240 bytes ---");
console.log(JSON.stringify(text.slice(0, 240)));
console.log("--- últimos 160 bytes ---");
console.log(JSON.stringify(text.slice(-160)));
