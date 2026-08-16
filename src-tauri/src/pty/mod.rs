//! PTY session manager with output coalescing.
//!
//! Architecture (the key to the performance):
//!
//! ```text
//!  [reader thread/session]  read(64K) ──► ring buffer
//!            │                             (rehydration)
//!            └── bounded channel ──► [1 global pump thread]
//!                                      accumulates per session and flushes
//!                                      every `flush_interval_ms` or when it
//!                                      reaches `max_chunk_bytes` ──► sink (IPC)
//!
//!  [1 supervisor thread]  try_wait() every 300 ms ──► exit event + release
//! ```
//!
//! A single output thread for every session keeps the thread count bounded and,
//! more importantly, the number of IPC messages: an agent doing thousands of
//! small writes becomes ~80 messages per second per session.

pub mod ring;
pub mod session;

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Sender};
use parking_lot::RwLock;

pub use session::{PtySession, SpawnSpec};

/// Destination for already-coalesced output.
pub trait OutputSink: Send + Sync + 'static {
    fn data(&self, session_id: &str, bytes: Vec<u8>);
    fn exit(&self, session_id: &str, code: i32);
}

enum PumpMsg {
    /// Output chunk plus the total byte count of the session after it.
    Data(String, Vec<u8>, u64),
    /// A terminal starts listening: it receives the retained history and, from
    /// then on, only what is newer than it.
    Attach(String, Arc<PtySession>),
    Detach(String),
    Exit(String, i32),
    Flush(String),
    Stop,
}

#[derive(Debug, Clone, Copy)]
pub struct PumpConfig {
    pub flush_interval: Duration,
    pub max_chunk_bytes: usize,
    /// How often finished processes are checked for.
    pub supervise_interval: Duration,
}

impl Default for PumpConfig {
    fn default() -> Self {
        Self {
            flush_interval: Duration::from_millis(12),
            max_chunk_bytes: 32 * 1024,
            supervise_interval: Duration::from_millis(300),
        }
    }
}

type Sessions = Arc<RwLock<HashMap<String, Arc<PtySession>>>>;

pub struct SessionManager {
    sessions: Sessions,
    tx: Sender<PumpMsg>,
    stop: Arc<AtomicBool>,
    threads: parking_lot::Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl SessionManager {
    pub fn new(sink: Arc<dyn OutputSink>, cfg: PumpConfig) -> Self {
        // Bounded queue: if the UI stalls, readers slow down instead of eating
        // memory without limit.
        let (tx, rx) = bounded::<PumpMsg>(4096);
        let sessions: Sessions = Arc::new(RwLock::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let mut threads = Vec::with_capacity(2);
        threads.push(
            std::thread::Builder::new()
                .name("sessions-pump".into())
                .spawn(move || pump_loop(rx, sink, cfg))
                .expect("could not spawn the output thread"),
        );
        {
            let sessions = sessions.clone();
            let tx = tx.clone();
            let stop = stop.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("sessions-supervisor".into())
                    .spawn(move || supervise_loop(sessions, tx, stop, cfg.supervise_interval))
                    .expect("could not spawn the supervisor thread"),
            );
        }

        Self { sessions, tx, stop, threads: parking_lot::Mutex::new(threads) }
    }

    pub fn spawn_session(&self, spec: SpawnSpec) -> Result<Arc<PtySession>> {
        let id = spec.session_id.clone();
        let (session, mut reader) = PtySession::spawn(&spec)?;
        self.sessions.write().insert(id.clone(), session.clone());

        let tx = self.tx.clone();
        let s = session.clone();
        std::thread::Builder::new()
            .name(format!("pty-read-{id}"))
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let end = s.on_output(&buf[..n]);
                            if tx.send(PumpMsg::Data(s.id.clone(), buf[..n].to_vec(), end)).is_err() {
                                break;
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                // The exit notification is emitted by whoever wins
                // `claim_exit_notify`: here (EOF, typical on Unix) or the supervisor
                // (Windows).
                let code = s.poll_exit().unwrap_or(0);
                if s.claim_exit_notify() {
                    let _ = tx.send(PumpMsg::Exit(s.id.clone(), code));
                }
            })
            .map_err(|e| anyhow!("could not create the reader thread: {e}"))?;

        Ok(session)
    }

    pub fn get(&self, id: &str) -> Option<Arc<PtySession>> {
        self.sessions.read().get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.sessions.read().keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.sessions.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.read().is_empty()
    }

    pub fn write_input(&self, id: &str, data: &[u8]) -> Result<()> {
        self.get(id).ok_or_else(|| anyhow!("sesión {id} no encontrada"))?.write_input(data)
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        self.get(id).ok_or_else(|| anyhow!("sesión {id} no encontrada"))?.resize(cols, rows)
    }

    /// Forces an immediate flush of what is buffered (used on tab switch).
    pub fn flush(&self, id: &str) {
        let _ = self.tx.send(PumpMsg::Flush(id.to_string()));
    }

    /// Hooks a terminal up to a session: the pump sends it the retained history
    /// and then only newer output. Doing it inside the pump is what keeps the
    /// history and the live stream from overlapping.
    pub fn attach(&self, id: &str) -> bool {
        match self.get(id) {
            Some(session) => {
                let _ = self.tx.send(PumpMsg::Attach(id.to_string(), session));
                true
            }
            None => false,
        }
    }

    /// Stops delivering output for a session (its terminal was released).
    pub fn detach(&self, id: &str) {
        let _ = self.tx.send(PumpMsg::Detach(id.to_string()));
    }

    /// Kills the process and drops the session from the registry.
    pub fn close(&self, id: &str) -> Result<()> {
        if let Some(s) = self.sessions.write().remove(id) {
            if s.is_alive() {
                let _ = s.kill();
            }
            s.release();
        }
        Ok(())
    }

    pub fn close_all(&self) {
        let sessions: Vec<_> = self.sessions.write().drain().map(|(_, s)| s).collect();
        for s in sessions {
            if s.is_alive() {
                let _ = s.kill();
            }
            s.release();
        }
    }

    /// Closes everything and joins the internal threads. Call it on shutdown.
    pub fn shutdown(&self) {
        self.close_all();
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(PumpMsg::Stop);
        for h in self.threads.lock().drain(..) {
            let _ = h.join();
        }
    }
}

struct Acc {
    buf: Vec<u8>,
    since: Instant,
}

fn pump_loop(rx: crossbeam_channel::Receiver<PumpMsg>, sink: Arc<dyn OutputSink>, cfg: PumpConfig) {
    let mut acc: HashMap<String, Acc> = HashMap::new();
    // Sessions with a listening terminal, and the byte count it already has.
    let mut attached: HashMap<String, u64> = HashMap::new();

    fn flush(acc: &mut HashMap<String, Acc>, id: &str, sink: &Arc<dyn OutputSink>) {
        if let Some(a) = acc.get_mut(id) {
            if !a.buf.is_empty() {
                sink.data(id, std::mem::take(&mut a.buf));
            }
            a.since = Instant::now();
        }
    }

    loop {
        match rx.recv_timeout(cfg.flush_interval) {
            Ok(PumpMsg::Data(id, bytes, end)) => {
                // Without a terminal listening there is nothing to send: the bytes
                // are already in the ring buffer, which is what a later attach
                // replays.
                let Some(&mark) = attached.get(&id) else { continue };
                // Drop what the snapshot already covered; trim a chunk that
                // straddles the boundary.
                let start = end.saturating_sub(bytes.len() as u64);
                if end <= mark {
                    continue;
                }
                let bytes = if start < mark {
                    bytes[(mark - start) as usize..].to_vec()
                } else {
                    bytes
                };

                let a = acc.entry(id.clone()).or_insert_with(|| Acc {
                    buf: Vec::with_capacity(cfg.max_chunk_bytes.min(64 * 1024)),
                    since: Instant::now(),
                });
                a.buf.extend_from_slice(&bytes);
                if a.buf.len() >= cfg.max_chunk_bytes {
                    flush(&mut acc, &id, &sink);
                }
            }
            Ok(PumpMsg::Attach(id, session)) => {
                // The snapshot and the mark are taken together, inside the pump,
                // so nothing is delivered twice nor lost in between.
                let snapshot = session.scrollback();
                let mark = session.total_output_bytes();
                acc.remove(&id);
                attached.insert(id.clone(), mark);
                if !snapshot.is_empty() {
                    sink.data(&id, snapshot);
                }
            }
            Ok(PumpMsg::Detach(id)) => {
                attached.remove(&id);
                acc.remove(&id);
            }
            Ok(PumpMsg::Flush(id)) => flush(&mut acc, &id, &sink),
            Ok(PumpMsg::Exit(id, code)) => {
                flush(&mut acc, &id, &sink);
                acc.remove(&id);
                attached.remove(&id);
                sink.exit(&id, code);
            }
            Ok(PumpMsg::Stop) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }

        // Flush whatever has already served its coalescing window.
        let now = Instant::now();
        let due: Vec<String> = acc
            .iter()
            .filter(|(_, a)| !a.buf.is_empty() && now.duration_since(a.since) >= cfg.flush_interval)
            .map(|(k, _)| k.clone())
            .collect();
        for id in due {
            flush(&mut acc, &id, &sink);
        }
    }

    // Final flush so the last lines are not lost.
    let ids: Vec<String> = acc.keys().cloned().collect();
    for id in ids {
        flush(&mut acc, &id, &sink);
    }
}

/// Detects finished processes (essential on Windows, where the PTY reader never
/// sees EOF) and releases their handles.
fn supervise_loop(
    sessions: Sessions,
    tx: Sender<PumpMsg>,
    stop: Arc<AtomicBool>,
    interval: Duration,
) {
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(interval);
        let alive: Vec<Arc<PtySession>> = sessions.read().values().cloned().collect();
        for s in alive {
            if let Some(code) = s.poll_exit() {
                if s.claim_exit_notify() {
                    // Give the reader a moment to pick up the final output.
                    std::thread::sleep(Duration::from_millis(60));
                    let _ = tx.send(PumpMsg::Exit(s.id.clone(), code));
                    s.release();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct TestSink {
        data: Mutex<HashMap<String, Vec<u8>>>,
        messages: Mutex<usize>,
        exits: Mutex<Vec<(String, i32)>>,
    }

    impl TestSink {
        fn text(&self, id: &str) -> String {
            String::from_utf8_lossy(&self.data.lock().get(id).cloned().unwrap_or_default())
                .to_string()
        }
    }

    impl OutputSink for Arc<TestSink> {
        fn data(&self, session_id: &str, bytes: Vec<u8>) {
            *self.messages.lock() += 1;
            self.data.lock().entry(session_id.to_string()).or_default().extend_from_slice(&bytes);
        }
        fn exit(&self, session_id: &str, code: i32) {
            self.exits.lock().push((session_id.to_string(), code));
        }
    }

    fn spec(id: &str, cmd: &str) -> SpawnSpec {
        let (program, args) = session::tests::shell_program(cmd);
        SpawnSpec {
            session_id: id.to_string(),
            program,
            args,
            cwd: std::env::temp_dir(),
            env: vec![],
            cols: 100,
            rows: 30,
            ring_bytes: 256 * 1024,
            busy_hints: vec![],
        }
    }

    /// Waits for a condition, polling at short intervals.
    fn wait_for<F: Fn() -> bool>(seconds: u64, f: F) -> bool {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    /// Waits for the process to exit, answering `ESC[6n` like xterm does in the
    /// UI: without that answer ConPTY emits nothing.
    fn wait_for_exit(sink: &Arc<TestSink>, mgr: &SessionManager, id: &str, seconds: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut answered = false;
        while Instant::now() < deadline {
            if !answered && sink.text(id).contains("\u{1b}[6n") {
                let _ = mgr.write_input(id, b"\x1b[1;1R");
                answered = true;
            }
            if sink.exits.lock().iter().any(|(s, _)| s == id) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[test]
    fn output_reaches_the_sink_and_exit_is_reported() {
        let sink = Arc::new(TestSink::default());
        let mgr = SessionManager::new(Arc::new(sink.clone()), PumpConfig::default());
        mgr.spawn_session(spec("s1", "echo MARK-1")).unwrap();
        assert!(mgr.attach("s1"), "the terminal must attach");

        assert!(wait_for_exit(&sink, &mgr, "s1", 30), "no exit received");
        assert!(sink.text("s1").contains("MARK-1"), "received: {:?}", sink.text("s1"));
        assert_eq!(sink.exits.lock()[0].1, 0);
        // No duplicates even though reader and supervisor both participate.
        assert_eq!(sink.exits.lock().iter().filter(|(s, _)| s == "s1").count(), 1);
        mgr.shutdown();
    }

    #[test]
    fn coalescing_reduces_ipc_messages() {
        let sink = Arc::new(TestSink::default());
        // Wide window: many writes must be grouped into few messages.
        let cfg = PumpConfig {
            flush_interval: Duration::from_millis(120),
            max_chunk_bytes: 1024 * 1024,
            ..Default::default()
        };
        let mgr = SessionManager::new(Arc::new(sink.clone()), cfg);

        let cmd = if cfg!(windows) {
            "for /L %i in (1,1,300) do @echo line-%i"
        } else {
            "i=1; while [ $i -le 300 ]; do echo line-$i; i=$((i+1)); done"
        };
        mgr.spawn_session(spec("s2", cmd)).unwrap();
        assert!(mgr.attach("s2"), "the terminal must attach");
        assert!(wait_for_exit(&sink, &mgr, "s2", 40), "did not finish");

        let text = sink.text("s2");
        assert!(text.contains("line-1"), "missing the beginning");
        assert!(text.contains("line-300"), "missing the end");

        let msgs = *sink.messages.lock();
        assert!(msgs > 0);
        assert!(msgs < 100, "too many IPC messages for 300 lines: {msgs}");
        mgr.shutdown();
    }

    #[test]
    fn writing_to_the_child_stdin() {
        let sink = Arc::new(TestSink::default());
        let mgr = SessionManager::new(Arc::new(sink.clone()), PumpConfig::default());

        // Reads a line and echoes it back with a prefix (`!X!` = delayed expansion).
        let cmd =
            if cfg!(windows) { "set /p X= && echo GOT:!X!" } else { "read X; echo GOT:$X" };
        mgr.spawn_session(spec("s3", cmd)).unwrap();
        assert!(mgr.attach("s3"), "the terminal must attach");

        // Answer the DSR first, then send the line.
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline && !sink.text("s3").contains("\u{1b}[6n") {
            std::thread::sleep(Duration::from_millis(25));
        }
        mgr.write_input("s3", b"\x1b[1;1R").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        mgr.write_input("s3", b"joe\r").unwrap();

        assert!(wait_for_exit(&sink, &mgr, "s3", 30), "did not finish");
        assert!(sink.text("s3").contains("GOT:joe"), "received: {:?}", sink.text("s3"));
        mgr.shutdown();
    }

    #[test]
    fn close_drops_the_session_from_the_registry() {
        let sink = Arc::new(TestSink::default());
        let mgr = SessionManager::new(Arc::new(sink.clone()), PumpConfig::default());
        let cmd = if cfg!(windows) { "pause" } else { "sleep 60" };
        mgr.spawn_session(spec("s4", cmd)).unwrap();
        assert!(mgr.attach("s4"), "the terminal must attach");
        assert_eq!(mgr.len(), 1);
        mgr.close("s4").unwrap();
        assert!(mgr.is_empty());
        assert!(mgr.get("s4").is_none());
        assert!(mgr.write_input("s4", b"x").is_err());
        mgr.shutdown();
    }

    #[test]
    fn attaching_does_not_deliver_the_history_twice() {
        let sink = Arc::new(TestSink::default());
        let mgr = SessionManager::new(Arc::new(sink.clone()), PumpConfig::default());
        mgr.spawn_session(spec("dup", "echo MARK-ONCE")).unwrap();

        // Answer ConPTY's cursor query so the process produces its output, all
        // before any terminal is listening.
        std::thread::sleep(Duration::from_millis(300));
        let _ = mgr.write_input("dup", b"\x1b[1;1R");
        let session = mgr.get("dup").expect("session");
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&session.scrollback()).contains("MARK-ONCE") {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(sink.text("dup").is_empty(), "nothing should be sent before attaching");

        // On attaching, the retained history arrives exactly once: the snapshot and
        // the live stream must not overlap. A duplicated `ESC[6n` would make the
        // emulator answer twice and leave stray characters in the agent's prompt.
        assert!(mgr.attach("dup"), "the terminal must attach");
        assert!(wait_for_exit(&sink, &mgr, "dup", 30), "did not finish");

        let text = sink.text("dup");
        assert_eq!(text.matches("MARK-ONCE").count(), 1, "delivered twice: {text:?}");
        assert_eq!(text.matches("\u{1b}[6n").count(), 1, "the cursor query was duplicated");
        mgr.shutdown();
    }

    #[test]
    fn detaching_stops_the_delivery() {
        let sink = Arc::new(TestSink::default());
        let mgr = SessionManager::new(Arc::new(sink.clone()), PumpConfig::default());
        let cmd = if cfg!(windows) { "pause" } else { "sleep 60" };
        mgr.spawn_session(spec("det", cmd)).unwrap();
        assert!(mgr.attach("det"), "the terminal must attach");
        std::thread::sleep(Duration::from_millis(300));
        let _ = mgr.write_input("det", b"\x1b[1;1R");
        assert!(
            wait_for(20, || !sink.text("det").is_empty()),
            "it should be receiving output"
        );

        mgr.detach("det");
        std::thread::sleep(Duration::from_millis(200));
        let before = sink.text("det").len();
        // Anything the process writes now stays in the ring buffer, not in the UI.
        let _ = mgr.write_input("det", b"x\r\n");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(sink.text("det").len(), before, "it kept sending after detaching");

        mgr.close("det").unwrap();
        mgr.shutdown();
    }

    #[test]
    fn several_sessions_do_not_mix() {
        let sink = Arc::new(TestSink::default());
        let mgr = SessionManager::new(Arc::new(sink.clone()), PumpConfig::default());
        mgr.spawn_session(spec("a", "echo AAA")).unwrap();
        assert!(mgr.attach("a"), "the terminal must attach");
        mgr.spawn_session(spec("b", "echo BBB")).unwrap();
        assert!(mgr.attach("b"), "the terminal must attach");
        assert!(wait_for_exit(&sink, &mgr, "a", 30));
        assert!(wait_for_exit(&sink, &mgr, "b", 30));
        assert!(sink.text("a").contains("AAA"));
        assert!(!sink.text("a").contains("BBB"));
        assert!(sink.text("b").contains("BBB"));
        mgr.shutdown();
    }
}
