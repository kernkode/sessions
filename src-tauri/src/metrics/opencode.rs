//! Metrics reader for OpenCode.
//!
//! OpenCode keeps its state in SQLite (`~/.local/share/opencode/opencode.db`).
//! The `session` table already holds per-session totals, so there is no need to
//! walk messages:
//!
//! ```text
//! session(id, directory, title, slug, agent, model, cost,
//!         tokens_input, tokens_output, tokens_reasoning,
//!         tokens_cache_read, tokens_cache_write, time_created, time_updated)
//! ```
//!
//! It is always opened read-only so the agent's own process is never disturbed.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::reader::{Usage, UsageReader};

pub struct OpencodeReader {
    db: PathBuf,
    cwd_norm: String,
    external_hint: Option<String>,
    since_ms: u64,
    conn: Option<Connection>,
    usage: Usage,
    /// Last measurement, used to derive the generation rate.
    last_total_out: u64,
    last_ts: u64,
    last_time_updated: i64,
}

impl OpencodeReader {
    pub fn new(
        db: impl AsRef<Path>,
        cwd: &str,
        external_hint: Option<String>,
        since_ms: u64,
    ) -> Self {
        Self {
            db: db.as_ref().to_path_buf(),
            cwd_norm: normalize(cwd),
            external_hint,
            since_ms,
            conn: None,
            usage: Usage::default(),
            last_total_out: 0,
            last_ts: 0,
            last_time_updated: 0,
        }
    }

    /// Usual database locations per platform.
    pub fn default_db() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let candidates = [
            home.join(".local").join("share").join("opencode").join("opencode.db"),
            home.join("AppData").join("Local").join("opencode").join("opencode.db"),
            home.join("Library")
                .join("Application Support")
                .join("opencode")
                .join("opencode.db"),
            home.join(".opencode").join("opencode.db"),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    fn connect(&mut self) -> bool {
        if self.conn.is_some() {
            return true;
        }
        if !self.db.is_file() {
            return false;
        }
        // Read-only, and without blocking the agent if it is writing.
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        match Connection::open_with_flags(&self.db, flags) {
            Ok(c) => {
                let _ = c.busy_timeout(std::time::Duration::from_millis(150));
                self.conn = Some(c);
                true
            }
            Err(_) => false,
        }
    }

    fn query(&self) -> Option<Row> {
        let c = self.conn.as_ref()?;
        const COLS: &str = "id, coalesce(title,''), coalesce(model,''), coalesce(cost,0), \
             coalesce(tokens_input,0), coalesce(tokens_output,0), coalesce(tokens_reasoning,0), \
             coalesce(tokens_cache_read,0), coalesce(tokens_cache_write,0), coalesce(time_updated,0)";

        let map = |r: &rusqlite::Row| -> rusqlite::Result<Row> {
            Ok(Row {
                id: r.get(0)?,
                title: r.get(1)?,
                model: r.get(2)?,
                cost: r.get(3)?,
                input: r.get::<_, i64>(4)?.max(0) as u64,
                output: r.get::<_, i64>(5)?.max(0) as u64,
                reasoning: r.get::<_, i64>(6)?.max(0) as u64,
                cache_read: r.get::<_, i64>(7)?.max(0) as u64,
                cache_write: r.get::<_, i64>(8)?.max(0) as u64,
                time_updated: r.get(9)?,
            })
        };

        if let Some(id) = &self.external_hint {
            let sql = format!("select {COLS} from session where id = ?1 limit 1");
            return c.query_row(&sql, [id], map).ok();
        }

        // The session in this directory with the most recent activity.
        // `directory` is stored with `/` separators, hence the normalized compare.
        let sql = format!(
            "select {COLS} from session \
             where replace(lower(directory),'\\','/') = ?1 and coalesce(time_updated,0) >= ?2 \
             order by time_updated desc limit 1"
        );
        c.query_row(
            &sql,
            rusqlite::params![self.cwd_norm, self.since_ms as i64 - 5_000],
            map,
        )
        .ok()
    }
}

struct Row {
    id: String,
    title: String,
    model: String,
    cost: f64,
    input: u64,
    output: u64,
    reasoning: u64,
    cache_read: u64,
    cache_write: u64,
    time_updated: i64,
}

impl UsageReader for OpencodeReader {
    fn poll(&mut self) -> Option<Usage> {
        if !self.connect() {
            return None;
        }
        let r = self.query()?;
        if r.time_updated == self.last_time_updated && self.last_ts != 0 {
            return None; // nothing changed
        }

        let now = crate::model::now_ms();
        let total_out = r.output + r.reasoning;
        if self.last_ts != 0 && total_out > self.last_total_out {
            let duration = now.saturating_sub(self.last_ts);
            if duration > 0 {
                self.usage.last_turn_output = total_out - self.last_total_out;
                self.usage.last_turn_ms = duration;
            }
        }
        self.last_total_out = total_out;
        self.last_ts = now;
        self.last_time_updated = r.time_updated;

        self.usage.external_id = Some(r.id);
        self.usage.input = r.input;
        self.usage.output = r.output;
        self.usage.reasoning = r.reasoning;
        self.usage.cache_read = r.cache_read;
        self.usage.cache_write = r.cache_write;
        self.usage.total_input = r.input + r.cache_read + r.cache_write;
        self.usage.total_output = total_out;
        // OpenCode accumulates per session: the live context is the last entry.
        self.usage.context_used = r.input + r.cache_read;
        if !r.model.is_empty() {
            self.usage.model = Some(r.model);
        }
        if r.cost > 0.0 {
            self.usage.cost_usd = Some(r.cost);
        }
        if !r.title.is_empty() && self.usage.turns == 0 {
            self.usage.turns = 1;
        }
        Some(self.usage.clone())
    }
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a database with the same (partial) schema OpenCode uses.
    fn test_db() -> PathBuf {
        let p = std::env::temp_dir().join(format!("sessions-oc-{}.db", uuid::Uuid::new_v4()));
        let c = Connection::open(&p).unwrap();
        c.execute_batch(
            "create table session(
                id text primary key, project_id text, parent_id text, slug text,
                directory text, title text, version text, agent text, model text,
                cost real, tokens_input integer, tokens_output integer,
                tokens_reasoning integer, tokens_cache_read integer,
                tokens_cache_write integer, time_created integer, time_updated integer);",
        )
        .unwrap();
        c.execute(
            "insert into session(id,directory,title,model,cost,tokens_input,tokens_output,
                tokens_reasoning,tokens_cache_read,tokens_cache_write,time_created,time_updated)
             values('ses_1','C:/Users/ana/proj','Optimise build','anthropic/claude-sonnet-4-5',
                0.42, 1000, 200, 50, 300, 100, 1000, 2000)",
            [],
        )
        .unwrap();
        c.execute(
            "insert into session(id,directory,title,model,tokens_input,tokens_output,time_updated)
             values('ses_other','C:/other/place','Other',' m', 9, 9, 3000)",
            [],
        )
        .unwrap();
        p
    }

    #[test]
    fn reads_the_directory_totals() {
        let db = test_db();
        let mut r = OpencodeReader::new(&db, "C:\\Users\\ana\\proj", None, 0);
        let u = r.poll().expect("usage");
        assert_eq!(u.external_id.as_deref(), Some("ses_1"));
        assert_eq!(u.input, 1000);
        assert_eq!(u.output, 200);
        assert_eq!(u.reasoning, 50);
        assert_eq!(u.total_output, 250);
        assert_eq!(u.total_input, 1400);
        assert_eq!(u.context_used, 1300);
        assert_eq!(u.model.as_deref(), Some("anthropic/claude-sonnet-4-5"));
        assert_eq!(u.cost_usd, Some(0.42));

        // Without a change in time_updated there is no update.
        assert!(r.poll().is_none());
        std::fs::remove_file(db).ok();
    }

    #[test]
    fn computes_the_rate_between_polls() {
        let db = test_db();
        let mut r = OpencodeReader::new(&db, "C:/Users/ana/proj", None, 0);
        r.poll().unwrap();

        let c = Connection::open(&db).unwrap();
        c.execute(
            "update session set tokens_output = 400, time_updated = 5000 where id='ses_1'",
            [],
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(120));

        let u = r.poll().expect("second read");
        assert_eq!(u.total_output, 450);
        assert_eq!(u.last_turn_output, 200);
        assert!(u.last_turn_ms >= 100, "measured duration: {}", u.last_turn_ms);
        assert!(u.turn_tps() > 0.0);
        std::fs::remove_file(db).ok();
    }

    #[test]
    fn honours_the_given_id() {
        let db = test_db();
        let mut r = OpencodeReader::new(&db, "irrelevant/path", Some("ses_other".into()), 0);
        let u = r.poll().expect("usage");
        assert_eq!(u.external_id.as_deref(), Some("ses_other"));
        assert_eq!(u.input, 9);
        std::fs::remove_file(db).ok();
    }

    #[test]
    fn a_missing_database_does_not_fail() {
        let mut r = OpencodeReader::new(
            std::env::temp_dir().join("no-such-opencode-xyz.db"),
            "C:/x",
            None,
            0,
        );
        assert!(r.poll().is_none());
    }

    #[test]
    fn ignores_sessions_older_than_startup() {
        let db = test_db();
        // `since_ms` later than the row's time_updated.
        let mut r = OpencodeReader::new(&db, "C:/Users/ana/proj", None, 100_000);
        assert!(r.poll().is_none());
        std::fs::remove_file(db).ok();
    }
}
