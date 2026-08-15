//! Loading, validation and hot reload of the TOML configuration.

pub mod agents;
pub mod app;
pub mod edit;
pub mod providers;

use std::fs;

use anyhow::Result;
use parking_lot::RwLock;
use serde::Serialize;

use crate::paths::Paths;
use agents::{Agent, AgentsFile};
use app::AppConfig;
use providers::{Provider, ProvidersFile};

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
    pub providers: Vec<Provider>,
    pub agents: Vec<Agent>,
    pub issues: Vec<ConfigIssue>,
    /// Real paths, so the UI can open them in an editor.
    pub paths: ConfigPaths,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConfigPaths {
    pub root: String,
    pub config: String,
    pub providers: String,
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

    pub fn perf(&self) -> app::PerformanceSection {
        self.snapshot.read().app.performance.sanitized()
    }

    pub fn agent(&self, id: &str) -> Option<Agent> {
        self.snapshot.read().agents.iter().find(|a| a.id == id).cloned()
    }

    pub fn provider(&self, id: &str) -> Option<Provider> {
        self.snapshot.read().providers.iter().find(|p| p.id == id).cloned()
    }

    /// Context window declared for a provider/model pair.
    pub fn context_window(&self, provider_id: Option<&str>, model: Option<&str>) -> Option<u64> {
        let snap = self.snapshot.read();
        if let Some(pid) = provider_id {
            if let Some(p) = snap.providers.iter().find(|p| p.id == pid) {
                if let Some(cw) = p.context_window_for(model) {
                    return Some(cw);
                }
            }
        }
        // Without a provider: look the model up in any known provider.
        let model = model?;
        snap.providers
            .iter()
            .filter_map(|p| p.find_model(model))
            .find_map(|m| m.context_window)
    }
}

fn build_snapshot(paths: &Paths) -> ConfigSnapshot {
    let mut issues = Vec::new();

    let app: AppConfig = read_toml(&paths.config, app::DEFAULT_CONFIG_TOML, &mut issues);
    let providers_file: ProvidersFile =
        read_toml(&paths.providers, providers::DEFAULT_PROVIDERS_TOML, &mut issues);
    let agents_file: AgentsFile =
        read_toml(&paths.agents, agents::DEFAULT_AGENTS_TOML, &mut issues);

    let mut providers = providers_file.providers;
    let mut seen = std::collections::HashSet::new();
    providers.retain(|p| {
        if p.id.trim().is_empty() {
            issues.push(ConfigIssue {
                file: "providers.toml".into(),
                message: "proveedor sin `id`: descartado".into(),
            });
            return false;
        }
        if !seen.insert(p.id.clone()) {
            issues.push(ConfigIssue {
                file: "providers.toml".into(),
                message: format!("`id` duplicado «{}»: se ignora la repetición", p.id),
            });
            return false;
        }
        true
    });
    // Computed field: the UI uses it so the compatibility rules live in one place.
    for p in providers.iter_mut() {
        p.supported_agents = p.compute_supported_agents();
    }

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
        providers,
        agents,
        issues,
        paths: ConfigPaths {
            root: paths.root.display().to_string(),
            config: paths.config.display().to_string(),
            providers: paths.providers.display().to_string(),
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
        assert!(snap.providers.len() >= 3);
        assert!(snap.agents.iter().any(|a| a.id == "claude"));
        // `mi-gateway` ships disabled but is still listed.
        assert!(snap.providers.iter().any(|p| p.id == "mi-gateway" && !p.enabled));
        // The computed field reaches the UI.
        let anth = snap.providers.iter().find(|p| p.id == "anthropic").unwrap();
        assert!(anth.supported_agents.contains(&"claude".to_string()));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn invalid_toml_does_not_break_the_app() {
        let (store, dir) = temp_store();
        fs::write(&store.paths().providers, "this ][ is not toml").unwrap();
        let snap = store.reload();
        assert!(snap.issues.iter().any(|i| i.file == "providers.toml"));
        // There are still usable providers (the factory ones).
        assert!(!snap.providers.is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn duplicate_ids_are_discarded() {
        let (store, dir) = temp_store();
        let dup = r#"
[[provider]]
id = "x"
[[provider.model]]
id = "m"
context_window = 1000

[[provider]]
id = "x"
[[provider.model]]
id = "m2"
context_window = 2000
"#;
        fs::write(&store.paths().providers, dup).unwrap();
        let snap = store.reload();
        assert_eq!(snap.providers.iter().filter(|p| p.id == "x").count(), 1);
        assert!(snap.issues.iter().any(|i| i.message.contains("duplicado")));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn context_window_resolves_by_provider_and_by_model() {
        let (store, dir) = temp_store();
        assert_eq!(
            store.context_window(Some("anthropic"), Some("claude-sonnet-4-5")),
            Some(200_000)
        );
        // Without an explicit provider, the model is looked up in all of them.
        assert_eq!(store.context_window(None, Some("gemini-3-pro")), Some(1_048_576));
        assert_eq!(store.context_window(None, Some("does-not-exist")), None);
        fs::remove_dir_all(dir).ok();
    }
}
