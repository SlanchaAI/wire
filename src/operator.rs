use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;
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

#[derive(Debug, Deserialize)]
pub struct LinkRequest {
    pub sessions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupRequest {
    pub name: String,
    pub creator: String,
    pub members: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MutationResult {
    pub ok: bool,
    pub message: String,
    pub changed_sessions: Vec<String>,
}

#[derive(Debug, Error)]
pub enum OperatorError {
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Conflict(String),
    #[error("operator action failed")]
    Internal(#[source] anyhow::Error),
    #[error("{message}")]
    Partial {
        message: String,
        changed_sessions: Vec<String>,
    },
}

fn validate_link_request<'a>(
    request: &LinkRequest,
    live: &'a [LiveSession],
) -> Result<[&'a LiveSession; 2], OperatorError> {
    if request.sessions.len() != 2 || request.sessions[0] == request.sessions[1] {
        return Err(OperatorError::Validation(
            "select exactly two distinct live sessions".to_string(),
        ));
    }
    let find = |id: &str| {
        live.iter()
            .find(|session| session.id == id)
            .ok_or_else(|| OperatorError::Conflict(format!("session `{id}` is no longer live")))
    };
    Ok([find(&request.sessions[0])?, find(&request.sessions[1])?])
}

fn validate_group_request<'a>(
    request: &GroupRequest,
    live: &'a [LiveSession],
) -> Result<Vec<&'a LiveSession>, OperatorError> {
    if request.name.trim().is_empty() {
        return Err(OperatorError::Validation(
            "group name cannot be empty".to_string(),
        ));
    }
    let distinct: HashSet<&str> = request.members.iter().map(String::as_str).collect();
    if distinct.len() < 2 || distinct.len() != request.members.len() {
        return Err(OperatorError::Validation(
            "select at least two distinct live sessions".to_string(),
        ));
    }
    if !distinct.contains(request.creator.as_str()) {
        return Err(OperatorError::Validation(
            "group creator must be selected".to_string(),
        ));
    }
    let mut selected = Vec::with_capacity(request.members.len());
    for id in &request.members {
        selected.push(
            live.iter()
                .find(|session| session.id == *id)
                .ok_or_else(|| {
                    OperatorError::Conflict(format!("session `{id}` is no longer live"))
                })?,
        );
    }
    Ok(selected)
}

fn run_wire_at(home: &Path, args: &[String]) -> Result<serde_json::Value, OperatorError> {
    const MAX_OUTPUT: usize = 256 * 1024;
    let binary = crate::platform::current_exe_resolved()
        .map_err(|error| OperatorError::Internal(error.into()))?;
    let output = Command::new(binary)
        .args(args)
        .env("WIRE_HOME", home)
        .env("WIRE_HOME_FORCE", "1")
        .env("WIRE_QUIET_AUTOSESSION", "1")
        .env_remove("WIRE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("AGENT")
        .env_remove("AGENT_SESSION_ID")
        .env_remove("COPILOT_AGENT_SESSION_ID")
        .env_remove("VSCODE_GIT_REPOSITORY_ROOT")
        .env_remove("WIRE_LOCAL_PAIR_ONE_WAY")
        .output()
        .map_err(|error| OperatorError::Internal(error.into()))?;
    let capped = |bytes: &[u8]| {
        let end = bytes.len().min(MAX_OUTPUT);
        String::from_utf8_lossy(&bytes[..end])
            .chars()
            .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
            .collect::<String>()
    };
    if !output.status.success() {
        return Err(OperatorError::Internal(anyhow::anyhow!(
            "wire command failed with {}: {}",
            output.status,
            capped(&output.stderr).trim()
        )));
    }
    serde_json::from_str(capped(&output.stdout).trim()).map_err(|error| {
        OperatorError::Internal(anyhow::anyhow!(
            "wire command returned invalid JSON: {error}"
        ))
    })
}

fn session_info<'a>(
    sessions: &'a [crate::session::SessionInfo],
    id: &str,
) -> Result<&'a crate::session::SessionInfo, OperatorError> {
    sessions
        .iter()
        .find(|session| session.name == id)
        .ok_or_else(|| OperatorError::Conflict(format!("session `{id}` is no longer available")))
}

fn has_verified_peer(
    owner: &crate::session::SessionInfo,
    peer: &crate::session::SessionInfo,
) -> bool {
    crate::dash::read_peers(
        &owner.home_dir,
        owner.did.as_deref(),
        owner.handle.as_deref(),
    )
    .iter()
    .any(|row| {
        row.tier == "VERIFIED"
            && (peer.did.as_deref() == Some(row.did.as_str())
                || peer.handle.as_deref() == Some(row.handle.as_str()))
    })
}

fn bilateral_verified(
    first: &crate::session::SessionInfo,
    second: &crate::session::SessionInfo,
) -> bool {
    has_verified_peer(first, second) && has_verified_peer(second, first)
}

pub fn link_local_sessions(request: LinkRequest) -> Result<MutationResult, OperatorError> {
    let report = collect_live_sessions().map_err(OperatorError::Internal)?;
    let selected = validate_link_request(&request, &report.sessions)?;
    let ids = [selected[0].id.clone(), selected[1].id.clone()];
    let sessions = crate::session::list_sessions().map_err(OperatorError::Internal)?;
    let first = session_info(&sessions, &ids[0])?;
    let second = session_info(&sessions, &ids[1])?;

    if bilateral_verified(first, second) {
        return Ok(MutationResult {
            ok: true,
            message: format!("{} and {} are already linked", first.name, second.name),
            changed_sessions: Vec::new(),
        });
    }

    run_wire_at(
        &first.home_dir,
        &[
            "add".to_string(),
            second.name.clone(),
            "--local-sister".to_string(),
            "--json".to_string(),
        ],
    )?;
    if !bilateral_verified(first, second) {
        return Err(OperatorError::Internal(anyhow::anyhow!(
            "local link did not converge to bilateral VERIFIED"
        )));
    }
    Ok(MutationResult {
        ok: true,
        message: format!("linked {} and {}", first.name, second.name),
        changed_sessions: ids.into_iter().collect(),
    })
}

pub fn create_local_group(request: GroupRequest) -> Result<MutationResult, OperatorError> {
    let report = collect_live_sessions().map_err(OperatorError::Internal)?;
    let selected = validate_group_request(&request, &report.sessions)?;
    let ids: Vec<String> = selected.iter().map(|session| session.id.clone()).collect();
    let sessions = crate::session::list_sessions().map_err(OperatorError::Internal)?;
    let creator = session_info(&sessions, &request.creator)?;
    let created = run_wire_at(
        &creator.home_dir,
        &[
            "group".to_string(),
            "create".to_string(),
            request.name.trim().to_string(),
            "--json".to_string(),
        ],
    )?;
    let group_id = created
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            OperatorError::Internal(anyhow::anyhow!("group create response omitted id"))
        })?
        .to_string();
    let invite = run_wire_at(
        &creator.home_dir,
        &[
            "group".to_string(),
            "invite".to_string(),
            group_id.clone(),
            "--json".to_string(),
        ],
    )?;
    let code = invite
        .get("code")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            OperatorError::Internal(anyhow::anyhow!("group invite response omitted code"))
        })?
        .to_string();

    let mut changed = vec![creator.name.clone()];
    for id in &ids {
        if id == &creator.name {
            continue;
        }
        let member = session_info(&sessions, id)?;
        if let Err(error) = run_wire_at(
            &member.home_dir,
            &[
                "group".to_string(),
                "join".to_string(),
                code.clone(),
                "--json".to_string(),
            ],
        ) {
            return Err(OperatorError::Partial {
                message: format!(
                    "created group `{}` for {}; failed while joining {}: {error}",
                    request.name.trim(),
                    changed.join(", "),
                    member.name
                ),
                changed_sessions: changed,
            });
        }
        changed.push(member.name.clone());
    }

    for id in &ids {
        let member = session_info(&sessions, id)?;
        let path = member
            .home_dir
            .join("config/wire/groups")
            .join(format!("{group_id}.json"));
        let valid = std::fs::read(&path)
            .ok()
            .and_then(|body| serde_json::from_slice::<crate::group::Group>(&body).ok())
            .is_some_and(|group| group.id == group_id);
        if !valid {
            return Err(OperatorError::Partial {
                message: format!(
                    "group `{}` did not materialize for {}",
                    request.name.trim(),
                    member.name
                ),
                changed_sessions: changed,
            });
        }
    }

    Ok(MutationResult {
        ok: true,
        message: format!(
            "created shared group `{}` for {} sessions",
            request.name.trim(),
            ids.len()
        ),
        changed_sessions: changed,
    })
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

    fn live(id: &str) -> LiveSession {
        LiveSession {
            id: id.to_string(),
            handle: id.to_string(),
            did: format!("did:wire:{id}-11111111"),
            emoji: "🦎".to_string(),
            primary_hex: "#45e456".to_string(),
            agent_host: "codex-cli".to_string(),
            project_dir: None,
            started_at: None,
            age_seconds: None,
            direct_link_count: 0,
            health: "healthy".to_string(),
        }
    }

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

    #[test]
    fn link_validation_rejects_invalid_selection() {
        let live = vec![live("alice"), live("bob"), live("carol")];
        for sessions in [
            vec!["alice".to_string()],
            vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
            vec!["alice".to_string(), "alice".to_string()],
        ] {
            let error = validate_link_request(&LinkRequest { sessions }, &live).unwrap_err();
            assert!(matches!(error, OperatorError::Validation(_)));
        }

        let error = validate_link_request(
            &LinkRequest {
                sessions: vec!["alice".to_string(), "missing".to_string()],
            },
            &live,
        )
        .unwrap_err();
        assert!(matches!(error, OperatorError::Conflict(_)));
    }

    #[test]
    fn group_validation_rejects_invalid_selection() {
        let live = vec![live("alice"), live("bob"), live("carol")];
        let cases = [
            GroupRequest {
                name: " ".to_string(),
                creator: "alice".to_string(),
                members: vec!["alice".to_string(), "bob".to_string()],
            },
            GroupRequest {
                name: "crew".to_string(),
                creator: "alice".to_string(),
                members: vec!["alice".to_string()],
            },
            GroupRequest {
                name: "crew".to_string(),
                creator: "missing".to_string(),
                members: vec!["alice".to_string(), "bob".to_string()],
            },
            GroupRequest {
                name: "crew".to_string(),
                creator: "carol".to_string(),
                members: vec!["alice".to_string(), "bob".to_string()],
            },
        ];
        for request in cases {
            assert!(validate_group_request(&request, &live).is_err());
        }
    }
}
