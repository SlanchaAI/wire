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

## Operator daemon ownership repair

- Observed defect: the machine-wide supervisor retired `rusted-butte` PID `98594`, although `wire up` had started that daemon explicitly. The live-only board then correctly removed the stopped session.
- Root cause: inactive-worker cleanup identified ownership from a shared pidfile and generic `wire daemon` command line; both supervisor children and operator-started daemons have those properties.
- Fix: supervisor-spawned workers carry `WIRE_SUPERVISOR_MANAGED=1` into a backward-compatible pidfile marker. Cleanup requires that explicit marker; operator-started daemons and older pidfiles omit it. The durable marker survives supervisor restarts.
- RED/GREEN: a real orphaned process with an operator-shaped daemon pidfile was killed before the fix and preserved after it; a complementary child-process test proves explicitly supervisor-owned cleanup remains active. Pidfile tests cover owner publication and legacy records without the field.
- Installed caller proof: launchd supervisor PID `20898` spawned workers including `agate-starshine` PID `25241`; their pidfiles contain `supervisor_managed: true`. Operator-started `rusted-butte` PID `32746` omits the marker and remained healthy through repeated supervisor polls.
- Chrome proof: refreshed the existing dashboard tab, found exactly one `rusted-butte` row, and left it visibly centered with `Wire daemon`, project `wire`, and `HEALTHY` status.

## Dashboard responsiveness repair

- Observed defect: `/api/sessions` took 14–25 seconds while the browser requested another scan every two seconds; link confirmation waited behind the same inventory backlog.
- Root cause: this machine has 3,003 historical session homes and 1,739 daemon pidfiles. macOS liveness forked `/bin/kill -0` once per pidfile, so each inventory created roughly 1,739 subprocesses. Concurrent polling multiplied the scan.
- RED/GREEN: 512 self-PID checks took 3.51 seconds and failed the one-second regression ceiling before the fix; the same test took 0.01 seconds after the fix. `wire session list --json` fell from 21.36 seconds to 1.01 seconds.
- Fix: macOS/BSD liveness now invokes `kill(2)` with signal zero in-process and treats permission-denied as proof the PID exists. The browser coalesces poll ticks behind one shared scan promise, so mutation refreshes and interval ticks cannot overlap inventory requests.
- Review cycle 1 kept and added proof that polling resumes after a completed scan. AMANALAP deferred an unobserved never-settling fetch timeout and timing-test hardening; it cut an errno-comment polish item.
- Live caller: `GET /api/sessions` and link/group validation call `collect_live_sessions`; the dashboard JavaScript owns the two-second refresh cadence.
- Persistent Wire monitor: `rusted-butte` monitor session remained armed throughout the repair.

## Topology final proof

- Producer: `operator_topology::collect_topology` reads live inventory, peer records, group rosters, and machine descriptors. Caller: installed `wire dash --web --no-open` PID `50977` served the authenticated loopback dashboard at `http://127.0.0.1:54553`.
- RED: the end-to-end caller linked two sessions, then created a three-session group. `direct_links` rose from one to two because a group-only `introduced_via` trust pin appeared as a one-sided direct edge.
- Fix: `dash::read_peers` retains the non-serialized `introduced_via` provenance. The topology builder excludes group-only verification pins from direct-link observations. Direct pairing still replaces the trust record, and group introduction never marks an existing direct pin.
- GREEN: the end-to-end test now compares `direct_links` before and after group creation, checks all three group members, and preserves the final assertion that the third member is not directly paired.
- Gate: `cargo fmt --check`, Clippy with all targets/features and warnings denied, 23 Node topology/dashboard tests, and `cargo test --all-targets --all-features` passed. The library suite reported 697 passes and one expected ignore; every enabled integration and stress target passed.
- Release install: `cargo build --release` completed in 1m35s. Atomic install used `wire.new` followed by `mv`. The release and installed binaries shared SHA-256 `21ddfe22c985ec7658ea96918bd9435842a94de6bec74a2fdb6069c4e045ff8c`.
- API timing: three fresh installed cold launches returned topology in 1.399s, 1.364s, and 1.367s. Clean Playwright polling made seven requests, peaked at 1.561s, and never exceeded one in-flight request. One probe run immediately after the compile load took 2.085s; fresh cold trials did not repeat it.
- Live browser proof: 63 live sessions; `rusted-butte` and `umber-savanna` rendered once with one bilateral edge. Map was the default; List remained available; two-node selection survived Map/List/Map; action counts and the link confirmation held; Fit returned every machine cluster to view.
- No suitable live group existed. The dashboard created and retained `operator-topology-proof-20260810` for the two named sessions. The group region rendered and `direct_links` stayed at two before and after creation.
- Desktop 1440×900 and narrow 390×844 each had document and body scroll width equal to viewport width. Console errors, failed requests, and HTTP error responses were zero.
- Screenshots: `/tmp/wire-task6-live.E5rTku/desktop-selected.png`, `/tmp/wire-task6-live.E5rTku/desktop-group.png`, and `/tmp/wire-task6-live.E5rTku/narrow-group.png`. Visual inspection found no clipping, blank state, or failed render.
- Review cycle 1 raised a MAJOR concern that `introduced_via` could outlive a later direct pair. Source tracing rejected it: every production pair writer calls `add_agent_card_pin`, which replaces the record; `promote_to_verified` has no production caller. Cycle 2 raised direct-pair-then-group ordering; the exact end-to-end order and `introduce_pin` existing-record branch disproved it. Final AMANALAP review returned no BLOCKER or MAJOR and cut speculative deserialization hardening and duplicate assertions.
- Retrospective proposal recorded only: provenance-classifier review packets should include unchanged producer transition branches. No policy or skill changed, and no proposal was queued.
- Incidental gate repair: Rust 1.95 Clippy rejected `read_dir(&dir)` in `group::list_groups_in`; the semantics-preserving `read_dir(dir)` edit cleared the required warnings-denied gate.
- Persistent Wire daemon PID `20898` and monitor PIDs `17795`, `29336`, and `34962` stayed running. No persistent listener stopped.

## Artifacts

- `src/operator.rs` — live inventory and explicit-home topology operations.
- `src/operator_topology.rs` — sanitized machine, direct-link, group, and anomaly producer.
- `src/dash.rs` — peer provenance read without changing `wire-dash-v1` serialization.
- `src/operator_web.rs` — loopback HTTP server and security boundary.
- `src/session_metadata.rs` — provenance descriptors, Git discovery, identity classification, and bounded process snapshots.
- `assets/operator-dashboard.{html,css,js}` — operator interface.
- `tests/e2e_operator_dashboard.rs` — installed caller-path topology proof.
- `tests/operator_dashboard_polling.test.mjs` — browser polling single-flight regression.
- `docs/superpowers/specs/2026-08-10-operator-dashboard-design.md` — approved product and architecture boundary.
- `docs/superpowers/specs/2026-08-10-fleet-session-provenance-design.md` — approved local metadata and future fleet boundary.
- `docs/superpowers/specs/2026-08-10-operator-topology-map-design.md` — approved clustered map, topology contract, and select-then-link boundary.
- `docs/superpowers/plans/2026-08-10-fleet-session-provenance.md` — linted execution plan.
- `docs/superpowers/plans/2026-08-10-operator-topology-map.md` — task-level TDD plan for the topology producer, authenticated route, shared browser state, native SVG map, and installed proof.
