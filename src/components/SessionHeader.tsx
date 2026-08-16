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
    if (dirty && !window.confirm("Hay cambios sin commitear; la operación los descartará. ¿Continuar?"))
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
    <span className="chip" title={dirty ? "cambios sin commitear" : "árbol limpio"}>
      <IconBranch width={11} height={11} />
      {branch}
      {dirty ? " •" : ""}
      <button title="Deshacer (checkpoint anterior)" onClick={() => void act(() => api.gitUndo(cwd), "Deshecho")}>
        ↩
      </button>
      <button title="Rehacer" onClick={() => void act(() => api.gitRedo(cwd), "Rehecho")}>
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
          <span className="crumb dim">Ninguna sesión seleccionada</span>
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
          title="Doble clic para renombrar"
          onDoubleClick={() => {
            const t = window.prompt("Título de la sesión", session.title);
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

      <span className="chip" title={`Agente: ${agent?.name ?? session.agent_id}`}>
        <span className="dot" style={{ background: agent?.color ?? "var(--txt-3)", boxShadow: "none" }} />
        {agent?.name ?? session.agent_id}
      </span>

      {m?.model && (
        <span className="chip mono" title="Modelo que informa el CLI">
          {m.model}
        </span>
      )}
      {m?.effort && (
        <span className="chip mono" title="Esfuerzo de razonamiento del CLI">
          {m.effort}
        </span>
      )}
      <GitBar cwd={session.cwd} />
      <span className="chip mono" title="Tiempo de sesión">
        {fmtDuration(m?.uptime_ms ?? 0)}
      </span>

      <div className="sep" />

      <button
        className="icon-btn"
        title="Buscar en el terminal (Ctrl+Shift+F)"
        onClick={() => setDialog("search")}
      >
        <IconSearch />
      </button>
      <button
        className="icon-btn"
        title="Limpiar terminal (Ctrl+Shift+K)"
        onClick={() => void clearTerminal(session.id)}
      >
        <IconTerminal />
      </button>
      {live ? (
        <button
          className="chip btn danger"
          title="Detener proceso"
          onClick={() => void stopSession(session.id)}
        >
          <IconStop width={13} height={13} /> Detener
        </button>
      ) : (
        <>
          <button
            className="chip btn"
            title="Volver a lanzar"
            onClick={() => void restartSession(session.id, false)}
          >
            <IconPlay width={13} height={13} /> Relanzar
          </button>
          {session.external_id && (
            <button
              className="chip btn"
              title={`Reanudar la sesión ${session.external_id} del CLI`}
              onClick={() => void restartSession(session.id, true)}
            >
              <IconRefresh width={13} height={13} /> Reanudar
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
