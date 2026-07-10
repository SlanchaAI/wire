//! `wire dash` — one pane for every wire identity on this box.
//!
//! Renders [`crate::dash::collect`]: each session's persona, daemon liveness,
//! pinned peers, relay binding, and sync recency. Paired sessions float to the
//! top; the idle solo-daemon throwaways collapse into a count (`--all` expands).
//! Read-only — never spawns or kills a daemon.

use crate::character::sanitize_display_text;
use crate::dash::{self, CollectOpts, DaemonState, SessionSnapshot};
use anyhow::Result;
use std::io::{IsTerminal, Write};
use std::time::Duration;

/// Visible width of the name column (emoji + handle). Emoji display width is
/// terminal-dependent (usually 2); we pad on Rust char count, so rows with an
/// emoji may sit ~1 column wider than plain rows — acceptable for a glance tool.
const NAME_W: usize = 24;
/// Visible width of the daemon cell — every label (`● live` / `○ husk` /
/// `· none`) is exactly 6 chars, matching the `DAEMON` header.
const DAEMON_W: usize = 6;
/// Fingerprints are 8 hex chars; give the column one space of breathing room.
const FP_W: usize = 9;
const CWD_MAX: usize = 30;

/// Write a frame to stdout, exiting cleanly on a broken pipe. `wire dash` is
/// meant to be piped (`head`, `jq`, the Mission Control reporter, an SSH tail);
/// the default `print!`/`println!` macros PANIC on EPIPE, so a downstream reader
/// closing early would crash wire with a backtrace. Exit 0 instead.
fn emit(text: &str) {
    let mut out = std::io::stdout().lock();
    let r = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    if let Err(e) = r
        && e.kind() == std::io::ErrorKind::BrokenPipe
    {
        std::process::exit(0);
    }
}

/// Parsed `wire dash` flags.
pub struct DashArgs {
    pub watch: bool,
    pub json: bool,
    pub all: bool,
    pub probe: bool,
    pub retired: bool,
    pub retire_idle: bool,
    pub older_than: Option<u64>,
    pub dry_run: bool,
    pub force: bool,
}

/// A warning banner when this shell's wire identity is split — the env resolves
/// a different identity than Claude Code's live session (a stale wire process,
/// usually an MCP server, answering as the wrong identity). Empty when healthy.
fn split_banner(color: bool) -> String {
    let Some(s) = crate::session::detect_identity_split() else {
        return String::new();
    };
    let headline = match (s.operational_handle.as_deref(), s.live_handle.as_deref()) {
        (Some(op), Some(live)) => {
            format!("this process operates as {op} but your live Claude session is {live}")
        }
        _ => format!(
            "the wire identity this process serves (via {}) doesn't match your live Claude session",
            s.source
        ),
    };
    let body = format!(
        "⚠ identity split — {headline}.\n  A stale wire process (usually an MCP server) is bound to the wrong identity. Fix: reconnect it (/mcp) or restart this session.\n\n"
    );
    if color {
        format!("\x1b[33m{body}\x1b[0m")
    } else {
        body
    }
}

pub fn cmd_dash(args: DashArgs) -> Result<()> {
    if args.retire_idle {
        return cmd_retire_idle(
            args.older_than.unwrap_or(7),
            args.dry_run,
            args.force,
            args.json,
        );
    }
    let opts = CollectOpts {
        probe_relays: args.probe,
    };
    let color = std::io::stdout().is_terminal();
    if args.watch {
        // Watch is the outer loop: each tick emits JSON (one compact object)
        // or the table, so `--watch --json` streams and `--watch | pipe` keeps
        // looping instead of silently printing once.
        loop {
            let report = dash::collect(&opts)?;
            let mut frame = String::from("\x1b[2J\x1b[H"); // clear + home
            if args.json {
                frame.push_str(&serde_json::to_string(&report)?);
                frame.push('\n');
            } else {
                frame.push_str(&split_banner(color));
                frame.push_str(&render(&report, args.all, args.retired, color));
            }
            emit(&frame);
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    let report = dash::collect(&opts)?;
    let frame = if args.json {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    } else {
        format!(
            "{}{}",
            split_banner(color),
            render(&report, args.all, args.retired, color)
        )
    };
    emit(&frame);
    Ok(())
}

fn fmt_age(s: Option<u64>) -> String {
    match s {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) if s < 86400 => format!("{}h", s / 3600),
        Some(s) => format!("{}d", s / 86400),
    }
}

/// A fixed-width daemon cell: pad the *plain* label first, then wrap it in
/// color so the visible width stays `DAEMON_W` (wrapping an already-padded
/// ANSI string with `{:<N}` would add zero fill and misalign every color row).
fn daemon_cell(d: &DaemonState, color: bool) -> String {
    let (label, code) = match d {
        DaemonState::Running { .. } => ("● live", "32"), // green
        DaemonState::StalePid { .. } => ("○ husk", "31"), // red
        DaemonState::None => ("· none", "2"),            // dim
    };
    let padded = format!("{label:<DAEMON_W$}");
    if color {
        format!("\x1b[{code}m{padded}\x1b[0m")
    } else {
        padded
    }
}

fn dim(text: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Display name for a session: sanitized `handle` (or nickname, or key).
fn display_name(s: &SessionSnapshot) -> String {
    let raw = s
        .handle
        .as_deref()
        .or(s.nickname.as_deref())
        .unwrap_or(&s.key);
    sanitize_display_text(raw)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// The retired marker cell, fixed to `DAEMON_W` visible chars (magenta).
fn retired_cell(color: bool) -> String {
    let label = "◌ retd";
    let padded = format!("{label:<DAEMON_W$}");
    if color {
        format!("\x1b[35m{padded}\x1b[0m")
    } else {
        padded
    }
}

fn render(report: &dash::DashReport, all: bool, retired_only: bool, color: bool) -> String {
    let total = report.sessions.len();
    let running = report
        .sessions
        .iter()
        .filter(|s| s.daemon.is_running())
        .count();
    let paired = report
        .sessions
        .iter()
        .filter(|s| !s.peers.is_empty())
        .count();
    let idle = report.sessions.iter().filter(|s| s.likely_idle).count();
    let retired_n = report.sessions.iter().filter(|s| s.retired).count();
    let husks = report
        .sessions
        .iter()
        .filter(|s| matches!(s.daemon, DaemonState::StalePid { .. }))
        .count();

    let mut out = String::new();
    out.push_str(&format!(
        "wire dash — {total} identities · {running} running · {paired} paired · {idle} idle · {retired_n} retired · {husks} husks\n"
    ));
    for r in &report.relays {
        let health = if r.unprobed {
            dim("(unprobed — --probe for health)", color)
        } else if r.ok {
            let s = r.status.map(|c| c.to_string()).unwrap_or_default();
            if color {
                format!("\x1b[32m[{s} ok]\x1b[0m")
            } else {
                format!("[{s} ok]")
            }
        } else {
            let s = r
                .status
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unreachable".into());
            if color {
                format!("\x1b[31m[{s}]\x1b[0m")
            } else {
                format!("[{s}]")
            }
        };
        out.push_str(&format!("relay {}  {health}\n", r.url));
    }
    out.push('\n');

    // Header row (padded to the same widths the data rows use).
    out.push_str(&dim(
        &format!(
            "{:<NAME_W$} {:<DAEMON_W$} {:<FP_W$} {:>5} {:>6}  {}\n",
            "IDENTITY", "DAEMON", "FP", "PEERS", "SYNC", "RELAY / CWD"
        ),
        color,
    ));

    let mut hidden_idle = 0usize;
    let mut hidden_retired = 0usize;
    let mut shown = 0usize;
    for s in &report.sessions {
        // Visibility: `--retired` shows only retired; otherwise idle solo
        // daemons AND retired identities collapse unless `--all`.
        let show = if retired_only {
            s.retired
        } else if all {
            true
        } else {
            !s.likely_idle && !s.retired
        };
        if !show {
            if !retired_only {
                if s.retired {
                    hidden_retired += 1;
                } else if s.likely_idle {
                    hidden_idle += 1;
                }
            }
            continue;
        }
        shown += 1;
        let name = display_name(s);
        let emoji = s.emoji.as_deref().unwrap_or("·");
        let plain_name = format!("{emoji} {name}");
        let name_pad = NAME_W.saturating_sub(plain_name.chars().count());
        // Colorize just the name; pad with plain spaces after (color must not
        // affect the computed column width).
        let name_col = if color && let Some(c) = s.ansi256_primary {
            format!(
                "{emoji} \x1b[38;5;{c}m{name}\x1b[0m{}",
                " ".repeat(name_pad)
            )
        } else {
            format!("{plain_name}{}", " ".repeat(name_pad))
        };
        let fp = s.fingerprint.as_deref().unwrap_or("—");
        let peers = s.peers.len();
        let sync = fmt_age(s.last_sync_age_s);
        let relay = s.relay_url.as_deref().unwrap_or("—");
        // cwd is the reap-decision signal — which project an idle/retired
        // daemon belongs to. Dim, truncated.
        let cwd = s
            .cwd
            .as_deref()
            .map(|c| format!("  {}", dim(&truncate(c, CWD_MAX), color)))
            .unwrap_or_default();
        // Retired identities show a distinct marker, not their (stopped) daemon.
        let daemon = if s.retired {
            retired_cell(color)
        } else {
            daemon_cell(&s.daemon, color)
        };
        out.push_str(&format!(
            "{name_col} {daemon} {fp:<FP_W$} {peers:>5} {sync:>6}  {relay}{cwd}\n"
        ));
        // Pinned peers under a paired session (sanitized — handle/tier are
        // peer-published text that must not inject terminal escapes).
        if !s.peers.is_empty() {
            let list: Vec<String> = s
                .peers
                .iter()
                .map(|p| {
                    format!(
                        "{} ({})",
                        sanitize_display_text(&p.handle),
                        sanitize_display_text(&p.tier)
                    )
                })
                .collect();
            out.push_str(&dim(&format!("    ↳ {}\n", list.join(", ")), color));
        }
    }
    if retired_only {
        if shown == 0 {
            out.push_str(&dim("no retired identities.\n", color));
        }
    } else {
        if hidden_idle > 0 {
            let p = if hidden_idle == 1 { "" } else { "s" };
            out.push_str(&dim(
                &format!(
                    "\n… {hidden_idle} idle solo daemon{p} hidden (--all to show · `wire dash --retire-idle` to clean up)\n"
                ),
                color,
            ));
        }
        if hidden_retired > 0 {
            let noun = if hidden_retired == 1 {
                "identity"
            } else {
                "identities"
            };
            out.push_str(&dim(
                &format!(
                    "… {hidden_retired} retired {noun} hidden (--retired to list · `wire revive <handle>` to restore)\n"
                ),
                color,
            ));
        }
    }
    out.push_str(&dim(
        "run `wire dash` on each box for a fleet view.\n",
        color,
    ));
    out
}

/// `wire dash --retire-idle` — reversibly retire every idle solo daemon.
/// Selection: running daemon ∧ 0 pinned peers ∧ not the current identity ∧ no
/// pending inbound pair ∧ not already retired ∧ daemon running longer than the
/// cutoff. Dry-run unless confirmed; guards re-checked at kill time (TOCTOU).
fn cmd_retire_idle(older_than_days: u64, dry_run: bool, force: bool, json: bool) -> Result<()> {
    use crate::retire;
    use std::path::PathBuf;
    // Fail closed: if we can't identify our own home, refuse — never risk
    // retiring the identity the operator is using right now.
    let current = retire::current_home().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot resolve the current identity's home — refusing --retire-idle (would risk retiring the session you're using)"
        )
    })?;
    let cutoff_s = older_than_days.saturating_mul(86_400);
    let is_current = |home: &std::path::Path| -> bool {
        std::fs::canonicalize(home)
            .map(|h| h == current)
            .unwrap_or(false)
    };

    // Iterate session homes DIRECTLY — each SessionInfo carries its own
    // home_dir. Deliberately NOT a name-keyed map: SessionInfo.name is the
    // DID-derived handle, which can collide across ~270 identities (32-bit
    // nickname space → birthday paradox), and a collision would collapse two
    // homes and retire the wrong one.
    struct Cand {
        home: PathBuf,
        label: String,
        fp: String,
        emoji: String,
        cwd: Option<String>,
        did: Option<String>,
        handle: Option<String>,
    }
    let sessions = crate::session::list_sessions()?;
    let mut candidates: Vec<Cand> = Vec::new();
    for si in &sessions {
        let home = &si.home_dir;
        if !si.daemon_running
            || retire::is_retired(home)
            || is_current(home)
            || retire::has_pending_inbound(home)
            || retire::identity_age_s(home)
                .map(|a| a < cutoff_s)
                .unwrap_or(true)
        {
            continue;
        }
        // Peer read last (a file read per session).
        if !crate::dash::read_peers(home, si.did.as_deref(), si.handle.as_deref()).is_empty() {
            continue;
        }
        candidates.push(Cand {
            home: home.clone(),
            label: si.handle.clone().unwrap_or_else(|| si.name.clone()),
            fp: si
                .did
                .as_deref()
                .and_then(crate::dash::fingerprint_from_did)
                .unwrap_or_else(|| "—".into()),
            emoji: si
                .character
                .as_ref()
                .map(|c| c.emoji.clone())
                .unwrap_or_else(|| "·".into()),
            cwd: si.cwd.clone(),
            did: si.did.clone(),
            handle: si.handle.clone(),
        });
    }

    if candidates.is_empty() {
        if json {
            emit(&format!(
                "{}\n",
                serde_json::to_string_pretty(
                    &serde_json::json!({"retired":[],"note":"no idle identities matched"})
                )?
            ));
        } else {
            eprintln!(
                "no idle solo daemons to retire (identity older than {older_than_days}d, 0 peers, not current/paired/pending)."
            );
        }
        return Ok(());
    }

    // JSON dry-run: structured preview for automation.
    if json && dry_run {
        let list: Vec<_> = candidates
            .iter()
            .map(|c| serde_json::json!({"handle": c.handle, "fp": c.fp, "cwd": c.cwd}))
            .collect();
        emit(&format!(
            "{}\n",
            serde_json::to_string_pretty(
                &serde_json::json!({"would_retire": list, "count": candidates.len()})
            )?
        ));
        return Ok(());
    }

    // Name the victims before the confirm gate (like `wire nuke`).
    eprintln!(
        "wire dash --retire-idle would retire {} idle identit{}:",
        candidates.len(),
        if candidates.len() == 1 { "y" } else { "ies" }
    );
    for c in &candidates {
        eprintln!(
            "  {} {}  {}  {}",
            c.emoji,
            c.label,
            c.fp,
            c.cwd.as_deref().unwrap_or("")
        );
    }
    if dry_run {
        eprintln!(
            "\n(dry run — nothing retired. Re-run without --dry-run; `wire revive <handle>` undoes any.)"
        );
        return Ok(());
    }

    // Typed confirm, unless --force. --force skips the prompt ONLY — it never
    // bypasses the selection guards above.
    if !force {
        use std::io::{IsTerminal, Write};
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "refusing to retire {} identities without a TTY confirmation (use --force for automation)",
                candidates.len()
            );
        }
        eprint!(
            "\nType `retire` to retire these {} identities (reversible): ",
            candidates.len()
        );
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        if line.trim() != "retire" {
            eprintln!("aborted — nothing retired.");
            return Ok(());
        }
    }

    // Execute, re-checking guards per target at kill time (TOCTOU: a candidate
    // could have paired / become current / gotten a pending request during the
    // confirm latency).
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut retired: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new(); // a guard changed during confirm
    let mut errored: Vec<String> = Vec::new();
    let total = candidates.len();
    for (i, c) in candidates.iter().enumerate() {
        let still_ok = !is_current(&c.home)
            && !retire::has_pending_inbound(&c.home)
            && !retire::is_retired(&c.home)
            && crate::dash::read_peers(&c.home, c.did.as_deref(), c.handle.as_deref()).is_empty();
        if !still_ok {
            skipped.push(c.label.clone());
            continue;
        }
        if !json {
            eprintln!("  [{}/{total}] retiring {}…", i + 1, c.label);
        }
        match retire::retire_session(
            &c.home,
            "idle-sweep",
            now_unix,
            retire::stop_daemon_graceful_then_force,
        ) {
            Ok(_) => retired.push(c.label.clone()),
            Err(e) => errored.push(format!("{}: {e}", c.label)),
        }
    }

    if json {
        emit(&format!(
            "{}\n",
            serde_json::to_string_pretty(
                &serde_json::json!({"retired": retired, "skipped": skipped, "errored": errored})
            )?
        ));
    } else {
        let mut note = String::new();
        if !skipped.is_empty() {
            note.push_str(&format!(
                "skipped {} (changed during confirm). ",
                skipped.len()
            ));
        }
        if !errored.is_empty() {
            note.push_str(&format!("{} errored. ", errored.len()));
        }
        eprintln!(
            "\nretired {} idle identit{}. {note}`wire dash --retired` lists them; `wire revive <handle>` restores one.",
            retired.len(),
            if retired.len() == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::{DashReport, PeerRow, RelayHealth};

    fn snap(key: &str, running: bool, peers: usize) -> SessionSnapshot {
        SessionSnapshot {
            key: key.to_string(),
            handle: Some(key.to_string()),
            did: Some(format!("did:wire:{key}-deadbeef")),
            fingerprint: Some("deadbeef".to_string()),
            nickname: Some(key.to_string()),
            emoji: Some("🦊".to_string()),
            primary_hex: Some("#da60a3".to_string()),
            ansi256_primary: Some(175),
            daemon: if running {
                DaemonState::Running { pid: 1 }
            } else {
                DaemonState::None
            },
            daemon_version: Some("0.16.0".to_string()),
            relay_url: Some("https://wireup.net".to_string()),
            slot_id: Some("abc".to_string()),
            last_sync_age_s: Some(5),
            peers: (0..peers)
                .map(|i| PeerRow {
                    handle: format!("peer{i}"),
                    did: format!("did:wire:peer{i}-0000"),
                    tier: "VERIFIED".to_string(),
                })
                .collect(),
            cwd: None,
            likely_idle: running && peers == 0,
            retired: false,
        }
    }

    #[test]
    fn idle_solo_daemons_collapse_unless_all() {
        let report = DashReport {
            schema: dash::SCHEMA,
            sessions: vec![snap("paired", true, 1), snap("idle", true, 0)],
            relays: vec![RelayHealth {
                url: "https://wireup.net".to_string(),
                ok: false,
                status: None,
                unprobed: true,
            }],
        };
        let collapsed = render(&report, false, false, false);
        assert!(collapsed.contains("paired"), "paired session always shown");
        assert!(
            collapsed.contains("1 idle solo daemon hidden"),
            "idle collapses (singular):\n{collapsed}"
        );
        // The paired session is shown even collapsed; only the idle row hides.
        assert!(
            !collapsed.contains("🦊 idle"),
            "idle row is hidden:\n{collapsed}"
        );

        let expanded = render(&report, true, false, false);
        assert!(
            !expanded.contains("hidden"),
            "--all shows everything:\n{expanded}"
        );
        assert!(
            expanded.contains("↳ peer0 (VERIFIED)"),
            "peer line under paired"
        );
    }

    #[test]
    fn retired_collapses_by_default_and_lists_with_retired_flag() {
        let mut r = snap("gone", false, 0); // daemon stopped
        r.retired = true;
        r.likely_idle = false;
        let report = DashReport {
            schema: dash::SCHEMA,
            sessions: vec![snap("live", true, 1), r],
            relays: vec![],
        };
        // Default view: retired row hidden (NOT via likely_idle, which is false
        // once the daemon dies), counted in the summary + hidden note.
        let def = render(&report, false, false, false);
        assert!(
            !def.contains("🦊 gone"),
            "retired hidden by default:\n{def}"
        );
        assert!(
            def.contains("1 retired ·"),
            "retired counted in summary:\n{def}"
        );
        assert!(
            def.contains("retired identity hidden"),
            "retired hidden note:\n{def}"
        );
        // --retired: only retired rows.
        let only = render(&report, false, true, false);
        assert!(
            only.contains("🦊 gone"),
            "retired shown under --retired:\n{only}"
        );
        assert!(
            !only.contains("🦊 live"),
            "non-retired hidden under --retired:\n{only}"
        );
    }

    #[test]
    fn header_counts_are_accurate() {
        let report = DashReport {
            schema: dash::SCHEMA,
            sessions: vec![snap("a", true, 2), snap("b", true, 0), snap("c", false, 0)],
            relays: vec![],
        };
        let out = render(&report, true, false, false);
        assert!(out.contains("3 identities"), "{out}");
        assert!(out.contains("2 running"), "{out}");
        assert!(out.contains("1 paired"), "{out}");
        assert!(out.contains("1 idle"), "{out}");
    }

    #[test]
    fn peer_handle_terminal_escape_is_stripped() {
        let mut s = snap("victim", true, 0);
        s.peers = vec![PeerRow {
            handle: "evil\x1b[2Jhandle".to_string(),
            did: "did:wire:evil-0000".to_string(),
            tier: "VERIFIED".to_string(),
        }];
        s.likely_idle = false;
        let report = DashReport {
            schema: dash::SCHEMA,
            sessions: vec![s],
            relays: vec![],
        };
        let out = render(&report, true, false, true);
        // The live screen-clear escape (ESC[2J) must not survive into output.
        // sanitize_display_text strips the ESC byte; the inert "[2J" text may
        // remain, which is harmless (no ESC = no terminal action).
        assert!(
            !out.contains("\x1b[2J"),
            "injected ESC[2J must not survive:\n{out:?}"
        );
        assert!(out.contains("evil"), "sanitized handle text still shown");
        assert!(out.contains("handle"), "sanitized handle text still shown");
    }

    #[test]
    fn fmt_age_scales() {
        assert_eq!(fmt_age(None), "—");
        assert_eq!(fmt_age(Some(5)), "5s");
        assert_eq!(fmt_age(Some(120)), "2m");
        assert_eq!(fmt_age(Some(7200)), "2h");
        assert_eq!(fmt_age(Some(172800)), "2d");
    }

    #[test]
    fn cwd_rendered_when_present() {
        let mut s = snap("proj", true, 1);
        s.cwd = Some("/Users/x/Source/wire".to_string());
        let report = DashReport {
            schema: dash::SCHEMA,
            sessions: vec![s],
            relays: vec![],
        };
        let out = render(&report, false, false, false);
        assert!(out.contains("/Users/x/Source/wire"), "cwd shown:\n{out}");
    }
}
