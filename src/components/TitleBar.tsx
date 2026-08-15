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
          <span className="tab on">Sesiones</span>
          <span className="chip" title="Sesiones con proceso vivo">
            <span className={`dot ${aliveCount > 0 ? "working" : "exited"}`} />
            {aliveCount} activa{aliveCount === 1 ? "" : "s"}
          </span>
        </div>
      </div>

      <button
        className="icon-btn acc"
        title="Nueva sesión (Ctrl+Shift+T)"
        onClick={() => setDialog("new-session")}
      >
        <IconPlus />
      </button>
      <button className="icon-btn" title="Barra lateral (Ctrl+Shift+B)" onClick={toggleSidebar}>
        <IconPanel />
      </button>
      <button className="icon-btn" title="Métricas (Ctrl+Shift+M)" onClick={toggleMetrics}>
        <IconChart />
      </button>
      <button className="icon-btn" title={`Ajustes · v${version}`} onClick={() => setDialog("settings")}>
        <IconGear />
      </button>

      <div className="win-btns">
        <button className="win-btn" onClick={() => void win.minimize()} title="Minimizar">
          <IconMin />
        </button>
        <button className="win-btn" onClick={() => void win.toggleMaximize()} title="Maximizar">
          <IconMax />
        </button>
        <button
          className="win-btn close"
          title="Cerrar"
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
