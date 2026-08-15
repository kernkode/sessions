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
  provider_id: string | null;
  model: string | null;
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
  external_id: string | null;
  turns: number;
  uptime_ms: number;
  cpu_percent: number;
  memory_bytes: number;
  status: SessionStatus;
  updated_at: number;
}

export interface Pricing {
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
}

export interface ProviderModel {
  id: string;
  name: string | null;
  context_window: number | null;
  max_output_tokens: number | null;
  reasoning: boolean;
  vision: boolean;
  tool_call: boolean;
  aliases: string[];
  pricing: Pricing | null;
  remote_id: string | null;
}

export interface Provider {
  id: string;
  name: string | null;
  kind: string;
  enabled: boolean;
  base_url: string | null;
  api_key: string | null;
  api_key_env: string | null;
  api_key_file: string | null;
  api_key_json_path: string | null;
  api_key_command: string | null;
  organization: string | null;
  project: string | null;
  region: string | null;
  api_version: string | null;
  timeout_ms: number | null;
  max_retries: number | null;
  headers: Record<string, string>;
  default_model: string | null;
  small_model: string | null;
  model: ProviderModel[];
  env: Record<string, Record<string, string>>;
  args: Record<string, string[]>;
  agents: string[];
  notes: string | null;
  /** Computed by the backend: agents this provider can be used with. */
  supported_agents: string[];
}

/** A provider just created in the UI, before saving it. */
export function emptyProvider(id = ""): Provider {
  return {
    id,
    name: null,
    kind: "openai-chat",
    enabled: true,
    base_url: null,
    api_key: null,
    api_key_env: null,
    api_key_file: null,
    api_key_json_path: null,
    api_key_command: null,
    organization: null,
    project: null,
    region: null,
    api_version: null,
    timeout_ms: null,
    max_retries: null,
    headers: {},
    default_model: null,
    small_model: null,
    model: [],
    env: {},
    args: {},
    agents: [],
    notes: null,
    supported_agents: [],
  };
}

export function emptyModel(id = ""): ProviderModel {
  return {
    id,
    name: null,
    context_window: null,
    max_output_tokens: null,
    reasoning: false,
    vision: false,
    tool_call: true,
    aliases: [],
    pricing: null,
    remote_id: null,
  };
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
  providers: string[];
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
  };
  terminal: TerminalConfig;
  performance: PerformanceConfig;
  defaults: {
    agent: string | null;
    provider: string | null;
    model: string | null;
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
  providers: string;
  agents: string;
}

export interface ConfigSnapshot {
  app: AppConfig;
  providers: Provider[];
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
  provider_id?: string | null;
  model?: string | null;
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

/** Result of the provider editor's "Check" button. */
export interface AgentPreview {
  agent_id: string;
  agent_name: string;
  installed: boolean;
  supported: boolean;
  from_template: boolean;
  env: [string, string][];
  args: string[];
}

export interface ProviderCheck {
  key_source: "literal" | "file" | "command" | "env" | "none";
  key_found: boolean;
  key_hint: string | null;
  key_error: string | null;
  model: string | null;
  context_window: number | null;
  agents: AgentPreview[];
}
