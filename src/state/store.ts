// Global state. zustand with narrow selectors so terminal output (which never
// goes through React) and metrics do not trigger cascading re-renders.
import { create } from "zustand";

import { api, onMetrics, onSessionExit } from "../lib/ipc";
import { pool } from "../term/pool";
import type {
  AgentStatus,
  Bootstrap,
  ConfigSnapshot,
  CreateSessionRequest,
  Project,
  SessionMeta,
  SessionMetrics,
} from "../lib/types";

export type Dialog = null | "new-session" | "settings" | "palette" | "search";

interface State {
  ready: boolean;
  error: string | null;
  config: ConfigSnapshot | null;
  agents: AgentStatus[];
  platform: string;
  home: string | null;
  version: string;

  projects: Project[];
  sessions: SessionMeta[];
  metrics: Record<string, SessionMetrics>;
  activeId: string | null;
  /** Sessions with a live process. */
  alive: string[];

  sidebarOpen: boolean;
  metricsOpen: boolean;
  dialog: Dialog;
  notice: string | null;

  init: () => Promise<void>;
  resumeSaved: () => Promise<void>;
  reloadConfig: () => Promise<void>;
  setActive: (id: string | null) => Promise<void>;
  createSession: (req: CreateSessionRequest) => Promise<SessionMeta | null>;
  closeSession: (id: string, keep?: boolean) => Promise<void>;
  stopSession: (id: string) => Promise<void>;
  restartSession: (id: string, resume: boolean) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  clearTerminal: (id: string) => Promise<void>;
  addProject: (path: string, name?: string) => Promise<Project | null>;
  removeProject: (id: string) => Promise<void>;
  toggleProject: (id: string) => Promise<void>;
  setDialog: (d: Dialog) => void;
  toggleSidebar: () => void;
  toggleMetrics: () => void;
  notify: (msg: string | null) => void;
  cycleSession: (delta: number) => Promise<void>;
}

function applyBootstrap(b: Bootstrap) {
  pool.setConfig(b.config.app.terminal, b.config.app.performance.max_live_terminals);
  return {
    ready: true,
    config: b.config,
    agents: b.agents,
    platform: b.platform,
    home: b.home,
    version: b.version,
    projects: b.projects,
    sessions: b.sessions,
  };
}

export const useStore = create<State>((set, get) => ({
  ready: false,
  error: null,
  config: null,
  agents: [],
  platform: "",
  home: null,
  version: "",

  projects: [],
  sessions: [],
  metrics: {},
  activeId: null,
  alive: [],

  sidebarOpen: true,
  metricsOpen: true,
  dialog: null,
  notice: null,

  async init() {
    try {
      const b = await api.bootstrap();
      set(applyBootstrap(b));

      const alive = await api.sessionActiveIds();
      set({ alive });

      // Typing into a session whose process died: explain it instead of silently
      // dropping the keystroke.
      pool.setInputRejectedHandler((id) => {
        const s = get();
        if (s.alive.includes(id)) return;
        s.notify("La sesión terminó. Pulsa Relanzar o Reanudar para volver a abrirla.");
      });

      // Metrics live in their own map so lists are not re-rendered.
      await onMetrics((m) => {
        set((s) => ({
          metrics: { ...s.metrics, [m.session_id]: m },
          sessions: s.sessions.map((x) =>
            x.id === m.session_id && x.status !== m.status ? { ...x, status: m.status } : x,
          ),
        }));
      });

      await onSessionExit(({ session_id, code }) => {
        set((s) => ({
          sessions: s.sessions.map((x) =>
            x.id === session_id ? { ...x, status: "exited", exit_code: code, pid: null } : x,
          ),
          alive: s.alive.filter((v) => v !== session_id),
        }));
        pool.write(
          session_id,
          `\r\n\x1b[38;5;244m── proceso terminado (código ${code}) ──\x1b[0m\r\n`,
        );
        pool.collect(new Set(get().alive));
      });

      // Bring the saved sessions back, so the user finds their agents already
      // running instead of having to press «Reanudar».
      await get().resumeSaved();
    } catch (e) {
      set({ error: String(e), ready: true });
    }
  },

  /**
   * Relaunches the saved sessions on startup. Agents that support it resume
   * their conversation (`--resume <id>`); the rest start again in the same
   * directory. The scope comes from `[app]` in config.toml.
   */
  async resumeSaved() {
    let result;
    try {
      result = await api.sessionsAutoresume();
    } catch (e) {
      get().notify(String(e));
      return;
    }
    if (result.sessions.length === 0 && result.failed.length === 0) return;

    // A relaunched session replaces its previous record: the backend gives it a
    // new id and drops the old one.
    const resumed = new Set(result.sessions.map((s) => s.id));
    const replacedKey = new Set(result.sessions.map((r) => r.project_id + "\u0000" + r.title));
    set((s) => ({
      sessions: [
        ...s.sessions.filter(
          (x) => !resumed.has(x.id) && !replacedKey.has(x.project_id + "\u0000" + x.title),
        ),
        ...result.sessions,
      ],
      alive: [...s.alive, ...result.sessions.map((x) => x.id)],
    }));

    // Every live session needs its emulator: ConPTY emits nothing until someone
    // answers its `ESC[6n`, and that answer comes from xterm.
    for (const meta of result.sessions) {
      try {
        await pool.ensure(meta.id, { live: true });
      } catch {
        // A terminal that cannot be created is no reason to abort startup.
      }
    }

    if (result.failed.length > 0) {
      const [title, reason] = result.failed[0];
      get().notify("No se pudo reanudar «" + title + "»: " + reason);
    } else if (result.sessions.length > 1) {
      get().notify(result.sessions.length + " sesiones reanudadas");
    }

    // The resumed sessions carry new ids, so whatever was selected before no
    // longer exists. They come oldest first: the last one is the most recent.
    const newest = result.sessions[result.sessions.length - 1];
    if (newest) await get().setActive(newest.id);
  },

  async reloadConfig() {
    try {
      const config = await api.configReload();
      pool.setConfig(config.app.terminal, config.app.performance.max_live_terminals);
      set({ config });
      const issues = config.issues.length;
      get().notify(
        issues > 0 ? `Configuración recargada con ${issues} aviso(s)` : "Configuración recargada",
      );
    } catch (e) {
      get().notify(String(e));
    }
  },

  async setActive(id) {
    // Guard against a stale id: relaunching replaces the session's id, and the
    // selection could still point at the one that is gone.
    if (id && !get().sessions.some((s) => s.id === id)) {
      set({ activeId: null });
      return;
    }
    set({ activeId: id });
    if (!id) return;
    try {
      const meta = get().sessions.find((s) => s.id === id);
      // A session with no live process is rehydrated by replaying its history at
      // the size it had; a live one receives the bytes untouched.
      await pool.show(id, {
        live: get().alive.includes(id),
        cols: meta?.cols,
        rows: meta?.rows,
      });
      pool.collect(new Set(get().alive));
      const m = await api.sessionMetrics(id);
      if (m) set((s) => ({ metrics: { ...s.metrics, [id]: m } }));
    } catch (e) {
      get().notify(String(e));
    }
  },

  async createSession(req) {
    try {
      const meta = await api.sessionCreate(req);
      set((s) => ({
        sessions: [...s.sessions.filter((x) => x.id !== meta.id), meta],
        alive: [...s.alive, meta.id],
      }));
      // If the project is new, refresh the list.
      if (!get().projects.some((p) => p.id === meta.project_id)) {
        set({ projects: await api.projectList() });
      }
      await get().setActive(meta.id);
      return meta;
    } catch (e) {
      get().notify(String(e));
      return null;
    }
  },

  async closeSession(id, keep = false) {
    try {
      await api.sessionClose(id, keep);
    } catch (e) {
      get().notify(String(e));
    }
    pool.dispose(id);
    const remaining = get().sessions.filter((s) => (keep ? true : s.id !== id));
    set((s) => ({
      sessions: keep
        ? s.sessions.map((x) => (x.id === id ? { ...x, status: "exited", pid: null } : x))
        : remaining,
      alive: s.alive.filter((v) => v !== id),
      metrics: keep
        ? s.metrics
        : Object.fromEntries(Object.entries(s.metrics).filter(([k]) => k !== id)),
    }));
    if (get().activeId === id) {
      const next = get().sessions.find((s) => s.id !== id) ?? null;
      await get().setActive(next ? next.id : null);
    }
  },

  async stopSession(id) {
    try {
      await api.sessionKill(id);
    } catch (e) {
      get().notify(String(e));
    }
  },

  async restartSession(id, resume) {
    try {
      const previous = get().sessions.find((s) => s.id === id);
      const meta = await api.sessionRestart(id, resume);
      pool.dispose(id);
      set((s) => ({
        sessions: [...s.sessions.filter((x) => x.id !== id), meta],
        alive: [...s.alive.filter((v) => v !== id), meta.id],
        metrics: Object.fromEntries(Object.entries(s.metrics).filter(([k]) => k !== id)),
      }));
      if (previous && get().activeId === id) await get().setActive(meta.id);
    } catch (e) {
      get().notify(String(e));
    }
  },

  async renameSession(id, title) {
    await api.sessionSetTitle(id, title);
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? { ...x, title } : x)) }));
  },

  async clearTerminal(id) {
    pool.clear(id);
    await api.sessionClear(id);
  },

  async addProject(path, name) {
    try {
      const p = await api.projectAdd(path, name);
      set((s) => ({ projects: [...s.projects.filter((x) => x.id !== p.id), p] }));
      return p;
    } catch (e) {
      get().notify(String(e));
      return null;
    }
  },

  async removeProject(id) {
    const removed = await api.projectRemove(id);
    for (const sid of removed) pool.dispose(sid);
    set((s) => ({
      projects: s.projects.filter((p) => p.id !== id),
      sessions: s.sessions.filter((x) => x.project_id !== id),
      alive: s.alive.filter((v) => !removed.includes(v)),
    }));
    if (get().activeId && removed.includes(get().activeId!)) {
      await get().setActive(get().sessions[0]?.id ?? null);
    }
  },

  async toggleProject(id) {
    const p = get().projects.find((x) => x.id === id);
    if (!p) return;
    const collapsed = !p.collapsed;
    set((s) => ({ projects: s.projects.map((x) => (x.id === id ? { ...x, collapsed } : x)) }));
    await api.projectSetCollapsed(id, collapsed);
  },

  setDialog: (d) => set({ dialog: d }),
  toggleSidebar: () => {
    set((s) => ({ sidebarOpen: !s.sidebarOpen }));
    requestAnimationFrame(() => pool.fit());
  },
  toggleMetrics: () => {
    set((s) => ({ metricsOpen: !s.metricsOpen }));
    requestAnimationFrame(() => pool.fit());
  },
  notify: (msg) => {
    set({ notice: msg });
    if (msg) window.setTimeout(() => set((s) => (s.notice === msg ? { notice: null } : s)), 4000);
  },

  async cycleSession(delta) {
    const { sessions, activeId } = get();
    if (sessions.length === 0) return;
    const i = sessions.findIndex((s) => s.id === activeId);
    const next = sessions[(i + delta + sessions.length) % sessions.length];
    await get().setActive(next.id);
  },
}));

// Reusable selectors.
export const selActiveSession = (s: State) => s.sessions.find((x) => x.id === s.activeId) ?? null;
export const selActiveMetrics = (s: State) => (s.activeId ? s.metrics[s.activeId] ?? null : null);
