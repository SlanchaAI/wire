# Fleet Session Provenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every live operator-dashboard row identify its harness, Wire identity source, machine, and Git project with explicit provenance and safe unknowns.

**Architecture:** Add a focused `session_metadata` module containing serializable descriptors, pure harness inference, Git filesystem discovery, and a bounded cached process snapshot. New MCP leases persist descriptors; the operator collector merges lease, registry, and process evidence into schema v2. The browser renders compact summaries and a separate expandable detail row without changing selection behavior.

**Tech stack:** Rust, Serde, Axum, macOS/Linux/Windows platform adapters, vanilla JavaScript/CSS, Playwright.

**Approved design:** `docs/superpowers/specs/2026-08-10-fleet-session-provenance-design.md`

---

## Task 1: Define metadata descriptors and harness inference

**Files:**

- Create: `src/session_metadata.rs`
- Modify: `src/lib.rs`
- Test: `src/session_metadata.rs`

- [ ] Add failing unit tests for explicit, inferred, and unknown harnesses. Include exact executable-boundary cases for Codex CLI, ChatGPT Codex app-server, Claude Code, Goose, Cursor, VS Code, and a false-positive command argument containing `codex`.
- [ ] Run `cargo test session_metadata::tests::harness -- --nocapture` and confirm the new test target fails before implementation.
- [ ] Add the smallest descriptor model:

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MetadataConfidence { Explicit, Inferred, Unknown }

pub struct MachineDescriptor { /* fingerprint, hostname, os, arch, wire_version */ }
pub struct HarnessDescriptor { /* kind, label, mode, confidence, evidence */ }
pub struct IdentityDescriptor { /* source, class, warning */ }
pub struct ProjectDescriptor { /* repo/root/cwd/relative/branch/revision/worktree/remote/provenance */ }
```

- [ ] Add a private `ProcessObservation` value and pure `infer_harness(session_source, ancestry)` function. Match executable basenames and explicit app-server arguments only; return evidence classes such as `lease-source`, `process-executable`, or `process-ancestry`, never a raw command line.
- [ ] Export the module from `src/lib.rs`.
- [ ] Run the focused test and `cargo fmt --check`.
- [ ] Commit: `feat: model session provenance`

## Task 2: Discover Git projects without per-row subprocesses

**Files:**

- Create: `src/session_metadata.rs` (continued from Task 1)
- Test: `src/session_metadata.rs`

- [ ] Add failing fixture tests for a normal repository, nested working directory, linked worktree, detached HEAD, missing remote, and non-Git directory.
- [ ] Run `cargo test session_metadata::tests::project -- --nocapture` and confirm failure.
- [ ] Implement `describe_project(cwd)` by walking ancestors for `.git`, resolving directory and `gitdir:` file forms, reading `HEAD`, `commondir`, and Git config. Do not invoke `git`.
- [ ] Normalize an origin remote to a descriptive repository name while retaining the local operator-visible remote string. For unknown facts, keep `Option::None` and `MetadataConfidence::Unknown`.
- [ ] Run focused project tests and `cargo fmt --check`.
- [ ] Commit: `feat: discover session projects`

## Task 3: Capture one bounded process snapshot

**Files:**

- Create: `src/session_metadata.rs` (continued from Task 1)
- Modify: `src/platform.rs`
- Test: `src/session_metadata.rs`

- [ ] Before modifying existing platform helpers, run GitNexus upstream impact analysis for each touched symbol and stop on a HIGH or CRITICAL result.
- [ ] Add failing tests proving one snapshot serves multiple sessions, the cache refreshes only when the sorted live PID set changes, ancestry is bounded, and probe failure returns unknown metadata.
- [ ] Run `cargo test session_metadata::tests::process -- --nocapture` and confirm failure.
- [ ] Implement a platform-neutral snapshot interface. On macOS, issue one bounded `ps` process-table read and one bounded `lsof` cwd read for the active PIDs. On Linux, read `/proc` for requested PIDs plus at most eight ancestors. On Windows, use one bounded process-table probe; leave unavailable cwd values unknown.
- [ ] Cache the snapshot by sorted active PID set. Keep only executable basename, parent PID, safe launch classification inputs, and cwd; never serialize raw arguments.
- [ ] Run focused tests plus `cargo test platform -- --nocapture`.
- [ ] Commit: `feat: snapshot live agent processes`

## Task 4: Enrich leases with backward-compatible snapshots

**Files:**

- Modify: `src/session_lifecycle.rs`
- Test: `src/session_lifecycle.rs`

- [ ] Run GitNexus upstream impact analysis for `LeaseRecord`, `write_lease_at`, `heartbeat_lease_at`, and `LeaseGuard::acquire_at`. Report the blast radius before editing.
- [ ] Add failing tests that deserialize the old lease shape, round-trip new optional machine/harness/project fields, and prove heartbeat preserves known acquisition metadata while filling only unknown facts.
- [ ] Run `cargo test session_lifecycle::tests -- --nocapture` and confirm failure.
- [ ] Add `#[serde(default)]` optional descriptors to `LeaseRecord`. Keep old records readable and preserve the existing lease schema unless compatibility requires a version union.
- [ ] At MCP lease acquisition, capture machine, harness, and project once. During heartbeat, retain explicit values and refresh fields that remain unknown.
- [ ] Update all test helpers and call sites intentionally; do not hide new arguments behind speculative builders.
- [ ] Run focused lease tests.
- [ ] Commit: `feat: persist session provenance in leases`

## Task 5: Serve schema-v2 live inventory

**Files:**

- Modify: `src/operator.rs`
- Modify: `tests/e2e_operator_dashboard.rs`
- Test: `src/operator.rs`
- Test: `tests/e2e_operator_dashboard.rs`

- [ ] Run GitNexus upstream impact analysis for `LiveSession`, `LiveSessionReport`, `collect_live_sessions`, and `collect_live_from`. Warn before editing if risk is HIGH or CRITICAL.
- [ ] Replace old assertions on `agent_host` and `project_dir` with failing assertions for structured `machine`, `harness`, `identity`, and `project` objects. Cover explicit lease precedence, inferred old leases, registry fallback, machine-default warning, and failed probes.
- [ ] Expand the end-to-end fixture with Codex, Claude, Goose, machine-default, full Git metadata, and unknown values. Confirm `/api/sessions` exposes no raw thread IDs, environment values, or command lines.
- [ ] Run `cargo test operator::tests -- --nocapture` and `cargo test --test e2e_operator_dashboard -- --nocapture`; confirm failures.
- [ ] Advance `LIVE_SESSION_SCHEMA` to `wire-live-sessions-v2`. Replace the two overloaded flat fields with the four descriptors while preserving session ID, persona, age, topology, and health.
- [ ] Merge facts in this order: explicit lease metadata, session registry metadata, cached live-process inference, unknown. Classify identity source independently from harness.
- [ ] Run focused unit and end-to-end tests.
- [ ] Commit: `feat: serve descriptive live sessions`

## Task 6: Render compact rows and expandable details

**Files:**

- Modify: `assets/operator-dashboard.html`
- Modify: `assets/operator-dashboard.css`
- Modify: `assets/operator-dashboard.js`
- Modify: `src/operator_web.rs`
- Modify: `tests/e2e_operator_dashboard.rs`

- [ ] Run GitNexus impact analysis for any existing Rust symbol changed in `operator_web.rs`.
- [ ] Add failing asset/API assertions for Harness, Project, Machine, Identity, and the detail control. Add browser assertions that row selection and detail expansion are independent.
- [ ] Run `cargo test operator_web::tests -- --nocapture` and the dashboard end-to-end test; confirm failure.
- [ ] Render compact columns: session, harness/confidence, repository/branch, machine, identity warning, links, and signal. Render `Unknown` for missing facts.
- [ ] Add an explicit details button with `aria-expanded` and a sibling detail row containing DID fingerprint, identity source/class, PID and launch mode, project paths/worktree/remote, machine fingerprint/platform/Wire version, and evidence classes.
- [ ] Preserve sessionStorage selection, link/group actions, responsive layout, keyboard access, and loopback-only presentation.
- [ ] Run focused tests and `cargo fmt --check`.
- [ ] Commit: `feat: show session provenance in dashboard`

## Task 7: Full verification, live proof, review, and install

**Files:**

- Modify: `SESSION_LOG_2026_08_10.md`
- Modify only if a verified defect requires it: implementation files above

- [ ] Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`.
- [ ] Run GitNexus `detect_changes({scope: "compare", base_ref: "main"})`; verify only planned symbols and flows changed before the final commit.
- [ ] Build the release binary, stop only the prior dashboard process, install the verified binary with the repository’s existing install path, and relaunch the loopback dashboard.
- [ ] Use Playwright against the installed dashboard at desktop and mobile widths. Verify compact rows, expanded details, unknown rendering, selection persistence, link/group controls, and zero console errors.
- [ ] Sample at least Codex, Claude, and Goose live rows. Compare displayed harness and cwd against one fresh process snapshot; record evidence without exposing command lines.
- [ ] Run the build-loop semantic review. Then run a separate read-only AMANALAP scope-cut review. Apply only observed defects or missing approved success criteria, then rerun affected checks.
- [ ] Update `SESSION_LOG_2026_08_10.md` with the model correction, source precedence, caller/producer wiring, tests, live proof, and artifact catalog.
- [ ] Commit: `docs: record session provenance delivery`
- [ ] Push `feat/operator-dashboard`. Leave the verified dashboard open for the operator. Do not merge or delete the branch.
