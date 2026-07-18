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
