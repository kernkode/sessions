import { useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";

import { describeKey, supportsAgent } from "../lib/providers";
import type { Provider } from "../lib/types";
import { useStore } from "../state/store";
import { IconCopy, IconEdit, IconPlus, IconRefresh } from "./Icons";
import { ProviderEditor } from "./ProviderEditor";

type Tab = "general" | "providers" | "agents" | "performance";

const TAB_LABEL: Record<Tab, string> = {
  general: "General",
  providers: "Proveedores",
  agents: "Agentes",
  performance: "Rendimiento",
};

export function SettingsDialog() {
  const close = () => useStore.getState().setDialog(null);
  const config = useStore((s) => s.config);
  const agents = useStore((s) => s.agents);
  const version = useStore((s) => s.version);
  const platform = useStore((s) => s.platform);
  const reloadConfig = useStore((s) => s.reloadConfig);
  const [tab, setTab] = useState<Tab>("general");
  // `null` = closed; `{ provider: null }` = creating a new provider.
  const [editing, setEditing] = useState<{ provider: Provider | null } | null>(null);

  if (!config) return null;
  const { paths, issues, app } = config;

  const open = (p: string) => void openPath(p).catch(() => {});

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="dialog" style={{ width: "min(860px, calc(100vw - 48px))" }}>
        <div className="dialog-head">
          Ajustes
          <div className="tabs" style={{ marginLeft: 10 }}>
            {(Object.keys(TAB_LABEL) as Tab[]).map((t) => (
              <button key={t} className={`tab ${tab === t ? "on" : ""}`} onClick={() => setTab(t)}>
                {TAB_LABEL[t]}
              </button>
            ))}
          </div>
          <div style={{ flex: 1 }} />
          <button className="chip btn" onClick={() => void reloadConfig()} title="Volver a leer los .toml">
            <IconRefresh width={13} height={13} /> Recargar
          </button>
        </div>

        <div className="dialog-body">
          {issues.length > 0 && (
            <div style={{ marginBottom: 12 }}>
              {issues.map((i, n) => (
                <div className="issue" key={n}>
                  <b>{i.file}</b>: {i.message}
                </div>
              ))}
            </div>
          )}

          {tab === "general" && (
            <table className="info">
              <tbody>
                <tr>
                  <td>Carpeta de datos</td>
                  <td className="mono">
                    <button className="chip btn" onClick={() => open(paths.root)}>
                      {paths.root}
                    </button>
                  </td>
                </tr>
                <tr>
                  <td>config.toml</td>
                  <td className="mono">
                    <button className="chip btn" onClick={() => open(paths.config)}>
                      Abrir
                    </button>{" "}
                    {paths.config}
                  </td>
                </tr>
                <tr>
                  <td>providers.toml</td>
                  <td className="mono">
                    <button className="chip btn" onClick={() => open(paths.providers)}>
                      Abrir
                    </button>{" "}
                    {paths.providers}
                  </td>
                </tr>
                <tr>
                  <td>agents.toml</td>
                  <td className="mono">
                    <button className="chip btn" onClick={() => open(paths.agents)}>
                      Abrir
                    </button>{" "}
                    {paths.agents}
                  </td>
                </tr>
                <tr>
                  <td>Tema / idioma</td>
                  <td className="mono">
                    {app.app.theme} · {app.app.language}
                  </td>
                </tr>
                <tr>
                  <td>Scrollback persistente</td>
                  <td className="mono">{app.app.persist_scrollback ? "sí" : "no"}</td>
                </tr>
                <tr>
                  <td>Versión / plataforma</td>
                  <td className="mono">
                    v{version} · {platform}
                  </td>
                </tr>
                <tr>
                  <td>Atajos</td>
                  <td className="mono">
                    {Object.entries(app.keybinds)
                      .map(([k, v]) => `${k}: ${v}`)
                      .join("  ·  ")}
                  </td>
                </tr>
              </tbody>
            </table>
          )}

          {tab === "providers" && (
            <>
              <div className="section">
                <span>providers.toml · {config.providers.length} proveedores</span>
                <span className="line" />
                <button className="chip btn" onClick={() => setEditing({ provider: null })}>
                  <IconPlus width={12} height={12} /> Nuevo proveedor
                </button>
              </div>
              <div className="hint" style={{ marginBottom: 10 }}>
                El <b>tipo de API</b> decide qué agentes pueden usar cada proveedor y qué variables
                reciben; solo hay que bajar al detalle para casos especiales. Al guardar desde aquí se
                reescribe solo su bloque: los comentarios del fichero se conservan.
              </div>
              {config.providers.map((p) => (
                <div key={p.id} className="card" style={{ cursor: "default" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <b>{p.name ?? p.id}</b>
                    <span className="badge">{p.kind}</span>
                    <span className={`badge ${p.enabled ? "ok" : "err"}`}>
                      {p.enabled ? "activo" : "inactivo"}
                    </span>
                    <span className="badge">{p.model.length} modelo(s)</span>
                    <span style={{ flex: 1 }} />
                    <button className="icon-btn" title="Editar" onClick={() => setEditing({ provider: p })}>
                      <IconEdit width={13} height={13} />
                    </button>
                    <button
                      className="icon-btn"
                      title="Duplicar"
                      onClick={() =>
                        setEditing({
                          provider: {
                            ...p,
                            id: `${p.id}-copia`,
                            name: p.name ? `${p.name} (copia)` : null,
                          },
                        })
                      }
                    >
                      <IconCopy width={13} height={13} />
                    </button>
                  </div>
                  <div className="card-sub" style={{ marginTop: 4 }}>
                    {p.base_url ?? "—"}
                  </div>
                  <div className="hint">
                    clave: {describeKey(p)}
                    {" · "}
                    agentes: {p.supported_agents.join(", ") || "ninguno compatible"}
                    {" · "}
                    modelo por defecto: {p.default_model ?? "—"}
                  </div>
                </div>
              ))}
            </>
          )}

          {tab === "agents" && (
            <>
              {agents.map((a) => (
                <div key={a.id} className="card" style={{ cursor: "default" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <span className="dot" style={{ background: a.color ?? "var(--txt-3)", boxShadow: "none" }} />
                    <b>{a.name}</b>
                    <span className={`badge ${a.installed ? "ok" : "err"}`}>
                      {a.installed ? "instalado" : "no encontrado"}
                    </span>
                    {a.metrics && <span className="badge">métricas de tokens</span>}
                    <span style={{ flex: 1 }} />
                    <span className="card-sub">
                      {config.providers.filter((p) => supportsAgent(p, a.id)).length} proveedor(es)
                    </span>
                  </div>
                  <div className="hint">{a.path ?? "revisa el PATH o ajusta command en agents.toml"}</div>
                </div>
              ))}
            </>
          )}

          {tab === "performance" && (
            <table className="info">
              <tbody>
                {Object.entries(app.performance).map(([k, v]) => (
                  <tr key={k}>
                    <td>{k}</td>
                    <td className="mono">{String(v)}</td>
                  </tr>
                ))}
                {Object.entries(app.terminal).map(([k, v]) => (
                  <tr key={k}>
                    <td>terminal.{k}</td>
                    <td className="mono">{String(v)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        <div className="dialog-foot">
          <button className="btn primary" onClick={close}>
            Cerrar
          </button>
        </div>
      </div>

      {editing && <ProviderEditor initial={editing.provider} onClose={() => setEditing(null)} />}
    </div>
  );
}
