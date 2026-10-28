# Relay denial-of-service hardening

Design for SlanchaAI/wire#291. Source baseline: `42d4346` (origin/main).
Documentation only; no implementation, deployment, or live relay probes.
Source references below describe that revision, not deployed configuration.

## Current endpoint inventory

All references in this section are `src/relay_server.rs:line`.
**G** means the single shared `GlobalKeyExtractor` governor configured with
`.per_second(10).burst_size(50)` at `src/relay_server.rs:453`.
The comment calls this 10 requests/second; do not treat that comment as measured
behavior. Verify the locked tower-governor version's refill semantics before
implementation (an interval of ten seconds is not ten requests/second).
**None** means no application request-rate governor, not absence of authentication.
No proxy protection is assumed: the router comment is not deployment evidence.

| Method and endpoint | Governor | Current limits and source |
| --- | --- | --- |
| POST `/v1/slot/allocate` | G | 200,000 slots, 503 at cap; inserts under lock (`src/relay_server.rs:878`). |
| POST `/v1/pair` | G | 50,000 pair slots; existing code can reuse one; role conflict rejected (`src/relay_server.rs:1123`). |
| POST `/v1/pair/:pair_id/bootstrap` | G | Existing pair required; replaces one role's sealed value; no explicit field byte cap (`src/relay_server.rs:1226`). |
| POST `/v1/pair/abandon` | G | Removes matching pair/lookup; no allocation (`src/relay_server.rs:1112`). |
| POST `/v1/events/:slot_id` | None | Bearer required; serialized event 256 KiB, slot 64 MiB, 413 on excess; dedupe scan (`src/relay_server.rs:906`). |
| GET `/v1/events/:slot_id` | None | Bearer required; default 100, max 1,000 events; clones requested window, scans for cursor under lock (`src/relay_server.rs:2196`). No response byte cap independent of slot quota. |
| GET `/v1/events/:slot_id/stream` | None | Bearer required; 256 live subscribers/slot, 503 at cap; unbounded channel per subscriber, 30-second keepalive (`src/relay_server.rs:1027`). |
| GET `/v1/slot/:slot_id/state` | None | Bearer required; returns counts and stored health record (`src/relay_server.rs:2245`). |
| POST `/v1/slot/:slot_id/responder-health` | None | Slot syntax and bearer checks; rewrites health file, no explicit record byte cap (`src/relay_server.rs:2269`). |
| GET `/v1/pair/:pair_id` | None | Pair existence/role checks; clones pair, refreshes idle timer and runs eviction scan (`src/relay_server.rs:1188`). |
| POST `/v1/handle/intro/:nick` | None | Target gate 5 intros/300 seconds, 429; card checks; 256 KiB event and 64 MiB slot caps (`src/relay_server.rs:1888`). |
| POST `/v1/handle/claim` | None | Ownership/card checks; 100,000 handles for new nick, existing owner can reclaim (`src/relay_server.rs:1287`, `src/relay_server.rs:1396`). |
| DELETE `/v1/handle/claim/:nick` | None | Ownership checks and file/map deletion (`src/relay_server.rs:1492`). |
| GET `/v1/handles` | None | Default 100, max 500 results; clones **all** records before sorting/filtering (`src/relay_server.rs:1766`). |
| POST `/v1/invite/register` | None | 50,000 invites; URL 8,192 bytes; lifetime default 1 day, clamped 60 seconds–7 days; optional use count (`src/relay_server.rs:1589`). |
| GET `/i/:token` | None | Token-shape, expiry/use checks; script fetch can decrement uses; URL lookup does not (`src/relay_server.rs:1670`). Registration emits 16 hex characters but fetch accepts 6: existing compatibility defect, not a rate defense. |
| GET `/.well-known/wire/agent` | None | Handle lookup, one stored card (`src/relay_server.rs:2162`). |
| GET `/.well-known/agent-card.json` | None | Handle lookup and card conversion (`src/relay_server.rs:2064`). |
| GET `/stats`, `/stats.json` | None | Aggregate snapshot; `/stats` negotiates HTML/JSON (`src/relay_server.rs:771`, `src/relay_server.rs:784`). |
| GET `/stats.history` | None | Default 24 hours, max 168; reads entire history file before filtering, no row/byte cap (`src/relay_server.rs:710`). |
| GET `/stats.html` | None | Embedded static page (`src/relay_server.rs:743`). |
| GET `/phonebook`, `/phonebook.html` | None | Embedded static page (`src/relay_server.rs:755`). |
| GET `/` | None | Embedded static page (`src/relay_server.rs:810`). |
| GET `/favicon.svg`, `/og.png` | None | Embedded assets (`src/relay_server.rs:819`, `src/relay_server.rs:828`). |
| GET `/install`, `/install.sh`, `/openshell-policy.sh` | None | Embedded scripts (`src/relay_server.rs:840`, `src/relay_server.rs:855`). |
| GET `/healthz` | None | Constant response (`src/relay_server.rs:703`). |

Route registration is authoritative (`src/relay_server.rs:465`). Local-only mode
omits landing/discovery/invites/stats, but retains claim/unclaim and core routes
(`src/relay_server.rs:492`). All registered GET routes above are ungoverned,
including their framework-provided HEAD behavior. Authentication and pagination
bound access/results, not request frequency or work before pagination.

Existing object caps are backstops, not a total memory budget. In particular,
200,000 × 64 MiB permits about 12.2 TiB of serialized slot data; decoded JSON,
indices and channel copies cost extra. Event quota check and insertion use
separate lock acquisitions (`src/relay_server.rs:935`, `src/relay_server.rs:975`),
as do new-handle capacity check and later publication: concurrent requests need
atomic reservations, not just another threshold. Pair idle eviction and intro
tracking cleanup already run every 60 seconds (`src/relay_server.rs:564`).

## Proposed per-IP governor

These numbers are initial design budgets, not measured production capacity or
new defaults shipped by this PR. Keep the existing tower-governor dependency;
verify its locked API and express rates as explicit refill intervals with tests.

Wire admission into `Relay::router_with_mode`, before JSON extraction, signature
verification, state locks and disk work. `serve_with_mode` currently passes the
router directly to `axum::serve` (`src/relay_server.rs:2452`); it must supply trusted
socket connection information. For Unix-domain sockets (`src/relay_server.rs:2464`),
use a separate bounded local bucket, not a missing-IP error or implicit bypass.

Key direct traffic by canonical socket IP (normalize IPv4-mapped IPv6). Accept a
forwarded client IP only from an explicitly configured trusted proxy peer with a
single validated client-IP field that the proxy overwrites. Ignore forwarded
headers from everyone else. Missing trusted metadata fails closed with 503.
Default proxy trust list is empty. Do not key on attacker-supplied handles or
slot IDs; group routes by fixed class so random paths cannot allocate buckets.

Every request consumes both its IP/class bucket and a process-wide backstop.
Buckets below are shared across all routes in a class, including failed auth,
404s, HEAD, and malformed requests. Classify unknown paths into ordinary reads.

| Class | Routes | Per-IP sustained / burst | Process sustained / burst |
| --- | --- | --- | --- |
| Allocation | Slot allocate, pair open, handle claim, invite register | 1/second / 10 | 10/second / 50 |
| Mutation | All other POST/DELETE routes | 10/second / 30 | 100/second / 200 |
| Ordinary reads | Event list, state, pair get, well-known cards, invite fetch, static assets, unknown paths | 20/second / 40 | 200/second / 400 |
| Expensive reads | Handles directory, stats JSON/negotiated stats, stats history | 2/second / 5 | 20/second / 40 |
| Stream admission | Event stream GET | 1/second / 4 | 20/second / 40 |
| Health | Healthz GET/HEAD | 2/second / 4 | 20/second / 40 |

Keep the existing per-target intro gate in addition to source admission. Use a
separate health bucket so ordinary overload does not starve health checks.
Return 429 with `Retry-After` rounded up to seconds for request budget exhaustion;
503 for object/concurrency capacity; 413 for byte limits. Reject before mutation.
No unbounded waiting queue. Bound non-stream in-flight requests to 128 globally
and 8/IP; body-read deadline 5 seconds and ordinary handler deadline 10 seconds.
Do not cancel persistence after mutation without rollback/commit semantics.

Bound governor state to 20,000 IP entries, fixed class buckets per entry. Reap
entries idle for 10 minutes every 60 seconds; when full, refuse new keys with 503
rather than evicting active entries and resetting their budget. The relay startup
owns this cleanup task and shutdown cancels it. IP rotation still requires global
caps; large shared-network clients can hit per-IP limits. Observe aggregate
rejections by route class/reason, bucket count and active requests; no raw IPs,
credentials or unbounded metric labels. IPv6 prefix aggregation is a later tuning
decision, not an assumed identity guarantee.

## Allocation and work budgets

Proposed federation defaults below deliberately replace the much larger count
ceilings. Make limits configurable with validated positive upper bounds; no
unset value may disable enforcement. Local mode keeps finite limits too.

| Resource | Proposed hard budget | Enforcement/reclamation |
| --- | --- | --- |
| Slots | 10,000 process-wide; 60 successful new slots/IP/hour | Atomic count reservation before token generation/persistence. Never silently evict durable slots. |
| Handles | 10,000; 60 new handles/IP/hour; signed card 16 KiB | Atomic new-name reservation; reclaim/update does not consume a new-object count, still consumes request budget. |
| Pair slots | 2,000; each role message/bootstrap 16 KiB; absolute lifetime 15 minutes | Bound code hash to 128 bytes; keep existing idle cleanup and add absolute age so reads cannot keep pairs forever. |
| Invites | 5,000; keep 8 KiB URL and 7-day maximum lifetime | Remove expired/exhausted entries before admission and on 60-second sweep; persist deletion. |
| Event storage | Keep 256 KiB/event and 64 MiB/slot; add 512 MiB total serialized event bytes and 10,000 events/slot | Atomically reserve slot/global bytes and count for both post and intro. Duplicate event consumes no new capacity. Include restored data. |
| Streams | 16/slot, 16/IP, 256 globally | Reserve before channel creation; release on disconnect/error; channel max 16 events AND 1 MiB/subscriber. Disconnect lagging readers; reconnect via event cursor. |
| Read output | Event list max 1 MiB serialized page, still max 1,000 items | Stop before next event exceeds budget; existing last-event cursor continues pagination. One max-size event fits. |
| Directory work | Max 500 results and 1 MiB/page | Ordered index of eligible records; clone only fitting page, no full-card clone per request. Preserve hidden-handle and cursor semantics. |
| History work | Max 20,160 rows, 8 MiB retained file and 1 MiB response | Bounded retention/read; return newest fitting records with explicit truncation metadata. Never read an unbounded historical file. |
| Request bodies | 272 KiB event/intro envelopes; 32 KiB claim/pair/bootstrap/invite bodies; 4 KiB other mutations | Apply raw byte limit before deserialization, then field-specific caps. Bound URI to 4 KiB and headers to 16 KiB at transport admission. |

Serialized event budget is not a resident-memory guarantee. Measure decoded JSON
and index overhead offline with worst-case tiny events; require process memory
below 2 GiB under the capped fixture before enabling these defaults. Stream byte
budgets permit at most 256 MiB queued payload globally. If the memory proof fails,
lower defaults or change storage representation before rollout, not after OOM.

Admission reservations must cover check, insertion and persistence outcome.
Release on failed writes; concurrent requests must not oversubscribe caps.
Startup reconstructs counts/bytes from disk with bounded reads, and reports an
over-capacity state that rejects growth but permits bounded reads, updates that
reduce usage and deletion. Do not delete old durable data automatically. Existing
states exceeding memory budgets require an explicit migration/storage decision;
this design does not authorize deployment or data loss.

## Tests to add in the implementation follow-up

Use in-process router requests, temporary state directories, injected connection
metadata and a fake clock. No public relay traffic or paid services required.
Extend `tests/relay.rs` and focused `src/relay_server.rs` unit tests:

1. Enumerate every route/method above in both server modes. Exhaust its class
   bucket; next request is 429 before handler work. HEAD and invalid requests
   cannot bypass admission. Fake time proves exact refill interval, burst and
   `Retry-After`, catching the current comment/API ambiguity.
2. Exhaust IP A; IP B still succeeds below global cap. Exhaust global budget;
   both fail. Forged forwarding headers cannot change direct-peer identity;
   trusted-proxy extraction, malformed/missing metadata, mapped IPv6 and Unix
   socket policy are explicit test cases.
3. Fill 20,000 governor keys with reduced test limits; next key fails boundedly.
   Active keys keep debt, idle cleanup reclaims entries, and cleanup stops on
   shutdown. Health remains available when the ordinary class is exhausted.
4. For each allocation/count/byte cap, exercise cap minus one, exact cap and
   cap plus one through real handlers. Race concurrent requests at the final
   unit: exactly one reservation wins. Test event post versus intro together,
   duplicate events, same-owner reclaim, rollback on disk failure and restart
   reconstruction. Rejected requests produce no extra files/counters/state.
5. Pair polling cannot extend absolute lifetime. Expired invites reclaim both
   memory and persisted capacity. Add register-to-fetch invite regression for
   the existing 16-versus-6-character token mismatch before relying on that path.
6. Open streams to every cap; next connection gets 503. A slow reader cannot
   exceed event/byte queue bounds; disconnect releases all permits. A healthy
   reader continues receiving events while a lagging reader is removed.
7. Oversized raw/chunked bodies fail before JSON parsing; maximum valid event
   envelopes still pass. Huge cursor/headers, stalled bodies and simultaneous
   requests hit explicit budgets without accumulating waiting tasks.
8. Large directory, event and history fixtures prove bounded output and work;
   retain directory privacy/filtering and event cursor semantics. At capped
   worst-case storage, record memory and handler progress for a second client.
   Count allocations/visited records where possible rather than flaky timing
   assertions. Existing round-trip, auth, persistence and stream tests stay green.

This row's proof only verifies the design artifact exists and mentions the
governor; it does not claim these protections or tests are implemented.
