# Scoped delegation and acknowledgements for wire

**Status:** REVISED after council review — run `20260810-162446-0ca9d81b` (DEGRADED: 3 of 4 seats
drafted; cross-critique stage failed for all seats, so no claim below was adversarially tested)
**Verdict:** unanimous REVISE. Ship P1 + P3. P2 rejected as specified and as placed.
**Author:** salt-plateau (Paul's assistant), after a 12-agent overnight supervision run

## Decision question

Should wire add two generic primitives — a **correlation ID** on sends and a **scoped,
signed, expiring delegation envelope** — so one agent can lead a crew of agents across
harnesses without wire becoming a task framework? Or is this the wrong layer, and should
both live entirely in a conductor above wire?

## Why now — the evidence

Overnight 2026-08-10 I supervised twelve autonomous agents (Goose on DeepSeek) building one
game across five git worktrees. Two failures were structural, not incidental:

1. **No acknowledgements.** I issued one agent (`ws2`) the same rebase directive three times
   across ninety minutes. Every time, I discovered non-compliance by manually inspecting git
   state (`git merge-tree`, branch counts). The root cause — I had been appending directives
   to the bottom of a 7,000-character brief, where they were never read — took three
   supervision cycles to find. A required ack, or a nack, surfaces that in seconds.
2. **Ambient, unbounded authority.** I rewrote agents' briefs, killed their sessions, and
   reassigned file ownership between workstreams. Nothing recorded that I was permitted to,
   nothing expired, and nothing could revoke it. It worked because I am the only owner. It
   does not survive contact with agents owned by different people or machines.

The coordination bus that night was files (`REQUESTS.md`, `WS*_VERDICT.md`) — no identity,
no delivery guarantee, no correlation between a request and its fulfilment.

## What already exists (verified 2026-08-10, primary sources)

| Layer | Provides | Does NOT provide |
|---|---|---|
| **ACP** (agentclientprotocol.com) | `session/new`, `session/prompt`, `session/cancel`, `session/close`, `requestId` correlation *within* a session, `session/request_permission` | Any agent hierarchy, cross-owner delegation, or authority model |
| **Gas City** (gastownhall/gascity, MIT, 1.1k★, updated today) | Controller/supervisor loop reconciling desired to running state; packs; orchestrates Claude, Codex, Gemini | No ownership model, no trust boundary between operators, no cross-machine delegation (README) |
| **Omnigent** (omnigent-ai/omnigent, Apache 2.0, 8.5k★, updated today) | Meta-harness across Claude Code, Codex, Cursor, Pi | Same: single-owner assumption |
| **wire** | Stable session identity (DID), consent-based pairing, signed delivery, role routing, rooms | Correlation IDs, acks, scoped delegation |

**The white space is real.** Nobody in this stack provides scoped, signed, expiring
delegation across a trust boundary. That is the control layer, and it is the part genuinely
ours to build rather than buy.

## Ownership line (agreed independently by two agents)

```
Conductor (BUY: Gas City / Omnigent)   task lifecycle, retries, budgets, worktrees, restart
        │  tasks, cancellation, results
        ▼
Harness adapters (BUY: ACP)            session/new, prompt, cancel, close
        │  identity, discovery, rooms, signed delivery, DELEGATION
        ▼
wire (BUILD: the two primitives below)
```

Wire must not acquire: spawning/stopping agents, worktree isolation, prompt injection,
token budgets, retries, or structured result collection. Those differ per harness and would
turn wire from transport into a task framework — the expansion its own positioning brief
(`docs/design/2026-07-04-open-band-positioning-brief.md`) explicitly declines.

## The proposal

### P1 — Correlation ID on send (small)

Add an optional `correlation_id` to `wire_send` and surface it on inbox reads. Any reply
carrying the same id is mechanically joinable to its request. No semantics beyond that; wire
never interprets it.

### P2 — Scoped delegation envelope (the real work)

A signed, self-expiring grant, not a permanent trust tier:

```
{ job_id, leader_did, member_dids[], allowed_actions[], deadline, cancellation_authority,
  delegation_chain[] }
```

Properties that matter:

- **Expires by construction.** No deadline, no envelope. Authority cannot outlive the job.
- **Scoped to named actions**, never "leader may do anything to member".
- **Signed chain**, so a member can verify the grant without asking the leader.
- **Revocable** by the named cancellation authority before the deadline.
- **No permanent boss tier.** Leading one job grants nothing about the next.

### P3 — Acknowledgements (small, high value)

A member receiving a work order under an envelope must ack or nack it, with the
correlation ID. The leader can then distinguish "not delivered" from "delivered and
ignored" — the exact distinction that cost me three cycles overnight.

## Constraints

- Wire is transport and identity. If a proposed feature needs to know what the work *is*,
  it belongs in the conductor.
- Additive and backward compatible; existing peers unaffected when fields are absent.
- No new always-on daemon surface, and no dependency on a relay being fresh — the overnight
  run saw `stale_sync` at 10.5h twice, so supervision must not hard-depend on relay liveness.
- One writer per file; the wire repo has its own review discipline.

## Reversibility

High for P1/P3 (optional fields, ignorable by old peers). **Low for P2** — a delegation
format becomes a security surface and is hard to change once agents rely on it. P2 is the
part that most needs this review.

## Success checks (falsifiable)

1. A leader issues a work order to two members of a different harness; both acks are joinable
   to the order by correlation ID without reading message bodies.
2. A member independently verifies a delegation envelope's signature chain and rejects one
   whose deadline has passed, without contacting the leader.
3. Revoking an envelope before its deadline causes the next member action under it to fail closed.
4. An expired envelope grants nothing: the same leader/member pair has no residual authority.
5. Existing unmodified peers exchange messages unaffected when the new fields are absent.

## The question for the council, precisely

1. Is P2 correctly placed in wire, or does a delegation envelope belong in the conductor
   (Gas City/Omnigent) where the job actually lives?
2. Is the envelope shape sound as a security object, or does it have a hole (replay,
   confused deputy, chain forgery, clock skew on `deadline`)?
3. Given ACP already has `session/request_permission`, is P2 duplicating an existing seam?
4. Is P1+P3 worth shipping alone if P2 is rejected?


---

# Council ruling — 2026-08-10, run 20260810-162446-0ca9d81b

Seats: codex (0.97), deepseek (0.92), amanalap (0.88). All three: **revise**.

## The design defect that decides it

**Success checks 1 and 2 are mutually unsatisfiable.** A member cannot both verify an envelope
offline without contacting the leader AND have revocation fail closed on the next action: a
self-contained signed grant proves issuance and expiry, never the absence of a newer revocation
(codex, blocker 1). My own relay constraint then finishes it — with `stale_sync` seen at 10.5h,
"fail closed on next action" silently becomes "succeeds for up to 10.5h" (amanalap).

P2 must pick one model explicitly: **short-lived leases with a stated, accepted revocation
latency**, or an authoritative freshness check that fails closed when freshness is unprovable.
That choice belongs where the job lives — the conductor — not in transport.

## Ruling

- **P1 + P3 ship now**, tightened: collision-resistant correlation IDs scoped by sender;
  authenticated ack/nack linkage; idempotent duplicate handling; distinct statuses for
  receipt / accepted / rejected / started / completed.
- **P2 is rejected in wire as specified.** `allowed_actions[]` forces wire to interpret an action
  vocabulary its own ownership line assigns to the conductor (unanimous). Wire carries the
  capability as **opaque bytes** and provides identity + signature verification only.
- **Q1 answered:** neither pure placement was right. Opaque carriage in wire, schema and
  enforcement in the conductor.
- **Q3 answered:** P2 does not duplicate ACP `session/request_permission`. That seam solicits
  harness-local permission; a conductor-issued capability supplies the *evidence* for it. Map
  capability authorization INTO ACP; do not build a competing permission protocol.
- **The "white space" claim is downgraded.** The cross-owner gap is real but has **zero current
  consumers** — the overnight evidence is single-owner. Building a low-reversibility security
  surface now is speculative hardening (amanalap). P2 waits for a real second owner.

## What the six-field sketch was missing

Unique grant identifier and anti-replay rule; cryptographic audience and target binding;
canonical serialization and signature-suite rules; chain root-of-authority and key rotation;
monotonic attenuation at every hop; maximum chain depth; time representation, boundary rule,
max TTL, allowable skew, and rollback-resistant clock behaviour.

## Correction I have to own

**An ack proves receipt and acceptance, never compliance.** The ws2 incident was a *compliance*
failure, and what actually caught it was executable verification (`git merge-tree`) — conductor
work. P3 shortens that loop from ninety minutes to seconds; it does not close it. The doc
overclaimed and now says so.

## Downgrade path (missed entirely in the original)

A protected work order reaching an old peer that ignores the envelope must **fail closed at
enforcement**. Optional-field parsing is not enough; silently stripping the field while the
action still executes is the failure mode.
