import { Fragment, useEffect, useRef } from "react";

import { CommandPalette } from "./components/CommandPalette";
import { MetricsBar } from "./components/MetricsBar";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { SearchBar } from "./components/SearchBar";
import { SessionHeader } from "./components/SessionHeader";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { IconPlay, IconPlus, IconRefresh } from "./components/Icons";
import { useT } from "./lib/i18n";
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
      if (e.key === "k") {
        take();
        s.setDialog("palette");
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
    return <div className="loading">{<LoadingText />}</div>;
  }

  return (
    <div className="app">
      <TitleBar />
      <SessionHeader />
      <div className="body">
        {sidebarOpen && <Sidebar />}
        <div className="console-col">
          <main>
            <div id="term-host" ref={host} />
            {!activeId && <EmptyState />}
            <EndedBanner />
            {dialog === "search" && <SearchBar />}
          </main>
          {metricsOpen && <MetricsBar />}
        </div>
      </div>

      {dialog === "new-session" && <NewSessionDialog />}
      {dialog === "settings" && <SettingsDialog />}
      {dialog === "palette" && <CommandPalette />}

      {(notice || error) && <div className="toast">{error ?? notice}</div>}
    </div>
  );
}

/**
 * A session whose process ended shows its saved output as a transcript, but
 * nothing can be typed into it. This makes that explicit and puts the two ways
 * out within reach.
 */
function LoadingText() {
  const t = useT();
  return <>{t("app.loading")}</>;
}

function EndedBanner() {
  const session = useStore(selActiveSession);
  const metrics = useStore((s) => (s.activeId ? s.metrics[s.activeId] : null));
  const alive = useStore((s) => s.alive);
  const restartSession = useStore((s) => s.restartSession);
  const t = useT();

  if (!session) return null;
  const status = metrics?.status ?? session.status;
  if (status !== "exited" && status !== "error") return null;
  if (alive.includes(session.id)) return null;

  return (
    <div className="session-ended">
      <span>
        <b>{t("end.title")}</b> {t("end.body")}
      </span>
      <button className="chip btn" onClick={() => void restartSession(session.id, false)}>
        <IconPlay width={12} height={12} /> {t("end.relaunch")}
      </button>
      {session.external_id && (
        <button className="chip btn" onClick={() => void restartSession(session.id, true)}>
          <IconRefresh width={12} height={12} /> {t("end.resume")}
        </button>
      )}
    </div>
  );
}

const EMPTY_KEYS: [string, string[]][] = [
  ["empty.k.new", ["Ctrl", "Shift", "T"]],
  ["empty.k.switch", ["Ctrl", "Tab"]],
  ["empty.k.sidebar", ["Ctrl", "Shift", "B"]],
  ["empty.k.reload", ["Ctrl", "Shift", "R"]],
];

function EmptyState() {
  const setDialog = useStore((s) => s.setDialog);
  const t = useT();
  return (
    <div className="empty">
      <div>
        <span className="empty-mark">›</span>
        <h2>{t("empty.title")}</h2>
        <p>{t("empty.body")}</p>
        <p style={{ marginTop: 18 }}>
          <button className="btn primary" onClick={() => setDialog("new-session")}>
            <IconPlus width={13} height={13} /> {t("empty.new")}
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
              {t(label)}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
