# Codex thread identity adapter design

## Goal

Give each Codex thread a stable, independent Wire identity without a shared
literal `WIRE_SESSION_ID` override.

## Evidence

Codex exposes a unique `CODEX_THREAD_ID` to tool subprocesses. It does not
expose the `CODEX_SESSION_ID` name Wire currently reads. The fixed user-level
`WIRE_SESSION_ID` therefore wins for every concurrent Codex process and
collapses them onto one Wire home.

## Selected change

Add `CODEX_THREAD_ID` to `resolve_session_key` immediately after the existing
`CODEX_SESSION_ID` adapter. Both names report the `codex-cli` source label.
Keep `WIRE_SESSION_ID` as the highest-priority operator override and preserve
all other host adapters and fallback behavior.

This makes the live Codex caller work without launcher interpolation or
process/rollout-file discovery. After deploying the change, remove the fixed
user-level `WIRE_SESSION_ID`; existing sessions keep their current identity
until restarted, while fresh Codex threads resolve from their thread ID.

## Rejected approaches

- Map the thread ID in static Codex configuration. Static TOML cannot expand a
  per-thread runtime value reliably for MCP children.
- Discover the thread through rollout filenames or process ancestry. That adds
  filesystem timing and host-internal coupling when a stable environment value
  already exists.

## Safety and compatibility

- Never print or persist the raw thread ID outside the existing hashed session
  home mapping.
- Reject empty and unexpanded `${...}` values through `valid_session_key`.
- Existing explicit `WIRE_SESSION_ID`, Claude Code, Copilot, and VS Code
  precedence remains unchanged.
- The live config override is removed only after the compatible binary is
  installed.

## Verification

- A focused test must fail before implementation because two distinct
  `CODEX_THREAD_ID` values currently resolve to no Codex adapter.
- Focused session tests must cover distinct IDs, `CODEX_SESSION_ID` precedence,
  `WIRE_SESSION_ID` precedence, and placeholder rejection.
- Run `test-env/run.sh`, GitNexus change detection, protected GitHub checks, and
  an isolated live probe with the fixed override absent from the probe only.
- After deployment and config correction, fresh Codex sessions must resolve to
  distinct by-key homes and `wire doctor` must clear the literal-override
  warning.
