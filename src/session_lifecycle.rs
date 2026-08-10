use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub const LEASE_SCHEMA: &str = "wire-session-lease-v1";
pub const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(90);
pub const LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LeaseRecord {
    pub schema: String,
    pub role: String,
    pub pid: u32,
    pub heartbeat_at: String,
    pub expires_at: String,
    pub wire_version: String,
    pub bin_path: String,
    pub session_source: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub machine: Option<crate::session_metadata::MachineDescriptor>,
    #[serde(default)]
    pub harness: Option<crate::session_metadata::HarnessDescriptor>,
    #[serde(default)]
    pub project: Option<crate::session_metadata::ProjectDescriptor>,
}

pub fn lease_dir(home: &Path) -> PathBuf {
    home.join("state").join("wire").join("leases")
}

fn format_time(ts: OffsetDateTime) -> Result<String> {
    ts.format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| anyhow!("formatting lease timestamp: {e}"))
}

fn parse_time(raw: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).ok()
}

fn persist_record(path: &Path, record: &LeaseRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating lease directory {parent:?}"))?;
    }
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(record)?)
        .with_context(|| format!("writing session lease {tmp:?}"))?;
    if cfg!(windows) && path.exists() {
        std::fs::remove_file(path).with_context(|| format!("replacing session lease {path:?}"))?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("committing session lease {path:?}"))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn write_lease_at(
    home: &Path,
    role: &str,
    pid: u32,
    now: OffsetDateTime,
    ttl: Duration,
    wire_version: &str,
    bin_path: &Path,
    session_source: &str,
    cwd: Option<&Path>,
) -> Result<PathBuf> {
    if role.is_empty()
        || !role
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
    {
        return Err(anyhow!("invalid lease role `{role}`"));
    }
    let path = lease_dir(home).join(format!("{role}-{pid}.json"));
    let expires = now + time::Duration::seconds(ttl.as_secs() as i64);
    let snapshot = crate::session_metadata::process_snapshot(&[pid]);
    let record = LeaseRecord {
        schema: LEASE_SCHEMA.to_string(),
        role: role.to_string(),
        pid,
        heartbeat_at: format_time(now)?,
        expires_at: format_time(expires)?,
        wire_version: wire_version.to_string(),
        bin_path: bin_path.to_string_lossy().into_owned(),
        session_source: session_source.to_string(),
        started_at: Some(format_time(now)?),
        cwd: cwd.map(|path| path.to_string_lossy().into_owned()),
        machine: Some(crate::session_metadata::machine_descriptor(wire_version)),
        harness: Some(crate::session_metadata::harness_from_snapshot(
            &snapshot,
            pid,
            session_source,
        )),
        project: Some(crate::session_metadata::project_from_snapshot(
            &snapshot, pid, cwd,
        )),
    };
    persist_record(&path, &record)?;
    Ok(path)
}

pub fn heartbeat_lease_at(path: &Path, now: OffsetDateTime, ttl: Duration) -> Result<()> {
    let body = std::fs::read(path).with_context(|| format!("reading session lease {path:?}"))?;
    let mut record: LeaseRecord =
        serde_json::from_slice(&body).with_context(|| format!("parsing session lease {path:?}"))?;
    let snapshot = crate::session_metadata::process_snapshot(&[record.pid]);
    if record.machine.is_none() {
        record.machine = Some(crate::session_metadata::machine_descriptor(
            &record.wire_version,
        ));
    }
    if record.harness.as_ref().is_none_or(|value| {
        value.confidence == crate::session_metadata::MetadataConfidence::Unknown
    }) {
        record.harness = Some(crate::session_metadata::harness_from_snapshot(
            &snapshot,
            record.pid,
            &record.session_source,
        ));
    }
    if record.project.as_ref().is_none_or(|value| {
        value.confidence == crate::session_metadata::MetadataConfidence::Unknown
    }) {
        record.project = Some(crate::session_metadata::project_from_snapshot(
            &snapshot,
            record.pid,
            record.cwd.as_deref().map(Path::new),
        ));
    }
    record.heartbeat_at = format_time(now)?;
    record.expires_at = format_time(now + time::Duration::seconds(ttl.as_secs() as i64))?;
    persist_record(path, &record)
}

fn read_lease(path: &Path) -> Option<LeaseRecord> {
    let body = std::fs::read(path).ok()?;
    let record: LeaseRecord = serde_json::from_slice(&body).ok()?;
    (record.schema == LEASE_SCHEMA).then_some(record)
}

pub fn active_leases_at(
    home: &Path,
    now: OffsetDateTime,
    is_alive: impl Fn(u32) -> bool,
) -> Vec<LeaseRecord> {
    let Ok(entries) = std::fs::read_dir(lease_dir(home)) else {
        return Vec::new();
    };
    let mut active: Vec<LeaseRecord> = entries
        .flatten()
        .filter_map(|entry| read_lease(&entry.path()))
        .filter(|record| {
            parse_time(&record.expires_at).is_some_and(|expires| expires > now)
                && is_alive(record.pid)
        })
        .collect();
    active.sort_by(|a, b| a.role.cmp(&b.role).then(a.pid.cmp(&b.pid)));
    active
}

pub fn prune_stale_leases_at(
    home: &Path,
    now: OffsetDateTime,
    is_alive: impl Fn(u32) -> bool,
) -> usize {
    let Ok(entries) = std::fs::read_dir(lease_dir(home)) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let stale = read_lease(&entry.path()).is_none_or(|record| {
                parse_time(&record.expires_at).is_none_or(|expires| expires <= now)
                    || !is_alive(record.pid)
            });
            stale && std::fs::remove_file(entry.path()).is_ok()
        })
        .count()
}

pub struct LeaseGuard {
    path: PathBuf,
    ttl: Duration,
}

impl LeaseGuard {
    pub fn acquire(role: &str) -> Result<Self> {
        let state_dir = crate::config::state_dir()?;
        let home = state_dir
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| anyhow!("state directory has no session-home parent"))?;
        let bin = crate::platform::current_exe_resolved()?;
        let cwd = std::env::current_dir().ok();
        Self::acquire_at(
            home,
            role,
            std::process::id(),
            OffsetDateTime::now_utc(),
            DEFAULT_LEASE_TTL,
            env!("CARGO_PKG_VERSION"),
            &bin,
            crate::session::session_source(),
            cwd.as_deref(),
        )
    }

    pub fn heartbeat(&self) -> Result<()> {
        self.heartbeat_at(OffsetDateTime::now_utc())
    }

    #[allow(clippy::too_many_arguments)]
    fn acquire_at(
        home: &Path,
        role: &str,
        pid: u32,
        now: OffsetDateTime,
        ttl: Duration,
        wire_version: &str,
        bin_path: &Path,
        session_source: &str,
        cwd: Option<&Path>,
    ) -> Result<Self> {
        let path = write_lease_at(
            home,
            role,
            pid,
            now,
            ttl,
            wire_version,
            bin_path,
            session_source,
            cwd,
        )?;
        Ok(Self { path, ttl })
    }

    fn heartbeat_at(&self, now: OffsetDateTime) -> Result<()> {
        heartbeat_lease_at(&self.path, now, self.ttl)
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;
    use time::OffsetDateTime;

    fn write_test_lease(
        home: &Path,
        pid: u32,
        now: OffsetDateTime,
        ttl: Duration,
    ) -> std::path::PathBuf {
        write_lease_at(
            home,
            "mcp",
            pid,
            now,
            ttl,
            "0.17.0",
            Path::new("/opt/wire"),
            "override",
            Some(Path::new("/work/wire")),
        )
        .unwrap()
    }

    #[test]
    fn active_lease_round_trips_without_raw_session_key() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let path = write_test_lease(tmp.path(), 42, now, Duration::from_secs(90));

        let body = std::fs::read_to_string(path).unwrap();
        assert!(!body.contains("WIRE_SESSION_ID"));
        assert!(!body.contains("codex-skill-router"));
        let leases = active_leases_at(tmp.path(), now, |pid| pid == 42);
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].role, "mcp");
        assert_eq!(leases[0].pid, 42);
        assert_eq!(leases[0].session_source, "override");
        assert_eq!(
            leases[0].started_at.as_deref(),
            Some("2023-11-14T22:13:20Z")
        );
        assert_eq!(leases[0].cwd.as_deref(), Some("/work/wire"));
    }

    #[test]
    fn lease_without_inventory_metadata_remains_readable() {
        let record: LeaseRecord = serde_json::from_str(
            r#"{
                "schema":"wire-session-lease-v1",
                "role":"mcp",
                "pid":42,
                "heartbeat_at":"2023-11-14T22:13:20Z",
                "expires_at":"2023-11-14T22:14:50Z",
                "wire_version":"0.17.0",
                "bin_path":"/opt/wire",
                "session_source":"override"
            }"#,
        )
        .unwrap();

        assert_eq!(record.started_at, None);
        assert_eq!(record.cwd, None);
        assert_eq!(record.machine, None);
        assert_eq!(record.harness, None);
        assert_eq!(record.project, None);
    }

    #[test]
    fn new_lease_round_trips_structured_metadata() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let path = write_test_lease(tmp.path(), std::process::id(), now, Duration::from_secs(90));

        let record: LeaseRecord = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(record.machine.is_some());
        assert!(record.harness.is_some());
        assert_eq!(
            record
                .project
                .as_ref()
                .and_then(|value| value.cwd.as_deref()),
            Some("/work/wire")
        );
    }

    #[test]
    fn expired_or_dead_owner_is_not_active() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        write_test_lease(tmp.path(), 42, now, Duration::from_secs(90));

        assert!(active_leases_at(tmp.path(), now, |_| false).is_empty());
        assert!(
            active_leases_at(tmp.path(), now + time::Duration::seconds(91), |_| true).is_empty()
        );
    }

    #[test]
    fn heartbeat_extends_persisted_expiry_across_restart_read() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let path = write_test_lease(tmp.path(), 42, now, Duration::from_secs(90));
        heartbeat_lease_at(
            &path,
            now + time::Duration::seconds(60),
            Duration::from_secs(90),
        )
        .unwrap();

        let restarted_at = now + time::Duration::seconds(120);
        let leases = active_leases_at(tmp.path(), restarted_at, |pid| pid == 42);
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].pid, 42);
    }

    #[test]
    fn heartbeat_preserves_known_metadata() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let path = write_test_lease(tmp.path(), 42, now, Duration::from_secs(90));
        let mut record: LeaseRecord =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        record.harness = Some(crate::session_metadata::HarnessDescriptor {
            kind: "goose".to_string(),
            label: "Goose".to_string(),
            mode: Some("mcp-host".to_string()),
            confidence: crate::session_metadata::MetadataConfidence::Explicit,
            evidence: "lease-source".to_string(),
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

        heartbeat_lease_at(
            &path,
            now + time::Duration::seconds(30),
            Duration::from_secs(90),
        )
        .unwrap();

        let refreshed: LeaseRecord = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(refreshed.harness, record.harness);
        assert_eq!(refreshed.started_at, record.started_at);
    }

    #[test]
    fn pruning_removes_expired_dead_and_malformed_records_only() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let live = write_test_lease(tmp.path(), 1, now, Duration::from_secs(90));
        let dead = write_test_lease(tmp.path(), 2, now, Duration::from_secs(90));
        let malformed = lease_dir(tmp.path()).join("bad.json");
        std::fs::write(&malformed, "not-json").unwrap();

        let removed = prune_stale_leases_at(tmp.path(), now, |pid| pid == 1);
        assert_eq!(removed, 2);
        assert!(live.exists());
        assert!(!dead.exists());
        assert!(!malformed.exists());
    }

    #[test]
    fn guard_heartbeats_and_removes_lease_on_clean_drop() {
        let tmp = tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let guard = LeaseGuard::acquire_at(
            tmp.path(),
            "mcp",
            42,
            now,
            Duration::from_secs(90),
            "0.17.0",
            Path::new("/opt/wire"),
            "codex-cli",
            Some(Path::new("/work/wire")),
        )
        .unwrap();
        let path = guard.path.clone();
        guard
            .heartbeat_at(now + time::Duration::seconds(30))
            .unwrap();
        assert_eq!(active_leases_at(tmp.path(), now, |_| true).len(), 1);
        drop(guard);
        assert!(!path.exists());
    }
}
