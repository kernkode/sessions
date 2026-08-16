//! Resolution and bootstrap of the `~/.sessions` data directory.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Name of the root directory inside the user's home.
pub const ROOT_DIR: &str = ".sessions";

/// Layout of `~/.sessions`.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub agents: PathBuf,
    pub state: PathBuf,
    pub scrollback: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    /// Resolves the layout. `SESSIONS_HOME` overrides it (tests / portable use).
    pub fn resolve() -> Result<Self> {
        let root = match std::env::var_os("SESSIONS_HOME") {
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => dirs::home_dir()
                .context("could not determine the HOME directory")?
                .join(ROOT_DIR),
        };
        Ok(Self::from_root(root))
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self {
            config: root.join("config.toml"),
            agents: root.join("agents.toml"),
            state: root.join("state"),
            scrollback: root.join("scrollback"),
            logs: root.join("logs"),
            root,
        }
    }

    /// Creates the directory structure and the default TOML files if missing.
    pub fn bootstrap(&self) -> Result<()> {
        for dir in [&self.root, &self.state, &self.scrollback, &self.logs] {
            fs::create_dir_all(dir)
                .with_context(|| format!("creando directorio {}", dir.display()))?;
        }
        write_if_absent(&self.config, crate::config::app::DEFAULT_CONFIG_TOML)?;
        write_if_absent(&self.agents, crate::config::agents::DEFAULT_AGENTS_TOML)?;
        Ok(())
    }

    pub fn projects_file(&self) -> PathBuf {
        self.state.join("projects.json")
    }

    pub fn scrollback_file(&self, session_id: &str) -> PathBuf {
        self.scrollback.join(format!("{session_id}.bin"))
    }
}

fn write_if_absent(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, contents).with_context(|| format!("escribiendo {}", path.display()))
}

/// Atomic write: writes to `.tmp` and renames. Prevents corrupt files if the app
/// dies halfway through a save.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("dat")
    ));
    fs::write(&tmp, contents).with_context(|| format!("escribiendo {}", tmp.display()))?;
    // On Windows, rename fails if the destination exists.
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).with_context(|| format!("renombrando a {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_creates_full_layout() {
        let tmp = std::env::temp_dir().join(format!("sessions-test-{}", uuid::Uuid::new_v4()));
        let paths = Paths::from_root(tmp.clone());
        paths.bootstrap().unwrap();

        assert!(paths.config.is_file());
        assert!(paths.agents.is_file());
        assert!(paths.scrollback.is_dir());
        assert!(paths.state.is_dir());

        // Idempotent: does not overwrite.
        fs::write(&paths.config, b"# touched").unwrap();
        paths.bootstrap().unwrap();
        assert_eq!(fs::read_to_string(&paths.config).unwrap(), "# touched");

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn write_atomic_overwrites() {
        let tmp = std::env::temp_dir().join(format!("sessions-atomic-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("a.json");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "two");
        fs::remove_dir_all(&tmp).ok();
    }
}
