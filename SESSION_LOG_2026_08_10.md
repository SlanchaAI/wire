# Session log — 2026-08-10

## Goal

Repair local Wire identity/daemon ambiguity and ship a one-machine operator dashboard for live agent sessions. The dashboard may view sessions, link exactly two, and create one shared group. Remote machines, retired-session management, history, and messaging remain deferred.

## Root causes

- Bare shell and background monitor processes used the `machine-default` identity because no session key reached them.
- One standalone daemon ran outside the managed all-session supervisor.
- Goose exposed its thread identity through `AGENT_SESSION_ID`, but Wire did not recognize that source.
- Existing MCP leases lacked enough safe metadata to render harness, identity provenance, machine, project, and uptime. The dashboard also mislabeled `session_source` as the agent harness.

## Changes

- Resolve guarded Goose sessions when `AGENT=goose`; preserve Codex precedence and scrub adapter identity variables from child commands.
- Record lease acquisition time, working directory, machine, harness, and project descriptors while remaining compatible with old leases.
- Collect only live, initialized, non-retired MCP leases. Expose no raw thread ID, token, or private path.
- Add explicit-home local pair and shared-group operations with postcondition checks.
- Add `wire dash --web [--no-open]`: loopback-only Axum server, per-launch 256-bit token, authenticated inventory and mutations, local Host/Origin checks, CSP/security headers, and confirmation-race protection.
- Advance inventory to `wire-live-sessions-v2` with separate machine, harness, identity, and project objects. Old leases recover facts from one PID-set-cached process snapshot; Git discovery reads repository/worktree files without per-row `git` subprocesses.
- Add the Open Band operator UI with compact harness/project/machine/identity columns and an independent expandable provenance panel. Missing facts render `Unknown`.
- Strip URL userinfo before exposing Git remotes. On Linux, a vanished `/proc` ancestor now fails open for that row instead of emptying the whole cached snapshot.

## Live callers and producers

- Caller: installed `wire dash --web` starts `operator_web::serve`, which calls inventory and topology operations.
- Producer: MCP startup writes `state/wire/leases/mcp-<pid>.json`; the dashboard reads active leases.
- Producer: `session_metadata::process_snapshot` takes one bounded active-PID snapshot; `operator::collect_live_from` merges lease, registry, then inferred facts.
- Producer: `wire group create/invite/join` writes the same group into each selected session home.
- Producer: managed `wire daemon --all-sessions` supervises per-session workers.

## Verification

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- Focused operator, web, CLI, group, and dashboard end-to-end tests.
- Full `cargo test --all-targets --all-features`: exit 0 after review fixes; 679 library tests passed, one expected library ignore, and every enabled integration/stress test passed.
- Dashboard end-to-end test: three live sessions including Goose, exact bilateral pair, one shared group, no full mesh.
- Playwright: desktop and 390 px mobile render, token removed from visible URL, no horizontal overflow, no console errors, assets/API successful.
- Playwright confirmation-race probe: selected session removed during polling, zero link POSTs, actionable notice rendered.
- Installed identity probes: schema v3.2, distinct `codex-cli` and `goose` session sources and session-keyed homes.
- Installed dashboard API and security headers exercised on loopback.
- Provenance Playwright proof: 35 live rows; Codex, Claude, machine-default warnings, repository/branch data, expanded details, selection persistence, desktop and 390 px mobile, no horizontal overflow, zero console errors, zero bad responses.
- Installed 1440 px proof found and fixed 78 px of table overflow: moving Inspect into the Session cell reduced `scrollWidth` from 1172 to the 1094 px frame width; expanding details leaves `scrollLeft: 0`.

## Daemon repair

- Kept managed launchd supervisor PID `4613` and its active worker children.
- Stopped exact unmanaged processes: daemon `79368`, monitors `79416` and `75012`, and monitor wrapper `75010`.
- Postcondition: supervisor alive, `unmanaged_pids: []`, no stale binary or stale unmanaged sessions.
- No session data was deleted.

## Review dispositions

- Kept and fixed: DID-first verified-peer matching; authenticated inventory; local Host/Origin validation; confirmation snapshot across polling.
- Rejected with evidence: group invite replay concern (three-member end-to-end test passes); missing launch authorization (256-bit token already enforced).
- Cut: redundant 660-line implementation plan. Retained the concise design spec.
- Kept and fixed after provenance review: credential-bearing Git remote sanitization, per-row Linux `/proc` race handling, safe missing-PID rendering.
- Rejected with cumulative caller evidence: cutting `identity_descriptor` (live schema-v2 consumer exists); cutting Cursor/VS Code inference (explicit approved harness list); treating Codex like other explicit sources (`codex-cli` covers both CLI and ChatGPT app-server and needs process disambiguation).
- Deferred by AMANALAP: cache TTL, Git config includes/exotic remotes, transient hostname retries, macOS command paths with spaces, zebra-striping polish, and duplicate-Unknown copy.
- Deferred: storage abstraction, cookie redemption, history/retirement, remote machines, extra browser scenarios.

## macOS harness detector repair

- Observed defect: 12 Goose sessions and two Claude Desktop sessions rendered as `Unknown` although their live parent chains reached `Goose.app` or `Claude.app`.
- Root cause: macOS `ps -o comm` truncated executable paths to 16 characters (`/private/var/fol`, `/Applications/Cl`).
- RED: the long-path detector regression classified the test process as its Codex parent instead of Goose; the spoof regression accepted a fake `/tmp/goose` argv path.
- Fix: retain the bounded `ps` ancestry snapshot, then enrich each selected process from the kernel command-name `c` field in the existing bounded `lsof` cwd probe. Never classify from argv.
- GREEN: both macOS parser regressions pass; arbitrary argument mentions still remain unknown; `cargo fmt --check`, Clippy with warnings denied, and 681 library tests pass with one expected ignore.
- Review cycle 1 kept and fixed the argv spoofing blocker and path-with-spaces concern.
- Review cycle 2 kept and removed the unverified first-`txt` ordering and high-volume mapped-image scan by using `lsof`'s command field instead.
- Final review findings cut/deferred: `+c 0` is unnecessary for every supported command (`codex`, `Claude`, `goose`, `Cursor`, `Code`); fail-open diagnostics and generic future descriptor hardening do not affect the observed defect.

## Live daemon-only sessions

- Observed defect: `rusted-butte` was initialized by bare `wire up` and had a live daemon, but the board omitted it because inventory required an MCP lease.
- RED: inventory fixture expected one MCP-backed and one daemon-only row but received only the MCP row.
- Fix: when no active MCP lease exists, accept an initialized, non-retired session whose versioned daemon pidfile names a live PID and does not contradict the home DID. MCP remains the preferred runtime, preventing duplicate rows.
- Daemon-only rows use the daemon pid/start/version, live process cwd for project discovery, `Wire daemon` as the observed runtime, and by-key versus registry identity provenance.
- DID guard mutation check: removing the mismatch filter changed the expected two rows to three; restoring it returned green.
- Review disposition: cut the alleged missing-DID panic because the existing initialized-session gate proves DID and handle before candidate creation; kept and tested DID mismatch rejection; deferred legacy pidfiles with no DID and registry-label coverage.
- Full `cargo test --all-targets --all-features`: exit 0 after the daemon inventory change.

## Recovery note

A browser race probe accidentally ran an older debug binary and linked `agate-starshine` to the `bubbling-kelp` session at `.../9583f4349f98ddea`. The exact bilateral pins were removed immediately with `wire forget-peer` on both homes; verification showed only each session's self-attestation remained. No files were purged.

After the first installed launch, refreshing the clean URL lost the in-memory launch token and left the inventory request unauthorized. The browser now stores the token in per-tab `sessionStorage` before removing it from the visible URL. Installed Playwright proof showed 34 rows before and after reload, no notice, and zero console errors.

## Artifacts

- `src/operator.rs` — live inventory and explicit-home topology operations.
- `src/operator_web.rs` — loopback HTTP server and security boundary.
- `src/session_metadata.rs` — provenance descriptors, Git discovery, identity classification, and bounded process snapshots.
- `assets/operator-dashboard.{html,css,js}` — operator interface.
- `tests/e2e_operator_dashboard.rs` — installed caller-path topology proof.
- `docs/superpowers/specs/2026-08-10-operator-dashboard-design.md` — approved product and architecture boundary.
- `docs/superpowers/specs/2026-08-10-fleet-session-provenance-design.md` — approved local metadata and future fleet boundary.
- `docs/superpowers/plans/2026-08-10-fleet-session-provenance.md` — linted execution plan.
