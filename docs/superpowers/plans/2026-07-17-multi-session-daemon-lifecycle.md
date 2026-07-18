# Multi-session daemon lifecycle implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound multi-session service work using persisted leases, diagnose the
observed machine failures, preserve service tuning, and make local pairing
converge without manual pulls.

**Architecture:** A new lifecycle module owns lease persistence and pure
eligibility classification. The existing fork supervisor consumes that model,
selects at most a configured cap, and exposes its queue. Existing relay and
pair state machines remain authoritative; pairing adds preflight and drives both
pull halves synchronously.

**Tech Stack:** Rust 2024, clap, serde/serde_json, fs2 locks, existing blocking
relay client, launchd/systemd/Task Scheduler renderers, Cargo tests and shell CI.

## Global constraints

- All subprocess tests using temporary Wire homes set both `WIRE_HOME=<temp>`
  and `WIRE_HOME_FORCE=1` and remove inherited session IDs.
- No test or probe writes the live Wire root.
- No launchd restart, service install, daemon hand-start, upgrade, wildcard
  kill, or live-home rewrite.
- Automatic pruning never deletes a private key or identity-bearing home.
- Default worker cap: 16. Default MCP lease TTL: 90 seconds. Heartbeat: 30
  seconds.
- Branch is pushed but never merged.

---

### Task 1: Persisted lifecycle leases

**Files:**
- Create: `src/session_lifecycle.rs`
- Modify: `src/lib.rs`
- Modify: `src/mcp.rs`
- Test: `src/session_lifecycle.rs`

**Interfaces:**
- Produces `LeaseGuard::acquire(role)`, `LeaseGuard::heartbeat()`,
  `active_leases_at(home, now)`, `prune_expired_leases_at(home, now)`, and
  `classify_home(home, session, now)`.
- Lease JSON stores schema, role, PID, heartbeat/expiry RFC3339 timestamps,
  version, executable path, and `session_source`; no raw key.

- [ ] Add tests for acquisition, heartbeat, clean removal, crash/expiry,
  restart reads, dead PID, malformed records, and no secret field; run the
  lifecycle test filter and observe missing-symbol failures.
- [ ] Implement atomic per-PID lease writes and conservative reads/pruning.
- [ ] Acquire before MCP daemon orchestration; heartbeat on the existing watcher
  cadence and drop on clean shutdown.
- [ ] Run lifecycle and MCP unit filters; commit `Add restart-safe session leases`.

### Task 2: Bounded supervisor plan and recovery

**Files:**
- Modify: `src/daemon_supervisor.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/relay.rs`
- Modify: `src/ensure_up.rs`
- Test: `src/daemon_supervisor.rs`

**Interfaces:**
- `run_supervisor(interval_secs, max_workers, as_json)`.
- `SupervisorPlan { selected, queued, inactive, retired }` from a pure planner.
- `SupervisorState` exposes `max_workers`, counts, queued sessions, worker
  versions, and binary paths.

- [ ] Add failing table tests for 1, 10, and 655 homes; 650 stale; cap 16;
  deterministic queue; retired/missing/expired lease; pending outbox; restart;
  crashed child backoff; externally running workers counting toward cap.
- [ ] Remove private-key and daemon-sync eligibility. Add planner and cap.
- [ ] Prevent MCP bypass spawn when supervisor is alive.
- [ ] Add precise validation before targeted stop of stale/overflow daemon
  pidfiles; never wildcard-kill.
- [ ] Run supervisor/ensure-up filters and commit `Bound all-session daemon workers`.

### Task 3: Preserve service limits

**Files:**
- Modify: `src/service.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/upgrade.rs`
- Test: `src/service.rs`

**Interfaces:**
- `DaemonServiceOptions { interval_secs, max_workers }`.
- `wire service install [--interval N] [--max-workers N]`.

- [ ] Add failing render/parse tests for defaults, explicit values, and
  preservation from existing launchd, systemd, and Task Scheduler text.
- [ ] Thread optional CLI overrides into installer and generate both flags on
  every platform.
- [ ] Report effective options without changing local-relay installation.
- [ ] Run service and CLI parsing tests; commit `Preserve daemon service limits`.

### Task 4: Read-only doctor failure classification

**Files:**
- Modify: `src/cli/status.rs`
- Modify: `src/daemon_supervisor.rs`
- Modify: `src/session.rs`
- Modify: `src/service.rs`
- Modify: `src/platform.rs`
- Test: module tests plus `tests/cli.rs`

**Interfaces:**
- `DoctorCheck.classification` is one of `wire_defect`, `operator_config`,
  `runtime_health`.
- Pure verdict helpers accept process/session/service snapshots.

- [ ] Add failing fixtures for 655-home fan-out, duplicate override home,
  distinct homes, stale MCP pid, PATH/service shadow, same-version duplicates,
  version skew, local relay down/absent, and healthy state.
- [ ] Reuse home collision detection; never inspect or render raw session IDs.
- [ ] Add safe executable/PATH/service snapshot helpers and supervisor lifecycle
  counts.
- [ ] Replace doctor endpoint healing with a read-only verdict.
- [ ] Run doctor/CLI tests and commit `Diagnose multi-session failure modes`.

### Task 5: Relay preflight and bilateral local pairing

**Files:**
- Modify: `src/cli/pairing.rs`
- Modify: `src/relay_client.rs`
- Modify: `src/send.rs` or its direct-delivery caller
- Test: `tests/stress_within_system.rs`
- Test: `tests/cli.rs`

**Interfaces:**
- A relay preflight returns a classified actionable error for an unavailable
  local endpoint.
- `add_local_sister_core` returns only after both effective tiers verify.

- [ ] Add failing relay-unavailable test proving trust/relay files unchanged.
- [ ] Add failing bilateral test: one dial, both tiers `VERIFIED`, direct send
  delivered without manual pull.
- [ ] Preflight before mutation; preserve normal multi-endpoint failover.
- [ ] Run sister pull child with `WIRE_HOME_FORCE=1`, then caller pull; verify
  signed ack through existing state machine.
- [ ] Run pairing/send focused tests; commit `Converge local sister pairing`.

### Task 6: Isolation audit and complete verification

**Files:**
- Modify only test helpers missing `WIRE_HOME_FORCE=1`
- Update: `SESSION_LOG_2026_07_17.md`

- [ ] Audit every `Command::new`/`assert_cmd` Wire spawn and add forced-home
  isolation where missing; run affected tests serially.
- [ ] Run `cargo fmt --all -- --check`, focused suites, and
  `cargo clippy --all-targets -- -D warnings`.
- [ ] Build a 655-home isolated fixture, run the supervisor briefly, and record
  supervisor/worker process count and RSS; assert worker count never exceeds 16.
- [ ] Run `test-env/run.sh`.
- [ ] Run GitNexus `detect-changes --scope compare --base-ref main`; inspect all
  affected flows and actual diff.
- [ ] Run the branch binary's read-only doctor against the live host and record
  detected actual classes without exposing secrets.
- [ ] Run one independent read-only semantic review; fix BLOCKER/MAJOR findings
  and rerun deterministic checks.
- [ ] Commit final log/test audit, push branch, and do not merge.
