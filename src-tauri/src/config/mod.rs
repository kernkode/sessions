//! Loading, validation and hot reload of the TOML configuration.

pub mod agents;
pub mod app;

use std::fs;

use anyhow::Result;
use parking_lot::RwLock;
use serde::Serialize;

use crate::paths::Paths;
use agents::{Agent, AgentsFile};
use app::AppConfig;

/// A non-fatal problem while loading configuration: it is shown in the UI and the
/// app carries on with factory defaults instead of refusing to start.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigIssue {
    pub file: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConfigSnapshot {
    pub app: AppConfig,
    pub agents: Vec<Agent>,
    pub issues: Vec<ConfigIssue>,
    /// Real paths, so the UI can open them in an editor.
    pub paths: ConfigPaths,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConfigPaths {
    pub root: String,
    pub config: String,
    pub agents: String,
}

/// Shared configuration state. `RwLock` because it is read on every session
/// launch and written only on reload.
pub struct ConfigStore {
    paths: Paths,
    snapshot: RwLock<ConfigSnapshot>,
}

impl ConfigStore {
    pub fn load(paths: Paths) -> Result<Self> {
        paths.bootstrap()?;
        let snapshot = build_snapshot(&paths);
        Ok(Self { paths, snapshot: RwLock::new(snapshot) })
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.snapshot.read().clone()
    }

    /// Re-reads the TOML files from disk. Returns the new snapshot.
    pub fn reload(&self) -> ConfigSnapshot {
        let fresh = build_snapshot(&self.paths);
        *self.snapshot.write() = fresh.clone();
        fresh
    }

    pub fn app_config(&self) -> AppConfig {
        self.snapshot.read().app.clone()
    }

    /// Replaces the `[app]` section, persists it to config.toml and reloads.
    /// Comments in the file are lost on rewrite; values win.
    pub fn set_app(&self, section: app::AppSection) -> ConfigSnapshot {
        let mut cfg = self.app_config();
        cfg.app = section;
        if let Ok(raw) = toml::to_string_pretty(&cfg) {
            let _ = fs::write(&self.paths.config, raw);
        }
        self.reload()
    }

    pub fn perf(&self) -> app::PerformanceSection {
        self.snapshot.read().app.performance.sanitized()
    }

    pub fn agent(&self, id: &str) -> Option<Agent> {
        self.snapshot.read().agents.iter().find(|a| a.id == id).cloned()
    }
}

fn build_snapshot(paths: &Paths) -> ConfigSnapshot {
    let mut issues = Vec::new();

    let app: AppConfig = read_toml(&paths.config, app::DEFAULT_CONFIG_TOML, &mut issues);
    let agents_file: AgentsFile =
        read_toml(&paths.agents, agents::DEFAULT_AGENTS_TOML, &mut issues);

    let mut agents = agents_file.agents;
    let mut seen_agents = std::collections::HashSet::new();
    agents.retain(|a| {
        !a.id.trim().is_empty() && !a.command.trim().is_empty() && seen_agents.insert(a.id.clone())
    });
    agents.retain(|a| a.enabled);

    if agents.is_empty() {
        issues.push(ConfigIssue {
            file: "agents.toml".into(),
            message: "no hay agentes habilitados; se cargan los de fábrica".into(),
        });
        if let Ok(f) = toml::from_str::<AgentsFile>(agents::DEFAULT_AGENTS_TOML) {
            agents = f.agents.into_iter().filter(|a| a.enabled).collect();
        }
    }

    ConfigSnapshot {
        app,
        agents,
        issues,
        paths: ConfigPaths {
            root: paths.root.display().to_string(),
            config: paths.config.display().to_string(),
            agents: paths.agents.display().to_string(),
        },
    }
}

/// Reads a TOML file; on failure records the issue and falls back to the factory
/// contents.
fn read_toml<T>(path: &std::path::Path, fallback: &str, issues: &mut Vec<ConfigIssue>) -> T
where
    T: serde::de::DeserializeOwned + Default,
{
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => {
            issues.push(ConfigIssue { file: name, message: format!("no se pudo leer: {e}") });
            return toml::from_str(fallback).unwrap_or_default();
        }
    };
    match toml::from_str::<T>(&raw) {
        Ok(v) => v,
        Err(e) => {
            issues.push(ConfigIssue {
                file: name,
                message: format!("TOML inválido, usando valores de fábrica: {e}"),
            });
            toml::from_str(fallback).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (ConfigStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sessions-cfg-{}", uuid::Uuid::new_v4()));
        let store = ConfigStore::load(Paths::from_root(dir.clone())).unwrap();
        (store, dir)
    }

    #[test]
    fn loads_defaults_without_issues() {
        let (store, dir) = temp_store();
        let snap = store.snapshot();
        assert!(snap.issues.is_empty(), "issues: {:?}", snap.issues);
        assert!(snap.agents.iter().any(|a| a.id == "claude"));
        assert!(snap.agents.iter().any(|a| a.id == "pi"));
        // The factory config ships an auto-install argv for Claude Code.
        let claude = snap.agents.iter().find(|a| a.id == "claude").unwrap();
        assert!(claude.install.iter().any(|s| s.contains("claude-code")));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn invalid_toml_does_not_break_the_app() {
        let (store, dir) = temp_store();
        fs::write(&store.paths().agents, "this ][ is not toml").unwrap();
        let snap = store.reload();
        assert!(snap.issues.iter().any(|i| i.file == "agents.toml"));
        // There are still usable agents (the factory ones).
        assert!(!snap.agents.is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn duplicate_and_disabled_agents_are_discarded() {
        let (store, dir) = temp_store();
        let dup = r#"
schema = 1

[[agent]]
id = "x"
command = "cmd"

[[agent]]
id = "x"
command = "cmd"

[[agent]]
id = "off"
command = "cmd"
enabled = false
"#;
        fs::write(&store.paths().agents, dup).unwrap();
        let snap = store.reload();
        assert_eq!(snap.agents.iter().filter(|a| a.id == "x").count(), 1);
        assert!(!snap.agents.iter().any(|a| a.id == "off"));
        fs::remove_dir_all(dir).ok();
    }
}
