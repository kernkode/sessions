//! Shared application state and the bridges towards the UI.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter};

use crate::config::ConfigStore;
use crate::metrics::{MetricsHub, MetricsSink};
use crate::model::SessionMetrics;
use crate::pty::{OutputSink, PumpConfig, SessionManager};
use crate::store::Store;

/// Events the backend emits to the UI (JSON, low frequency).
pub const EV_EXIT: &str = "session:exit";
pub const EV_METRICS: &str = "session:metrics";
pub const EV_CONFIG: &str = "config:reloaded";

/// Installs, in the background and without a shell, every enabled agent whose
/// executable is missing but that declares an `install` argv. Runs once at
/// startup so a fresh machine gets its CLIs without manual setup; failures are
/// logged and never block the app.
pub fn spawn_agent_installer(config: Arc<ConfigStore>) {
    let _ = std::thread::Builder::new()
        .name("sessions-agent-install".into())
        .spawn(move || {
            for a in config.snapshot().agents {
                if a.resolve_program().is_some() || a.install.is_empty() {
                    continue;
                }
                let (prog, args) = a.install.split_first().unwrap();
                // Resolve through PATH/PATHEXT so `npm` finds npm.cmd on Windows.
                let prog = match crate::config::agents::which(prog) {
                    Some(p) => p,
                    None => {
                        eprintln!("Sessions: «{}» sin instalador disponible ({prog} no está en PATH)", a.id);
                        continue;
                    }
                };
                eprintln!("Sessions: instalando «{}»…", a.id);
                match std::process::Command::new(prog)
                    .args(args)
                    .env("npm_config_fund", "false")
                    .env("npm_config_audit", "false")
                    .output()
                {
                    Ok(out) if out.status.success() => eprintln!("Sessions: «{}» instalado.", a.id),
                    Ok(out) => eprintln!(
                        "Sessions: no se pudo instalar «{}»: {}",
                        a.id,
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                    Err(e) => eprintln!("Sessions: no se pudo instalar «{}»: {e}", a.id),
                }
            }
        });
}

/// Binary PTY output channels, one per session.
///
/// Terminal output travels through a `Channel` as raw bytes: this avoids
/// serialising to JSON and decoding it again in the UI, which is the biggest cost
/// when an agent writes a lot.
#[derive(Default)]
pub struct ChannelRegistry {
    channels: RwLock<HashMap<String, Channel<InvokeResponseBody>>>,
}

impl ChannelRegistry {
    pub fn set(&self, session_id: &str, ch: Channel<InvokeResponseBody>) {
        self.channels.write().insert(session_id.to_string(), ch);
    }

    pub fn remove(&self, session_id: &str) {
        self.channels.write().remove(session_id);
    }

    pub fn has(&self, session_id: &str) -> bool {
        self.channels.read().contains_key(session_id)
    }

    fn send(&self, session_id: &str, bytes: Vec<u8>) {
        let channel = self.channels.read().get(session_id).cloned();
        if let Some(c) = channel {
            if c.send(InvokeResponseBody::Raw(bytes)).is_err() {
                // The window was closed or the channel is no longer valid.
                self.remove(session_id);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExitPayload {
    pub session_id: String,
    pub code: i32,
}

/// PTY → UI bridge.
pub struct TauriOutputSink {
    app: AppHandle,
    channels: Arc<ChannelRegistry>,
}

impl OutputSink for TauriOutputSink {
    fn data(&self, session_id: &str, bytes: Vec<u8>) {
        self.channels.send(session_id, bytes);
    }

    fn exit(&self, session_id: &str, code: i32) {
        let _ = self
            .app
            .emit(EV_EXIT, ExitPayload { session_id: session_id.to_string(), code });
    }
}

/// Metrics → UI bridge. It also persists the session id discovered by the
/// agent's reader, so the session can be resumed later.
pub struct TauriMetricsSink {
    app: AppHandle,
    store: Arc<Store>,
}

impl MetricsSink for TauriMetricsSink {
    fn metrics(&self, m: &SessionMetrics) {
        if let Some(ext) = m.external_id.as_deref() {
            let known = self
                .store
                .session(&m.session_id)
                .and_then(|s| s.external_id)
                .is_some_and(|e| e == ext);
            if !known {
                self.store.update_session(&m.session_id, |s| {
                    s.external_id = Some(ext.to_string());
                });
            }
        }
        let _ = self.app.emit(EV_METRICS, m);
    }
}

pub struct AppState {
    pub config: Arc<ConfigStore>,
    pub store: Arc<Store>,
    pub sessions: Arc<SessionManager>,
    pub metrics: Arc<MetricsHub>,
    pub channels: Arc<ChannelRegistry>,
}

impl AppState {
    pub fn new(app: AppHandle, config: Arc<ConfigStore>, store: Arc<Store>) -> Self {
        let perf = config.perf();
        let channels = Arc::new(ChannelRegistry::default());

        let output = Arc::new(TauriOutputSink { app: app.clone(), channels: channels.clone() });
        let sessions = Arc::new(SessionManager::new(
            output,
            PumpConfig {
                flush_interval: std::time::Duration::from_millis(perf.flush_interval_ms),
                max_chunk_bytes: perf.max_chunk_bytes,
                supervise_interval: std::time::Duration::from_millis(300),
            },
        ));

        let metrics = Arc::new(MetricsHub::new(
            config.clone(),
            Arc::new(TauriMetricsSink { app: app.clone(), store: store.clone() }),
        ));

        Self { config, store, sessions, metrics, channels }
    }

    /// Orderly shutdown: saves scrollback, kills processes and stops threads.
    pub fn shutdown(&self) {
        if self.config.app_config().app.persist_scrollback {
            for id in self.sessions.ids() {
                if let Some(s) = self.sessions.get(&id) {
                    self.store.save_scrollback(&id, &s.scrollback());
                }
            }
        }
        self.store.reset_runtime_state();
        let _ = self.store.save();
        self.metrics.shutdown();
        self.sessions.shutdown();
    }
}
