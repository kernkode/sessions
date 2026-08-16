import { useEffect, useState } from "react";

import { fmtDuration, shortPath } from "../lib/format";
import { api } from "../lib/ipc";
import { selActiveMetrics, selActiveSession, useStore } from "../state/store";
import { IconBranch, IconPlay, IconRefresh, IconSearch, IconStop, IconTerminal, IconTrash } from "./Icons";

/** Rama + deshacer/rehacer del repo de la sesión (checkpoints git). */
function GitBar({ cwd }: { cwd: string }) {
  const notify = useStore((s) => s.notify);
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
    if (dirty && !window.confirm("There are uncommitted changes; this will discard them. Continue?"))
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
    <span className="chip" title={dirty ? "uncommitted changes" : "clean tree"}>
      <IconBranch width={11} height={11} />
      {branch}
      {dirty ? " •" : ""}
      <button title="Undo (previous checkpoint)" onClick={() => void act(() => api.gitUndo(cwd), "Undone")}>
        ↩
      </button>
      <button title="Redo" onClick={() => void act(() => api.gitRedo(cwd), "Redone")}>
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

  if (!session) {
    return (
      <div className="header">
        <div className="crumbs">
          <span className="crumb dim">No session selected</span>
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
          title="Double-click to rename"
          onDoubleClick={() => {
            const t = window.prompt("Session title", session.title);
            if (t && t.trim()) void renameSession(session.id, t.trim());
          }}
        >
          {session.title}
        </span>
        <span className="crumb dim">/</span>
        <span className="crumb dim" title={project?.path ?? session.cwd}>
          {project?.name ?? shortPath(session.cwd, 28)}
        </span>
      </div>

      <span className="chip" title={`Agent: ${agent?.name ?? session.agent_id}`}>
        <span className="dot" style={{ background: agent?.color ?? "var(--txt-3)", boxShadow: "none" }} />
        {agent?.name ?? session.agent_id}
      </span>

      {m?.model && (
        <span className="chip mono" title="Model reported by the CLI">
          {m.model}
        </span>
      )}
      {m?.effort && (
        <span className="chip mono" title="CLI reasoning effort">
          {m.effort}
        </span>
      )}
      <GitBar cwd={session.cwd} />
      <span className="chip mono" title="Session uptime">
        {fmtDuration(m?.uptime_ms ?? 0)}
      </span>

      <div className="sep" />

      <button
        className="icon-btn"
        title="Search in terminal (Ctrl+Shift+F)"
        onClick={() => setDialog("search")}
      >
        <IconSearch />
      </button>
      <button
        className="icon-btn"
        title="Clear terminal (Ctrl+Shift+K)"
        onClick={() => void clearTerminal(session.id)}
      >
        <IconTerminal />
      </button>
      {live ? (
        <button
          className="chip btn danger"
          title="Stop process"
          onClick={() => void stopSession(session.id)}
        >
          <IconStop width={13} height={13} /> Stop
        </button>
      ) : (
        <>
          <button
            className="chip btn"
            title="Relaunch"
            onClick={() => void restartSession(session.id, false)}
          >
            <IconPlay width={13} height={13} /> Relaunch
          </button>
          {session.external_id && (
            <button
              className="chip btn"
              title={`Resume CLI session ${session.external_id}`}
              onClick={() => void restartSession(session.id, true)}
            >
              <IconRefresh width={13} height={13} /> Resume
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
