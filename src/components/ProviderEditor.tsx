import { useEffect, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api } from "../lib/ipc";
import { SCOPE_ALL, type KeySource } from "../lib/providers";
import {
  emptyModel,
  emptyProvider,
  type Provider,
  type ProviderCheck,
  type ProviderModel,
} from "../lib/types";
import { useStore } from "../state/store";
import { IconEye, IconFolder, IconKey, IconPlus, IconTrash, IconX } from "./Icons";

type Tab = "basic" | "models" | "advanced";

const KEY_OPTIONS: { value: KeySource; label: string; help: string }[] = [
  {
    value: "literal",
    label: "Escribirla aquí",
    help: "Se guarda tal cual en providers.toml, sin cifrar.",
  },
  { value: "env", label: "Variable del entorno", help: "La app la lee al lanzar la sesión." },
  {
    value: "file",
    label: "Fichero (JSON o texto)",
    help: "Útil si otra herramienta ya guarda tus claves, p. ej. ~/.pi/agent/auth.json",
  },
  { value: "command", label: "Comando", help: "Para gestores de secretos: op, pass, gopass…" },
  { value: "none", label: "Sin clave", help: "Endpoints locales que no piden autenticación." },
];

/** What each kind of API means, in plain language. */
const KIND_HELP: Record<string, string> = {
  anthropic: "Protocolo de Anthropic (/v1/messages). Compatible con Claude Code.",
  "openai-chat":
    "OpenAI /chat/completions. Compatible con Codex. Es el caso de la mayoría de gateways.",
  "openai-responses": "OpenAI /responses (el de la propia OpenAI). Compatible con Codex y OpenCode.",
  google: "API de Google Generative AI. Compatible con Gemini CLI y OpenCode.",
  ollama: "Servidor local de Ollama, compatible con OpenAI. Se usa con Codex.",
  bedrock: "AWS Bedrock. Necesitarás declarar el entorno a mano en Avanzado.",
  vertex: "Google Vertex AI. Necesitarás declarar el entorno a mano en Avanzado.",
  custom: "Sin plantilla: define tú las variables en la pestaña Avanzado.",
};

const SOURCE_LABEL: Record<string, string> = {
  literal: "escrita en el editor",
  env: "variable del entorno",
  file: "fichero",
  command: "comando",
  none: "sin definir",
};

/** Where an existing provider's key comes from. For a new one we offer typing it
 *  in directly, which is the most common case. */
function initialKeySource(p: Provider | null): KeySource {
  if (!p) return "literal";
  if (p.api_key) return "literal";
  if (p.api_key_file) return "file";
  if (p.api_key_command) return "command";
  if (p.api_key_env) return "env";
  return "literal";
}

/** Create and edit a provider; writes to providers.toml through the backend. */
export function ProviderEditor({
  initial,
  onClose,
}: {
  initial: Provider | null;
  onClose: () => void;
}) {
  const saveProvider = useStore((s) => s.saveProvider);
  const deleteProvider = useStore((s) => s.deleteProvider);
  const agents = useStore((s) => s.agents);
  const [kinds, setKinds] = useState<string[]>([]);
  const [p, setP] = useState<Provider>(() => initial ?? emptyProvider());
  const [tab, setTab] = useState<Tab>("basic");
  const [check, setCheck] = useState<ProviderCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [saving, setSaving] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  // The key source is UI state: you must be able to pick "type it here" before
  // having typed anything.
  const [keySource, setKeySource] = useState<KeySource>(() => initialKeySource(initial));
  const [showKey, setShowKey] = useState(false);
  const isNew = initial === null;

  useEffect(() => {
    void api.providerKinds().then(setKinds).catch(() => {});
  }, []);

  // The preview refreshes itself: that is how the mapping becomes understandable.
  useEffect(() => {
    const t = window.setTimeout(() => {
      void api
        .providerCheck(p, p.default_model)
        .then(setCheck)
        .catch(() => setCheck(null));
    }, 350);
    return () => window.clearTimeout(t);
  }, [p]);

  const set = <K extends keyof Provider>(k: K, v: Provider[K]) => setP((x) => ({ ...x, [k]: v }));
  const text = (v: string) => (v.trim() === "" ? null : v);

  // A plain terminal does not use providers: it is left out of the scopes and of
  // the allowed-agents list, unless it already has a block of its own.
  const aiAgents = useMemo(
    () => agents.filter((a) => a.metrics || p.env[a.id] || p.args[a.id]),
    [agents, p.env, p.args],
  );
  const scopes = useMemo(() => [SCOPE_ALL, ...aiAgents.map((a) => a.id)], [aiAgents]);

  function setKey(source: KeySource, value: string, jsonPath?: string) {
    setKeySource(source);
    // One source at a time: switching clears the other fields.
    setP((x) => ({
      ...x,
      api_key: source === "literal" ? text(value) : null,
      api_key_env: source === "env" ? text(value) : null,
      api_key_file: source === "file" ? text(value) : null,
      api_key_json_path: source === "file" ? text(jsonPath ?? x.api_key_json_path ?? "") : null,
      api_key_command: source === "command" ? text(value) : null,
    }));
  }

  function setModel(i: number, changes: Partial<ProviderModel>) {
    setP((x) => ({ ...x, model: x.model.map((m, j) => (j === i ? { ...m, ...changes } : m)) }));
  }

  function setPrice(i: number, field: keyof NonNullable<ProviderModel["pricing"]>, v: number) {
    setP((x) => ({
      ...x,
      model: x.model.map((m, j) =>
        j === i
          ? {
              ...m,
              pricing: { input: 0, output: 0, cache_read: 0, cache_write: 0, ...(m.pricing ?? {}), [field]: v },
            }
          : m,
      ),
    }));
  }

  async function pickFile() {
    const sel = await openDialog({ multiple: false, title: "Fichero con la clave" });
    if (typeof sel === "string") setKey("file", sel);
  }

  async function save() {
    setSaving(true);
    const ok = await saveProvider(p);
    setSaving(false);
    if (ok) onClose();
  }

  const compatible = check?.agents.filter((a) => a.supported) ?? [];

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" style={{ width: "min(900px, calc(100vw - 40px))" }}>
        <div className="dialog-head">
          {isNew ? "Nuevo proveedor" : `Editar «${initial?.id}»`}
          <div className="tabs" style={{ marginLeft: 10 }}>
            <button className={`tab ${tab === "basic" ? "on" : ""}`} onClick={() => setTab("basic")}>
              Básico
            </button>
            <button className={`tab ${tab === "models" ? "on" : ""}`} onClick={() => setTab("models")}>
              Modelos {p.model.length > 0 && <span className="badge">{p.model.length}</span>}
            </button>
            <button
              className={`tab ${tab === "advanced" ? "on" : ""}`}
              onClick={() => setTab("advanced")}
            >
              Avanzado
            </button>
          </div>
          <span style={{ flex: 1 }} />
          <button className="icon-btn" onClick={onClose} title="Cerrar">
            <IconX />
          </button>
        </div>

        <div className="dialog-body">
          {tab === "basic" && (
            <>
              <div className="row">
                <div className="field">
                  <label>Identificador</label>
                  <input
                    value={p.id}
                    onChange={(e) => set("id", e.target.value)}
                    placeholder="mi-gateway"
                    spellCheck={false}
                  />
                </div>
                <div className="field">
                  <label>Nombre visible</label>
                  <input
                    value={p.name ?? ""}
                    onChange={(e) => set("name", text(e.target.value))}
                    placeholder="Mi Gateway"
                  />
                </div>
              </div>

              <div className="field">
                <label>Tipo de API</label>
                <select value={p.kind} onChange={(e) => set("kind", e.target.value)}>
                  {(kinds.length ? kinds : [p.kind]).map((k) => (
                    <option key={k} value={k}>
                      {k}
                    </option>
                  ))}
                </select>
                <div className="hint">{KIND_HELP[p.kind] ?? "—"}</div>
              </div>

              <div className="field">
                <label>URL base</label>
                <input
                  value={p.base_url ?? ""}
                  onChange={(e) => set("base_url", text(e.target.value))}
                  placeholder="https://api.ejemplo.dev/v1"
                  spellCheck={false}
                />
              </div>

              {/* The key sits right below the URL: it is what you fill in next.
                  The dropdown on the right changes the source without moving the
                  field around. */}
              <div className="field">
                <div className="label-row">
                  <label>Clave API</label>
                  <select
                    className="mini"
                    value={keySource}
                    onChange={(e) => setKey(e.target.value as KeySource, "")}
                    title="De dónde sale la clave"
                  >
                    {KEY_OPTIONS.map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </div>

                {keySource === "literal" && (
                  <>
                    <div className="row">
                      <input
                        type={showKey ? "text" : "password"}
                        value={p.api_key ?? ""}
                        spellCheck={false}
                        autoComplete="off"
                        placeholder="sk-…"
                        onChange={(e) => setKey("literal", e.target.value)}
                      />
                      <button
                        className="btn"
                        style={{ flex: "none" }}
                        onClick={() => setShowKey((v) => !v)}
                        title={showKey ? "Ocultar" : "Mostrar"}
                      >
                        <IconEye crossed={showKey} /> {showKey ? "Ocultar" : "Ver"}
                      </button>
                    </div>
                    <div className="hint" style={{ color: "var(--warn)" }}>
                      Se guarda sin cifrar en providers.toml.
                    </div>
                  </>
                )}

                {keySource === "env" && (
                  <>
                    <input
                      value={p.api_key_env ?? ""}
                      spellCheck={false}
                      placeholder="MI_API_KEY"
                      onChange={(e) => setKey("env", e.target.value)}
                    />
                    <div className="hint">
                      Nombre de la variable; la app la lee al lanzar la sesión.
                    </div>
                  </>
                )}

                {keySource === "file" && (
                  <div className="row">
                    <div style={{ flex: 2, minWidth: 0 }}>
                      <div className="row">
                        <input
                          value={p.api_key_file ?? ""}
                          spellCheck={false}
                          placeholder="~/.pi/agent/auth.json"
                          onChange={(e) => setKey("file", e.target.value)}
                        />
                        <button className="btn" style={{ flex: "none" }} onClick={() => void pickFile()}>
                          <IconFolder /> Elegir
                        </button>
                      </div>
                      <div className="hint">Fichero con la clave.</div>
                    </div>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <input
                        value={p.api_key_json_path ?? ""}
                        spellCheck={false}
                        placeholder="gorouter.key"
                        onChange={(e) => setKey("file", p.api_key_file ?? "", e.target.value)}
                      />
                      <div className="hint">
                        Ruta dentro del JSON; vacío si el fichero es solo la clave.
                      </div>
                    </div>
                  </div>
                )}

                {keySource === "command" && (
                  <>
                    <input
                      value={p.api_key_command ?? ""}
                      spellCheck={false}
                      placeholder="op read op://priv/gw/credential"
                      onChange={(e) => setKey("command", e.target.value)}
                    />
                    <div className="hint">Su salida se usa como clave.</div>
                  </>
                )}

                {keySource === "none" && (
                  <div className="hint">{KEY_OPTIONS.find((o) => o.value === "none")?.help}</div>
                )}
              </div>

              <div className="row" style={{ alignItems: "center", marginTop: 4 }}>
                <div className="field" style={{ margin: 0 }}>
                  <label>Modelo por defecto</label>
                  <select
                    value={p.default_model ?? ""}
                    onChange={(e) => set("default_model", text(e.target.value))}
                  >
                    <option value="">—</option>
                    {p.model.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.id}
                      </option>
                    ))}
                  </select>
                  {p.model.length === 0 && (
                    <div className="hint">
                      Añade modelos en la pestaña <b>Modelos</b>.
                    </div>
                  )}
                </div>
                <div className="field" style={{ margin: 0 }}>
                  <label>&nbsp;</label>
                  <label className="chk" style={{ height: 32 }}>
                    <input
                      type="checkbox"
                      checked={p.enabled}
                      onChange={(e) => set("enabled", e.target.checked)}
                    />
                    Habilitado
                  </label>
                </div>
              </div>

              <Section
                title="Comprobación"
                action={
                  <button
                    className="chip btn"
                    disabled={checking}
                    onClick={async () => {
                      setChecking(true);
                      setCheck(await api.providerCheck(p, p.default_model).catch(() => null));
                      setChecking(false);
                    }}
                  >
                    <IconKey width={12} height={12} /> {checking ? "Comprobando…" : "Comprobar ahora"}
                  </button>
                }
              />

              {!p.id.trim() || !p.base_url ? (
                <div className="hint">
                  Rellena el identificador y la URL base para ver qué recibirá cada agente.
                </div>
              ) : (
                check && (
                  <>
                    <div className={`key-status ${check.key_found ? "ok" : "bad"}`}>
                      {check.key_found ? (
                        <>
                          Clave localizada ({check.key_hint}). Origen:{" "}
                          {SOURCE_LABEL[check.key_source] ?? check.key_source}.
                        </>
                      ) : (
                        <>No hay clave: {check.key_error}</>
                      )}
                    </div>

                    {compatible.length === 0 && (
                      <div className="issue" style={{ marginTop: 8 }}>
                        Con <b>{p.kind}</b> ningún agente instalado puede usar este proveedor. Cambia el
                        tipo de API o define el mapeo a mano en <b>Avanzado</b>.
                      </div>
                    )}

                    {check.agents.map((a) => (
                      <div key={a.agent_id} className={`preview ${a.supported ? "" : "off"}`}>
                        <div className="preview-head">
                          <b>{a.agent_name}</b>
                          {a.supported ? (
                            <span className="badge ok">compatible</span>
                          ) : (
                            <span className="badge">no compatible</span>
                          )}
                          {!a.installed && <span className="badge err">no instalado</span>}
                          {a.supported && a.from_template && (
                            <span className="badge" title="Sale de la plantilla del tipo de API">
                              automático
                            </span>
                          )}
                        </div>
                        {a.supported && (
                          <div className="preview-body">
                            {a.env.map(([k, v]) => (
                              <div key={k}>
                                <span className="ev-k">{k}</span>=<span className="ev-v">{v}</span>
                              </div>
                            ))}
                            {a.args.length > 0 && <div className="ev-args">{a.args.join(" ")}</div>}
                            {a.env.length === 0 && a.args.length === 0 && (
                              <div className="hint">Sin variables: revisa la clave y la URL.</div>
                            )}
                          </div>
                        )}
                      </div>
                    ))}
                  </>
                )
              )}
            </>
          )}

          {tab === "models" && (
            <>
              <Section
                title="Modelos"
                action={
                  <button
                    className="chip btn"
                    onClick={() => setP((x) => ({ ...x, model: [...x.model, emptyModel()] }))}
                  >
                    <IconPlus width={12} height={12} /> Añadir modelo
                  </button>
                }
              />
              {p.model.length === 0 && (
                <div className="hint">
                  Sin modelos no podrás elegir ninguno al crear la sesión, y no habrá ventana de
                  contexto ni coste.
                </div>
              )}
              {p.model.map((m, i) => (
                <div className="editor-row" key={i}>
                  <div className="editor-grid">
                    <Field span={2} label="id">
                      <input value={m.id} onChange={(e) => setModel(i, { id: e.target.value })} spellCheck={false} />
                    </Field>
                    <Field span={2} label="nombre">
                      <input value={m.name ?? ""} onChange={(e) => setModel(i, { name: text(e.target.value) })} />
                    </Field>
                    <Field span={2} label="contexto">
                      <input
                        type="number"
                        value={m.context_window ?? ""}
                        onChange={(e) =>
                          setModel(i, { context_window: e.target.value ? Number(e.target.value) : null })
                        }
                      />
                    </Field>
                    <Field span={2} label="máx. salida">
                      <input
                        type="number"
                        value={m.max_output_tokens ?? ""}
                        onChange={(e) =>
                          setModel(i, { max_output_tokens: e.target.value ? Number(e.target.value) : null })
                        }
                      />
                    </Field>
                    <Field span={2} label="id remoto">
                      <input
                        value={m.remote_id ?? ""}
                        onChange={(e) => setModel(i, { remote_id: text(e.target.value) })}
                        spellCheck={false}
                      />
                    </Field>
                    <Field span={2} label="$/M ent.">
                      <input
                        type="number"
                        step="0.01"
                        value={m.pricing?.input ?? ""}
                        onChange={(e) => setPrice(i, "input", Number(e.target.value))}
                      />
                    </Field>
                    <Field span={2} label="$/M sal.">
                      <input
                        type="number"
                        step="0.01"
                        value={m.pricing?.output ?? ""}
                        onChange={(e) => setPrice(i, "output", Number(e.target.value))}
                      />
                    </Field>
                    <Field span={2} label="$/M caché">
                      <input
                        type="number"
                        step="0.01"
                        value={m.pricing?.cache_read ?? ""}
                        onChange={(e) => setPrice(i, "cache_read", Number(e.target.value))}
                      />
                    </Field>
                  </div>
                  <div className="editor-actions">
                    <label className="chk" title="Razonamiento">
                      <input
                        type="checkbox"
                        checked={m.reasoning}
                        onChange={(e) => setModel(i, { reasoning: e.target.checked })}
                      />
                      razona
                    </label>
                    <label className="chk" title="Entrada de imágenes">
                      <input
                        type="checkbox"
                        checked={m.vision}
                        onChange={(e) => setModel(i, { vision: e.target.checked })}
                      />
                      visión
                    </label>
                    <button
                      className="icon-btn"
                      title="Quitar modelo"
                      onClick={() => setP((x) => ({ ...x, model: x.model.filter((_, j) => j !== i) }))}
                    >
                      <IconTrash width={13} height={13} />
                    </button>
                  </div>
                </div>
              ))}

              <div className="field" style={{ marginTop: 14 }}>
                <label>Modelo económico (tareas auxiliares)</label>
                <select value={p.small_model ?? ""} onChange={(e) => set("small_model", text(e.target.value))}>
                  <option value="">—</option>
                  {p.model.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.id}
                    </option>
                  ))}
                </select>
              </div>
            </>
          )}

          {tab === "advanced" && (
            <>
              <div className="hint" style={{ marginBottom: 12 }}>
                Nada de esto es obligatorio: por defecto el mapeo sale del tipo de API. Usa esta
                pestaña solo para añadir o corregir algo puntual. Una variable con valor vacío anula
                la heredada.
              </div>

              <Section title="Límites" />
              <div className="row">
                <div className="field">
                  <label>Timeout (ms)</label>
                  <input
                    type="number"
                    value={p.timeout_ms ?? ""}
                    onChange={(e) => set("timeout_ms", e.target.value ? Number(e.target.value) : null)}
                    placeholder="600000"
                  />
                </div>
                <div className="field">
                  <label>Reintentos</label>
                  <input
                    type="number"
                    value={p.max_retries ?? ""}
                    onChange={(e) => set("max_retries", e.target.value ? Number(e.target.value) : null)}
                    placeholder="3"
                  />
                </div>
              </div>

              <Section title="Agentes permitidos" />
              <div className="hint" style={{ marginBottom: 6 }}>
                Sin marcar nada, se permiten los que admita el tipo de API
                {check ? `: ${compatible.map((a) => a.agent_name).join(", ") || "ninguno"}` : ""}.
              </div>
              <div className="chk-row">
                {aiAgents.map((a) => (
                  <label className="chk" key={a.id}>
                    <input
                      type="checkbox"
                      checked={p.agents.includes(a.id)}
                      onChange={(e) =>
                        set("agents", e.target.checked ? [...p.agents, a.id] : p.agents.filter((x) => x !== a.id))
                      }
                    />
                    {a.name}
                  </label>
                ))}
              </div>

              <Section title="Variables de entorno" />
              <div className="hint" style={{ marginBottom: 8 }}>
                Plantillas:{" "}
                <span className="mono">
                  {"{base_url} {api_key} {model} {max_output_tokens} {env_var}"}
                </span>
                …
              </div>
              {scopes.map((scope) => (
                <KeyValueEditor
                  key={scope}
                  title={scope === SCOPE_ALL ? "all · cualquier agente" : scope}
                  inherited={check?.agents.find((a) => a.agent_id === scope)?.env.map(([k]) => k) ?? []}
                  values={p.env[scope] ?? {}}
                  onChange={(v) => setP((x) => ({ ...x, env: { ...x.env, [scope]: v } }))}
                />
              ))}

              <Section title="Argumentos extra" />
              {scopes.map((scope) => (
                <div className="field" key={scope}>
                  <label>{scope === SCOPE_ALL ? "all" : scope}</label>
                  <textarea
                    rows={Math.min(8, Math.max(2, (p.args[scope]?.length ?? 0) + 1))}
                    spellCheck={false}
                    value={(p.args[scope] ?? []).join("\n")}
                    placeholder={"-c\nmodel={model}"}
                    onChange={(e) =>
                      setP((x) => ({
                        ...x,
                        args: { ...x.args, [scope]: e.target.value.split("\n").filter((l) => l.trim() !== "") },
                      }))
                    }
                  />
                  <div className="hint">Un argumento por línea. Reemplaza la plantilla de este agente.</div>
                </div>
              ))}

              <Section title="Otros" />
              <KeyValueEditor
                title="Cabeceras HTTP"
                values={p.headers}
                inherited={[]}
                onChange={(v) => set("headers", v)}
              />
              <div className="field">
                <label>Notas</label>
                <input value={p.notes ?? ""} onChange={(e) => set("notes", text(e.target.value))} />
              </div>
            </>
          )}
        </div>

        <div className="dialog-foot">
          {!isNew &&
            (confirmingDelete ? (
              <div style={{ marginRight: "auto", display: "flex", gap: 8, alignItems: "center" }}>
                <span className="hint" style={{ margin: 0 }}>
                  ¿Eliminar «{initial?.id}» de providers.toml?
                </span>
                <button className="btn" onClick={() => setConfirmingDelete(false)}>
                  No
                </button>
                <button
                  className="btn"
                  style={{ background: "var(--err)", borderColor: "var(--err)", color: "#fff" }}
                  onClick={async () => {
                    await deleteProvider(initial!.id);
                    onClose();
                  }}
                >
                  Sí, eliminar
                </button>
              </div>
            ) : (
              <button
                className="btn"
                style={{ marginRight: "auto", color: "var(--err)" }}
                onClick={() => setConfirmingDelete(true)}
              >
                <IconTrash width={13} height={13} /> Eliminar
              </button>
            ))}
          <button className="btn" onClick={onClose}>
            Cancelar
          </button>
          <button className="btn primary" disabled={!p.id.trim() || saving} onClick={() => void save()}>
            {saving ? "Guardando…" : "Guardar"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Section({ title, action }: { title: string; action?: React.ReactNode }) {
  return (
    <div className="section">
      <span>{title}</span>
      <span className="line" />
      {action}
    </div>
  );
}

function Field({ label, children, span = 1 }: { label: string; children: React.ReactNode; span?: number }) {
  return (
    <div style={{ gridColumn: `span ${span}` }}>
      <label>{label}</label>
      {children}
    </div>
  );
}

/** Key/value editor with an always-present empty row at the end. */
function KeyValueEditor({
  title,
  values,
  inherited,
  onChange,
}: {
  title: string;
  values: Record<string, string>;
  inherited: string[];
  onChange: (v: Record<string, string>) => void;
}) {
  const rows = [...Object.entries(values), ["", ""] as [string, string]];
  const own = new Set(Object.keys(values));
  const onlyInherited = inherited.filter((k) => !own.has(k));

  const update = (i: number, key: string, value: string) => {
    const next: Record<string, string> = {};
    rows.forEach(([k, v], j) => {
      const kk = j === i ? key : k;
      const vv = j === i ? value : v;
      if (kk.trim() !== "") next[kk] = vv;
    });
    onChange(next);
  };

  return (
    <div className="kvmap">
      <div className="kvmap-title">
        {title}
        {onlyInherited.length > 0 && (
          <span className="kvmap-inherited"> · automáticas: {onlyInherited.join(", ")}</span>
        )}
      </div>
      {rows.map(([k, v], i) => (
        <div className="kvmap-row" key={i}>
          <input value={k} placeholder="VARIABLE" spellCheck={false} onChange={(e) => update(i, e.target.value, v)} />
          <input value={v} placeholder="{base_url}" spellCheck={false} onChange={(e) => update(i, k, e.target.value)} />
          {k !== "" && (
            <button
              className="icon-btn"
              title="Quitar variable"
              onClick={() => {
                const next = { ...values };
                delete next[k];
                onChange(next);
              }}
            >
              <IconTrash width={12} height={12} />
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
