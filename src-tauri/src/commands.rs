//! Commands exposed to the UI.

use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::State;

use crate::app::AppState;
use crate::config::{ConfigPaths, ConfigSnapshot};
use crate::metrics::TrackSpec;
use crate::model::{
    now_ms, CreateSessionRequest, Project, SessionMeta, SessionMetrics, SessionStatus,
};
use crate::pty::SpawnSpec;

/// Errors reach the UI as readable text.
type Outcome<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub path: Option<String>,
    pub color: Option<String>,
    pub metrics: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bootstrap {
    pub config: ConfigSnapshot,
    pub agents: Vec<AgentStatus>,
    pub projects: Vec<Project>,
    pub sessions: Vec<SessionMeta>,
    pub platform: String,
    pub home: Option<String>,
    pub version: String,
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Bootstrap {
    let snap = state.config.snapshot();
    Bootstrap {
        agents: agent_statuses(&snap),
        projects: state.store.projects(),
        sessions: state.store.sessions(),
        platform: std::env::consts::OS.to_string(),
        home: dirs::home_dir().map(|p| p.display().to_string()),
        version: env!("CARGO_PKG_VERSION").to_string(),
        config: snap,
    }
}

fn agent_statuses(snap: &ConfigSnapshot) -> Vec<AgentStatus> {
    snap.agents
        .iter()
        .map(|a| {
            let path = a.resolve_program();
            AgentStatus {
                id: a.id.clone(),
                name: a.display_name().to_string(),
                // An agent with no executable is still listed, flagged as missing.
                installed: path.is_some(),
                path: path.map(|p| p.display().to_string()),
                color: a.color.clone(),
                metrics: a.metrics != crate::config::agents::MetricsSource::None,
            }
        })
        .collect()
}

#[tauri::command]
pub fn config_reload(state: State<'_, AppState>) -> ConfigSnapshot {
    state.config.reload()
}

#[tauri::command]
pub fn config_paths(state: State<'_, AppState>) -> ConfigPaths {
    state.config.snapshot().paths
}

// ───────────────────────────── Projects ─────────────────────────────

#[tauri::command]
pub fn project_add(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Outcome<Project> {
    if !std::path::Path::new(&path).is_dir() {
        return Err(format!("«{path}» no es un directorio"));
    }
    Ok(state.store.upsert_project(&path, name.as_deref()))
}

#[tauri::command]
pub fn project_rename(state: State<'_, AppState>, id: String, name: String) -> bool {
    state.store.rename_project(&id, &name)
}

#[tauri::command]
pub fn project_set_collapsed(state: State<'_, AppState>, id: String, collapsed: bool) {
    state.store.set_project_collapsed(&id, collapsed);
}

/// Removes the project, closes its sessions and returns the removed ids.
#[tauri::command]
pub fn project_remove(state: State<'_, AppState>, id: String) -> Vec<String> {
    let removed = state.store.remove_project(&id);
    for sid in &removed {
        let _ = state.sessions.close(sid);
        state.metrics.untrack(sid);
        state.channels.remove(sid);
    }
    removed
}

#[tauri::command]
pub fn project_list(state: State<'_, AppState>) -> Vec<Project> {
    state.store.projects()
}

// ───────────────────────────── Sessions ─────────────────────────────

#[tauri::command]
pub fn session_list(state: State<'_, AppState>) -> Vec<SessionMeta> {
    state.store.sessions()
}

#[tauri::command]
pub fn session_create(
    state: State<'_, AppState>,
    req: CreateSessionRequest,
) -> Outcome<SessionMeta> {
    create_session(&state, req)
}

fn create_session(state: &AppState, req: CreateSessionRequest) -> Outcome<SessionMeta> {
    let plan = crate::launcher::plan(&state.config, &req).map_err(err)?;
    let cfg = state.config.app_config();
    let perf = state.config.perf();

    // Project: by id, or created from the path.
    let project = match req.project_id.clone() {
        Some(id) if state.store.projects().iter().any(|p| p.id == id) => id,
        _ => {
            let path = req
                .project_path
                .clone()
                .unwrap_or_else(|| plan.cwd.display().to_string());
            state.store.upsert_project(&path, None).id
        }
    };

    let cols = req.cols.or(cfg.defaults.cols).unwrap_or(120);
    let rows = req.rows.or(cfg.defaults.rows).unwrap_or(32);
    let id = format!("ses_{}", uuid::Uuid::new_v4().simple());

    let session = state
        .sessions
        .spawn_session(SpawnSpec {
            session_id: id.clone(),
            program: plan.program.clone(),
            args: plan.args.clone(),
            cwd: plan.cwd.clone(),
            env: plan.env.clone(),
            cols,
            rows,
            ring_bytes: perf.ring_buffer_kb * 1024,
        })
        .map_err(err)?;

    let meta = SessionMeta {
        id: id.clone(),
        project_id: project,
        title: req.title.clone().unwrap_or_else(|| plan.agent.display_name().to_string()),
        agent_id: plan.agent.id.clone(),
        cwd: plan.cwd.display().to_string(),
        external_id: req.resume_external_id.clone(),
        created_at: now_ms(),
        last_active_at: now_ms(),
        status: SessionStatus::Idle,
        exit_code: None,
        pid: session.pid(),
        cols,
        rows,
        command_line: Some(plan.command_line.clone()),
    };
    state.store.upsert_session(meta.clone());

    state.metrics.track(TrackSpec {
        session_id: id,
        agent_id: plan.agent.id.clone(),
        metrics_source: plan.agent.metrics,
        metrics_path: plan.agent.metrics_path.clone(),
        cwd: meta.cwd.clone(),
        external_id: meta.external_id.clone(),
        pty: session,
    });

    Ok(meta)
}

/// Hooks the binary channel that streams PTY output up to a session.
///
/// For a live session the history is sent by the pump itself (which is what keeps
/// it from overlapping with the live stream), so this returns nothing. For a
/// session that already ended it returns the saved raw bytes: the UI replays them
/// through an off-screen terminal of the original size, the only faithful way to
/// reproduce a TUI screen.
#[tauri::command]
pub fn session_attach(
    state: State<'_, AppState>,
    session_id: String,
    channel: Channel<InvokeResponseBody>,
) -> Response {
    state.channels.set(&session_id, channel);
    if state.sessions.attach(&session_id) {
        return Response::new(Vec::new());
    }
    Response::new(state.store.load_scrollback(&session_id))
}

#[tauri::command]
pub fn session_detach(state: State<'_, AppState>, session_id: String) {
    state.sessions.detach(&session_id);
    state.channels.remove(&session_id);
}

#[tauri::command]
pub fn session_input(
    state: State<'_, AppState>,
    session_id: String,
    data: String,
) -> Outcome<()> {
    state.sessions.write_input(&session_id, data.as_bytes()).map_err(err)
}

/// Resizes the PTY. If the session already ended only the size is stored: the UI
/// also resizes the terminals of closed sessions.
#[tauri::command]
pub fn session_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Outcome<()> {
    if state.sessions.get(&session_id).is_some() {
        state.sessions.resize(&session_id, cols, rows).map_err(err)?;
    }
    state.store.update_session(&session_id, |s| {
        s.cols = cols;
        s.rows = rows;
    });
    Ok(())
}

/// Ends the process but keeps the session in the list.
#[tauri::command]
pub fn session_kill(state: State<'_, AppState>, session_id: String) -> Outcome<()> {
    let s = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| format!("la sesión {session_id} no está activa"))?;
    s.kill().map_err(err)?;
    Ok(())
}

/// Closes the session and removes it from the registry.
#[tauri::command]
pub fn session_close(
    state: State<'_, AppState>,
    session_id: String,
    keep: bool,
) -> Outcome<()> {
    if state.config.app_config().app.persist_scrollback && keep {
        if let Some(s) = state.sessions.get(&session_id) {
            state.store.save_scrollback(&session_id, &s.scrollback());
        }
    }
    state.sessions.close(&session_id).map_err(err)?;
    state.metrics.untrack(&session_id);
    state.channels.remove(&session_id);
    if keep {
        state.store.update_session(&session_id, |s| {
            s.status = SessionStatus::Exited;
            s.pid = None;
        });
    } else {
        state.store.remove_session(&session_id);
    }
    Ok(())
}

/// Relaunches an existing session (same settings, optionally resuming).
#[tauri::command]
pub fn session_restart(
    state: State<'_, AppState>,
    session_id: String,
    resume: bool,
) -> Outcome<SessionMeta> {
    let previous = state
        .store
        .session(&session_id)
        .ok_or_else(|| format!("sesión {session_id} desconocida"))?;
    relaunch(&state, &previous, resume)
}

/// Launches a session again from its saved record.
///
/// The new one is created **before** dropping the old record: if the launch fails
/// (the agent is gone, the directory no longer exists...) the entry is not lost.
fn relaunch(state: &AppState, previous: &SessionMeta, resume: bool) -> Outcome<SessionMeta> {
    let _ = state.sessions.close(&previous.id);
    state.metrics.untrack(&previous.id);
    state.channels.remove(&previous.id);

    let req = CreateSessionRequest {
        project_id: Some(previous.project_id.clone()),
        project_path: None,
        agent_id: previous.agent_id.clone(),
        title: Some(previous.title.clone()),
        cwd: Some(previous.cwd.clone()),
        resume_external_id: if resume { previous.external_id.clone() } else { None },
        continue_last: false,
        cols: Some(previous.cols),
        rows: Some(previous.rows),
        extra_args: Vec::new(),
    };
    let created = create_session(state, req)?;
    state.store.remove_session(&previous.id);
    Ok(created)
}

/// Sessions to bring back on startup, according to `[app]` in config.toml:
/// `active` (the most recently used) or `all`.
fn sessions_to_resume(sessions: &[SessionMeta], scope: &str) -> Vec<SessionMeta> {
    match scope {
        "all" => {
            let mut all = sessions.to_vec();
            all.sort_by_key(|s| s.last_active_at);
            all
        }
        "active" => sessions
            .iter()
            .max_by_key(|s| s.last_active_at)
            .cloned()
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoResume {
    pub scope: String,
    pub sessions: Vec<SessionMeta>,
    /// Sessions that could not be relaunched, with the reason.
    pub failed: Vec<(String, String)>,
}

/// Relaunches the saved sessions when the app starts, so the user does not have
/// to press «Reanudar». Agents that support it resume their conversation
/// (`--resume <id>`); the rest simply start again in the same directory.
#[tauri::command]
pub fn sessions_autoresume(state: State<'_, AppState>) -> AutoResume {
    let scope = state.config.app_config().app.resume_scope().to_string();
    let candidates = sessions_to_resume(&state.store.sessions(), &scope);

    let mut sessions = Vec::new();
    let mut failed = Vec::new();
    for previous in candidates {
        // Only sessions whose process is gone, which after a restart is all of them.
        if state.sessions.get(&previous.id).is_some() {
            continue;
        }
        let resume = previous.external_id.is_some();
        match relaunch(&state, &previous, resume) {
            Ok(meta) => sessions.push(meta),
            Err(e) => failed.push((previous.title.clone(), e)),
        }
    }
    AutoResume { scope, sessions, failed }
}

#[tauri::command]
pub fn session_set_title(state: State<'_, AppState>, session_id: String, title: String) {
    state.store.update_session(&session_id, |s| s.title = title);
}

#[tauri::command]
pub fn session_clear(state: State<'_, AppState>, session_id: String) {
    if let Some(s) = state.sessions.get(&session_id) {
        s.clear_scrollback();
    }
    state.store.remove_scrollback(&session_id);
}

#[tauri::command]
pub fn session_metrics(
    state: State<'_, AppState>,
    session_id: String,
) -> Option<SessionMetrics> {
    state.metrics.snapshot(&session_id)
}

#[tauri::command]
pub fn session_metrics_all(state: State<'_, AppState>) -> Vec<SessionMetrics> {
    state.metrics.all()
}

/// Sessions alive according to the process manager (diagnostics).
#[tauri::command]
pub fn session_active_ids(state: State<'_, AppState>) -> Vec<String> {
    state.sessions.ids()
}

// ───────────────────────────── Utilities ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

/// Subdirectories of a path: feeds the project picker without dialogs.
#[tauri::command]
pub fn list_dirs(path: String) -> Outcome<Vec<DirEntry>> {
    let base = std::path::Path::new(&path);
    let rd = std::fs::read_dir(base).map_err(err)?;
    let mut out: Vec<DirEntry> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| DirEntry {
            name: e.file_name().to_string_lossy().to_string(),
            path: e.path().display().to_string(),
        })
        .take(500)
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

#[tauri::command]
pub fn home_dir() -> Option<String> {
    dirs::home_dir().map(|p| p.display().to_string())
}

/// Orderly shutdown requested by the UI before destroying the window.
#[tauri::command]
pub fn app_shutdown(state: State<'_, AppState>) {
    state.shutdown();
}

/// List of commands to register in the `Builder`.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        bootstrap,
        config_reload,
        config_paths,
        project_add,
        project_rename,
        project_set_collapsed,
        project_remove,
        project_list,
        session_list,
        session_create,
        session_attach,
        session_detach,
        session_input,
        session_resize,
        session_kill,
        session_close,
        session_restart,
        sessions_autoresume,
        session_set_title,
        session_clear,
        session_metrics,
        session_metrics_all,
        session_active_ids,
        list_dirs,
        home_dir,
        app_shutdown,
    ]
}

/// State the commands need, grouped for `setup`.
pub type SharedState = Arc<AppState>;

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, last_active: u64) -> SessionMeta {
        SessionMeta {
            id: id.into(),
            title: id.into(),
            last_active_at: last_active,
            ..Default::default()
        }
    }

    #[test]
    fn resume_selection_follows_the_configured_scope() {
        let sessions = vec![session("a", 100), session("b", 300), session("c", 200)];

        // `active`: only the most recently used one.
        let active = sessions_to_resume(&sessions, "active");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "b");

        // `all`: every one, oldest first so the newest ends up selected.
        let all = sessions_to_resume(&sessions, "all");
        assert_eq!(all.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a", "c", "b"]);

        // `none`: nothing is relaunched.
        assert!(sessions_to_resume(&sessions, "none").is_empty());
        // And with no saved sessions there is nothing to do either.
        assert!(sessions_to_resume(&[], "all").is_empty());
        assert!(sessions_to_resume(&[], "active").is_empty());
    }
}
