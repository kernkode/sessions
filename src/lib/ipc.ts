// The single bridge to the backend: every call and event goes through here.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AutoResume,
  Bootstrap,
  ConfigPaths,
  ConfigSnapshot,
  CreateSessionRequest,
  DirEntry,
  Project,
  SessionMeta,
  SessionMetrics,
} from "./types";

export const EV_EXIT = "session:exit";
export const EV_METRICS = "session:metrics";

export const api = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  configReload: () => invoke<ConfigSnapshot>("config_reload"),
  configPaths: () => invoke<ConfigPaths>("config_paths"),

  projectAdd: (path: string, name?: string) => invoke<Project>("project_add", { path, name }),
  projectRename: (id: string, name: string) => invoke<boolean>("project_rename", { id, name }),
  projectSetCollapsed: (id: string, collapsed: boolean) =>
    invoke<void>("project_set_collapsed", { id, collapsed }),
  projectRemove: (id: string) => invoke<string[]>("project_remove", { id }),
  projectList: () => invoke<Project[]>("project_list"),

  sessionList: () => invoke<SessionMeta[]>("session_list"),
  sessionCreate: (req: CreateSessionRequest) => invoke<SessionMeta>("session_create", { req }),
  sessionInput: (sessionId: string, data: string) =>
    invoke<void>("session_input", { sessionId, data }),
  sessionResize: (sessionId: string, cols: number, rows: number) =>
    invoke<void>("session_resize", { sessionId, cols, rows }),
  sessionKill: (sessionId: string) => invoke<void>("session_kill", { sessionId }),
  sessionClose: (sessionId: string, keep: boolean) =>
    invoke<void>("session_close", { sessionId, keep }),
  sessionRestart: (sessionId: string, resume: boolean) =>
    invoke<SessionMeta>("session_restart", { sessionId, resume }),
  sessionsAutoresume: () => invoke<AutoResume>("sessions_autoresume"),
  sessionSetTitle: (sessionId: string, title: string) =>
    invoke<void>("session_set_title", { sessionId, title }),
  sessionClear: (sessionId: string) => invoke<void>("session_clear", { sessionId }),
  sessionDetach: (sessionId: string) => invoke<void>("session_detach", { sessionId }),
  sessionMetrics: (sessionId: string) =>
    invoke<SessionMetrics | null>("session_metrics", { sessionId }),
  sessionMetricsAll: () => invoke<SessionMetrics[]>("session_metrics_all"),
  sessionActiveIds: () => invoke<string[]>("session_active_ids"),

  listDirs: (path: string) => invoke<DirEntry[]>("list_dirs", { path }),
  homeDir: () => invoke<string | null>("home_dir"),
  appShutdown: () => invoke<void>("app_shutdown"),
};

/**
 * Opens a session's binary channel. PTY output arrives as an ArrayBuffer without
 * going through JSON, and `session_attach` returns the retained history.
 */
export async function attachSession(
  sessionId: string,
  onData: (chunk: Uint8Array) => void,
): Promise<Uint8Array> {
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (msg) => {
    // Small payloads arrive as an ArrayBuffer; large ones through a binary fetch.
    onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : new Uint8Array(msg as never));
  };
  const scrollback = await invoke<ArrayBuffer>("session_attach", { sessionId, channel });
  return new Uint8Array(scrollback);
}

export function onSessionExit(
  cb: (p: { session_id: string; code: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ session_id: string; code: number }>(EV_EXIT, (e) => cb(e.payload));
}

export function onMetrics(cb: (m: SessionMetrics) => void): Promise<UnlistenFn> {
  return listen<SessionMetrics>(EV_METRICS, (e) => cb(e.payload));
}
