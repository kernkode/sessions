//! Metrics hub: tokens, context window, rate and resources.
//!
//! A single thread polls each agent's telemetry source with an adaptive interval
//! (faster while the session is producing output) and publishes a
//! `SessionMetrics` only when something changed, so the IPC is not flooded.

pub mod claude;
pub mod codex;
pub mod pi;
pub mod reader;
pub mod tail;
pub mod time;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::config::agents::MetricsSource;
use crate::config::ConfigStore;
use crate::model::{now_ms, SessionMetrics, SessionStatus};
use crate::pty::PtySession;
use reader::{NullReader, Usage, UsageReader};

/// Destination for published metrics.
pub trait MetricsSink: Send + Sync + 'static {
    fn metrics(&self, m: &SessionMetrics);
}

/// Everything needed to track a session.
pub struct TrackSpec {
    pub session_id: String,
    pub agent_id: String,
    pub metrics_source: MetricsSource,
    pub metrics_path: Option<String>,
    pub cwd: String,
    pub external_id: Option<String>,
    pub pty: Arc<PtySession>,
}

struct Track {
    spec: TrackSpec,
    reader: Box<dyn UsageReader>,
    usage: Usage,
    last: SessionMetrics,
    /// Exponential moving average of tok/s (smooths per-turn spikes).
    ewma_tps: f64,
    peak_tps: f64,
    previous_bytes: u64,
    bytes_ts: u64,
    next_poll: Instant,
}

pub struct MetricsHub {
    tracks: Arc<Mutex<HashMap<String, Track>>>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl MetricsHub {
    pub fn new(config: Arc<ConfigStore>, sink: Arc<dyn MetricsSink>) -> Self {
        let tracks: Arc<Mutex<HashMap<String, Track>>> = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let thread = {
            let tracks = tracks.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("sessions-metrics".into())
                .spawn(move || poll_loop(tracks, config, sink, stop))
                .expect("could not spawn the metrics thread")
        };

        Self { tracks, stop, thread: Mutex::new(Some(thread)) }
    }

    pub fn track(&self, spec: TrackSpec) {
        let reader = build_reader(&spec);
        let id = spec.session_id.clone();
        let initial = SessionMetrics {
            session_id: id.clone(),
            status: SessionStatus::Idle,
            updated_at: now_ms(),
            ..Default::default()
        };
        let track = Track {
            spec,
            reader,
            usage: Usage::default(),
            last: initial,
            ewma_tps: 0.0,
            peak_tps: 0.0,
            previous_bytes: 0,
            bytes_ts: now_ms(),
            next_poll: Instant::now(),
        };
        self.tracks.lock().insert(id, track);
    }

    pub fn untrack(&self, session_id: &str) {
        self.tracks.lock().remove(session_id);
    }

    pub fn snapshot(&self, session_id: &str) -> Option<SessionMetrics> {
        self.tracks.lock().get(session_id).map(|t| t.last.clone())
    }

    pub fn all(&self) -> Vec<SessionMetrics> {
        self.tracks.lock().values().map(|t| t.last.clone()).collect()
    }

    /// The CLI session id that was detected (to resume it later).
    pub fn external_id(&self, session_id: &str) -> Option<String> {
        self.tracks.lock().get(session_id).and_then(|t| t.usage.external_id.clone())
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.thread.lock().take() {
            let _ = h.join();
        }
        self.tracks.lock().clear();
    }
}

fn build_reader(spec: &TrackSpec) -> Box<dyn UsageReader> {
    let since = now_ms();
    match spec.metrics_source {
        MetricsSource::ClaudeJsonl => {
            let base = spec
                .metrics_path
                .clone()
                .map(std::path::PathBuf::from)
                .or_else(claude::ClaudeReader::base_dir);
            match base {
                Some(b) => Box::new(claude::ClaudeReader::new(b, &spec.cwd, since)),
                None => Box::new(NullReader),
            }
        }
        MetricsSource::CodexRollout => {
            let base = spec
                .metrics_path
                .clone()
                .map(std::path::PathBuf::from)
                .or_else(codex::CodexReader::base_dir);
            match base {
                Some(b) => Box::new(codex::CodexReader::new(
                    b,
                    &spec.cwd,
                    spec.external_id.clone(),
                    since,
                )),
                None => Box::new(NullReader),
            }
        }
        MetricsSource::PiJsonl => {
            let base = spec
                .metrics_path
                .clone()
                .map(std::path::PathBuf::from)
                .or_else(pi::PiReader::base_dir);
            match base {
                Some(b) => Box::new(pi::PiReader::new(
                    b,
                    &spec.cwd,
                    spec.external_id.clone(),
                    since,
                )),
                None => Box::new(NullReader),
            }
        }
        MetricsSource::None => Box::new(NullReader),
    }
}

fn poll_loop(
    tracks: Arc<Mutex<HashMap<String, Track>>>,
    config: Arc<ConfigStore>,
    sink: Arc<dyn MetricsSink>,
    stop: Arc<AtomicBool>,
) {
    let mut sampler = ProcSampler::new();

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
        let perf = config.perf();
        let active = Duration::from_millis(perf.metrics_poll_ms_active);
        let idle = Duration::from_millis(perf.metrics_poll_ms_idle);

        if perf.process_sample_ms > 0 {
            sampler.maybe_refresh(Duration::from_millis(perf.process_sample_ms));
        }

        let now = Instant::now();
        let mut to_publish: Vec<SessionMetrics> = Vec::new();

        {
            let mut guard = tracks.lock();
            for t in guard.values_mut() {
                if now < t.next_poll {
                    continue;
                }

                let mut status = session_status(&t.spec.pty);
                // Fallback: recent CLI activity in the JSONL counts as working,
                // so a Claude update that changes the spinner glyphs cannot
                // silently break detection while the process is alive.
                if status != SessionStatus::Exited
                    && now_ms().saturating_sub(t.usage.last_activity_ms) < 1_500
                {
                    status = SessionStatus::Working;
                }
                let working = status == SessionStatus::Working;
                t.next_poll = now + if working { active } else { idle };

                if let Some(u) = t.reader.poll() {
                    let tps = u.turn_tps();
                    if tps > 0.0 {
                        // High α: reacts fast without sawtoothing.
                        t.ewma_tps = if t.ewma_tps == 0.0 { tps } else { 0.6 * tps + 0.4 * t.ewma_tps };
                        t.peak_tps = t.peak_tps.max(tps);
                    }
                    t.usage = u;
                }

                // Byte rate: an activity signal valid for any agent.
                let total_bytes = t.spec.pty.total_output_bytes();
                let now_ms_value = now_ms();
                let dt = now_ms_value.saturating_sub(t.bytes_ts);
                let bps = if dt >= 200 {
                    let d = total_bytes.saturating_sub(t.previous_bytes) as f64 * 1000.0 / dt as f64;
                    t.previous_bytes = total_bytes;
                    t.bytes_ts = now_ms_value;
                    d
                } else {
                    t.last.bytes_per_second
                };

                let (cpu, mem) = sampler.usage(t.spec.pty.pid());
                let m = compose(t, status, total_bytes, bps, cpu, mem);

                if is_relevant_change(&t.last, &m) {
                    t.last = m.clone();
                    to_publish.push(m);
                } else {
                    t.last.updated_at = m.updated_at;
                }
            }
        }

        for m in to_publish {
            sink.metrics(&m);
        }
    }
}

fn session_status(pty: &Arc<PtySession>) -> SessionStatus {
    if !pty.is_alive() {
        return SessionStatus::Exited;
    }
    let now = now_ms();
    // Agents with busy_hints are Working only while their own activity
    // markers appear: the echo of the user's typing is not work. Agents
    // without hints (a plain terminal) fall back to any recent output.
    if pty.has_busy_hints() {
        if now.saturating_sub(pty.last_busy_at()) < 1_500 {
            SessionStatus::Working
        } else {
            SessionStatus::Idle
        }
    } else if now.saturating_sub(pty.last_output_at()) < 1_500 {
        SessionStatus::Working
    } else {
        SessionStatus::Idle
    }
}

fn compose(t: &Track, status: SessionStatus, total_bytes: u64, bps: f64, cpu: f32, mem: u64) -> SessionMetrics {
    let u = &t.usage;
    // Window and cost are whatever the agent itself reports.
    let window = u.context_window;
    let cost = u.cost_usd.unwrap_or(0.0);

    SessionMetrics {
        session_id: t.spec.session_id.clone(),
        input_tokens: u.input,
        output_tokens: u.output,
        cache_read_tokens: u.cache_read,
        cache_write_tokens: u.cache_write,
        reasoning_tokens: u.reasoning,
        total_input_tokens: u.total_input,
        total_output_tokens: u.total_output,
        total_tokens: u.total_input + u.total_output,
        context_used: u.context_used,
        context_window: window,
        tokens_per_second: round2(t.ewma_tps),
        peak_tokens_per_second: round2(t.peak_tps),
        bytes_per_second: round2(bps),
        total_bytes,
        cost_usd: cost,
        model: u.model.clone(),
        effort: u.effort.clone(),
        external_id: u.external_id.clone(),
        turns: u.turns,
        uptime_ms: t.spec.pty.uptime_ms(),
        cpu_percent: cpu,
        memory_bytes: mem,
        status,
        updated_at: now_ms(),
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Avoids publishing for irrelevant changes (e.g. only `uptime`).
fn is_relevant_change(a: &SessionMetrics, b: &SessionMetrics) -> bool {
    a.status != b.status
        || a.total_tokens != b.total_tokens
        || a.context_used != b.context_used
        || a.context_window != b.context_window
        || a.model != b.model
        || a.effort != b.effort
        || a.external_id != b.external_id
        || a.turns != b.turns
        || (a.tokens_per_second - b.tokens_per_second).abs() > 0.05
        || (a.bytes_per_second - b.bytes_per_second).abs() > 1.0
        || (a.cost_usd - b.cost_usd).abs() > 1e-9
        || (a.cpu_percent - b.cpu_percent).abs() > 0.5
        || a.memory_bytes.abs_diff(b.memory_bytes) > 4 * 1024 * 1024
        // Courtesy refresh every 2 s so the on-screen clock moves.
        || b.updated_at.saturating_sub(a.updated_at) > 2_000
}

/// CPU/RAM sampling of each session's process tree.
struct ProcSampler {
    sys: sysinfo::System,
    last: Instant,
    /// root pid → (cpu %, bytes)
    cache: HashMap<u32, (f32, u64)>,
    active: bool,
}

impl ProcSampler {
    fn new() -> Self {
        Self {
            sys: sysinfo::System::new(),
            last: Instant::now() - Duration::from_secs(60),
            cache: HashMap::new(),
            active: false,
        }
    }

    fn maybe_refresh(&mut self, every: Duration) {
        if self.last.elapsed() < every {
            return;
        }
        self.last = Instant::now();
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        self.active = true;

        // Sum per tree: agent CLIs spawn child processes (node, git...).
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, p) in self.sys.processes() {
            if let Some(parent) = p.parent() {
                children.entry(parent.as_u32()).or_default().push(pid.as_u32());
            }
        }
        let mut fresh = HashMap::new();
        for pid in self.sys.processes().keys() {
            let root = pid.as_u32();
            let mut cpu = 0.0f32;
            let mut mem = 0u64;
            let mut stack = vec![root];
            let mut seen = 0;
            while let Some(current) = stack.pop() {
                seen += 1;
                if seen > 256 {
                    break;
                }
                if let Some(p) = self.sys.process(sysinfo::Pid::from_u32(current)) {
                    cpu += p.cpu_usage();
                    mem += p.memory();
                }
                if let Some(cs) = children.get(&current) {
                    stack.extend(cs.iter().copied());
                }
            }
            fresh.insert(root, (cpu, mem));
        }
        self.cache = fresh;
    }

    fn usage(&self, pid: Option<u32>) -> (f32, u64) {
        if !self.active {
            return (0.0, 0);
        }
        pid.and_then(|p| self.cache.get(&p)).copied().unwrap_or((0.0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    struct TestSink(Mutex<Vec<SessionMetrics>>);

    impl MetricsSink for Arc<TestSink> {
        fn metrics(&self, m: &SessionMetrics) {
            self.0.lock().push(m.clone());
        }
    }

    fn temp_config() -> (Arc<ConfigStore>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sessions-hub-{}", uuid::Uuid::new_v4()));
        let store = Arc::new(ConfigStore::load(Paths::from_root(dir.clone())).unwrap());
        (store, dir)
    }

    fn test_pty(cmd: &str) -> Arc<PtySession> {
        let (program, args) = crate::pty::session::tests::shell_program(cmd);
        let spec = crate::pty::SpawnSpec {
            session_id: "m1".into(),
            program,
            args,
            cwd: std::env::temp_dir(),
            env: vec![],
            cols: 80,
            rows: 24,
            ring_bytes: 32 * 1024,
            busy_hints: vec![],
        };
        let (s, _r) = PtySession::spawn(&spec).unwrap();
        s
    }

    #[test]
    fn publishes_metrics_and_detects_exit() {
        let (config, dir) = temp_config();
        let sink = Arc::new(TestSink(Mutex::new(Vec::new())));
        let hub = MetricsHub::new(config, Arc::new(sink.clone()));

        let pty = test_pty("echo hello");
        hub.track(TrackSpec {
            session_id: "m1".into(),
            agent_id: "shell".into(),
            metrics_source: MetricsSource::None,
            metrics_path: None,
            cwd: std::env::temp_dir().display().to_string(),
            external_id: None,
            pty: pty.clone(),
        });

        // Force the process to end and wait for the hub to reflect it.
        let _ = pty.kill();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while Instant::now() < deadline {
            if let Some(m) = hub.snapshot("m1") {
                if m.status == SessionStatus::Exited {
                    seen = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(seen, "the hub must detect the exit");
        assert!(!sink.0.lock().is_empty(), "it must have published at least once");

        hub.untrack("m1");
        assert!(hub.snapshot("m1").is_none());
        hub.shutdown();
        pty.release();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn relevant_change_ignores_noise() {
        let a = SessionMetrics { updated_at: 1000, ..Default::default() };
        let mut b = a.clone();
        b.updated_at = 1500;
        assert!(!is_relevant_change(&a, &b), "only the clock changed");
        b.updated_at = 4000;
        assert!(is_relevant_change(&a, &b), "courtesy refresh");
        let mut c = a.clone();
        c.total_tokens = 10;
        assert!(is_relevant_change(&a, &c));
    }
}
