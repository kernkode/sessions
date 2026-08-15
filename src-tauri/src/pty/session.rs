//! A single PTY session: child process, writer and ring buffer.
//!
//! Two ConPTY (Windows) behaviours shape this design:
//!
//! 1. On startup it emits `ESC[6n` (a cursor position request) and **produces no
//!    further output until it gets an answer**. The UI terminal answers it: when
//!    the ring buffer is rehydrated, xterm processes the sequence and replies,
//!    which unblocks the process.
//! 2. The master reader **never sees EOF** when the child exits. Exit is detected
//!    with `try_wait` from a supervisor, which then releases the handles
//!    (`release`), and that is what unblocks the reader thread.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};

use super::ring::Ring;

pub struct SpawnSpec {
    pub session_id: String,
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub ring_bytes: usize,
    /// Texts in the output that mean the agent is working. When present,
    /// only they mark the session as Working: the echo of the user's own
    /// typing must not count as agent activity.
    pub busy_hints: Vec<String>,
}

pub struct PtySession {
    pub id: String,
    /// `None` after `release()`: the process ended and the handles were closed.
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    ring: Mutex<Ring>,
    pid: Option<u32>,
    alive: AtomicBool,
    /// -1 until it has finished.
    exit_code: AtomicI64,
    exit_notified: AtomicBool,
    last_output_at: AtomicU64,
    /// Lowercased busy_hints from the agent definition.
    busy_hints: Vec<String>,
    max_hint_len: usize,
    /// Tail of the previous chunk, so hints split across reads still match.
    busy_tail: Mutex<Vec<u8>>,
    last_busy_at: AtomicU64,
    started_at: u64,
    cols: AtomicU64,
    rows: AtomicU64,
}

impl PtySession {
    /// Spawns the process and returns the session plus the PTY reader.
    pub fn spawn(spec: &SpawnSpec) -> Result<(Arc<Self>, Box<dyn std::io::Read + Send>)> {
        let pty = portable_pty::native_pty_system();
        let cols = spec.cols.max(20);
        let rows = spec.rows.max(4);
        let pair = pty
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("abriendo PTY")?;

        let mut cmd = CommandBuilder::new(spec.program.as_os_str());
        for a in &spec.args {
            cmd.arg(a);
        }
        cmd.cwd(&spec.cwd);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        // Agent CLIs detect colour and width from these variables.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("COLUMNS", cols.to_string());
        cmd.env("LINES", rows.to_string());

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("lanzando {}", spec.program.display()))?;
        let pid = child.process_id();

        let busy_hints: Vec<String> = spec
            .busy_hints
            .iter()
            .map(|h| h.to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        let max_hint_len = busy_hints.iter().map(|h| h.len()).max().unwrap_or(0);

        // Close the slave side in the parent: on Unix that is what surfaces EOF.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().context("clonando lector del PTY")?;
        let writer = pair.master.take_writer().context("obteniendo escritor del PTY")?;

        let session = Arc::new(Self {
            id: spec.session_id.clone(),
            master: Mutex::new(Some(pair.master)),
            writer: Mutex::new(Some(writer)),
            child: Mutex::new(child),
            ring: Mutex::new(Ring::new(spec.ring_bytes)),
            pid,
            alive: AtomicBool::new(true),
            exit_code: AtomicI64::new(-1),
            exit_notified: AtomicBool::new(false),
            last_output_at: AtomicU64::new(crate::model::now_ms()),
            busy_hints,
            max_hint_len,
            busy_tail: Mutex::new(Vec::new()),
            last_busy_at: AtomicU64::new(0),
            started_at: crate::model::now_ms(),
            cols: AtomicU64::new(cols as u64),
            rows: AtomicU64::new(rows as u64),
        });

        Ok((session, reader))
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub fn exit_code(&self) -> Option<i32> {
        let c = self.exit_code.load(Ordering::Relaxed);
        if c < 0 {
            None
        } else {
            Some(c as i32)
        }
    }

    pub fn started_at(&self) -> u64 {
        self.started_at
    }

    pub fn uptime_ms(&self) -> u64 {
        crate::model::now_ms().saturating_sub(self.started_at)
    }

    pub fn last_output_at(&self) -> u64 {
        self.last_output_at.load(Ordering::Relaxed)
    }

    /// Last instant the output contained one of the agent's busy hints.
    /// 0 when no hint has ever been seen.
    pub fn last_busy_at(&self) -> u64 {
        self.last_busy_at.load(Ordering::Relaxed)
    }

    /// `false` for agents without hints: their activity falls back to any output.
    pub fn has_busy_hints(&self) -> bool {
        !self.busy_hints.is_empty()
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols.load(Ordering::Relaxed) as u16, self.rows.load(Ordering::Relaxed) as u16)
    }

    /// Records output in the ring buffer. Returns the total number of bytes the
    /// session has produced, which the pump uses to avoid delivering twice what
    /// an attaching terminal already received in its snapshot.
    pub fn on_output(&self, data: &[u8]) -> u64 {
        let now = crate::model::now_ms();
        self.last_output_at.store(now, Ordering::Relaxed);
        if !self.busy_hints.is_empty() {
            self.scan_busy(data, now);
        }
        let mut ring = self.ring.lock();
        ring.push(data);
        ring.total_bytes()
    }

    /// Looks for the agent's busy hints in the output, with ANSI sequences
    /// stripped and a tail carried over so hints split across chunks match.
    fn scan_busy(&self, data: &[u8], now: u64) {
        let mut tail = self.busy_tail.lock();
        let mut text = String::with_capacity(tail.len() + data.len());
        text.push_str(&String::from_utf8_lossy(&tail));
        text.push_str(&String::from_utf8_lossy(data));
        // The tail is raw bytes: ANSI wrappers around the hint cost extra,
        // so budget well past the hint length.
        let keep = (self.max_hint_len * 2 + 16).min(data.len());
        tail.clear();
        tail.extend_from_slice(&data[data.len() - keep..]);
        let plain = strip_ansi(&text).to_ascii_lowercase();
        if self.busy_hints.iter().any(|h| plain.contains(h.as_str())) {
            self.last_busy_at.store(now, Ordering::Relaxed);
        }
    }

    pub fn total_output_bytes(&self) -> u64 {
        self.ring.lock().total_bytes()
    }

    /// Retained history, used to rehydrate the terminal in the UI.
    pub fn scrollback(&self) -> Vec<u8> {
        self.ring.lock().snapshot()
    }

    pub fn scrollback_truncated(&self) -> bool {
        self.ring.lock().truncated()
    }

    pub fn clear_scrollback(&self) {
        self.ring.lock().clear();
    }

    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut guard = self.writer.lock();
        let w = guard.as_mut().ok_or_else(|| anyhow!("la sesión ya terminó"))?;
        w.write_all(data).context("escribiendo al PTY")?;
        w.flush().context("flush del PTY")?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let cols = cols.max(20);
        let rows = rows.max(4);
        {
            let guard = self.master.lock();
            let m = guard.as_ref().ok_or_else(|| anyhow!("la sesión ya terminó"))?;
            m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                .context("redimensionando PTY")?;
        }
        self.cols.store(cols as u64, Ordering::Relaxed);
        self.rows.store(rows as u64, Ordering::Relaxed);
        Ok(())
    }

    /// Non-blocking check for child exit. This is the reliable path on Windows.
    pub fn poll_exit(&self) -> Option<i32> {
        if !self.alive.load(Ordering::Relaxed) {
            return self.exit_code();
        }
        // `try_lock`: if the lock is busy, retry on the supervisor's next cycle
        // instead of blocking it for every session.
        let status = self.child.try_lock().and_then(|mut c| c.try_wait().ok().flatten());
        if let Some(s) = status {
            let code = s.exit_code() as i32;
            self.mark_exited(code);
            return Some(code);
        }
        None
    }

    pub fn mark_exited(&self, code: i32) {
        self.exit_code.store(code as i64, Ordering::Relaxed);
        self.alive.store(false, Ordering::Relaxed);
    }

    /// `true` exactly once: whoever gets it is responsible for reporting the exit.
    pub fn claim_exit_notify(&self) -> bool {
        self.exit_notified
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Closes master and writer. On Windows this closes the ConPTY, which is what
    /// unblocks the reader thread; on Unix it causes EOF.
    pub fn release(&self) {
        self.writer.lock().take();
        self.master.lock().take();
    }

    /// Blocking wait for the child (used in tests and when shutting down).
    pub fn wait(&self) -> i32 {
        let code = self.child.lock().wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
        self.mark_exited(code);
        code
    }

    pub fn kill(&self) -> Result<()> {
        let res = self.child.lock().kill().context("terminando proceso");
        self.alive.store(false, Ordering::Relaxed);
        if self.exit_code.load(Ordering::Relaxed) < 0 {
            self.exit_code.store(130, Ordering::Relaxed);
        }
        res
    }
}

/// Removes CSI (`\x1b[…<letter>`) and OSC (`\x1b]…\x07`) sequences so hints
/// remain searchable when the TUI interleaves escape codes with the text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            if matches!(chars.peek(), Some('\\')) {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    pub(crate) fn shell_program(cmd: &str) -> (std::path::PathBuf, Vec<String>) {
        if cfg!(windows) {
            (
                crate::config::agents::which("cmd").expect("cmd on PATH"),
                vec!["/v:on".into(), "/c".into(), cmd.to_string()],
            )
        } else {
            (
                crate::config::agents::which("sh").expect("sh on PATH"),
                vec!["-c".to_string(), cmd.to_string()],
            )
        }
    }

    fn spec(session_id: &str, cmd: &str) -> SpawnSpec {
        let (program, args) = shell_program(cmd);
        SpawnSpec {
            session_id: session_id.to_string(),
            program,
            args,
            cwd: std::env::temp_dir(),
            env: vec![],
            cols: 80,
            rows: 24,
            ring_bytes: 64 * 1024,
            busy_hints: vec![],
        }
    }

    /// Reads the output answering `ESC[6n` the way the UI terminal would, and
    /// stops when the process dies (on Windows there is no EOF).
    fn read_until_exit(mut reader: Box<dyn Read + Send>, session: &Arc<PtySession>) -> String {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        let mut acc = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut ended: Option<Instant> = None;
        while Instant::now() < deadline {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
                session.on_output(&chunk);
                acc.extend_from_slice(&chunk);
                if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = session.write_input(b"\x1b[1;1R");
                }
            }
            // Grace period after the process exits, to collect the last output.
            if session.poll_exit().is_some() {
                match ended {
                    Some(t) if Instant::now() > t => break,
                    Some(_) => {}
                    None => ended = Some(Instant::now() + Duration::from_millis(400)),
                }
            }
        }
        session.release();
        String::from_utf8_lossy(&acc).to_string()
    }

    #[test]
    fn spawns_a_process_and_captures_output() {
        let (session, reader) = PtySession::spawn(&spec("t1", "echo HELLO-PTY")).expect("spawn");
        assert!(session.pid().is_some());

        let out = read_until_exit(reader, &session);
        assert!(out.contains("HELLO-PTY"), "actual output: {out:?}");
        assert_eq!(session.exit_code(), Some(0));
        assert!(!session.is_alive());

        let snap = String::from_utf8_lossy(&session.scrollback()).to_string();
        assert!(snap.contains("HELLO-PTY"));
        assert!(session.total_output_bytes() > 0);
    }

    #[test]
    fn env_reaches_the_child_process() {
        let cmd = if cfg!(windows) { "echo VAR=%SESSIONS_MARK%" } else { "echo VAR=$SESSIONS_MARK" };
        let (program, args) = shell_program(cmd);
        let s = SpawnSpec {
            session_id: "t2".into(),
            program,
            args,
            cwd: std::env::temp_dir(),
            env: vec![("SESSIONS_MARK".into(), "abc123".into())],
            cols: 80,
            rows: 24,
            ring_bytes: 16 * 1024,
            busy_hints: vec![],
        };
        let (session, reader) = PtySession::spawn(&s).unwrap();
        let out = read_until_exit(reader, &session);
        assert!(out.contains("VAR=abc123"), "actual output: {out:?}");
    }

    #[test]
    fn resize_updates_the_size_and_fails_after_release() {
        let (session, _reader) = PtySession::spawn(&spec("t3", "echo x")).unwrap();
        session.resize(140, 40).expect("resize");
        assert_eq!(session.size(), (140, 40));
        session.resize(1, 1).unwrap(); // minimums are applied
        assert_eq!(session.size(), (20, 4));

        session.release();
        assert!(session.resize(100, 30).is_err());
        assert!(session.write_input(b"x").is_err());
        let _ = session.kill();
    }

    #[test]
    fn kill_ends_a_long_running_process() {
        let cmd = if cfg!(windows) { "pause" } else { "sleep 60" };
        let (session, _reader) = PtySession::spawn(&spec("t4", cmd)).unwrap();
        assert!(session.poll_exit().is_none(), "should not have exited yet");
        session.kill().expect("kill");
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.poll_exit().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!session.is_alive());
        session.release();
    }

    #[test]
    fn busy_hints_match_across_chunks_and_ansi() {
        let mut s = spec("t6", "echo x");
        s.busy_hints = vec!["esc to interrupt".into()];
        let (session, _r) = PtySession::spawn(&s).unwrap();
        assert_eq!(session.last_busy_at(), 0);

        // Plain output (e.g. the user's echoed typing) is not activity.
        session.on_output(b"hello world\r\n");
        assert_eq!(session.last_busy_at(), 0, "echo must not mark busy");

        // The hint arrives wrapped in ANSI and split across two chunks.
        session.on_output(b"\x1b[2mesc to inter\x1b[22m");
        session.on_output(b"\x1b[2mrupt\x1b[22m\r\n");
        assert!(session.last_busy_at() > 0, "split hint must match");
        let _ = session.kill();
        session.release();
    }

    #[test]
    fn strip_ansi_removes_csi_and_osc() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b]0;title\x07txt"), "txt");
        assert_eq!(strip_ansi("\x1b]0;title\x1b\\txt"), "txt");
        assert_eq!(strip_ansi("\x1b[2mesc to interrupt\x1b[22m"), "esc to interrupt");
    }

    #[test]
    fn claim_exit_notify_only_once() {
        let (session, _r) = PtySession::spawn(&spec("t5", "echo x")).unwrap();
        assert!(session.claim_exit_notify());
        assert!(!session.claim_exit_notify());
        let _ = session.kill();
        session.release();
    }
}
