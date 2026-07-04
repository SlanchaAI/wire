//! dash — a read-only observability snapshot across every wire identity on
//! this machine.
//!
//! `collect()` walks the local session store (via [`crate::session::list_sessions`])
//! and enriches each session with its daemon liveness, relay binding, pinned
//! peers, and sync recency. It powers `wire dash` (the terminal pane) and the
//! Mission Control reporter.
//!
//! Hard invariants (a naive aggregate over ~270 sessions dies without these):
//! - **Read-only, no spawn, no kill.** Never starts or stops a daemon.
//! - **No per-session network I/O.** Reads on-disk state only. Relay `/healthz`
//!   is probed once per *distinct* relay, and only when explicitly asked
//!   ([`CollectOpts::probe_relays`]).
//! - **Explicit paths, never the session-scoped config helpers.**
//!   [`crate::config::read_trust`] / `read_relay_state` resolve against the
//!   *current* session's home (WIRE_HOME / session-key context); using them in
//!   a cross-session walk would read the same session 270 times. Every read
//!   here is rooted at the session's own `home_dir`.

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

/// Liveness of a session's sync daemon, derived from its `daemon.pid` file +
/// the pid-alive check [`crate::session::list_sessions`] already computes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DaemonState {
    /// `daemon.pid` present and the recorded pid is a live process.
    Running { pid: u32 },
    /// `daemon.pid` present but the process is gone (a true husk).
    StalePid { pid: u32 },
    /// No `daemon.pid` file.
    None,
}

impl DaemonState {
    pub fn is_running(&self) -> bool {
        matches!(self, DaemonState::Running { .. })
    }
}

/// One pinned peer of a session, read from its `trust.json`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PeerRow {
    pub handle: String,
    pub did: String,
    pub tier: String,
}

/// A single wire identity on this box + its live-ish state.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    /// Session key/name (the `by-key/<hash>` dir name, or a named session).
    pub key: String,
    pub handle: Option<String>,
    pub did: Option<String>,
    /// Short DID fingerprint (the trailing hex segment of the DID).
    pub fingerprint: Option<String>,
    pub nickname: Option<String>,
    pub emoji: Option<String>,
    /// Primary persona color as `#rrggbb`, for UI/JSON consumers.
    pub primary_hex: Option<String>,
    /// Primary persona color as an ANSI-256 index, for terminal glyph coloring.
    pub ansi256_primary: Option<u8>,
    pub daemon: DaemonState,
    pub daemon_version: Option<String>,
    pub relay_url: Option<String>,
    pub slot_id: Option<String>,
    /// Seconds since this session's daemon last synced (mtime of
    /// `state/wire/last_sync.json`). `None` if it never synced.
    pub last_sync_age_s: Option<u64>,
    pub peers: Vec<PeerRow>,
    pub cwd: Option<String>,
    /// A running daemon with no real pinned peers — the throwaway
    /// Claude-session daemon pattern, a candidate for the husk reaper.
    /// Deliberately NOT a "usage" claim: a live daemon heartbeat-syncs
    /// regardless of use, so peers (not sync-age) are the honest signal.
    pub likely_idle: bool,
}

/// `/healthz` result for one distinct relay URL.
#[derive(Debug, Clone, Serialize)]
pub struct RelayHealth {
    pub url: String,
    pub ok: bool,
    pub status: Option<u16>,
    /// True when not probed (probe is opt-in); `ok`/`status` are then unknown.
    pub unprobed: bool,
}

/// The whole-machine snapshot — the golden surface `--json` emits and the
/// Mission Control reporter consumes.
#[derive(Debug, Clone, Serialize)]
pub struct DashReport {
    pub schema: &'static str,
    pub sessions: Vec<SessionSnapshot>,
    pub relays: Vec<RelayHealth>,
}

#[derive(Debug, Clone, Default)]
pub struct CollectOpts {
    /// Probe each distinct relay's `/healthz` (one blocking GET per relay,
    /// 2s timeout). Off by default: `dash` is a local pane; network is opt-in.
    pub probe_relays: bool,
}

pub const SCHEMA: &str = "wire-dash-v1";

/// Extract the short fingerprint from a DID (`did:wire:terra-plain-e6511a52`
/// → `e6511a52`). The nickname may contain hyphens, so take the final segment.
pub fn fingerprint_from_did(did: &str) -> Option<String> {
    let tail = did.rsplit(':').next()?; // "terra-plain-e6511a52"
    tail.rsplit('-').next().map(|s| s.to_string())
}

/// Read a session's pinned peers from `<home>/config/wire/trust.json`,
/// excluding the session's own identity (`trust.json` lists self as an agent).
pub fn read_peers(home: &Path, own_did: Option<&str>) -> Vec<PeerRow> {
    let path = home.join("config").join("wire").join("trust.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Vec::new();
    };
    let Some(agents) = v.get("agents").and_then(|a| a.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (handle, rec) in agents {
        let did = rec.get("did").and_then(|d| d.as_str()).unwrap_or("");
        // Skip the self entry — trust.json always lists the owning identity.
        if let Some(own) = own_did
            && did == own
        {
            continue;
        }
        out.push(PeerRow {
            handle: handle.clone(),
            did: did.to_string(),
            tier: rec
                .get("tier")
                .and_then(|t| t.as_str())
                .unwrap_or("UNTRUSTED")
                .to_string(),
        });
    }
    out.sort_by(|a, b| a.handle.cmp(&b.handle));
    out
}

/// Read `<home>/config/wire/relay.json` → `(relay_url, slot_id)`.
pub fn read_relay_binding(home: &Path) -> (Option<String>, Option<String>) {
    let path = home.join("config").join("wire").join("relay.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return (None, None);
    };
    let sf = v.get("self");
    let url = sf
        .and_then(|s| s.get("relay_url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());
    let slot = sf
        .and_then(|s| s.get("slot_id"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());
    (url, slot)
}

/// Seconds since `<home>/state/wire/last_sync.json` was last written.
pub fn last_sync_age_s(home: &Path) -> Option<u64> {
    let path = home.join("state").join("wire").join("last_sync.json");
    let mtime = std::fs::metadata(&path).ok()?.modified().ok()?;
    mtime.elapsed().ok().map(|d| d.as_secs())
}

/// Read the daemon `version` from `<home>/state/wire/daemon.pid`.
fn read_daemon_version(home: &Path) -> Option<String> {
    let path = home.join("state").join("wire").join("daemon.pid");
    let bytes = std::fs::read(&path).ok()?;
    let v = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
    v.get("version")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

/// Build the snapshot for one already-enumerated session. Pure w.r.t. the
/// network; only reads files under `si.home_dir`.
fn snapshot_one(si: &crate::session::SessionInfo) -> SessionSnapshot {
    let home = &si.home_dir;
    let pid = crate::session::session_daemon_pid(home);
    let daemon = match (pid, si.daemon_running) {
        (Some(pid), true) => DaemonState::Running { pid },
        (Some(pid), false) => DaemonState::StalePid { pid },
        (None, _) => DaemonState::None,
    };
    let peers = read_peers(home, si.did.as_deref());
    let (relay_url, slot_id) = read_relay_binding(home);
    let (nickname, emoji, primary_hex, ansi256_primary) = match &si.character {
        Some(c) => (
            Some(c.nickname.clone()),
            Some(c.emoji.clone()),
            Some(c.palette.primary_hex.clone()),
            Some(c.palette.ansi256_primary),
        ),
        None => (None, None, None, None),
    };
    let likely_idle = daemon.is_running() && peers.is_empty();
    SessionSnapshot {
        key: si.name.clone(),
        handle: si.handle.clone(),
        fingerprint: si.did.as_deref().and_then(fingerprint_from_did),
        did: si.did.clone(),
        nickname,
        emoji,
        primary_hex,
        ansi256_primary,
        daemon,
        daemon_version: read_daemon_version(home),
        relay_url,
        slot_id,
        last_sync_age_s: last_sync_age_s(home),
        peers,
        cwd: si.cwd.clone(),
        likely_idle,
    }
}

fn probe_relay(url: &str) -> RelayHealth {
    let base = url.trim_end_matches('/');
    let build = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();
    let Ok(client) = build else {
        return RelayHealth {
            url: url.to_string(),
            ok: false,
            status: None,
            unprobed: false,
        };
    };
    match client.get(format!("{base}/healthz")).send() {
        Ok(r) => RelayHealth {
            url: url.to_string(),
            ok: r.status().is_success(),
            status: Some(r.status().as_u16()),
            unprobed: false,
        },
        Err(_) => RelayHealth {
            url: url.to_string(),
            ok: false,
            status: None,
            unprobed: false,
        },
    }
}

/// Snapshot every wire identity on this box. Read-only; never spawns/kills a
/// daemon; does network I/O only when `opts.probe_relays` is set.
pub fn collect(opts: &CollectOpts) -> anyhow::Result<DashReport> {
    let sessions = crate::session::list_sessions()?;
    let mut snaps = Vec::with_capacity(sessions.len());
    let mut relay_urls: BTreeSet<String> = BTreeSet::new();
    for si in &sessions {
        let snap = snapshot_one(si);
        if let Some(u) = &snap.relay_url {
            relay_urls.insert(u.clone());
        }
        snaps.push(snap);
    }
    // Sort: paired sessions first (most peers), then by daemon liveness,
    // then name — so the wires that matter float to the top and the idle
    // throwaways sink.
    snaps.sort_by(|a, b| {
        b.peers
            .len()
            .cmp(&a.peers.len())
            .then(b.daemon.is_running().cmp(&a.daemon.is_running()))
            .then(a.key.cmp(&b.key))
    });
    let relays = relay_urls
        .into_iter()
        .map(|u| {
            if opts.probe_relays {
                probe_relay(&u)
            } else {
                RelayHealth {
                    url: u,
                    ok: false,
                    status: None,
                    unprobed: true,
                }
            }
        })
        .collect();
    Ok(DashReport {
        schema: SCHEMA,
        sessions: snaps,
        relays,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn fingerprint_extracts_trailing_hex() {
        assert_eq!(
            fingerprint_from_did("did:wire:terra-plain-e6511a52").as_deref(),
            Some("e6511a52")
        );
        // Multi-hyphen nickname must not confuse the parse.
        assert_eq!(
            fingerprint_from_did("did:wire:a-b-c-deadbeef").as_deref(),
            Some("deadbeef")
        );
        assert_eq!(fingerprint_from_did("garbage").as_deref(), Some("garbage"));
    }

    #[test]
    fn read_peers_excludes_self_and_reads_tier() {
        let dir = tempfile::tempdir().unwrap();
        let cw = dir.path().join("config").join("wire");
        fs::create_dir_all(&cw).unwrap();
        fs::write(
            cw.join("trust.json"),
            r#"{"agents":{
                "terra-plain":{"did":"did:wire:terra-plain-e6511a52","tier":"ATTESTED"},
                "raven-kettle":{"did":"did:wire:raven-kettle-11112222","tier":"VERIFIED"}
            },"version":1}"#,
        )
        .unwrap();
        let peers = read_peers(dir.path(), Some("did:wire:terra-plain-e6511a52"));
        assert_eq!(peers.len(), 1, "self must be excluded");
        assert_eq!(peers[0].handle, "raven-kettle");
        assert_eq!(peers[0].tier, "VERIFIED");
    }

    #[test]
    fn read_peers_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_peers(dir.path(), None).is_empty());
    }

    #[test]
    fn read_relay_binding_parses_self() {
        let dir = tempfile::tempdir().unwrap();
        let cw = dir.path().join("config").join("wire");
        fs::create_dir_all(&cw).unwrap();
        fs::write(
            cw.join("relay.json"),
            r#"{"self":{"relay_url":"https://wireup.net","slot_id":"abc123"},"peers":{}}"#,
        )
        .unwrap();
        let (url, slot) = read_relay_binding(dir.path());
        assert_eq!(url.as_deref(), Some("https://wireup.net"));
        assert_eq!(slot.as_deref(), Some("abc123"));
    }

    #[test]
    fn daemon_state_serializes_with_tag() {
        let j = serde_json::to_value(DaemonState::Running { pid: 42 }).unwrap();
        assert_eq!(j["state"], "running");
        assert_eq!(j["pid"], 42);
        let n = serde_json::to_value(DaemonState::None).unwrap();
        assert_eq!(n["state"], "none");
    }
}
