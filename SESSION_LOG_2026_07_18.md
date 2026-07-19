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
- Doctor's aggregate `supervisor_fanout` check reported 53 live workers while
  process ancestry showed only 12 managed children. That discrepancy was not
  old MCP-owned workers; the post-merge audit below traced it to stale daemon
  pidfiles whose numeric PIDs had been reused by unrelated live processes.
  No broad process kill or extra service restart was used.

## Missed runtime-test caller

`test-env/runtime-210.sh` still invoked the removed positional form
`wire init seed --offline`. Before correction its runtime gate reported 1 pass
and 10 failures with uninitialized identities. Changing that caller to
`wire init --offline` produced 11 passes and 0 failures in the isolated Docker
XDG root. This one-line follow-up and this final rollout record are carried on
`fix/codex-thread-rollout-record` for a second protected merge.

PR #369 merged that follow-up through 14 green protected checks as
`80b271e90011a44eec3082fb9db1222acfebe91c`.

## Post-merge doctor topology correction

The final installed-binary audit found launchd and the local relay healthy but
`wire doctor` emitted two false daemon failures:

- `supervisor_fanout` counted 50 "live workers" from historical daemon
  pidfiles by checking only whether each numeric PID existed. Exact command
  roles and ancestry showed one launchd supervisor plus 11 child daemons; old
  pidfile numbers had been reused by unrelated processes.
- The current inherited-literal session had been retired by the bounded
  supervisor, leaving its per-session pidfile stale. The legacy single-session
  doctor path then labeled every healthy supervisor child an orphan, even
  though all 11 children had the supervisor as parent and no unmanaged daemon
  existed.

GitNexus classified `check_daemon_health` and
`check_daemon_pid_consistency` LOW risk (doctor-only diagnostics, no indexed
upstream callers). `check_supervisor_fanout` was absent from the graph; direct
source inspection found only `cmd_doctor` as caller, so risk remained confined
to the diagnostic surface.

TDD added four topology regressions: actual daemon-role counting, isolation
from another WIRE_HOME, healthy-supervisor precedence over a retired session
pidfile, and unmanaged-daemon failure. RED failed on missing helpers/signature;
GREEN passed 27 doctor unit tests. Formatting, clippy, the 655-stale-home
restart test, and a live candidate doctor run passed. Candidate doctor reported
11 workers within cap 16 across 719 homes, supervisor + 11 children, no
unmanaged daemons, consistent retired pidfile handling, and zero FAIL results.
The canonical `test-env/run.sh` gate then passed end-to-end: 657 library tests,
all serial Rust targets, release build, demos, and 11/11 integration scripts.
No service mutation occurred during this audit.

PR #370 merged the topology correction through 14 green protected checks as
`b6a6d490610705d98a6d9a40c878ca46726df192`. The merged and reviewed trees
matched. Installed release SHA-256
`569ae2eb7b8ce356bc05d43930d4459a88c19221a58935ad368ca42d1991e28b`
atomically and preserved the prior adapter binary at
`~/.cargo/bin/wire.pre-doctor-topology-20260718`; launchd was not restarted.

The installed audit then exposed one last lifecycle-policy mismatch:
`sync_freshness` warned that an intentionally inactive identity's loop might be
wedged and recommended reactivation. The supervisor correctly had no worker
for that home, no lease was live, and no event was queued. GitNexus rated the
doctor-only change MEDIUM (five direct diagnostic/test callers, no execution
flows). TDD added an inactive-identity verdict: stale sync now PASSes only when
the supervisor is healthy, the current registered home has no cwd binding or
live lease, and nothing is queued. Queued work and explicitly live identities
retain the existing stale-sync WARN/FAIL behavior. The live candidate changed
that warning to: `identity inactive with no queued events; supervisor correctly
leaves its worker retired`. Final `test-env/run.sh` passed 659 library tests,
all serial Rust targets, the release demos, and 11/11 integration scripts.

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

## GitHub Actions usage reduction

### Diagnosis

GitHub's organization billing API showed high gross-equivalent Actions usage
but zero net charges for Wire: $44.37 gross / $0 net in May, $66.34 / $0 in
June, and $8.20 / $0 through July 18. The public repository's standard runner
usage and storage were fully discounted.

The volume was real:

- May: 699 workflow runs, including 404 CI runs, 177 Fly deploys, and 99
  release runs. June: 671 runs, including 460 CI and 178 Fly deploys.
- Current CI launched twelve jobs per run and built the same Linux release
  binary independently in seven jobs. Twenty-five July CI runs created 300
  jobs; pull-request checks repeated after every merge on `main`.
- Fifty-five job/ref-specific Rust caches occupied 11.76 GB.
- GitHub retained 684 Actions artifacts / 2.30 GB. Six hundred eighteen were
  six-platform temporary handoffs from 101 releases, duplicating durable
  GitHub Release assets. Nightly relay backups accounted for only 142 MB.

Selected design and implementation plan:

- `docs/superpowers/specs/2026-07-18-github-actions-cost-reduction-design.md`
- `docs/superpowers/plans/2026-07-18-github-actions-cost-reduction.md`

The pre-change structural assertion failed as expected with
`missing linux-e2e`, proving the approved consolidated job and one-day release
handoff policy were absent before implementation.

### Decisions and changes

- Pull requests now run six protected checks: `test`, `fmt`, `clippy`,
  `docs-lint`, `linux-e2e`, and `install-smoke-windows`. Main pushes run only
  Linux and Windows cache warmers.
- The Linux end-to-end job builds the release binary once, then runs the invite,
  one-command demo, five-iteration hello-world, five-agent mesh, CLI integration,
  fresh-user/nuke, and installer callers serially. Linux and Windows smoke jobs
  set `WIRE_HOME_FORCE=1` at job scope.
- Rust caches use stable platform shared keys. Pull-request jobs restore but do
  not save; main warmers save and cannot be cancelled before their post-job
  cache upload. Superseded pull-request runs still cancel by ref.
- Temporary six-platform release handoffs now expire after one day; tag and
  publish jobs restore but do not save Rust caches. Durable release assets are
  unchanged.
- Fly deploy skips paths the production Dockerfile does not consume. Nightly
  backups retain 90 days; their storage comment now describes the public-repo
  discount precisely.
- `require-ci.sh` contains the six replacement contexts but was not executed.
  Branch protection must stay unchanged until a pull request proves the new
  check names green.

At July's observed mix of 15 pull-request and 10 main CI runs, the new topology
would launch 110 jobs instead of 300 (63% fewer). Release-binary build
invocations fall from seven per CI run (175 at that mix) to one per run (25,
86% fewer). These are topology projections; actual post-merge usage still needs
measurement from GitHub.

### Verification and review

- `actionlint .github/workflows/*.yml`: pass.
- Structural policy assertion: pass with eight jobs, six protected pull-request
  checks, one Linux release build, forced homes, one-day handoffs, expected Fly
  exclusions, and exact protection contexts.
- Focused review regressions ran RED then GREEN for pull-request-only
  cancellation and missing docs-lint inputs. The current docs-lint body also
  ran successfully against the repository.
- `test-env/run.sh`: pass. This covered formatting, clippy, 658 passing library
  tests (one ignored), 72 CLI tests, serial end-to-end suites, release build,
  demos, and all 11 CLI integration scripts. Tests ran inside Docker, not the
  live Wire home.
- Cross-provider review ran three bounded cycles. Accepted findings prevented
  main cache-warmers from being cancelled and made missing docs inputs fail
  closed. Final review returned no blocker or major findings. Remaining minors
  describe preserved demo-pipeline behavior; script inspection confirmed each
  demo/integration caller uses isolated state.
- GitNexus compare against `origin/main`: low risk, zero affected runtime
  processes. `git diff --check`: pass.

Live launchd services, Wire homes, branch protection, existing caches, existing
artifacts, Fly state, releases, and deployments were not changed.

### Artifacts

- `.github/workflows/ci.yml` — bounded PR checks and main cache warmers.
- `.github/workflows/release.yml` — one-day temporary handoffs and restore-only
  release caches.
- `.github/workflows/fly-deploy.yml` — non-image path exclusions.
- `.github/workflows/backup-relay-state.yml` — accurate storage-cost note.
- `require-ci.sh` — post-validation protection migration caller.
- `docs/superpowers/specs/2026-07-18-github-actions-cost-reduction-design.md` —
  approved design.
- `docs/superpowers/plans/2026-07-18-github-actions-cost-reduction.md` — execution
  and verification plan.
