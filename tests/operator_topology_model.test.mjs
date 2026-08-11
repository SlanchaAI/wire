import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const fixture = () => ({
  machines: [
    { id: "machine-b", hostname: "Bravo", os: "linux", arch: "x86_64", identity_confidence: "verified" },
    { id: "machine-a", hostname: "Alpha", os: "macos", arch: "aarch64", identity_confidence: "verified" }
  ],
  sessions: [
    {
      machine_id: "machine-b",
      session: {
        id: "bravo", did: "did:wire:bravo-00000002", handle: "Brass-Marten", health: "healthy",
        harness: { label: "Goose Shell" }, project: { name: "Signal", branch: "ops" }
      }
    },
    {
      machine_id: "machine-a",
      session: {
        id: "amber", did: "did:wire:amber-00000001", handle: "Amber-Finch", health: "healthy",
        harness: { label: "Codex CLI" }, project: { name: "Signal", branch: "main" }
      }
    },
    {
      machine_id: "machine-b",
      session: {
        id: "delta", did: "did:wire:delta-00000004", handle: "Delta-Kite", health: "healthy",
        harness: { label: "Claude Code" }, project: { name: "Archive", branch: "release" }
      }
    },
    {
      machine_id: "machine-a",
      session: {
        id: "cedar", did: "did:wire:cedar-00000003", handle: "Cedar-Wren", health: "warning",
        harness: { label: "Claude Code" }, project: { name: "Studio", branch: "feature/map" }
      }
    }
  ],
  direct_links: [
    {
      id: "amber-bravo", source_did: "did:wire:amber-00000001",
      target_did: "did:wire:bravo-00000002", state: "bilateral"
    },
    {
      id: "cedar-delta", source_did: "did:wire:cedar-00000003",
      target_did: "did:wire:delta-00000004", state: "bilateral"
    }
  ],
  groups: [
    {
      id: "crew", name: "Crew", creator_did: "did:wire:amber-00000001", epoch: 1,
      members: [
        { did: "did:wire:amber-00000001", tier: "creator", live: true },
        { did: "did:wire:bravo-00000002", tier: "member", live: true },
        { did: "did:wire:cedar-00000003", tier: "member", live: true },
        { did: "did:wire:historical-99999999", tier: "member", live: false }
      ]
    }
  ]
});

const topology = () => {
  const source = readFileSync(new URL("../assets/operator-topology.js", import.meta.url), "utf8");
  const window = {};
  vm.runInNewContext(source, { window });
  return window.WireTopology;
};

const dids = (visible) => Array.from(visible.sessions, (entry) => entry.session.did);

test("visibleTopology searches session fields case-insensitively without mutating its snapshot", () => {
  const WireTopology = topology();
  const snapshot = fixture();
  const before = structuredClone(snapshot);

  assert.deepEqual(dids(WireTopology.visibleTopology(snapshot, { search: "amber-finch" })), ["did:wire:amber-00000001"]);
  assert.deepEqual(dids(WireTopology.visibleTopology(snapshot, { search: "signal" })), ["did:wire:amber-00000001", "did:wire:bravo-00000002"]);
  assert.deepEqual(dids(WireTopology.visibleTopology(snapshot, { search: "FEATURE/MAP" })), ["did:wire:cedar-00000003"]);
  assert.deepEqual(dids(WireTopology.visibleTopology(snapshot, { search: "goose shell" })), ["did:wire:bravo-00000002"]);
  assert.deepEqual(snapshot, before);
});

test("visibleTopology composes machine, harness, project, health, and connected filters", () => {
  const WireTopology = topology();
  const visible = WireTopology.visibleTopology(fixture(), {
    machine: "machine-a", harness: "codex cli", project: "signal", health: "healthy", connectedOnly: true
  });

  assert.deepEqual(dids(visible), ["did:wire:amber-00000001"]);
  assert.deepEqual(visible.machines.map((machine) => machine.id), ["machine-a"]);
  assert.deepEqual(visible.directLinks, []);
});

test("visibleTopology removes edges with filtered endpoints and trims group members to visible live sessions", () => {
  const WireTopology = topology();
  const machineVisible = WireTopology.visibleTopology(fixture(), { machine: "machine-a" });
  const layout = WireTopology.layoutTopology(machineVisible, { width: 800, height: 600 });

  assert.deepEqual(machineVisible.directLinks, []);
  assert.deepEqual(Array.from(machineVisible.groups[0].members, (member) => member.did), [
    "did:wire:amber-00000001", "did:wire:cedar-00000003"
  ]);
  assert.deepEqual(Array.from(layout.groupRegions[0].memberDids), [
    "did:wire:amber-00000001", "did:wire:cedar-00000003"
  ]);

  const oneMember = WireTopology.visibleTopology(fixture(), { search: "amber" });
  assert.deepEqual(oneMember.groups, []);
  assert.deepEqual(Array.from(WireTopology.layoutTopology(oneMember, { width: 800, height: 600 }).groupRegions), []);
});

test("layoutTopology has stable machine and DID-sorted node positions when source arrays reverse", () => {
  const WireTopology = topology();
  const original = fixture();
  const reversed = structuredClone(original);
  reversed.machines.reverse();
  reversed.sessions.reverse();
  reversed.direct_links.reverse();
  reversed.groups.reverse();

  const viewport = { width: 800, height: 600 };
  const first = WireTopology.layoutTopology(WireTopology.visibleTopology(original, {}), viewport);
  const second = WireTopology.layoutTopology(WireTopology.visibleTopology(reversed, {}), viewport);
  const positions = (layout) => Array.from(layout.nodes, ({ machineId, did, x, y }) => ({ machineId, did, x, y }));

  assert.deepEqual(Array.from(first.machines, (machine) => machine.id), ["machine-a", "machine-b"]);
  assert.deepEqual(Array.from(first.machines, ({ id, x, y }) => ({ id, x, y })), Array.from(second.machines, ({ id, x, y }) => ({ id, x, y })));
  assert.deepEqual(positions(first), positions(second));
  assert.deepEqual(Array.from(first.nodes, (node) => node.did), [
    "did:wire:amber-00000001", "did:wire:cedar-00000003", "did:wire:bravo-00000002", "did:wire:delta-00000004"
  ]);
});

test("layoutTopology preserves both endpoints for a cross-machine bilateral edge", () => {
  const WireTopology = topology();
  const layout = WireTopology.layoutTopology(WireTopology.visibleTopology(fixture(), {}), { width: 800, height: 600 });
  const edge = layout.edges.find((candidate) => candidate.id === "cedar-delta");

  assert.deepEqual(
    { sourceDid: edge.sourceDid, targetDid: edge.targetDid, sourceMachineId: edge.source.machineId, targetMachineId: edge.target.machineId },
    {
      sourceDid: "did:wire:cedar-00000003", targetDid: "did:wire:delta-00000004",
      sourceMachineId: "machine-a", targetMachineId: "machine-b"
    }
  );
  assert.match(edge.path, /^M /);
});

test("layoutTopology emits one group fragment per machine for members split across machines", () => {
  const WireTopology = topology();
  const snapshot = fixture();
  snapshot.groups[0].members = [
    { did: "did:wire:amber-00000001", tier: "creator", live: true },
    { did: "did:wire:bravo-00000002", tier: "member", live: true }
  ];
  const visible = WireTopology.visibleTopology(snapshot, {});
  const layout = WireTopology.layoutTopology(visible, { width: 800, height: 600 });

  assert.equal(visible.groups.length, 1, "the two-member group remains visible");
  assert.deepEqual(Array.from(layout.groupRegions, (region) => ({
    machineId: region.machineId,
    memberDids: Array.from(region.memberDids),
    color: region.color
  })), [
    { machineId: "machine-a", memberDids: ["did:wire:amber-00000001"], color: WireTopology.groupColor("crew") },
    { machineId: "machine-b", memberDids: ["did:wire:bravo-00000002"], color: WireTopology.groupColor("crew") }
  ]);
});

test("fitTransform returns a finite positive scale for empty and populated layouts", () => {
  const WireTopology = topology();
  const viewport = { width: 800, height: 600 };
  const empty = WireTopology.layoutTopology(WireTopology.visibleTopology({ machines: [], sessions: [], direct_links: [], groups: [] }, {}), viewport);
  const populated = WireTopology.layoutTopology(WireTopology.visibleTopology(fixture(), {}), viewport);

  for (const transform of [WireTopology.fitTransform(empty, viewport), WireTopology.fitTransform(populated, viewport)]) {
    assert.ok(Number.isFinite(transform.x));
    assert.ok(Number.isFinite(transform.y));
    assert.ok(Number.isFinite(transform.scale));
    assert.ok(transform.scale > 0);
  }
});

test("layoutTopology does not reorder visible group data", () => {
  const WireTopology = topology();
  const visible = WireTopology.visibleTopology(fixture(), {});
  visible.groups = [
    { ...visible.groups[0], id: "zeta", members: [...visible.groups[0].members].reverse() },
    { ...visible.groups[0], id: "alpha", members: [...visible.groups[0].members] }
  ];
  const before = structuredClone(visible);

  WireTopology.layoutTopology(visible, { width: 800, height: 600 });

  assert.equal(JSON.stringify(visible), JSON.stringify(before));
});
