# Wire Operator Topology Map Design

## Goal

Add a map view to Wire Operator that shows how live sessions connect. The map
must distinguish bilateral Wire links from group membership, preserve the
existing consent model, and scale from one machine to an operator-owned fleet.

The map extends the current dashboard. It does not create a second operator
application or a new agent harness.

## Confirmed choices

- Use a clustered graph.
- Place each live session inside its machine boundary.
- Draw bilateral links as solid lines.
- Draw Wire groups as labeled translucent regions around their live members.
- Let the operator select nodes, then invoke **Link selected** or
  **Create group** through the existing explicit actions.
- Keep drag-to-connect out of scope.
- Show live sessions only. Historical and retired identities stay off the map.
- Preserve the list view for dense inspection and keyboard fallback.
- Design the data contract for multiple machines while shipping one-machine
  discovery first.

## Placement options considered

### Integrated Map tab — selected

Add **Map** and **List** tabs to Wire Operator. Both views consume one topology
snapshot and share selection, filters, mutations, notices, and inspection
state. This keeps topology changes in the existing loopback security boundary.

### List overlay

Draw edges behind the current table. This preserves row density but produces
long crossing lines and fails once sessions span machines.

### Separate topology application

Build a dedicated graph service and link to it from Wire Operator. This creates
two security boundaries, two inventories, and two selection models for one
operator task.

## Interaction design

The dashboard opens on **Map**. A segmented **Map / List** control switches
views without losing selection or filters.

### Graph

- A machine cluster uses its stable machine fingerprint as its key. The header
  shows hostname, operating system, architecture, and live-session count.
- A session node shows persona emoji, handle, harness label, and health ring.
- A solid line joins two sessions only when both homes contain the bilateral
  relationship.
- A one-sided relationship renders as an amber dashed anomaly. The map never
  upgrades it to a bilateral link.
- A group region shows group name and live-member count. Its boundary encloses
  live roster members. A group spanning machines gets one same-colored region
  fragment per machine. Group membership never creates pairwise link edges.
- Cross-machine links cross cluster boundaries. The edge keeps the same
  bilateral semantics.

The first layout uses stable positions: machine clusters form a grid; nodes use
a DID-derived order inside each cluster. Refreshes keep nodes in place unless
the topology changes. Native SVG provides pan, zoom, edges, regions, node
focus, and selection. The build adds no graph dependency or remote script.

### Selection and actions

- Clicking a node toggles selection.
- A selected node opens its existing details in the inspector.
- Two selected nodes enable **Link selected**.
- Two or more selected nodes enable **Create group**.
- Switching to List preserves the same selected session IDs.
- A session that disappears drops from selection during the next snapshot.
- `Escape` clears selection. `Enter` or `Space` toggles a focused node.
- The existing confirmation dialog remains the link consent ceremony.

The map never mutates topology through dragging, edge clicks, or group-region
movement.

### Filters

Map and List share these filters:

- text search across handle, project, branch, and harness;
- machine;
- harness;
- project;
- health;
- connected only.

Filtering hides nodes from the presentation, not from the topology snapshot.
Edges and group regions recompute from visible nodes. A hidden endpoint never
causes a visible dangling edge.

## Topology contract

Add a read-only `GET /api/topology` route. It returns one server-built snapshot:

```json
{
  "schema": "wire-topology-v1",
  "generated_at": "2026-08-10T20:00:00Z",
  "machines": [
    {
      "id": "<machine-fingerprint>",
      "hostname": "Pauls-MacBook-Pro-2.local",
      "os": "macos",
      "arch": "aarch64",
      "identity_confidence": "verified"
    }
  ],
  "sessions": [
    {
      "machine_id": "<machine-fingerprint>",
      "session": "<wire-live-sessions-v2 row>"
    }
  ],
  "direct_links": [
    {
      "id": "<sorted-did-a>:<sorted-did-b>",
      "source_did": "did:wire:rusted-butte-b1616319",
      "target_did": "did:wire:umber-savanna-e98187b5",
      "state": "bilateral"
    }
  ],
  "groups": [
    {
      "id": "<group-id>",
      "name": "crew-health",
      "creator_did": "did:wire:rusted-butte-b1616319",
      "epoch": 1,
      "members": [
        {
          "did": "did:wire:rusted-butte-b1616319",
          "tier": "creator",
          "live": true
        }
      ]
    }
  ],
  "anomalies": []
}
```

Each `sessions` entry wraps one complete, unchanged `wire-live-sessions-v2` row
with the containing `machine_id`. The topology route does not revise the live
session schema.

When a machine fingerprint is unavailable, the server emits an
`unverified:<hostname>:<os>:<arch>` ID and sets `identity_confidence` to
`unverified`. The UI marks that cluster. This fallback groups rows for display;
it does not authorize a cross-machine mutation.

### Direct-link construction

The server reads the peer state for every live session. It canonicalizes each
candidate edge by sorted endpoint DID. An edge is `bilateral` only when both
live endpoints name each other. A relationship present on one side becomes an
anomaly and an edge with state `one-sided`.

The response omits edges whose endpoint DID does not match a live session. It
may report the omitted stale peer count in `anomalies`; it never creates a
historical node.

### Group construction

The server reads group rosters already accepted by the existing group subsystem
from live session homes and merges copies by group ID. The highest epoch wins.
Copies at the same epoch must agree on creator DID and member DIDs. A conflict
becomes an anomaly and suppresses the group region until resolved.

The response exposes group ID, name, creator DID, epoch, member DID, membership
tier, and live state. It never exposes relay URLs, slot IDs, slot tokens,
signing keys, signatures, or filesystem paths.

## Components

### Topology producer

`operator` gains one read-only topology builder. It composes the live-session
inventory, peer records, group rosters, and machine descriptors. Link and group
mutations continue to validate against the live-session inventory and use the
existing routes.

### HTTP route

`operator_web` serves `GET /api/topology` under the existing loopback launch
token, Host, Origin, content-security, and no-store rules. The route performs
one single-flight snapshot at a time, matching the session inventory polling
contract.

### Browser state

One client state object owns the topology snapshot, selected session IDs,
filters, active view, expanded inspector, notice, and in-flight scan promise.
Map and List render from that state. Neither view fetches its own competing
inventory.

### Map renderer

The renderer is a focused module inside the embedded dashboard JavaScript. It
computes stable machine and node positions, group boundaries, visible edges,
and SVG accessibility attributes. It has no mutation authority; it emits
selection intents to the shared controller.

## Data flow

1. The browser requests `/api/topology`.
2. The server collects live sessions once.
3. The topology builder joins machine, bilateral peer, and sanitized group
   state.
4. The browser replaces its snapshot and drops vanished selections.
5. Map and List render from the same state.
6. Selection enables the existing mutation actions.
7. A successful mutation triggers one topology refresh.

## Error handling

- Inventory failure preserves the last rendered snapshot and shows a stale
  banner with the failed scan time.
- A partial peer relationship renders as a dashed anomaly, not a solid link.
- Conflicting group copies suppress the affected region and surface the group
  ID in the inspector.
- Missing machine fingerprint marks the cluster unverified.
- A mutation against a vanished session returns the existing conflict response,
  clears vanished selection, and refreshes topology.
- An empty topology renders a start-session prompt, not a blank canvas.
- Pan and zoom reset through a visible **Fit map** control.

## Security and privacy

- The server remains loopback-only.
- The launch token protects the topology route as it protects session inventory.
- The response contains DIDs and operator-facing metadata already visible in
  Wire Operator.
- The response strips group room credentials, peer transport credentials,
  public keys, signatures, raw host session keys, and command lines. Existing
  project paths remain part of the embedded live-session row.
- Map selection grants no new authority. Existing link confirmation and group
  creation rules remain the mutation boundary.

## Verification

### Rust

- Topology builder emits one machine cluster for same-fingerprint sessions.
- Bilateral peer records produce one canonical solid edge.
- One-sided peer records produce one anomaly and never a bilateral edge.
- Group membership produces a region record without pairwise edges.
- Conflicting same-epoch group copies suppress the group and report an anomaly.
- Stale peers and historical group members never become live nodes.
- Serialized output excludes every credential and key field.
- HTTP tests cover token rejection and the topology schema.

### Browser

- Map and List share selection across view changes.
- Two selected nodes enable Link; two or more enable Create group.
- Polling coalesces unfinished topology scans and resumes after completion.
- Filtering removes dangling edges and recomputes group regions.
- Keyboard focus, `Enter`, `Space`, and `Escape` work on map nodes.
- A stale response preserves the last map and displays a warning.
- Desktop and narrow viewport renders have no horizontal page overflow.

### Live proof

- Run the installed `wire dash --web --no-open` caller.
- Observe `rusted-butte` and `umber-savanna` as live nodes joined by one solid
  edge.
- Create a test group from selected live sessions and observe one group region
  without new pairwise edges.
- Switch Map to List and confirm selection persists.
- Capture console and network logs with no errors or failed requests.

## Success criteria

1. The dashboard opens on a clustered map of live sessions.
2. Every session belongs to one explicit machine cluster.
3. Solid edges mean bilateral Wire links and nothing else.
4. Group regions show group membership without implying a full mesh.
5. Select-then-link and select-then-group use the existing confirmation and
   mutation routes.
6. Map and List share filters, selection, notices, and one polling loop.
7. The map remains legible with 20 live sessions and structurally supports more
   than one machine.
8. The topology response exposes no credentials, raw session keys, or hidden
   historical identities.
9. Focused, full, and live-browser verification passes.

## Deferred work

- Cross-machine discovery, enrollment, and mutation.
- Drag-to-connect or canvas editing.
- Supervisor hierarchy and boss/subagent control.
- Messaging, conversation, retirement, and process lifecycle controls.
- Salud health overlays and per-group external links.
- Historical topology playback.
