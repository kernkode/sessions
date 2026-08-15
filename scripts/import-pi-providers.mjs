// Imports the providers configured in pi (~/.pi/agent) into ~/.sessions/providers.toml
//
//   node scripts/import-pi-providers.mjs [--dry] [--incluir-openai] [--pi <dir>] [--salida <toml>]
//
// Reads models.json (custom providers), models-store.json (downloaded catalogues)
// and auth.json (to know which providers have a key). Keys are NOT copied: they
// are referenced with api_key_file + api_key_json_path, which the app reads when
// the session is launched.
//
// Idempotent: it replaces, by id, the blocks written by a previous import and
// leaves the rest of the file untouched.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

const argv = process.argv.slice(2);
const flag = (n) => argv.includes(n);
const option = (n, fallback) => {
  const i = argv.indexOf(n);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : fallback;
};

const PI = resolve(option("--pi", join(homedir(), ".pi", "agent")));
const OUT = resolve(
  option("--salida", join(process.env.SESSIONS_HOME ?? join(homedir(), ".sessions"), "providers.toml")),
);
const DRY = flag("--dry");
const INCLUDE_OPENAI = flag("--incluir-openai");

const readJson = (name) => {
  const p = join(PI, name);
  if (!existsSync(p)) return null;
  try {
    return JSON.parse(readFileSync(p, "utf8"));
  } catch (e) {
    console.warn(`aviso: ${name} no se pudo leer (${e.message})`);
    return null;
  }
};

const models = readJson("models.json");
const store = readJson("models-store.json");
const auth = readJson("auth.json") ?? {};
if (!models && !store) {
  console.error(`No encontré configuración de pi en ${PI}`);
  process.exit(1);
}

/** Gathers the providers from both files into a common shape. */
function collect() {
  const out = new Map();

  for (const [id, p] of Object.entries(models?.providers ?? {})) {
    out.set(id, { id, baseUrl: p.baseUrl, api: p.api, models: p.models ?? [] });
  }
  // In models-store.json each model carries its own baseUrl and api.
  for (const [id, p] of Object.entries(store ?? {})) {
    const list = p.models ?? [];
    if (list.length === 0) continue;
    const previous = out.get(id);
    const base = previous?.baseUrl ?? list.find((m) => m.baseUrl)?.baseUrl;
    const api = previous?.api ?? list.find((m) => m.api)?.api;
    out.set(id, { id, baseUrl: base, api, models: [...(previous?.models ?? []), ...list] });
  }
  return [...out.values()].filter((p) => p.baseUrl && p.models.length > 0);
}

const KIND = {
  "anthropic-messages": "anthropic",
  "openai-completions": "openai-chat",
  "openai-responses": "openai-responses",
};

const str = (v) => JSON.stringify(String(v)); // basic TOML string
const num = (v) => (Number.isInteger(v) ? String(v) : String(Number(v.toFixed(4))));
const envVar = (id) => `${id.toUpperCase().replace(/[^A-Z0-9]/g, "_")}_API_KEY`;

/** Model chosen by default: the one pi has configured, or the first reasonable one. */
function defaultModel(list) {
  const preferred = readJson("settings.json")?.defaultModel;
  if (preferred && list.some((m) => m.id === preferred)) return preferred;
  return list[0]?.id;
}

/** TOML block for a provider. The environment mapping is derived by the app from
 *  `kind`, so only the essentials are written here. */
function toToml(p, finalId) {
  const kind = KIND[p.api] ?? "custom";
  const isAnthropic = kind === "anthropic";
  const hasKey = Boolean(auth[p.id]);
  // Claude Code speaks the Anthropic protocol; Codex speaks OpenAI's.
  const agents = isAnthropic ? ["claude"] : ["codex"];

  const L = [];
  L.push(`[[provider]]`);
  L.push(`id = ${str(finalId)}`);
  L.push(`name = ${str(`${p.id} (pi)`)}`);
  L.push(`kind = ${str(kind)}`);
  L.push(`base_url = ${str(p.baseUrl)}`);
  if (hasKey) {
    // The key is read from pi when launching: it is not duplicated on disk.
    L.push(`api_key_file = ${str(join(PI, "auth.json").replace(/\\/g, "/"))}`);
    L.push(`api_key_json_path = ${str(`${p.id}.key`)}`);
  } else {
    L.push(`api_key_env = ${str(envVar(finalId))}`);
  }
  const preferred = defaultModel(p.models);
  if (preferred) L.push(`default_model = ${str(preferred)}`);
  L.push(`agents = [${agents.map(str).join(", ")}]`);
  L.push(`notes = ${str(`Importado de ${join(PI, "models.json").replace(/\\/g, "/")}`)}`);
  L.push("");

  const seen = new Set();
  for (const m of p.models) {
    if (!m.id || seen.has(m.id)) continue;
    seen.add(m.id);
    L.push(`[[provider.model]]`);
    L.push(`id = ${str(m.id)}`);
    if (m.name) L.push(`name = ${str(m.name)}`);
    if (m.contextWindow) L.push(`context_window = ${num(m.contextWindow)}`);
    if (m.maxTokens) L.push(`max_output_tokens = ${num(m.maxTokens)}`);
    if (m.reasoning) L.push(`reasoning = true`);
    if (Array.isArray(m.input) && m.input.includes("image")) L.push(`vision = true`);
    const c = m.cost;
    if (c && (c.input || c.output || c.cacheRead || c.cacheWrite)) {
      L.push(`[provider.model.pricing]`);
      L.push(`input = ${num(c.input ?? 0)}`);
      L.push(`output = ${num(c.output ?? 0)}`);
      if (c.cacheRead) L.push(`cache_read = ${num(c.cacheRead)}`);
      if (c.cacheWrite) L.push(`cache_write = ${num(c.cacheWrite)}`);
    }
    L.push("");
  }
  return L.join("\n");
}

const previous = existsSync(OUT) ? readFileSync(OUT, "utf8") : "";

/**
 * Splits the file into a preamble plus top-level `[[provider]]` blocks. Comment
 * markers are not used: editing a provider from the app may rewrite its block and
 * take the preceding comment with it.
 */
function split(text) {
  const lines = text.split(/\r?\n/);
  const blocks = [];
  const preamble = [];
  let current = null;
  for (const l of lines) {
    if (/^\s*\[\[provider\]\]\s*$/.test(l)) {
      if (current) blocks.push(current);
      current = { lines: [l], id: null };
      continue;
    }
    if (current) {
      const m = /^\s*id\s*=\s*"([^"]+)"/.exec(l);
      if (m && current.id === null) current.id = m[1];
      current.lines.push(l);
    } else {
      preamble.push(l);
    }
  }
  if (current) blocks.push(current);
  return { preamble: preamble.join("\n"), blocks };
}

const { preamble, blocks: existing } = split(previous);
const providers = collect();

// Ids we are about to (re)write: pi's plus their suffixed variants.
const toReplace = new Set();
for (const p of providers) {
  if (p.id === "openai" && !INCLUDE_OPENAI) continue;
  toReplace.add(p.id);
  toReplace.add(`${p.id}-pi`);
}

// Foreign blocks are kept, in order and with their comments.
const kept = existing.filter((b) => !b.id || !toReplace.has(b.id));
const foreignIds = new Set(kept.map((b) => b.id).filter(Boolean));

const generated = [];
const summary = [];

for (const p of providers) {
  if (p.id === "openai" && !INCLUDE_OPENAI) {
    summary.push(`  · ${p.id}: omitido (ya existe un bloque «openai» de fábrica; usa --incluir-openai)`);
    continue;
  }
  // If the id clashes with a provider that is not ours, rename it.
  const finalId = foreignIds.has(p.id) ? `${p.id}-pi` : p.id;
  generated.push(toToml(p, finalId));
  const count = new Set(p.models.map((m) => m.id)).size;
  summary.push(
    `  · ${finalId}: ${KIND[p.api] ?? "custom"} · ${count} modelo(s) · ${p.baseUrl}` +
      (auth[p.id] ? " · clave leída de pi" : " · sin clave en pi (usa api_key_env)"),
  );
}

if (generated.length === 0) {
  console.log("No hay nada que importar.");
  process.exit(0);
}

const header = [
  "# ─── proveedores importados de ~/.pi/agent ───",
  "# Reimportar con: npm run import:pi   (reemplaza estos bloques por id)",
  "",
].join("\n");

const keptBody = kept.map((b) => b.lines.join("\n").replace(/\s+$/, "")).join("\n\n");
const output = [preamble.replace(/\s+$/, ""), keptBody, header + generated.join("\n")]
  .filter((t) => t.trim() !== "")
  .join("\n\n")
  .replace(/\n{4,}/g, "\n\n\n")
  .concat("\n");

console.log(`pi:      ${PI}`);
console.log(`destino: ${OUT}`);
console.log(`proveedores:`);
console.log(summary.join("\n"));
const replaced = existing.length - kept.length;
if (replaced > 0) console.log(`  (se reemplazaron ${replaced} bloque(s) de una importación anterior)`);

if (DRY) {
  console.log(`\n--- bloques generados (${generated.length}, no escrito por --dry) ---\n`);
  console.log(header + generated.join("\n"));
} else {
  writeFileSync(OUT, output, "utf8");
  console.log(`\nEscrito. Aplica los cambios en la app con Ctrl+Shift+R.`);
}
