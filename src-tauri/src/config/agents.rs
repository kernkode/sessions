//! CLI agent definitions (`~/.sessions/agents.toml`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_AGENTS_TOML: &str = include_str!("../../assets/agents.default.toml");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentsFile {
    #[serde(default = "one")]
    pub schema: u32,
    #[serde(default, rename = "agent")]
    pub agents: Vec<Agent>,
}

fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}

/// Where an agent's token metrics come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricsSource {
    /// `~/.claude/projects/<cwd-slug>/<session>.jsonl`
    ClaudeJsonl,
    /// `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
    CodexRollout,
    /// SQLite `~/.local/share/opencode/opencode.db`
    OpencodeSqlite,
    /// No telemetry of its own: process metrics and output throughput only.
    #[default]
    None,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,

    /// Default executable.
    pub command: String,
    /// Per-platform overrides (npm shims on Windows are `.cmd`).
    #[serde(default)]
    pub command_windows: Option<String>,
    #[serde(default)]
    pub command_macos: Option<String>,
    #[serde(default)]
    pub command_linux: Option<String>,

    #[serde(default)]
    pub args: Vec<String>,
    /// Arguments to resume a specific session (`{session_id}`).
    #[serde(default)]
    pub resume_args: Vec<String>,
    /// Arguments to continue the last session in the directory.
    #[serde(default)]
    pub continue_args: Vec<String>,
    /// How to pass the model when the provider does not do it via env (`{model}`).
    #[serde(default)]
    pub model_args: Vec<String>,

    /// Fixed agent variables (applied before the provider's).
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    #[serde(default)]
    pub metrics: MetricsSource,
    /// Alternative metrics path (if the CLI uses its own directory).
    #[serde(default)]
    pub metrics_path: Option<String>,

    #[serde(default)]
    pub color: Option<String>,
    /// Words in the output that mean the agent is working.
    #[serde(default)]
    pub busy_hints: Vec<String>,
}

impl Agent {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Command for the current platform.
    pub fn platform_command(&self) -> &str {
        let specific = if cfg!(windows) {
            self.command_windows.as_deref()
        } else if cfg!(target_os = "macos") {
            self.command_macos.as_deref()
        } else {
            self.command_linux.as_deref()
        };
        specific.filter(|s| !s.is_empty()).unwrap_or(&self.command)
    }

    /// Absolute path to the executable, or `None` if it is not on PATH.
    pub fn resolve_program(&self) -> Option<PathBuf> {
        which(self.platform_command())
    }
}

/// Executable lookup on PATH, honouring PATHEXT on Windows.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let raw = Path::new(cmd);
    if raw.is_absolute() || cmd.contains('/') || cmd.contains('\\') {
        return candidates(raw).into_iter().find(|p| p.is_file());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let base = dir.join(cmd);
        if let Some(hit) = candidates(&base).into_iter().find(|p| p.is_file()) {
            return Some(hit);
        }
    }
    None
}

fn candidates(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(5);
    if !cfg!(windows) {
        out.push(base.to_path_buf());
        return out;
    }
    // On Windows try the path as given, then each executable extension.
    let has_ext = base.extension().is_some();
    if has_ext {
        out.push(base.to_path_buf());
    }
    let pathext = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .to_ascii_lowercase();
    let name = base.file_name().map(|n| n.to_string_lossy().to_string());
    if let (Some(dir), Some(name)) = (base.parent(), name) {
        for ext in pathext.split(';').filter(|e| !e.is_empty()) {
            out.push(dir.join(format!("{name}{ext}")));
        }
    }
    if !has_ext {
        out.push(base.to_path_buf());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_agents_parse() {
        let f: AgentsFile = toml::from_str(DEFAULT_AGENTS_TOML).expect("valid toml");
        let ids: Vec<_> = f.agents.iter().map(|a| a.id.as_str()).collect();
        for expected in ["claude", "codex", "opencode", "shell"] {
            assert!(ids.contains(&expected), "missing agent {expected}: {ids:?}");
        }
        let claude = f.agents.iter().find(|a| a.id == "claude").unwrap();
        assert_eq!(claude.metrics, MetricsSource::ClaudeJsonl);
        assert!(!claude.resume_args.is_empty());
    }

    #[test]
    fn which_finds_a_system_binary() {
        let probe = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(which(probe).is_some(), "{probe} not found on PATH");
        assert!(which("binary-that-does-not-exist-xyz-123").is_none());
    }

    #[test]
    fn platform_command_prefers_the_override() {
        let mut a = Agent { command: "claude".into(), ..Default::default() };
        a.command_windows = Some("claude.cmd".into());
        if cfg!(windows) {
            assert_eq!(a.platform_command(), "claude.cmd");
        } else {
            assert_eq!(a.platform_command(), "claude");
        }
        a.command_windows = Some(String::new());
        assert_eq!(a.platform_command(), "claude");
    }
}
