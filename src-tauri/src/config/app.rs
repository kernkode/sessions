//! General application settings (`~/.sessions/config.toml`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG_TOML: &str = include_str!("../../assets/config.default.toml");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub app: AppSection,
    #[serde(default)]
    pub terminal: TerminalSection,
    #[serde(default)]
    pub performance: PerformanceSection,
    #[serde(default)]
    pub defaults: DefaultsSection,
    #[serde(default)]
    pub keybinds: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSection {
    pub theme: String,
    pub language: String,
    /// Master switch: reopen the saved sessions when the app starts.
    pub restore_sessions: bool,
    /// Which ones to reopen when `restore_sessions` is on:
    /// `active` (the last used one) or `all`.
    pub auto_resume: String,
    pub confirm_on_close: bool,
    /// Persist scrollback to disk when the app closes.
    pub persist_scrollback: bool,
    /// Relaunch or resume a session whose process ended on its own, instead of
    /// leaving the «sesión terminada» transcript on screen.
    pub auto_relaunch: bool,
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            language: "es".into(),
            restore_sessions: true,
            auto_resume: "active".into(),
            confirm_on_close: true,
            persist_scrollback: true,
            auto_relaunch: true,
        }
    }
}

impl AppSection {
    /// Normalised resume scope: `none`, `active` or `all`.
    pub fn resume_scope(&self) -> &str {
        if !self.restore_sessions {
            return "none";
        }
        match self.auto_resume.trim().to_ascii_lowercase().as_str() {
            "all" => "all",
            "none" | "off" | "false" => "none",
            _ => "active",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSection {
    pub font_family: String,
    pub font_size: u16,
    pub line_height: f32,
    /// Lines of history kept by xterm.
    pub scrollback: u32,
    pub cursor_blink: bool,
    pub cursor_style: String,
    /// `webgl` (fast) | `canvas` | `dom`.
    pub renderer: String,
    pub bell: bool,
    pub copy_on_select: bool,
}

impl Default for TerminalSection {
    fn default() -> Self {
        Self {
            font_family: "JetBrains Mono, Cascadia Code, Menlo, Consolas, monospace".into(),
            font_size: 13,
            line_height: 1.25,
            scrollback: 8000,
            cursor_blink: true,
            cursor_style: "bar".into(),
            renderer: "webgl".into(),
            bell: false,
            copy_on_select: true,
        }
    }
}

/// PTY-to-UI pipeline knobs. These are what actually govern performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceSection {
    /// Coalescing window for PTY output (ms). Higher means less IPC.
    pub flush_interval_ms: u64,
    /// Size that forces an immediate send without waiting for the window.
    pub max_chunk_bytes: usize,
    /// Ring buffer per session in KiB (used to rehydrate on tab switch).
    pub ring_buffer_kb: usize,
    /// Metrics polling while the session is active / idle.
    pub metrics_poll_ms_active: u64,
    pub metrics_poll_ms_idle: u64,
    /// Live xterm instances kept in memory (LRU); the rest are rehydrated.
    pub max_live_terminals: usize,
    /// CPU/RAM sampling of child processes (0 = disabled). The metrics bar no
    /// longer shows them, so it ships off: enabling it means enumerating every
    /// process on the machine on each tick.
    pub process_sample_ms: u64,
}

impl Default for PerformanceSection {
    fn default() -> Self {
        Self {
            flush_interval_ms: 12,
            max_chunk_bytes: 32 * 1024,
            ring_buffer_kb: 512,
            metrics_poll_ms_active: 300,
            metrics_poll_ms_idle: 2000,
            max_live_terminals: 4,
            process_sample_ms: 0,
        }
    }
}

impl PerformanceSection {
    /// Clamps values so a hand-edited config cannot degrade the app.
    pub fn sanitized(&self) -> Self {
        Self {
            flush_interval_ms: self.flush_interval_ms.clamp(4, 250),
            max_chunk_bytes: self.max_chunk_bytes.clamp(4 * 1024, 1024 * 1024),
            ring_buffer_kb: self.ring_buffer_kb.clamp(32, 8 * 1024),
            metrics_poll_ms_active: self.metrics_poll_ms_active.clamp(100, 5_000),
            metrics_poll_ms_idle: self.metrics_poll_ms_idle.clamp(500, 60_000),
            max_live_terminals: self.max_live_terminals.clamp(1, 24),
            process_sample_ms: if self.process_sample_ms == 0 {
                0
            } else {
                self.process_sample_ms.clamp(500, 60_000)
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DefaultsSection {
    pub agent: Option<String>,
    /// Initial working directory for new sessions.
    pub cwd: Option<String>,
    /// Initial PTY size, before the first `fit`.
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses() {
        let c: AppConfig = toml::from_str(DEFAULT_CONFIG_TOML).expect("valid toml");
        assert_eq!(c.app.language, "es");
        assert!(c.terminal.scrollback >= 1000);
        assert!(c.performance.flush_interval_ms > 0);
        assert!(!c.keybinds.is_empty());
        assert_eq!(c.app.resume_scope(), "active");
    }

    #[test]
    fn resume_scope_is_normalised() {
        let mut a = AppSection::default();
        assert_eq!(a.resume_scope(), "active");
        a.auto_resume = "ALL".into();
        assert_eq!(a.resume_scope(), "all");
        a.auto_resume = "cualquier-cosa".into();
        assert_eq!(a.resume_scope(), "active", "an unknown value falls back to active");
        // The master switch wins.
        a.restore_sessions = false;
        assert_eq!(a.resume_scope(), "none");
        // A config that predates `auto_resume` keeps resuming the active one.
        let legacy: AppConfig =
            toml::from_str("[app]\nrestore_sessions = true\n").expect("valid toml");
        assert_eq!(legacy.app.resume_scope(), "active");
        let off: AppConfig =
            toml::from_str("[app]\nrestore_sessions = false\n").expect("valid toml");
        assert_eq!(off.app.resume_scope(), "none");
    }

    #[test]
    fn empty_config_uses_defaults() {
        let c: AppConfig = toml::from_str("").unwrap();
        assert_eq!(c.terminal.renderer, "webgl");
        assert_eq!(c.performance.max_live_terminals, 4);
        // Process sampling ships disabled: nothing in the UI shows CPU/RAM.
        assert_eq!(c.performance.process_sample_ms, 0);
        assert_eq!(c.performance.sanitized().process_sample_ms, 0);
    }

    #[test]
    fn sanitized_clamps_absurd_values() {
        let p = PerformanceSection {
            flush_interval_ms: 0,
            max_chunk_bytes: 1,
            ring_buffer_kb: 0,
            metrics_poll_ms_active: 1,
            metrics_poll_ms_idle: 1,
            max_live_terminals: 999,
            process_sample_ms: 1,
        }
        .sanitized();
        assert_eq!(p.flush_interval_ms, 4);
        assert_eq!(p.max_chunk_bytes, 4096);
        assert_eq!(p.ring_buffer_kb, 32);
        assert_eq!(p.max_live_terminals, 24);
        assert_eq!(p.process_sample_ms, 500);
    }
}
