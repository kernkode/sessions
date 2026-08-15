//! End-to-end tests of the backend: TOML configuration → launch plan → PTY →
//! metrics → persistence. No GUI required.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sessions_lib::config::agents::MetricsSource;
use sessions_lib::config::ConfigStore;
use sessions_lib::metrics::{MetricsHub, MetricsSink, TrackSpec};
use sessions_lib::model::{now_ms, CreateSessionRequest, SessionMeta, SessionMetrics, SessionStatus};
use sessions_lib::paths::Paths;
use sessions_lib::pty::{OutputSink, PumpConfig, SessionManager, SpawnSpec};
use sessions_lib::store::Store;

#[derive(Default)]
struct Sink {
    data: Mutex<HashMap<String, Vec<u8>>>,
    exits: Mutex<Vec<(String, i32)>>,
}

impl Sink {
    fn text(&self, id: &str) -> String {
        String::from_utf8_lossy(&self.data.lock().get(id).cloned().unwrap_or_default()).to_string()
    }
    fn finished(&self, id: &str) -> bool {
        self.exits.lock().iter().any(|(s, _)| s == id)
    }
}

/// Local wrapper: the orphan rule forbids implementing the trait on `Arc`.
struct OutputBridge(Arc<Sink>);

impl OutputSink for OutputBridge {
    fn data(&self, session_id: &str, bytes: Vec<u8>) {
        self.0.data.lock().entry(session_id.to_string()).or_default().extend_from_slice(&bytes);
    }
    fn exit(&self, session_id: &str, code: i32) {
        self.0.exits.lock().push((session_id.to_string(), code));
    }
}

struct MetricsCollector(Mutex<Vec<SessionMetrics>>);

struct MetricsBridge(Arc<MetricsCollector>);

impl MetricsSink for MetricsBridge {
    fn metrics(&self, m: &SessionMetrics) {
        self.0 .0.lock().push(m.clone());
    }
}

struct Env {
    dir: std::path::PathBuf,
    config: Arc<ConfigStore>,
    store: Arc<Store>,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sessions-e2e-{name}-{}", uuid::Uuid::new_v4()));
        let paths = Paths::from_root(dir.clone());
        let config = Arc::new(ConfigStore::load(paths.clone()).unwrap());
        let store = Arc::new(Store::load(paths));
        Self { dir, config, store }
    }

    /// Defines an agent pointing at a real shell whose environment can be
    /// verified in the process output.
    fn with_echo_agent(&self) {
        let (cmd, echo) = if cfg!(windows) {
            ("cmd", "echo MODEL=%MY_MODEL% URL=%MY_URL% KEY=%MY_KEY%")
        } else {
            ("sh", "echo MODEL=$MY_MODEL URL=$MY_URL KEY=$MY_KEY")
        };
        let args = if cfg!(windows) {
            format!(r#"args = ["/c", "{echo}"]"#)
        } else {
            format!(r#"args = ["-c", "{echo}"]"#)
        };
        std::fs::write(
            &self.config.paths().agents,
            format!(
                r#"
schema = 1

[[agent]]
id = "echo"
name = "Echo Agent"
command = "{cmd}"
{args}
metrics = "none"

[agent.env]
MY_URL = "https://api.test.dev/v1"
MY_MODEL = "big-model"
MY_KEY = "sk-secret"
"#
            ),
        )
        .unwrap();
        self.config.reload();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn wait_for<F: Fn() -> bool>(seconds: u64, f: F) -> bool {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

/// Answers `ESC[6n` the way the UI terminal does: without it ConPTY waits and the
/// process emits nothing.
fn answer_dsr(sink: &Arc<Sink>, mgr: &SessionManager, id: &str) {
    let _ = wait_for(10, || sink.text(id).contains("\u{1b}[6n"));
    let _ = mgr.write_input(id, b"\x1b[1;1R");
}

#[test]
fn full_cycle_config_pty_and_persistence() {
    let env = Env::new("cycle");
    env.with_echo_agent();

    // 1. Launch plan built from the TOML files.
    let req = CreateSessionRequest {
        agent_id: "echo".into(),
        cwd: Some(std::env::temp_dir().display().to_string()),
        ..Default::default()
    };
    let plan = sessions_lib::launcher::plan(&env.config, &req).expect("plan");
    let map: HashMap<_, _> = plan.env.iter().cloned().collect();
    assert_eq!(map["MY_URL"], "https://api.test.dev/v1");
    assert_eq!(map["MY_KEY"], "sk-secret");
    assert_eq!(map["MY_MODEL"], "big-model");

    // 2. Real launch and output through the manager.
    let sink = Arc::new(Sink::default());
    let mgr = SessionManager::new(Arc::new(OutputBridge(sink.clone())), PumpConfig::default());
    let id = "e2e-1".to_string();
    let session = mgr
        .spawn_session(SpawnSpec {
            session_id: id.clone(),
            program: plan.program.clone(),
            args: plan.args.clone(),
            cwd: plan.cwd.clone(),
            env: plan.env.clone(),
            cols: 100,
            rows: 30,
            ring_bytes: 64 * 1024,
            busy_hints: vec![],
        })
        .expect("spawn");
    assert!(session.pid().is_some());
    // Nothing is delivered until a terminal attaches: the pump sends the retained
    // history first and then only newer output.
    assert!(mgr.attach(&id), "the terminal must attach");

    // 3. Metrics: the session is tracked even if the agent reports no tokens.
    let collector = Arc::new(MetricsCollector(Mutex::new(Vec::new())));
    let hub = MetricsHub::new(env.config.clone(), Arc::new(MetricsBridge(collector.clone())));
    hub.track(TrackSpec {
        session_id: id.clone(),
        agent_id: "echo".into(),
        metrics_source: MetricsSource::None,
        metrics_path: None,
        cwd: plan.cwd.display().to_string(),
        external_id: None,
        pty: session.clone(),
    });

    // 4. Session persistence.
    let project = env.store.upsert_project(&plan.cwd.display().to_string(), None);
    env.store.upsert_session(SessionMeta {
        id: id.clone(),
        project_id: project.id.clone(),
        title: "Echo".into(),
        agent_id: "echo".into(),
        cwd: plan.cwd.display().to_string(),
        created_at: now_ms(),
        pid: session.pid(),
        cols: 100,
        rows: 30,
        command_line: Some(plan.command_line.clone()),
        ..Default::default()
    });

    answer_dsr(&sink, &mgr, &id);
    assert!(wait_for(30, || sink.finished(&id)), "the process did not finish");

    let out = sink.text(&id);
    assert!(out.contains("MODEL=big-model"), "output: {out:?}");
    assert!(out.contains("URL=https://api.test.dev/v1"), "output: {out:?}");
    assert!(out.contains("KEY=sk-secret"), "output: {out:?}");

    assert!(wait_for(10, || hub
        .snapshot(&id)
        .map(|m| m.status == SessionStatus::Exited)
        .unwrap_or(false)));

    // Scrollback is kept and can be saved to and read back from disk. It stays
    // raw on purpose: the UI replays it through a terminal emulator, which is the
    // only faithful way to reproduce a TUI screen.
    env.store.save_scrollback(&id, &session.scrollback());
    let restored = String::from_utf8_lossy(&env.store.load_scrollback(&id)).to_string();
    assert!(restored.contains("MODEL=big-model"));

    hub.shutdown();
    mgr.shutdown();
}

#[test]
fn concurrent_sessions_do_not_interfere() {
    let env = Env::new("concurrent");
    env.with_echo_agent();

    let sink = Arc::new(Sink::default());
    let mgr = SessionManager::new(Arc::new(OutputBridge(sink.clone())), PumpConfig::default());

    let (program, base) = if cfg!(windows) {
        (sessions_lib::config::agents::which("cmd").unwrap(), vec!["/c".to_string()])
    } else {
        (sessions_lib::config::agents::which("sh").unwrap(), vec!["-c".to_string()])
    };

    let ids: Vec<String> = (0..4).map(|i| format!("conc-{i}")).collect();
    for (i, id) in ids.iter().enumerate() {
        let mut args = base.clone();
        args.push(format!("echo SESSION-{i}"));
        mgr.spawn_session(SpawnSpec {
            session_id: id.clone(),
            program: program.clone(),
            args,
            cwd: std::env::temp_dir(),
            env: vec![],
            cols: 80,
            rows: 24,
            ring_bytes: 32 * 1024,
            busy_hints: vec![],
        })
        .unwrap();
    }

    for id in &ids {
        assert!(mgr.attach(id), "the terminal must attach");
    }
    for id in &ids {
        answer_dsr(&sink, &mgr, id);
    }
    for id in &ids {
        assert!(wait_for(40, || sink.finished(id)), "{id} did not finish");
    }
    for (i, id) in ids.iter().enumerate() {
        let t = sink.text(id);
        assert!(t.contains(&format!("SESSION-{i}")), "{id}: {t:?}");
        for (j, _) in ids.iter().enumerate() {
            if i != j {
                assert!(!t.contains(&format!("SESSION-{j}")), "{id} polluted with SESSION-{j}");
            }
        }
    }
    mgr.shutdown();
}

#[test]
fn hot_reload_of_agents_toml() {
    let env = Env::new("reload");
    env.with_echo_agent();
    assert_eq!(env.config.agent("echo").unwrap().env["MY_MODEL"], "big-model");

    // The user edits the file while the app is running.
    let contents = std::fs::read_to_string(&env.config.paths().agents).unwrap();
    std::fs::write(
        &env.config.paths().agents,
        contents.replace("big-model", "other-model"),
    )
    .unwrap();
    let snap = env.config.reload();
    assert!(snap.issues.is_empty(), "{:?}", snap.issues);
    assert_eq!(env.config.agent("echo").unwrap().env["MY_MODEL"], "other-model");

    // A broken TOML does not take the app down: it warns and uses factory values.
    std::fs::write(&env.config.paths().agents, "]]not toml[[").unwrap();
    let snap = env.config.reload();
    assert!(snap.issues.iter().any(|i| i.file == "agents.toml"));
    assert!(!snap.agents.is_empty());
}

#[test]
fn bootstrap_creates_the_user_directory_with_the_toml_files() {
    let env = Env::new("bootstrap");
    let p = env.config.paths();
    for f in [&p.config, &p.agents] {
        assert!(f.is_file(), "missing {}", f.display());
        let text = std::fs::read_to_string(f).unwrap();
        assert!(text.len() > 200, "{} looks empty", f.display());
    }
    assert!(p.scrollback.is_dir());
    assert!(p.state.is_dir());
    assert!(p.logs.is_dir());

    // The factory agents cover the CLIs the app knows how to launch.
    let snap = env.config.snapshot();
    for agent in ["claude", "codex", "opencode", "pi", "shell"] {
        assert!(
            snap.agents.iter().any(|a| a.id == agent),
            "the factory agents do not include «{agent}»"
        );
    }
}
