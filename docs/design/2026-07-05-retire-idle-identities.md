# Plan (v2, post gate-1) — retire/revive idle identities + `wire dash --retire-idle`

Date: 2026-07-05 · Tier: **HIGH** (kills daemon processes, touches the cross-cutting supervisor
lifecycle, shared-write to session state) · Branch: `observability-open-band`
Revised after a 4-persona plan review folded 5 BLOCKERs + 8 MAJORs (see end).

## Problem (from research)

`wire dash` surfaced **258 idle solo daemons**. They are NOT husks: each is a *real identity*
(`config/wire/private.key`) that syncs. The supervisor **deliberately keeps a daemon alive for
every real identity** so it can receive mail (`daemon_supervisor.rs:151-203`). So there is no way to
retire an identity you're done with, and killing its daemon is futile — the supervisor respawns it
within one poll (~10s).

## Decisions (operator-ratified)

- **Reversible:** retire = stop daemon + `.retired` marker; keep home/identity/slot. `wire revive`
  undoes. Later `--purge-retired` reclaims disk.
- **Manual only:** `wire retire` / `wire revive` / `wire dash --retire-idle`. **No auto-retire.**
- **CLI-only, no MCP tool.** A destructive host-lifecycle action mirrors `nuke` (also CLI-only);
  per `agentic-action-safety`, an agent must not kill another identity's daemon unsupervised.

## Mechanism (reuses the supervisor reconcile loop; hardened per gate-1)

1. **Marker:** `<home>/state/wire/retired.json` = `{schema:"wire-retired-v1", retired_at, reason}`.
   `is_retired(home)` is a **pure existence check** (`.exists()`, mirrors `fs_has_identity`) — never
   content-dependent, so a torn write can't flip it to "not retired". Any body-parsing (for display)
   **fails closed** (parse error ⇒ still treated as retired).
2. **`supervisor_eligible` — filter retired FIRST, above everything** (fixes B1+B2):
   retired homes are removed at the very top of the function, **before** the `max_idle == None`
   early-return AND **before** the `cwd.is_some()` and `has_identity` branches. So a retired home is
   ineligible in every config (`WIRE_ALL_SESSIONS_MAX_IDLE_DAYS=0`, cwd-bound project, idle identity
   — all of it). Injected `is_retired: H` probe (like the existing `has_identity`) for unit tests.
   The supervisor's existing step-3 then kills its owned child + step-4 never respawns — for free.
3. **`retire` also kills the pid directly, graceful-then-force** (fixes B3): write marker, then
   `platform::kill_process(pid, false)`; if still alive after a short grace, `kill_process(pid, true)`
   — mirroring the `upgrade.rs` #2 fix (bare `taskkill /PID` without `/F` is a no-op for a headless
   Windows daemon). Covers operator-spawned daemons (never in the supervisor's `children` map).
   **Order is marker-first, then kill** (a unit test asserts this) — reverse it and the respawn race
   returns.

## Surface

- `wire retire <handle|fp|key>` — resolve box-wide → guard → marker → kill → report.
- `wire revive <handle|fp|key>` — remove marker → supervisor respawns next poll.
- `wire dash --retire-idle [--older-than <days>] [--dry-run] [--force] [--json]` — bulk.
- `wire dash --retired` — list retired identities (the revive-discoverability answer, B5/M6).
- `wire dash` — retired identities **collapse into a count by default** (a new `retired` predicate,
  NOT `likely_idle` which goes false once the daemon is dead); summary gains a `retired` tally;
  `--all` / `--retired` reveal them.

### Box-wide resolver (M5)

`<handle|fp|key>` resolves over `session::list_sessions()` (NOT the current-session trust resolver):
match if arg == handle, == fingerprint, or == session key/dir name (many idle homes never claimed a
handle → key/fp is the only address). **Zero match → error. Multiple match → error listing them.**

## Hard safety guards (fixes B4, M1, M2, M3, M4)

Selection for `--retire-idle` = daemon running ∧ 0 pinned peers ∧ **no pending inbound pair**
(`pending_inbound_pair` record, M1) ∧ last-active > threshold (default 7d) ∧ not already retired ∧
**not the current identity**.

- **Current identity by HOME PATH, fail closed** (B4): identify "me" by the effective home this
  process resolves for itself (`WIRE_HOME`/session-key/cwd-detect/default chain — the same one
  `detect_session_wire_home` uses), and exclude by **home_dir equality**, not by
  `resolve_session_key()` (which is `None` on a bare terminal). If the current home cannot be
  resolved at all, **refuse** the bulk sweep (fail closed), never proceed with an empty exclusion.
- **Paired = sacrosanct:** `peers.len() > 0` excluded unconditionally on every path.
- **`--force` = skip the typed confirmation ONLY** (M2). It never bypasses the paired/current/
  pending/recent guards, on either the single-target or bulk path. Force-retiring a paired identity
  is out of scope.
- **Dry-run is the DEFAULT for bulk**; the real run **lists every victim handle** (M4, like
  `nuke`), then demands a typed `retire` confirm.
- **Re-check guards per target at kill time** (M3, TOCTOU): if a candidate paired/became-current/
  got a pending request during the confirm latency, **drop it** (don't kill) and report the drop.
- Single-target `wire retire <x>` refuses a paired/current/pending/recent target (message explains
  why); it does not have a guard-bypass — the current identity can never be retired by any flag.

## Reversibility (verified against the relay, gate-1 reviewer 3)

`relay_server.rs:20` — slot tokens **never expire**; `post_event` retains mail regardless of pulls;
the daemon's pull cursor lives in kept `relay.json` → **revive drains the full backlog, identity
identical.** The husk-reaper skips any home with `private.key` (`daemon_supervisor.rs:295-302`) → a
retired-but-identityful home is never swept. So retire is genuinely reversible. Caveat (MINOR): a
slot has a 64MB cap; a peer messaging a long-retired identity could eventually 413 — pre-existing,
surface it in the retired view later, not blocking.

## Files

- **new** `src/retire.rs` — marker read/write/remove, `is_retired`, `retire_session`/`revive_session`
  (kill injected for test), the box-wide resolver. Unit-tested.
- **edit** `src/daemon_supervisor.rs` — `fs_is_retired` + retired-first filter in `supervisor_eligible`
  + tests (retired ineligible with cwd, with identity, and under `max_idle=None`).
- **edit** `src/dash.rs` — `retired: bool` on `SessionSnapshot`; retired excluded from `likely_idle`;
  a `retired` count.
- **edit** `src/cli/dash.rs` — retired collapse + count + `--retired` + `--retire-idle` path.
- **edit** `src/cli/mod.rs` + `src/cli/lifecycle.rs` — `Command::Retire`, `Command::Revive`, new
  `Dash` flags; `cmd_retire`/`cmd_revive`/`cmd_retire_idle`.

## Success criteria (runnable checks)

- `cargo build` + `cargo test --lib` + `cargo fmt --check` + `cargo clippy -D warnings` green.
- Unit: `supervisor_eligible` returns false for a retired home **with cwd**, **with identity**, and
  **under `max_idle=None`**; retire writes marker before kill; revive removes marker; bulk selection
  excludes current(by home)/paired/pending-inbound/recent; resolver errors on zero + ambiguous;
  dash render **collapses retired by default** + `--retired` reveals.
- **In-situ (HIGH):** retire ONE known throwaway idle identity (0 peers, not current) → daemon dies
  AND stays dead across >1 supervisor poll (~20s, generous for backoff, M-minor); `wire dash` shows
  it retired (collapsed) + `--retired` lists it; `revive` brings it back; current session + all
  paired identities untouched; pid-count drops by exactly 1 (no cascade). Reversible round-trip
  proven before the operator touches the 258.

## Explicitly NOT doing / follow-ups

- No auto-retire; no hard-delete; no slot release; no MCP tool.
- Not bulk-retiring the 258 here — operator runs `--retire-idle` after seeing the round-trip.
- `--purge-retired` (reclaim disk; dash still walks retired homes until then) — follow-up.
- **Root-cause follow-up (both critics):** a Claude `SessionEnd` hook that self-retires its own
  identity iff 0 pinned peers at session end — stops the pile growing at the source, no false-positive
  on paired identities. Log after the manual tool ships.

## Gate-1 findings folded

BLOCKERs: B1 retired-check-before-cwd-branch · B2 retired-above-max_idle-early-return · B3 Windows
graceful-then-force kill · B4 current-by-home-path-fail-closed · B5 retired-collapse-in-dash.
MAJORs: pending-inbound exclusion · unified `--force` · TOCTOU re-check · name victims · box-wide
resolver · `--retired` list · MCP-exclusion declared · slot-cap note. MINORs: pure `.exists()`
marker fail-closed · marker-before-kill test · backoff timing note · `--json`/`--watch` handling.
