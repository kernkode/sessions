import { useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";

import { useStore } from "../state/store";
import { IconRefresh } from "./Icons";

type Tab = "general" | "agents" | "performance";

const TAB_LABEL: Record<Tab, string> = {
  general: "General",
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
    </div>
  );
}
