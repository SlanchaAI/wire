# Wire Operator Topology Map Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a default clustered Map view to Wire Operator that shows live sessions by machine, bilateral links as edges, and Wire groups as regions while preserving the existing select-then-link and select-then-group consent flow.

**Architecture:** A new read-only topology producer composes the existing live-session report with each live home’s peers and sanitized group rosters. `GET /api/topology` exposes that snapshot behind the current loopback token and one shared scan lock. A dependency-free browser helper filters and lays out the graph; Map and List render from one client state object and one polling loop.

**Tech Stack:** Rust, Axum, Serde, embedded HTML/CSS/JavaScript, native SVG, Node’s built-in test runner, Cargo integration tests, Playwright live-browser verification.

## Global Constraints

- Work only on `feat/operator-dashboard`; never stage the user-owned `AGENTS.md` or `.superpowers/` visual-companion output.
- Before editing an existing Rust or JavaScript symbol, run GitNexus upstream impact analysis and report any HIGH or CRITICAL result before proceeding.
- After every task, run `node .gitnexus/run.cjs detect-changes --scope staged --repo wire` before committing.
- Keep the current mutation contracts: `/api/links` links exactly two live sessions after confirmation; `/api/groups` creates one shared room for two or more selected live sessions.
- Keep the dashboard loopback-only and token-gated. Do not add remote scripts, graph dependencies, lifecycle control, messaging, or cross-machine mutation.
- Emit live sessions only. Never synthesize nodes for stale peers, retired sessions, or historical group members.
- Never serialize group relay coordinates, room tokens, signing keys, signatures, filesystem homes, raw session keys, or command lines.
- Preserve `wire-live-sessions-v2` unchanged inside each topology session wrapper.
- Use Test-Driven Development (TDD): establish RED, implement the minimum GREEN change, then refactor only if checks remain green.
- Each task ends with a verified commit and push. Do not merge.

---

## File Structure

| Path | Responsibility |
|---|---|
| `src/operator_topology.rs` | Topology Data Transfer Objects (DTOs), pure merge/canonicalization logic, live-home collection, and focused Rust tests. |
| `src/operator.rs` | Reuse the existing live inventory from a caller-supplied `SessionInfo` slice so topology collection lists homes once. |
| `src/group.rs` | Add an explicit-home, read-only group-list function; retain the current session-scoped public wrapper. |
| `src/lib.rs` | Export the topology module. |
| `src/operator_web.rs` | Serve `/api/topology`, share one inventory scan lock, embed the topology helper asset, and test auth/security. |
| `assets/operator-topology.js` | Pure filtering, canonical visible-subgraph construction, and deterministic machine/node/group geometry. |
| `assets/operator-dashboard.js` | Shared topology state, polling, Map/List controller, SVG renderer, pan/zoom, selection, dialogs, and stale-snapshot behavior. |
| `assets/operator-dashboard.html` | Map/List switch, filters, map canvas, inspector, Fit map control, and retained table/dialog markup. |
| `assets/operator-dashboard.css` | Machine clusters, nodes, edges, group regions, health states, responsive controls, and accessible focus states. |
| `tests/operator_topology_model.test.mjs` | Pure browser filtering and deterministic-layout tests. |
| `tests/operator_dashboard_topology.test.mjs` | Real dashboard controller tests for one polling loop, shared selection, keyboard actions, and stale responses. |
| `tests/e2e_operator_dashboard.rs` | Real binary/API proof for topology schema, bilateral edges, sanitized groups, and existing mutations. |
| `SESSION_LOG_2026_08_10.md` | Decision, caller, verification, and artifact record. |

---

### Task 1: Build the topology snapshot from explicit live-session homes

**Files:**
- Create: `src/operator_topology.rs`
- Modify: `src/operator.rs`
- Modify: `src/group.rs`
- Modify: `src/lib.rs`

**Interfaces:**

```rust
pub const TOPOLOGY_SCHEMA: &str = "wire-topology-v1";

#[derive(Clone, Debug, Serialize)]
pub struct TopologyReport {
    pub schema: &'static str,
    pub generated_at: String,
    pub machines: Vec<TopologyMachine>,
    pub sessions: Vec<TopologySession>,
    pub direct_links: Vec<DirectLink>,
    pub groups: Vec<TopologyGroup>,
    pub anomalies: Vec<TopologyAnomaly>,
}

pub fn collect_topology() -> anyhow::Result<TopologyReport>;
pub(crate) fn list_groups_at(home: &Path) -> anyhow::Result<Vec<Group>>;
pub(crate) fn collect_live_sessions_from(
    sessions: &[crate::session::SessionInfo],
) -> anyhow::Result<LiveSessionReport>;

#[derive(Clone)]
struct TopologySource {
    session: LiveSession,
    peers: Vec<crate::dash::PeerRow>,
    groups: Vec<crate::group::Group>,
}

fn build_topology(
    sources: Vec<TopologySource>,
    generated_at: OffsetDateTime,
) -> TopologyReport;
```

`TopologyMachine` contains `id`, `hostname`, `os`, `arch`, and `identity_confidence`. `TopologySession` contains `machine_id` plus the complete `LiveSession`. `DirectLink` contains `id`, `source_did`, `target_did`, and `state`. `TopologyGroup` contains only `id`, `name`, `creator_did`, `epoch`, and sanitized `{did, tier, live}` members. `TopologyAnomaly` contains `kind`, `subject_id`, and an operator-safe `message`.

- [ ] Run impact analysis before modifying existing symbols:

```bash
node .gitnexus/run.cjs impact --target collect_live_sessions --direction upstream --repo wire
node .gitnexus/run.cjs impact --target list_groups --direction upstream --repo wire
```

- [ ] Add focused RED tests inside `src/operator_topology.rs` using pure `TopologySource` fixtures. Cover:

  - two sessions with one machine fingerprint produce one verified machine;
  - missing fingerprints produce `unverified:<hostname>:<os>:<arch>` and unverified confidence;
  - reciprocal peer DIDs produce one sorted, canonical `bilateral` edge;
  - one peer record produces one `one-sided` edge and one anomaly;
  - a peer DID absent from live sessions produces no node or edge;
  - group membership produces one region record and no pairwise direct links;
  - the highest group epoch wins;
  - equal highest epochs with different creator/member DIDs suppress the group and emit an anomaly;
  - historical members remain in the sanitized roster with `live: false` but never become session nodes;
  - serialized output omits `relay_url`, `slot_id`, `slot_token`, `key_id`, `key`, `creator_sig`, `home_dir`, and `command_line`.

- [ ] Run the new test target and confirm RED because `operator_topology` does not exist:

```bash
cargo test operator_topology --lib
```

Expected: compile failure for the missing module/types.

- [ ] Add `pub mod operator_topology;` to `src/lib.rs`.

- [ ] Refactor `src/group.rs` without changing behavior:

```rust
pub(crate) fn list_groups_at(home: &Path) -> Result<Vec<Group>> {
    list_groups_in(&home.join("config/wire/groups"))
}

pub fn list_groups() -> Result<Vec<Group>> {
    list_groups_in(&groups_dir()?)
}
```

Keep the existing JSON-extension filter, parse-failure skip, and name sort in the shared private `list_groups_in` body.

- [ ] Refactor `src/operator.rs` so `collect_live_sessions()` lists once and delegates:

```rust
pub fn collect_live_sessions() -> anyhow::Result<LiveSessionReport> {
    let sessions = crate::session::list_sessions()?;
    collect_live_sessions_from(&sessions)
}

pub(crate) fn collect_live_sessions_from(
    sessions: &[crate::session::SessionInfo],
) -> anyhow::Result<LiveSessionReport> {
    collect_live_from(
        sessions,
        OffsetDateTime::now_utc(),
        crate::platform::process_alive,
    )
}
```

- [ ] Implement `collect_topology()` with one `session::list_sessions()` call, one `collect_live_sessions_from(&sessions)` call, and an `id -> home_dir` map drawn only from the already-listed `SessionInfo` records. Read peers with `dash::read_peers` and groups with `group::list_groups_at` only for IDs present in the live report.

- [ ] Implement the pure builder with `BTreeMap`/sorted vectors for deterministic output. Canonicalize every link by sorting endpoint DIDs. Mark it bilateral only when both directed observations exist. Merge group copies by highest epoch; compare creator DID and the sorted `(did, tier)` roster at equal highest epoch.

- [ ] Format and run focused GREEN checks:

```bash
cargo fmt --check
cargo test operator_topology --lib
cargo test group::tests --lib
cargo test operator::tests --lib
```

Expected: all pass.

- [ ] Stage only Task 1 files, inspect scope, commit, and push:

```bash
git add src/operator_topology.rs src/operator.rs src/group.rs src/lib.rs
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "feat: build live operator topology"
git push
```

---

### Task 2: Serve one authenticated, single-flight topology route

**Files:**
- Modify: `src/operator_web.rs`
- Modify: `tests/e2e_operator_dashboard.rs`

**Interfaces:**

```rust
#[derive(Clone)]
struct AppState {
    token: String,
    scan_lock: Arc<tokio::sync::Mutex<()>>,
}

async fn get_topology(State(state): State<AppState>, headers: HeaderMap) -> Response;
```

- [ ] Run impact analysis:

```bash
node .gitnexus/run.cjs impact --target router --direction upstream --repo wire
node .gitnexus/run.cjs impact --target get_sessions --direction upstream --repo wire
```

- [ ] Extend `operator_web::tests::mutation_routes_require_token_and_json` with RED assertions that `/api/topology` rejects a missing/wrong token and hostile Host/Origin exactly like `/api/sessions`.

- [ ] Extend `tests/e2e_operator_dashboard.rs` so the real dashboard binary fetches `/api/topology` and asserts:

```rust
assert_eq!(topology["schema"], "wire-topology-v1");
assert_eq!(topology["sessions"].as_array().unwrap().len(), 3);
assert_eq!(topology["machines"].as_array().unwrap().len(), 1);
```

After the existing link and group POSTs, refetch topology and assert one bilateral edge exists for the selected pair, the group contains all three DIDs, and the serialized response contains none of the secret field names from Task 1.

- [ ] Run RED:

```bash
cargo test operator_web::tests::mutation_routes_require_token_and_json --lib
cargo test --test e2e_operator_dashboard
```

Expected: `/api/topology` returns 404.

- [ ] Add `.route("/api/topology", get(get_topology))`. Initialize one `Arc<Mutex<()>>` in `router`; acquire it in both `get_sessions` and `get_topology` before their `spawn_blocking` calls so old List clients and new Map clients cannot overlap full inventory scans.

- [ ] Return `operator_topology::collect_topology()` as JSON. Preserve the current generic `session inventory failed`/`topology inventory failed` response boundary; never return internal filesystem or parse errors.

- [ ] Run GREEN and security checks:

```bash
cargo fmt --check
cargo test operator_web::tests --lib
cargo test --test e2e_operator_dashboard
```

- [ ] Stage, inspect, commit, and push:

```bash
git add src/operator_web.rs tests/e2e_operator_dashboard.rs
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "feat: serve operator topology snapshot"
git push
```

---

### Task 3: Add the pure browser topology model and stable layout

**Files:**
- Create: `assets/operator-topology.js`
- Create: `tests/operator_topology_model.test.mjs`
- Modify: `src/operator_web.rs`
- Modify: `assets/operator-dashboard.html`

**Interfaces:**

```javascript
window.WireTopology = Object.freeze({
  visibleTopology,
  layoutTopology,
  fitTransform,
  groupColor
});
```

`visibleTopology(snapshot, filters)` returns `{machines, sessions, directLinks, groups}` without mutating the snapshot. `layoutTopology(visible, viewport)` returns machine rectangles, node points, edge paths, and per-machine group-region rectangles. `fitTransform(layout, viewport)` returns `{x, y, scale}`.

- [ ] Run impact analysis before changing asset routes and page scripts:

```bash
node .gitnexus/run.cjs impact --target router --direction upstream --repo wire
node .gitnexus/run.cjs impact --target index --direction upstream --repo wire
```

- [ ] Write RED Node tests with a two-machine, four-session fixture. Assert:

  - search matches handle, project name, branch, and harness label case-insensitively;
  - machine, harness, project, health, and connected-only filters compose;
  - a filtered endpoint removes its edge;
  - a group region contains only visible members and disappears below two visible live members;
  - machine order and DID-sorted node positions remain identical when input arrays are reversed;
  - a cross-machine bilateral edge retains both endpoints;
  - `fitTransform` returns a finite positive scale for empty and populated layouts.

- [ ] Run RED:

```bash
node --test tests/operator_topology_model.test.mjs
```

Expected: missing `assets/operator-topology.js`.

- [ ] Implement `assets/operator-topology.js` as a strict-mode Immediately Invoked Function Expression (IIFE). Use no DOM APIs in the helper. Use stable string comparison on machine ID and session DID. Build a machine grid with fixed cluster/node dimensions; calculate each group fragment as the padded bounding rectangle of visible members on that machine. Use a deterministic hue derived from group ID.

- [ ] Add `const TOPOLOGY_JAVASCRIPT = include_str!("../assets/operator-topology.js");`, serve it as `/topology.js` with the existing JavaScript content type, and load it before `/dashboard.js` with `defer`.

- [ ] Extend the asset security test to assert the helper route has no remote URL, `innerHTML`, `eval`, or dynamic script construction.

- [ ] Run GREEN:

```bash
node --test tests/operator_topology_model.test.mjs
cargo test operator_web::tests::dashboard_assets_are_served_with_local_security_contract --lib
```

- [ ] Stage, inspect, commit, and push:

```bash
git add assets/operator-topology.js assets/operator-dashboard.html src/operator_web.rs tests/operator_topology_model.test.mjs
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "feat: add deterministic topology model"
git push
```

---

### Task 4: Move Map and List onto one topology state and polling loop

**Files:**
- Modify: `assets/operator-dashboard.html`
- Modify: `assets/operator-dashboard.js`
- Modify: `assets/operator-dashboard.css`
- Modify: `tests/operator_dashboard_polling.test.mjs`
- Create: `tests/operator_dashboard_topology.test.mjs`

**Interfaces:**

```javascript
const state = {
  topology: emptyTopology,
  selected: new Set(),
  expanded: new Set(),
  filters: {
    search: "", machine: "", harness: "", project: "", health: "",
    connectedOnly: false
  },
  activeView: "map",
  busy: false,
  scanPromise: null,
  stale: false
};
```

The controller fetches only `/api/topology`. Session rows become a derived list
of wrapped `entry.session` values, preserving current mutation request bodies.

- [ ] Run GitNexus impact analysis for the existing dashboard `scan`, `render`, and selection flow. If the JavaScript symbols are absent from the index, record that limitation and inspect all direct DOM listeners before editing.

- [ ] Update the real-script VM harnesses with distinct element stubs and write RED behaviors:

  - initial load calls `/api/topology` once;
  - unfinished poll ticks coalesce and polling resumes after settlement;
  - failed refresh keeps the previous topology and exposes a stale/error notice;
  - vanished session IDs are removed from `selected` after a successful refresh;
  - selecting in Map, switching to List, and switching back preserves the same selected IDs;
  - exactly two selections enable Link and two or more enable Create group;
  - `Escape` clears selection;
  - filter changes render both views from the same visible topology.

- [ ] Run RED:

```bash
node --test tests/operator_dashboard_polling.test.mjs tests/operator_dashboard_topology.test.mjs
```

Expected: the controller still requests `/api/sessions` and has no Map/List/filter state.

- [ ] Add the segmented Map/List control with `aria-pressed`, a filter bar for text/machine/harness/project/health/connected-only, `#map-panel`, retained `#list-panel`, `#topology-map`, `#map-inspector`, and `#fit-map`. Open on Map.

- [ ] Change `scan()` to fetch `/api/topology`. On success, replace `state.topology`, intersect selection with the new live session IDs, clear stale state, repopulate filter options, and call one `render()`. On failure, preserve the prior snapshot, mark it stale, and render a warning with the failed scan time.

- [ ] Extract `toggleSelection(id)` and call it from both table checkboxes and map intents. Keep `selectedSessions()` as the sole source for the existing link and group dialogs. Preserve `confirmedPair` race validation against current live IDs.

- [ ] Render the List from `WireTopology.visibleTopology(state.topology, state.filters)`, not from a second inventory. Hide filtered rows rather than deleting snapshot data. Update empty copy based on “no live sessions” versus “filters hide all sessions.”

- [ ] Implement view and filter CSS without map geometry yet. At 390 px, controls stack and the page body must not overflow horizontally.

- [ ] Run GREEN:

```bash
node --test tests/operator_dashboard_polling.test.mjs tests/operator_dashboard_topology.test.mjs
cargo test operator_web::tests::dashboard_assets_are_served_with_local_security_contract --lib
```

- [ ] Stage, inspect, commit, and push:

```bash
git add assets/operator-dashboard.html assets/operator-dashboard.js assets/operator-dashboard.css tests/operator_dashboard_polling.test.mjs tests/operator_dashboard_topology.test.mjs
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "feat: share topology state across map and list"
git push
```

---

### Task 5: Render and operate the native SVG topology map

**Files:**
- Modify: `assets/operator-dashboard.js`
- Modify: `assets/operator-dashboard.css`
- Modify: `tests/operator_dashboard_topology.test.mjs`

**Interfaces:**

`renderMap(visible)` owns SVG rendering but no mutations.
`setViewport({x, y, scale})` updates one viewport `<g>`.
`fitMap()` passes `WireTopology.fitTransform(layout, viewport)` to
`setViewport`.

- [ ] Run GitNexus impact analysis for `render` and `toggleSelection`, or document the index gap and inspect their direct callers.

- [ ] Add RED VM assertions that rendered SVG semantics include:

  - one labeled cluster per machine;
  - `role="button"`, `tabindex="0"`, and `aria-pressed` on session nodes;
  - solid bilateral edges and amber dashed one-sided edges;
  - group regions before edges/nodes in paint order;
  - one group fragment per machine for a cross-machine group;
  - no direct edge created from group membership;
  - `Enter` and `Space` toggle a focused node;
  - Fit map resets a changed transform.

- [ ] Run RED:

```bash
node --test tests/operator_dashboard_topology.test.mjs
```

Expected: map panel exists but contains no graph semantics or interactions.

- [ ] Implement `renderMap` with `document.createElementNS`. Paint in this order: machine rectangles/labels, translucent group fragments/labels, direct-link paths, then session nodes. A node shows emoji, handle, harness label, and a health ring. The inspector reuses the existing safe text-node detail fields for the selected node.

- [ ] Wire click and keyboard selection to `toggleSelection`. Wire `Escape` at document level. Do not attach mutations to drag, edges, or group regions.

- [ ] Implement bounded pan and zoom on the viewport group: pointer drag pans; wheel zooms between `0.35` and `2.5`; Fit map calls the pure helper. Keep nodes keyboard reachable independent of current zoom.

- [ ] Style machine boundaries, verified/unverified labels, persona colors, health rings, solid/dashed edges, translucent group fragments, selected/focused nodes, and the inspector. Respect `prefers-reduced-motion`; use no animated layout.

- [ ] Run GREEN:

```bash
node --test tests/operator_topology_model.test.mjs tests/operator_dashboard_polling.test.mjs tests/operator_dashboard_topology.test.mjs
cargo test operator_web::tests --lib
```

- [ ] Stage, inspect, commit, and push:

```bash
git add assets/operator-dashboard.js assets/operator-dashboard.css tests/operator_dashboard_topology.test.mjs
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "feat: render interactive operator topology map"
git push
```

---

### Task 6: Prove the installed caller, responsive UI, and complete scope

**Files:**
- Modify: `tests/e2e_operator_dashboard.rs`
- Modify: `SESSION_LOG_2026_08_10.md`

- [ ] Extend the end-to-end test to prove group creation does not increase `direct_links`, while the created group appears with all selected members. Preserve the existing assertion that the third member is not directly paired.

- [ ] Run the complete automated gate:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
node --test tests/operator_topology_model.test.mjs tests/operator_dashboard_polling.test.mjs tests/operator_dashboard_topology.test.mjs
cargo test --all-targets --all-features
```

- [ ] Build release and atomically install without overwriting a running Mach-O image in place:

```bash
cargo build --release
install -m 755 target/release/wire /Users/laul_pogan/.cargo/bin/wire.new
mv /Users/laul_pogan/.cargo/bin/wire.new /Users/laul_pogan/.cargo/bin/wire
```

- [ ] Start the real caller without stopping the persistent Wire daemon/monitor:

```bash
wire dash --web --no-open
```

Capture the printed loopback URL/token, then exercise `/api/topology` with the launch token. Confirm response time stays below two seconds on the current 3,000-home machine and no second inventory request overlaps an unfinished request.

- [ ] Use Playwright against the installed dashboard at 1440×900 and 390×844. Record:

  - Map is the default and List remains available;
  - `rusted-butte` and `umber-savanna` are live nodes with exactly one solid bilateral edge;
  - selecting both nodes, switching Map/List, and switching back preserves selection;
  - selected link/group controls obey counts and existing confirmation;
  - an existing live group renders a region without extra pairwise edges; if no suitable group exists, create one through the dashboard and retain it as the authorized live proof;
  - Fit map restores all clusters to view;
  - desktop and narrow layouts have no horizontal page overflow;
  - console contains no errors and network contains no failed asset/API requests.

- [ ] Run the required fresh-eyes adversarial review, then a separate read-only AMANALAP scope review. Fix only an original success-criterion failure, essential safety issue, or observed defect; rerun the affected and complete gates.

- [ ] Update `SESSION_LOG_2026_08_10.md` with topology producer/caller, review dispositions, timings, installed proof, and artifact paths.

- [ ] Run final GitNexus and repository checks:

```bash
git status --short
git diff --check
node .gitnexus/run.cjs detect-changes --scope compare --base-ref main --repo wire
```

Confirm `AGENTS.md` and `.superpowers/` remain unstaged.

- [ ] Stage only the proof/logical finalization files, inspect, commit, and push:

```bash
git add tests/e2e_operator_dashboard.rs SESSION_LOG_2026_08_10.md
node .gitnexus/run.cjs detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "test: prove operator topology map"
git push
```

## Completion Evidence

Implementation is complete only when all six tasks are committed and pushed, the full automated gate is green, the installed `wire dash --web` caller renders the live map, `rusted-butte` and `umber-savanna` appear with one bilateral edge, a group region appears without implying a full mesh, Map/List selection persists, and desktop/mobile browser evidence has no console, network, or overflow failures.
