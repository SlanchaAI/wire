//! `wire daemon --all-sessions` — multi-session supervisor.
//!
//! ## Why
//!
//! honey-pine's 2026-06-01 dogfood (#162) surfaced a launchd-vs-session
//! isolation gap: the `sh.slancha.wire.daemon` launchd unit invokes
//! `wire daemon --interval 5` with **no cwd context**. With WIRE_HOME
//! unset, the daemon resolves to the *default* session WIRE_HOME and
//! silently skips every other initialized session. Operators with
//! multiple per-project sessions (slancha-mesh, wire, etc.) saw their
//! shell `wire status` report `running:false` even with the launchd
//! daemon perfectly alive — same daemon, different state tree.
//!
//! Her working remedy was `launchctl bootout` + `nohup wire daemon`
//! from the project cwd. That works for one session but doesn't scale
//! to N. The architectural fix is a supervisor that owns the
//! multi-session orchestration: one supervisor process per launchd
//! unit, a bounded set of child `wire daemon` processes — each with
//! its own pinned `WIRE_HOME` and its own pidfile under that session's
//! state dir. `wire status` from any cwd then sees its session's child
//! pid and reports truthfully.
//!
//! ## Model
//!
//! - **Fork-exec, not threads.** Each session's daemon needs its own
//!   `WIRE_HOME`. We set it via the child process env so the daemon
//!   code path stays unchanged. Threads would mean global mutable
//!   `WIRE_HOME` and cross-session races.
//! - **Idempotent spawn.** Before spawning a child for session S,
//!   check `daemon_singleton_holder()` on that session's home. If a
//!   live daemon already exists (operator ran `wire daemon` directly
//!   in S's cwd, or supervisor restarted and the old child is still
//!   alive), leave it alone.
//! - **Reap via polling, not SIGCHLD.** macOS launchd-supervised
//!   processes already get SIGCHLD overhead; `try_wait` polling on a
//!   short interval is simpler and bug-free across platforms.
//! - **Backoff on rapid failure.** A child that exits within 10s of
//!   spawn doubles its respawn delay (1s → 60s cap). Prevents a broken
//!   session (corrupt key, missing relay) from fork-bombing.
//! - **Don't exit on zero sessions.** Sleep and re-poll the registry —
//!   new sessions get picked up without supervisor restart.
//! - **Lease-backed lifecycle.** A registry binding, live process lease,
//!   or pending outbox makes a session eligible. A private key and sync
//!   timestamps do not. Retired and inactive homes stay inactive across
//!   supervisor restarts.
//! - **Hard worker cap.** Excess eligible sessions remain in an
//!   observable queue instead of expanding process count without bound.
//!
//! ## Invariants
//!
//! - One supervisor per launchd unit per machine. Singleton guard on
//!   `sessions_root()/supervisor.pid` (separate from per-session
//!   daemon pidfiles).
//! - Child env contains exactly one wire-relevant variable:
//!   `WIRE_HOME=<session-home>`. Any other inherited WIRE_* vars are
//!   stripped so the operator's shell config doesn't leak in.
//! - Per-session daemon code is *unchanged* — supervisor is a pure
//!   orchestrator.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use serde_json::json;

/// How often the supervisor re-reads the session registry. Tradeoff: a
/// new session created at `wire session new` waits up to this many
/// seconds before its daemon comes up. 10s strikes a balance — fast
/// enough that operators don't notice, slow enough that registry
/// fork-execs don't dominate.
const REGISTRY_POLL_SECS: u64 = 10;

/// Initial respawn delay after a child exits unexpectedly. Doubles on
/// each rapid failure (exit within `RAPID_FAIL_WINDOW`) up to
/// `MAX_BACKOFF`.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const RAPID_FAIL_WINDOW: Duration = Duration::from_secs(10);
pub const DEFAULT_MAX_WORKERS: usize = 16;

fn next_worker_backoff(previous: Option<Duration>, rapid_failure: bool) -> Duration {
    if rapid_failure {
        (previous.unwrap_or(INITIAL_BACKOFF) * 2).min(MAX_BACKOFF)
    } else {
        INITIAL_BACKOFF
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerVersion {
    Current,
    Skewed,
}

fn classify_session_worker(
    record: &crate::ensure_up::DaemonPid,
    alive: bool,
    cmdline: Option<&str>,
) -> Option<WorkerVersion> {
    if !alive
        || record.schema != crate::ensure_up::DAEMON_PID_SCHEMA
        || record.pid == std::process::id()
    {
        return None;
    }
    let cmdline = cmdline?;
    let args: Vec<&str> = cmdline.split_whitespace().collect();
    if !args.contains(&"daemon") || args.contains(&"--all-sessions") {
        return None;
    }
    Some(if record.version == env!("CARGO_PKG_VERSION") {
        WorkerVersion::Current
    } else {
        WorkerVersion::Skewed
    })
}

/// Stop one pidfile-owned worker for a session that lifecycle planning did
/// not select. Validation is deliberately narrow: JSON pidfile, live PID,
/// daemon command line, and never the all-session supervisor. No process
/// family signal and no home mutation.
fn retire_inactive_worker(session: &crate::session::SessionInfo) {
    let pidfile = session
        .home_dir
        .join("state")
        .join("wire")
        .join("daemon.pid");
    let Ok(body) = std::fs::read_to_string(pidfile) else {
        return;
    };
    let Ok(record) = serde_json::from_str::<crate::ensure_up::DaemonPid>(&body) else {
        return;
    };
    let alive = crate::platform::process_alive(record.pid);
    let cmdline = crate::platform::pid_cmdline(record.pid);
    let Some(version) = classify_session_worker(&record, alive, cmdline.as_deref()) else {
        return;
    };
    eprintln!(
        "supervisor: retiring unselected session worker '{}' pid={} version={version:?}",
        session.name, record.pid
    );
    if !crate::platform::kill_process(record.pid, false) {
        eprintln!(
            "supervisor: failed to signal unselected session worker '{}' pid={}",
            session.name, record.pid
        );
    }
}

/// Newest mtime among a session home's activity files — the
/// supervisor's "last actually *synced*" signal. These live under the
/// session's `state/wire/` subtree (same root the per-session daemon
/// and `existing_daemon_for_session` use), NOT the home root.
/// `last_sync.json` is rewritten on every successful daemon relay
/// cycle; the cursors move on inbox/reactor activity. Returns `None`
/// for a home that has never synced (a husk).
///
/// Deliberately excludes `daemon.pid`: it's written on *spawn*, so
/// counting it would make eligibility self-perpetuating — the
/// supervisor spawns a daemon, the pidfile refreshes, and the session
/// would never age out even if it never actually syncs anything.
fn fs_last_active(home: &Path) -> Option<SystemTime> {
    let state = home.join("state").join("wire");
    ["last_sync.json", "notify.cursor", "reactor.cursor"]
        .iter()
        .filter_map(|f| std::fs::metadata(state.join(f)).ok())
        .filter_map(|m| m.modified().ok())
        .max()
}

/// True iff the session home has been retired (`state/wire/retired.json`).
/// A retired home is ineligible for a daemon regardless of cwd/identity/idle —
/// the supervisor kills any running child and never respawns. Pure existence
/// check (see [`crate::retire::is_retired`]).
fn fs_is_retired(home: &Path) -> bool {
    crate::retire::is_retired(home)
}

fn fs_has_live_lease(home: &Path) -> bool {
    !crate::session_lifecycle::active_leases_at(
        home,
        time::OffsetDateTime::now_utc(),
        crate::platform::process_alive,
    )
    .is_empty()
}

fn fs_has_pending_outbox(home: &Path) -> bool {
    let outbox = home.join("state").join("wire").join("outbox");
    let Ok(entries) = std::fs::read_dir(outbox) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.extension().and_then(|s| s.to_str()) == Some("jsonl")
            && !path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| name.ends_with(".pushed.jsonl"))
            && std::fs::read_to_string(path).is_ok_and(|body| !body.trim().is_empty())
    })
}

#[derive(Debug, Clone)]
struct SupervisorPlan {
    selected: Vec<crate::session::SessionInfo>,
    queued: Vec<String>,
    inactive: Vec<String>,
    retired: Vec<String>,
}

#[cfg(test)]
impl SupervisorPlan {
    fn selected_names(&self) -> Vec<&str> {
        self.selected
            .iter()
            .map(|session| session.name.as_str())
            .collect()
    }
}

fn plan_supervisor_sessions<F, G, H>(
    sessions: Vec<crate::session::SessionInfo>,
    max_workers: usize,
    has_live_lease: F,
    has_pending_outbox: G,
    is_retired: H,
) -> SupervisorPlan
where
    F: Fn(&Path) -> bool,
    G: Fn(&Path) -> bool,
    H: Fn(&Path) -> bool,
{
    let mut eligible = Vec::new();
    let mut inactive = Vec::new();
    let mut retired = Vec::new();
    for session in sessions {
        if is_retired(&session.home_dir) {
            retired.push(session.name);
            continue;
        }
        if session.did.is_none() {
            inactive.push(session.name);
            continue;
        }
        if session.cwd.is_some()
            || has_live_lease(&session.home_dir)
            || has_pending_outbox(&session.home_dir)
        {
            eligible.push(session);
        } else {
            inactive.push(session.name);
        }
    }
    eligible.sort_by(|a, b| {
        b.cwd
            .is_some()
            .cmp(&a.cwd.is_some())
            .then_with(|| a.name.cmp(&b.name))
    });
    let split = eligible.len().min(max_workers);
    let queued = eligible[split..]
        .iter()
        .map(|session| session.name.clone())
        .collect();
    let selected = eligible.into_iter().take(split).collect();
    SupervisorPlan {
        selected,
        queued,
        inactive,
        retired,
    }
}

// ---- husk reaper (the 175-dir by-key accumulation fix) ----

/// Default age below which a husk is left alone, in hours. Generous on
/// purpose: a brand-new agent session may mint its by-key home minutes
/// before it first inits/sends. Two days is far past any plausible
/// "about to become real" window while still draining the backlog
/// (honey-pine regrew 9 husks in one minute; 175 over two weeks).
const DEFAULT_HUSK_REAP_MAX_AGE_HOURS: u64 = 48;

/// How often the supervisor sweeps for husks. The reap is cheap (one
/// readdir + a few stats per entry) but there's no reason to run it on
/// every 10s registry poll — husks age in days, not seconds.
const HUSK_REAP_INTERVAL: Duration = Duration::from_secs(3600);

/// Parse the husk reap cutoff. `None` raw → default; a `0` value →
/// `None` (reaper disabled); any other integer → that many hours;
/// unparseable → default.
fn parse_husk_reap_max_age(raw: Option<&str>) -> Option<Duration> {
    match raw {
        Some(v) => {
            let hours: u64 = v.trim().parse().unwrap_or(DEFAULT_HUSK_REAP_MAX_AGE_HOURS);
            (hours != 0).then(|| Duration::from_secs(hours * 3600))
        }
        None => Some(Duration::from_secs(DEFAULT_HUSK_REAP_MAX_AGE_HOURS * 3600)),
    }
}

/// Read the husk reap cutoff from the environment.
/// `WIRE_HUSK_REAP_MAX_AGE_HOURS=0` disables the reaper entirely.
fn husk_reap_max_age_from_env() -> Option<Duration> {
    parse_husk_reap_max_age(
        std::env::var("WIRE_HUSK_REAP_MAX_AGE_HOURS")
            .ok()
            .as_deref(),
    )
}

/// Delete husk session homes under `by_key_root` and return what was
/// removed.
///
/// Every wire invocation inside an agent terminal mints a
/// `sessions/by-key/<hash>/` home via session adoption (RFC-008), even
/// for read-only commands, and nothing ever deleted them — a dev box
/// accumulated 175 empty dirs in two weeks. Lifecycle planning prevents
/// those homes from starting workers; this reaper removes old identityless
/// husks without touching initialized identities.
///
/// A dir is reaped only if ALL of these hold:
/// - its name has the by-key shape (exactly 16 lowercase hex chars,
///   `session_home_for_key`'s output) — named sessions are
///   operator-created and never touched;
/// - it holds NO identity (`config/wire/private.key` absent);
/// - it has never synced (`fs_last_active` → None);
/// - it is not registry-bound (`bound_names`);
/// - no live daemon owns it (`daemon_live`, injected for testability);
/// - it is older than `max_age` (top-dir mtime; future mtimes count as
///   young — clock-skew never deletes).
///
/// Failures are per-entry best-effort (warn + continue): one undeletable
/// dir must not stop the sweep.
fn reap_husks<F>(
    by_key_root: &Path,
    max_age: Duration,
    now: SystemTime,
    bound_names: &std::collections::HashSet<String>,
    daemon_live: F,
) -> Vec<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut reaped = Vec::new();
    let Ok(entries) = std::fs::read_dir(by_key_root) else {
        return reaped; // no by-key dir yet — nothing to do
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let is_by_key_shape =
            name.len() == 16 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !is_by_key_shape {
            continue;
        }
        if bound_names.contains(name) {
            continue;
        }
        if path
            .join("config")
            .join("wire")
            .join("private.key")
            .exists()
        {
            continue;
        }
        if fs_last_active(&path).is_some() {
            continue;
        }
        if daemon_live(&path) {
            continue;
        }
        let old_enough = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= max_age);
        if !old_enough {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => reaped.push(path),
            Err(e) => eprintln!("supervisor: husk reap failed for {}: {e:#}", path.display()),
        }
    }
    reaped
}

/// State the supervisor tracks per session it has spawned a child for.
struct ChildState {
    child: Child,
    spawned_at: Instant,
}

/// Entrypoint for `wire daemon --all-sessions`. Loops forever; only
/// returns Err on a setup error (e.g. cannot resolve sessions_root).
pub fn run_supervisor(interval_secs: u64, max_workers: usize, as_json: bool) -> Result<()> {
    // Supervisor singleton — one per machine. Separate pidfile from the
    // per-session daemon pidfile so the two layers can't collide.
    let pid_path = supervisor_pid_path()?;
    if let Some(existing) = read_alive_supervisor_pid(&pid_path)? {
        let msg = json!({
            "status": "skipped",
            "reason": "supervisor already running",
            "holder_pid": existing,
        });
        if as_json {
            println!("{msg}");
        } else {
            eprintln!(
                "wire daemon --all-sessions: another supervisor is already running (pid {existing}); not starting a second one."
            );
        }
        return Ok(());
    }
    write_supervisor_pid(&pid_path)?;
    let _cleanup = SupervisorPidGuard {
        path: pid_path.clone(),
    };

    if !as_json {
        eprintln!(
            "wire daemon --all-sessions: supervisor up. interval={interval_secs}s, registry-poll={REGISTRY_POLL_SECS}s, max-workers={max_workers}. SIGINT to stop."
        );
    } else {
        println!(
            "{}",
            json!({
                "status": "supervisor_started",
                "interval_secs": interval_secs,
                "registry_poll_secs": REGISTRY_POLL_SECS,
                "max_workers": max_workers,
            })
        );
    }

    // Husk reap cutoff — also read once at startup.
    let husk_max_age = husk_reap_max_age_from_env();
    eprintln!(
        "supervisor: husk reap cutoff = {}",
        match husk_max_age {
            Some(d) => format!("{} hours", d.as_secs() / 3600),
            None => "disabled".to_string(),
        }
    );
    let mut last_husk_reap: Option<Instant> = None;

    let mut children: HashMap<String, ChildState> = HashMap::new();
    // Per-session backoff that survives a child's reap → respawn → reap
    // cycle. Distinguishes "session crashes hard repeatedly" from
    // "child exited cleanly and we're spawning a fresh one".
    let mut session_last_exit: HashMap<String, Instant> = HashMap::new();
    let mut session_backoff: HashMap<String, Duration> = HashMap::new();

    loop {
        // 1. Reap any exited children. Detect rapid failures + update
        //    per-session backoff so the next spawn waits.
        let mut exited: Vec<String> = Vec::new();
        for (name, state) in children.iter_mut() {
            if let Ok(Some(status)) = state.child.try_wait() {
                let lived = state.spawned_at.elapsed();
                let rapid = lived < RAPID_FAIL_WINDOW;
                eprintln!(
                    "supervisor: child '{name}' exited (status={status:?}, lived={}s, rapid={rapid})",
                    lived.as_secs()
                );
                let next_backoff = next_worker_backoff(session_backoff.get(name).copied(), rapid);
                session_backoff.insert(name.clone(), next_backoff);
                session_last_exit.insert(name.clone(), Instant::now());
                exited.push(name.clone());
            }
        }
        for n in exited {
            children.remove(&n);
        }

        // 2. Read registry and select only explicitly live sessions.
        //    Private keys and sync timestamps are historical state, not
        //    liveness signals.
        let all_sessions = crate::session::list_sessions().unwrap_or_default();
        let total_sessions = all_sessions.len();
        for session in &all_sessions {
            crate::session_lifecycle::prune_stale_leases_at(
                &session.home_dir,
                time::OffsetDateTime::now_utc(),
                crate::platform::process_alive,
            );
        }
        let plan = plan_supervisor_sessions(
            all_sessions.clone(),
            max_workers,
            fs_has_live_lease,
            fs_has_pending_outbox,
            fs_is_retired,
        );
        let wanted = plan.selected;
        if wanted.len() != total_sessions || !plan.queued.is_empty() {
            eprintln!(
                "supervisor: {} running target(s), {} queued, {} inactive, {} retired from {} discovered (cap {})",
                wanted.len(),
                plan.queued.len(),
                plan.inactive.len(),
                plan.retired.len(),
                total_sessions,
                max_workers,
            );
        }

        // 2b. Hourly husk sweep: delete by-key homes that were minted
        //     by session adoption but never grew an identity or synced.
        //     Runs on the first loop iteration, then once per
        //     HUSK_REAP_INTERVAL.
        if let Some(max_age) = husk_max_age
            && last_husk_reap.is_none_or(|t| t.elapsed() >= HUSK_REAP_INTERVAL)
        {
            last_husk_reap = Some(Instant::now());
            let bound: std::collections::HashSet<String> = crate::session::read_registry()
                .unwrap_or_default()
                .by_cwd
                .values()
                .cloned()
                .collect();
            if let Ok(root) = crate::session::sessions_root() {
                let reaped = reap_husks(
                    &root.join("by-key"),
                    max_age,
                    SystemTime::now(),
                    &bound,
                    // On a liveness-probe error assume live — never
                    // delete a home we couldn't safely inspect.
                    |home| existing_daemon_for_session(home).unwrap_or(true),
                );
                if !reaped.is_empty() {
                    eprintln!(
                        "supervisor: reaped {} husk session home(s): {}",
                        reaped.len(),
                        reaped
                            .iter()
                            .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
        }

        // 3. Kill children whose session has been removed from the
        //    registry since last poll. (Operator ran `wire session
        //    forget` or similar.)
        let wanted_names: std::collections::HashSet<String> =
            wanted.iter().map(|s| s.name.clone()).collect();
        let to_kill: Vec<String> = children
            .keys()
            .filter(|n| !wanted_names.contains(n.as_str()))
            .cloned()
            .collect();
        for name in to_kill {
            if let Some(mut state) = children.remove(&name) {
                eprintln!("supervisor: session '{name}' gone from registry; terminating its child");
                let _ = state.child.kill();
                let _ = state.child.wait();
            }
        }
        for session in &all_sessions {
            if !wanted_names.contains(&session.name) {
                retire_inactive_worker(session);
            }
        }

        // 4. Spawn missing children, respecting backoff + existing
        //    pidfiles (operator-spawned daemons coexist).
        for info in wanted {
            if info.did.is_none() {
                continue;
            }
            if children.contains_key(&info.name) {
                continue;
            }
            // Backoff gate: if this session is in a rapid-fail loop,
            // wait the remaining backoff before respawning.
            if let Some(last_exit) = session_last_exit.get(&info.name) {
                let wait = session_backoff
                    .get(&info.name)
                    .copied()
                    .unwrap_or(INITIAL_BACKOFF);
                if last_exit.elapsed() < wait {
                    continue;
                }
            }
            // Singleton check: an operator-spawned `wire daemon` may
            // already own this session. Leave it alone — re-checking
            // next poll is cheap.
            if existing_daemon_for_session(&info.home_dir)? {
                continue;
            }
            match spawn_child_for_session(&info.name, &info.home_dir, interval_secs) {
                Ok(child) => {
                    eprintln!(
                        "supervisor: spawned child for session '{}' (pid {})",
                        info.name,
                        child.id()
                    );
                    children.insert(
                        info.name.clone(),
                        ChildState {
                            child,
                            spawned_at: Instant::now(),
                        },
                    );
                }
                Err(e) => {
                    eprintln!(
                        "supervisor: spawn failed for session '{}': {e:#}",
                        info.name
                    );
                    // Treat spawn failure as a rapid failure so the
                    // backoff curve kicks in.
                    let next = next_worker_backoff(session_backoff.get(&info.name).copied(), true);
                    session_backoff.insert(info.name.clone(), next);
                    session_last_exit.insert(info.name.clone(), Instant::now());
                }
            }
        }

        std::thread::sleep(Duration::from_secs(REGISTRY_POLL_SECS));
    }
}

/// Spawn `wire daemon --interval <i>` as a child with `WIRE_HOME`
/// pinned via env. Strips inherited WIRE_* env so the operator's
/// shell config (test overrides like `WIRE_DAEMON_NO_SINGLETON=1`)
/// can't leak in.
///
/// v0.14.2 #170 hotfix: the original implementation also passed
/// `--session <character-name>` as a belt-and-suspenders check.
/// That broke 127 of 133 sessions on a real multi-session box —
/// `cmd_daemon`'s `--session` handler calls
/// `session::session_dir(name)` which resolves
/// `sessions_root/<name>`, correct for v0.6 top-level layout but
/// WRONG for v0.13's `by-key/<hash>` layout where the character
/// name is *derived* from the card DID, not the directory name.
/// Children bailed → supervisor fork-bombed (10s poll × 60s
/// backoff × 127 failing sessions). WIRE_HOME env alone is the
/// correct contract: every daemon code path flows through
/// `state_dir()` / `config_dir()` which honor it. No second
/// source of truth.
fn spawn_child_for_session(
    name: &str,
    home_dir: &std::path::Path,
    interval_secs: u64,
) -> Result<Child> {
    let exe = std::env::current_exe().context("resolving current exe for child fork")?;
    let mut cmd = Command::new(&exe);
    cmd.args(["daemon", "--interval", &interval_secs.to_string()]);
    // Strip WIRE_* env so operator shell-vars don't leak into the
    // child. Then pin WIRE_HOME exactly.
    let leaks: Vec<String> = std::env::vars()
        .filter(|(k, _)| k.starts_with("WIRE_"))
        .map(|(k, _)| k)
        .collect();
    for k in leaks {
        cmd.env_remove(&k);
    }
    cmd.env("WIRE_HOME", home_dir);
    // Children inherit stdout/stderr → land in the launchd log file
    // (StandardOutPath in the plist). Operators see "supervisor:
    // spawned ..." lines interleaved with each session's daemon log.
    cmd.spawn().with_context(|| {
        format!(
            "fork-exec `wire daemon` for session '{name}' (binary {} WIRE_HOME={})",
            exe.display(),
            home_dir.display()
        )
    })
}

/// True iff this session's `daemon.pid` names a live process. Used by
/// the supervisor to coexist with operator-spawned `wire daemon`
/// invocations: if the operator already started one in a tmux pane,
/// we skip the spawn and let theirs own the cursor.
fn existing_daemon_for_session(home_dir: &std::path::Path) -> Result<bool> {
    let pid_path = home_dir.join("state").join("wire").join("daemon.pid");
    if !pid_path.exists() {
        return Ok(false);
    }
    let body = match std::fs::read_to_string(&pid_path) {
        Ok(b) => b,
        Err(_) => return Ok(false),
    };
    // Pidfile is either JSON `{"pid": <n>, ...}` (v0.5.11+) or a bare
    // integer (legacy). Try JSON+pid-field first; if that yields
    // None (parse failed OR JSON had no pid field, e.g. a bare
    // integer body parses as JSON number with no `.pid`), fall
    // through to the bare-int path.
    let pid = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("pid").and_then(serde_json::Value::as_u64))
        .or_else(|| body.trim().parse::<u64>().ok());
    Ok(pid
        .map(|p| crate::ensure_up::pid_is_alive(p as u32))
        .unwrap_or(false))
}

/// Read-only snapshot of the supervisor's current topology — supervisor
/// liveness + per-session daemon liveness + orphan pids the supervisor
/// is not currently managing. Used by `wire supervisor` (the CLI
/// counterpart to single-session `wire status`) so operators can ask
/// "what is the multi-session supervisor doing?" in one command
/// instead of cross-referencing `pgrep` against per-session pidfiles
/// by hand.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SupervisorState {
    /// Pid the `supervisor.pid` file names; None if file missing.
    pub supervisor_pid: Option<u32>,
    /// True iff that pid is currently a live process.
    pub supervisor_alive: bool,
    /// Per-session liveness across every initialized session, in
    /// `list_sessions()` order.
    pub sessions: Vec<SupervisedSession>,
    /// `wire daemon` pids found via cmdline-scan that are NOT mapped
    /// to any session's pidfile AND are not the supervisor itself.
    /// Could be legacy operator-spawned daemons, leftover children
    /// from a crashed prior supervisor, or daemons serving the
    /// default WIRE_HOME (no `--all-sessions`). Operators see them
    /// here so they can decide whether to kill.
    pub unmanaged_pids: Vec<u32>,
    /// v0.14.2: session names whose live daemon's recorded
    /// `pidfile.version` is older than this CLI's own
    /// `CARGO_PKG_VERSION`. The supervisor's existing-pidfile check
    /// skips alive daemons regardless of their binary version, so
    /// stale-binary daemons persist until they exit. Surfaced for
    /// operator visibility — they can `pkill -TERM <pid>` or use a
    /// future `wire upgrade --refresh-stale-children` to force the
    /// supervisor to respawn them on the current binary.
    pub stale_binary_sessions: Vec<String>,
    /// Subset of `stale_binary_sessions` that lacks a registry binding,
    /// live lease, or pending outbox. The supervisor will not respawn
    /// these inactive sessions, so upgrade must not kill them under the
    /// assumption that a replacement worker will appear.
    pub stale_unmanaged_sessions: Vec<String>,
}

/// One session as seen by the supervisor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SupervisedSession {
    /// Session name (`info.name` from `session::list_sessions`).
    pub name: String,
    /// `home_dir` filesystem path.
    pub home_dir: String,
    /// Pid the session's `daemon.pid` records; None if file missing.
    pub daemon_pid: Option<u32>,
    /// True iff that pid is currently a live process.
    pub daemon_alive: bool,
    /// Seconds since the session's daemon last completed a sync
    /// cycle (read from `last_sync.json`); None if never recorded.
    pub last_sync_age_seconds: Option<u64>,
    /// Version string the running daemon recorded when it wrote its
    /// pidfile (`PidRecord::Json.version`). None when the pidfile is
    /// missing or corrupt. Surfaced so operators can spot version drift across
    /// the supervisor fleet — the supervisor's pre-spawn
    /// existing-pidfile check skips alive daemons regardless of
    /// their binary version, so a daemon spawned on v0.13.x and
    /// still running after the supervisor was bounced to v0.14.x
    /// keeps the old binary in memory until it exits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_version: Option<String>,
}

/// Build a `SupervisorState` snapshot. Pure read; no fork / no
/// pidfile mutation. Best-effort on every component (filesystem
/// errors yield None / empty rather than failing the whole call).
pub fn read_supervisor_state() -> Result<SupervisorState> {
    let pid_path = supervisor_pid_path()?;
    let supervisor_pid = read_supervisor_pid(&pid_path);
    let supervisor_alive = supervisor_pid
        .map(crate::ensure_up::pid_is_alive)
        .unwrap_or(false);

    // Per-session liveness — walk list_sessions, read each home's
    // pidfile + last_sync.
    let infos = crate::session::list_sessions().unwrap_or_default();

    // Use the same lifecycle predicate as the supervisor. No cap here: this
    // set answers whether a worker is respawnable eventually, not whether it
    // occupies a slot in this poll.
    let eligible_names: std::collections::HashSet<String> = plan_supervisor_sessions(
        infos.clone(),
        usize::MAX,
        fs_has_live_lease,
        fs_has_pending_outbox,
        fs_is_retired,
    )
    .selected
    .into_iter()
    .map(|s| s.name)
    .collect();

    let sessions: Vec<SupervisedSession> = infos
        .into_iter()
        .map(|info| {
            let daemon_pid = crate::session::session_daemon_pid(&info.home_dir);
            let daemon_alive = daemon_pid
                .map(crate::ensure_up::pid_is_alive)
                .unwrap_or(false);
            // last_sync.json lives under <home>/state/wire/last_sync.json.
            let last_sync_age_seconds = read_session_last_sync_age(&info.home_dir);
            // v0.14.2: read the daemon-recorded version from the JSON
            // pidfile. Legacy bare-integer pidfiles return None
            // (can't surface a version we don't have).
            let daemon_version = read_session_pidfile_version(&info.home_dir);
            SupervisedSession {
                name: info.name,
                home_dir: info.home_dir.to_string_lossy().into_owned(),
                daemon_pid,
                daemon_alive,
                last_sync_age_seconds,
                daemon_version,
            }
        })
        .collect();

    // Unmanaged pids: every `wire daemon` cmdline scan hit that isn't
    // (a) the supervisor itself, (b) any session's pidfile pid.
    let all_daemon_pids: std::collections::HashSet<u32> =
        crate::platform::find_processes_by_cmdline("wire daemon")
            .into_iter()
            .collect();
    let known_session_pids: std::collections::HashSet<u32> = sessions
        .iter()
        .filter_map(|s| if s.daemon_alive { s.daemon_pid } else { None })
        .collect();
    let mut unmanaged_pids: Vec<u32> = all_daemon_pids
        .into_iter()
        .filter(|p| Some(*p) != supervisor_pid && !known_session_pids.contains(p))
        .collect();
    unmanaged_pids.sort_unstable();

    // v0.14.2: derive the stale-binary set. Compare each live
    // daemon's recorded version against the running CLI's version.
    // "Stale" iff alive + has a recorded version + that version is
    // strictly less than ours by dotted-integer compare (so 0.10.0 >
    // 0.9.0). Unparseable strings are conservatively "not stale" — a
    // pre-release suffix like 0.14.2-rc.1 stays unflagged rather than
    // false-positive against 0.14.2.
    let our_version = env!("CARGO_PKG_VERSION");
    let stale_binary_sessions: Vec<String> = sessions
        .iter()
        .filter(|s| {
            s.daemon_alive
                && s.daemon_version
                    .as_deref()
                    .map(|v| version_lt(v, our_version))
                    .unwrap_or(false)
        })
        .map(|s| s.name.clone())
        .collect();

    // #275: split the stale set by whether the supervisor would respawn it.
    // The "unmanaged" ones must not be killed by `--refresh-stale-children`.
    let (_respawnable, stale_unmanaged_sessions) =
        partition_stale_by_eligibility(&stale_binary_sessions, &eligible_names);

    Ok(SupervisorState {
        supervisor_pid,
        supervisor_alive,
        sessions,
        unmanaged_pids,
        stale_binary_sessions,
        stale_unmanaged_sessions,
    })
}

/// Split stale-binary session names into `(respawnable, unmanaged)`: a stale
/// session is respawnable iff the `--all-sessions` supervisor would re-own it
/// (its name is in `eligible`). The `unmanaged` ones are stale daemons the
/// supervisor's eligibility filter drops (unbound + idle past the cutoff, or
/// never-synced) — killing one orphans it because nothing respawns it. Pure +
/// unit-tested so `wire upgrade --refresh-stale-children`'s "don't kill what
/// you can't respawn" contract (#275) is locked. Order-preserving.
fn partition_stale_by_eligibility(
    stale: &[String],
    eligible: &std::collections::HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    stale
        .iter()
        .cloned()
        .partition(|name| eligible.contains(name))
}

/// Compare two dotted-integer version strings: `a < b`?
///
/// Splits on `.`, parses each segment as `u32`, compares
/// element-wise (left-pad shorter with 0 so `0.14` < `0.14.1` is
/// `true`). Anything that fails to parse as `u32` makes the whole
/// compare return `false` — we'd rather under-flag a pre-release
/// suffix like `0.14.2-rc.1` than false-positive against a stable
/// peer of the same major.minor.patch.
fn version_lt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u32>> { s.split('.').map(|p| p.parse().ok()).collect() };
    let (Some(av), Some(bv)) = (parse(a), parse(b)) else {
        return false;
    };
    let n = av.len().max(bv.len());
    for i in 0..n {
        let ai = av.get(i).copied().unwrap_or(0);
        let bi = bv.get(i).copied().unwrap_or(0);
        if ai != bi {
            return ai < bi;
        }
    }
    false
}

/// Read the daemon-recorded version string from a session's
/// `<home>/state/wire/daemon.pid` JSON pidfile. Returns None for
/// legacy bare-integer pidfiles (no version field) and for absent /
/// unreadable files.
fn read_session_pidfile_version(home_dir: &std::path::Path) -> Option<String> {
    let pidfile = home_dir.join("state").join("wire").join("daemon.pid");
    let body = std::fs::read_to_string(&pidfile).ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Read `supervisor.pid` without the liveness check (the snapshot
/// builder runs the check itself, separated so an absent file is
/// just `None` rather than an Err).
fn read_supervisor_pid(path: &std::path::Path) -> Option<u32> {
    if !path.exists() {
        return None;
    }
    let body = std::fs::read_to_string(path).ok()?;
    body.trim().parse::<u32>().ok()
}

/// Read `<home>/state/wire/last_sync.json`'s timestamp and return
/// "seconds since now". None on absent / unreadable / unparseable.
fn read_session_last_sync_age(home_dir: &std::path::Path) -> Option<u64> {
    let path = home_dir.join("state").join("wire").join("last_sync.json");
    let body = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let ts = v.get("ts").and_then(serde_json::Value::as_str)?;
    let parsed =
        time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).ok()?;
    let age = (time::OffsetDateTime::now_utc() - parsed).whole_seconds();
    if age < 0 {
        // Clock skew: timestamp is in the future. Treat as fresh.
        Some(0)
    } else {
        Some(age as u64)
    }
}

fn supervisor_pid_path() -> Result<PathBuf> {
    let root = crate::session::sessions_root()
        .context("resolving sessions_root for supervisor pidfile")?;
    std::fs::create_dir_all(&root).with_context(|| format!("creating {root:?}"))?;
    Ok(root.join("supervisor.pid"))
}

/// True when the machine-wide all-session supervisor owns daemon lifecycle.
/// Read-only and fail-closed: a missing, corrupt, or dead pidfile does not
/// suppress normal single-session recovery.
pub fn supervisor_is_alive() -> bool {
    supervisor_pid_path()
        .and_then(|path| read_alive_supervisor_pid(&path))
        .ok()
        .flatten()
        .is_some()
}

fn read_alive_supervisor_pid(path: &std::path::Path) -> Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let body = std::fs::read_to_string(path).ok();
    let pid = body.as_deref().and_then(|s| s.trim().parse::<u32>().ok());
    match pid {
        Some(p) if crate::ensure_up::pid_is_alive(p) => Ok(Some(p)),
        _ => Ok(None),
    }
}

fn write_supervisor_pid(path: &std::path::Path) -> Result<()> {
    let pid = std::process::id();
    std::fs::write(path, pid.to_string())
        .with_context(|| format!("writing supervisor pidfile {path:?}"))?;
    Ok(())
}

struct SupervisorPidGuard {
    path: PathBuf,
}

impl Drop for SupervisorPidGuard {
    fn drop(&mut self) {
        // Only remove if it still names us — same pattern as
        // DaemonPidGuard in ensure_up.rs.
        if let Ok(body) = std::fs::read_to_string(&self.path)
            && let Ok(pid) = body.trim().parse::<u32>()
            && pid == std::process::id()
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn version_lt_dotted_integer_compare() {
        // Lexical string-compare footgun cases — these must come out right.
        assert!(version_lt("0.9.0", "0.10.0"));
        assert!(version_lt("0.13.5", "0.14.1"));
        assert!(version_lt("0.14.0", "0.14.1"));
        // Equal / greater → not stale.
        assert!(!version_lt("0.14.1", "0.14.1"));
        assert!(!version_lt("0.14.2", "0.14.1"));
        // Shorter version pads with zero.
        assert!(version_lt("0.14", "0.14.1"));
        assert!(!version_lt("0.14.1", "0.14"));
        // Unparseable (pre-release suffix, garbage) is conservatively NOT-stale
        // — under-flagging beats false-positive on `0.14.2-rc.1` vs `0.14.2`.
        assert!(!version_lt("0.14.2-rc.1", "0.14.2"));
        assert!(!version_lt("garbage", "0.14.1"));
        assert!(!version_lt("0.14.1", "garbage"));
    }

    #[test]
    fn read_alive_supervisor_pid_returns_none_when_missing() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("supervisor.pid");
        assert_eq!(read_alive_supervisor_pid(&p).unwrap(), None);
    }

    #[test]
    fn read_alive_supervisor_pid_returns_none_for_dead_pid() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("supervisor.pid");
        // pid 999999 is almost certainly not running.
        std::fs::write(&p, "999999").unwrap();
        assert_eq!(read_alive_supervisor_pid(&p).unwrap(), None);
    }

    #[test]
    fn read_alive_supervisor_pid_returns_pid_for_self() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("supervisor.pid");
        let our_pid = std::process::id();
        std::fs::write(&p, our_pid.to_string()).unwrap();
        assert_eq!(read_alive_supervisor_pid(&p).unwrap(), Some(our_pid));
    }

    #[test]
    fn pid_guard_only_removes_when_pid_still_matches() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("supervisor.pid");
        // Write a foreign pid into the file, then drop a guard for
        // our pid. The guard should leave the foreign pidfile alone.
        std::fs::write(&p, "12345").unwrap();
        {
            let _g = SupervisorPidGuard { path: p.clone() };
        }
        assert!(p.exists(), "guard removed a pidfile that didn't name us");
    }

    #[test]
    fn pid_guard_removes_when_pid_matches() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("supervisor.pid");
        let our_pid = std::process::id();
        std::fs::write(&p, our_pid.to_string()).unwrap();
        {
            let _g = SupervisorPidGuard { path: p.clone() };
        }
        assert!(!p.exists(), "guard left our own pidfile behind");
    }

    #[test]
    fn existing_daemon_for_session_returns_false_when_pidfile_missing() {
        let tmp = tempdir().unwrap();
        // home_dir has no state/wire/daemon.pid
        assert!(!existing_daemon_for_session(tmp.path()).unwrap());
    }

    #[test]
    fn existing_daemon_for_session_returns_false_for_dead_pid() {
        let tmp = tempdir().unwrap();
        let state = tmp.path().join("state").join("wire");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("daemon.pid"), "999999").unwrap();
        assert!(!existing_daemon_for_session(tmp.path()).unwrap());
    }

    #[test]
    fn existing_daemon_for_session_returns_true_for_self_pid() {
        let tmp = tempdir().unwrap();
        let state = tmp.path().join("state").join("wire");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("daemon.pid"), std::process::id().to_string()).unwrap();
        assert!(existing_daemon_for_session(tmp.path()).unwrap());
    }

    #[test]
    fn inactive_worker_classification_distinguishes_version_skew() {
        let mut record = crate::ensure_up::DaemonPid {
            schema: crate::ensure_up::DAEMON_PID_SCHEMA.to_string(),
            pid: 4242,
            bin_path: "/opt/wire".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            started_at: "2026-07-17T00:00:00Z".to_string(),
            did: None,
            relay_url: None,
        };
        assert_eq!(
            classify_session_worker(&record, true, Some("/opt/wire daemon --interval 5")),
            Some(WorkerVersion::Current)
        );
        record.version = "0.16.0".to_string();
        assert_eq!(
            classify_session_worker(&record, true, Some("/opt/wire daemon --interval 5")),
            Some(WorkerVersion::Skewed)
        );
        assert_eq!(
            classify_session_worker(&record, true, Some("/opt/wire daemon --all-sessions")),
            None,
            "never classify the supervisor itself as a session worker"
        );
        assert_eq!(classify_session_worker(&record, false, None), None);
    }

    #[test]
    fn crashed_worker_backoff_is_bounded_and_healthy_run_resets() {
        assert_eq!(next_worker_backoff(None, true), Duration::from_secs(2));
        assert_eq!(
            next_worker_backoff(Some(Duration::from_secs(32)), true),
            MAX_BACKOFF
        );
        assert_eq!(next_worker_backoff(Some(MAX_BACKOFF), true), MAX_BACKOFF);
        assert_eq!(
            next_worker_backoff(Some(MAX_BACKOFF), false),
            INITIAL_BACKOFF
        );
    }

    // ---- stale binary lifecycle partitioning ----

    #[test]
    fn partition_stale_splits_respawnable_from_unmanaged() {
        // #275: stale sessions the supervisor would respawn (eligible) vs ones
        // it would orphan. `wire upgrade --refresh-stale-children` may kill the
        // former (supervisor brings them back) but must leave the latter.
        let stale = vec![
            "bound".to_string(),
            "active".to_string(),
            "orphan".to_string(),
        ];
        let eligible: std::collections::HashSet<String> =
            ["bound".to_string(), "active".to_string()]
                .into_iter()
                .collect();
        let (respawnable, unmanaged) = partition_stale_by_eligibility(&stale, &eligible);
        assert_eq!(respawnable, vec!["bound".to_string(), "active".to_string()]);
        assert_eq!(unmanaged, vec!["orphan".to_string()]);
    }

    #[test]
    fn partition_stale_all_unmanaged_when_none_eligible() {
        // No supervisor-eligible sessions → every stale daemon is unmanaged →
        // none may be killed (the silent-orphan footgun from #275).
        let stale = vec!["a".to_string(), "b".to_string()];
        let eligible = std::collections::HashSet::new();
        let (respawnable, unmanaged) = partition_stale_by_eligibility(&stale, &eligible);
        assert!(respawnable.is_empty());
        assert_eq!(unmanaged, stale);
    }

    // ---- husk reaper ----

    use std::collections::HashSet;

    /// Make a by-key-shaped husk home (`state/wire` only, no identity)
    /// under `root` and return its path. The dir's real mtime is "now",
    /// so tests control age by passing a future `now` to `reap_husks`.
    fn mk_husk(root: &Path, name: &str) -> PathBuf {
        let home = root.join(name);
        std::fs::create_dir_all(home.join("state").join("wire")).unwrap();
        home
    }

    /// `now` far enough in the future that any just-created dir is past
    /// the default 48h cutoff.
    fn far_future() -> SystemTime {
        SystemTime::now() + Duration::from_secs(100 * 3600)
    }

    const CUTOFF_48H: Duration = Duration::from_secs(48 * 3600);

    #[test]
    fn reap_removes_old_identityless_unsynced_husk() {
        let tmp = tempdir().unwrap();
        let home = mk_husk(tmp.path(), "abcdef0123456789");
        let reaped = reap_husks(
            tmp.path(),
            CUTOFF_48H,
            far_future(),
            &HashSet::new(),
            |_| false,
        );
        assert_eq!(reaped, vec![home.clone()]);
        assert!(!home.exists(), "husk dir should be gone");
    }

    #[test]
    fn reap_keeps_identity_homes_regardless_of_age() {
        let tmp = tempdir().unwrap();
        let home = mk_husk(tmp.path(), "abcdef0123456789");
        let cfg = home.join("config").join("wire");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("private.key"), "k").unwrap();
        let reaped = reap_husks(
            tmp.path(),
            CUTOFF_48H,
            far_future(),
            &HashSet::new(),
            |_| false,
        );
        assert!(reaped.is_empty());
        assert!(home.exists(), "identity-bearing home must never be reaped");
    }

    #[test]
    fn reap_keeps_homes_that_ever_synced() {
        let tmp = tempdir().unwrap();
        let home = mk_husk(tmp.path(), "abcdef0123456789");
        std::fs::write(home.join("state").join("wire").join("last_sync.json"), "{}").unwrap();
        let reaped = reap_husks(
            tmp.path(),
            CUTOFF_48H,
            far_future(),
            &HashSet::new(),
            |_| false,
        );
        assert!(reaped.is_empty());
        assert!(home.exists(), "synced home must never be reaped");
    }

    #[test]
    fn reap_keeps_young_husks() {
        let tmp = tempdir().unwrap();
        let home = mk_husk(tmp.path(), "abcdef0123456789");
        // `now` = actual now → dir age ≈ 0 < 48h.
        let reaped = reap_husks(
            tmp.path(),
            CUTOFF_48H,
            SystemTime::now(),
            &HashSet::new(),
            |_| false,
        );
        assert!(reaped.is_empty());
        assert!(home.exists(), "young husk must get its grace window");
    }

    #[test]
    fn reap_keeps_registry_bound_names() {
        let tmp = tempdir().unwrap();
        let home = mk_husk(tmp.path(), "abcdef0123456789");
        let bound: HashSet<String> = ["abcdef0123456789".to_string()].into();
        let reaped = reap_husks(tmp.path(), CUTOFF_48H, far_future(), &bound, |_| false);
        assert!(reaped.is_empty());
        assert!(home.exists(), "operator-bound home must never be reaped");
    }

    #[test]
    fn reap_keeps_homes_with_live_daemon() {
        let tmp = tempdir().unwrap();
        let home = mk_husk(tmp.path(), "abcdef0123456789");
        let reaped = reap_husks(
            tmp.path(),
            CUTOFF_48H,
            far_future(),
            &HashSet::new(),
            |_| true,
        );
        assert!(reaped.is_empty());
        assert!(home.exists(), "daemon-owned home must never be reaped");
    }

    #[test]
    fn reap_ignores_non_by_key_shaped_names() {
        let tmp = tempdir().unwrap();
        // Named session, uppercase hex, and wrong-length hex — all
        // outside the by-key shape, all untouchable.
        let named = mk_husk(tmp.path(), "my-session");
        let upper = mk_husk(tmp.path(), "ABCDEF0123456789");
        let short = mk_husk(tmp.path(), "abcdef012345678");
        let reaped = reap_husks(
            tmp.path(),
            CUTOFF_48H,
            far_future(),
            &HashSet::new(),
            |_| false,
        );
        assert!(reaped.is_empty());
        assert!(named.exists() && upper.exists() && short.exists());
    }

    #[test]
    fn reap_missing_root_is_a_noop() {
        let tmp = tempdir().unwrap();
        let reaped = reap_husks(
            &tmp.path().join("no-such-by-key"),
            CUTOFF_48H,
            far_future(),
            &HashSet::new(),
            |_| false,
        );
        assert!(reaped.is_empty());
    }

    #[test]
    fn husk_reap_max_age_parsing() {
        // Unset → 48h default.
        assert_eq!(
            parse_husk_reap_max_age(None),
            Some(Duration::from_secs(48 * 3600))
        );
        // 0 → disabled.
        assert_eq!(parse_husk_reap_max_age(Some("0")), None);
        // Explicit hours.
        assert_eq!(
            parse_husk_reap_max_age(Some("12")),
            Some(Duration::from_secs(12 * 3600))
        );
        // Garbage → default, not disabled.
        assert_eq!(
            parse_husk_reap_max_age(Some("soon")),
            Some(Duration::from_secs(48 * 3600))
        );
    }

    fn initialized_session(name: &str, bound: bool) -> crate::session::SessionInfo {
        crate::session::SessionInfo {
            name: name.to_string(),
            cwd: bound.then(|| format!("/projects/{name}")),
            home_dir: PathBuf::from(format!("/sessions/{name}")),
            did: Some(format!("did:wire:{name}-1234abcd")),
            handle: Some(format!("{name}-1234abcd")),
            daemon_running: false,
            character: None,
        }
    }

    #[test]
    fn planner_covers_one_ten_and_655_homes_with_hard_cap() {
        for total in [1usize, 10, 655] {
            let sessions: Vec<_> = (0..total)
                .map(|i| initialized_session(&format!("s{i:03}"), false))
                .collect();
            let plan = plan_supervisor_sessions(sessions, 16, |_| true, |_| false, |_| false);
            assert_eq!(plan.selected.len(), total.min(16));
            assert_eq!(plan.queued.len(), total.saturating_sub(16));
            assert!(plan.inactive.is_empty());
        }
    }

    #[test]
    fn planner_keeps_five_live_and_leaves_650_stale_inactive() {
        let sessions: Vec<_> = (0..655)
            .map(|i| initialized_session(&format!("s{i:03}"), false))
            .collect();
        let plan = plan_supervisor_sessions(
            sessions,
            16,
            |home| {
                home.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_prefix('s'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .is_some_and(|i| i < 5)
            },
            |_| false,
            |_| false,
        );
        assert_eq!(plan.selected.len(), 5);
        assert!(plan.queued.is_empty());
        assert_eq!(plan.inactive.len(), 650);
    }

    #[test]
    fn private_key_without_bound_lease_or_outbox_is_inactive() {
        let tmp = tempdir().unwrap();
        let config = tmp.path().join("config/wire");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("private.key"), [7u8; 32]).unwrap();
        let mut session = initialized_session("historical", false);
        session.home_dir = tmp.path().to_path_buf();

        let plan = plan_supervisor_sessions(vec![session], 16, |_| false, |_| false, |_| false);
        assert!(plan.selected.is_empty());
        assert_eq!(plan.inactive, vec!["historical"]);
    }

    #[test]
    fn planner_state_is_restart_stable_and_retirement_wins() {
        let sessions = vec![
            initialized_session("bound", true),
            initialized_session("leased", false),
            initialized_session("retired", true),
        ];
        let build = || {
            plan_supervisor_sessions(
                sessions.clone(),
                16,
                |home| home.ends_with("leased"),
                |_| false,
                |home| home.ends_with("retired"),
            )
        };
        let before = build();
        let after_restart = build();
        assert_eq!(before.selected_names(), after_restart.selected_names());
        assert_eq!(before.retired, vec!["retired"]);
        assert_eq!(before.selected_names(), vec!["bound", "leased"]);
    }
}
