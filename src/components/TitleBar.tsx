import { getCurrentWindow } from "@tauri-apps/api/window";

import { api } from "../lib/ipc";
import { useT } from "../lib/i18n";
import { useStore } from "../state/store";
import { IconChart, IconGear, IconMax, IconMin, IconPanel, IconPlus, IconX } from "./Icons";

const win = getCurrentWindow();

export function TitleBar() {
  const version = useStore((s) => s.version);
  const aliveCount = useStore((s) => s.alive.length);
  const setDialog = useStore((s) => s.setDialog);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const toggleMetrics = useStore((s) => s.toggleMetrics);
  const t = useT();

  return (
    <div className="titlebar">
      <div className="drag" data-tauri-drag-region>
        <div className="brand">
          <span className="brand-mark">›</span>
          Sessions
        </div>
        <div className="tabs">
          <span className="tab on">{t("tb.sessions")}</span>
          <span className="chip" title={t("tb.liveTip")}>
            <span className={`dot ${aliveCount > 0 ? "working" : "exited"}`} />
            {aliveCount === 1 ? t("tb.liveOne") : t("tb.liveMany", { n: aliveCount })}
          </span>
        </div>
      </div>

      <button
        className="icon-btn acc"
        title={t("tb.new")}
        onClick={() => setDialog("new-session")}
      >
        <IconPlus />
      </button>
      <button className="icon-btn" title={t("tb.sidebar")} onClick={toggleSidebar}>
        <IconPanel />
      </button>
      <button className="icon-btn" title={t("tb.metrics")} onClick={toggleMetrics}>
        <IconChart />
      </button>
      <button className="icon-btn" title={t("tb.settings", { v: version })} onClick={() => setDialog("settings")}>
        <IconGear />
      </button>

      <div className="win-btns">
        <button className="win-btn" onClick={() => void win.minimize()} title={t("tb.min")}>
          <IconMin />
        </button>
        <button className="win-btn" onClick={() => void win.toggleMaximize()} title={t("tb.max")}>
          <IconMax />
        </button>
        <button
          className="win-btn close"
          title={t("tb.close")}
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
