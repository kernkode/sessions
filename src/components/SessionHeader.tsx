import { useEffect, useState } from "react";


import { fmtDuration, shortPath } from "../lib/format";
import { api } from "../lib/ipc";
import { useT } from "../lib/i18n";
import { selActiveMetrics, selActiveSession, useStore } from "../state/store";
import { IconBranch, IconPlay, IconRefresh, IconSearch, IconStop, IconTerminal, IconTrash } from "./Icons";

/** Uptime that ticks every second, interpolating between metric publishes.
 *  The backend only refreshes metrics every ~2 s, so without interpolation the
 *  clock would visibly jump two seconds at a time. */
function Uptime({ ms }: { ms?: number }) {
  const t = useT();
  const [base, setBase] = useState({ ms: ms ?? 0, at: Date.now() });
  const [, setTick] = useState(0);
  useEffect(() => {
    setBase({ ms: ms ?? 0, at: Date.now() });
  }, [ms]);
  useEffect(() => {
    const id = window.setInterval(() => setTick((x) => x + 1), 1000);
    return () => window.clearInterval(id);
  }, []);
  const value = base.ms + (Date.now() - base.at);
  return <span className="chip mono" title={t("hdr.uptime")}>{fmtDuration(value)}</span>;
}

/** Rama + deshacer/rehacer del repo de la sesión (checkpoints git). */
function GitBar({ cwd }: { cwd: string }) {
  const notify = useStore((s) => s.notify);
  const t = useT();
  const [st, setSt] = useState<[boolean, string] | null>(null);
  const refresh = () => {
    api
      .gitStatus(cwd)
      .then(setSt)
      .catch(() => setSt(null));
  };
  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd]);
  if (!st || !st[1]) return null;
  const [dirty, branch] = st;
  const act = async (fn: () => Promise<string>, msg: string) => {
    if (dirty && !window.confirm(t("hdr.undoConfirm")))
      return;
    try {
      await fn();
      notify(msg);
    } catch (e) {
      notify(String(e));
    }
    refresh();
  };
  return (
    <span className="chip" title={dirty ? t("hdr.dirty") : t("hdr.clean")}>
      <IconBranch width={11} height={11} />
      {branch}
      {dirty ? " •" : ""}
      <button title={t("hdr.undo")} onClick={() => void act(() => api.gitUndo(cwd), t("hdr.undone"))}>
        ↩
      </button>
      <button title={t("hdr.redo")} onClick={() => void act(() => api.gitRedo(cwd), t("hdr.redone"))}>
        ↪
      </button>
    </span>
  );
}

export function SessionHeader() {
  const session = useStore(selActiveSession);
  const m = useStore(selActiveMetrics);
  const projects = useStore((s) => s.projects);
  const agents = useStore((s) => s.agents);
  const stopSession = useStore((s) => s.stopSession);
  const restartSession = useStore((s) => s.restartSession);
  const closeSession = useStore((s) => s.closeSession);
  const clearTerminal = useStore((s) => s.clearTerminal);
  const setDialog = useStore((s) => s.setDialog);
  const renameSession = useStore((s) => s.renameSession);
  const t = useT();

  if (!session) {
    return (
      <div className="header">
        <div className="crumbs">
          <span className="crumb dim">{t("hdr.none")}</span>
        </div>
      </div>
    );
  }

  const project = projects.find((p) => p.id === session.project_id);
  const agent = agents.find((a) => a.id === session.agent_id);
  const status = m?.status ?? session.status;
  const live = status !== "exited" && status !== "error";

  return (
    <div className="header">
      <div className="crumbs">
        <span className={`dot ${status}`} />
        <span
          className="crumb"
          title={t("hdr.renameTip")}
          onDoubleClick={() => {
            const nt = window.prompt(t("hdr.renameTitle"), session.title);
            if (nt && nt.trim()) void renameSession(session.id, nt.trim());
          }}
        >
          {session.title}
        </span>
        <span className="crumb dim">/</span>
        <span className="crumb dim" title={project?.path ?? session.cwd}>
          {project?.name ?? shortPath(session.cwd, 28)}
        </span>
      </div>

      <span className="chip" title={t("hdr.agent", { a: agent?.name ?? session.agent_id })}>
        <span className="dot" style={{ background: agent?.color ?? "var(--txt-3)", boxShadow: "none" }} />
        {agent?.name ?? session.agent_id}
      </span>

      {m?.model && (
        <span className="chip mono" title={t("hdr.model")}>
          {m.model}
        </span>
      )}
      {m?.effort && (
        <span className="chip mono" title={t("hdr.effort")}>
          {m.effort}
        </span>
      )}
      <GitBar cwd={session.cwd} />
      <Uptime ms={m?.uptime_ms} />

      <div className="sep" />

      <button
        className="icon-btn"
        title={t("hdr.search")}
        onClick={() => setDialog("search")}
      >
        <IconSearch />
      </button>
      <button
        className="icon-btn"
        title={t("hdr.clear")}
        onClick={() => void clearTerminal(session.id)}
      >
        <IconTerminal />
      </button>
      {live ? (
        <button
          className="chip btn danger"
          title={t("hdr.stopTip")}
          onClick={() => void stopSession(session.id)}
        >
          <IconStop width={13} height={13} /> {t("hdr.stop")}
        </button>
      ) : (
        <>
          <button
            className="chip btn"
            title={t("hdr.relaunchTip")}
            onClick={() => void restartSession(session.id, false)}
          >
            <IconPlay width={13} height={13} /> {t("hdr.relaunch")}
          </button>
          {session.external_id && (
            <button
              className="chip btn"
              title={t("hdr.resumeTip", { id: session.external_id })}
              onClick={() => void restartSession(session.id, true)}
            >
              <IconRefresh width={13} height={13} /> {t("hdr.resume")}
            </button>
          )}
        </>
      )}
      <button
        className="icon-btn danger"
        title="Cerrar y quitar de la lista"
        onClick={() => void closeSession(session.id, false)}
      >
        <IconTrash />
      </button>
    </div>
  );
}
