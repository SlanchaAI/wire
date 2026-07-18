# Codex Thread Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve fresh Codex processes from their stable `CODEX_THREAD_ID` so removing a shared `WIRE_SESSION_ID` restores independent Wire identities.

**Architecture:** Extend the existing ordered environment adapter in `resolve_session_key`; do not add another identity path. Keep `WIRE_SESSION_ID`, Claude Code, and `CODEX_SESSION_ID` precedence intact, reuse the `codex-cli` source label, and make every relevant test process clear the newly recognized host variable.

**Tech Stack:** Rust 2024, Cargo tests, shell CI mirror, launchd-managed local service, GitHub protected checks.

## Global Constraints

- Preserve live Wire homes and launchd ownership; do not hand-start daemons, run `wire upgrade`, kill process families, or rewrite live homes.
- Every spawned `wire` process using temporary `WIRE_HOME` must set `WIRE_HOME_FORCE=1`.
- Never print or persist the raw Codex thread ID outside Wire's existing hashed session-home mapping.
- Install the compatible binary before removing the fixed user-level `WIRE_SESSION_ID` override.
- Run GitNexus change detection before every commit and `test-env/run.sh` before merge.

---

### Task 1: Codex thread adapter regression

**Files:**
- Modify: `src/session.rs:838-867`
- Test: `src/session.rs:1847-2188`
- Modify: `test-env/runtime-210.sh:23-24`

**Interfaces:**
- Consumes: `resolve_session_key() -> Option<(String, &'static str)>` and `valid_session_key(&str) -> bool`.
- Produces: `CODEX_THREAD_ID` resolution with source `codex-cli`, after `CODEX_SESSION_ID` and before Copilot.

- [ ] **Step 1: Extend the existing Codex test with a failing thread-ID case**

Snapshot and clear `CODEX_THREAD_ID` beside `CODEX_SESSION_ID`, then add these assertions before setting `CODEX_SESSION_ID`:

```rust
unsafe { std::env::set_var("CODEX_THREAD_ID", "019f1111-1111-7111-8111-111111111111") };
let thread_a = resolve_session_key();
assert!(
    matches!(&thread_a, Some((key, source))
        if key == "019f1111-1111-7111-8111-111111111111" && *source == "codex-cli"),
    "CODEX_THREAD_ID must resolve as codex-cli; got {thread_a:?}"
);
let thread_home_a = session_home_for_key(&thread_a.as_ref().unwrap().0).unwrap();
unsafe { std::env::set_var("CODEX_THREAD_ID", "019f2222-2222-7222-8222-222222222222") };
let thread_home_b = session_home_for_key(&resolve_session_key().unwrap().0).unwrap();
assert_ne!(thread_home_a, thread_home_b);
```

- [ ] **Step 2: Run the focused test to prove RED**

Run:

```bash
cargo test session::tests::resolve_session_key_codex_cli_adapter_and_priority -- --exact --nocapture
```

Expected: FAIL because `resolve_session_key` does not yet read `CODEX_THREAD_ID`.

- [ ] **Step 3: Add the minimal adapter and precedence assertions**

Insert the adapter immediately after `CODEX_SESSION_ID`:

```rust
("CODEX_SESSION_ID", "codex-cli"),
("CODEX_THREAD_ID", "codex-cli"),
```

Update the function documentation to state Codex currently exposes `CODEX_THREAD_ID`. In the test, assert `CODEX_SESSION_ID` wins when both Codex variables exist, `WIRE_SESSION_ID` wins over both, and `${SOME_PLACEHOLDER}` in `CODEX_THREAD_ID` never resolves.

- [ ] **Step 4: Make adapter tests and the runtime harness hermetic**

In the VS Code, Copilot, and Codex adapter tests, snapshot, clear, and restore `CODEX_THREAD_ID` beside `CODEX_SESSION_ID`. Add `CODEX_THREAD_ID` to the `unset` list in `test-env/runtime-210.sh`.

- [ ] **Step 5: Run focused verification**

Run:

```bash
cargo test resolve_session_key_ -- --nocapture
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all adapter tests PASS; formatting and clippy exit 0.

- [ ] **Step 6: Commit the adapter**

Run:

```bash
git add src/session.rs test-env/runtime-210.sh
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "fix: resolve Codex identities by thread"
```

Expected: change detection lists only session-key resolution/test scope; commit succeeds.

### Task 2: Canonical verification and session record

**Files:**
- Modify: `SESSION_LOG_2026_07_18.md`

**Interfaces:**
- Consumes: Task 1's adapter and tests.
- Produces: canonical gate evidence and auditable deployment commands.

- [ ] **Step 1: Run the canonical CI mirror**

Run:

```bash
test-env/run.sh
```

Expected: exit 0 with every gate section passing.

- [ ] **Step 2: Record results without raw identifiers**

Append the RED failure, focused PASS commands, canonical gate result, and planned config migration to `SESSION_LOG_2026_07_18.md`. Record only presence and precedence of environment variables, never their values.

- [ ] **Step 3: Review the actual branch diff**

Run:

```bash
git diff --check origin/main...HEAD
node .gitnexus/run.cjs detect-changes --scope compare --base-ref main --repo wire
git diff --stat origin/main...HEAD
git diff origin/main...HEAD -- src/session.rs test-env/runtime-210.sh SESSION_LOG_2026_07_18.md
```

Expected: only design, plan, session adapter, runtime isolation, and session log changes.

- [ ] **Step 4: Commit the verification record**

Run:

```bash
git add SESSION_LOG_2026_07_18.md
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "docs: record Codex identity verification"
```

Expected: documentation-only commit succeeds.

### Task 3: Protected merge and managed deployment

**Files:**
- Modify after compatible install: `~/.codex/config.toml` (remove only the fixed `WIRE_SESSION_ID` entry)

**Interfaces:**
- Consumes: verified branch and existing launchd services.
- Produces: merged Wire binary plus future Codex processes keyed by their unique thread IDs.

- [ ] **Step 1: Push and open the pull request**

Run:

```bash
git push -u origin fix/codex-thread-identity
gh pr create --base main --head fix/codex-thread-identity --fill
```

Expected: branch pushed and PR URL returned.

- [ ] **Step 2: Wait for every protected check, then merge**

Run:

```bash
pr_number="$(gh pr view --json number --jq .number)"
gh pr checks --watch "$pr_number"
gh pr merge "$pr_number" --merge
```

Expected: all required checks pass and GitHub reports a merge commit on `main`.

- [ ] **Step 3: Build and atomically install the merged binary**

Build merged `main` in the isolated worktree, compare SHA-256 hashes for candidate and staged file, preserve the currently installed binary as `~/.cargo/bin/wire.pre-codex-thread-20260718`, then atomically move the staged candidate onto `~/.cargo/bin/wire`.

- [ ] **Step 4: Remove only the fixed override**

Back up `~/.codex/config.toml` with mode and timestamps preserved. Remove only
the `WIRE_SESSION_ID` assignment from `[shell_environment_policy.set]` without
printing its value.

Do not rewrite the file or edit shell dotfiles. Existing Codex processes keep their inherited identity; fresh processes use `CODEX_THREAD_ID`.

- [ ] **Step 5: Verify managed runtime state**

Run read-only checks for `wire service status`, launchd service state, daemon/MCP process counts and aggregate RSS, TCP health on `127.0.0.1:8771`, binary version/hash consistency, and `wire doctor`. Use isolated tests for thread-key behavior; do not mint probe identities in the live Wire root.

Expected: local relay healthy, daemon count stays bounded, no new version skew, doctor no longer reports the fixed-literal operator collision, and no destructive cleanup is needed.
