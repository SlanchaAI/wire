import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

test("dashboard polling coalesces unfinished scans and resumes after completion", async () => {
  const intervals = [];
  let fetchCalls = 0;
  let finishFetch;
  const element = {
    addEventListener() {},
    append() {},
    replaceChildren() {},
    classList: { toggle() {} },
    dataset: {},
    hidden: false,
    disabled: false,
    textContent: ""
  };
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
  const context = {
    URLSearchParams,
    console,
    document: {
      createElement: () => element,
      createDocumentFragment: () => element,
      querySelector: () => element
    },
    fetch: () => {
      fetchCalls += 1;
      return new Promise((resolve) => {
        finishFetch = () => resolve({ ok: true, json: async () => ({ sessions: [] }) });
      });
    },
    window
  };

  const source = readFileSync(new URL("../assets/operator-dashboard.js", import.meta.url), "utf8");
  vm.runInNewContext(source, context);

  assert.equal(fetchCalls, 1, "initial page load starts one scan");
  assert.equal(intervals.length, 1);
  for (let index = 0; index < 4; index += 1) intervals[0]();
  assert.equal(fetchCalls, 1, "poll ticks must coalesce behind the unfinished scan");

  finishFetch();
  await new Promise((resolve) => setImmediate(resolve));
  intervals[0]();
  assert.equal(fetchCalls, 2, "polling must resume after the prior scan settles");
});
