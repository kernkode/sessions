//! Metrics reader for Codex CLI.
//!
//! Real format: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
//!
//! ```json
//! {"type":"session_meta","payload":{"id":"019f...","cwd":"C:\\...","cli_version":"0.139.0"}}
//! {"type":"event_msg","payload":{"type":"token_count","info":{
//!    "total_token_usage":{"input_tokens":11659,"cached_input_tokens":8576,
//!      "output_tokens":470,"reasoning_output_tokens":273,"total_tokens":12129},
//!    "last_token_usage":{...},"model_context_window":258400}}}
//! {"type":"event_msg","payload":{"type":"task_started","model_context_window":353400}}
//! ```

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::claude::mtime_ms;
use super::reader::{Usage, UsageReader};
use super::tail::TailReader;
use super::time::parse_iso8601_ms;

pub struct CodexReader {
    base: PathBuf,
    cwd_norm: String,
    external_hint: Option<String>,
    since_ms: u64,
    tail: Option<TailReader>,
    usage: Usage,
    /// Timestamp of the previous event, to measure the real generation rate.
    previous_ts: Option<i64>,
    dirty: bool,
}

impl CodexReader {
    /// `base` is normally `~/.codex/sessions`.
    pub fn new(
        base: impl AsRef<Path>,
        cwd: &str,
        external_hint: Option<String>,
        since_ms: u64,
    ) -> Self {
        Self {
            base: base.as_ref().to_path_buf(),
            cwd_norm: normalize(cwd),
            external_hint,
            since_ms,
            tail: None,
            usage: Usage::default(),
            previous_ts: None,
            dirty: false,
        }
    }

    pub fn base_dir() -> Option<PathBuf> {
        Some(dirs::home_dir()?.join(".codex").join("sessions"))
    }

    fn locate_file(&mut self) {
        if self.tail.is_some() {
            return;
        }
        for f in recent_rollouts(&self.base, self.since_ms) {
            // When resuming a specific session, the file name is enough.
            if let Some(hint) = &self.external_hint {
                if f.to_string_lossy().contains(hint.as_str()) {
                    self.usage.external_id = Some(hint.clone());
                    self.tail = Some(TailReader::new(f));
                    return;
                }
                continue;
            }
            if let Some((id, cwd)) = read_meta(&f) {
                if normalize(&cwd) == self.cwd_norm {
                    self.usage.external_id = Some(id);
                    self.tail = Some(TailReader::new(f));
                    return;
                }
            }
        }
    }

    fn process(&mut self, line: &str) {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return,
        };
        let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_iso8601_ms);
        let payload = match v.get("payload") {
            Some(p) => p,
            None => return,
        };

        // The kind lives in `payload.type` (events) or in the envelope: the real
        // `turn_context` does not repeat the type inside the payload.
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| v.get("type").and_then(Value::as_str));

        match kind {
            Some("token_count") => {
                let info = match payload.get("info") {
                    Some(i) if !i.is_null() => i,
                    _ => return,
                };
                let g = |path: &str, k: &str| {
                    info.pointer(path).and_then(|o| o.get(k)).and_then(Value::as_u64).unwrap_or(0)
                };
                let total_in = g("/total_token_usage", "input_tokens");
                let total_out = g("/total_token_usage", "output_tokens");
                let last_in = g("/last_token_usage", "input_tokens");
                let last_out = g("/last_token_usage", "output_tokens");
                let last_cached = g("/last_token_usage", "cached_input_tokens");
                let last_reasoning = g("/last_token_usage", "reasoning_output_tokens");

                // Real rate: output token delta between events.
                if let (Some(t), Some(prev)) = (ts, self.previous_ts) {
                    let duration = (t - prev).max(0) as u64;
                    let delta = total_out.saturating_sub(self.usage.total_output);
                    if duration > 0 && duration < 30 * 60 * 1000 && delta > 0 {
                        self.usage.last_turn_output = delta;
                        self.usage.last_turn_ms = duration;
                    }
                }

                self.usage.input = last_in;
                self.usage.output = last_out;
                self.usage.cache_read = last_cached;
                self.usage.reasoning = last_reasoning;
                self.usage.total_input = total_in;
                self.usage.total_output = total_out;
                // The context is occupied by the last prompt sent.
                if last_in > 0 {
                    self.usage.context_used = last_in;
                }
                if let Some(cw) = info.get("model_context_window").and_then(Value::as_u64) {
                    if cw > 0 {
                        self.usage.context_window = Some(cw);
                    }
                }
                if ts.is_some() {
                    self.previous_ts = ts;
                }
                self.dirty = true;
            }
            Some("task_started") => {
                if let Some(cw) = payload.get("model_context_window").and_then(Value::as_u64) {
                    if cw > 0 {
                        self.usage.context_window = Some(cw);
                    }
                }
                self.usage.turns += 1;
                self.dirty = true;
            }
            Some("turn_context") => {
                if let Some(m) = payload
                    .get("model")
                    .and_then(Value::as_str)
                    .or_else(|| payload.pointer("/collaboration_mode/settings/model").and_then(Value::as_str))
                {
                    self.usage.model = Some(m.to_string());
                    self.dirty = true;
                }
            }
            _ => {}
        }

        // The first timestamp is the reference for the first interval.
        if self.previous_ts.is_none() {
            self.previous_ts = ts;
        }
    }
}

impl UsageReader for CodexReader {
    fn poll(&mut self) -> Option<Usage> {
        self.locate_file();
        let tail = self.tail.as_mut()?;
        for l in tail.read_new_lines() {
            self.process(&l);
        }
        if std::mem::take(&mut self.dirty) {
            Some(self.usage.clone())
        } else {
            None
        }
    }
}

fn normalize(p: &str) -> String {
    p.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// Candidate rollouts, newest first. Only the three newest day directories are
/// scanned, to avoid walking the whole history.
fn recent_rollouts(base: &Path, since_ms: u64) -> Vec<PathBuf> {
    let mut days: Vec<PathBuf> = Vec::new();
    collect_days(base, 0, &mut days);
    days.sort();
    days.reverse();

    let mut files: Vec<(u64, PathBuf)> = Vec::new();
    for d in days.into_iter().take(3) {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                    continue;
                }
                let mtime = e.metadata().ok().map(|m| mtime_ms(&m)).unwrap_or(0);
                if mtime + 5_000 < since_ms {
                    continue;
                }
                files.push((mtime, p));
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().map(|(_, p)| p).collect()
}

fn collect_days(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    if depth == 3 {
        out.push(dir.to_path_buf());
        return;
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_days(&p, depth + 1, out);
            } else if depth > 0 && p.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                // Flat layout (possible in older versions).
                if let Some(parent) = p.parent() {
                    let parent = parent.to_path_buf();
                    if !out.contains(&parent) {
                        out.push(parent);
                    }
                }
            }
        }
    }
}

/// `(id, cwd)` from `session_meta`, looked up in the first few lines.
fn read_meta(path: &Path) -> Option<(String, String)> {
    let contents = std::fs::read_to_string(path).ok()?;
    for l in contents.lines().take(5) {
        let v: Value = match serde_json::from_str(l) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(Value::as_str) == Some("session_meta") {
            let id = v.pointer("/payload/id").and_then(Value::as_str)?.to_string();
            let cwd = v.pointer("/payload/cwd").and_then(Value::as_str)?.to_string();
            return Some((id, cwd));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn prepare(cwd: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("sessions-codex-{}", uuid::Uuid::new_v4()));
        let day = base.join("2026").join("07").join("17");
        std::fs::create_dir_all(&day).unwrap();
        let f = day.join("rollout-2026-07-17T13-28-16-019f711f-2d29-7143-be23-d56f987933fc.jsonl");
        let meta = format!(
            r#"{{"timestamp":"2026-07-17T13:28:16.000Z","type":"session_meta","payload":{{"id":"019f711f-2d29-7143-be23-d56f987933fc","cwd":"{}","cli_version":"0.144.5"}}}}"#,
            cwd.replace('\\', "\\\\")
        );
        std::fs::write(&f, format!("{meta}\n")).unwrap();
        (base, f)
    }

    fn token_count(ts: &str, tin: u64, tout: u64, lin: u64, lout: u64, cw: u64) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{tin},"cached_input_tokens":0,"output_tokens":{tout},"reasoning_output_tokens":0,"total_tokens":{}}},"last_token_usage":{{"input_tokens":{lin},"cached_input_tokens":0,"output_tokens":{lout},"reasoning_output_tokens":0,"total_tokens":{}}},"model_context_window":{cw}}}}}}}"#,
            tin + tout,
            lin + lout
        )
    }

    #[test]
    fn reads_usage_window_and_model() {
        let cwd = "C:\\Users\\ana\\proj";
        let (base, f) = prepare(cwd);
        let mut r = CodexReader::new(&base, cwd, None, 0);

        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(
            h,
            r#"{{"timestamp":"2026-07-17T13:28:17.000Z","type":"turn_context","payload":{{"type":"turn_context","model":"gpt-5.6-sol","cwd":"x"}}}}"#
        )
        .unwrap();
        writeln!(h, "{}", token_count("2026-07-17T13:28:20.000Z", 1000, 200, 1000, 200, 258400)).unwrap();
        h.flush().unwrap();

        let u = r.poll().expect("usage");
        assert_eq!(u.total_input, 1000);
        assert_eq!(u.total_output, 200);
        assert_eq!(u.context_used, 1000);
        assert_eq!(u.context_window, Some(258_400));
        assert_eq!(u.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(u.external_id.as_deref(), Some("019f711f-2d29-7143-be23-d56f987933fc"));

        // Second event: the rate comes from the output delta.
        writeln!(h, "{}", token_count("2026-07-17T13:28:24.000Z", 3000, 600, 3000, 400, 258400)).unwrap();
        h.flush().unwrap();
        let u2 = r.poll().unwrap();
        assert_eq!(u2.total_output, 600);
        assert_eq!(u2.last_turn_output, 400);
        assert_eq!(u2.last_turn_ms, 4000);
        assert!((u2.turn_tps() - 100.0).abs() < 0.001, "tps: {}", u2.turn_tps());

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn does_not_confuse_sessions_from_another_directory() {
        let (base, _f) = prepare("C:\\other\\place");
        let mut r = CodexReader::new(&base, "C:\\Users\\ana\\proj", None, 0);
        assert!(r.poll().is_none(), "must not adopt a rollout from another cwd");
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn resuming_locates_by_id() {
        let (base, f) = prepare("C:\\does\\not\\matter");
        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(h, "{}", token_count("2026-07-17T13:28:20.000Z", 10, 5, 10, 5, 1000)).unwrap();
        h.flush().unwrap();

        let id = "019f711f-2d29-7143-be23-d56f987933fc".to_string();
        let mut r = CodexReader::new(&base, "C:\\path\\that\\does\\not\\match", Some(id.clone()), 0);
        let u = r.poll().expect("must locate it by id");
        assert_eq!(u.external_id, Some(id));
        assert_eq!(u.total_output, 5);
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn real_turn_context_does_not_repeat_the_type_in_the_payload() {
        // Real shape: the type only appears in the envelope.
        let cwd = "C:\\p2";
        let (base, f) = prepare(cwd);
        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(
            h,
            r#"{{"timestamp":"2026-07-17T13:28:17.000Z","type":"turn_context","payload":{{"turn_id":"t1","cwd":"C:\\p2","approval_policy":"on-request","model":"gpt-5.6-sol"}}}}"#
        )
        .unwrap();
        h.flush().unwrap();

        let mut r = CodexReader::new(&base, cwd, None, 0);
        let u = r.poll().expect("should detect the model");
        assert_eq!(u.model.as_deref(), Some("gpt-5.6-sol"));
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn task_started_provides_window_and_turns() {
        let cwd = "C:\\p";
        let (base, f) = prepare(cwd);
        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(
            h,
            r#"{{"timestamp":"2026-07-17T13:28:18.000Z","type":"event_msg","payload":{{"type":"task_started","model_context_window":353400}}}}"#
        )
        .unwrap();
        h.flush().unwrap();
        let mut r = CodexReader::new(&base, cwd, None, 0);
        let u = r.poll().unwrap();
        assert_eq!(u.context_window, Some(353_400));
        assert_eq!(u.turns, 1);
        std::fs::remove_dir_all(base).ok();
    }
}
