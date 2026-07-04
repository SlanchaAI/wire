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

pub fn cmd_dash(watch: bool, json: bool, all: bool, probe: bool) -> Result<()> {
    let opts = CollectOpts {
        probe_relays: probe,
    };
    let color = std::io::stdout().is_terminal();
    if watch {
        // Watch is the outer loop: each tick emits JSON (one compact object)
        // or the table, so `--watch --json` streams and `--watch | pipe` keeps
        // looping instead of silently printing once.
        loop {
            let report = dash::collect(&opts)?;
            print!("\x1b[2J\x1b[H"); // clear + home
            if json {
                println!("{}", serde_json::to_string(&report)?);
            } else {
                print!("{}", render(&report, all, color));
            }
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    let report = dash::collect(&opts)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render(&report, all, color));
    }
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

fn render(report: &dash::DashReport, all: bool, color: bool) -> String {
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
    let husks = report
        .sessions
        .iter()
        .filter(|s| matches!(s.daemon, DaemonState::StalePid { .. }))
        .count();

    let mut out = String::new();
    out.push_str(&format!(
        "wire dash — {total} identities · {running} running · {paired} paired · {idle} idle · {husks} husks\n"
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

    let mut hidden = 0usize;
    for s in &report.sessions {
        // Collapse idle solo daemons unless --all.
        if s.likely_idle && !all {
            hidden += 1;
            continue;
        }
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
        // cwd is the reap-decision signal for idle daemons — which project a
        // stale daemon belongs to. Dim, truncated.
        let cwd = s
            .cwd
            .as_deref()
            .map(|c| format!("  {}", dim(&truncate(c, CWD_MAX), color)))
            .unwrap_or_default();
        out.push_str(&format!(
            "{name_col} {daemon} {fp:<FP_W$} {peers:>5} {sync:>6}  {relay}{cwd}\n",
            daemon = daemon_cell(&s.daemon, color),
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
    if hidden > 0 {
        let plural = if hidden == 1 { "" } else { "s" };
        out.push_str(&dim(
            &format!("\n… {hidden} idle solo daemon{plural} hidden (--all to show)\n"),
            color,
        ));
    }
    out.push_str(&dim(
        "run `wire dash` on each box for a fleet view.\n",
        color,
    ));
    out
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
        let collapsed = render(&report, false, false);
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

        let expanded = render(&report, true, false);
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
    fn header_counts_are_accurate() {
        let report = DashReport {
            schema: dash::SCHEMA,
            sessions: vec![snap("a", true, 2), snap("b", true, 0), snap("c", false, 0)],
            relays: vec![],
        };
        let out = render(&report, true, false);
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
        let out = render(&report, true, true);
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
        let out = render(&report, false, false);
        assert!(out.contains("/Users/x/Source/wire"), "cwd shown:\n{out}");
    }
}
