import { useEffect, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { shortPath } from "../lib/format";
import { providersFor } from "../lib/providers";
import { useStore } from "../state/store";
import { IconFolder } from "./Icons";

export function NewSessionDialog() {
  const close = () => useStore.getState().setDialog(null);
  const config = useStore((s) => s.config);
  const agents = useStore((s) => s.agents);
  const projects = useStore((s) => s.projects);
  const sessions = useStore((s) => s.sessions);
  const home = useStore((s) => s.home);
  const createSession = useStore((s) => s.createSession);

  const defaults = config?.app.defaults;
  const [path, setPath] = useState<string>(() => defaults?.cwd ?? home ?? "");
  const [agentId, setAgentId] = useState<string>(defaults?.agent ?? agents[0]?.id ?? "");
  const [providerId, setProviderId] = useState<string>(defaults?.provider ?? "");
  const [model, setModel] = useState<string>(defaults?.model ?? "");
  const [title, setTitle] = useState("");
  const [resumeId, setResumeId] = useState<string>("");
  const [continueLast, setContinueLast] = useState(false);
  const [creating, setCreating] = useState(false);

  // Lets the sidebar preselect a project.
  useEffect(() => {
    const h = (e: Event) => setPath(String((e as CustomEvent).detail ?? ""));
    window.addEventListener("sessions:preset-project", h);
    return () => window.removeEventListener("sessions:preset-project", h);
  }, []);

  const agent = agents.find((a) => a.id === agentId);

  // Recomputed from providers.toml, so a brand new provider shows up here
  // without restarting the app.
  const validProviders = useMemo(
    () => providersFor(config?.providers ?? [], agentId),
    [config, agentId],
  );

  // If the chosen provider does not work for the agent, clear it.
  useEffect(() => {
    if (providerId && !validProviders.some((p) => p.id === providerId)) setProviderId("");
  }, [providerId, validProviders]);

  const provider = validProviders.find((p) => p.id === providerId);
  const models = provider?.model ?? [];

  useEffect(() => {
    if (!provider) return;
    if (!models.some((m) => m.id === model)) setModel(provider.default_model ?? models[0]?.id ?? "");
  }, [provider, models, model]);

  const previous = useMemo(
    () =>
      sessions.filter(
        (s) =>
          s.agent_id === agentId &&
          s.external_id &&
          s.cwd.replace(/\\/g, "/") === path.replace(/\\/g, "/"),
      ),
    [sessions, agentId, path],
  );

  const canLaunch = Boolean(path && agentId && agent?.installed) && !creating;

  async function pickFolder() {
    const sel = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: path || home || undefined,
    });
    if (typeof sel === "string") setPath(sel);
  }

  async function launch() {
    setCreating(true);
    const meta = await createSession({
      project_path: path,
      agent_id: agentId,
      provider_id: providerId || null,
      model: model || null,
      title: title.trim() || null,
      cwd: path,
      resume_external_id: resumeId || null,
      continue_last: continueLast,
    });
    setCreating(false);
    if (meta) close();
  }

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="dialog">
        <div className="dialog-head">Nueva sesión</div>
        <div className="dialog-body">
          <div className="field">
            <label>Directorio de trabajo</label>
            <div className="row">
              <input
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="C:\ruta\al\proyecto"
                spellCheck={false}
              />
              <button className="btn" style={{ flex: "none" }} onClick={() => void pickFolder()}>
                <IconFolder /> Examinar
              </button>
            </div>
            {projects.length > 0 && (
              <div className="pick-list" style={{ marginTop: 8 }}>
                {projects.map((p) => (
                  <div
                    key={p.id}
                    className={`pick ${p.path === path ? "on" : ""}`}
                    onClick={() => setPath(p.path)}
                  >
                    <IconFolder width={13} height={13} />
                    {p.name}
                    <span className="path">{shortPath(p.path, 46)}</span>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="row">
            <div className="field">
              <label>Agente</label>
              <select value={agentId} onChange={(e) => setAgentId(e.target.value)}>
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                    {a.installed ? "" : " (no instalado)"}
                  </option>
                ))}
              </select>
              {agent && !agent.installed && (
                <div className="hint" style={{ color: "var(--err)" }}>
                  No se encontró el ejecutable en el PATH.
                </div>
              )}
              {agent?.path && <div className="hint">{agent.path}</div>}
            </div>

            <div className="field">
              <label>Proveedor</label>
              <select value={providerId} onChange={(e) => setProviderId(e.target.value)}>
                <option value="">(el del propio agente)</option>
                {validProviders.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name ?? p.id}
                  </option>
                ))}
              </select>
              <div className="hint">Definidos en providers.toml</div>
            </div>
          </div>

          <div className="row">
            <div className="field">
              <label>Modelo</label>
              <select value={model} onChange={(e) => setModel(e.target.value)} disabled={!provider}>
                {!provider && <option value="">—</option>}
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name ?? m.id}
                    {m.context_window ? ` · ${Math.round(m.context_window / 1000)}k` : ""}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>Título (opcional)</label>
              <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Auto" />
            </div>
          </div>

          <div className="field">
            <label>Reanudar</label>
            <select
              value={resumeId}
              onChange={(e) => {
                setResumeId(e.target.value);
                if (e.target.value) setContinueLast(false);
              }}
            >
              <option value="">Sesión nueva</option>
              {previous.map((s) => (
                <option key={s.id} value={s.external_id!}>
                  {s.title} · {s.external_id}
                </option>
              ))}
            </select>
            <label className="chk" style={{ marginTop: 8 }}>
              <input
                type="checkbox"
                checked={continueLast}
                disabled={Boolean(resumeId)}
                onChange={(e) => setContinueLast(e.target.checked)}
              />
              Continuar la última sesión del directorio
            </label>
          </div>
        </div>

        <div className="dialog-foot">
          <button className="btn" onClick={close}>
            Cancelar
          </button>
          <button className="btn primary" disabled={!canLaunch} onClick={() => void launch()}>
            {creating ? "Lanzando…" : "Lanzar sesión"}
          </button>
        </div>
      </div>
    </div>
  );
}
