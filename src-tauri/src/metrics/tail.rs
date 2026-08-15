//! Incremental reading of session files (JSONL) by byte offset.
//!
//! Agents append lines to their session files; re-reading the whole file on every
//! poll would be O(n²). This reader consumes only the new bytes and keeps the
//! last partial line until its `\n` arrives.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct TailReader {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl TailReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), offset: 0, partial: String::new() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Switches files (a new CLI session), resetting the state.
    pub fn retarget(&mut self, path: impl Into<PathBuf>) {
        self.path = path.into();
        self.offset = 0;
        self.partial.clear();
    }

    /// Returns the complete lines appended since the last call.
    pub fn read_new_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        let meta = match std::fs::metadata(&self.path) {
            Ok(m) => m,
            Err(_) => return lines,
        };
        let size = meta.len();
        if size < self.offset {
            // File truncated or rotated: start over.
            self.offset = 0;
            self.partial.clear();
        }
        if size == self.offset {
            return lines;
        }

        let mut f = match File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return lines,
        };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return lines;
        }
        let mut buf = Vec::with_capacity((size - self.offset).min(8 * 1024 * 1024) as usize);
        let read = BufReader::new(&mut f).take(8 * 1024 * 1024).read_to_end(&mut buf);
        let n = match read {
            Ok(n) => n as u64,
            Err(_) => return lines,
        };
        self.offset += n;

        self.partial.push_str(&String::from_utf8_lossy(&buf));
        // Split on newlines; the remainder stays as the partial line.
        while let Some(i) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=i).collect();
            let clean = line.trim_end_matches(['\n', '\r']).to_string();
            if !clean.is_empty() {
                lines.push(clean);
            }
        }
        // Avoid growing without bound if a "line" is absurdly long.
        if self.partial.len() > 4 * 1024 * 1024 {
            self.partial.clear();
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("sessions-tail-{}-{name}", uuid::Uuid::new_v4()));
        std::fs::write(&p, b"").unwrap();
        p
    }

    #[test]
    fn reads_only_what_is_new() {
        let p = temp_file("a.jsonl");
        let mut t = TailReader::new(&p);
        assert!(t.read_new_lines().is_empty());

        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "one").unwrap();
        writeln!(f, "two").unwrap();
        f.flush().unwrap();
        assert_eq!(t.read_new_lines(), vec!["one", "two"]);
        assert!(t.read_new_lines().is_empty(), "lines must not repeat");

        writeln!(f, "three").unwrap();
        f.flush().unwrap();
        assert_eq!(t.read_new_lines(), vec!["three"]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn keeps_the_partial_line() {
        let p = temp_file("b.jsonl");
        let mut t = TailReader::new(&p);
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();

        write!(f, "{{\"a\":").unwrap();
        f.flush().unwrap();
        assert!(t.read_new_lines().is_empty(), "an incomplete line is not delivered");

        writeln!(f, "1}}").unwrap();
        f.flush().unwrap();
        assert_eq!(t.read_new_lines(), vec!["{\"a\":1}"]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn restarts_if_the_file_is_truncated() {
        let p = temp_file("c.jsonl");
        let mut t = TailReader::new(&p);
        std::fs::write(&p, b"long-line-1\nlong-line-2\n").unwrap();
        assert_eq!(t.read_new_lines().len(), 2);

        std::fs::write(&p, b"new\n").unwrap();
        assert_eq!(t.read_new_lines(), vec!["new"]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn retarget_switches_files() {
        let p1 = temp_file("d1.jsonl");
        let p2 = temp_file("d2.jsonl");
        std::fs::write(&p1, b"a\n").unwrap();
        std::fs::write(&p2, b"b\n").unwrap();
        let mut t = TailReader::new(&p1);
        assert_eq!(t.read_new_lines(), vec!["a"]);
        t.retarget(&p2);
        assert_eq!(t.offset(), 0);
        assert_eq!(t.read_new_lines(), vec!["b"]);
        std::fs::remove_file(p1).ok();
        std::fs::remove_file(p2).ok();
    }

    #[test]
    fn a_missing_file_does_not_fail() {
        let mut t = TailReader::new(std::env::temp_dir().join("does-not-exist-xyz-123.jsonl"));
        assert!(t.read_new_lines().is_empty());
    }
}
