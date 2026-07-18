# Session log — 2026-07-17

## Objective

Fix Wire multi-session daemon and identity failure modes while preserving the
live launchd-managed installation. Work occurs on
`fix/multi-session-daemon-lifecycle` in an isolated worktree from `main`.

## Safety boundary

No service install/reinstall, `wire upgrade`, daemon hand-start, launchd
restart, wildcard process kill, dotfile edit, or live-home rewrite. Live checks
are read-only and redact session keys, relay tokens, private keys, and raw
environment values. Tests use temporary forced homes.

## Baseline evidence

Captured before source edits:

- `wire service status`: daemon launchd unit loaded from
  `~/Library/LaunchAgents/sh.slancha.wire.daemon.plist`.
- `launchctl print`: `/Users/laul_pogan/.cargo/bin/wire daemon
  --all-sessions --interval 5`; supervisor PID owned 495 children.
- Process snapshot: 559 `wire daemon` processes, aggregate 4,538,096 KiB RSS;
  556 launched from `.cargo/bin`, 3 from `.local/bin`; 24 children already
  reparented to PID 1.
- Both on-disk binaries report Wire 0.17.0, so same-version duplicates exist.
- Session root: 677 by-key homes, all 677 containing private keys; 78 retired;
  676 with sync timestamps in seven days and 592 in one day.
- Pidfiles: 565 live daemon PIDs; recorded versions include 84 at 0.16.0 and
  593 at 0.17.0. Recorded binary paths include 673 `.cargo/bin` and 4
  `.local/bin`. Version-skewed and same-version processes therefore coexist.
- MCP pidfiles: 167 total, 41 live.
- Local relay TCP `127.0.0.1:8771`: connection refused. launchd local-relay
  label absent.
- Daemon log: 1,540,032 lines, including 451,421 timeout matches and 330,475
  connection-refused matches.
- Current identity reports schema v3.2, valid suffixed DID, and session source
  `override`; no raw override value was printed.
- Baseline isolated `cargo test --lib`: 645 passed, 0 failed, 1 ignored.

## Root causes

1. `supervisor_eligible` gives every private-key home permanent eligibility.
   Since every historical home is initialized, the idle filter is bypassed.
2. `fs_last_active` reads daemon-written sync files. Spawned workers refresh
   their own activity signal, making historical homes self-perpetuating.
3. Supervisor has backoff but no hard worker cap or queue. Restart adopts live
   pidfiles without retiring now-ineligible legacy workers.
4. MCP unconditionally calls per-home daemon startup even when an all-session
   supervisor exists, bypassing global orchestration.
5. Service renderers hardcode interval 5 and reinstall from scratch.
6. Doctor treats large all-session fan-out as legitimate, lacks lease/home,
   override collision, PATH/service shadow, stale MCP, and local-relay service
   checks, and includes a mutating endpoint-heal check.
7. Local-sister dial writes caller trust/relay state before its first relay POST
   and returns after `pair_drop`, leaving verification dependent on later daemon
   pull/ack timing.

## Decision

Use persisted live-owner leases and a capped fork supervisor. Default cap 16;
MCP lease TTL 90 seconds with 30-second heartbeat. Keep process isolation for
now; a single multiplexed process requires converting process-global
`WIRE_HOME` APIs and belongs in a follow-up architecture change. Preserve
identity-bearing homes; prune only safe husks. Classify operator configuration
separately in doctor. Do not edit dotfiles; emit a precise handoff for the
literal Codex override.

## Artifacts

- `docs/superpowers/specs/2026-07-17-multi-session-daemon-lifecycle-design.md`
  — selected architecture and invariants.
- `docs/superpowers/plans/2026-07-17-multi-session-daemon-lifecycle.md` — TDD
  implementation and verification plan.
- `SESSION_LOG_2026_07_17.md` — durable evidence, decisions, commands, and final
  results.

## Iteration 1 — lifecycle leases

- RED: `cargo test session_lifecycle::tests --lib -- --test-threads=1` failed
  with eight missing lifecycle symbols.
- GREEN: five lease unit tests pass: persistence, expiry/dead owner, heartbeat
  restart read, conservative pruning, and clean guard removal.
- RED: `cargo test --test session_lifecycle -- --test-threads=1` failed because
  MCP published no lease.
- GREEN: MCP process integration passes with forced temporary home; lease exists
  for process lifetime and disappears on clean stdin EOF.
- MCP now writes the lease before daemon orchestration and renews it from the
  existing watcher thread every 30 seconds. No raw session key is persisted.

## Iteration 2 — bounded supervisor

- RED: planner tests failed before `plan_supervisor_sessions` existed.
- GREEN: 1-, 10-, and 655-home fixtures pass. A 655-home fixture selects at
  most 16 workers; 650 stale homes remain inactive; retirement wins over a
  live lease; planning is stable across restart.
- Deleted private-key and daemon-sync-time eligibility. Registry binding, live
  lease, or pending outbox now establishes eligibility.
- MCP processes publish leases. Daemon workers deliberately do not: worker-
  generated activity would recreate the self-perpetuating eligibility bug.
  MCP skips direct daemon startup when a live all-session supervisor owns
  lifecycle.
- Supervisor reports selected/queued/inactive/retired counts, uses bounded
  exponential crash backoff, and validates exact JSON pidfile-owned workers
  before targeted SIGTERM. Logs distinguish current-version and skewed workers.
  No process-family signal is used.

## Iteration 3 — durable service options

- RED/GREEN parser tests cover existing launchd plist, systemd unit, and Task
  Scheduler XML plus first-install defaults and zero rejection.
- `wire service install --interval N --max-workers N` now configures the daemon
  service. Omitted flags preserve installed values; only first install uses
  5 seconds and 16 workers. Every platform renders both flags.
- Local-relay service arguments remain unchanged. No service command ran on the
  live machine.

## Iteration 4 — read-only doctor

- Added classified verdicts: `wire_defect`, `operator_config`, and
  `runtime_health`.
- Added supervisor fan-out/stale-home, fixed launcher override, PATH/service
  executable shadow, stale MCP version, and local-relay health checks.
- Production doctor no longer calls the endpoint mutation helper; endpoint
  repair is reported, never applied.
- Read-only branch doctor against the live machine reported: 563 live session
  workers over cap 16 across 681 homes; 681 homes with no lease; one fixed
  Codex `WIRE_SESSION_ID` launcher override while 43 Wire MCP processes run;
  PATH/service executable mismatch relative to the worktree build; 39 live MCP
  pidfiles with no current skew; local relay absent and connection refused.
  It made no service or home changes.

## Iteration 5 — local relay preflight and bilateral convergence

- RED: direct local-sister dial returned `drop_sent`; relay-down dial mutated
  initiator state before connection refusal; send returned a generic transport
  error.
- GREEN: relay health is checked before trust/relay mutation. Failure says no
  pairing state changed and points to local-relay service status.
- A local-sister dial now performs a guarded reverse one-way dial, pulls both
  pair events/acks synchronously in isolated forced homes, and verifies
  `VERIFIED` on both sides before returning `status: verified`.
- Local-scope send failures identify the local relay and state that send state
  did not change.
- Focused end-to-end tests pass for bilateral convergence without manual pull,
  direct delivery after pairing, unavailable relay with no half-pair state,
  three-session local-only mesh, and pair-all-local idempotence.

## Isolated runtime measurement

`cargo test --test supervisor_bounded -- --test-threads=1 --nocapture` created
655 initialized but stale homes and started the real all-session supervisor
twice with a cap of four. Restart 0: 0 children, 12,336 KiB RSS. Restart 1:
0 children, 12,608 KiB RSS. The live installation remained unchanged.

## Canonical gate repair

- The first complete `test-env/run.sh` attempt passed formatting, clippy, and
  every Rust test, then Cargo 1.88 lost a release bytecode path in the shared
  Docker target volume. A release-only build succeeded on a fresh task-scoped
  volume, while fresh full-gate builds reproduced missing incremental and
  fingerprint paths on both volume names.
- `CARGO_INCREMENTAL=0` made the same failed clippy cache pass immediately.
  The test image now disables incremental compilation, matching CI's
  disposable job workspaces and avoiding Docker named-volume rename races.
- The full gate then exposed a pre-existing fixture gap in
  `100-fleet-link.sh`: the minimal Debian image had neither Linux machine-id
  path, so Wire correctly failed closed. The image now supplies a deterministic
  non-secret machine-id fixture. Focused fleet-link then passed all assertions.
- One generated, unmounted, corrupt `wire-testenv-target` cache volume was
  removed and recreated. No source, service state, or identity state lived in
  that volume.

## Final verification

- `test-env/run.sh`: exit 0. Formatting and clippy passed; Rust unit and
  integration targets passed serially; the 655-home restart test passed;
  release build and both demos passed; shell integration suite reported
  11 passed, 0 failed.
- Focused lifecycle, supervisor, relay-down, bilateral local-sister, mesh, and
  fleet-link tests passed before the full gate.
- Test subprocess audit confirmed temporary `WIRE_HOME` helpers set
  `WIRE_HOME_FORCE=1`; `tests/it/lib.sh` exports it for shell fixtures.

## Final live read-only snapshot

Captured after verification, before push: 575 `wire daemon` processes,
4,596,528 KiB aggregate RSS, 52 `wire mcp` processes, 681 by-key homes, and no
new-format live lease files because the deployed binary predates this branch.
The daemon launchd unit remains loaded. Local relay `127.0.0.1:8771` still
refuses connections. Counts grew during the session under the unchanged legacy
service, confirming that no live remediation or restart occurred.

## Caller-side handoff

Do not change this repo to compensate for the launcher collision. In
`/Users/laul_pogan/.codex/config.toml`, caller Codex configuration currently
sets one literal `WIRE_SESSION_ID` under `[shell_environment_policy.set]`.
Remove that fixed override and have the Codex launcher/session adapter pass a
stable unique `CODEX_SESSION_ID` (or unique `WIRE_SESSION_ID`) per concurrent
session. Wire doctor now classifies this as `operator_config` and warns when
concurrent Wire MCP processes share the fixed launcher identity.

## Final review

- Build-loop cross-provider review was attempted twice through the read-only
  Claude subscription reviewer: once with the complete 137 KiB diff (300s),
  then with the 84 KiB executable-only diff (420s). Both timed out without a
  verdict or findings. No reviewer approval is claimed; no third retry was
  made.
- Manual review covered every critical diff named by GitNexus. It found one
  lifecycle edge: MCP heartbeat shared the inbox watcher thread's early-return
  path, so a watcher initialization failure could let a live MCP lease expire.
  The watcher is now best-effort and retries while the same thread continues
  the authoritative heartbeat. Focused MCP tests passed after the correction.
- GitNexus impact for `mcp::run` was LOW with no upstream dependants. Final
  staged/compare change detection and the full canonical gate were rerun after
  the correction before commit.
