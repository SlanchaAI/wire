# GitHub Actions Cost Reduction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve Wire's protected behavior coverage while cutting GitHub Actions job fan-out, repeated compilation, fragmented caches, redundant artifact retention, and docs-only deployments.

**Architecture:** Full behavior checks run once on pull requests in six jobs; merged `main` commits run two shared-cache warmers instead of the full suite. One serial Linux end-to-end job builds the release binary once and drives every existing Linux CLI caller. Release artifacts remain durable on GitHub Releases while their temporary workflow handoffs expire after one day.

**Tech Stack:** GitHub Actions YAML, Bash and PowerShell workflow steps, Cargo/Rust, `Swatinem/rust-cache@v2`, `actionlint`, Docker-backed `test-env/run.sh`, GitHub branch protection API.

## Global Constraints

- Do not delete existing Actions artifacts or caches in this branch.
- Do not mutate production Fly state, secrets, branch protection, or live Wire services during local implementation.
- Preserve every current Linux and Windows behavior command.
- Every temporary `WIRE_HOME` used by spawned Wire processes sets `WIRE_HOME_FORCE=1`.
- Keep six release targets and nightly backups at 90-day retention.
- Run GitNexus change detection before every commit and `test-env/run.sh` before publication.

---

### Task 1: Prove current workflow violates the approved budget

**Files:**
- Inspect: `.github/workflows/ci.yml`
- Inspect: `.github/workflows/release.yml`
- Inspect: `.github/workflows/fly-deploy.yml`
- Inspect: `require-ci.sh`

**Interfaces:**
- Consumes: current workflow job IDs, cache inputs, triggers, and artifact retention.
- Produces: captured RED evidence for missing consolidation and retention controls.

- [ ] **Step 1: Run a structural assertion against the current YAML**

Run a Ruby YAML assertion that expects the approved end state:

```bash
ruby -ryaml -e '
ci = YAML.load_file(".github/workflows/ci.yml")
jobs = ci.fetch("jobs")
raise "missing linux-e2e" unless jobs.key?("linux-e2e")
raise "legacy demo job remains" if jobs.key?("demo-invite")
release = YAML.load_file(".github/workflows/release.yml")
upload = release.dig("jobs", "build", "steps").find { |s| s["uses"] == "actions/upload-artifact@v7" }
raise "release retention is not one day" unless upload.dig("with", "retention-days") == 1
'
```

Expected: FAIL with `missing linux-e2e` before production YAML changes.

- [ ] **Step 2: Record RED evidence in `SESSION_LOG_2026_07_18.md`**

Record the command, expected failure, current 12-job PR fan-out, seven Linux release builds, 11.76 GB cache usage, and 2.30 GB artifact usage.

### Task 2: Consolidate CI and cache ownership

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `require-ci.sh`

**Interfaces:**
- Consumes: existing demo scripts, `scripts/hello-world-validate.sh`, `tests/it/run-all.sh`, installer smoke commands, and Windows smoke commands.
- Produces: PR checks `test`, `fmt`, `clippy`, `docs-lint`, `linux-e2e`, and `install-smoke-windows`; main jobs `warm-cache-linux` and `warm-cache-windows`.

- [ ] **Step 1: Add event conditions and concurrency**

Keep `push: main` and `pull_request: main`; add:

```yaml
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
```

Gate the six protected jobs with `if: github.event_name == 'pull_request'`.
Gate both warmers with `if: github.event_name == 'push'`.

- [ ] **Step 2: Establish shared restore-only PR caches**

Use this Linux cache configuration in `test`, `clippy`, and `linux-e2e`:

```yaml
- uses: Swatinem/rust-cache@v2
  with:
    shared-key: ci-linux
    save-if: ${{ github.event_name == 'push' }}
```

Because those jobs run only on pull requests, they restore the default-branch
cache without saving merge-ref copies. `warm-cache-linux` uses the same
`shared-key`, runs both `cargo build --all-targets` and
`cargo build --release --bin wire`, and saves on `main`.

Use `shared-key: ci-windows` in the Windows smoke and warmer with the same
save policy. The main warmer runs `cargo build --release --bin wire`.

- [ ] **Step 3: Replace seven Linux jobs with `linux-e2e`**

Set `WIRE_HOME_FORCE: "1"` at job scope, install `jq` and `bc` once, build the
release binary once, then preserve these existing steps in serial order:

```bash
WIRE=./target/release/wire bash demo-invite.sh
./target/release/wire demo --json | tee /dev/stderr | grep -q '"ok":true'
WIRE_BIN=./target/release/wire bash scripts/hello-world-validate.sh 5
WIRE=./target/release/wire bash demo-hotline.sh
WIRE=./target/release/wire tests/it/run-all.sh
```

Follow them with the existing fresh-user/nuke smoke and local HTTP installer
smoke bodies unchanged except for inheriting `WIRE_HOME_FORCE=1`.

- [ ] **Step 4: Update the protection caller**

Replace obsolete contexts in `require-ci.sh` with exactly:

```json
{"context": "test"},
{"context": "fmt"},
{"context": "clippy"},
{"context": "docs-lint"},
{"context": "linux-e2e"},
{"context": "install-smoke-windows"}
```

Do not execute `require-ci.sh` locally. It is the post-push migration caller.

- [ ] **Step 5: Re-run structural assertions**

Run the Task 1 Ruby assertion plus checks for six pull-request jobs, two push
warmers, shared cache keys, and the six protection contexts.

Expected: PASS.

### Task 3: Bound release storage and docs-only deploys

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/backup-relay-state.yml`
- Modify: `.github/workflows/fly-deploy.yml`

**Interfaces:**
- Consumes: six-target release matrix, release aggregation job, nightly relay backup, and Fly root Docker build.
- Produces: one-day temporary release handoffs, restore-only tag caches, accurate backup cost documentation, and safe deploy exclusions.

- [ ] **Step 1: Bound tag-only cache and handoff lifetime**

For release build and crates.io publish cache actions, add:

```yaml
save-if: false
```

For the release build `upload-artifact` step, add:

```yaml
retention-days: 1
```

- [ ] **Step 2: Exclude non-image paths from Fly deploy**

Under the `push: main` trigger, add `paths-ignore` entries for `**/*.md`,
`docs/**`, `skills/**`, `hooks/**`, `examples/**`, `scoop/**`, and
`.gemini-plugin/**`. None is copied by the root production Dockerfile.

- [ ] **Step 3: Correct backup storage documentation**

Replace the claim that GitHub Actions artifacts cost zero with the precise
statement that snapshots count toward pooled Actions storage even while the
current public-repository discount makes net ledger cost zero.

### Task 4: Verify, review, publish, and hand off protection migration

**Files:**
- Modify: `SESSION_LOG_2026_07_18.md`
- Verify: all changed workflow and caller files

**Interfaces:**
- Consumes: changed workflows and protection caller.
- Produces: verified branch, review evidence, pushed commits, and exact live rollout handoff.

- [ ] **Step 1: Run deterministic checks**

```bash
actionlint .github/workflows/*.yml
git diff --check
test-env/run.sh
```

Expected: actionlint and diff checks exit zero; canonical container gate passes
formatting, clippy, serial Rust tests, release build, demos, and integration scripts.

- [ ] **Step 2: Run GitNexus change detection**

```bash
node .gitnexus/run.cjs detect-changes --scope compare --base-ref origin/main --repo wire --branch chore/reduce-actions-usage
```

Expected: workflow, shell caller, docs, and no Rust execution-flow changes.

- [ ] **Step 3: Run one independent semantic review**

Use `build-loop/scripts/review.py` with writer `codex`, reviewer `claude`, and a
packet covering preserved commands, event conditions, cache scope, retention,
deploy exclusions, branch-protection migration, and deterministic results.

Expected: no BLOCKER or MAJOR finding. Fix accepted findings within the
three-cycle cap and rerun deterministic checks.

- [ ] **Step 4: Commit logical units and push**

```bash
git add .github/workflows/ci.yml require-ci.sh
git commit -m "ci: consolidate protected checks"
git add .github/workflows/release.yml .github/workflows/backup-relay-state.yml .github/workflows/fly-deploy.yml
git commit -m "ci: bound Actions storage and deploys"
git add SESSION_LOG_2026_07_18.md docs/superpowers/plans/2026-07-18-github-actions-cost-reduction.md
git commit -m "docs: record Actions usage reduction"
git push -u origin chore/reduce-actions-usage
```

- [ ] **Step 5: Inspect real checks without changing protection**

Create a pull request only if publication workflow calls for it, inspect its
GitHub Actions check names and durations, and report the exact command still
required after checks are green:

```bash
./require-ci.sh
```

Do not run that mutation or merge without explicit rollout authority.
