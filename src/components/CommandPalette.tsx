// Command palette (Ctrl+K): jump to sessions and run global actions from the
// keyboard, Raycast-style. Lives outside the terminal so it never steals PTY
// keys while closed.
import { useEffect, useMemo, useRef, useState } from "react";

import { useStore } from "../state/store";
import { useT } from "../lib/i18n";

interface Cmd {
  id: string;
  label: string;
  hint?: string;
  run: () => void;
}

export function CommandPalette() {
  const close = () => useStore.getState().setDialog(null);
  const sessions = useStore((s) => s.sessions);
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const metricsOpen = useStore((s) => s.metricsOpen);
  const [q, setQ] = useState("");
  const [hi, setHi] = useState(0);
  const t = useT();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const cmds = useMemo<Cmd[]>(() => {
    const st = useStore.getState();
    const actions: Cmd[] = [
      { id: "new", label: t("pal.new"), hint: "Ctrl+Shift+T", run: () => st.setDialog("new-session") },
      { id: "settings", label: t("pal.settings"), hint: "Ctrl+,", run: () => st.setDialog("settings") },
      {
        id: "sidebar",
        label: sidebarOpen ? t("pal.hideSidebar") : t("pal.showSidebar"),
        hint: "Ctrl+Shift+B",
        run: () => st.toggleSidebar(),
      },
      {
        id: "metrics",
        label: metricsOpen ? t("pal.hideMetrics") : t("pal.showMetrics"),
        hint: "Ctrl+Shift+M",
        run: () => st.toggleMetrics(),
      },
      { id: "reload", label: t("pal.reload"), hint: "Ctrl+Shift+R", run: () => void st.reloadConfig() },
    ];
    const jumps: Cmd[] = sessions.map((s) => ({
      id: "s:" + s.id,
      label: t("pal.goTo", { t: s.title }),
      hint: s.agent_id,
      run: () => void st.setActive(s.id),
    }));
    return [...actions, ...jumps];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessions, sidebarOpen, metricsOpen, t]);

  const list = useMemo(() => {
    const t = q.trim().toLowerCase();
    return t ? cmds.filter((c) => c.label.toLowerCase().includes(t)) : cmds;
  }, [cmds, q]);

  useEffect(() => setHi(0), [q]);

  const runAt = (i: number) => {
    const c = list[i];
    if (!c) return;
    close();
    c.run();
  };

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="dialog" style={{ width: "min(560px, calc(100vw - 48px))" }}>
        <div className="dialog-head" style={{ padding: 10 }}>
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t("pal.placeholder")}
            spellCheck={false}
            style={{ flex: 1 }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setHi((h) => Math.min(h + 1, list.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setHi((h) => Math.max(h - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                runAt(hi);
              } else if (e.key === "Escape") {
                close();
              }
            }}
          />
        </div>
        <div className="dialog-body" style={{ padding: 6 }}>
          {list.length === 0 && <div className="hint" style={{ padding: 8 }}>{t("pal.noResults")}</div>}
          {list.map((c, i) => (
            <div
              key={c.id}
              className={`pick ${i === hi ? "on" : ""}`}
              onMouseEnter={() => setHi(i)}
              onClick={() => runAt(i)}
            >
              <span style={{ flex: 1 }}>{c.label}</span>
              {c.hint && <span className="badge">{c.hint}</span>}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
