//! Persistence of projects and sessions in `~/.sessions/state/projects.json`.

use std::path::Path;

use anyhow::Result;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::model::{now_ms, Project, SessionMeta, SessionStatus};
use crate::paths::{write_atomic, Paths};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub sessions: Vec<SessionMeta>,
}

pub struct Store {
    paths: Paths,
    state: RwLock<State>,
    /// `false` when `projects.json` existed but could not be read: in that case
    /// nothing is pruned, so no history is destroyed on top of it.
    loaded_ok: bool,
}

impl Store {
    pub fn load(paths: Paths) -> Self {
        let file = paths.projects_file();
        let raw = std::fs::read_to_string(&file).ok();
        let mut loaded_ok = true;
        let state = match raw.as_deref() {
            None => State::default(),
            Some(text) => match serde_json::from_str::<State>(text) {
                Ok(s) => s,
                Err(e) => {
                    // Never overwrite an unreadable file: it holds the user's
                    // projects. It is set aside so nothing is lost.
                    loaded_ok = false;
                    let stamp = now_ms();
                    let backup = file.with_extension(format!("corrupt-{stamp}.json"));
                    let _ = std::fs::rename(&file, &backup);
                    eprintln!(
                        "Sessions: {} could not be read ({e}); a backup was saved to {}",
                        file.display(),
                        backup.display()
                    );
                    State::default()
                }
            },
        };
        Self { paths, state: RwLock::new(state), loaded_ok }
    }

    pub fn snapshot(&self) -> State {
        self.state.read().clone()
    }

    pub fn projects(&self) -> Vec<Project> {
        self.state.read().projects.clone()
    }

    pub fn sessions(&self) -> Vec<SessionMeta> {
        self.state.read().sessions.clone()
    }

    pub fn session(&self, id: &str) -> Option<SessionMeta> {
        self.state.read().sessions.iter().find(|s| s.id == id).cloned()
    }

    /// Creates or returns the project for a path, comparing normalized paths.
    pub fn upsert_project(&self, path: &str, name: Option<&str>) -> Project {
        let norm = normalize_path(path);
        {
            let st = self.state.read();
            if let Some(p) = st.projects.iter().find(|p| normalize_path(&p.path) == norm) {
                return p.clone();
            }
        }
        let project = Project {
            id: format!("prj_{}", uuid::Uuid::new_v4().simple()),
            name: name.map(str::to_string).unwrap_or_else(|| name_from_path(path)),
            path: path.to_string(),
            created_at: now_ms(),
            collapsed: false,
        };
        self.state.write().projects.push(project.clone());
        let _ = self.save();
        project
    }

    pub fn rename_project(&self, id: &str, name: &str) -> bool {
        let mut st = self.state.write();
        if let Some(p) = st.projects.iter_mut().find(|p| p.id == id) {
            p.name = name.to_string();
            drop(st);
            let _ = self.save();
            return true;
        }
        false
    }

    pub fn set_project_collapsed(&self, id: &str, collapsed: bool) {
        {
            let mut st = self.state.write();
            if let Some(p) = st.projects.iter_mut().find(|p| p.id == id) {
                p.collapsed = collapsed;
            }
        }
        let _ = self.save();
    }

    /// Removes a project and its registered sessions. Returns the removed ids.
    pub fn remove_project(&self, id: &str) -> Vec<String> {
        let removed: Vec<String>;
        {
            let mut st = self.state.write();
            st.projects.retain(|p| p.id != id);
            removed = st
                .sessions
                .iter()
                .filter(|s| s.project_id == id)
                .map(|s| s.id.clone())
                .collect();
            st.sessions.retain(|s| s.project_id != id);
        }
        for sid in &removed {
            self.remove_scrollback(sid);
        }
        let _ = self.save();
        removed
    }

    pub fn upsert_session(&self, meta: SessionMeta) {
        {
            let mut st = self.state.write();
            match st.sessions.iter_mut().find(|s| s.id == meta.id) {
                Some(existing) => *existing = meta,
                None => st.sessions.push(meta),
            }
        }
        let _ = self.save();
    }

    /// Applies a targeted change to one session.
    pub fn update_session<F: FnOnce(&mut SessionMeta)>(&self, id: &str, f: F) -> Option<SessionMeta> {
        let updated = {
            let mut st = self.state.write();
            let s = st.sessions.iter_mut().find(|s| s.id == id)?;
            f(s);
            s.clone()
        };
        let _ = self.save();
        Some(updated)
    }

    pub fn mark_exited(&self, id: &str, code: i32) -> Option<SessionMeta> {
        self.update_session(id, |s| {
            s.status = SessionStatus::Exited;
            s.exit_code = Some(code);
            s.last_active_at = now_ms();
        })
    }

    pub fn remove_session(&self, id: &str) {
        self.state.write().sessions.retain(|s| s.id != id);
        self.remove_scrollback(id);
        let _ = self.save();
    }

    /// On startup no session is still alive: processes do not survive the app.
    pub fn reset_runtime_state(&self) {
        {
            let mut st = self.state.write();
            for s in st.sessions.iter_mut() {
                if s.status != SessionStatus::Exited {
                    s.status = SessionStatus::Exited;
                }
                s.pid = None;
            }
        }
        let _ = self.save();
    }

    pub fn save(&self) -> Result<()> {
        let data = serde_json::to_vec_pretty(&*self.state.read())?;
        write_atomic(&self.paths.projects_file(), &data)
    }

    // ---- Scrollback on disk ----

    pub fn save_scrollback(&self, id: &str, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let _ = write_atomic(&self.paths.scrollback_file(id), data);
    }

    pub fn load_scrollback(&self, id: &str) -> Vec<u8> {
        std::fs::read(self.paths.scrollback_file(id)).unwrap_or_default()
    }

    pub fn remove_scrollback(&self, id: &str) {
        let _ = std::fs::remove_file(self.paths.scrollback_file(id));
    }

    /// Deletes scrollback files with no matching session.
    /// Deletes scrollback files with no matching session. It is skipped when the
    /// state could not be read, so a bad `projects.json` never takes the saved
    /// history with it.
    pub fn prune_scrollback(&self) {
        if !self.loaded_ok {
            return;
        }
        let alive: std::collections::HashSet<String> =
            self.state.read().sessions.iter().map(|s| s.id.clone()).collect();
        if let Ok(rd) = std::fs::read_dir(&self.paths.scrollback) {
            for e in rd.flatten() {
                let p = e.path();
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_string());
                if let Some(stem) = stem {
                    if !alive.contains(&stem) {
                        let _ = std::fs::remove_file(p);
                    }
                }
            }
        }
    }
}

fn normalize_path(p: &str) -> String {
    let s = p.replace('\\', "/");
    let s = s.trim_end_matches('/').to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

fn name_from_path(p: &str) -> String {
    Path::new(&p.replace('\\', "/"))
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| p.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sessions-store-{}", uuid::Uuid::new_v4()));
        let paths = Paths::from_root(dir.clone());
        paths.bootstrap().unwrap();
        (Store::load(paths), dir)
    }

    fn session(id: &str, prj: &str) -> SessionMeta {
        SessionMeta {
            id: id.into(),
            project_id: prj.into(),
            title: "Sesión".into(),
            agent_id: "claude".into(),
            cwd: "C:/tmp".into(),
            created_at: now_ms(),
            status: SessionStatus::Idle,
            ..Default::default()
        }
    }

    #[test]
    fn projects_are_deduplicated_by_path() {
        let (s, dir) = temp_store();
        let a = s.upsert_project("C:\\Users\\ana\\proy", None);
        let b = s.upsert_project("C:/Users/ana/proy/", None);
        assert_eq!(a.id, b.id, "the same path must not create a second project");
        assert_eq!(a.name, "proy");
        assert_eq!(s.projects().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn persists_and_reloads() {
        let (s, dir) = temp_store();
        let p = s.upsert_project("C:/tmp/x", Some("Equis"));
        s.upsert_session(session("ses1", &p.id));
        s.save().unwrap();

        let s2 = Store::load(Paths::from_root(dir.clone()));
        assert_eq!(s2.projects().len(), 1);
        assert_eq!(s2.projects()[0].name, "Equis");
        assert_eq!(s2.sessions().len(), 1);
        assert_eq!(s2.session("ses1").unwrap().agent_id, "claude");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn updates_and_marks_exit() {
        let (s, dir) = temp_store();
        let p = s.upsert_project("C:/tmp/y", None);
        s.upsert_session(session("ses2", &p.id));

        s.update_session("ses2", |m| m.title = "Nuevo título".into());
        assert_eq!(s.session("ses2").unwrap().title, "Nuevo título");

        let m = s.mark_exited("ses2", 3).unwrap();
        assert_eq!(m.status, SessionStatus::Exited);
        assert_eq!(m.exit_code, Some(3));
        assert!(s.update_session("missing", |_| {}).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn removing_a_project_takes_sessions_and_scrollback() {
        let (s, dir) = temp_store();
        let p = s.upsert_project("C:/tmp/z", None);
        s.upsert_session(session("ses3", &p.id));
        s.save_scrollback("ses3", b"previous output");
        assert_eq!(s.load_scrollback("ses3"), b"previous output");

        let removed = s.remove_project(&p.id);
        assert_eq!(removed, vec!["ses3"]);
        assert!(s.sessions().is_empty());
        assert!(s.load_scrollback("ses3").is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reset_runtime_marks_everything_as_exited() {
        let (s, dir) = temp_store();
        let p = s.upsert_project("C:/tmp/w", None);
        let mut m = session("ses4", &p.id);
        m.status = SessionStatus::Working;
        m.pid = Some(1234);
        s.upsert_session(m);

        s.reset_runtime_state();
        let r = s.session("ses4").unwrap();
        assert_eq!(r.status, SessionStatus::Exited);
        assert!(r.pid.is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn prune_deletes_orphan_scrollback() {
        let (s, dir) = temp_store();
        s.save_scrollback("orphan", b"x");
        s.prune_scrollback();
        assert!(s.load_scrollback("orphan").is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupt_json_does_not_prevent_startup_nor_lose_data() {
        let dir = std::env::temp_dir().join(format!("sessions-store-{}", uuid::Uuid::new_v4()));
        let paths = Paths::from_root(dir.clone());
        paths.bootstrap().unwrap();
        std::fs::write(paths.projects_file(), b"{ broken").unwrap();

        let s = Store::load(paths.clone());
        assert!(s.projects().is_empty());

        // The unreadable file is set aside instead of being overwritten.
        s.upsert_project("C:/tmp/new", None);
        let kept: Vec<_> = std::fs::read_dir(&paths.state)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            kept.iter().any(|n| n.contains("corrupt-")),
            "the original should have been preserved: {kept:?}"
        );
        assert_eq!(
            std::fs::read_to_string(paths.state.join(kept.iter().find(|n| n.contains("corrupt-")).unwrap())).unwrap(),
            "{ broken"
        );

        // And a corrupt state does not prune the saved history either.
        let target = paths.scrollback_file("ses_from_before");
        crate::paths::write_atomic(&target, b"valuable output").expect("escribiendo scrollback");
        s.prune_scrollback();
        assert_eq!(s.load_scrollback("ses_from_before"), b"valuable output", "path: {}", target.display());
        std::fs::remove_dir_all(dir).ok();
    }
}
