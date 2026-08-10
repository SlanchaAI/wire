# Wire Operator Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repair Codex and Goose session identity resolution, then ship a localhost Wire dashboard that lists live local agent sessions, links two, and creates one shared group room.

**Architecture:** Extend the existing Rust binary. A new operator domain module reads active MCP lifecycle leases and runs existing Wire commands against explicit session homes without a shell. A small Axum server exposes that domain to embedded Open Band HTML, CSS, and JavaScript through token-guarded loopback routes.

**Tech Stack:** Rust 2024, Axum 0.7, Tokio, Serde, existing Wire session/group/pairing primitives, embedded HTML/CSS/vanilla JavaScript.

## Global Constraints

- Bind only to `127.0.0.1`; do not ship a configurable network host.
- Show only sessions with a live `mcp` lifecycle lease.
- Support 10–20 live sessions without pagination.
- Link exactly two selected sessions through the existing local-sister bilateral path.
- Create one shared group room from two or more selected sessions; do not create a full mesh.
- Keep messaging, history, retirement, remote machines, and network exposure out of scope.
- Require a random launch token on every mutation request.
- Never expose raw host session keys, relay slot tokens, private keys, or arbitrary filesystem paths.
- Do not edit `add_local_sister_core`; GitNexus rates its upstream impact CRITICAL.
- Preserve unrelated `AGENTS.md` changes and all files in the original working tree.
- Baseline note: `os_notify::tests::toast_dedup_public_api_suppresses_repeat` failed once under the parallel suite and passed alone.

---

### Task 1: Guarded Goose identity and lifecycle metadata

**Files:**
- Modify: `src/session.rs`
- Modify: `src/session_lifecycle.rs`
- Test: `src/session.rs`
- Test: `src/session_lifecycle.rs`

**Interfaces:**
- Consumes: host environment variables already read by `resolve_session_key()`.
- Produces: `resolve_session_key() -> Option<(String, &'static str)>` with source `goose`; `LeaseRecord.started_at: Option<String>` and `LeaseRecord.cwd: Option<String>` for live inventory.

- [ ] **Step 1: Add a failing guarded-Goose resolver test**

Add a serial environment test beside the Codex adapter test:

```rust
#[test]
fn resolve_session_key_goose_adapter_is_guarded_and_ordered() {
    let _guard = crate::config::test_support::ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let names = [
        "WIRE_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
        "AGENT",
        "AGENT_SESSION_ID",
        "COPILOT_AGENT_SESSION_ID",
        "VSCODE_GIT_REPOSITORY_ROOT",
    ];
    let previous: Vec<_> = names
        .iter()
        .map(|name| (*name, std::env::var_os(name)))
        .collect();
    unsafe {
        for name in names {
            std::env::remove_var(name);
        }
    }
    unsafe {
        std::env::set_var("AGENT", "goose");
        std::env::set_var("AGENT_SESSION_ID", "20260810_7");
    }
    assert_eq!(
        resolve_session_key(),
        Some(("20260810_7".into(), "goose"))
    );
    unsafe { std::env::set_var("AGENT", "another-host") };
    assert!(!matches!(resolve_session_key(), Some((key, _)) if key == "20260810_7"));
    unsafe {
        std::env::set_var("AGENT", "goose");
        std::env::set_var("AGENT_SESSION_ID", "${UNEXPANDED}");
    }
    assert!(!matches!(resolve_session_key(), Some((key, _)) if key.contains("${")));
    unsafe {
        for (name, value) in previous {
            std::env::remove_var(name);
            if let Some(value) = value {
                std::env::set_var(name, value);
            }
        }
    }
}
```

Keep the test's save/restore list synchronized with every adapter variable read by `resolve_session_key()`.

- [ ] **Step 2: Run the resolver test and prove it fails**

Run:

```bash
cargo test session::tests::resolve_session_key_goose_adapter_is_guarded_and_ordered -- --exact
```

Expected: FAIL because `resolve_session_key()` does not return source `goose`.

- [ ] **Step 3: Implement guarded Goose resolution**

Add this branch after `CODEX_THREAD_ID` and before Copilot:

```rust
if std::env::var("AGENT").ok().as_deref() == Some("goose")
    && let Ok(value) = std::env::var("AGENT_SESSION_ID")
    && valid_session_key(&value)
{
    return Some((value.trim().to_string(), "goose"));
}
```

Update session-source documentation, startup warnings, and every isolated child-command environment scrub to remove `AGENT_SESSION_ID` and `AGENT` when `WIRE_HOME` is pinned.

- [ ] **Step 4: Add failing lifecycle metadata tests**

Extend the lease round-trip test:

```rust
assert_eq!(leases[0].started_at.as_deref(), Some("2023-11-14T22:13:20Z"));
assert_eq!(leases[0].cwd.as_deref(), Some("/work/wire"));
```

Add a compatibility test that parses a lease JSON document without either field and expects both fields to be `None`.

- [ ] **Step 5: Run lifecycle tests and prove they fail**

Run:

```bash
cargo test session_lifecycle::tests --lib
```

Expected: compile failure because `LeaseRecord` lacks the two fields.

- [ ] **Step 6: Implement additive lease metadata**

Add optional fields with Serde defaults:

```rust
#[serde(default)]
pub started_at: Option<String>,
#[serde(default)]
pub cwd: Option<String>,
```

New leases set `started_at` to the acquisition time and `cwd` to `std::env::current_dir()` when available. Heartbeats preserve both values. Old leases remain readable.

Extend `write_lease_at` with one final path argument and update its internal callers:

```rust
pub fn write_lease_at(
    home: &Path,
    role: &str,
    pid: u32,
    now: OffsetDateTime,
    ttl: Duration,
    wire_version: &str,
    bin_path: &Path,
    session_source: &str,
    cwd: Option<&Path>,
) -> Result<PathBuf>
```

- [ ] **Step 7: Run focused identity and lifecycle checks**

Run:

```bash
cargo test session::tests::resolve_session_key_codex_cli_adapter_and_priority -- --exact
cargo test session::tests::resolve_session_key_goose_adapter_is_guarded_and_ordered -- --exact
cargo test session_lifecycle::tests --lib
cargo fmt --check
```

Expected: all PASS.

- [ ] **Step 8: Run GitNexus change detection and commit**

Run:

```bash
git add src/session.rs src/session_lifecycle.rs
node /Users/laul_pogan/Source/wire/.gitnexus/run.cjs detect-changes --scope staged --repo /Users/laul_pogan/Source/wire/.worktrees/operator-dashboard
git commit -m "fix: resolve Goose sessions by agent session id"
```

Expected: identity startup flows affected; no unrelated files staged.

### Task 2: Live operator inventory

**Files:**
- Create: `src/operator.rs`
- Modify: `src/lib.rs`
- Test: `src/operator.rs`

**Interfaces:**
- Consumes: `session::list_sessions()`, `session_lifecycle::active_leases_at()`, `dash::read_peers()`, session daemon state, and retire markers.
- Produces: `collect_live_sessions() -> anyhow::Result<LiveSessionReport>` and opaque `LiveSession.id` values used by mutation routes.

- [ ] **Step 1: Write failing inventory fixture tests**

Define the public JSON types:

```rust
#[derive(Clone, Debug, Serialize)]
pub struct LiveSession {
    pub id: String,
    pub handle: String,
    pub did: String,
    pub emoji: String,
    pub primary_hex: String,
    pub agent_host: String,
    pub project_dir: Option<String>,
    pub started_at: Option<String>,
    pub age_seconds: Option<u64>,
    pub direct_link_count: usize,
    pub health: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveSessionReport {
    pub schema: &'static str,
    pub sessions: Vec<LiveSession>,
}
```

Create temp homes for: live MCP lease, live daemon-only lease, expired MCP lease, retired MCP home, and live MCP lease with a dead PID. Assert only the first appears.

- [ ] **Step 2: Run the inventory test and prove it fails**

Run:

```bash
cargo test operator::tests::inventory_includes_only_live_mcp_sessions -- --exact
```

Expected: compile failure because `operator` is absent.

- [ ] **Step 3: Implement the inventory producer**

Use a testable internal function:

```rust
fn collect_live_from(
    sessions: &[crate::session::SessionInfo],
    now: time::OffsetDateTime,
    is_alive: impl Fn(u32) -> bool + Copy,
) -> anyhow::Result<LiveSessionReport>
```

Rules:

- require an initialized DID and handle;
- reject retired homes;
- require at least one active lease with `role == "mcp"`;
- derive `agent_host` from the newest MCP lease's `session_source`;
- derive project and start metadata from that lease, then fall back to `SessionInfo.cwd`;
- count direct peers with `dash::read_peers`;
- map daemon running and sync age to `healthy`, `sync-stale`, or `daemon-down`;
- sort by handle.

The opaque ID is the registered session name/home key already returned by `list_sessions()`, never a raw host thread ID.

- [ ] **Step 4: Add negative disclosure assertions**

Serialize a report and assert it excludes:

```rust
assert!(!json.contains("AGENT_SESSION_ID"));
assert!(!json.contains("slot_token"));
assert!(!json.contains("private.key"));
```

- [ ] **Step 5: Run inventory checks**

Run:

```bash
cargo test operator::tests --lib
cargo test dash::tests --lib
cargo fmt --check
```

Expected: all PASS; existing `wire dash --json` shape stays green.

- [ ] **Step 6: Run GitNexus change detection and commit**

Run:

```bash
git add src/operator.rs src/lib.rs
node /Users/laul_pogan/Source/wire/.gitnexus/run.cjs detect-changes --scope staged --repo /Users/laul_pogan/Source/wire/.worktrees/operator-dashboard
git commit -m "feat: collect live operator sessions"
```

Expected: new operator inventory plus module export only.

### Task 3: Explicit-home link and group operations

**Files:**
- Continue: `src/operator.rs` created in Task 2
- Test: `src/operator.rs`
- Test: `tests/e2e_group.rs`

**Interfaces:**
- Consumes: live opaque session IDs and the current Wire executable.
- Produces: `link_local_sessions(request) -> Result<MutationResult>` and `create_local_group(request) -> Result<MutationResult>`.

- [ ] **Step 1: Write failing validation tests**

Define request and result types:

```rust
#[derive(Debug, Deserialize)]
pub struct LinkRequest { pub sessions: Vec<String> }

#[derive(Debug, Deserialize)]
pub struct GroupRequest {
    pub name: String,
    pub creator: String,
    pub members: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MutationResult {
    pub ok: bool,
    pub message: String,
    pub changed_sessions: Vec<String>,
}
```

Assert link rejects one, three, duplicate, unknown, retired, and non-live IDs. Assert group rejects an empty name, fewer than two distinct members, unknown creator, and creator absent from members.

- [ ] **Step 2: Run validation tests and prove they fail**

Run:

```bash
cargo test operator::tests::link_validation_rejects_invalid_selection -- --exact
cargo test operator::tests::group_validation_rejects_invalid_selection -- --exact
```

Expected: compile failure because mutation functions are absent.

- [ ] **Step 3: Implement one explicit-home command runner**

The runner calls the current Wire executable directly, never `sh -c`:

```rust
fn run_wire_at(home: &Path, args: &[&str]) -> anyhow::Result<serde_json::Value>
```

Set `WIRE_HOME`, `WIRE_HOME_FORCE=1`, and `WIRE_QUIET_AUTOSESSION=1`. Remove every session adapter variable, including `AGENT`, `AGENT_SESSION_ID`, `CODEX_THREAD_ID`, and existing Claude/Codex/Copilot/VS Code names. Require a successful exit and parse one JSON value from stdout. Cap captured stdout and stderr at 256 KiB before including sanitized errors.

- [ ] **Step 4: Implement bilateral link through the existing caller**

Resolve both IDs from a fresh live inventory. Run from A's explicit home:

```text
wire add <B-session-name> --local-sister --json
```

Then read both homes' trust state and require `VERIFIED` in both directions. If already verified, return an idempotent success without launching a child.

Do not modify `add_local_sister_core`.

- [ ] **Step 5: Implement shared group materialization**

From the creator home:

```text
wire group create <name> --json
wire group invite <group-id> --json
```

For every other selected home:

```text
wire group join <join-code> --json
```

Verify `<home>/config/wire/groups/<group-id>.json` exists and parses for every selected member. Return the completed/failed boundary on error. Do not call local pairing and do not create a full mesh.

- [ ] **Step 6: Add an end-to-end local topology test**

Extend the existing hermetic group relay fixture to create three session homes, acquire live MCP leases, create a dashboard group, and assert:

```rust
assert!(group_exists(&alice, &group_id));
assert!(group_exists(&bob, &group_id));
assert!(group_exists(&carol, &group_id));
assert!(!directly_paired(&bob, &carol));
```

Add a two-session link case that checks bilateral `VERIFIED` state.

- [ ] **Step 7: Run topology checks**

Run:

```bash
cargo test operator::tests --lib
cargo test --test e2e_group
cargo test --test stress_within_system pair_all_local_mesh_pairs_every_sister_session_v0_6_0 -- --exact
cargo fmt --check
```

Expected: all PASS, including the untouched legacy pairing path.

- [ ] **Step 8: Run GitNexus change detection and commit**

Run:

```bash
git add src/operator.rs tests/e2e_group.rs
node /Users/laul_pogan/Source/wire/.gitnexus/run.cjs detect-changes --scope staged --repo /Users/laul_pogan/Source/wire/.worktrees/operator-dashboard
git commit -m "feat: add local topology operations"
```

Expected: operator and group test flows affected; CRITICAL pairing core unchanged.

### Task 4: Loopback server and Open Band interface

**Files:**
- Create: `src/operator_web.rs`
- Create: `assets/operator-dashboard.html`
- Create: `assets/operator-dashboard.css`
- Create: `assets/operator-dashboard.js`
- Modify: `src/lib.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/dash.rs`
- Test: `src/operator_web.rs`
- Test: `tests/cli.rs`

**Interfaces:**
- Consumes: `operator::collect_live_sessions`, `operator::link_local_sessions`, and `operator::create_local_group`.
- Produces: `serve(ServeOptions) -> anyhow::Result<()>`; CLI flags `wire dash --web --no-open`.

- [ ] **Step 1: Write failing route-security tests**

Build the router with a fixed test token and assert:

```rust
assert_eq!(post_json("/api/links", None, body).status(), StatusCode::FORBIDDEN);
assert_eq!(post_json("/api/links", Some("wrong"), body).status(), StatusCode::FORBIDDEN);
assert_eq!(post_text("/api/links", "test-token", body).status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
assert_eq!(get("/api/sessions").status(), StatusCode::OK);
```

- [ ] **Step 2: Run route tests and prove they fail**

Run:

```bash
cargo test operator_web::tests --lib
```

Expected: compile failure because `operator_web` is absent.

- [ ] **Step 3: Implement server state and routes**

Define:

```rust
pub struct ServeOptions { pub open_browser: bool }

struct AppState {
    token: String,
}
```

Routes:

- `GET /` embeds the three assets and injects no secret into logs;
- `GET /api/sessions` returns `LiveSessionReport`;
- `POST /api/links` and `POST /api/groups` require `application/json` and `X-Wire-Token`;
- domain validation errors map to 400, vanished live sessions to 409, token errors to 403, and internal failures to sanitized 500 responses.

Bind with `TcpListener::bind((Ipv4Addr::LOCALHOST, 0))`. Print the complete tokenized URL before opening the browser. Browser-open failure prints a warning but leaves the server alive.

- [ ] **Step 4: Add CLI flags and dispatch**

Extend `Command::Dash` and `DashArgs`:

```rust
#[arg(long, conflicts_with_all = ["watch", "json", "retire_idle"])]
web: bool,
#[arg(long, requires = "web")]
no_open: bool,
```

`cmd_dash` enters the Axum runtime only for `--web`; every existing terminal path stays unchanged.

- [ ] **Step 5: Build the Open Band browser client**

The HTML contains semantic table, empty, loading, error, confirmation, and group-dialog states. CSS uses existing Wire tokens:

```css
:root {
  --paper: #eee3ce;
  --paper-shadow: #d9c8a7;
  --ink: #241712;
  --frame: #5b1a2e;
  --frame-deep: #401020;
  --dial: #8fb04a;
  --phosphor: #7fffb0;
  --phosphor-bg: #0b130d;
}
```

JavaScript reads the token from the initial query string, removes it from the visible URL with `history.replaceState`, polls every two seconds, preserves still-live selections, and sends the token only in the custom header. Buttons enforce exact selection cardinality before requests.

- [ ] **Step 6: Add CLI and asset contract tests**

Add tests that:

- `wire dash --web --json` fails argument parsing;
- `wire dash --no-open` fails without `--web`;
- embedded HTML references both mutation actions and accessible dialog labels;
- JavaScript contains no remote URL and no `innerHTML` assignment from API data;
- the server reports a `127.0.0.1` URL.

- [ ] **Step 7: Run server and CLI checks**

Run:

```bash
cargo test operator_web::tests --lib
cargo test --test cli dash
cargo test operator::tests --lib
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Expected: all PASS.

- [ ] **Step 8: Run GitNexus change detection and commit**

Run:

```bash
git add src/operator_web.rs src/lib.rs src/cli/mod.rs src/cli/dash.rs assets/operator-dashboard.html assets/operator-dashboard.css assets/operator-dashboard.js tests/cli.rs
node /Users/laul_pogan/Source/wire/.gitnexus/run.cjs detect-changes --scope staged --repo /Users/laul_pogan/Source/wire/.worktrees/operator-dashboard
git commit -m "feat: add localhost operator dashboard"
```

Expected: dashboard CLI and new web flows only.

### Task 5: Installed runtime, live browser proof, and evidence

**Files:**
- Create: `SESSION_LOG_2026_08_10.md`

**Interfaces:**
- Consumes: built `wire` binary, current Codex `CODEX_THREAD_ID`, a Goose STDIO extension environment, supervisor state, and the real browser.
- Produces: installed working binary, managed daemon topology, live dashboard proof, and persisted session evidence.

- [ ] **Step 1: Run complete deterministic verification**

Run:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test operator::tests --lib
cargo test operator_web::tests --lib
cargo test --test cli
cargo test --test e2e_group
cargo test
cargo test os_notify::tests::toast_dedup_public_api_suppresses_repeat -- --exact
```

Record pass/fail counts and whether the known parallel-only toast failure recurs.

- [ ] **Step 2: Install through the repository path**

Run the repository's documented local install command after inspecting `install.sh` for its exact binary destination. Verify:

```bash
which wire
wire --version
```

The resolved binary must be the freshly built branch artifact or its installed copy.

- [ ] **Step 3: Verify Codex and Goose identity resolution in situ**

Run the installed binary under explicit representative host signals:

```bash
CODEX_THREAD_ID="$CODEX_THREAD_ID" wire whoami --json
AGENT=goose AGENT_SESSION_ID=wire-goose-proof wire whoami --json
```

Verify source labels `codex-cli` and `goose`, distinct config homes, schema v3.2+, and suffixed DIDs. Do not print private keys or relay tokens in the session log.

- [ ] **Step 4: Repair daemon topology without wildcard kills**

Read `wire supervisor --json`, role PID files, parent PIDs, and each candidate's Wire home. Stop only processes that are all of:

- daemon or monitor role;
- parent PID 1 or otherwise outside the supervisor tree;
- serving a home already owned by the managed supervisor or a machine-default manual start;
- not the active MCP server.

Restart through the existing service manager. Verify the supervisor is alive, workers stay within its cap, and no unmanaged daemon serves the active home.

- [ ] **Step 5: Run the real localhost dashboard**

Start:

```bash
wire dash --web --no-open
```

Capture the printed tokenized localhost URL without committing it. Drive the real page in Playwright: load, inspect console and failed requests, verify only live rows render, select two fixture sessions, link them, create a group from selected fixtures, refresh, and confirm topology changes.

Use temporary session homes and a local-only relay for mutation proof; never pair or group unrelated real sessions during verification.

- [ ] **Step 6: Run rendered-page audit**

Check desktop and narrow widths, keyboard selection, focus visibility, dialog labels, loading/empty/error states, overflow, console errors, and failed network requests. Fix only defects that block the approved success criteria or accessibility floor.

- [ ] **Step 7: Run independent semantic and AMANALAP reviews**

Build the required review packet with goal, boundaries, success criteria, diff, named CLI/browser callers, exact verification, and assumptions. Run one fresh read-only semantic review through the build-loop reviewer. Send its findings through a separate AMANALAP cut review. Fix surviving BLOCKER/MAJOR findings, remove CUT work, and rerun affected checks.

- [ ] **Step 8: Write the session log**

Record:

- root causes and why earlier `wire up` repairs targeted fallback identities;
- Codex and Goose adapter evidence;
- files changed and named callers;
- exact verification results;
- unmanaged processes stopped and whether recovery is possible;
- semantic-review findings and AMANALAP dispositions;
- deferred remote-machine registry and session retirement work.

- [ ] **Step 9: Run final GitNexus check and commit**

Run:

```bash
git diff --check
git status --short
git add SESSION_LOG_2026_08_10.md
node /Users/laul_pogan/Source/wire/.gitnexus/run.cjs detect-changes --scope staged --repo /Users/laul_pogan/Source/wire/.worktrees/operator-dashboard
node /Users/laul_pogan/Source/wire/.gitnexus/run.cjs detect-changes --scope compare --base-ref main --repo /Users/laul_pogan/Source/wire/.worktrees/operator-dashboard
git commit -m "docs: record operator dashboard verification"
```

Expected: only approved feature files and evidence commits on `feat/operator-dashboard`; unrelated `AGENTS.md` remains uncommitted.
