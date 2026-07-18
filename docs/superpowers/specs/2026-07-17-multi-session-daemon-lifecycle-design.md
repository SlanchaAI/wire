# Multi-session daemon lifecycle design

## Goal

Keep Wire responsive across hundreds of historical session homes without
reactivating stale identities, and make identity, binary, relay, and pairing
failures explicit and recoverable.

## Constraints

- Never use a private key, daemon-written sync timestamp, or historical home
  existence as proof that a session is live.
- Keep launchd-managed production state untouched during development and test
  only under forced, temporary `WIRE_HOME` roots.
- Preserve keys, inboxes, outboxes, and trust state. Automatic pruning may
  delete only identity-less, unbound, inactive husks.
- Bound worker count even when every discovered home is eligible.
- Keep service configuration stable across reinstall unless the operator passes
  an explicit replacement value.
- Diagnose raw session-key collisions without printing session-key values,
  relay tokens, private keys, or full process environments.

## Considered approaches

1. **Persisted leases plus capped worker processes (selected).** Keep current
   process isolation, replace identity-based eligibility with leases and
   deliberate registry bindings, cap workers, and expose overflow as a queue.
   This is compatible with existing global `WIRE_HOME` code and can ship with
   surgical changes.
2. **One multiplexed daemon process.** Pass an explicit home/context through all
   config, relay, trust, and cursor APIs and service every session from one
   runtime. Best long-term RSS, but the present code relies on process-global
   `WIRE_HOME`; converting it safely is a separate architectural project.
3. **One launchd job per session.** Let launchd own every worker. This moves but
   does not solve stale eligibility, creates hundreds of service definitions,
   and makes cross-platform parity worse.

## Lifecycle model

Each live long-running session owner writes a versioned lease under its own
`state/wire/leases/` directory. A lease contains role, PID, heartbeat and expiry
times, Wire version, executable path, and session-source label. It never stores
the raw session identifier. MCP acquires a lease after bootstrap, renews it from
its existing watcher thread, and removes it on clean shutdown. Crashed owners
leave a bounded stale record which expires and is pruned.

Supervisor eligibility becomes:

- never retired;
- initialized; and
- registry-bound, carrying an unexpired live-owner lease, or holding pending
  outbound work.

Missing lifecycle state means inactive during migration. A private key and
daemon-generated `last_sync` do not make a home eligible. Retired and expired
states survive supervisor restart because markers and lease expiries live on
disk. Identity-less unbound husks retain the existing conservative age-based
pruner; identity-bearing homes are never auto-deleted.

## Supervisor bounds and recovery

Default worker cap is 16. `--max-workers` configures it. Selection is stable:
registry-bound sessions first, then live leases ordered by newest expiry, then
pending-outbox sessions. Remaining eligible sessions appear in an observable
queue. Existing verified per-session daemons count toward the cap.

On restart, the supervisor adopts eligible live workers by pidfile. It
terminates only a precisely validated daemon belonging to an ineligible or
overflow session: live PID, daemon role, and matching home. It never kills a
process family or wildcard. Rapid child failures retain exponential backoff;
queue admission never exceeds the cap. State snapshots expose discovered,
eligible, running, queued, stale, retired, cap, binary version, and executable
path counts.

MCP writes its lease before daemon startup. When a live all-session supervisor
exists, MCP relies on it instead of spawning a bypass worker. Without a
supervisor, the existing single-home fallback remains.

## Doctor

`wire doctor` remains read-only and adds checks with an explicit classification:
`wire_defect`, `operator_config`, or `runtime_health`.

- Supervisor fan-out, cap overflow, queue, stale/retired homes, and legacy homes
  lacking lifecycle state.
- Concurrent inbox owners sharing the current home. When the session source is
  the universal override, report a likely literal `WIRE_SESSION_ID` collision
  and prescribe distinct launcher session IDs. Never print the value.
- PATH candidates, installed service executable, live daemon/MCP executable
  paths, pidfile versions, and stale/dead MCP pidfiles.
- Local relay TCP/health state plus installed-service state.
- Same-version duplicates separately from version-skewed processes.

Existing endpoint-userinfo repair is removed from doctor; diagnostics do not
mutate configuration.

## Service configuration

Daemon service install accepts `--interval` and `--max-workers`. Omitted values
are parsed from the existing launchd plist, systemd unit, or Windows task; only
a first install uses defaults (5 seconds, 16 workers). Generated units always
carry both values. Install output reports effective settings.

## Relay preflight and local pairing

Local-sister pairing checks the selected local relay before any trust or relay
state mutation. Failure names the endpoint, states that no pairing state
changed, and points to `wire service status --local-relay` / install recovery.
Send preserves endpoint failover; when all attempted endpoints are local and
down, it returns the same precise local-relay classification.

After a successful local pair-drop, Wire synchronously runs the existing pull
state machine for the sister home and then the caller home. Both sides therefore
consume signed pair/drop-ack events and reach effective `VERIFIED` before dial
returns. Every spawned Wire child pins `WIRE_HOME` and `WIRE_HOME_FORCE=1` and
removes inherited session identifiers.

## Verification

- Unit fixtures for 1, 10, and 655 homes; mostly stale; restart persistence;
  expired/crashed leases; worker cap and queue; relay failure; service option
  preservation; collision/path/version classification.
- Bilateral isolated local-relay integration: one dial, both effective tiers
  `VERIFIED`, direct message delivery, no manual pull.
- Audit every test helper that spawns `wire`; temporary homes must also set
  `WIRE_HOME_FORCE=1`.
- Focused serial tests, `cargo fmt`, `cargo clippy`, `test-env/run.sh`,
  GitNexus change detection against `main`, isolated 655-home runtime process
  measurement, read-only branch-binary doctor against the live machine, diff
  review, and independent semantic review.
