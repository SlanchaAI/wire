# Review of SlanchaAI/wire#356

Reviewed RFC-009 at head `17e8fe8ccc66955d878ee66b6bc77ebd5ff93222` of [SlanchaAI/wire#356](https://github.com/SlanchaAI/wire/pull/356). All file:line references below refer to that head. This reviews the proposed protocol and migration, not an implementation delivered by this documentation-only PR. The direction is plausible; the claimed security equivalence and retirement conditions need correction before adoption.

Evidence: read the PR metadata and full diff, then the referenced send, receive, endpoint, pairing and relay implementations. Source priors: [P, wire source at the reviewed immutable head, TBD: verify confidence score]; [P, official NIP-42 specification accessed 2026-10-22, TBD: verify confidence score]. Findings distinguish observed code from consequences inferred for the proposed migration.

## Findings

### High — Relay admission does not preserve recipient-scoped write authorization

**Location:** `docs/rfc/0009-nostr-addressing-primary.md:47` and `:67`.

The RFC equates trust-ring AUTH plus a receiver pin-filter with slot write authorization. Existing `src/relay_server.rs:912` checks the token for the target slot before accepting storage. Under the proposed policy, an admitted member A whom recipient B has never accepted can publish events addressed to B. B dropping these after download does not prevent relay storage consumption or receive-window starvation; the existing Nostr pull requests only 200 events (`src/cli/relay.rs:1513`). The reject test at RFC line 86 covers outsiders, not this admitted-but-unconsented sender.

**Fix:** Define whether the ring is global or recipient-specific. Specify authenticated-key-to-event-author binding, per-recipient admission/revocation and storage quotas, or explicitly acknowledge the weaker boundary and revise the acceptance/kill criteria. Test A admitted to the relay but unpinned by B, including sustained traffic and revoked consent. [NIP-42](https://github.com/nostr-protocol/nips/blob/master/42.md) authenticates a connection; authorization policy remains a separate requirement.

### High — Write-only AUTH does not keep metadata within the operator boundary

**Location:** `docs/rfc/0009-nostr-addressing-primary.md:44` and `:48`.

The proposed relay gates writes, while “reads are pin-filtered” describes receiver behavior, not relay read authorization. An otherwise standard relay can answer a stranger's subscription for addressed events, revealing clear sender/recipient tags even though wire operates it. Existing slot access is explicitly token-scoped for both reads and writes (`src/relay_server.rs:183`). Operator ownership alone cannot justify the metadata parity asserted at RFC line 68.

**Fix:** Require authenticated, recipient-authorized subscriptions and historical queries on every fleet member; reject broad and unauthorized filters, and define permitted sender access. Add a negative test proving another relay member and an anonymous reader cannot retrieve B's events or routing tags. If this policy is intentionally absent, require metadata protection before this deployment too, rather than only on third-party public relays.

### High — First-contact admission conflicts with rejection of all unpinned senders

**Location:** `docs/rfc/0009-nostr-addressing-primary.md:39` and `:54`.

A new caller must publish its card before the receiver can pin it, yet the design requires pinned identities at relay admission and drops unpinned events at receive. If the trust ring means receiver pins, first contact cannot arrive. If it means operator membership, the RFC still needs a separate pending-introduction path that survives the ordinary pin-filter. Existing Nostr pairing already separates requests from ordinary messages: `src/cli/nostr.rs:181` builds a pair request; `:222` fetches request and ack kinds, whereas daemon receive at `src/cli/relay.rs:1515` fetches only kind 1. Accept verifies the card and its transport-key binding before pinning (`src/cli/nostr.rs:104`). A normal addressed publish alone does not replace this workflow.

**Fix:** Specify bounded pre-consent admission, pending-request processing, card/binding verification and reject behavior, including enrollment of an identity outside the operator ring. Keep introductions separate from trusted inbox delivery. Test two initially unpaired identities through pending, explicit consent, rejection and first message; define when “no-gap” delivery starts relative to consent.

### Medium — Retirement has no executable compatibility gate for slotless peers

**Location:** `docs/rfc/0009-nostr-addressing-primary.md:59`.

The described `peers[*].transport` precedence switch is not the current routing representation. `src/endpoints.rs:399` stores `nostr_transport` with a single relay, and `src/send.rs:238` uses it as fallback only when a local transport key exists. More critically, `run_sync_pull` returns when HTTP endpoints are empty (`src/cli/relay.rs:1556`), before reaching Nostr receive at `:1635`. Retiring slot allocation without changing this guard leaves a newly created Nostr-only receiver silent. One elapsed release also does not establish that existing pins or older clients have enrolled transport keys.

**Fix:** Name the schema migration, enrollment/card refresh and capability negotiation, preserve HTTP routing for legacy peers until an explicit retirement gate passes, and make Nostr receive independent of HTTP endpoints. Require a fresh slotless identity to receive through the real daemon, plus mixed-version send/receive and rollback coverage. These are RFC acceptance criteria, not a request to put implementation into this PR.

### Medium — Relay fleet durability and catch-up semantics are unspecified

**Location:** `docs/rfc/0009-nostr-addressing-primary.md:38` and `:85`.

The RFC promises publish/read across a relay set without defining success or recovery. Existing peer coordinates contain one relay (`src/endpoints.rs:413`); publish reports that relay's acceptance (`src/nostr_ws.rs:69`). If success means one acceptance and that relay fails before replication or receive, a second reader relay cannot recover the event. Independently, current pull uses a 200-event window without pagination (`src/cli/relay.rs:1513`), so a receiver offline for a larger burst cannot rely on this path to drain all retained history. “0 delivered messages” lost does not distinguish relay acceptance from recipient delivery.

**Fix:** Define the persisted relay-set format, required acknowledgments for send success, durable retry after partial publication, recipient acknowledgment semantics, retention and paginated restart catch-up. Preserve logical event identity across replicas for deduplication. Extend the outage test to kill the sole accepting relay before recipient receipt, reconnect after more than 200 events, and verify complete delivery without duplicate inbox entries.

## Validation

Static review only; no runtime behavior was changed or claimed tested. The row's document proof checks nonempty content, Findings and Verdict headings, and a verdict token. GitNexus tools/CLI were unavailable; direct diff inspection verifies that the review adds only this file and changes no symbols. Protocol statements were checked against the official NIP-42 source, not a relay vendor summary.

## Verdict

**NOT-SHIP** as an accepted migration design at the reviewed head. Resolve the authorization, metadata and bootstrap contradictions, and add explicit slotless/mixed-version and fleet-recovery gates before endorsing retirement. Keeping this RFC in Discussion is reasonable; this verdict does not claim its documentation-only diff changes production behavior.
