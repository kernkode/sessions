// Types mirroring exactly what the backend serialises (snake_case).

export type SessionStatus = "idle" | "working" | "exited" | "error";

export interface Project {
  id: string;
  name: string;
  path: string;
  created_at: number;
  collapsed: boolean;
}

export interface SessionMeta {
  id: string;
  project_id: string;
  title: string;
  agent_id: string;
  cwd: string;
  external_id: string | null;
  created_at: number;
  last_active_at: number;
  status: SessionStatus;
  exit_code: number | null;
  pid: number | null;
  cols: number;
  rows: number;
  command_line: string | null;
}

export interface SessionMetrics {
  session_id: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  reasoning_tokens: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_tokens: number;
  context_used: number;
  context_window: number | null;
  tokens_per_second: number;
  peak_tokens_per_second: number;
  bytes_per_second: number;
  total_bytes: number;
  cost_usd: number;
  model: string | null;
  effort: string | null;
  external_id: string | null;
  turns: number;
  uptime_ms: number;
  cpu_percent: number;
  memory_bytes: number;
  status: SessionStatus;
  updated_at: number;
}

export interface AgentDef {
  id: string;
  name: string | null;
  enabled: boolean;
  command: string;
  metrics: string;
  color: string | null;
}

export interface AgentStatus {
  id: string;
  name: string;
  installed: boolean;
  path: string | null;
  color: string | null;
  metrics: boolean;
}

export interface TerminalConfig {
  font_family: string;
  font_size: number;
  line_height: number;
  scrollback: number;
  cursor_blink: boolean;
  cursor_style: string;
  renderer: string;
  bell: boolean;
  copy_on_select: boolean;
}

export interface PerformanceConfig {
  flush_interval_ms: number;
  max_chunk_bytes: number;
  ring_buffer_kb: number;
  metrics_poll_ms_active: number;
  metrics_poll_ms_idle: number;
  max_live_terminals: number;
  process_sample_ms: number;
}

export interface AppConfig {
  app: {
    theme: string;
    language: string;
    restore_sessions: boolean;
    auto_resume: string;
    confirm_on_close: boolean;
    persist_scrollback: boolean;
    auto_relaunch: boolean;
  };
  terminal: TerminalConfig;
  performance: PerformanceConfig;
  defaults: {
    agent: string | null;
    cwd: string | null;
    cols: number | null;
    rows: number | null;
  };
  keybinds: Record<string, string>;
}

export interface ConfigIssue {
  file: string;
  message: string;
}

export interface ConfigPaths {
  root: string;
  config: string;
  agents: string;
}

export interface ConfigSnapshot {
  app: AppConfig;
  agents: (AgentDef & { metrics_path: string | null })[];
  issues: ConfigIssue[];
  paths: ConfigPaths;
}

export interface Bootstrap {
  config: ConfigSnapshot;
  agents: AgentStatus[];
  projects: Project[];
  sessions: SessionMeta[];
  platform: string;
  home: string | null;
  version: string;
}

/** Result of relaunching the saved sessions on startup. */
export interface AutoResume {
  scope: "none" | "active" | "all";
  sessions: SessionMeta[];
  /** `[title, reason]` for the ones that could not be relaunched. */
  failed: [string, string][];
}

export interface CreateSessionRequest {
  project_id?: string | null;
  project_path?: string | null;
  agent_id: string;
  title?: string | null;
  cwd?: string | null;
  resume_external_id?: string | null;
  continue_last?: boolean;
  cols?: number | null;
  rows?: number | null;
  extra_args?: string[];
}

export interface DirEntry {
  name: string;
  path: string;
}
