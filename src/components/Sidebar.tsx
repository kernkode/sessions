import { useEffect, useMemo, useState } from "react";
import type { CSSProperties } from "react";

import { fmtRate, shortId, shortPath } from "../lib/format";
import { api } from "../lib/ipc";
import { useT } from "../lib/i18n";
import type { SessionMeta } from "../lib/types";
import { useStore } from "../state/store";
import { IconBranch, IconChevron, IconEdit, IconPlus, IconSearch, IconTrash } from "./Icons";

// HEAD commit por cwd, para no volver a preguntar por cada tarjeta igual.
const gitHeadCache = new Map<string, string | null>();

/** Commit HEAD (corto) del repo que contiene el cwd de la sesión. */
function useGitHead(cwd: string): string | null {
  const [hash, setHash] = useState<string | null>(gitHeadCache.get(cwd) ?? null);
  useEffect(() => {
    let on = true;
    if (gitHeadCache.has(cwd)) {
      setHash(gitHeadCache.get(cwd) ?? null);
      return;
    }
    api
      .gitHead(cwd)
      .then((h) => {
        if (!on) return;
        gitHeadCache.set(cwd, h);
        setHash(h);
      })
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [cwd]);
  return hash;
}

export function Sidebar() {
  const projects = useStore((s) => s.projects);
  const sessions = useStore((s) => s.sessions);
  const activeId = useStore((s) => s.activeId);
  const setActive = useStore((s) => s.setActive);
  const toggleProject = useStore((s) => s.toggleProject);
  const removeProject = useStore((s) => s.removeProject);
  const renameProject = useStore((s) => s.renameProject);
  const setDialog = useStore((s) => s.setDialog);
  const t = useT();
  const [filter, setFilter] = useState("");
  // Removing a project takes its sessions with it: confirm in place.
  const [pendingRemoval, setPendingRemoval] = useState<string | null>(null);

  const groups = useMemo(() => {
    const q = filter.trim().toLowerCase();
    const matches = (s: SessionMeta) =>
      !q ||
      s.title.toLowerCase().includes(q) ||
      s.agent_id.toLowerCase().includes(q) ||
      (s.external_id ?? "").toLowerCase().includes(q);

    const byProject = new Map<string, SessionMeta[]>();
    for (const s of sessions) {
      if (!matches(s)) continue;
      const list = byProject.get(s.project_id) ?? [];
      list.push(s);
      byProject.set(s.project_id, list);
    }
    // Projects with the most recent activity first.
    return projects
      .map((p) => ({
        project: p,
        list: (byProject.get(p.id) ?? []).sort((a, b) => b.created_at - a.created_at),
      }))
      .filter((g) => !q || g.list.length > 0)
      .sort(
        (a, b) =>
          (b.list[0]?.created_at ?? b.project.created_at) -
          (a.list[0]?.created_at ?? a.project.created_at),
      );
  }, [projects, sessions, filter]);

  return (
    <aside className="sidebar">
      <div className="sidebar-top">
        <div className="sidebar-search">
          <IconSearch width={13} height={13} />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={t("sb.filter")}
            spellCheck={false}
          />
        </div>
        <button className="icon-btn" title={t("sb.addProject")} onClick={() => setDialog("new-session")}>
          <IconPlus />
        </button>
      </div>

      <div className="sidebar-list">
        {groups.length === 0 && (
          <div className="hint" style={{ padding: "10px 6px" }}>
            {t("sb.noProjects")}
          </div>
        )}

        {groups.map(({ project, list }) => (
          <div key={project.id}>
            <div className="group-head" title={project.path}>
              <button
                onClick={() => void toggleProject(project.id)}
                style={{ display: "grid", placeItems: "center", opacity: 1 }}
                title={project.collapsed ? t("sb.expand") : t("sb.collapse")}
              >
                <IconChevron open={!project.collapsed} width={13} height={13} />
              </button>
              <span className="name">{project.name}</span>
              <button
                title={t("sb.newInProject")}
                onClick={() => {
                  useStore.setState({ dialog: "new-session" });
                  window.dispatchEvent(
                    new CustomEvent("sessions:preset-project", { detail: project.path }),
                  );
                }}
              >
                <IconPlus width={13} height={13} />
              </button>
              <button
                title={t("sb.renameProject")}
                onClick={() => {
                  const n = window.prompt(t("sb.projectName"), project.name);
                  if (n && n.trim() && n.trim() !== project.name)
                    void renameProject(project.id, n.trim());
                }}
              >
                <IconEdit width={13} height={13} />
              </button>
              <button title={t("sb.removeProject")} onClick={() => setPendingRemoval(project.id)}>
                <IconTrash width={13} height={13} />
              </button>
            </div>

            {pendingRemoval === project.id && (
              <div className="confirm-row">
                <span>
                  {t("sb.confirmRemove", { name: project.name, n: list.length })}
                </span>
                <button className="chip btn" onClick={() => setPendingRemoval(null)}>
                  {t("sb.no")}
                </button>
                <button
                  className="chip btn danger"
                  onClick={() => {
                    setPendingRemoval(null);
                    void removeProject(project.id);
                  }}
                >
                  {t("sb.yes")}
                </button>
              </div>
            )}

            {!project.collapsed &&
              list.map((s) => (
                <SessionCard key={s.id} s={s} on={s.id === activeId} onClick={() => void setActive(s.id)} />
              ))}

            {!project.collapsed && list.length === 0 && (
              <div className="hint" style={{ padding: "0 6px 8px" }}>
                {shortPath(project.path, 34)}
              </div>
            )}
          </div>
        ))}
      </div>
    </aside>
  );
}

function SessionCard({ s, on, onClick }: { s: SessionMeta; on: boolean; onClick: () => void }) {
  // Narrow subscription: only this card repaints when its metrics change.
  const m = useStore((st) => st.metrics[s.id]);
  const t = useT();
  const agent = useStore((st) => st.agents.find((a) => a.id === s.agent_id));
  const gitHead = useGitHead(s.cwd);
  const status = m?.status ?? s.status;
  const label =
    status === "working"
      ? t("st.working")
      : status === "idle"
        ? t("st.idle")
        : status === "error"
          ? t("st.error")
          : t("st.ended");

  return (
    <div className={`card ${on ? "on" : ""}`} onClick={onClick} title={s.command_line ?? s.title}>
      <span className="card-tile" style={{ "--c": agent?.color } as CSSProperties}>
        {(agent?.name ?? s.agent_id).charAt(0).toUpperCase()}
      </span>
      <div className="card-body">
        <div className="card-title">{s.title}</div>
        <div className="card-sub">
          <span className="ext">{s.agent_id}</span>
          {gitHead ? (
            <>
              <IconBranch width={11} height={11} />
              <span className="ext">{gitHead}</span>
            </>
          ) : (
            s.external_id && (
              <>
                <IconBranch width={11} height={11} />
                <span className="ext">{shortId(s.external_id, 18)}</span>
              </>
            )
          )}
        </div>
        <div className={`card-state ${status}`}>
          <span className={`dot ${status}`} />
          {label}
          {m && m.tokens_per_second > 0 && (
            <span className="card-tps">{fmtRate(m.tokens_per_second)} tok/s</span>
          )}
        </div>
      </div>
    </div>
  );
}
