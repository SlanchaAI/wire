# Design — wire observability (`wire dash` + Mission Control) & the "open BAND" position

Date: 2026-07-04 · Status: DRAFT (design, pre-implementation) · Branch: `observability-open-band`

## Origin

Prompted by looking at [band.ai](https://www.band.ai/) and asking "should we build an open-source
clone of this dashboard?" Research reframed it twice:

1. **band.ai (BAND) is not a dashboard — it is a direct competitor to wire.** It is
   "enterprise interaction infrastructure for multi-agent systems": persistent agent identity,
   shared rooms, @mention routing, consent-based discovery, cross-agent memory, unified audit
   trail, governance. That is wire's thesis. Closed/proprietary, **$17M raised** (Sierra Ventures,
   Hetz, Team8), enterprise-security founders (Sygnia→Temasek, Ermetic→Tenable). Deploys
   SaaS / private-cloud / edge. Cross-framework SDKs (LangGraph, CrewAI, OpenAI, Gemini,
   Pydantic AI) + MCP + ACP. The dashboard is one thin surface on top.
2. **The generic "watch my agents" dashboard is already a saturated OSS category** — building
   another from scratch is shelf-ware. Closest existing tools:
   [Mission Control](https://github.com/builderz-labs/mission-control) (MIT, 1.8k★, already
   auto-discovers `~/.claude/projects/` sessions, 31 panels), seunggabi/schmoli TUIs,
   Claude-Code-Agent-Monitor, agent-deck, plus native `claude daemon status`.

**Conclusion:** don't clone BAND's dashboard. Instead (a) ship a *wire-aware* observability pane
none of those tools have (DIDs / personas / sync / peers / relay), reusing an existing OSS UI
where it buys leverage, and (b) treat BAND as validation that wire should lean into being **the
open, decentralized BAND**. Three sub-projects, two buildable now, one strategic.

## Local reality that motivates P1

On this Mac right now: **272 session dirs** under
`~/Library/Application Support/wire/sessions/by-key/`, **270 wire binaries** in `ps`, **264 live
daemons**. These are not dead husks — they are *abandoned-but-still-running* daemons, one per old
Claude Code session. There is no single pane that shows which wire identities exist, which have a
live daemon, and which are actually in use vs abandoned. That is the L1 pain `wire dash` kills.

Scope confirmed with operator: all four layers wanted —
- **L1** local sessions (this box), **L2** fleet (Mac + Dell + GB10 + Air),
  **L3** remote peers, **L4** relay health.
- Aggregation splits cleanly: the relay already sees every *synced* identity (L2/L3/L4); only a
  filesystem walk sees local/abandoned state (L1). No existing surface gives all four.

---

## P0 — shared core: `src/dash.rs`

The reusable mechanism both P1 (TUI) and P2 (MC adapter) consume. Build once.

```rust
pub struct SessionSnapshot {
    pub key: String,            // by-key hash (session::SessionInfo)
    pub handle: String,         // agent-card.json
    pub did: String,            // did:wire:...
    pub fingerprint: String,    // did-fp
    pub persona: Persona,       // emoji + palette → TUI color
    pub daemon: DaemonState,    // Live{pid} | Abandoned{pid} | Dead{pid} | None
    pub sync_age_s: Option<u64>,// relay.json / state
    pub relay_url: String,      // relay.json
    pub peers: Vec<PeerRow>,    // trust.json (pinned peers)
    pub pending_inbound: usize,
    pub session_source: String, // claude-code | cli | named
    pub in_use: bool,           // recent sync OR live MCP-registered cwd
    pub started_at: Option<String>,
}

pub struct DashReport {
    pub sessions: Vec<SessionSnapshot>,
    pub relays: Vec<RelayHealth>,   // deduped by relay_url, one /healthz each
}

pub fn collect(opts: CollectOpts) -> Result<DashReport>;
```

### Load-bearing constraints (a naive build dies at 270 sessions)

- **Read-only, zero spawn, zero per-session network.** `collect()` reads on-disk state only.
- **Per-session liveness = cheap primitives already in-tree**, never a machine-wide scan per
  session: `session::list_sessions()` → per `SessionInfo.home_dir` →
  `session::session_daemon_pid(home)` → `platform::process_alive(pid)`. This is exactly the walk
  `ensure_up::daemon_liveness()` already does internally to compute orphans (#170 / v0.13.2 A2) —
  reuse/extend it, do not reinvent, and do not call the full `daemon_liveness()` once-per-session.
- **`DaemonState` classification:**
  - `None` — no `daemon.pid`.
  - `Dead{pid}` — pidfile present, `process_alive == false` (true husk).
  - `Live{pid}` — pid alive **and** session is in use (recent sync / MCP-registered cwd).
  - `Abandoned{pid}` — pid alive but session not in use (the ~260 case).
- **Relay health (L4) fetched once per distinct relay_url**, not per session. `--probe` (opt-in,
  default off) adds a single relay probe per distinct peer for L3 liveness; off = fully offline.
- **`--json` emits `DashReport` verbatim** — the golden surface P2 and any external tool consume.
  Add an anti-drift golden-JSON test (wire convention).

Build-loop tier: **MEDIUM** (multi-file, reads live session store, no external mutation).

---

## P1 — `wire dash` (CLI subcommand)

Consumes `dash::collect()`. New `Command::Dash { watch, json, all, probe }` in `cli/mod.rs`
(enum at `src/cli/mod.rs:75`); impl in `src/cli/dash.rs` mirroring `src/cli/status.rs` patterns.

- **Default one-shot table**, columns:
  `persona · handle · did-fp · ●live/◐abandoned/○dead/–none · sync-age · peers · pending · source`.
  Sort: in-use live first, then abandoned by sync-age, dead last. Abandoned/dead collapsed to a
  count line; `--all` expands. Header line = relay health (L4) + fleet hint.
- **`--watch`** — 2s redraw. **Prefer crossterm/ANSI redraw over pulling `ratatui`** (wire's
  minimal-dep ethos: pure rustls, no bloat). Interactivity (jump/kill) is explicitly phase 2.
- **`--json`** — emit `DashReport`.
- **L2 (fleet)** in v1 = "run `wire dash` on each box over SSH" — no new network surface. A
  relay-side `/v1/my/slots` (key-authenticated, scoped to slots you can prove you own — NOT a
  global enumeration, which would leak the social graph BAND-style) is the phase-2 upgrade for a
  single phone-viewable fleet pane. Deliberately deferred until threat-modeled.
- **Husk reaping is OUT of P1.** wire *already has* a husk reaper (see host-decay engines). `wire
  dash` **surfaces** abandoned/dead daemons and points at the existing reaper; it does **not** grow
  a second destructive path. A `--reap` that kills processes would be HIGH-tier + hits the `wire
  nuke` host-scoping trap — deferred.

Build-loop tier: **MEDIUM**. In-situ test: run against the live 272-session store, confirm it
classifies live vs abandoned vs dead correctly and does not spawn/kill anything.

---

## P2 — Mission Control adapter (read-only reporter)

Not a new UI — a thin reporter reusing `dash::collect()`, so wire identities appear in MC's
existing 31-panel dashboard. Correction to an earlier framing: this pulls in **no** Next.js dep —
MC runs as a **separate process**; wire is only an HTTP client (`reqwest`, already in-tree).

Confirmed MC contract:
- `POST /api/agents/register` — `Authorization: Bearer <MC_API_KEY>`, body `{ "name", "role" }`.
- `GET /api/tasks/queue?agent=<name>` — task poll (**not used**; wire is not a task executor).
- Real-time via WebSocket + SSE (schema in MC's `openapi.json` — build agent must fetch/confirm).
- Default port **3000**. Agent fields: `id, name, status, session, tokens, cost, logs`.
- Generic-fallback adapter exists → wire maps onto it.

Surface: `wire dash --report mission-control --mc-url <url> --mc-key <env-ref>` (loop mode).
Honest field mapping:

| MC field | wire source |
|---|---|
| `name` | handle |
| `role` | `"wire-peer"` |
| `session` | key |
| `status` | `DaemonState` (live/abandoned/dead) |
| `logs` | inbox JSONL tail |
| `tokens` / `cost` | **null** — wire is not an LLM runner; do not fabricate |

- **MC_API_KEY via env / 1Password (`op`), never committed.** Adapter is send-only (wire→MC); it
  does not accept commands from MC (no task execution). If MC→wire control is ever wanted, that is
  a separate, HIGH-tier design (agentic-action-safety).

Build-loop tier: **MEDIUM**. In-situ test: run a local MC on :3000, confirm wire sessions register
and status updates land; verify no secret in the diff.

---

## P3 — "the open BAND" (research-spec brief, NOT code)

Strategic frame P1+P2 slot into. wire already ships BAND's expensive core (self-certifying DID
identity, consent pairing, rooms/groups, signed audit) — **open + decentralized**. The brief maps
the gaps and locks the stance:

| Pillar | Gap vs BAND | Locked stance |
|---|---|---|
| Observability | none after P1+P2 | ✅ delivered by P1+P2 |
| Cross-framework reach | BAND ships N SDKs; wire = MCP/CLI | **MCP-as-universal-adapter** (operator-locked 2026-07-04). Every MCP-speaking framework (Claude Code, Cursor, LangGraph-via-MCP, …) gets wire free — one surface, not N SDKs. Brief evaluates where MCP coverage is thin; native SDKs only if a real gap is found. |
| Governance / audit | BAND's enterprise wedge | dash is the seed; roadmap adds a queryable audit log + policy hooks. Post-1.0 additive. |
| Cross-agent memory | BAND ships it | **Explicit NON-GOAL / anti-feature** (operator-locked 2026-07-04). wire = transport; agents bring their own memory (Soul). Record in `docs/ANTI_FEATURES.md` so it is a decision, not a gap. |
| Positioning | — | **"No server owns your agent graph."** BAND's private-cloud is still their platform; wire's relay is a bypassable blind mailbox (loopback / self-host). Lean into decentralized + offline + crypto-verifiable + AGPL. |

Deliverable = a `research-spec` brief + a full BAND **deep-crawl dossier** (first pass done; a
`deep-crawl` completes it and lands in the dossier-cache). Trust-prior-tagged, KPI-gated. Runs in
**parallel** with the code (it is a document, no code dependency).

Build-loop tier: research, not code — `research-spec` machinery (research log + persona review +
KPI gate). Opus.

---

## Sequencing & build plan (Opus/Sonnet agents, per operator; no Fable)

```
P0 dash::collect()  ──►  P1 wire dash  (Sonnet build, Opus/Sonnet review)
      (serial root)  └►  P2 MC adapter (Sonnet build, review)
P3 open-BAND brief   ── parallel ──►    (Opus: research-spec + deep-crawl)
```

- **P0 is the serial root**; P1 and P2 both depend on it and are otherwise independent.
- **P3 is independent of all code** — dispatch it in parallel from the start.
- Each code component runs its own **build-loop** (research → plan → gate → develop → in-situ test
  → gate). MEDIUM tier: 2–3 reviewers over the diff, one gate, self in-situ round-trip.
- Every code component lands with its caller in the same change (no shelf-ware): P0 lands *with*
  P1 consuming it; P2 lands as a real subcommand flag.
- **Non-negotiable in-situ tests** run against the live session store / a local MC on :3000 — never
  a mock-only pass. No externally-visible mutation is tested against real targets.

## Files touched (anticipated)

- **new** `src/dash.rs` (P0), `src/cli/dash.rs` (P1)
- **edit** `src/cli/mod.rs` (`Command::Dash` enum + dispatch)
- **new** MC reporter (in `src/cli/dash.rs` or `src/dash_report.rs`) (P2)
- **new** `docs/design/2026-07-04-observability-and-open-band.md` (this doc)
- **edit** `docs/ANTI_FEATURES.md` (memory-as-non-goal, from P3)
- **new** P3 brief + BAND dossier (docs / dossier-cache)
- tests: `tests/dash_collect_*.rs` (classification), golden-JSON anti-drift, MC-mapping unit

## Open questions / deferred

- Relay-side `/v1/my/slots` for the single-pane fleet view (L2) — deferred until threat-modeled
  (must be key-scoped, must not enable social-graph enumeration).
- `wire dash --reap` (destructive husk cleanup) — deferred; use the existing reaper.
- MC→wire task control — out of scope; separate HIGH-tier design if ever wanted.
