//! Metrics reader for Claude Code.
//!
//! Real format: `~/.claude/projects/<cwd-slug>/<session-id>.jsonl`, one JSON
//! entry per line. `type: "assistant"` entries carry `message.usage`:
//!
//! ```json
//! {"type":"assistant","timestamp":"2026-08-15T04:22:14.101Z","sessionId":"...",
//!  "cwd":"C:\\...","message":{"model":"claude-opus-5","usage":{
//!    "input_tokens":145230,"cache_creation_input_tokens":0,
//!    "cache_read_input_tokens":0,"output_tokens":35,
//!    "output_tokens_details":{"thinking_tokens":0}}}}
//! ```

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Usage, UsageReader};
use super::tail::TailReader;
use super::time::parse_iso8601_ms;

pub struct ClaudeReader {
    dir: PathBuf,
    cwd: String,
    /// Only files touched at or after this instant are considered.
    since_ms: u64,
    tail: Option<TailReader>,
    usage: Usage,
    /// Timestamp of the previous entry, used to measure the turn duration.
    previous_ts: Option<i64>,
    dirty: bool,
}

impl ClaudeReader {
    /// `base` is the projects directory (normally `~/.claude/projects`).
    pub fn new(base: impl AsRef<Path>, cwd: &str, since_ms: u64) -> Self {
        let base = base.as_ref();
        let dir = resolve_project_dir(base, cwd);
        Self {
            dir,
            cwd: normalize_cwd(cwd),
            since_ms,
            tail: None,
            usage: Usage::default(),
            previous_ts: None,
            dirty: false,
        }
    }

    /// Default path: `~/.claude/projects`.
    pub fn base_dir() -> Option<PathBuf> {
        Some(dirs::home_dir()?.join(".claude").join("projects"))
    }

    fn locate_file(&mut self) {
        if self.tail.is_some() {
            return;
        }
        let candidate = newest_file(&self.dir, self.since_ms)
            .or_else(|| find_by_cwd(self.dir.parent(), &self.cwd, self.since_ms));
        if let Some(p) = candidate {
            let id = p.file_stem().map(|s| s.to_string_lossy().to_string());
            self.usage.external_id = id;
            self.tail = Some(TailReader::new(p));
        }
    }

    fn process(&mut self, line: &str) {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return,
        };
        let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_iso8601_ms);

        if v.get("type").and_then(Value::as_str) == Some("assistant") {
            if let Some(u) = v.pointer("/message/usage") {
                let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                let input = g("input_tokens");
                let output = g("output_tokens");
                let cache_read = g("cache_read_input_tokens");
                let cache_write = g("cache_creation_input_tokens");
                let reasoning = u
                    .pointer("/output_tokens_details/thinking_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);

                self.usage.input = input;
                self.usage.output = output;
                self.usage.cache_read = cache_read;
                self.usage.cache_write = cache_write;
                self.usage.reasoning = reasoning;
                self.usage.total_input += input + cache_read + cache_write;
                self.usage.total_output += output;
                // What occupies the window: the prompt sent on this turn.
                self.usage.context_used = input + cache_read + cache_write;
                self.usage.turns += 1;

                if let Some(m) = v.pointer("/message/model").and_then(Value::as_str) {
                    self.usage.model = Some(m.to_string());
                }
                if let (Some(t), Some(prev)) = (ts, self.previous_ts) {
                    let duration = (t - prev).max(0) as u64;
                    // Discard huge gaps (the user away between turns).
                    if duration > 0 && duration < 30 * 60 * 1000 {
                        self.usage.last_turn_output = output;
                        self.usage.last_turn_ms = duration;
                    }
                }
                self.dirty = true;
            }
            if self.usage.external_id.is_none() {
                if let Some(id) = v.get("sessionId").and_then(Value::as_str) {
                    self.usage.external_id = Some(id.to_string());
                    self.dirty = true;
                }
            }
        }

        if let Some(t) = ts {
            self.previous_ts = Some(t);
        }
    }
}

impl UsageReader for ClaudeReader {
    fn poll(&mut self) -> Option<Usage> {
        self.locate_file();
        let tail = self.tail.as_mut()?;
        let lines = tail.read_new_lines();
        for l in lines {
            self.process(&l);
        }
        if std::mem::take(&mut self.dirty) {
            Some(self.usage.clone())
        } else {
            None
        }
    }
}

/// Claude Code's directory slug: every non-alphanumeric character becomes `-`.
/// `C:\Users\x\proj` → `C--Users-x-proj`.
pub fn slug_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn normalize_cwd(cwd: &str) -> String {
    cwd.replace('/', "\\").to_lowercase()
}

fn resolve_project_dir(base: &Path, cwd: &str) -> PathBuf {
    let direct = base.join(slug_cwd(cwd));
    if direct.is_dir() {
        return direct;
    }
    // Windows: CLIs may register the path with different separators.
    let alternate = base.join(slug_cwd(&cwd.replace('/', "\\")));
    if alternate.is_dir() {
        return alternate;
    }
    direct
}

fn newest_file(dir: &Path, since_ms: u64) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let m = e.metadata().ok()?;
        let mtime = mtime_ms(&m);
        // 5 s of slack: the file may be created just before the app looks.
        if mtime + 5_000 < since_ms {
            continue;
        }
        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, p));
        }
    }
    best.map(|(_, p)| p)
}

/// Last resort: walk the project directories and compare the `cwd` recorded
/// inside the JSONL. Only runs when the slug does not match.
fn find_by_cwd(base: Option<&Path>, cwd_norm: &str, since_ms: u64) -> Option<PathBuf> {
    let base = base?;
    let mut candidates: Vec<(u64, PathBuf)> = Vec::new();
    for e in std::fs::read_dir(base).ok()?.flatten() {
        let d = e.path();
        if !d.is_dir() {
            continue;
        }
        if let Some(f) = newest_file(&d, since_ms) {
            let mtime = std::fs::metadata(&f).ok().map(|m| mtime_ms(&m)).unwrap_or(0);
            candidates.push((mtime, f));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, f) in candidates.into_iter().take(12) {
        if let Ok(contents) = std::fs::read_to_string(&f) {
            if let Some(first) = contents.lines().find(|l| l.contains("\"cwd\"")) {
                if let Ok(v) = serde_json::from_str::<Value>(first) {
                    if let Some(c) = v.get("cwd").and_then(Value::as_str) {
                        if normalize_cwd(c) == cwd_norm {
                            return Some(f);
                        }
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn mtime_ms(m: &std::fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn slug_reproduces_the_real_directory_names() {
        assert_eq!(
            slug_cwd("C:\\Users\\KernKode\\Desktop\\my-roleplay"),
            "C--Users-KernKode-Desktop-my-roleplay"
        );
        assert_eq!(
            slug_cwd("C:\\Users\\KernKode\\AppData\\Local\\Temp\\opencode"),
            "C--Users-KernKode-AppData-Local-Temp-opencode"
        );
        assert_eq!(slug_cwd("/home/ana/proj.v2"), "-home-ana-proj-v2");
    }

    fn env(name: &str) -> (PathBuf, String) {
        let base = std::env::temp_dir().join(format!("sessions-claude-{}", uuid::Uuid::new_v4()));
        let cwd = format!("C:\\tmp\\{name}");
        let dir = base.join(slug_cwd(&cwd));
        std::fs::create_dir_all(&dir).unwrap();
        (base, cwd)
    }

    fn assistant_line(ts: &str, input: u64, cache_read: u64, output: u64, model: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"sess-xyz","cwd":"C:\\tmp\\p","message":{{"model":"{model}","usage":{{"input_tokens":{input},"cache_creation_input_tokens":0,"cache_read_input_tokens":{cache_read},"output_tokens":{output},"output_tokens_details":{{"thinking_tokens":7}}}}}}}}"#
        )
    }

    #[test]
    fn accumulates_tokens_and_computes_turn_tps() {
        let (base, cwd) = env("p1");
        let dir = base.join(slug_cwd(&cwd));
        let f = dir.join("sess-xyz.jsonl");
        std::fs::write(&f, b"").unwrap();

        let mut r = ClaudeReader::new(&base, &cwd, 0);
        assert!(r.poll().is_none(), "no data yet");

        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        // A user entry that marks the start of the turn.
        writeln!(h, r#"{{"type":"user","timestamp":"2026-08-15T04:22:10.000Z"}}"#).unwrap();
        writeln!(h, "{}", assistant_line("2026-08-15T04:22:14.000Z", 1000, 200, 400, "claude-opus-5")).unwrap();
        h.flush().unwrap();

        let u = r.poll().expect("usage expected");
        assert_eq!(u.input, 1000);
        assert_eq!(u.cache_read, 200);
        assert_eq!(u.output, 400);
        assert_eq!(u.reasoning, 7);
        assert_eq!(u.context_used, 1200);
        assert_eq!(u.total_output, 400);
        assert_eq!(u.turns, 1);
        assert_eq!(u.model.as_deref(), Some("claude-opus-5"));
        assert_eq!(u.external_id.as_deref(), Some("sess-xyz"));
        // 400 tokens in 4 s = 100 tok/s.
        assert_eq!(u.last_turn_ms, 4000);
        assert!((u.turn_tps() - 100.0).abs() < 0.001, "tps: {}", u.turn_tps());

        // Second turn: accumulates and does not repeat the first.
        writeln!(h, r#"{{"type":"user","timestamp":"2026-08-15T04:23:00.000Z"}}"#).unwrap();
        writeln!(h, "{}", assistant_line("2026-08-15T04:23:02.000Z", 1600, 400, 100, "claude-opus-5")).unwrap();
        h.flush().unwrap();
        let u2 = r.poll().unwrap();
        assert_eq!(u2.turns, 2);
        assert_eq!(u2.total_output, 500);
        assert_eq!(u2.context_used, 2000);
        assert!(r.poll().is_none(), "no new lines means no update");

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn ignores_corrupt_lines() {
        let (base, cwd) = env("p2");
        let dir = base.join(slug_cwd(&cwd));
        let f = dir.join("s.jsonl");
        let contents = format!(
            "not json\n{}\n{{\"type\":\"assistant\"}}\n",
            assistant_line("2026-08-15T04:22:14.000Z", 10, 0, 5, "m")
        );
        std::fs::write(&f, contents).unwrap();

        let mut r = ClaudeReader::new(&base, &cwd, 0);
        let u = r.poll().expect("usage");
        assert_eq!(u.turns, 1);
        assert_eq!(u.output, 5);
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn locates_by_cwd_when_the_slug_does_not_match() {
        let base = std::env::temp_dir().join(format!("sessions-claude-{}", uuid::Uuid::new_v4()));
        // A directory whose name is not derived from the cwd.
        let dir = base.join("unexpected-name");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("other.jsonl");
        std::fs::write(
            &f,
            format!(
                "{{\"type\":\"user\",\"cwd\":\"C:\\\\tmp\\\\odd\",\"timestamp\":\"2026-08-15T04:22:10.000Z\"}}\n{}\n",
                assistant_line("2026-08-15T04:22:12.000Z", 50, 0, 20, "m")
            ),
        )
        .unwrap();

        let mut r = ClaudeReader::new(&base, "C:\\tmp\\odd", 0);
        let u = r.poll().expect("should find it by comparing cwd");
        assert_eq!(u.output, 20);
        assert_eq!(u.external_id.as_deref(), Some("other"));
        std::fs::remove_dir_all(base).ok();
    }
}
