# GitHub Actions cost-reduction design

## Goal

Keep Wire's protected cross-platform behavior coverage while removing repeated
compilation, duplicate post-merge validation, fragmented caches, unnecessary
deploys, and long-lived release handoff artifacts.

## Evidence

- GitHub's billing ledger reports zero net Actions charges because Wire is a
  public repository on standard runners, but gross-equivalent usage reached
  $44.37 in May, $66.34 in June, and $8.20 through July 18.
- May produced 699 workflow runs, including 99 releases and 404 CI runs. June
  produced 671 runs, including 460 CI runs.
- Current CI launches twelve jobs per run. Seven Linux jobs independently run
  `cargo build --release --bin wire`; 25 July CI runs created 300 jobs.
- Fifty-five active Rust caches occupy 11.76 GB because the default cache key
  includes job ID and Git ref. Base-branch caches are available to pull
  requests, but pull-request caches are not reusable by unrelated branches.
- GitHub retains 684 artifacts / 2.30 GB. Six hundred eighteen are temporary
  six-platform release handoffs already duplicated on durable GitHub Releases.
- Branch protection requires the twelve current CI job names. Any job
  consolidation therefore needs a matching `require-ci.sh` context migration.

## Selected architecture

### Pull-request CI

Run six protected jobs on pull requests:

1. `test` — existing serial all-target Rust test gate.
2. `fmt` — Rust formatting.
3. `clippy` — Rust lint.
4. `docs-lint` — existing documentation surface check.
5. `linux-e2e` — one release build followed by the existing demo, hello-world,
   integration, fresh-install, nuke, and installer checks in serial order.
6. `install-smoke-windows` — existing Windows build and smoke behavior.

Serial Linux end-to-end execution is intentional. These tests already use
isolated homes and processes, and serial execution removes seven redundant
release builds without dropping a caller path.

### Main-branch cache warming

Do not rerun the complete protected suite after a checked pull request merges.
On `main`, run one Linux and one Windows cache-warming build. Both write stable
shared caches; pull-request jobs restore those caches but never save
pull-request-scoped copies. This preserves fast fresh PRs without cache fan-out.

Use workflow concurrency keyed by Git ref with `cancel-in-progress: true` so a
new commit cancels obsolete work for the same PR or `main` ref.

### Releases and deployment

- Keep all six release targets and durable GitHub Release assets.
- Set temporary `upload-artifact` handoffs to one-day retention.
- Restore but do not save tag-scoped Rust caches; tag caches have no reusable
  downstream branch.
- Keep nightly relay snapshots at 90 days; they are operational backups and
  account for only 142 MB. Correct the claim that artifact storage is free.
- Ignore deploy triggers for known files absent from the production Docker
  build context: Markdown, docs, skills, hooks, examples, Scoop metadata, and
  Gemini plugin metadata. Source, tests, landing, assets, manifests, Docker,
  Fly config, and unknown future paths still deploy.

## Protection rollout

Update `require-ci.sh` to require `test`, `fmt`, `clippy`, `docs-lint`,
`linux-e2e`, and `install-smoke-windows`. Do not execute that live mutation
until the branch's new checks have run successfully; otherwise the pull request
would be blocked on contexts that do not yet exist.

## Safety boundaries

- Do not delete current Actions artifacts or caches in this change.
- Do not alter release targets, production Fly state, secrets, branch
  protection, or live Wire services during local implementation.
- Preserve every existing Linux and Windows behavioral command.
- Every temporary `WIRE_HOME` caller continues to set `WIRE_HOME_FORCE=1`.
- No new action or runtime dependency.

## Verification

- A structural assertion must fail against current workflows before edits and
  pass afterward, proving six PR jobs, two main warmers, one Linux release
  build, stable restore-only PR caches, one-day release handoffs, deploy
  exclusions, and protection contexts.
- `actionlint` must accept all workflows.
- `test-env/run.sh` must pass the canonical isolated Wire gate.
- GitNexus change detection must report workflow/docs-only scope.
- A read-only independent reviewer must return no BLOCKER or MAJOR finding.
- After push, inspect real GitHub check names and durations. Live protection
  migration and artifact deletion remain separate operator-visible actions.

## Expected effect

For July's observed mix of 15 PR runs and 10 main pushes, job fan-out falls
from 300 jobs to about 110: six jobs per PR and two cache warmers per main push.
Seven Linux release builds per PR become one. PR caches stop multiplying by job
and merge ref. Temporary release handoffs expire after one day instead of 90.
