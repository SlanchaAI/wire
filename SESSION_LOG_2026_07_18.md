# Session log — 2026-07-18

## Objective

Merge and deploy the bounded multi-session lifecycle work, then complete the
caller-side Codex identity correction.

## Merge and deployment

- PR #367 merged to `main` as `67f9e1921b2b6d3f01fc3cd844d1edd8ad2a045e`
  after all 12 protected checks passed.
- Canonical local gate `test-env/run.sh` passed immediately before merge.
- Installed the merged release binary at `~/.cargo/bin/wire` after candidate
  and staged SHA-256 hashes matched. Preserved the previous binary at
  `~/.cargo/bin/wire.pre-daemon-lifecycle-20260718` for rollback.
- Installed the missing launchd local-relay service. It initially remained in
  `xpcproxy` under process pressure, then reached `exec` and accepted TCP after
  the managed daemon restart reduced load.
- Restarted only `sh.slancha.wire.daemon` through launchd. No wildcard kill,
  hand-started daemon, upgrade command, live-home rewrite, or destructive
  cleanup occurred.

## Measurements

- Before rollout: 592 daemons, 4,692,192 KiB aggregate RSS, 41 MCP processes,
  690 session homes, local relay refused connections.
- After managed restart: 60 daemons, about 760,240 KiB aggregate RSS, 41 MCP
  processes, local relay accepting connections.
- Remaining workers comprise legacy MCP-owned children and capped/reparented
  supervisor workers. Existing MCP processes loaded the prior binary and age
  out with their owning Codex sessions.

## Follow-up root cause

The fixed Codex config override cannot yet be removed safely. Live Codex
exposes `CODEX_THREAD_ID` but not `CODEX_SESSION_ID`; Wire reads only the latter.
Without a new adapter, Wire would fall through to a minted or machine-default
identity instead of a stable per-thread identity.

GitNexus impact for `resolve_session_key` is LOW: four direct dependants, six
impacted symbols, and two affected runtime entry flows (`cli::run` and
`mcp::run`). Selected fix is documented in
`docs/superpowers/specs/2026-07-18-codex-thread-identity-design.md`.

## Codex thread adapter implementation

- Added `CODEX_THREAD_ID` immediately after the existing
  `CODEX_SESSION_ID` compatibility adapter. Both resolve with source
  `codex-cli`; explicit Wire and Claude Code overrides retain precedence.
- RED: the focused Codex adapter test failed with `CODEX_THREAD_ID` resolving
  to `None` before the production change.
- GREEN: three session-key adapter tests passed. The isolated lifecycle,
  655-stale-home supervisor, relay-unavailable, and bilateral local-sister
  suites passed; local-sister dial converged without manual pull.
- `cargo fmt --all -- --check` and
  `cargo clippy --all-targets --all-features -- -D warnings` passed.
- Added `CODEX_THREAD_ID` cleanup anywhere an isolated test process already
  removed the older Codex variable. Every temporary `WIRE_HOME` spawn in this
  scope retains `WIRE_HOME_FORCE=1`.

## Canonical gate

- First `test-env/run.sh` attempt passed 653 library tests, then Cargo could
  not execute its just-built zero-test `src/bin/wire.rs` harness because that
  artifact was absent from the shared Docker target volume. Source inspection
  found no test invoking binary purge, no concurrent Wire test container, and
  a Dockerfile note documenting Cargo 1.88 target-path loss on named volumes.
- Rebuilding that single bin-test target in the same container volume passed.
- A second full `test-env/run.sh` run passed end-to-end, including formatting,
  clippy, all serial Cargo targets, release build, demos, and shell integration
  checks.

## Deployment decision

Push and merge through protected checks before changing caller configuration.
Install the merged binary atomically, then remove only the fixed
`WIRE_SESSION_ID` assignment from Codex's app-managed configuration. Preserve
a mode/timestamp-retaining backup; do not touch shell dotfiles. Existing Codex
processes keep inherited identity until they exit; fresh processes use their
thread ID.
