//! `wire dash` — one pane for every wire identity on this box.
//!
//! Renders [`crate::dash::collect`]: each session's persona, daemon liveness,
//! pinned peers, relay binding, and sync recency. Paired sessions float to the
//! top; the idle solo-daemon throwaways collapse into a count (`--all` expands).
//! Read-only — never spawns or kills a daemon.

use crate::dash::{self, CollectOpts, DaemonState, SessionSnapshot};
use anyhow::Result;
use std::io::IsTerminal;
use std::time::Duration;

pub fn cmd_dash(watch: bool, json: bool, all: bool, probe: bool) -> Result<()> {
    let opts = CollectOpts {
        probe_relays: probe,
    };
    if json {
        let report = dash::collect(&opts)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if watch {
        let color = std::io::stdout().is_terminal();
        loop {
            let report = dash::collect(&opts)?;
            // Clear screen + home cursor, then repaint.
            print!("\x1b[2J\x1b[H");
            print!("{}", render(&report, all, color));
            use std::io::Write;
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    let report = dash::collect(&opts)?;
    let color = std::io::stdout().is_terminal();
    print!("{}", render(&report, all, color));
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

fn daemon_cell(d: &DaemonState, color: bool) -> String {
    let (glyph, code) = match d {
        DaemonState::Running { .. } => ("● live", "32"), // green
        DaemonState::StalePid { .. } => ("○ husk", "31"), // red
        DaemonState::None => ("· none", "2"),            // dim
    };
    if color {
        format!("\x1b[{code}m{glyph}\x1b[0m")
    } else {
        glyph.to_string()
    }
}

fn name_cell(s: &SessionSnapshot, color: bool) -> String {
    let emoji = s.emoji.as_deref().unwrap_or("·");
    let name = s
        .handle
        .as_deref()
        .or(s.nickname.as_deref())
        .unwrap_or(&s.key);
    if color && let Some(c) = s.ansi256_primary {
        format!("{emoji} \x1b[38;5;{c}m{name}\x1b[0m")
    } else {
        format!("{emoji} {name}")
    }
}

fn dim(text: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{text}\x1b[0m")
    } else {
        text.to_string()
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

    // Header row.
    out.push_str(&dim(
        &format!(
            "{:<26} {:<9} {:<7} {:>5} {:>6}  {}\n",
            "IDENTITY", "DAEMON", "FP", "PEERS", "SYNC", "RELAY"
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
        let name = name_cell(s, color);
        // name may carry ANSI; pad on the visible-width by padding the plain form.
        let plain_name = format!(
            "{} {}",
            s.emoji.as_deref().unwrap_or("·"),
            s.handle
                .as_deref()
                .or(s.nickname.as_deref())
                .unwrap_or(&s.key)
        );
        let name_pad = 26usize.saturating_sub(plain_name.chars().count());
        let fp = s.fingerprint.as_deref().unwrap_or("—");
        let peers = s.peers.len();
        let sync = fmt_age(s.last_sync_age_s);
        let relay = s.relay_url.as_deref().unwrap_or("—");
        out.push_str(&format!(
            "{name}{pad} {daemon:<9} {fp:<7} {peers:>5} {sync:>6}  {relay}\n",
            pad = " ".repeat(name_pad),
            daemon = daemon_cell(&s.daemon, color),
            fp = fp,
            peers = peers,
            sync = sync,
            relay = relay,
        ));
        // Show pinned peers under a paired session.
        if !s.peers.is_empty() {
            let list: Vec<String> = s
                .peers
                .iter()
                .map(|p| format!("{} ({})", p.handle, p.tier))
                .collect();
            out.push_str(&dim(&format!("    ↳ {}\n", list.join(", ")), color));
        }
    }
    if hidden > 0 {
        out.push_str(&dim(
            &format!("\n… {hidden} idle solo daemons hidden (--all to show)\n"),
            color,
        ));
    }
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
            collapsed.contains("1 idle solo daemons hidden"),
            "idle collapses by default:\n{collapsed}"
        );
        // The paired session is shown even collapsed, and shows its own peer
        // line; only the idle session is hidden.
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
    fn fmt_age_scales() {
        assert_eq!(fmt_age(None), "—");
        assert_eq!(fmt_age(Some(5)), "5s");
        assert_eq!(fmt_age(Some(120)), "2m");
        assert_eq!(fmt_age(Some(7200)), "2h");
        assert_eq!(fmt_age(Some(172800)), "2d");
    }
}
