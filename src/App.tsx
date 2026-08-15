import { Fragment, useEffect, useRef } from "react";

import { MetricsBar } from "./components/MetricsBar";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { SearchBar } from "./components/SearchBar";
import { SessionHeader } from "./components/SessionHeader";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { IconPlay, IconPlus, IconRefresh } from "./components/Icons";
import { pool } from "./term/pool";
import { selActiveSession, useStore } from "./state/store";

export default function App() {
  const ready = useStore((s) => s.ready);
  const error = useStore((s) => s.error);
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const metricsOpen = useStore((s) => s.metricsOpen);
  const dialog = useStore((s) => s.dialog);
  const notice = useStore((s) => s.notice);
  const activeId = useStore((s) => s.activeId);
  const sessions = useStore((s) => s.sessions);
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void useStore.getState().init();
    return () => pool.disposeAll();
  }, []);

  useEffect(() => {
    pool.setHost(host.current);
  }, [ready]);

  // On startup, select the most recently used session. It also runs again if the
  // selected one disappears, which is what happens when a session is relaunched
  // and comes back with a new id.
  useEffect(() => {
    if (!ready || sessions.length === 0) return;
    if (activeId && sessions.some((s) => s.id === activeId)) return;
    const last = [...sessions].sort((a, b) => b.last_active_at - a.last_active_at)[0];
    void useStore.getState().setActive(last.id);
  }, [ready, activeId, sessions]);

  // Global shortcuts. They are captured before the terminal sees the key and only
  // Shift combinations are used: Ctrl+W, Ctrl+R or Ctrl+F belong to the agents and
  // shells themselves and must reach the PTY untouched.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useStore.getState();
      const ctrl = e.ctrlKey || e.metaKey;
      if (!ctrl) return;

      const take = () => {
        e.preventDefault();
        e.stopPropagation();
      };

      if (e.key === "Tab") {
        take();
        void s.cycleSession(e.shiftKey ? -1 : 1);
        return;
      }
      if (e.key === ",") {
        take();
        s.setDialog("settings");
        return;
      }
      if (!e.shiftKey) return;

      switch (e.key.toLowerCase()) {
        case "t":
          take();
          s.setDialog("new-session");
          break;
        case "w":
          if (s.activeId) {
            take();
            void s.closeSession(s.activeId, true);
          }
          break;
        case "b":
          take();
          s.toggleSidebar();
          break;
        case "m":
          take();
          s.toggleMetrics();
          break;
        case "f":
          take();
          s.setDialog("search");
          break;
        case "k":
          if (s.activeId) {
            take();
            void s.clearTerminal(s.activeId);
          }
          break;
        case "r":
          take();
          void s.reloadConfig();
          break;
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  if (!ready) {
    return <div className="loading">Cargando ~/.sessions…</div>;
  }

  return (
    <div className="app">
      <TitleBar />
      <SessionHeader />
      <div className="body">
        {sidebarOpen && <Sidebar />}
        <main>
          <div id="term-host" ref={host} />
          {!activeId && <EmptyState />}
          <EndedBanner />
          {dialog === "search" && <SearchBar />}
        </main>
      </div>
      {metricsOpen && <MetricsBar />}

      {dialog === "new-session" && <NewSessionDialog />}
      {dialog === "settings" && <SettingsDialog />}

      {(notice || error) && <div className="toast">{error ?? notice}</div>}
    </div>
  );
}

/**
 * A session whose process ended shows its saved output as a transcript, but
 * nothing can be typed into it. This makes that explicit and puts the two ways
 * out within reach.
 */
function EndedBanner() {
  const session = useStore(selActiveSession);
  const metrics = useStore((s) => (s.activeId ? s.metrics[s.activeId] : null));
  const alive = useStore((s) => s.alive);
  const restartSession = useStore((s) => s.restartSession);

  if (!session) return null;
  const status = metrics?.status ?? session.status;
  if (status !== "exited" && status !== "error") return null;
  if (alive.includes(session.id)) return null;

  return (
    <div className="session-ended">
      <span>
        <b>Sesión terminada.</b> Esto es el registro de la anterior; no acepta escritura.
      </span>
      <button className="chip btn" onClick={() => void restartSession(session.id, false)}>
        <IconPlay width={12} height={12} /> Relanzar
      </button>
      {session.external_id && (
        <button className="chip btn" onClick={() => void restartSession(session.id, true)}>
          <IconRefresh width={12} height={12} /> Reanudar
        </button>
      )}
    </div>
  );
}

const EMPTY_KEYS: [string, string[]][] = [
  ["Nueva sesión", ["Ctrl", "Shift", "T"]],
  ["Cambiar de sesión", ["Ctrl", "Tab"]],
  ["Panel lateral", ["Ctrl", "Shift", "B"]],
  ["Recargar configuración", ["Ctrl", "Shift", "R"]],
];

function EmptyState() {
  const setDialog = useStore((s) => s.setDialog);
  return (
    <div className="empty">
      <div>
        <span className="empty-mark">›</span>
        <h2>Sin sesiones abiertas</h2>
        <p>Lanza Claude Code, Codex, OpenCode o una terminal y sigue sus tokens en tiempo real.</p>
        <p style={{ marginTop: 18 }}>
          <button className="btn primary" onClick={() => setDialog("new-session")}>
            <IconPlus width={13} height={13} /> Nueva sesión
          </button>
        </p>
        <div className="empty-keys">
          {EMPTY_KEYS.map(([label, combo]) => (
            <span key={label} className="empty-key">
              <span className="keys">
                {combo.map((k, i) => (
                  <Fragment key={i}>
                    {i > 0 && <span className="kbd-sep">+</span>}
                    <kbd className="kbd">{k}</kbd>
                  </Fragment>
                ))}
              </span>
              {label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
