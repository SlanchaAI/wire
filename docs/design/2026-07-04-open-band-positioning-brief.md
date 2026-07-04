# Brief — wire as "the open BAND": positioning + roadmap

Date: 2026-07-04 · Status: DRAFT for ratification · Branch: `observability-open-band`
Companion: `docs/design/2026-07-04-observability-and-open-band.md` (the P1/P2 code) ·
Primary source: `dotfiles-claude/dossiers/band-ai.md` (30-page deep-crawl of band.ai)

## Thesis

BAND (band.ai) raised **$17M seed** (Sierra/Hetz/Team8) to build exactly wire's thesis —
persistent agent identity, consent-gated discovery, shared rooms, coordination across frameworks —
as a **closed, centralized SaaS**. That is not a threat to flee; it is **category validation with a
structural opening**. wire already ships BAND's expensive core (identity + consent + rooms + signed
audit) on the axis BAND *cannot* pivot to without unbuilding itself: **decentralized, self-hostable,
offline-capable, cryptographically self-certifying, open-source.** The move is to name and own that
axis — **"no server owns your agent graph"** — and close the two feature gaps that actually matter
(reach + observability), while explicitly *declining* the gaps that are traps (hosted memory, task
boards, enterprise-governance theater).

## The market moment (why now)

- "Copilot era → workforce era": many agents per team, crossing org/vendor boundaries, needing
  coordination + oversight. `[S, Team8, 2026, 72]` BAND, A2A (Google), ACP, MCP are all racing the
  same seam. A funded, well-sold closed incumbent legitimizes the category and creates demand for
  an **open** counterweight — the Langfuse-to-Datadog / Ollama-to-OpenAI pattern.
- wire's existing assets map 1:1 onto BAND's pitch, minus the server. This is a positioning
  reframe more than a rebuild.

## wire vs BAND — verified matrix

All BAND facts from the 30-page primary-source crawl (`dossiers/band-ai.md`), `[P, 2026-07-04, 92]`.

| Dimension | wire | BAND | Verdict |
|---|---|---|---|
| **Identity** | self-certifying DID + Ed25519, offline-verifiable, no account | account-issued API key + UUID; root of trust = human account; **zero crypto** | **wire wins** (hard) |
| **Encryption** | NIP-44 E2E (x25519 + ChaCha20-Poly1305), signed envelopes | none surfaced in 30 pages | **wire wins** (hard) |
| **Consent** | operator-gated bilateral accept | Contacts state machine, but `HUB_ROOM` allows LLM auto-accept + same-org auto-trust | **wire wins** (stricter) |
| **Decentralization** | self-host relay / loopback / offline; relay is a blind bypassable mailbox | coordination plane **SaaS-only** (app.band.ai); compute BYO but substrate centralized | **wire wins** (the wedge) |
| **License** | AGPL/Apache/MIT, OSS | closed/proprietary | **wire wins** |
| **Framework reach** | MCP + CLI (Claude-native) | **14 native SDK adapters** (LangGraph/CrewAI/OpenAI/Gemini/Pydantic/…, Py+TS) + A2A + ACP | **BAND wins** (the real gap) |
| **Rooms/routing** | group chat (shared-slot) | rooms + @mention routing, dynamic assembly, tool-gated sends | **parity / BAND richer** |
| **Memory** | none (punts to Soul) | structured records (no vectors), Enterprise-gated, off the real-time bus, absent from core-concepts | **contested → wire declines (see below)** |
| **Tasks/boards** | none | per-room kanban (beta) | **wire declines** |
| **Governance/audit** | signed envelopes + inbox JSONL | fragmented: no unified log, **no pre-action approval/pause/kill** | **both thin → greenfield** |
| **Observability** | none (pre-P1/P2) | dashboard (stream + cron) | **BAND wins today → P1/P2 closes it** |
| **Funding/GTM** | solo OSS | $17M + enterprise sales | **BAND wins** (not our game) |

## Strategic recommendation

**Own the decentralized/open axis; close reach + observability; decline the traps.**

### Build (close the gaps that matter)
1. **Observability** — P1 `wire dash` + P2 Mission Control adapter (companion doc). Removes BAND's
   only visible-surface lead. Ship first (in progress).
2. **Reach via MCP-as-universal-adapter** (locked). *Sharpened by the crawl:* BAND itself found MCP
   insufficient — its MCP server **"cannot receive messages,"** relegated to automation. wire's
   differentiator is that **wire made MCP a *live-participant* path** (daemon + monitor tail inbound
   → the agent replies in-context). So the bet is not "MCP like BAND's"; it is "MCP done as BAND
   *couldn't*." **Must-clear bar:** prove + document wire's MCP live-receive loop as first-class, and
   validate coverage against the frameworks BAND lists (every MCP-speaking host gets wire free). Only
   add a native SDK where a real framework has no MCP host (researched, not speculative).
3. **A2A interop adapter (evaluate).** BAND exposes peers as A2A endpoints. wire speaking A2A on the
   *outside* (per the standing positioning-lock: "speak A2A on the outside, Future-A on the inside")
   keeps wire interoperable with the BAND/Google/enterprise world without adopting their trust model.
   Scope in a follow-up spec; not v1.

### Steal (free wins from the crawl)
4. **Tool-gated sends / "raw LLM text is not a message."** BAND makes raw model output invisible in a
   room — only an explicit send tool delivers. This is an anti-hallucination / anti-double-act design
   that pairs with wire's `agentic-action-safety` posture. Cheap to adopt in wire's MCP tool contract;
   worth a short design note.

### Decline (name them anti-features, don't drift into them)
5. **Cross-agent memory — NON-GOAL** (locked). Vindicated by the crawl: BAND's own memory is a
   vector-less, Enterprise-gated bolt-on, absent from its core model. wire = transport; agents bring
   their own memory (Soul). Record in `docs/ANTI_FEATURES.md`.
6. **Task boards / kanban — NON-GOAL.** BAND's is beta; it is workflow-orchestration, not comms
   substrate. Out of wire's lane.
7. **Enterprise governance UI — DEFER, don't chase the theater.** BAND's "governance" is passive
   observability with no pre-action gate. wire's honest version = the signed, verifiable audit trail
   it *already* has (every envelope Ed25519-signed) surfaced queryably — a *stronger* claim than
   BAND's self-reported event log, and it falls out of P1's dash. Post-1.0 additive; don't build a
   policy engine on spec.

## The one-line positioning

> **wire — the open agent network. No server owns your agent graph.**
> Self-certifying identity, end-to-end encryption, self-hostable or fully offline. The parts BAND
> charges enterprise money for, decentralized and open.

## KPIs (falsifiable)

- **Reach proof:** a documented, tested wire↔agent round-trip from ≥3 distinct MCP hosts (e.g.
  Claude Code, Cursor, one LangGraph-via-MCP) — proving "MCP host ⇒ wire participant" without a
  native SDK. Target: before claiming reach parity. *Fail if any host needs bespoke wire glue.*
- **Observability proof:** `wire dash` correctly classifies the live 272-session store (live vs
  abandoned vs dead) with zero spawn/kill; MC adapter shows wire agents in MC's UI. *Fail if it
  mutates state or misses husks.*
- **Positioning proof:** the landing + README lead with the decentralization wedge and a 60-second
  "self-host / offline" demo BAND structurally cannot match. *Fail if the pitch is feature-parity
  ("we have rooms too") instead of axis-difference.*
- **Interop proof (stretch):** wire peer reachable as an A2A endpoint from a non-wire A2A client.

## Risks / honest counters

- **Reach is the real gap and MCP may not cover every framework.** Mitigation: the KPI forces a
  *proof*, not an assertion; native SDKs added only where a coverage hole is demonstrated.
- **BAND out-executes on GTM ($17M + enterprise sellers).** wire doesn't win enterprise sales; it
  wins the developers + privacy/sovereignty buyers who will never accept a mandatory central broker.
  Different ICP — don't fight on their field.
- **"Open BAND" invites feature-parity creep** (memory, boards, governance UI). Mitigation: the
  Decline list above is a standing gate; each is an explicit anti-feature.
- **Changelog/maturity unverifiable** (their mirror hides it) — don't over-index on BAND's velocity;
  compete on axis, not feature-race. `[flag: unverified]`

## Next actions

1. Ratify this brief (esp. the Decline list → `docs/ANTI_FEATURES.md` entries).
2. Proceed to P1/P2 code (companion doc) — observability first.
3. Spin a short follow-up spec for the MCP-live-receive reach proof + the A2A-outside adapter.
4. Feed the positioning one-liner to the landing/README refresh (separate change).
