# Session log — 2026-08-10

## Goal

Repair local Wire identity/daemon ambiguity and ship a one-machine operator dashboard for live agent sessions. The dashboard may view sessions, link exactly two, and create one shared group. Remote machines, retired-session management, history, and messaging remain deferred.

## Root causes

- Bare shell and background monitor processes used the `machine-default` identity because no session key reached them.
- One standalone daemon ran outside the managed all-session supervisor.
- Goose exposed its thread identity through `AGENT_SESSION_ID`, but Wire did not recognize that source.
- Existing MCP leases lacked enough safe metadata to render agent host, project, and uptime.

## Changes

- Resolve guarded Goose sessions when `AGENT=goose`; preserve Codex precedence and scrub adapter identity variables from child commands.
- Record lease acquisition time and working directory while remaining compatible with old leases.
- Collect only live, initialized, non-retired MCP leases. Expose no raw thread ID, token, or private path.
- Add explicit-home local pair and shared-group operations with postcondition checks.
- Add `wire dash --web [--no-open]`: loopback-only Axum server, per-launch 256-bit token, authenticated inventory and mutations, local Host/Origin checks, CSP/security headers, and confirmation-race protection.
- Add the Open Band operator UI with Codex, Claude, and Goose thread labels.

## Live callers and producers

- Caller: installed `wire dash --web` starts `operator_web::serve`, which calls inventory and topology operations.
- Producer: MCP startup writes `state/wire/leases/mcp-<pid>.json`; the dashboard reads active leases.
- Producer: `wire group create/invite/join` writes the same group into each selected session home.
- Producer: managed `wire daemon --all-sessions` supervises per-session workers.

## Verification

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- Focused operator, web, CLI, group, and dashboard end-to-end tests.
- Full `cargo test`: exit 0 after review fixes; 666 library tests passed, one expected library ignore, and every enabled integration test passed.
- Dashboard end-to-end test: three live sessions including Goose, exact bilateral pair, one shared group, no full mesh.
- Playwright: desktop and 390 px mobile render, token removed from visible URL, no horizontal overflow, no console errors, assets/API successful.
- Playwright confirmation-race probe: selected session removed during polling, zero link POSTs, actionable notice rendered.
- Installed identity probes: schema v3.2, distinct `codex-cli` and `goose` session sources and session-keyed homes.
- Installed dashboard API and security headers exercised on loopback.

## Daemon repair

- Kept managed launchd supervisor PID `4613` and its active worker children.
- Stopped exact unmanaged processes: daemon `79368`, monitors `79416` and `75012`, and monitor wrapper `75010`.
- Postcondition: supervisor alive, `unmanaged_pids: []`, no stale binary or stale unmanaged sessions.
- No session data was deleted.

## Review dispositions

- Kept and fixed: DID-first verified-peer matching; authenticated inventory; local Host/Origin validation; confirmation snapshot across polling.
- Rejected with evidence: group invite replay concern (three-member end-to-end test passes); missing launch authorization (256-bit token already enforced).
- Cut: redundant 660-line implementation plan. Retained the concise design spec.
- Deferred: storage abstraction, cookie redemption, history/retirement, remote machines, extra browser scenarios.

## Recovery note

A browser race probe accidentally ran an older debug binary and linked `agate-starshine` to the `bubbling-kelp` session at `.../9583f4349f98ddea`. The exact bilateral pins were removed immediately with `wire forget-peer` on both homes; verification showed only each session's self-attestation remained. No files were purged.

## Artifacts

- `src/operator.rs` — live inventory and explicit-home topology operations.
- `src/operator_web.rs` — loopback HTTP server and security boundary.
- `assets/operator-dashboard.{html,css,js}` — operator interface.
- `tests/e2e_operator_dashboard.rs` — installed caller-path topology proof.
- `docs/superpowers/specs/2026-08-10-operator-dashboard-design.md` — approved product and architecture boundary.
