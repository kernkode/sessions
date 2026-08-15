//! Per-session output ring buffer.
//!
//! Keeps the last N raw bytes (ANSI sequences included) so a terminal can be
//! rehydrated when its tab is reopened, without keeping every xterm instance
//! alive. When evicting it trims up to the next newline so playback does not
//! start in the middle of an escape sequence.

use std::collections::VecDeque;

pub struct Ring {
    buf: VecDeque<u8>,
    capacity: usize,
    /// Total bytes seen (not just the retained ones).
    total: u64,
    /// Whether history was dropped: the UI mentions it when rehydrating.
    truncated: bool,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(4 * 1024);
        Self {
            buf: VecDeque::with_capacity(capacity.min(64 * 1024)),
            capacity,
            total: 0,
            truncated: false,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        self.total += data.len() as u64;

        // A chunk larger than the capacity: only its tail can be kept.
        if data.len() >= self.capacity {
            self.buf.clear();
            let tail = &data[data.len() - self.capacity..];
            self.buf.extend(tail.iter().copied());
            self.truncated = true;
            self.align_to_line();
            return;
        }

        self.buf.extend(data.iter().copied());
        if self.buf.len() > self.capacity {
            let excess = self.buf.len() - self.capacity;
            self.buf.drain(..excess);
            self.truncated = true;
            self.align_to_line();
        }
    }

    /// Drops the leading fragment up to the first `\n` (probing at most 8 KiB).
    fn align_to_line(&mut self) {
        let limit = self.buf.len().min(8 * 1024);
        let mut cut = None;
        for i in 0..limit {
            if self.buf[i] == b'\n' {
                cut = Some(i + 1);
                break;
            }
        }
        if let Some(c) = cut {
            self.buf.drain(..c);
        }
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let (a, b) = self.buf.as_slices();
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        out
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn total_bytes(&self) -> u64 {
        self.total
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.truncated = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_up_to_capacity() {
        let mut r = Ring::new(4096);
        r.push(b"hello ");
        r.push(b"world\n");
        assert_eq!(r.snapshot(), b"hello world\n");
        assert_eq!(r.total_bytes(), 12);
        assert!(!r.truncated());
    }

    #[test]
    fn evicts_and_aligns_to_a_line() {
        let mut r = Ring::new(4096);
        // 4096 is the minimum; fill with short lines and check the cut.
        for i in 0..2000 {
            r.push(format!("line-{i}\n").as_bytes());
        }
        let snap = r.snapshot();
        assert!(snap.len() <= 4096);
        assert!(r.truncated());
        // After aligning, the buffer starts right after a newline.
        let text = String::from_utf8_lossy(&snap);
        assert!(text.starts_with("line-"), "starts with: {:?}", &text[..20.min(text.len())]);
        assert!(text.ends_with("line-1999\n"));
    }

    #[test]
    fn a_chunk_larger_than_capacity_keeps_the_tail() {
        let mut r = Ring::new(4096);
        let mut big = vec![b'x'; 10_000];
        big.extend_from_slice(b"\nEND\n");
        r.push(&big);
        let snap = r.snapshot();
        assert!(snap.len() <= 4096);
        assert_eq!(&snap, b"END\n");
        assert_eq!(r.total_bytes(), 10_005);
    }

    #[test]
    fn clear_resets_but_keeps_the_total() {
        let mut r = Ring::new(4096);
        r.push(b"something\n");
        r.clear();
        assert!(r.is_empty());
        assert_eq!(r.total_bytes(), 10);
    }
}
