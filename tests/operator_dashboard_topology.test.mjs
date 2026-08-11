import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

class ElementStub {
  constructor(id = "") {
    this.id = id;
    this.children = [];
    this.listeners = new Map();
    this.attributes = new Map();
    this.classList = { toggle() {} };
    this.dataset = {};
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    this.value = "";
    this.ownerDocument = null;
    this.resetCount = 0;
    this._textContent = "";
    this.style = {
      values: new Map(),
      setProperty(name, value) { this.values.set(name, String(value)); },
      getPropertyValue(name) { return this.values.get(name) || ""; }
    };
    this.clientWidth = 800;
    this.clientHeight = 520;
  }

  get textContent() {
    return this._textContent + this.children.map((child) => child && child.textContent || "").join("");
  }

  set textContent(value) {
    this._textContent = String(value);
    this.children = [];
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) || [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  dispatch(type, detail = {}) {
    const event = {
      defaultPrevented: false,
      preventDefault() { this.defaultPrevented = true; },
      target: this,
      type,
      ...detail
    };
    for (const listener of this.listeners.get(type) || []) listener(event);
    return event;
  }

  dispatchEvent(event) { return !this.dispatch(event.type, event).defaultPrevented; }

  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this._textContent = ""; this.children = children; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  showModal() { this.open = true; }
  close() { this.open = false; }
  focus() {
    this.focused = true;
    if (this.ownerDocument) this.ownerDocument.activeElement = this;
  }
  getBoundingClientRect() { return { left: 0, top: 0, width: this.clientWidth, height: this.clientHeight }; }
  setPointerCapture() {}
  releasePointerCapture() {}
  reset() { this.resetCount += 1; }
  reportValidity() { return true; }
}

const entry = (id, { machineId = "machine-a", harness = "Codex CLI", project = "Wire", health = "healthy" } = {}) => ({
  machine_id: machineId,
  session: {
    id,
    did: `did:wire:${id}-00000001`,
    handle: `${id}-handle`,
    health,
    harness: { label: harness },
    project: { name: project, branch: "main" },
    machine: { hostname: machineId, os: "macos", arch: "aarch64" },
    identity: { class: "session-keyed", source: "wire-session-id" },
    direct_link_count: 0,
    age_seconds: 3,
    pid: 42,
    emoji: "◆",
    primary_hex: "#5b1a2e"
  }
});

const snapshot = () => ({
  schema: "wire-topology-v1",
  generated_at: "2026-08-10T20:00:00Z",
  machines: [
    { id: "machine-a", hostname: "alpha", os: "macos", arch: "aarch64", identity_confidence: "verified" },
    { id: "machine-b", hostname: "bravo", os: "linux", arch: "x86_64", identity_confidence: "unverified" }
  ],
  sessions: [
    entry("amber"),
    entry("bravo", { machineId: "machine-b", harness: "Claude Code", project: "Studio", health: "sync-stale" }),
    entry("cedar", { machineId: "machine-b", harness: "Goose Shell", project: "Wire" }),
    entry("delta")
  ],
  direct_links: [
    {
      id: "amber-bravo",
      source_did: "did:wire:amber-00000001",
      target_did: "did:wire:bravo-00000001",
      state: "bilateral"
    },
    {
      id: "cedar-delta",
      source_did: "did:wire:cedar-00000001",
      target_did: "did:wire:delta-00000001",
      state: "one-sided"
    }
  ],
  groups: [{
    id: "crew",
    name: "Crew",
    members: ["amber", "bravo", "cedar", "delta"].map((id) => ({
      did: `did:wire:${id}-00000001`, live: true, tier: id === "amber" ? "creator" : "member"
    }))
  }],
  anomalies: []
});

const flush = () => new Promise((resolve) => setImmediate(resolve));

const selectors = [
  "#session-rows", "#table-wrap", "#map-panel", "#list-panel", "#topology-map", "#map-inspector", "#fit-map",
  "#map-view-button", "#list-view-button", "#loading", "#empty", "#empty-title", "#empty-copy",
  "#notice", "#live-count", "#last-scan", "#selection-count", "#action-hint", "#link-button",
  "#group-button", "#confirm-dialog", "#confirm-copy", "#confirm-link", "#group-dialog", "#group-form",
  "#group-name", "#group-creator", "#cancel-group", "#search-filter", "#machine-filter", "#harness-filter",
  "#project-filter", "#health-filter", "#connected-filter"
];

const dashboard = async ({ token = "test-token", fetchImpl, DateImpl = Date } = {}) => {
  const elements = new Map(selectors.map((selector) => [selector, new ElementStub(selector.slice(1))]));
  const created = [];
  const requests = [];
  const intervals = [];
  const document = new ElementStub("document");
  document.activeElement = null;
  for (const element of elements.values()) element.ownerDocument = document;
  document.querySelector = (selector) => {
    if (!elements.has(selector)) throw new Error(`Unexpected dashboard selector: ${selector}`);
    return elements.get(selector);
  };
  document.createElement = (tagName) => {
    const element = new ElementStub();
    element.tagName = tagName.toUpperCase();
    element.ownerDocument = document;
    created.push(element);
    return element;
  };
  document.createElementNS = (namespaceURI, tagName) => {
    const element = document.createElement(tagName);
    element.namespaceURI = namespaceURI;
    return element;
  };
  document.createDocumentFragment = () => new ElementStub("fragment");
  document.getElementById = (id) => elements.get(`#${id}`) || null;

  const storage = new Map();
  const window = {
    location: { search: token ? `?token=${token}` : "", pathname: "/" },
    history: { replaceState() {} },
    sessionStorage: {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value)
    },
    setInterval: (callback) => intervals.push(callback)
  };
  class CustomEvent {
    constructor(type, options = {}) {
      this.type = type;
      this.detail = options.detail;
    }
  }
  const context = {
    CustomEvent,
    Date: DateImpl,
    URLSearchParams,
    console,
    document,
    fetch: async (path, options) => {
      requests.push({ path, options });
      return fetchImpl
        ? fetchImpl(path, options)
        : { ok: true, json: async () => snapshot() };
    },
    window
  };
  vm.runInNewContext(readFileSync(new URL("../assets/operator-topology.js", import.meta.url), "utf8"), context);
  vm.runInNewContext(readFileSync(new URL("../assets/operator-dashboard.js", import.meta.url), "utf8"), context);
  await flush();
  return { created, document, elements, intervals, requests };
};

const descendants = (element) => element.children.flatMap((child) => [child, ...descendants(child)]);
const classNames = (element) => (element.attributes.get("class") || "").split(/\s+/).filter(Boolean);
const withClass = (element, className) => descendants(element).filter((candidate) => classNames(candidate).includes(className));
const mapNode = (page, id) => withClass(page.elements.get("#topology-map"), "topology-node")
  .find((node) => node.dataset.sessionId === id);

const renderedRows = (page) => {
  const fragment = page.elements.get("#session-rows").children[0];
  return fragment ? fragment.children.filter((element) => element.dataset.sessionId) : [];
};

const checkboxFor = (page, handle) => renderedRows(page)
  .map((row) => row.children[0].children[0])
  .find((element) => element.attributes.get("aria-label") === `Select ${handle}`);

const inspectFor = (page, id) => descendants(
  renderedRows(page).find((row) => row.dataset.sessionId === id)
).find((element) => element.className === "details-button");

test("map selection survives List and Map view changes", async () => {
  const page = await dashboard();
  const map = page.elements.get("#topology-map");
  const mapButton = page.elements.get("#map-view-button");
  const listButton = page.elements.get("#list-view-button");

  map.dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  listButton.dispatch("click");
  assert.equal(checkboxFor(page, "amber-handle").checked, true);
  assert.equal(page.elements.get("#map-panel").hidden, true);
  assert.equal(page.elements.get("#list-panel").hidden, false);
  mapButton.dispatch("click");

  assert.equal(page.elements.get("#selection-count").textContent, "1");
  assert.equal(page.elements.get("#map-panel").hidden, false);
  assert.equal(page.elements.get("#list-panel").hidden, true);
  assert.equal(mapButton.attributes.get("aria-pressed"), "true");
  assert.equal(listButton.attributes.get("aria-pressed"), "false");
});

test("map renders labeled machines, group fragments, direct edges, and accessible session nodes in paint order", async () => {
  const page = await dashboard();
  const map = page.elements.get("#topology-map");
  const svg = map.children[0];
  const viewport = svg.children[0];

  assert.equal(svg.tagName, "SVG");
  assert.equal(svg.attributes.get("role"), "group", "the SVG root must preserve descendant button semantics");
  assert.deepEqual(viewport.children.map((layer) => layer.dataset.layer), ["machines", "groups", "edges", "nodes"]);
  assert.equal(withClass(viewport.children[0], "topology-machine").length, 2);
  assert.match(withClass(viewport.children[0], "topology-machine")[0].attributes.get("aria-label"), /alpha.*verified/i);
  assert.match(withClass(viewport.children[0], "topology-machine")[1].attributes.get("aria-label"), /bravo.*unverified/i);

  const groupFragments = withClass(viewport.children[1], "topology-group");
  assert.equal(groupFragments.length, 2, "a cross-machine group paints one fragment in each machine");
  assert.ok(groupFragments.every((fragment) => /Crew/.test(fragment.textContent)));

  const edges = withClass(viewport.children[2], "topology-edge");
  assert.equal(edges.length, 2, "group membership must not synthesize direct edges");
  assert.ok(classNames(edges[0]).includes("topology-edge--bilateral"));
  assert.ok(classNames(edges[1]).includes("topology-edge--one-sided"));

  const nodes = withClass(viewport.children[3], "topology-node");
  assert.equal(nodes.length, 4);
  for (const node of nodes) {
    assert.equal(node.attributes.get("role"), "button");
    assert.equal(node.attributes.get("tabindex"), "0");
    assert.equal(node.attributes.get("aria-pressed"), "false");
    assert.equal(withClass(node, "topology-health-ring").length, 1);
  }
  assert.match(mapNode(page, "amber").textContent, /◆.*amber-handle.*Codex CLI/);
});

test("click, Enter, and Space emit shared map selection and keep the node keyboard reachable", async () => {
  const page = await dashboard();

  mapNode(page, "amber").dispatch("click");
  assert.equal(page.elements.get("#selection-count").textContent, "1");
  assert.equal(mapNode(page, "amber").attributes.get("aria-pressed"), "true");
  assert.match(page.elements.get("#map-inspector").textContent, /amber-handle.*Codex CLI.*machine-a/i);

  const enter = mapNode(page, "bravo").dispatch("keydown", { key: "Enter" });
  assert.equal(enter.defaultPrevented, true);
  assert.equal(page.elements.get("#selection-count").textContent, "2");
  assert.equal(mapNode(page, "bravo").focused, true);

  const space = mapNode(page, "bravo").dispatch("keydown", { key: " " });
  assert.equal(space.defaultPrevented, true);
  assert.equal(page.elements.get("#selection-count").textContent, "1");
  assert.equal(mapNode(page, "bravo").attributes.get("aria-pressed"), "false");
});

test("Fit map restores the helper-derived viewport after zoom and pan changes", async () => {
  const page = await dashboard();
  const map = page.elements.get("#topology-map");
  const fit = page.elements.get("#fit-map");
  const transform = () => map.children[0].children[0].attributes.get("transform");
  const fitted = transform();

  map.dispatch("wheel", { deltaY: -120, clientX: 400, clientY: 260 });
  map.dispatch("pointerdown", { pointerId: 7, clientX: 100, clientY: 100 });
  map.dispatch("pointermove", { pointerId: 7, clientX: 160, clientY: 140 });
  map.dispatch("pointerup", { pointerId: 7 });
  assert.notEqual(transform(), fitted);

  fit.dispatch("click");
  assert.equal(transform(), fitted);
});

test("Fit map can use a helper scale below the interactive wheel minimum for crowded topology", async () => {
  const crowded = snapshot();
  crowded.machines = crowded.machines.slice(0, 1);
  crowded.sessions = Array.from({ length: 40 }, (_, index) => entry(`session-${String(index).padStart(2, "0")}`));
  crowded.direct_links = [];
  crowded.groups = [];
  const page = await dashboard({ fetchImpl: async () => ({ ok: true, json: async () => crowded }) });
  const transform = page.elements.get("#topology-map").children[0].children[0].attributes.get("transform");
  const scale = Number(transform.match(/scale\(([^)]+)\)/)[1]);

  assert.ok(scale < 0.35, `expected Fit scale below wheel floor, got ${scale}`);
  assert.ok(scale > 0);
});

test("wheel zoom stays within the fitted lower bound and interactive upper bound", async () => {
  const crowded = snapshot();
  crowded.machines = crowded.machines.slice(0, 1);
  crowded.sessions = Array.from({ length: 40 }, (_, index) => entry(`session-${String(index).padStart(2, "0")}`));
  crowded.direct_links = [];
  crowded.groups = [];
  const page = await dashboard({ fetchImpl: async () => ({ ok: true, json: async () => crowded }) });
  const map = page.elements.get("#topology-map");
  const scale = () => Number(map.children[0].children[0].attributes.get("transform").match(/scale\(([^)]+)\)/)[1]);
  const fitted = scale();
  assert.ok(fitted < 0.35);

  map.dispatch("wheel", { deltaY: 100_000, clientX: 400, clientY: 260 });
  assert.ok(scale() <= fitted, `zooming out from ${fitted} must not jump in to ${scale()}`);

  map.dispatch("wheel", { deltaY: -100_000, clientX: 400, clientY: 260 });
  assert.equal(scale(), 2.5);
});

test("selection count enables Link for exactly two and Create group for two or more", async () => {
  const page = await dashboard();
  const map = page.elements.get("#topology-map");
  const link = page.elements.get("#link-button");
  const group = page.elements.get("#group-button");

  map.dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  assert.equal(link.disabled, true);
  assert.equal(group.disabled, true);
  map.dispatch("wire:toggle-selection", { detail: { id: "bravo" } });
  assert.equal(link.disabled, false);
  assert.equal(group.disabled, false);
  map.dispatch("wire:toggle-selection", { detail: { id: "cedar" } });
  assert.equal(link.disabled, true);
  assert.equal(group.disabled, false);
});

test("Escape clears the shared selection", async () => {
  const page = await dashboard();
  page.elements.get("#topology-map").dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  page.document.dispatch("keydown", { key: "Escape" });

  assert.equal(page.elements.get("#selection-count").textContent, "0");
  assert.equal(checkboxFor(page, "amber-handle").checked, false);
});

test("filter changes render List and Map from the same visible topology", async () => {
  const page = await dashboard();
  const search = page.elements.get("#search-filter");
  search.value = "amber";
  search.dispatch("input");

  const visibleRows = renderedRows(page);
  assert.deepEqual(visibleRows.map((row) => row.dataset.sessionId), ["amber"]);
  assert.match(page.elements.get("#map-inspector").textContent, /1 visible session/i);

  search.value = "missing";
  search.dispatch("input");
  assert.match(page.elements.get("#empty-title").textContent, /filters/i);
  assert.equal(page.elements.get("#empty").hidden, false);
});

test("successful scan repopulates filter options from the unfiltered snapshot", async () => {
  const page = await dashboard();
  const values = (selector) => page.elements.get(selector).children.map((option) => option.value);

  assert.deepEqual(values("#machine-filter"), ["", "machine-a", "machine-b"]);
  assert.deepEqual(values("#harness-filter"), ["", "Claude Code", "Codex CLI", "Goose Shell"]);
  assert.deepEqual(values("#project-filter"), ["", "Studio", "Wire"]);
  assert.deepEqual(values("#health-filter"), ["", "healthy", "sync-stale"]);
  assert.equal(page.requests[0].path, "/api/topology");
  assert.equal(page.requests[0].options.headers["X-Wire-Token"], "test-token");
});

test("successful inventory refresh does not hide the missing launch-token warning", async () => {
  const page = await dashboard({ token: "" });

  assert.equal(page.elements.get("#notice").dataset.kind, "error");
  assert.match(page.elements.get("#notice").textContent, /launch token missing/i);
  assert.equal(page.elements.get("#link-button").disabled, true);
  assert.equal(page.elements.get("#group-button").disabled, true);
});

test("interaction after a failed refresh preserves the stale scan timestamp", async () => {
  const responses = [
    { ok: true, json: async () => snapshot() },
    { ok: false, json: async () => ({}) },
    { ok: true, json: async () => snapshot() }
  ];
  const page = await dashboard({ fetchImpl: async () => responses.shift() });

  assert.equal(page.intervals.length, 1);
  page.intervals[0]();
  await flush();
  const failedLabel = page.elements.get("#last-scan").textContent;
  assert.match(failedLabel, /^Refresh failed .* · showing stale data$/);

  const search = page.elements.get("#search-filter");
  search.value = "amber";
  search.dispatch("input");

  assert.equal(page.elements.get("#last-scan").textContent, failedLabel);

  page.intervals[0]();
  await flush();
  const recoveredLabel = page.elements.get("#last-scan").textContent;
  assert.match(recoveredLabel, /^Scan /);
  assert.notEqual(recoveredLabel, failedLabel);
  assert.equal(responses.length, 0);
});

test("successful scan timestamp remains stable across selection and filter renders", async () => {
  let tick = 0;
  class TickDate {
    toLocaleTimeString() { return `time-${++tick}`; }
  }
  const page = await dashboard({ DateImpl: TickDate });
  const initial = page.elements.get("#last-scan").textContent;

  page.elements.get("#topology-map").dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  assert.equal(page.elements.get("#last-scan").textContent, initial);

  const search = page.elements.get("#search-filter");
  search.value = "amber";
  search.dispatch("input");
  assert.equal(page.elements.get("#last-scan").textContent, initial);
  assert.equal(tick, 1, "only the successful scan records a time");
});

test("successful polls restore focus to stable map, checkbox, and Inspect controls", async () => {
  const page = await dashboard();

  const oldMapNode = mapNode(page, "amber");
  oldMapNode.focus();
  page.intervals[0]();
  await flush();
  assert.notEqual(mapNode(page, "amber"), oldMapNode);
  assert.equal(page.document.activeElement, mapNode(page, "amber"));

  page.elements.get("#list-view-button").dispatch("click");
  const oldCheckbox = checkboxFor(page, "amber-handle");
  oldCheckbox.focus();
  page.intervals[0]();
  await flush();
  assert.notEqual(checkboxFor(page, "amber-handle"), oldCheckbox);
  assert.equal(page.document.activeElement, checkboxFor(page, "amber-handle"));

  const oldInspect = inspectFor(page, "amber");
  oldInspect.focus();
  page.intervals[0]();
  await flush();
  assert.notEqual(inspectFor(page, "amber"), oldInspect);
  assert.equal(page.document.activeElement, inspectFor(page, "amber"));
});

test("group Cancel closes and resets the dialog without a POST", async () => {
  const page = await dashboard();
  const map = page.elements.get("#topology-map");
  map.dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  map.dispatch("wire:toggle-selection", { detail: { id: "bravo" } });
  page.elements.get("#group-button").dispatch("click");
  page.elements.get("#group-name").value = "crew";
  page.elements.get("#group-creator").value = "amber";

  page.elements.get("#cancel-group").dispatch("click");

  assert.equal(page.elements.get("#group-dialog").open, false);
  assert.equal(page.elements.get("#group-form").resetCount, 1);
  assert.equal(page.requests.filter((request) => request.options?.method === "POST").length, 0);
});

test("group submit aborts with zero POST when a successful poll changes confirmed members", async () => {
  let topologyFetches = 0;
  let finishPoll;
  const page = await dashboard({
    fetchImpl: async (path) => {
      if (path === "/api/groups") {
        return { ok: true, json: async () => ({ ok: true, message: "created", changed_sessions: [] }) };
      }
      topologyFetches += 1;
      if (topologyFetches === 1) return { ok: true, json: async () => snapshot() };
      return new Promise((resolve) => {
        const changed = snapshot();
        changed.sessions = changed.sessions.filter((item) => item.session.id !== "bravo");
        finishPoll = () => resolve({ ok: true, json: async () => changed });
      });
    }
  });
  const map = page.elements.get("#topology-map");
  map.dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  map.dispatch("wire:toggle-selection", { detail: { id: "bravo" } });
  page.elements.get("#group-button").dispatch("click");
  page.elements.get("#group-name").value = "crew";
  page.elements.get("#group-creator").value = "amber";

  page.intervals[0]();
  finishPoll();
  await flush();
  page.elements.get("#group-form").dispatch("submit");

  assert.equal(page.requests.filter((request) => request.path === "/api/groups").length, 0);
  assert.match(page.elements.get("#notice").textContent, /changed|no longer live|select again/i);
});

test("mutation waits for a pre-action poll and starts a fresh post-action scan", async () => {
  let topologyFetches = 0;
  let finishPoll;
  const page = await dashboard({
    fetchImpl: async (path) => {
      if (path === "/api/links") {
        return { ok: true, json: async () => ({ ok: true, message: "linked", changed_sessions: ["amber", "bravo"] }) };
      }
      topologyFetches += 1;
      if (topologyFetches === 2) {
        return new Promise((resolve) => {
          finishPoll = () => resolve({ ok: true, json: async () => snapshot() });
        });
      }
      return { ok: true, json: async () => snapshot() };
    }
  });
  const map = page.elements.get("#topology-map");
  map.dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  map.dispatch("wire:toggle-selection", { detail: { id: "bravo" } });
  page.intervals[0]();
  page.elements.get("#link-button").dispatch("click");
  page.elements.get("#confirm-link").dispatch("click");

  assert.equal(page.requests.filter((request) => request.path === "/api/links").length, 0);
  finishPoll();
  await flush();
  await flush();

  assert.deepEqual(
    page.requests.map((request) => request.path),
    ["/api/topology", "/api/topology", "/api/links", "/api/topology"]
  );
});

test("link conflicts trigger a fresh topology scan", async () => {
  const page = await dashboard({
    fetchImpl: async (path) => path === "/api/links"
      ? { ok: false, json: async () => ({ error: "session vanished", changed_sessions: [] }) }
      : { ok: true, json: async () => snapshot() }
  });
  const map = page.elements.get("#topology-map");
  map.dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  map.dispatch("wire:toggle-selection", { detail: { id: "bravo" } });
  page.elements.get("#link-button").dispatch("click");
  page.elements.get("#confirm-link").dispatch("click");
  await flush();
  await flush();

  assert.equal(page.requests.filter((request) => request.path === "/api/topology").length, 2);
  assert.match(page.elements.get("#notice").textContent, /session vanished/i);
});

test("group partial failures refresh and expose changed sessions as text", async () => {
  const changed = "<b>bravo</b>";
  const page = await dashboard({
    fetchImpl: async (path) => path === "/api/groups"
      ? { ok: false, json: async () => ({ error: "join failed", changed_sessions: ["amber", changed] }) }
      : { ok: true, json: async () => snapshot() }
  });
  const map = page.elements.get("#topology-map");
  map.dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  map.dispatch("wire:toggle-selection", { detail: { id: "bravo" } });
  page.elements.get("#group-button").dispatch("click");
  page.elements.get("#group-name").value = "crew";
  page.elements.get("#group-creator").value = "amber";
  page.elements.get("#group-creator").dispatch("change");
  page.elements.get("#group-form").dispatch("submit");
  await flush();
  await flush();

  assert.equal(page.requests.filter((request) => request.path === "/api/topology").length, 2);
  assert.match(page.elements.get("#notice").textContent, /amber.*<b>bravo<\/b>/);
  assert.equal(page.created.some((element) => element.tagName === "B"), false);
});

test("safe topology anomalies retain the conflicting group subject ID in the inspector", async () => {
  const subject = "crew-<img src=x>";
  const topology = snapshot();
  topology.anomalies = [{
    kind: "conflicting-group",
    subject_id: subject,
    message: "Live sessions disagree about the highest group roster"
  }];
  const page = await dashboard({ fetchImpl: async () => ({ ok: true, json: async () => topology }) });

  assert.match(page.elements.get("#map-inspector").textContent, /Topology anomalies/);
  assert.match(page.elements.get("#map-inspector").textContent, /crew-<img src=x>/);
  page.elements.get("#topology-map").dispatch("wire:toggle-selection", { detail: { id: "amber" } });
  assert.match(page.elements.get("#map-inspector").textContent, /crew-<img src=x>/);
  assert.equal(page.created.some((element) => element.tagName === "IMG"), false);
});
