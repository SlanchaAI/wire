(() => {
  "use strict";

  const query = new URLSearchParams(window.location.search);
  const token = query.get("token") || "";
  window.history.replaceState({}, "", window.location.pathname);

  const state = { sessions: [], selected: new Set(), busy: false };
  const rows = document.querySelector("#session-rows");
  const tableWrap = document.querySelector("#table-wrap");
  const loading = document.querySelector("#loading");
  const empty = document.querySelector("#empty");
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

  const hostLabel = (source) => ({
    "codex-cli": "Codex thread",
    "claude-code": "Claude thread",
    "claude-code-pidfile": "Claude thread",
    "goose": "Goose thread",
    "copilot-cli": "Copilot thread",
    "vscode-workspace": "VS Code workspace",
    "override": "Pinned session"
  }[source] || source || "Agent session");

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

  const selectedSessions = () => state.sessions.filter((session) => state.selected.has(session.id));

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

  const render = () => {
    const liveIds = new Set(state.sessions.map((session) => session.id));
    state.selected = new Set([...state.selected].filter((id) => liveIds.has(id)));
    const fragment = document.createDocumentFragment();

    for (const session of state.sessions) {
      const row = document.createElement("tr");
      const selectCell = cell("Select");
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.className = "session-check";
      checkbox.checked = state.selected.has(session.id);
      checkbox.setAttribute("aria-label", `Select ${session.handle}`);
      checkbox.addEventListener("change", () => {
        if (checkbox.checked) state.selected.add(session.id);
        else state.selected.delete(session.id);
        updateActions();
      });
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
      name.append(emoji, handle);
      nameCell.append(name);

      const host = cell("Agent", "utility");
      host.textContent = hostLabel(session.agent_host);
      const project = cell("Project", "project");
      project.textContent = session.project_dir || "—";
      project.title = session.project_dir || "";
      const age = cell("Uptime", "utility");
      age.textContent = formatAge(session.age_seconds);
      const links = cell("Links", "link-count");
      links.textContent = String(session.direct_link_count);
      const health = cell("Signal");
      const signal = document.createElement("span");
      signal.className = `signal signal--${session.health}`;
      signal.textContent = session.health.replaceAll("-", " ");
      health.append(signal);

      row.append(selectCell, nameCell, host, project, age, links, health);
      fragment.append(row);
    }
    rows.replaceChildren(fragment);
    liveCount.textContent = String(state.sessions.length);
    loading.hidden = true;
    empty.hidden = state.sessions.length !== 0;
    tableWrap.hidden = state.sessions.length === 0;
    lastScan.textContent = `Scan ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`;
    updateActions();
  };

  const scan = async () => {
    try {
      const response = await fetch("/api/sessions", { cache: "no-store" });
      if (!response.ok) throw new Error("Could not read live sessions.");
      const report = await response.json();
      state.sessions = Array.isArray(report.sessions) ? report.sessions : [];
      render();
    } catch (error) {
      loading.hidden = true;
      showNotice(error.message || "Session scan failed.", "error");
    }
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
    confirmCopy.textContent = `${selected[0].handle} and ${selected[1].handle} will trust each other on this machine.`;
    confirmDialog.showModal();
  });

  confirmLink.addEventListener("click", (event) => {
    event.preventDefault();
    const sessions = selectedSessions().map((session) => session.id);
    confirmDialog.close();
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

  if (!token) showNotice("Launch token missing. Restart wire dash --web.", "error");
  void scan();
  window.setInterval(() => { if (!state.busy) void scan(); }, 2000);
})();
