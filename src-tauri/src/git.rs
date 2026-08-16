//! Opencode-style workspace checkpoints backed by git.
//!
//! Every snapshot is a real commit (message prefixed `sessions-checkpoint:`);
//! a sidecar file under `.git/` keeps the chronological list of checkpoint
//! hashes plus a pointer, so undo/redo can move back and forth even after a
//! `reset --hard` orphans newer commits.
//!
//! All git calls use argv arrays (no shell), `GIT_TERMINAL_PROMPT=0` and a
//! fallback commit identity: they never hang on a prompt and work on machines
//! without a configured git user. Git failures never block the app.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Result};

const MARK: &str = "sessions-checkpoint:";

fn is_repo(cwd: &Path) -> bool {
    cwd.join(".git").is_dir()
}

fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let mut full: Vec<&str> = Vec::new();
    if args.first() == Some(&"commit") {
        full.extend(["-c", "user.name=Sessions", "-c", "user.email=sessions@local"]);
    }
    full.extend_from_slice(args);
    let out = Command::new("git")
        .args(&full)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| anyhow!("git: {e}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {}: {}",
            args.first().unwrap_or(&"?"),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Keeps the app's own data out of every checkpoint (it may hold secrets).
pub fn ensure_excluded(cwd: &Path) {
    let info = cwd.join(".git").join("info");
    if !info.is_dir() && std::fs::create_dir_all(&info).is_err() {
        return;
    }
    let excl = info.join("exclude");
    let mut cur = std::fs::read_to_string(&excl).unwrap_or_default();
    let mut changed = false;
    for pat in [".sessions/"] {
        if !cur.lines().any(|l| l.trim() == pat) {
            cur.push_str(pat);
            cur.push('\n');
            changed = true;
        }
    }
    if changed {
        let _ = std::fs::write(&excl, cur);
    }
}

/// `(dirty, branch)`.
pub fn status(cwd: &Path) -> (bool, String) {
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let dirty = git(cwd, &["status", "--porcelain"]).map(|s| !s.is_empty()).unwrap_or(false);
    (dirty, branch)
}

fn state_path(cwd: &Path) -> PathBuf {
    cwd.join(".git").join("sessions-checkpoints")
}

/// `(list oldest→newest, pointer)`.
fn read_state(cwd: &Path) -> (Vec<String>, usize) {
    let raw = std::fs::read_to_string(state_path(cwd)).unwrap_or_default();
    let mut lines = raw.lines();
    let pointer = lines.next().and_then(|l| l.trim().parse::<usize>().ok()).unwrap_or(0);
    let list = lines.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>();
    (list, pointer)
}

fn write_state(cwd: &Path, list: &[String], pointer: usize) {
    let mut s = format!("{pointer}\n");
    for h in list {
        s.push_str(h);
        s.push('\n');
    }
    let _ = std::fs::write(state_path(cwd), s);
}

/// Creates a checkpoint commit (if there is anything to commit) and records it.
/// Returns the checkpoint hash. Never fails the caller: `None` if not a repo.
pub fn snapshot(cwd: &Path, label: &str) -> Option<String> {
    if !is_repo(cwd) {
        return None;
    }
    ensure_excluded(cwd);
    let _ = git(cwd, &["add", "-A"]);
    let dirty = git(cwd, &["status", "--porcelain"]).map(|s| !s.is_empty()).unwrap_or(false);
    if dirty {
        let msg = format!("{MARK} {label}");
        let _ = git(cwd, &["commit", "-m", &msg]);
    }
    let head = git(cwd, &["rev-parse", "HEAD"]).ok()?;

    let (mut list, pointer) = read_state(cwd);
    // A new checkpoint discards any redo tail beyond the pointer.
    list.truncate(pointer + 1);
    if list.last().map(|s| s.as_str()) != Some(head.as_str()) {
        list.push(head.clone());
    }
    write_state(cwd, &list, list.len() - 1);
    Some(head)
}

fn restore(cwd: &Path, hash: &str) -> Result<()> {
    git(cwd, &["reset", "--hard", hash])?;
    let _ = git(cwd, &["clean", "-fd"]);
    Ok(())
}

/// Moves the pointer to an older checkpoint and resets the tree to it.
pub fn undo(cwd: &Path) -> Result<String> {
    let (list, pointer) = read_state(cwd);
    if list.is_empty() {
        return Err(anyhow!("no checkpoints"));
    }
    if pointer == 0 {
        return Err(anyhow!("nothing to undo"));
    }
    let np = pointer - 1;
    let h = list[np].clone();
    restore(cwd, &h)?;
    write_state(cwd, &list, np);
    Ok(h)
}

/// Moves the pointer to a newer checkpoint and resets the tree to it.
pub fn redo(cwd: &Path) -> Result<String> {
    let (list, pointer) = read_state(cwd);
    if list.is_empty() {
        return Err(anyhow!("no checkpoints"));
    }
    if pointer + 1 >= list.len() {
        return Err(anyhow!("nothing to redo"));
    }
    let np = pointer + 1;
    let h = list[np].clone();
    restore(cwd, &h)?;
    write_state(cwd, &list, np);
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sessions-git-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .unwrap()
        };
        run(&["init", "-q"]);
        run(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--allow-empty", "-q", "-m", "base"]);
        dir
    }

    #[test]
    fn snapshot_undo_redo_roundtrip() {
        let dir = repo();
        // A first checkpoint on a clean tree records HEAD.
        let a = snapshot(&dir, "one").expect("checkpoint");
        // Make a change and checkpoint again.
        std::fs::write(dir.join("f.txt"), "v1").unwrap();
        let b = snapshot(&dir, "two").expect("checkpoint");
        assert_ne!(a, b);

        // Undo returns to the state before f.txt existed.
        undo(&dir).expect("undo");
        assert!(!dir.join("f.txt").exists(), "undo must drop the file");

        // Redo brings it back.
        redo(&dir).expect("redo");
        assert!(dir.join("f.txt").exists(), "redo must restore the file");

        // One more undo reaches the first checkpoint; beyond that it errors.
        undo(&dir).unwrap();
        assert!(undo(&dir).is_err(), "no more undo");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn excludes_session_data() {
        let dir = repo();
        ensure_excluded(&dir);
        let excl = std::fs::read_to_string(dir.join(".git").join("info").join("exclude")).unwrap();
        assert!(excl.lines().any(|l| l.trim() == ".sessions/"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
