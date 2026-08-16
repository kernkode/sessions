import { Fragment, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { openPath } from "@tauri-apps/plugin-opener";

import { useStore } from "../state/store";
import { fmtBytes } from "../lib/format";
import type { AgentStatus, AppConfig, ConfigPaths } from "../lib/types";
import {
  IconChart,
  IconEdit,
  IconFolder,
  IconGear,
  IconRefresh,
  IconTerminal,
  IconX,
} from "./Icons";

type Tab = "general" | "agents" | "performance";

const TAB_LABEL: Record<Tab, string> = {
  general: "General",
  agents: "Agents",
  performance: "Performance",
};

const TAB_ICON: Record<Tab, ReactNode> = {
  general: <IconGear />,
  agents: <IconTerminal />,
  performance: <IconChart />,
};

// Nombres legibles para los atajos conocidos; el resto se humaniza.
const KEYBIND_LABEL: Record<string, string> = {
  new_session: "New session",
  close_session: "Close session",
  next_session: "Next session",
  prev_session: "Previous session",
  toggle_sidebar: "Toggle sidebar",
  toggle_metrics: "Toggle metrics",
  reload_config: "Reload configuration",
  clear_terminal: "Clear terminal",
  find: "Search in terminal",
  settings: "Open settings",
};

const humanize = (k: string) => k.replace(/_/g, " ");

/** Da unidad a los valores numéricos según el sufijo de la clave de config. */
function fmtValue(key: string, v: unknown): string {
  if (typeof v === "boolean") return v ? "yes" : "no";
  if (typeof v === "number") {
    if (key.endsWith("_ms") || key.includes("_ms_")) return v === 0 ? "disabled" : `${v} ms`;
    if (key.endsWith("_kb")) return fmtBytes(v * 1024);
    if (key.endsWith("_bytes")) return fmtBytes(v);
  }
  return String(v);
}

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
      <div className="dialog settings">
        <div className="dialog-head">
          Settings
          <div style={{ flex: 1 }} />
          <button
            className="chip btn"
            onClick={() => void reloadConfig()}
            title="Re-read the .toml files"
          >
            <IconRefresh width={13} height={13} /> Reload
          </button>
          <button className="icon-btn" onClick={close} title="Close">
            <IconX width={13} height={13} />
          </button>
        </div>

        <div className="settings-main">
          <nav className="settings-nav">
            {(Object.keys(TAB_LABEL) as Tab[]).map((t) => (
              <button key={t} className={`snav ${tab === t ? "on" : ""}`} onClick={() => setTab(t)}>
                {TAB_ICON[t]}
                {TAB_LABEL[t]}
                {t === "agents" && <span className="n">{agents.length}</span>}
              </button>
            ))}
          </nav>

          <div className="settings-content">
            {issues.length > 0 && (
              <div style={{ marginBottom: 14 }}>
                {issues.map((i, n) => (
                  <div className="issue" key={n}>
                    <b>{i.file}</b>: {i.message}
                  </div>
                ))}
              </div>
            )}

            {tab === "general" && <GeneralTab app={app} paths={paths} open={open} />}
            {tab === "agents" && <AgentsTab agents={agents} onEdit={() => open(paths.agents)} />}
            {tab === "performance" && <PerformanceTab app={app} />}
          </div>
        </div>

        <div className="dialog-foot">
          <span className="foot-meta">
            v{version} · {platform}
          </span>
          <button className="btn primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/* ───────── Piezas de maquetación ───────── */

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="set-section">
      <h3>{title}</h3>
      <div className="set-group">{children}</div>
    </section>
  );
}

function Row({ label, children }: { label: ReactNode; children?: ReactNode }) {
  return (
    <div className="set-row">
      <div className="lab">
        <div className="name">{label}</div>
      </div>
      {children}
    </div>
  );
}

/** Fila de ruta: etiqueta, path mono truncado y botón para abrirla. */
function PathRow({ label, path, onOpen }: { label: string; path: string; onOpen: () => void }) {
  return (
    <div className="set-row">
      <div className="lab">
        <div className="name">{label}</div>
        <div className="desc mono" title={path}>
          {path}
        </div>
      </div>
      <button className="chip btn" onClick={onOpen}>
        <IconFolder width={12} height={12} /> Open
      </button>
    </div>
  );
}

const Bool = ({ v, onChange }: { v: boolean; onChange: (v: boolean) => void }) => (
  <button className={`badge ${v ? "ok" : ""}`} title="toggle" onClick={() => onChange(!v)}>
    {v ? "yes" : "no"}
  </button>
);

const Select = ({
  value,
  options,
  onChange,
}: {
  value: string;
  options: string[];
  onChange: (v: string) => void;
}) => (
  <select className="mini" value={value} onChange={(e) => onChange(e.target.value)}>
    {options.map((o) => (
      <option key={o} value={o}>
        {o}
      </option>
    ))}
  </select>
);

/** «Ctrl+Shift+T» como teclas físicas. */
function Keys({ combo }: { combo: string }) {
  return (
    <span className="keys">
      {combo.split("+").map((p, i) => (
        <Fragment key={i}>
          {i > 0 && <span className="kbd-sep">+</span>}
          <kbd className="kbd">{p}</kbd>
        </Fragment>
      ))}
    </span>
  );
}

/* ───────── Pestañas ───────── */

function GeneralTab({
  app,
  paths,
  open,
}: {
  app: AppConfig;
  paths: ConfigPaths;
  open: (p: string) => void;
}) {
  const set = (patch: Partial<AppConfig["app"]>) =>
    void useStore.getState().updateAppConfig(patch);
  return (
    <>
      <Section title="Storage">
        <PathRow label="Data folder" path={paths.root} onOpen={() => open(paths.root)} />
        <PathRow label="config.toml" path={paths.config} onOpen={() => open(paths.config)} />
        <PathRow label="agents.toml" path={paths.agents} onOpen={() => open(paths.agents)} />
      </Section>

      <Section title="Application">
        <Row label="Theme">
          <Select value={app.app.theme} options={["dark", "light"]} onChange={(v) => set({ theme: v })} />
        </Row>
        <Row label="Language">
          <Select value={app.app.language} options={["es", "en"]} onChange={(v) => set({ language: v })} />
        </Row>
        <Row label="Restore sessions on start">
          <Bool v={app.app.restore_sessions} onChange={(v) => set({ restore_sessions: v })} />
        </Row>
        <Row label="Auto-resume">
          <Select
            value={app.app.auto_resume}
            options={["active", "all", "none"]}
            onChange={(v) => set({ auto_resume: v })}
          />
        </Row>
        <Row label="Confirm on close">
          <Bool v={app.app.confirm_on_close} onChange={(v) => set({ confirm_on_close: v })} />
        </Row>
        <Row label="Persistent scrollback">
          <Bool v={app.app.persist_scrollback} onChange={(v) => set({ persist_scrollback: v })} />
        </Row>
        <Row label="Relaunch on exit">
          <Bool v={app.app.auto_relaunch} onChange={(v) => set({ auto_relaunch: v })} />
        </Row>
      </Section>

      <Section title="Keyboard shortcuts">
        {Object.entries(app.keybinds).map(([k, v]) => (
          <Row key={k} label={KEYBIND_LABEL[k] ?? humanize(k)}>
            <Keys combo={v} />
          </Row>
        ))}
      </Section>
    </>
  );
}

function AgentsTab({ agents, onEdit }: { agents: AgentStatus[]; onEdit: () => void }) {
  return (
    <>
      <div className="agents-head">
        <span className="hint">Agents and their commands are defined in agents.toml.</span>
        <button className="chip btn" onClick={onEdit}>
          <IconEdit width={12} height={12} /> Edit agents.toml
        </button>
      </div>
      {agents.map((a) => (
        <div key={a.id} className="agent">
          <span className="agent-tile" style={{ "--c": a.color } as CSSProperties}>
            {a.name.charAt(0).toUpperCase()}
          </span>
          <div className="agent-main">
            <div className="agent-top">
              <b>{a.name}</b>
              <span className={`badge ${a.installed ? "ok" : "err"}`}>
                {a.installed ? "installed" : "not found"}
              </span>
              {a.metrics && <span className="badge">token metrics</span>}
            </div>
            <div className={`agent-path ${a.path ? "" : "missing"}`} title={a.path ?? undefined}>
              {a.path ?? "check PATH or set command in agents.toml"}
            </div>
          </div>
        </div>
      ))}
    </>
  );
}

function PerformanceTab({ app }: { app: AppConfig }) {
  const rows = (obj: object) =>
    Object.entries(obj).map(([k, v]) => (
      <Row key={k} label={<span className="mono-key">{k}</span>}>
        <span className="val" title={String(v)}>
          {fmtValue(k, v)}
        </span>
      </Row>
    ));

  return (
    <>
      <Section title="Performance">{rows(app.performance)}</Section>
      <Section title="Terminal">{rows(app.terminal)}</Section>
      <p className="hint">
        These values are edited in config.toml and applied with the Reload button.
      </p>
    </>
  );
}
