(() => {
  "use strict";

  const query = new URLSearchParams(window.location.search);
  const queryToken = query.get("token") || "";
  if (queryToken) window.sessionStorage.setItem("wire-launch-token", queryToken);
  const token = queryToken || window.sessionStorage.getItem("wire-launch-token") || "";
  window.history.replaceState({}, "", window.location.pathname);

  const emptyTopology = Object.freeze({
    schema: "wire-topology-v1",
    generated_at: "",
    machines: [],
    sessions: [],
    direct_links: [],
    groups: [],
    anomalies: []
  });
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
    stale: false,
    confirmedPair: []
  };
  const rows = document.querySelector("#session-rows");
  const tableWrap = document.querySelector("#table-wrap");
  const mapPanel = document.querySelector("#map-panel");
  const listPanel = document.querySelector("#list-panel");
  const topologyMap = document.querySelector("#topology-map");
  const mapInspector = document.querySelector("#map-inspector");
  const mapViewButton = document.querySelector("#map-view-button");
  const listViewButton = document.querySelector("#list-view-button");
  const loading = document.querySelector("#loading");
  const empty = document.querySelector("#empty");
  const emptyTitle = document.querySelector("#empty-title");
  const emptyCopy = document.querySelector("#empty-copy");
  const notice = document.querySelector("#notice");
  const liveCount = document.querySelector("#live-count");
  const lastScan = document.querySelector("#last-scan");
  const selectionCount = document.querySelector("#selection-count");
  const actionHint = document.querySelector("#action-hint");
  const linkButton = document.querySelector("#link-button");
  const groupButton = document.querySelector("#group-button");
  const confirmDialog = document.querySelector("#confirm-dialog");
  const confirmCopy = document.querySelector("#confirm-copy");
  const confirmLink = document.querySelector("#confirm-link");
  const groupDialog = document.querySelector("#group-dialog");
  const groupForm = document.querySelector("#group-form");
  const groupName = document.querySelector("#group-name");
  const groupCreator = document.querySelector("#group-creator");
  const searchFilter = document.querySelector("#search-filter");
  const machineFilter = document.querySelector("#machine-filter");
  const harnessFilter = document.querySelector("#harness-filter");
  const projectFilter = document.querySelector("#project-filter");
  const healthFilter = document.querySelector("#health-filter");
  const connectedFilter = document.querySelector("#connected-filter");

  const known = (value) => value === null || value === undefined || value === "" ? "Unknown" : String(value);

  const formatAge = (seconds) => {
    if (seconds === null || seconds === undefined) return "—";
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
    return `${Math.floor(seconds / 86400)}d`;
  };

  const showNotice = (message, kind = "ok") => {
    notice.textContent = message;
    notice.dataset.kind = kind;
    notice.hidden = !message;
  };

  const sessionEntries = () => Array.isArray(state.topology.sessions) ? state.topology.sessions : [];
  const allSessions = () => sessionEntries()
    .filter((entry) => entry && entry.session)
    .map((entry) => entry.session);
  const selectedSessions = () => allSessions().filter((session) => state.selected.has(session.id));

  const updateActions = () => {
    const count = state.selected.size;
    selectionCount.textContent = String(count);
    linkButton.disabled = state.busy || count !== 2 || !token;
    groupButton.disabled = state.busy || count < 2 || !token;
    if (!token) actionHint.textContent = "Launch token missing. Restart wire dash --web.";
    else if (count === 0) actionHint.textContent = "Select two sessions to link them.";
    else if (count === 1) actionHint.textContent = "Select one more session for a topology action.";
    else if (count === 2) actionHint.textContent = "Link the pair or create a shared room.";
    else actionHint.textContent = "Create one shared room for the selected sessions.";
  };

  const cell = (label, className = "") => {
    const element = document.createElement("td");
    element.dataset.label = label;
    if (className) element.className = className;
    return element;
  };

  const stack = (primary, secondary, className = "") => {
    const wrapper = document.createElement("span");
    wrapper.className = `cell-stack ${className}`.trim();
    const main = document.createElement("strong");
    main.textContent = known(primary);
    const sub = document.createElement("small");
    sub.textContent = known(secondary);
    wrapper.append(main, sub);
    return wrapper;
  };

  const detailItem = (label, value) => {
    const wrapper = document.createElement("div");
    const term = document.createElement("dt");
    const description = document.createElement("dd");
    term.textContent = label;
    description.textContent = known(value);
    wrapper.append(term, description);
    return wrapper;
  };

  const detailSection = (title, items) => {
    const section = document.createElement("section");
    const heading = document.createElement("h3");
    const list = document.createElement("dl");
    heading.textContent = title;
    for (const [label, value] of items) list.append(detailItem(label, value));
    section.append(heading, list);
    return section;
  };

  const toggleSelection = (id) => {
    if (!allSessions().some((session) => session.id === id)) return;
    if (state.selected.has(id)) state.selected.delete(id);
    else state.selected.add(id);
    render();
  };

  const option = (value, label = value) => {
    const element = document.createElement("option");
    element.value = value;
    element.textContent = label;
    return element;
  };

  const replaceOptions = (select, values, labels = new Map()) => {
    const current = select.value;
    const options = [option("", "All")];
    for (const value of [...values].sort((left, right) => left.localeCompare(right))) {
      options.push(option(value, labels.get(value) || value));
    }
    select.replaceChildren(...options);
    select.value = values.has(current) ? current : "";
  };

  const populateFilterOptions = () => {
    const entries = sessionEntries().filter((entry) => entry && entry.session);
    const machineLabels = new Map((state.topology.machines || []).map((machine) => [machine.id, machine.hostname || machine.id]));
    replaceOptions(machineFilter, new Set(entries.map((entry) => entry.machine_id).filter(Boolean)), machineLabels);
    replaceOptions(harnessFilter, new Set(entries.map((entry) => entry.session.harness?.label).filter(Boolean)));
    replaceOptions(projectFilter, new Set(entries.map((entry) => entry.session.project?.name).filter(Boolean)));
    replaceOptions(healthFilter, new Set(entries.map((entry) => entry.session.health).filter(Boolean)));
    state.filters.machine = machineFilter.value;
    state.filters.harness = harnessFilter.value;
    state.filters.project = projectFilter.value;
    state.filters.health = healthFilter.value;
  };

  const render = () => {
    const visible = window.WireTopology.visibleTopology(state.topology, state.filters);
    const sessions = visible.sessions.map((entry) => entry.session);
    const fragment = document.createDocumentFragment();

    for (const session of sessions) {
      const row = document.createElement("tr");
      row.dataset.sessionId = session.id;
      const selectCell = cell("Select");
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.className = "session-check";
      checkbox.checked = state.selected.has(session.id);
      checkbox.setAttribute("aria-label", `Select ${session.handle}`);
      checkbox.addEventListener("change", () => toggleSelection(session.id));
      selectCell.append(checkbox);

      const nameCell = cell("Session");
      const name = document.createElement("span");
      name.className = "session-name";
      const emoji = document.createElement("span");
      emoji.className = "session-emoji";
      emoji.style.color = session.primary_hex;
      emoji.textContent = session.emoji;
      const handle = document.createElement("span");
      handle.textContent = session.handle;
      const identity = document.createElement("span");
      identity.className = "session-identity";
      identity.append(handle);
      const uptime = document.createElement("small");
      uptime.textContent = `${formatAge(session.age_seconds)} · PID ${known(session.pid)}`;
      const detailsButton = document.createElement("button");
      const expanded = state.expanded.has(session.id);
      const detailId = `details-${session.id}`;
      detailsButton.type = "button";
      detailsButton.className = "details-button";
      detailsButton.textContent = expanded ? "Hide details" : "Inspect details";
      detailsButton.setAttribute("aria-expanded", String(expanded));
      detailsButton.setAttribute("aria-controls", detailId);
      detailsButton.addEventListener("click", () => {
        if (state.expanded.has(session.id)) state.expanded.delete(session.id);
        else state.expanded.add(session.id);
        render();
      });
      identity.append(uptime, detailsButton);
      name.append(emoji, identity);
      nameCell.append(name);

      const host = cell("Harness", "utility");
      host.append(stack(session.harness?.label, session.harness?.confidence));
      const project = cell("Project", "project");
      project.append(stack(session.project?.name, session.project?.branch || session.project?.relative_cwd));
      project.title = known(session.project?.cwd);
      const machine = cell("Machine", "utility");
      machine.append(stack(session.machine?.hostname, `${known(session.machine?.os)} / ${known(session.machine?.arch)}`));
      const identityCell = cell("Identity", "utility");
      const identityLabel = session.identity?.warning ? "Needs session key" : session.identity?.class;
      identityCell.append(stack(identityLabel, session.identity?.source, session.identity?.warning ? "identity-warning" : ""));
      const links = cell("Links", "link-count");
      links.textContent = String(session.direct_link_count);
      const health = cell("Signal");
      const signal = document.createElement("span");
      signal.className = `signal signal--${session.health}`;
      signal.textContent = session.health.replaceAll("-", " ");
      health.append(signal);

      row.append(selectCell, nameCell, host, project, machine, identityCell, links, health);
      fragment.append(row);

      const detailRow = document.createElement("tr");
      detailRow.id = detailId;
      detailRow.className = "detail-row";
      detailRow.hidden = !expanded;
      const detailCell = document.createElement("td");
      detailCell.colSpan = 8;
      const grid = document.createElement("div");
      grid.className = "detail-grid";
      grid.append(
        detailSection("Identity", [
          ["DID", session.did],
          ["Source", session.identity?.source],
          ["Class", session.identity?.class],
          ["Warning", session.identity?.warning]
        ]),
        detailSection("Harness", [
          ["Kind", session.harness?.kind],
          ["Launch mode", session.harness?.mode],
          ["Confidence", session.harness?.confidence],
          ["Evidence", session.harness?.evidence]
        ]),
        detailSection("Project", [
          ["Repository", session.project?.name],
          ["Root", session.project?.root],
          ["Working directory", session.project?.cwd],
          ["Relative directory", session.project?.relative_cwd],
          ["Branch", session.project?.branch],
          ["Revision", session.project?.revision],
          ["Worktree", session.project?.worktree_name],
          ["Worktree path", session.project?.worktree_path],
          ["Remote", session.project?.remote],
          ["Evidence", session.project?.evidence]
        ]),
        detailSection("Machine", [
          ["Fingerprint", session.machine?.fingerprint],
          ["Hostname", session.machine?.hostname],
          ["Operating system", session.machine?.os],
          ["Architecture", session.machine?.arch],
          ["Wire version", session.machine?.wire_version]
        ])
      );
      detailCell.append(grid);
      detailRow.append(detailCell);
      fragment.append(detailRow);
    }
    rows.replaceChildren(fragment);
    const liveSessions = allSessions();
    const hasLiveSessions = liveSessions.length !== 0;
    const hasVisibleSessions = sessions.length !== 0;
    liveCount.textContent = String(liveSessions.length);
    loading.hidden = true;
    empty.hidden = hasVisibleSessions;
    emptyTitle.textContent = hasLiveSessions ? "Filters hide all live sessions" : "No live agent sessions";
    emptyCopy.textContent = hasLiveSessions
      ? "Clear or change filters to bring sessions back into view."
      : "Start a Codex, Claude, or Goose session with Wire enabled. It will appear on the next scan.";
    mapPanel.hidden = state.activeView !== "map" || !hasVisibleSessions;
    listPanel.hidden = state.activeView !== "list" || !hasVisibleSessions;
    tableWrap.hidden = !hasVisibleSessions;
    mapViewButton.setAttribute("aria-pressed", String(state.activeView === "map"));
    listViewButton.setAttribute("aria-pressed", String(state.activeView === "list"));
    topologyMap.dataset.visibleSessionIds = sessions.map((session) => session.id).join(",");
    mapInspector.textContent = `${sessions.length} visible session${sessions.length === 1 ? "" : "s"} · ${visible.directLinks.length} direct link${visible.directLinks.length === 1 ? "" : "s"}`;
    lastScan.textContent = `Scan ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`;
    updateActions();
  };

  const scan = () => {
    if (state.scanPromise) return state.scanPromise;
    state.scanPromise = (async () => {
      try {
        const response = await fetch("/api/topology", {
          cache: "no-store",
          headers: { "X-Wire-Token": token }
        });
        if (!response.ok) throw new Error("Could not refresh topology.");
        const topology = await response.json();
        const wasStale = state.stale;
        state.topology = topology && typeof topology === "object" ? topology : emptyTopology;
        const liveIds = new Set(allSessions().map((session) => session.id));
        state.selected = new Set([...state.selected].filter((id) => liveIds.has(id)));
        state.expanded = new Set([...state.expanded].filter((id) => liveIds.has(id)));
        state.stale = false;
        populateFilterOptions();
        if (wasStale) showNotice(token ? "" : "Launch token missing. Restart wire dash --web.", token ? "ok" : "error");
        render();
      } catch (error) {
        state.stale = true;
        loading.hidden = true;
        const failedAt = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
        lastScan.textContent = `Refresh failed ${failedAt} · showing stale data`;
        showNotice(`${error.message || "Topology refresh failed."} Showing the last known topology.`, "error");
        updateActions();
      }
    })().finally(() => { state.scanPromise = null; });
    return state.scanPromise;
  };

  const mutate = async (path, body) => {
    state.busy = true;
    updateActions();
    try {
      const response = await fetch(path, {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-Wire-Token": token },
        body: JSON.stringify(body)
      });
      const payload = await response.json();
      if (!response.ok) throw new Error(payload.error || "Topology action failed.");
      showNotice(payload.message || "Topology updated.");
      state.selected.clear();
      await scan();
    } catch (error) {
      showNotice(error.message || "Topology action failed.", "error");
    } finally {
      state.busy = false;
      updateActions();
    }
  };

  linkButton.addEventListener("click", () => {
    const selected = selectedSessions();
    if (selected.length !== 2) return;
    state.confirmedPair = selected.map((session) => session.id);
    confirmCopy.textContent = `${selected[0].handle} and ${selected[1].handle} will trust each other on this machine.`;
    confirmDialog.showModal();
  });

  confirmLink.addEventListener("click", (event) => {
    event.preventDefault();
    const liveIds = new Set(allSessions().map((session) => session.id));
    const sessions = [...state.confirmedPair];
    confirmDialog.close();
    if (sessions.length !== 2 || sessions.some((id) => !liveIds.has(id))) {
      showNotice("One of those sessions is no longer live. Select the pair again.", "error");
      state.confirmedPair = [];
      return;
    }
    state.confirmedPair = [];
    void mutate("/api/links", { sessions });
  });

  groupButton.addEventListener("click", () => {
    const selected = selectedSessions();
    if (selected.length < 2) return;
    const options = selected.map((session) => {
      const option = document.createElement("option");
      option.value = session.id;
      option.textContent = session.handle;
      return option;
    });
    groupCreator.replaceChildren(...options);
    groupDialog.showModal();
    groupName.focus();
  });

  groupForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!groupName.reportValidity()) return;
    const members = selectedSessions().map((session) => session.id);
    const body = { name: groupName.value.trim(), creator: groupCreator.value, members };
    groupDialog.close();
    groupForm.reset();
    void mutate("/api/groups", body);
  });

  const setView = (view) => {
    state.activeView = view;
    render();
  };
  mapViewButton.addEventListener("click", () => setView("map"));
  listViewButton.addEventListener("click", () => setView("list"));
  topologyMap.addEventListener("wire:toggle-selection", (event) => toggleSelection(event.detail?.id));

  const bindFilter = (element, key, eventName = "change") => {
    element.addEventListener(eventName, () => {
      state.filters[key] = element.value;
      render();
    });
  };
  bindFilter(searchFilter, "search", "input");
  bindFilter(machineFilter, "machine");
  bindFilter(harnessFilter, "harness");
  bindFilter(projectFilter, "project");
  bindFilter(healthFilter, "health");
  connectedFilter.addEventListener("change", () => {
    state.filters.connectedOnly = connectedFilter.checked;
    render();
  });
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape" || state.selected.size === 0) return;
    state.selected.clear();
    render();
  });

  if (!token) showNotice("Launch token missing. Restart wire dash --web.", "error");
  void scan();
  window.setInterval(() => { if (!state.busy) void scan(); }, 2000);
})();
