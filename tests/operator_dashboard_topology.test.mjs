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
    this.textContent = "";
    this.style = {};
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) || [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  dispatch(type, detail = {}) {
    const event = { preventDefault() {}, target: this, ...detail };
    for (const listener of this.listeners.get(type) || []) listener(event);
  }

  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  showModal() { this.open = true; }
  close() { this.open = false; }
  focus() {}
  reset() {}
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
    { id: "machine-a", hostname: "alpha", os: "macos", arch: "aarch64" },
    { id: "machine-b", hostname: "bravo", os: "linux", arch: "x86_64" }
  ],
  sessions: [
    entry("amber"),
    entry("bravo", { machineId: "machine-b", harness: "Claude Code", project: "Studio", health: "sync-stale" }),
    entry("cedar", { machineId: "machine-b", harness: "Goose Shell", project: "Wire" })
  ],
  direct_links: [{
    id: "amber-bravo",
    source_did: "did:wire:amber-00000001",
    target_did: "did:wire:bravo-00000001",
    state: "bilateral"
  }],
  groups: [],
  anomalies: []
});

const flush = () => new Promise((resolve) => setImmediate(resolve));

const selectors = [
  "#session-rows", "#table-wrap", "#map-panel", "#list-panel", "#topology-map", "#map-inspector",
  "#map-view-button", "#list-view-button", "#loading", "#empty", "#empty-title", "#empty-copy",
  "#notice", "#live-count", "#last-scan", "#selection-count", "#action-hint", "#link-button",
  "#group-button", "#confirm-dialog", "#confirm-copy", "#confirm-link", "#group-dialog", "#group-form",
  "#group-name", "#group-creator", "#search-filter", "#machine-filter", "#harness-filter",
  "#project-filter", "#health-filter", "#connected-filter"
];

const dashboard = async ({ token = "test-token", fetchImpl } = {}) => {
  const elements = new Map(selectors.map((selector) => [selector, new ElementStub(selector.slice(1))]));
  const created = [];
  const requests = [];
  const intervals = [];
  const document = new ElementStub("document");
  document.querySelector = (selector) => {
    if (!elements.has(selector)) throw new Error(`Unexpected dashboard selector: ${selector}`);
    return elements.get(selector);
  };
  document.createElement = (tagName) => {
    const element = new ElementStub();
    element.tagName = tagName.toUpperCase();
    created.push(element);
    return element;
  };
  document.createDocumentFragment = () => new ElementStub("fragment");

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
  const context = {
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

const renderedRows = (page) => {
  const fragment = page.elements.get("#session-rows").children[0];
  return fragment ? fragment.children.filter((element) => element.dataset.sessionId) : [];
};

const checkboxFor = (page, handle) => renderedRows(page)
  .map((row) => row.children[0].children[0])
  .find((element) => element.attributes.get("aria-label") === `Select ${handle}`);

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
