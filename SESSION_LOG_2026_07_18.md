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

## Codex adapter merge and deployment

- PR #368 merged through branch protection as
  `4b9985c68d57fe5a87ad37691d499496245a8071`. The first merge attempt was
  rejected because `main` advanced with the Gemini plugin manifest. Merged the
  new base into the branch, reran the focused adapter test and full canonical
  gate, then waited for all replacement checks. Linux, Windows, integration,
  demos, docs, and CodeRabbit review passed.
- Verified the branch tree and merged `main` tree were identical before the
  release build. Installed SHA-256
  `6cb24f98d42352e804a2d07e8a80b984e2ae4532234e6206b495733f530e2f44`
  atomically at `~/.cargo/bin/wire`; candidate, staged, and installed hashes
  matched. Preserved the preceding binary at
  `~/.cargo/bin/wire.pre-codex-thread-20260718`.
- Backed up Codex configuration byte-for-byte at
  `~/.codex/config.toml.pre-wire-session-fix-20260718`, preserving mode and
  timestamps, then removed only the fixed Wire identity override block from
  the app-managed config. `codex --version` parsed the resulting config and the
  override key is absent. No shell dotfile changed.
- Existing Codex/MCP processes retain their inherited literal until exit.
  `wire doctor` therefore still emits the correct operator-configuration
  collision warning for those old processes; this is not evidence that the
  config edit failed. Fresh Codex processes inherit `CODEX_THREAD_ID` and the
  merged Wire binary resolves it as `codex-cli`.

## Final runtime measurements

- 13 daemon processes (one launchd supervisor plus 12 child workers, below the
  configured cap of 16), 47 inherited MCP processes, and 525,344 KiB combined
  daemon+MCP RSS.
- 719 historical by-key homes; doctor reports 707 without active lifecycle
  signals. They remain inactive rather than becoming supervisor children.
- Local relay TCP healthy; launchd daemon unit loaded; PATH/service binary
  consistent; no reported MCP version skew.
- Doctor's aggregate `supervisor_fanout` check still counts old MCP-owned
  workers and reports 53 live workers. The managed supervisor itself owns only
  12 children. No broad process kill or extra service restart was used; old
  MCP-owned workers age out with their Codex sessions.

## Missed runtime-test caller

`test-env/runtime-210.sh` still invoked the removed positional form
`wire init seed --offline`. Before correction its runtime gate reported 1 pass
and 10 failures with uninitialized identities. Changing that caller to
`wire init --offline` produced 11 passes and 0 failures in the isolated Docker
XDG root. This one-line follow-up and this final rollout record are carried on
`fix/codex-thread-rollout-record` for a second protected merge.

## Artifacts

- `docs/superpowers/specs/2026-07-18-codex-thread-identity-design.md` — approved
  caller adapter design.
- `docs/superpowers/plans/2026-07-18-codex-thread-identity.md` — implementation,
  merge, and rollout plan.
- `SESSION_LOG_2026_07_18.md` — investigation, tests, merge, deployment, and
  before/after measurements.
- `~/.cargo/bin/wire.pre-codex-thread-20260718` — pre-adapter binary rollback.
- `~/.codex/config.toml.pre-wire-session-fix-20260718` — pre-migration Codex
  config rollback.

Caller documentation handoff: the dirty dotfiles worktree was not modified.
Its source file `~/Source/dotfiles-claude/codex/AGENTS.preamble.md` still says a
fixed Wire session override is current. The owner of that worktree should
replace that statement with: Codex exposes `CODEX_THREAD_ID`; Wire v0.17.0+
uses it automatically; do not set one global `WIRE_SESSION_ID`.
