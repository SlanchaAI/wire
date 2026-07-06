//! retire — decommission a wire identity you're done with, reversibly.
//!
//! The daemon supervisor keeps a daemon alive for *every* real identity (one
//! with a `private.key`) so it can still receive mail — so an idle throwaway
//! identity's daemon can't just be killed: the supervisor respawns it within
//! one poll. "Retiring" writes a durable `.retired` marker that makes the
//! supervisor treat the home as ineligible (it kills the child and never
//! respawns — see `daemon_supervisor::supervisor_eligible`), then stops the
//! running daemon directly.
//!
//! Reversible by construction: the marker is the ONLY state change. The home,
//! identity keypair, relay slot, and pull cursor are all kept, so `revive`
//! (remove the marker) brings the identity back intact and it drains any mail
//! that arrived while retired (relay slots never expire; mail is retained).
//!
//! `is_retired` is a pure existence check so a torn write can never flip an
//! identity back to "not retired". CLI-only: an agent must not retire another
//! identity's daemon unsupervised (mirrors `wire nuke`).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MARKER_SCHEMA: &str = "wire-retired-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetiredMarker {
    pub schema: String,
    pub retired_at_unix: u64,
    #[serde(default)]
    pub reason: String,
}

/// `<home>/state/wire/retired.json`.
pub fn marker_path(home: &Path) -> PathBuf {
    home.join("state").join("wire").join("retired.json")
}

/// Pure existence check — never parses the body, so a partial write can't
/// read as "not retired". The supervisor eligibility filter keys on this.
pub fn is_retired(home: &Path) -> bool {
    marker_path(home).exists()
}

/// Best-effort read of the marker body for display. A parse error is treated
/// as retired-with-unknown-details (fail closed), never as not-retired.
pub fn read_marker(home: &Path) -> Option<RetiredMarker> {
    let bytes = std::fs::read(marker_path(home)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_marker(home: &Path, reason: &str, now_unix: u64) -> Result<()> {
    let dir = home.join("state").join("wire");
    std::fs::create_dir_all(&dir)?;
    let m = RetiredMarker {
        schema: MARKER_SCHEMA.to_string(),
        retired_at_unix: now_unix,
        reason: reason.to_string(),
    };
    let tmp = dir.join("retired.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&m)?)?;
    std::fs::rename(&tmp, marker_path(home))?; // atomic
    Ok(())
}

fn remove_marker(home: &Path) -> Result<()> {
    let p = marker_path(home);
    if p.exists() {
        std::fs::remove_file(&p)?;
    }
    Ok(())
}

/// Retire a session home: write the marker FIRST (so the supervisor won't
/// respawn), then stop its daemon via the injected `stop` fn. Returns the pid
/// that was stopped, if any. Idempotent — re-retiring just rewrites the marker.
///
/// Marker-before-kill is load-bearing: reverse it and the supervisor can
/// respawn the daemon in the window between kill and marker.
pub fn retire_session<F>(home: &Path, reason: &str, now_unix: u64, stop: F) -> Result<Option<u32>>
where
    F: Fn(u32) -> bool,
{
    write_marker(home, reason, now_unix)?;
    let pid = crate::session::session_daemon_pid(home);
    if let Some(p) = pid {
        stop(p);
    }
    Ok(pid)
}

/// Bring a retired identity back: remove the marker; the supervisor respawns
/// its daemon on the next poll. No-op if not retired.
pub fn revive_session(home: &Path) -> Result<()> {
    remove_marker(home)
}

/// Stop a daemon by pid, graceful then force. Mirrors the `wire upgrade` fix:
/// a bare SIGTERM / `taskkill /PID` (no `/F`) is a no-op for a headless daemon
/// on Windows, so escalate to SIGKILL / `/F` if it's still alive after a grace.
pub fn stop_daemon_graceful_then_force(pid: u32) -> bool {
    crate::platform::kill_process(pid, false);
    for _ in 0..10 {
        if !crate::platform::process_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    crate::platform::kill_process(pid, true);
    for _ in 0..6 {
        if !crate::platform::process_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !crate::platform::process_alive(pid)
}

/// The session home THIS process resolves for itself (honoring WIRE_HOME /
/// session-key). Used to guarantee we never retire the current identity —
/// compared by canonical home path, since `resolve_session_key()` is `None`
/// on a bare terminal. `None` if it can't be resolved (caller must fail closed).
pub fn current_home() -> Option<PathBuf> {
    let cfg = crate::config::config_dir().ok()?; // <home>/config/wire
    let home = cfg.parent()?.parent()?; // <home>
    // Fail closed: only claim a home we can confirm is a real identity home
    // (has `config/wire/private.key`). Under a bare terminal with no WIRE_HOME
    // set, `config_dir()` is only one level deep (`dirs_config/wire`), so
    // `parent().parent()` walks PAST the config root into an unrelated ancestor
    // — the private.key check rejects that bogus path and returns None (the
    // caller then fails closed) instead of a plausible-looking wrong home.
    if !home
        .join("config")
        .join("wire")
        .join("private.key")
        .exists()
    {
        return None;
    }
    Some(std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf()))
}

/// True iff `home` is the current process's own identity home.
pub fn is_current(home: &Path) -> bool {
    match current_home() {
        Some(cur) => {
            let h = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
            h == cur
        }
        None => false,
    }
}

/// True iff the home has any pending inbound pair request awaiting `wire accept`
/// (`state/wire/pending-inbound-pairs/*.json`). Such a home has 0 pinned peers
/// but is NOT idle — a peer is actively trying to reach it — so the bulk sweep
/// must never retire it.
pub fn has_pending_inbound(home: &Path) -> bool {
    let dir = home
        .join("state")
        .join("wire")
        .join("pending-inbound-pairs");
    match std::fs::read_dir(&dir) {
        Ok(mut entries) => entries.any(|e| {
            e.ok()
                .and_then(|e| e.path().extension().map(|x| x == "json"))
                .unwrap_or(false)
        }),
        Err(_) => false,
    }
}

/// Seconds since this identity was created — the mtime of
/// `config/wire/private.key`, written exactly once at keygen and never
/// rewritten. This is the honest "how old is this throwaway" signal: unlike
/// `daemon.pid` (which resets to now on every supervisor respawn) or
/// `last_sync.json` (which a running daemon refreshes every heartbeat), the
/// key's mtime tracks the identity's actual age, so a freshly-created sibling
/// session is never swept. `None` if no key (not a real identity).
pub fn identity_age_s(home: &Path) -> Option<u64> {
    let p = home.join("config").join("wire").join("private.key");
    let mtime = std::fs::metadata(&p).ok()?.modified().ok()?;
    mtime.elapsed().ok().map(|d| d.as_secs())
}

/// Resolve `<handle|fingerprint|key>` to exactly one local session, box-wide
/// over `list_sessions()`. Many idle homes never claimed a handle, so the key
/// (by-key dir name) and fingerprint are also accepted. Errors on zero or
/// multiple matches — never guesses.
pub fn resolve_target(arg: &str) -> Result<crate::session::SessionInfo> {
    let a = arg.trim();
    if a.is_empty() {
        bail!("empty identity — pass a handle, fingerprint, or session key");
    }
    let sessions = crate::session::list_sessions()?;
    let matched: Vec<crate::session::SessionInfo> = sessions
        .into_iter()
        .filter(|s| {
            s.handle.as_deref() == Some(a)
                || s.name == a
                || s.did
                    .as_deref()
                    .and_then(crate::dash::fingerprint_from_did)
                    .as_deref()
                    == Some(a)
        })
        .collect();
    match matched.len() {
        0 => bail!("no wire identity matches '{a}' (try a handle, fingerprint, or `wire dash`)"),
        1 => Ok(matched.into_iter().next().unwrap()),
        n => {
            let names: Vec<String> = matched
                .iter()
                .map(|s| s.handle.clone().unwrap_or_else(|| s.name.clone()))
                .collect();
            bail!(
                "'{a}' is ambiguous — {n} identities match: {}",
                names.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_roundtrip_and_is_retired() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        assert!(!is_retired(home));
        write_marker(home, "test", 1_700_000_000).unwrap();
        assert!(is_retired(home), "marker present ⇒ retired");
        let m = read_marker(home).unwrap();
        assert_eq!(m.schema, MARKER_SCHEMA);
        assert_eq!(m.retired_at_unix, 1_700_000_000);
        assert_eq!(m.reason, "test");
        remove_marker(home).unwrap();
        assert!(!is_retired(home), "marker removed ⇒ not retired");
    }

    #[test]
    fn is_retired_is_pure_existence_not_content() {
        // A garbage (unparseable) marker still reads as retired — fail closed.
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join("state").join("wire")).unwrap();
        std::fs::write(marker_path(home), b"{ this is not json").unwrap();
        assert!(
            is_retired(home),
            "corrupt marker must still read as retired"
        );
        assert!(
            read_marker(home).is_none(),
            "corrupt body → None, but still retired"
        );
    }

    #[test]
    fn retire_writes_marker_before_kill() {
        // The stop closure asserts the marker already exists when it runs.
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        // No daemon.pid → stop never called, but marker still written.
        let called = std::cell::Cell::new(false);
        let pid = retire_session(&home, "r", 1, |_p| {
            called.set(true);
            true
        })
        .unwrap();
        assert_eq!(pid, None, "no pid file ⇒ nothing to stop");
        assert!(!called.get());
        assert!(is_retired(&home), "marker written even with no daemon");
    }

    #[test]
    fn revive_is_noop_when_not_retired() {
        let dir = tempfile::tempdir().unwrap();
        revive_session(dir.path()).unwrap();
        assert!(!is_retired(dir.path()));
    }
}
