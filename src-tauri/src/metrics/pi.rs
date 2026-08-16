//! Metrics reader for pi.
//!
//! Real format: `~/.pi/agent/sessions/<cwd-slug>/<ts>_<id>.jsonl`, one JSON
//! entry per line. The slug observed on disk wraps the cwd slug in `--`:
//! `--C--Users-x-proj--`. Events of interest:
//!
//! ```json
//! {"type":"session","version":3,"id":"<id>","timestamp":"…","cwd":"C:\\…"}
//! {"type":"model_change","provider":"gorouter","modelId":"claude-opus-5",…}
//! {"type":"thinking_level_change","thinkingLevel":"medium",…}
//! {"type":"message","timestamp":"…","message":{"role":"assistant",
//!   "model":"claude-opus-5","usage":{"input":2913,"output":138,
//!   "cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}},…}}
//! ```

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::claude::{find_by_cwd, newest_file, normalize_cwd, slug_cwd};
use super::reader::{Usage, UsageReader};
use super::tail::TailReader;
use super::time::parse_iso8601_ms;

pub struct PiReader {
    dir: PathBuf,
    cwd: String,
    /// The CLI session id from a previous run: restored sessions keep their
    /// old file, which the mtime filter would otherwise reject.
    external_id: Option<String>,
    /// Only files touched at or after this instant are considered.
    since_ms: u64,
    tail: Option<TailReader>,
    usage: Usage,
    /// Timestamp of the previous entry, used to measure the turn duration.
    previous_ts: Option<i64>,
    dirty: bool,
}

impl PiReader {
    /// `base` is the sessions directory (normally `~/.pi/agent/sessions`).
    pub fn new(
        base: impl AsRef<Path>,
        cwd: &str,
        external_id: Option<String>,
        since_ms: u64,
    ) -> Self {
        let base = base.as_ref();
        let dir = resolve_project_dir(base, cwd);
        Self {
            dir,
            cwd: normalize_cwd(cwd),
            external_id,
            since_ms,
            tail: None,
            usage: Usage::default(),
            previous_ts: None,
            dirty: false,
        }
    }

    /// Default path: `~/.pi/agent/sessions`.
    pub fn base_dir() -> Option<PathBuf> {
        Some(dirs::home_dir()?.join(".pi").join("agent").join("sessions"))
    }

    fn locate_file(&mut self) {
        if self.tail.is_some() {
            return;
        }
        let candidate = self
            .external_id
            .as_deref()
            .and_then(|id| by_external_id(&self.dir, id))
            .or_else(|| newest_file(&self.dir, self.since_ms, None))
            .or_else(|| find_by_cwd(self.dir.parent(), &self.cwd, self.since_ms));
        if let Some(p) = candidate {
            // File names are `<ts>_<uuid>.jsonl`; the id lives after the last `_`.
            let id = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .and_then(|stem| stem.rsplit('_').next().map(str::to_string));
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

        match v.get("type").and_then(Value::as_str) {
            Some("session") => {
                if let Some(id) = v.get("id").and_then(Value::as_str) {
                    self.usage.external_id = Some(id.to_string());
                    self.dirty = true;
                }
            }
            // pi announces model switches as their own event: no turn needed.
            Some("model_change") => {
                if let Some(m) = v.get("modelId").and_then(Value::as_str) {
                    self.usage.model = Some(m.to_string());
                    self.dirty = true;
                }
            }
            Some("thinking_level_change") => {
                if let Some(l) = v.get("thinkingLevel").and_then(Value::as_str) {
                    self.usage.effort = Some(l.to_string());
                    self.dirty = true;
                }
            }
            Some("message") => {
                // No early returns: the timestamp bookkeeping below must run
                // for user messages too, or the turn duration measures from
                // the wrong entry.
                if let Some(msg) = v.get("message") {
                    if msg.get("role").and_then(Value::as_str) == Some("assistant") {
                        if let Some(u) = msg.get("usage") {
                            let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                            let input = g("input");
                            let output = g("output");
                            let cache_read = g("cacheRead");
                            let cache_write = g("cacheWrite");

                            self.usage.input = input;
                            self.usage.output = output;
                            self.usage.cache_read = cache_read;
                            self.usage.cache_write = cache_write;
                            self.usage.total_input += input + cache_read + cache_write;
                            self.usage.total_output += output;
                            // What occupies the window: the prompt sent on this turn.
                            self.usage.context_used = input + cache_read + cache_write;
                            self.usage.turns += 1;

                            if self.usage.model.is_none() {
                                if let Some(m) = msg.get("model").and_then(Value::as_str) {
                                    self.usage.model = Some(m.to_string());
                                }
                            }
                            // Per-message cost: accumulate the session total.
                            if let Some(c) = u.pointer("/cost/total").and_then(Value::as_f64) {
                                if c > 0.0 {
                                    let acc = self.usage.cost_usd.unwrap_or(0.0) + c;
                                    self.usage.cost_usd = Some(acc);
                                }
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
                    }
                }
            }
            _ => {}
        }

        if let Some(t) = ts {
            self.previous_ts = Some(t);
        }
    }
}

impl UsageReader for PiReader {
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

/// Files are named `<ts>_<id>.jsonl`: a restored session resumes its old file.
fn by_external_id(dir: &Path, id: &str) -> Option<PathBuf> {
    let suffix = format!("_{id}.jsonl");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(&suffix)).unwrap_or(false))
}

/// pi wraps the cwd slug in `--…--`; older builds may differ, so try the
/// plausible variants before falling back to comparing the inner `cwd`.
fn resolve_project_dir(base: &Path, cwd: &str) -> PathBuf {
    let slug = slug_cwd(cwd);
    for name in [format!("--{slug}--"), format!("-{slug}-"), slug.clone()] {
        let d = base.join(name);
        if d.is_dir() {
            return d;
        }
    }
    base.join(format!("--{slug}--"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn env(name: &str) -> (PathBuf, String) {
        let base = std::env::temp_dir().join(format!("sessions-pi-{}", uuid::Uuid::new_v4()));
        let cwd = format!("C:\\tmp\\{name}");
        let dir = base.join(format!("--{}--", slug_cwd(&cwd)));
        std::fs::create_dir_all(&dir).unwrap();
        (base, cwd)
    }

    #[test]
    fn reads_model_thinking_level_and_usage() {
        let (base, cwd) = env("p1");
        let dir = base.join(format!("--{}--", slug_cwd(&cwd)));
        let f = dir.join("2026-08-12T02-22-35-627Z_sess-1.jsonl");
        std::fs::write(&f, b"").unwrap();

        let mut r = PiReader::new(&base, &cwd, None, 0);
        assert!(r.poll().is_none(), "no data yet");

        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(
            h,
            r#"{{"type":"session","version":3,"id":"sess-1","timestamp":"2026-08-12T02:22:35.627Z","cwd":"C:\\tmp\\p1"}}"#
        )
        .unwrap();
        writeln!(
            h,
            r#"{{"type":"model_change","timestamp":"2026-08-12T02:22:35.676Z","provider":"gorouter","modelId":"claude-opus-5"}}"#
        )
        .unwrap();
        writeln!(
            h,
            r#"{{"type":"thinking_level_change","timestamp":"2026-08-12T02:22:35.677Z","thinkingLevel":"medium"}}"#
        )
        .unwrap();
        writeln!(
            h,
            r#"{{"type":"message","timestamp":"2026-08-12T02:22:40.000Z","message":{{"role":"user","content":[]}}}}"#
        )
        .unwrap();
        writeln!(
            h,
            r#"{{"type":"message","timestamp":"2026-08-12T02:22:44.000Z","message":{{"role":"assistant","model":"claude-opus-5","usage":{{"input":1000,"output":400,"cacheRead":200,"cacheWrite":0,"cost":{{"total":0.01}}}}}}}}"#
        )
        .unwrap();
        h.flush().unwrap();

        let u = r.poll().expect("usage expected");
        assert_eq!(u.model.as_deref(), Some("claude-opus-5"));
        assert_eq!(u.effort.as_deref(), Some("medium"));
        assert_eq!(u.external_id.as_deref(), Some("sess-1"));
        assert_eq!(u.input, 1000);
        assert_eq!(u.context_used, 1200);
        assert_eq!(u.turns, 1);
        assert_eq!(u.cost_usd, Some(0.01));
        // 400 tokens in 4 s = 100 tok/s.
        assert!((u.turn_tps() - 100.0).abs() < 0.001, "tps: {}", u.turn_tps());

        // A model change mid-session is picked up without a new turn.
        writeln!(
            h,
            r#"{{"type":"model_change","timestamp":"2026-08-12T02:23:01.532Z","provider":"gorouter","modelId":"glm-5.2"}}"#
        )
        .unwrap();
        h.flush().unwrap();
        let u2 = r.poll().expect("model change must emit");
        assert_eq!(u2.model.as_deref(), Some("glm-5.2"));
        assert_eq!(u2.turns, 1, "a model change is not a turn");
        std::fs::remove_dir_all(base).ok();
    }
}
