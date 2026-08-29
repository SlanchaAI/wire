/**
 * wire — native Pi tools for the wire signed-message bus.
 *
 * Pi ships a four-tool core and deliberately has no MCP client (`README.md`:
 * "No MCP. Build CLI tools with READMEs, or build an extension that adds MCP
 * support."). So this package does NOT talk to `wire mcp`. Every tool here
 * shells out to the same `wire` CLI the operator types, parses its `--json`
 * body, and hands back a compact summary — which keeps the whole surface
 * token-cheap and debuggable by eye.
 *
 * Identity: each Pi session gets its own wire persona. Pi injects
 * `PI_SESSION_ID` into the env of commands run by its LLM-callable bash tool,
 * and wire resolves that into `sessions/by-key/<hash>` (src/session.rs,
 * `resolve_session_key`, source label `pi`). Pi does NOT put it in the env of
 * *this* extension process, and SDK-embedded hosts can disable the bash-tool
 * injection entirely, so we pin `WIRE_SESSION_ID` to the same session-id string
 * ourselves. `by_key_dir_name()` hashes only the key — never the source label —
 * so both paths land on ONE home for one conversation. That parity is asserted
 * by `resolve_session_key_pi_adapter_priority_and_home_parity`.
 *
 * Consent: `wire_setup` (binds a relay slot + claims a persona + starts a
 * daemon) and `wire_accept` (grants a stranger authenticated write access to
 * this inbox) both require an explicit operator yes — the `confirm` parameter
 * plus, when a dialog surface exists, a real prompt. Nothing here
 * self-authorizes.
 */

import { execFile, spawn, type ChildProcess } from "node:child_process";
import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

const BIN = process.env.WIRE_BIN ?? "wire";

/** Env for one `wire` invocation. Operator pins always win over the Pi key. */
function wireEnv(ctx: ExtensionContext): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...process.env };
  // `WIRE_HOME` is the RFC-008 §C deliberate fleet-share pin and
  // `WIRE_SESSION_ID` the operator-override channel; overwriting either would
  // silently split an intentionally shared identity into per-session ones.
  if (env.WIRE_HOME || env.WIRE_SESSION_ID) return env;
  env.WIRE_SESSION_ID = ctx.sessionManager.getSessionId();
  return env;
}

interface Run {
  code: number;
  stdout: string;
  stderr: string;
}

/** Run the wire CLI. Never throws: a non-zero exit is data, not an exception. */
function runWire(
  args: string[],
  ctx: ExtensionContext,
  opts: { timeoutMs?: number; signal?: AbortSignal } = {},
): Promise<Run> {
  return new Promise((resolve) => {
    execFile(
      BIN,
      args,
      {
        cwd: ctx.cwd,
        env: wireEnv(ctx),
        timeout: opts.timeoutMs ?? 20_000,
        maxBuffer: 8 * 1024 * 1024,
        windowsHide: true,
        signal: opts.signal,
      },
      (err, stdout, stderr) => {
        const code =
          err && typeof (err as { code?: unknown }).code === "number"
            ? ((err as { code: number }).code as number)
            : err
              ? 1
              : 0;
        resolve({ code, stdout: stdout ?? "", stderr: stderr ?? "" });
      },
    );
  });
}

function parseJson(raw: string): unknown | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  try {
    return JSON.parse(trimmed);
  } catch {
    return null;
  }
}

function text_result(text: string, details?: unknown) {
  return {
    content: [{ type: "text" as const, text }],
    details: { wire: details ?? null },
  };
}

const NOT_INIT = /not initialized|"initialized":false|run `wire up` first/;

/**
 * Shared tail for every tool: prefer the parsed `--json` body, keep it compact
 * (no indentation — this string goes straight into model context), and turn the
 * two failure classes operators actually hit into an instruction.
 */
async function wireJson(
  args: string[],
  ctx: ExtensionContext,
  opts: { timeoutMs?: number; signal?: AbortSignal } = {},
) {
  const run = await runWire(args, ctx, opts);
  const blob = `${run.stdout}\n${run.stderr}`;

  if (NOT_INIT.test(blob)) {
    return text_result(
      "wire: this session has no identity yet (initialized:false). Ask the operator " +
        "whether to come online, then call wire_setup — it binds a relay slot, claims " +
        "your persona, and starts the daemon. Do not run it on your own.",
    );
  }
  if (run.code !== 0) {
    return text_result(`wire ${args[0]} failed (exit ${run.code}):\n${run.stderr.trim() || run.stdout.trim() || "(no output)"}`);
  }
  const parsed = parseJson(run.stdout);
  if (parsed === null) {
    return text_result(run.stdout.trim() || "(no output)");
  }
  return text_result(JSON.stringify(parsed), parsed);
}

/**
 * Consent gate for the two verbs that change trust or spend a public resource.
 * Returns an error string when consent is missing, null when it is granted.
 */
async function consent(
  ctx: ExtensionContext,
  title: string,
  question: string,
  explicit: boolean | undefined,
): Promise<string | null> {
  if (explicit !== true) {
    return `wire: "${title}" needs operator consent. Re-call it with confirm:true only after the operator has approved it in their own words.`;
  }
  if (ctx.hasUI) {
    const ok = await ctx.ui.confirm(title, question);
    if (!ok) return `wire: the operator declined "${title}". Nothing was changed.`;
  }
  return null;
}

export default function (pi: ExtensionAPI) {
  // ---------------------------------------------------------------- read ----

  pi.registerTool({
    name: "wire_whoami",
    label: "Wire whoami",
    description:
      "This Pi session's wire identity: DID, persona (emoji + nickname), fingerprint, and the session home in use. Reports initialized:false when the session has no identity yet.",
    promptSnippet: "Show this session's wire identity and persona",
    promptGuidelines: ["Use wire_whoami before pairing or sending, so you quote your own persona from real output instead of inventing one."],
    parameters: Type.Object({}),
    async execute(_id, _params, signal, _onUpdate, ctx) {
      return wireJson(["whoami", "--json"], ctx, { signal, timeoutMs: 10_000 });
    },
  });

  pi.registerTool({
    name: "wire_here",
    label: "Wire here",
    description:
      'Cold-start orientation: {self, sister_sessions, pinned_peers}. Sister sessions are other agents on this machine reachable by session name without a relay round-trip. Call this when you need a dial target instead of guessing one.',
    promptSnippet: "Who am I and which agents can I reach right now",
    parameters: Type.Object({}),
    async execute(_id, _params, signal, _onUpdate, ctx) {
      return wireJson(["here", "--json"], ctx, { signal, timeoutMs: 10_000 });
    },
  });

  pi.registerTool({
    name: "wire_peers",
    label: "Wire peers",
    description:
      "List pinned peers with tier (UNTRUSTED/VERIFIED/ATTESTED) and advertised capabilities. Read-only.",
    promptSnippet: "List paired wire peers and their tiers",
    promptGuidelines: ["Never invent a wire peer name. Take it from wire_peers, wire_here, or the operator."],
    parameters: Type.Object({}),
    async execute(_id, _params, signal, _onUpdate, ctx) {
      return wireJson(["peers", "--json"], ctx, { signal, timeoutMs: 10_000 });
    },
  });

  pi.registerTool({
    name: "wire_status",
    label: "Wire status",
    description:
      "Daemon and sync-loop health: daemon_running, last_sync_age_seconds, inbox/outbox depth, peer count, and identity_split (non-null means this process is frozen to a stale identity — surface it, do not send).",
    parameters: Type.Object({}),
    async execute(_id, _params, signal, _onUpdate, ctx) {
      const out = await wireJson(["status", "--json"], ctx, { signal, timeoutMs: 15_000 });
      const details = (out.details as { wire?: { identity_split?: unknown } } | undefined)?.wire;
      if (details?.identity_split) {
        return text_result(
          "wire: IDENTITY SPLIT — " +
            JSON.stringify(details) +
            "\nThis process is serving a stale identity while the live session is another one. Tell the operator; do not pair or send until it is resolved.",
          details,
        );
      }
      return out;
    },
  });

  pi.registerTool({
    name: "wire_pending",
    label: "Wire pending",
    description:
      "List inbound pair requests waiting for operator consent. Call at session start and surface what you find — acceptance is the operator's decision, not yours.",
    promptSnippet: "List inbound wire pair requests awaiting consent",
    promptGuidelines: [
      "Call wire_pending at session start and report what is waiting to the operator in your own message.",
      "Never call wire_accept for a pending request the operator has not explicitly named. Accepting grants that peer authenticated write access to this inbox.",
    ],
    parameters: Type.Object({}),
    async execute(_id, _params, signal, _onUpdate, ctx) {
      return wireJson(["pending", "--json"], ctx, { signal, timeoutMs: 10_000 });
    },
  });

  pi.registerTool({
    name: "wire_tail",
    label: "Wire tail",
    description:
      "Read recent verified inbound events from this session's inbox, newest first. Each event carries a `verified` flag — the Ed25519 signature was checked before it landed.",
    promptSnippet: "Read recent inbound wire messages",
    parameters: Type.Object({
      peer: Type.Optional(Type.String({ description: "Filter to one peer handle." })),
      limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 500, default: 20, description: "Max events." })),
      oldest: Type.Optional(Type.Boolean({ default: false, description: "Oldest-first instead of newest-first." })),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const args = ["tail", "--json", "--limit", String(params.limit ?? 20)];
      if (params.oldest) args.push("--oldest");
      if (params.peer) args.push(params.peer);
      return wireJson(args, ctx, { signal, timeoutMs: 15_000 });
    },
  });

  pi.registerTool({
    name: "wire_pull",
    label: "Wire pull",
    description:
      "Trigger an immediate synchronous pull from this session's relay slot(s) instead of waiting for the daemon's ~5s cycle. Returns written[] / rejected[] / total_seen. Idempotent.",
    parameters: Type.Object({}),
    async execute(_id, _params, signal, _onUpdate, ctx) {
      return wireJson(["pull", "--json"], ctx, { signal, timeoutMs: 30_000 });
    },
  });

  // ------------------------------------------------------------- connect ----

  pi.registerTool({
    name: "wire_dial",
    label: "Wire dial",
    description:
      'Go talk to this name. Accepts a persona nickname, session name, card handle, DID, or a federation handle `<handle>@<relay>`. Resolves local sisters and federation, drives the right pair flow, and optionally sends a first message. Pairing is bilateral: the peer must accept too.',
    promptSnippet: "Dial and pair a wire peer by name (local or @relay)",
    promptGuidelines: [
      "Use wire_dial rather than hand-assembling pairing primitives; it picks the local-sister or federation flow itself.",
      "A dial you initiate still needs the peer's accept — tell the operator that until they confirm.",
    ],
    parameters: Type.Object({
      name: Type.String({ description: "Persona / session / handle / DID, or `<handle>@<relay>`." }),
      message: Type.Optional(Type.String({ description: "Optional first message to include." })),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const args = ["dial", params.name];
      if (params.message) args.push(params.message);
      args.push("--json");
      return wireJson(args, ctx, { signal, timeoutMs: 45_000 });
    },
  });

  pi.registerTool({
    name: "wire_send",
    label: "Wire send",
    description:
      "Sign and send an event to a peer. Synchronous by default: the returned `status` is the relay's real verdict — delivered | duplicate | peer_unknown | slot_stale | transport_error. `peer_unknown` / `slot_stale` mean run wire_dial first.",
    promptSnippet: "Send a signed message to a wire peer",
    promptGuidelines: [
      "Report the `status` you get back from wire_send; do not claim a message was delivered without seeing `delivered`.",
      "wire_send pairs on miss by default. Pass no_auto_pair:true when the operator wants strict no-implicit-pairing.",
    ],
    parameters: Type.Object({
      peer: Type.String({ description: "Peer handle (no did:wire: prefix)." }),
      body: Type.String({ description: "Message body. Free text or JSON." }),
      kind: Type.Optional(Type.String({ description: "Event kind (claim, decision, ack, …). Defaults to claim." })),
      queue: Type.Optional(Type.Boolean({ default: false, description: "Buffer in the outbox for the daemon instead of sending synchronously." })),
      no_auto_pair: Type.Optional(Type.Boolean({ default: false, description: "Fail loudly if the peer is not pinned." })),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      // Body goes through the CLI as one argv element, never through a shell,
      // so quotes and metacharacters in the message cannot be re-interpreted.
      const args = ["send", params.peer];
      if (params.kind) args.push(params.kind);
      args.push(params.body);
      if (params.queue) args.push("--queue");
      if (params.no_auto_pair) args.push("--no-auto-pair");
      args.push("--json");
      return wireJson(args, ctx, { signal, timeoutMs: 45_000 });
    },
  });

  pi.registerTool({
    name: "wire_accept",
    label: "Wire accept",
    description:
      "Accept one pending-inbound pair request by name. Pins the peer VERIFIED and ships our slot token back. Requires operator consent: pass confirm:true only after the operator approved, and this tool still asks in the UI when a dialog surface exists.",
    promptSnippet: "Accept a pending wire pair request (operator consent required)",
    promptGuidelines: [
      "wire_accept changes trust — only call it for a peer the operator named themselves, with confirm:true.",
    ],
    parameters: Type.Object({
      peer: Type.String({ description: "Pending peer nickname or handle from wire_pending." }),
      confirm: Type.Optional(Type.Boolean({ default: false, description: "Set true only when the operator has explicitly approved this peer." })),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const denied = await consent(
        ctx,
        `Accept wire pair: ${params.peer}`,
        "Accepting makes this peer VERIFIED and gives it authenticated write access to your inbox.",
        params.confirm,
      );
      if (denied) return text_result(denied);
      return wireJson(["accept", params.peer, "--json"], ctx, { signal, timeoutMs: 30_000 });
    },
  });

  pi.registerTool({
    name: "wire_reject",
    label: "Wire reject",
    description: "Refuse a pending-inbound pair request without pairing. Idempotent.",
    promptSnippet: "Reject a pending wire pair request",
    parameters: Type.Object({
      peer: Type.String({ description: "Pending peer nickname or handle." }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      return wireJson(["reject", params.peer, "--json"], ctx, { signal, timeoutMs: 20_000 });
    },
  });

  pi.registerTool({
    name: "wire_whois",
    label: "Wire whois",
    description:
      "Inspect an identity. With no handle, prints this session's own profile; with `nick@domain`, resolves through that domain's `.well-known/wire/agent` and verifies the returned signed card.",
    promptSnippet: "Inspect a wire identity or profile",
    parameters: Type.Object({
      handle: Type.Optional(Type.String({ description: "`nick@domain` to resolve, or omit for self." })),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const args = ["whois"];
      if (params.handle) args.push(params.handle);
      args.push("--json");
      return wireJson(args, ctx, { signal, timeoutMs: 30_000 });
    },
  });

  pi.registerTool({
    name: "wire_setup",
    label: "Wire setup",
    description:
      "Come online: mints this session's identity, binds a relay slot, claims the DID-derived persona, and starts the sync daemon. Idempotent. This is a network-visible action, so it needs the operator's go-ahead. Use `offline:true` for keygen with no relay binding.",
    promptGuidelines: [
      "Run wire_setup only after the operator agrees to come online — it contacts a relay and starts a daemon.",
      "Prefer relay `http://127.0.0.1:8771` or offline:true when the operator only wants same-machine sessions.",
    ],
    parameters: Type.Object({
      relay: Type.Optional(Type.String({ description: "Relay to bind and claim on, e.g. @wireup.net or http://127.0.0.1:8771. Omit for the default public relay." })),
      offline: Type.Optional(Type.Boolean({ default: false, description: "Mint the identity only — bind nothing, claim nothing." })),
      confirm: Type.Optional(Type.Boolean({ default: false, description: "Set true only when the operator has approved coming online." })),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const target = params.offline ? "offline keygen only (no relay, no claim)" : params.relay ?? "the default public relay (wireup.net)";
      const denied = await consent(
        ctx,
        "Bring this session online on wire",
        `This mints an identity, binds ${target}, claims your persona, and starts a daemon.`,
        params.confirm,
      );
      if (denied) return text_result(denied);
      const args = ["up"];
      if (params.offline) args.push("--offline");
      else if (params.relay) args.push(params.relay);
      args.push("--json");
      return wireJson(args, ctx, { signal, timeoutMs: 90_000 });
    },
  });

  // ------------------------------------------------ live inbox (opt-in) ----

  let watcher: ChildProcess | null = null;
  let buffer = "";

  function stopWatcher(ctx: ExtensionContext | undefined) {
    if (!watcher) return;
    watcher.kill("SIGTERM");
    watcher = null;
    ctx?.ui.notify("wire: inbox watcher stopped", "info");
  }

  function startWatcher(ctx: ExtensionContext) {
    if (watcher) return "already running";
    buffer = "";
    const child = spawn(BIN, ["monitor", "--json"], {
      cwd: ctx.cwd,
      env: wireEnv(ctx),
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    watcher = child;

    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      buffer += chunk;
      let nl: number;
      while ((nl = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, nl).trim();
        buffer = buffer.slice(nl + 1);
        if (!line) continue;
        const ev = parseJson(line) as
          | { from?: string; kind?: string; body?: unknown; persona?: { emoji?: string; nickname?: string } }
          | null;
        if (!ev) continue;
        const who = ev.persona?.nickname ?? ev.from ?? "peer";
        const glyph = ev.persona?.emoji ? `${ev.persona.emoji} ` : "";
        const body = typeof ev.body === "string" ? ev.body : JSON.stringify(ev.body ?? "");
        // Deliver as a follow-up so a peer message never yanks the wheel
        // mid-tool-batch, and only when the operator has opted in — an
        // auto-triggering turn is model spend they did not ask for.
        pi.sendMessage(
          {
            customType: "wire",
            content: `wire message from ${glyph}${who}${ev.kind ? ` (${ev.kind})` : ""}: ${body}`,
            display: true,
            details: ev,
          },
          { deliverAs: "followUp", triggerTurn: true },
        );
      }
    });
    child.on("exit", () => {
      if (watcher === child) watcher = null;
    });
    child.on("error", (err: Error) => {
      watcher = null;
      ctx.ui.notify(`wire: watcher failed — ${err.message}`, "error");
    });
    return "started";
  }

  pi.registerCommand("wire-watch", {
    description: "Stream inbound wire messages into this session (on | off | status)",
    handler: async (args, ctx) => {
      const want = (args ?? "").trim().toLowerCase();
      if (want === "on") {
        const state = startWatcher(ctx);
        ctx.ui.notify(`wire: inbox watcher ${state}`, "info");
      } else if (want === "off") {
        stopWatcher(ctx);
      } else {
        ctx.ui.notify(`wire: inbox watcher ${watcher ? "running" : "stopped"} — /wire-watch on | off`, "info");
      }
    },
  });

  pi.on("session_start", async (_event, ctx) => {
    // One cheap probe, reported to the operator, not to the model: a fresh
    // session has no identity, and auto-minting one would claim a relay slot
    // nobody asked for.
    const run = await runWire(["whoami", "--json"], ctx, { timeoutMs: 8_000 });
    const id = parseJson(run.stdout) as { initialized?: boolean; handle?: string; persona?: { emoji?: string } } | null;
    if (id?.initialized && id.handle) {
      ctx.ui.notify(`wire: ${id.persona?.emoji ?? ""} ${id.handle}`, "info");
    } else {
      ctx.ui.notify("wire: no identity for this session — run wire_setup when you want to come online", "info");
    }
  });

  // The watcher is session-lifetime infrastructure, not turn scaffolding
  // (wire AGENTS.md R7 — the 2026-05-12 agent-attention-layer incident root
  // caused exactly by tearing a listener down between iterations). It is torn
  // down only when the session ends or the operator says /wire-watch off.
  pi.on("session_shutdown", async () => {
    stopWatcher(undefined);
  });
}
