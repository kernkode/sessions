//! Data model shared between backend and UI.

use serde::{Deserialize, Serialize};

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Process alive, no recent output.
    #[default]
    Idle,
    /// Recent output: the agent is working.
    Working,
    /// The process finished.
    Exited,
    /// Could not be launched.
    Error,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMeta {
    /// Identifier used by this app.
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub agent_id: String,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub cwd: String,
    /// Session id of the CLI itself (to resume it and to read its metrics).
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub last_active_at: u64,
    #[serde(default)]
    pub status: SessionStatus,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub cols: u16,
    #[serde(default)]
    pub rows: u16,
    /// Effective command line it was launched with (shown in the UI).
    #[serde(default)]
    pub command_line: Option<String>,
}

/// Session creation request coming from the UI.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CreateSessionRequest {
    pub project_id: Option<String>,
    pub project_path: Option<String>,
    pub agent_id: String,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Resume this CLI session.
    #[serde(default)]
    pub resume_external_id: Option<String>,
    #[serde(default)]
    pub continue_last: bool,
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
    /// One-off extra arguments.
    #[serde(default)]
    pub extra_args: Vec<String>,
}

/// Per-session metrics pushed to the UI.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SessionMetrics {
    pub session_id: String,
    /// Last turn / current context usage.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    /// Session totals.
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    /// Context window occupancy.
    pub context_used: u64,
    pub context_window: Option<u64>,
    /// Output tokens per second (moving average).
    pub tokens_per_second: f64,
    /// Highest tok/s observed.
    pub peak_tokens_per_second: f64,
    /// PTY output rate: an activity signal that is always available.
    pub bytes_per_second: f64,
    pub total_bytes: u64,
    /// Estimated cost in USD from the provider's `pricing`.
    pub cost_usd: f64,
    pub model: Option<String>,
    /// Session id in the CLI itself (enables resuming).
    pub external_id: Option<String>,
    pub turns: u32,
    /// Session duration in ms.
    pub uptime_ms: u64,
    /// Resource usage of the process tree.
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub status: SessionStatus,
    /// Last metrics update (epoch ms).
    pub updated_at: u64,
}
