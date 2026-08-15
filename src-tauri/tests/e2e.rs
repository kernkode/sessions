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

    /// Defines an agent pointing at a real shell and a provider whose environment
    /// mapping can be verified in the process output.
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
model_args = ["--model", "{{model}}"]
metrics = "none"

[agent.env]
MY_URL = "agent-value"
"#
            ),
        )
        .unwrap();

        std::fs::write(
            &self.config.paths().providers,
            r#"
schema = 1

[[provider]]
id = "test"
name = "Test provider"
kind = "openai-chat"
base_url = "https://api.test.dev"
api_key = "sk-secret"
default_model = "big-model"
agents = ["echo"]

[[provider.model]]
id = "big-model"
remote_id = "test/big-1"
context_window = 123456
max_output_tokens = 4096

[provider.model.pricing]
input = 3.0
output = 15.0

[provider.env.all]
MY_KEY = "{api_key}"

[provider.env.echo]
MY_URL = "{base_url}/v1"
MY_MODEL = "{model}"
"#,
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
        provider_id: Some("test".into()),
        cwd: Some(std::env::temp_dir().display().to_string()),
        ..Default::default()
    };
    let plan = sessions_lib::launcher::plan(&env.config, &req).expect("plan");
    let map: HashMap<_, _> = plan.env.iter().cloned().collect();
    assert_eq!(map["MY_URL"], "https://api.test.dev/v1", "the provider wins over the agent");
    assert_eq!(map["MY_KEY"], "sk-secret");
    // `{model}` uses the default model's remote_id.
    assert_eq!(map["MY_MODEL"], "test/big-1");
    // The provider already injects the model, so `--model` is not added.
    assert!(!plan.args.contains(&"--model".to_string()));

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
        provider_id: Some("test".into()),
        model: Some("big-model".into()),
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
        provider_id: Some("test".into()),
        model: Some("big-model".into()),
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
    assert!(out.contains("MODEL=test/big-1"), "output: {out:?}");
    assert!(out.contains("URL=https://api.test.dev/v1"), "output: {out:?}");
    assert!(out.contains("KEY=sk-secret"), "output: {out:?}");

    // The context window comes from providers.toml.
    let m = hub.snapshot(&id).expect("metrics");
    assert_eq!(m.context_window, Some(123_456));
    assert!(wait_for(10, || hub
        .snapshot(&id)
        .map(|m| m.status == SessionStatus::Exited)
        .unwrap_or(false)));

    // Scrollback is kept and can be saved to and read back from disk. It stays
    // raw on purpose: the UI replays it through a terminal emulator, which is the
    // only faithful way to reproduce a TUI screen.
    env.store.save_scrollback(&id, &session.scrollback());
    let restored = String::from_utf8_lossy(&env.store.load_scrollback(&id)).to_string();
    assert!(restored.contains("MODEL=test/big-1"));

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
fn hot_reload_of_providers_toml() {
    let env = Env::new("reload");
    env.with_echo_agent();
    assert_eq!(env.config.context_window(Some("test"), Some("big-model")), Some(123_456));

    // The user edits the file while the app is running.
    let contents = std::fs::read_to_string(&env.config.paths().providers).unwrap();
    std::fs::write(
        &env.config.paths().providers,
        contents.replace("context_window = 123456", "context_window = 999000"),
    )
    .unwrap();
    let snap = env.config.reload();
    assert!(snap.issues.is_empty(), "{:?}", snap.issues);
    assert_eq!(env.config.context_window(Some("test"), Some("big-model")), Some(999_000));

    // A broken TOML does not take the app down: it warns and uses factory values.
    std::fs::write(&env.config.paths().providers, "]]not toml[[").unwrap();
    let snap = env.config.reload();
    assert!(snap.issues.iter().any(|i| i.file == "providers.toml"));
    assert!(!snap.providers.is_empty());
}

#[test]
fn bootstrap_creates_the_user_directory_with_the_toml_files() {
    let env = Env::new("bootstrap");
    let p = env.config.paths();
    for f in [&p.config, &p.providers, &p.agents] {
        assert!(f.is_file(), "missing {}", f.display());
        let text = std::fs::read_to_string(f).unwrap();
        assert!(text.len() > 200, "{} looks empty", f.display());
    }
    assert!(p.scrollback.is_dir());
    assert!(p.state.is_dir());
    assert!(p.logs.is_dir());

    // The factory providers cover the three main agents.
    let snap = env.config.snapshot();
    for agent in ["claude", "codex", "opencode"] {
        assert!(
            snap.providers.iter().any(|pr| pr.supported_agents.iter().any(|a| a == agent)),
            "no factory provider configures «{agent}»"
        );
    }
}
