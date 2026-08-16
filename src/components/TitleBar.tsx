import { getCurrentWindow } from "@tauri-apps/api/window";

import { api } from "../lib/ipc";
import { useStore } from "../state/store";
import { IconChart, IconGear, IconMax, IconMin, IconPanel, IconPlus, IconX } from "./Icons";

const win = getCurrentWindow();

export function TitleBar() {
  const version = useStore((s) => s.version);
  const aliveCount = useStore((s) => s.alive.length);
  const setDialog = useStore((s) => s.setDialog);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const toggleMetrics = useStore((s) => s.toggleMetrics);

  return (
    <div className="titlebar">
      <div className="drag" data-tauri-drag-region>
        <div className="brand">
          <span className="brand-mark">›</span>
          Sessions
        </div>
        <div className="tabs">
          <span className="tab on">Sessions</span>
          <span className="chip" title="Sessions with a live process">
            <span className={`dot ${aliveCount > 0 ? "working" : "exited"}`} />
            {aliveCount} live
          </span>
        </div>
      </div>

      <button
        className="icon-btn acc"
        title="New session (Ctrl+Shift+T)"
        onClick={() => setDialog("new-session")}
      >
        <IconPlus />
      </button>
      <button className="icon-btn" title="Sidebar (Ctrl+Shift+B)" onClick={toggleSidebar}>
        <IconPanel />
      </button>
      <button className="icon-btn" title="Metrics (Ctrl+Shift+M)" onClick={toggleMetrics}>
        <IconChart />
      </button>
      <button className="icon-btn" title={`Settings · v${version}`} onClick={() => setDialog("settings")}>
        <IconGear />
      </button>

      <div className="win-btns">
        <button className="win-btn" onClick={() => void win.minimize()} title="Minimize">
          <IconMin />
        </button>
        <button className="win-btn" onClick={() => void win.toggleMaximize()} title="Maximize">
          <IconMax />
        </button>
        <button
          className="win-btn close"
          title="Close"
          onClick={async () => {
            // Orderly shutdown: saves scrollback and kills the child processes.
            await api.appShutdown().catch(() => {});
            await win.close();
          }}
        >
          <IconX />
        </button>
      </div>
    </div>
  );
}
