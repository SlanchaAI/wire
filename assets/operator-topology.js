(() => {
  "use strict";

  const CLUSTER_WIDTH = 320;
  const CLUSTER_MIN_HEIGHT = 160;
  const CLUSTER_GAP = 32;
  const CLUSTER_PADDING = 20;
  const CLUSTER_HEADER_HEIGHT = 40;
  const NODE_WIDTH = 128;
  const NODE_HEIGHT = 52;
  const NODE_GAP_X = 16;
  const NODE_GAP_Y = 16;
  const REGION_PADDING = 12;

  const list = (value) => Array.isArray(value) ? value : [];
  const text = (value) => value === null || value === undefined ? "" : String(value);
  const compare = (left, right) => text(left) < text(right) ? -1 : text(left) > text(right) ? 1 : 0;
  const folded = (value) => text(value).toLocaleLowerCase();
  const matches = (value, filter) => !filter || folded(value) === folded(filter);
  const includes = (value, query) => !query || folded(value).includes(folded(query));

  const sessionEntry = (entry) => entry && entry.session && typeof entry.session === "object" ? entry : null;
  const sortedEntries = (entries) => list(entries)
    .map(sessionEntry)
    .filter(Boolean)
    .sort((left, right) => compare(left.session.did, right.session.did));

  const visibleTopology = (snapshot, filters = {}) => {
    const source = snapshot && typeof snapshot === "object" ? snapshot : {};
    const entries = sortedEntries(source.sessions);
    const liveDids = new Set(entries.map((entry) => entry.session.did));
    const connected = new Set();
    for (const link of list(source.direct_links)) {
      if (liveDids.has(link.source_did) && liveDids.has(link.target_did)) {
        connected.add(link.source_did);
        connected.add(link.target_did);
      }
    }

    const visibleSessions = entries.filter((entry) => {
      const session = entry.session;
      const query = text(filters.search).trim();
      const searchable = [session.handle, session.project && session.project.name, session.project && session.project.branch, session.harness && session.harness.label];
      return searchable.some((value) => includes(value, query))
        && matches(entry.machine_id, text(filters.machine).trim())
        && matches(session.harness && session.harness.label, text(filters.harness).trim())
        && matches(session.project && session.project.name, text(filters.project).trim())
        && matches(session.health, text(filters.health).trim())
        && (!filters.connectedOnly || connected.has(session.did));
    }).map((entry) => ({ machine_id: entry.machine_id, session: { ...entry.session } }));

    const visibleDids = new Set(visibleSessions.map((entry) => entry.session.did));
    const machineIds = new Set(visibleSessions.map((entry) => entry.machine_id));
    const machines = list(source.machines)
      .filter((machine) => machine && machineIds.has(machine.id))
      .map((machine) => ({ ...machine }))
      .sort((left, right) => compare(left.id, right.id));
    const directLinks = list(source.direct_links)
      .filter((link) => link && visibleDids.has(link.source_did) && visibleDids.has(link.target_did))
      .map((link) => ({ ...link }))
      .sort((left, right) => compare(left.id, right.id));
    const groups = list(source.groups).map((group) => {
      const members = list(group.members)
        .filter((member) => member && member.live && visibleDids.has(member.did))
        .map((member) => ({ ...member }))
        .sort((left, right) => compare(left.did, right.did));
      return { ...group, members };
    }).filter((group) => group.members.length >= 2)
      .sort((left, right) => compare(left.id, right.id));

    return { machines, sessions: visibleSessions, directLinks, groups };
  };

  const normalizedViewport = (viewport) => ({
    width: Number.isFinite(viewport && viewport.width) && viewport.width > 0 ? viewport.width : CLUSTER_WIDTH,
    height: Number.isFinite(viewport && viewport.height) && viewport.height > 0 ? viewport.height : CLUSTER_MIN_HEIGHT
  });

  const groupColor = (id) => {
    let hash = 0;
    for (const character of text(id)) hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
    return `hsl(${hash % 360} 64% 42%)`;
  };

  const layoutTopology = (visible, viewport) => {
    const input = visible && typeof visible === "object" ? visible : {};
    const viewportSize = normalizedViewport(viewport);
    const entries = sortedEntries(input.sessions);
    const entriesByMachine = new Map();
    for (const entry of entries) {
      const machineEntries = entriesByMachine.get(entry.machine_id) || [];
      machineEntries.push(entry);
      entriesByMachine.set(entry.machine_id, machineEntries);
    }

    const machineInputs = list(input.machines)
      .filter((machine) => machine && entriesByMachine.has(machine.id))
      .map((machine) => ({ ...machine, entries: entriesByMachine.get(machine.id) }))
      .sort((left, right) => compare(left.id, right.id));
    const columns = Math.max(1, Math.floor((viewportSize.width + CLUSTER_GAP) / (CLUSTER_WIDTH + CLUSTER_GAP)));
    const dimensions = machineInputs.map((machine) => {
      const rows = Math.max(1, Math.ceil(machine.entries.length / 2));
      return { ...machine, width: CLUSTER_WIDTH, height: Math.max(CLUSTER_MIN_HEIGHT, CLUSTER_HEADER_HEIGHT + CLUSTER_PADDING * 2 + rows * NODE_HEIGHT + (rows - 1) * NODE_GAP_Y) };
    });
    const rowHeights = [];
    for (let index = 0; index < dimensions.length; index += 1) {
      const row = Math.floor(index / columns);
      rowHeights[row] = Math.max(rowHeights[row] || 0, dimensions[index].height);
    }
    const rowOffsets = [];
    let nextRow = CLUSTER_GAP;
    for (let row = 0; row < rowHeights.length; row += 1) {
      rowOffsets[row] = nextRow;
      nextRow += rowHeights[row] + CLUSTER_GAP;
    }

    const machines = [];
    const nodes = [];
    const nodeByDid = new Map();
    for (let index = 0; index < dimensions.length; index += 1) {
      const machine = dimensions[index];
      const column = index % columns;
      const row = Math.floor(index / columns);
      const rectangle = { ...machine, x: CLUSTER_GAP + column * (CLUSTER_WIDTH + CLUSTER_GAP), y: rowOffsets[row] };
      delete rectangle.entries;
      machines.push(rectangle);
      for (let entryIndex = 0; entryIndex < machine.entries.length; entryIndex += 1) {
        const entry = machine.entries[entryIndex];
        const nodeColumn = entryIndex % 2;
        const nodeRow = Math.floor(entryIndex / 2);
        const left = rectangle.x + CLUSTER_PADDING + nodeColumn * (NODE_WIDTH + NODE_GAP_X);
        const top = rectangle.y + CLUSTER_HEADER_HEIGHT + CLUSTER_PADDING + nodeRow * (NODE_HEIGHT + NODE_GAP_Y);
        const node = {
          machineId: machine.id, did: entry.session.did, session: entry.session,
          x: left + NODE_WIDTH / 2, y: top + NODE_HEIGHT / 2,
          left, top, width: NODE_WIDTH, height: NODE_HEIGHT
        };
        nodes.push(node);
        nodeByDid.set(node.did, node);
      }
    }

    const edges = list(input.directLinks).map((link) => {
      const source = nodeByDid.get(link.source_did);
      const target = nodeByDid.get(link.target_did);
      if (!source || !target) return null;
      return {
        ...link, sourceDid: source.did, targetDid: target.did, source, target,
        path: `M ${source.x} ${source.y} L ${target.x} ${target.y}`
      };
    }).filter(Boolean).sort((left, right) => compare(left.id, right.id));

    const groupRegions = [];
    for (const group of list(input.groups).slice().sort((left, right) => compare(left.id, right.id))) {
      const byMachine = new Map();
      for (const member of list(group.members).slice().sort((left, right) => compare(left.did, right.did))) {
        const node = nodeByDid.get(member.did);
        if (!node) continue;
        const members = byMachine.get(node.machineId) || [];
        members.push(node);
        byMachine.set(node.machineId, members);
      }
      for (const [machineId, members] of [...byMachine.entries()].sort((left, right) => compare(left[0], right[0]))) {
        if (members.length < 2) continue;
        const left = Math.min(...members.map((node) => node.left)) - REGION_PADDING;
        const top = Math.min(...members.map((node) => node.top)) - REGION_PADDING;
        const right = Math.max(...members.map((node) => node.left + node.width)) + REGION_PADDING;
        const bottom = Math.max(...members.map((node) => node.top + node.height)) + REGION_PADDING;
        groupRegions.push({
          groupId: group.id, name: group.name, machineId, memberDids: members.map((node) => node.did).sort(compare),
          color: groupColor(group.id), x: left, y: top, width: right - left, height: bottom - top
        });
      }
    }

    const width = machines.length ? Math.max(...machines.map((machine) => machine.x + machine.width)) + CLUSTER_GAP : 0;
    const height = machines.length ? Math.max(...machines.map((machine) => machine.y + machine.height)) + CLUSTER_GAP : 0;
    return { machines, nodes, edges, groupRegions, bounds: { x: 0, y: 0, width, height } };
  };

  const fitTransform = (layout, viewport) => {
    const viewportSize = normalizedViewport(viewport);
    const bounds = layout && layout.bounds ? layout.bounds : { x: 0, y: 0, width: 0, height: 0 };
    const width = Number.isFinite(bounds.width) && bounds.width > 0 ? bounds.width : 0;
    const height = Number.isFinite(bounds.height) && bounds.height > 0 ? bounds.height : 0;
    if (!width || !height) return { x: viewportSize.width / 2, y: viewportSize.height / 2, scale: 1 };
    const padding = 32;
    const scale = Math.max(0.01, Math.min(1, (viewportSize.width - padding * 2) / width, (viewportSize.height - padding * 2) / height));
    return {
      x: (viewportSize.width - width * scale) / 2 - (bounds.x || 0) * scale,
      y: (viewportSize.height - height * scale) / 2 - (bounds.y || 0) * scale,
      scale
    };
  };

  window.WireTopology = Object.freeze({ visibleTopology, layoutTopology, fitTransform, groupColor });
})();
