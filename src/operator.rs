use serde::Serialize;
use time::OffsetDateTime;

pub const LIVE_SESSION_SCHEMA: &str = "wire-live-sessions-v1";

#[derive(Clone, Debug, Serialize)]
pub struct LiveSession {
    pub id: String,
    pub handle: String,
    pub did: String,
    pub emoji: String,
    pub primary_hex: String,
    pub agent_host: String,
    pub project_dir: Option<String>,
    pub started_at: Option<String>,
    pub age_seconds: Option<u64>,
    pub direct_link_count: usize,
    pub health: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveSessionReport {
    pub schema: &'static str,
    pub sessions: Vec<LiveSession>,
}

pub fn collect_live_sessions() -> anyhow::Result<LiveSessionReport> {
    let sessions = crate::session::list_sessions()?;
    collect_live_from(
        &sessions,
        OffsetDateTime::now_utc(),
        crate::platform::process_alive,
    )
}

fn collect_live_from(
    sessions: &[crate::session::SessionInfo],
    now: OffsetDateTime,
    is_alive: impl Fn(u32) -> bool + Copy,
) -> anyhow::Result<LiveSessionReport> {
    let mut live = Vec::new();
    for session in sessions {
        let (Some(did), Some(handle)) = (session.did.as_deref(), session.handle.as_deref()) else {
            continue;
        };
        if crate::retire::is_retired(&session.home_dir) {
            continue;
        }
        let leases = crate::session_lifecycle::active_leases_at(&session.home_dir, now, is_alive);
        let Some(lease) = leases
            .iter()
            .filter(|lease| lease.role == "mcp")
            .max_by(|left, right| left.heartbeat_at.cmp(&right.heartbeat_at))
        else {
            continue;
        };
        let character = session
            .character
            .clone()
            .unwrap_or_else(|| crate::character::Character::from_did(did));
        let peers = crate::dash::read_peers(&session.home_dir, Some(did), Some(handle));
        let age_seconds = lease.started_at.as_deref().and_then(|started| {
            OffsetDateTime::parse(started, &time::format_description::well_known::Rfc3339)
                .ok()
                .and_then(|started| {
                    let seconds = (now - started).whole_seconds();
                    (seconds >= 0).then_some(seconds as u64)
                })
        });
        let health = if !session.daemon_running {
            "daemon-down"
        } else if crate::dash::last_sync_age_s(&session.home_dir).is_some_and(|age| age > 60) {
            "sync-stale"
        } else {
            "healthy"
        };
        live.push(LiveSession {
            id: session.name.clone(),
            handle: handle.to_string(),
            did: did.to_string(),
            emoji: character.emoji,
            primary_hex: character.palette.primary_hex,
            agent_host: lease.session_source.clone(),
            project_dir: lease.cwd.clone().or_else(|| session.cwd.clone()),
            started_at: lease.started_at.clone(),
            age_seconds,
            direct_link_count: peers.len(),
            health: health.to_string(),
        });
    }
    live.sort_by(|left, right| left.handle.cmp(&right.handle));
    Ok(LiveSessionReport {
        schema: LIVE_SESSION_SCHEMA,
        sessions: live,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use tempfile::tempdir;
    use time::OffsetDateTime;

    use super::*;

    fn session(home: &Path, suffix: &str, daemon_running: bool) -> crate::session::SessionInfo {
        let did = format!("did:wire:session-{suffix}");
        crate::session::SessionInfo {
            name: format!("session-{suffix}"),
            cwd: Some(format!("/projects/{suffix}")),
            home_dir: home.to_path_buf(),
            did: Some(did.clone()),
            handle: Some(format!("session-{suffix}")),
            daemon_running,
            character: Some(crate::character::Character::from_did(&did)),
        }
    }

    fn lease(home: &Path, role: &str, pid: u32, now: OffsetDateTime, ttl_seconds: u64) {
        crate::session_lifecycle::write_lease_at(
            home,
            role,
            pid,
            now,
            Duration::from_secs(ttl_seconds),
            "0.17.0",
            Path::new("/opt/wire"),
            "codex-cli",
            Some(Path::new("/work/wire")),
        )
        .unwrap();
    }

    #[test]
    fn inventory_includes_only_live_mcp_sessions() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let live_home = tmp.path().join("live");
        let daemon_home = tmp.path().join("daemon-only");
        let expired_home = tmp.path().join("expired");
        let retired_home = tmp.path().join("retired");
        let dead_home = tmp.path().join("dead");

        lease(&live_home, "mcp", 101, now, 90);
        lease(&daemon_home, "daemon", 102, now, 90);
        lease(&expired_home, "mcp", 103, now, 1);
        lease(&retired_home, "mcp", 104, now, 90);
        lease(&dead_home, "mcp", 105, now, 90);
        std::fs::create_dir_all(retired_home.join("state/wire")).unwrap();
        std::fs::write(retired_home.join("state/wire/retired.json"), "{}").unwrap();

        let sessions = vec![
            session(&live_home, "11111111", true),
            session(&daemon_home, "22222222", true),
            session(&expired_home, "33333333", true),
            session(&retired_home, "44444444", true),
            session(&dead_home, "55555555", true),
        ];
        let report = collect_live_from(&sessions, now + time::Duration::seconds(2), |pid| {
            matches!(pid, 101..=104)
        })
        .unwrap();

        assert_eq!(report.schema, "wire-live-sessions-v1");
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].id, "session-11111111");
        assert_eq!(report.sessions[0].agent_host, "codex-cli");
        assert_eq!(
            report.sessions[0].project_dir.as_deref(),
            Some("/work/wire")
        );
        assert_eq!(
            report.sessions[0].started_at.as_deref(),
            Some("2023-11-14T22:13:20Z")
        );

        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("AGENT_SESSION_ID"));
        assert!(!json.contains("slot_token"));
        assert!(!json.contains("private.key"));
    }
}
