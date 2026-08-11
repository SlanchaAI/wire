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
    this._textContent = "";
    this.style = {};
  }

  get textContent() {
    return this._textContent + this.children.map((child) => child?.textContent || "").join("");
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
    const event = { preventDefault() {}, target: this, ...detail };
    for (const listener of this.listeners.get(type) || []) listener(event);
  }

  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this._textContent = ""; this.children = children; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  showModal() { this.open = true; }
  close() { this.open = false; }
  focus() {}
  reset() {}
  reportValidity() { return true; }
}

const emptyTopology = () => ({
  schema: "wire-topology-v1",
  generated_at: "2026-08-10T20:00:00Z",
  machines: [], sessions: [], direct_links: [], groups: [], anomalies: []
});

const sessionEntry = (id) => ({
  machine_id: "machine-a",
  session: {
    id,
    did: `did:wire:${id}-00000001`,
    handle: `${id}-handle`,
    health: "healthy",
    harness: { label: "Codex CLI" },
    project: { name: "Wire" },
    machine: { hostname: "alpha", os: "macos", arch: "aarch64" },
    identity: { class: "session-keyed", source: "wire-session-id" },
    direct_link_count: 0,
    age_seconds: 3,
    pid: 42,
    emoji: "◆",
    primary_hex: "#5b1a2e"
  }
});

const flush = () => new Promise((resolve) => setImmediate(resolve));

const selectors = [
  "#session-rows", "#table-wrap", "#map-panel", "#list-panel", "#topology-map", "#map-inspector",
  "#map-view-button", "#list-view-button", "#loading", "#empty", "#empty-title", "#empty-copy",
  "#notice", "#live-count", "#last-scan", "#selection-count", "#action-hint", "#link-button",
  "#group-button", "#confirm-dialog", "#confirm-copy", "#confirm-link", "#group-dialog", "#group-form",
  "#group-name", "#group-creator", "#cancel-group", "#search-filter", "#machine-filter", "#harness-filter",
  "#project-filter", "#health-filter", "#connected-filter"
];

const dashboard = ({ fetch }) => {
  const intervals = [];
  const elements = new Map(selectors.map((selector) => [selector, new ElementStub(selector.slice(1))]));
  const created = [];
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
    location: { search: "?token=test-token", pathname: "/" },
    history: { replaceState() {} },
    sessionStorage: {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value)
    },
    setInterval: (callback) => intervals.push(callback)
  };
  const context = { URLSearchParams, console, document, fetch, window };
  vm.runInNewContext(readFileSync(new URL("../assets/operator-topology.js", import.meta.url), "utf8"), context);
  vm.runInNewContext(readFileSync(new URL("../assets/operator-dashboard.js", import.meta.url), "utf8"), context);
  return { created, document, elements, intervals };
};

test("initial load fetches one topology snapshot and unfinished poll ticks coalesce", async () => {
  let fetchCalls = 0;
  let finishFetch;
  const paths = [];
  const page = dashboard({
    fetch: (path) => {
      fetchCalls += 1;
      paths.push(path);
      return new Promise((resolve) => {
        finishFetch = () => resolve({ ok: true, json: async () => emptyTopology() });
      });
    }
  });

  assert.equal(fetchCalls, 1, "initial page load starts one scan");
  assert.deepEqual(paths, ["/api/topology"]);
  assert.equal(page.intervals.length, 1);
  for (let index = 0; index < 4; index += 1) page.intervals[0]();
  assert.equal(fetchCalls, 1, "poll ticks must coalesce behind the unfinished scan");

  finishFetch();
  await flush();
  page.intervals[0]();
  assert.equal(fetchCalls, 2, "polling must resume after the prior scan settles");
});

test("failed refresh retains the last topology and reports a stale scan", async () => {
  const responses = [
    { ok: true, json: async () => ({ ...emptyTopology(), sessions: [sessionEntry("amber")] }) },
    { ok: false, json: async () => ({}) }
  ];
  const page = dashboard({ fetch: async () => responses.shift() });

  await flush();
  assert.equal(page.elements.get("#live-count").textContent, "1");
  page.intervals[0]();
  await flush();

  assert.equal(page.elements.get("#live-count").textContent, "1", "failed refresh preserves the prior snapshot");
  assert.equal(page.elements.get("#notice").dataset.kind, "error");
  assert.match(page.elements.get("#notice").textContent, /stale|failed|could not/i);
  assert.match(page.elements.get("#last-scan").textContent, /failed|stale/i);
});

test("successful refresh removes vanished session IDs from the shared selection", async () => {
  const responses = [
    { ok: true, json: async () => ({ ...emptyTopology(), sessions: [sessionEntry("amber"), sessionEntry("bravo")] }) },
    { ok: true, json: async () => ({ ...emptyTopology(), sessions: [sessionEntry("bravo")] }) }
  ];
  const page = dashboard({ fetch: async () => responses.shift() });

  await flush();
  const amber = page.created.find((element) => element.attributes.get("aria-label") === "Select amber-handle");
  amber.checked = true;
  amber.dispatch("change");
  assert.equal(page.elements.get("#selection-count").textContent, "1");

  page.intervals[0]();
  await flush();
  assert.equal(page.elements.get("#selection-count").textContent, "0");
});

test("successful refresh clears a filter whose option vanished", async () => {
  const machineA = { id: "machine-a", hostname: "alpha", os: "macos", arch: "aarch64" };
  const machineB = { id: "machine-b", hostname: "bravo", os: "linux", arch: "x86_64" };
  const amber = sessionEntry("amber");
  const bravo = { ...sessionEntry("bravo"), machine_id: "machine-b" };
  const responses = [
    { ok: true, json: async () => ({ ...emptyTopology(), machines: [machineA], sessions: [amber] }) },
    { ok: true, json: async () => ({ ...emptyTopology(), machines: [machineB], sessions: [bravo] }) }
  ];
  const page = dashboard({ fetch: async () => responses.shift() });

  await flush();
  const machineFilter = page.elements.get("#machine-filter");
  machineFilter.value = "machine-a";
  machineFilter.dispatch("change");
  page.intervals[0]();
  await flush();

  assert.equal(machineFilter.value, "", "the control returns to All when its option vanishes");
  assert.equal(page.elements.get("#empty").hidden, true, "the stale filter value no longer hides the new snapshot");
  assert.equal(page.elements.get("#map-inspector").textContent, "1 visible session · 0 direct links");
});
