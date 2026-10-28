# Legacy identity federation rejection — issue #364

Investigation for [SlanchaAI/wire#364](https://github.com/SlanchaAI/wire/issues/364),
against `origin/main` at `42d4346`. Source inspection only: no build, live relay
calls, pairing, or message sends. Line references below refer to this revision.

## Finding

A legacy v3.1 card can be correctly self-signed yet unusable for new federation
pairing because its session DID lacks the public-key fingerprint suffix. Relay
storage success and receiver verification are separate verdicts. `wire send`
reports the former as `delivered`; it does not wait for the receiver to accept,
pin, decrypt, or write the event. Keep the DID-to-key check strict.

The issue reports a July 14/15 incident with `rejected_n: 2`. This investigation
explains the source paths; it does not independently reproduce that incident or
infer the exact rejection reason of both events from that count.

## Sender: how delivery appears successful

1. `src/cli/comms.rs:315` reads the local key and card, then takes the DID and
   verify key directly. There is no self-card verification here.
2. `src/cli/comms.rs:370` signs via `sign_message_v31`.
   `src/signing.rs:199` checks key lengths and signs the event hash; it does not
   enforce a self-certifying DID. The function's v31 name is not a legacy-card
   compatibility policy.
3. `src/cli/comms.rs:387` calls `send::attempt_deliver` for synchronous sends.
   `src/send.rs:177` posts to a pinned endpoint; `src/send.rs:195` maps a
   duplicate response to `Duplicate`, and other successful responses to
   `Delivered` (`src/send.rs:202`). `src/send.rs:117` names that `delivered`.
4. `src/cli/comms.rs:388` prints the JSON delivery verdict; the text branch at
   `src/cli/comms.rs:400` likewise says delivered. The success condition at
   `src/cli/comms.rs:433` is `reached_relay`, not a receiver acknowledgment.
5. Ordinary slot POST handling (`src/relay_server.rs:906`) checks the slot token,
   size and quota, persists the event (`src/relay_server.rs:982`), and returns
   `status: stored` (`src/relay_server.rs:1010`). This route does not perform
   receiver trust/card verification. Thus a legitimate relay success does not
   establish federation acceptance.

The precondition is a usable pinned endpoint: unknown peers or failed transport
produce other verdicts. An unpaired sender cannot be assumed to reach this path.

## Receiver: where the legacy card fails

`src/cli/relay.rs:1549` runs daemon pulls; its slot loop passes events to
`pull::process_events` at `src/cli/relay.rs:1620`. Processing attempts to consume
pair drops before ordinary event verification (`src/pull.rs:266`).

For a recognized pair drop allowed by the nonce/open-mode gates
(`src/pair_invite.rs:510`–`550`), `src/pair_invite.rs:561` calls
`verify_agent_card` on the embedded sender card. That verifier:

- validates the Ed25519 self-signature (`src/agent_card.rs:527`);
- reads the DID and checks `did_commits_to_key` (`src/agent_card.rs:537`);
- returns `CardError::DidKeyMismatch` at `src/agent_card.rs:542` on mismatch.

`did_commits_to_key` (`src/agent_card.rs:126`) requires a session DID whose last
hyphen-separated component equals the signing-key fingerprint. A suffixless
`did:wire:paul-mac` fails despite a valid signature. Operator/organization DID
prefixes are rejected by this session-card check too; do not classify them as
legacy session identities.

The error becomes `pair_drop peer card sig invalid: ...`
(`src/pair_invite.rs:562`). `src/pull.rs:270`–`288` logs the consumption failure,
records it locally, and leaves the sender unpinned. Processing continues into
`verify_message_v31` (`src/pull.rs:360`). For an otherwise unknown remote sender,
trust lookup fails with `UnknownAgent` (`src/signing.rs:269`); the pull result
records that rejection at `src/pull.rs:432`. Later ordinary messages from that
untrusted sender fail at the same trust lookup. This is a downstream trust
failure, not another direct `DidKeyMismatch` from the event verifier.

`src/cli/relay.rs:1628` collects rejected events. The daemon records the count as
`rejected_n` at `src/cli/relay.rs:1232`–`1233` and logs counts at
`src/cli/relay.rs:1248`. None of these paths sends a rejection verdict back to
the originating `wire send` call. Existing trust or local-sister auto-pinning
can change the downstream result; legacy cards are not universally rejected by
the ordinary event-signature verifier alone.

## Version checks and current-main caveat

This is a DID/key-binding failure, not a `schema_version == v3.1` rejection.
`verify_agent_card` (`src/agent_card.rs:495`–`545`) does not gate on card schema
version. Card version upgrading for identity claims is separate
(`src/agent_card.rs:367`–`387`). Event ingress compares schema **major** versions
(`src/pull.rs:227`–`247`), allowing the v3 minor versions through that gate.
Changing the card's version label cannot repair the missing key commitment.

Current main also verifies cards at the relay's handle-intro endpoint
(`src/relay_server.rs:1961`), returning HTTP 400 on failure. Federation dialing
routes through `cmd_add` (`src/cli/pairing.rs:17`), reads our card without a local
self-check (`src/cli/pairing.rs:1387`), and posts the intro at
`src/cli/pairing.rs:1429`. On a relay running this revision, a legacy intro can
therefore fail synchronously before the receiver daemon sees it. Claim handling
also verifies cards (`src/relay_server.rs:1327`); peer discovery verifies the
resolved peer card (`src/pair_profile.rs:317`). These checks do not preflight
our own identity before ordinary sends. The incident's deployed relay revision
is unknown; do not claim all current federation introductions silently succeed.

`wire whoami` reads and displays the existing card (`src/cli/identity.rs:295`),
while `wire up` treats existing initialization as sufficient to skip keygen
(`src/cli/setup.rs:63`). `config::read_agent_card` (`src/config.rs:608`) reads
JSON, without invoking `verify_agent_card`. These are opportunities to diagnose
an unusable local identity before a remote rejection.

## Ranked fixes to propose (not implemented)

1. **Local self-preflight before outbound side effects.** Share a check across
   CLI/MCP send, queued send, dial/add, up, and claim. Validate our own DID/key
   binding before network calls, pin changes, outbox writes, or signing. Reject
   with a structured code and nonzero failure, plus: “Legacy v3.1 identity has
   no key commitment; peers enforcing DID/key binding reject its card. Mint a
   fresh session identity (`WIRE_SESSION_ID=<new-unique-id> wire up`) and pair
   again.” Do not rewrite a DID in place or relabel its schema as migration.
   This gives the sender an immediate actionable answer for this failure,
   independent of receiver availability or relay version.
2. **Precise receiver and relay diagnostics.** Preserve `DidKeyMismatch` as a
   hard rejection. Distinguish a legacy-shaped session DID lacking the 8-hex
   suffix from a present-but-wrong fingerprint; keep malformed/op/org cases
   distinct. Include the remedy in pull diagnostics and HTTP intro/claim
   errors. Treat “legacy-shaped” as a diagnostic, not proof of benign intent.
   Receiver-only logs help operators but cannot resolve sender asymmetry alone.
3. **Clarify delivery semantics and design authenticated receipts.** Expose
   relay acceptance explicitly in CLI/MCP output and help. If end-to-end
   acceptance is required, add a separately authenticated, event-ID-correlated
   receiver verdict with timeout/unknown states. Never mark a relay POST as
   receiver acceptance. An untrusted legacy sender cannot safely bootstrap
   trusted rejection receipts merely by supplying return coordinates; design
   authentication, consent and amplification limits before implementing this.
4. **Read-only health warning.** Add an actionable warning to whoami/status/
   doctor for an existing incompatible identity. Useful discovery, but weaker
   than preflight because scripts may never inspect health output.

No fix should accept suffixless cards at a weaker trust tier or bypass the
commitment check. A self-signature alone does not bind a claimed DID to its key.

## Tests to add with the implementation

- Card fixtures: correctly signed suffixless v3.1 card rejects; same legacy DID
  relabeled v3.2 still rejects; matching commitment with v3.1 label passes the
  card verifier; wrong fingerprint, malformed key and op/org cases retain
  strict rejection. Extend existing binding tests at `src/agent_card.rs:675`
  and `src/agent_card.rs:706`.
- Self-preflight: test send (sync and queue), dial/add, up and claim, plus MCP
  equivalents, against a legacy local card. Assert actionable error/code,
  no transport call and no outbox/trust/relay-state mutation. Matching current
  identities retain the happy path; missing initialization retains its own
  remedy. Include an already-pinned peer so auto-pairing does not hide the bug.
- Offline receiver processing: signed legacy pair-drop fixture with open-mode
  enabled, then a claim. Assert no trust pin/inbox write, explicit card-binding
  diagnostic, downstream unknown-signer rejection and correct cursor behavior.
  Separate closed-mode/nonce fixtures must preserve their consent gates.
- Local mock relay: ordinary slot POST succeeds while receiver rejects;
  sender output must represent relay acceptance only. Separately assert current
  intro and claim endpoints return actionable errors for the same legacy card.
  Never require the public relay or real identities for these regressions.
- If receipts are adopted: authenticated acceptance/rejection, wrong signer,
  wrong event ID, duplicate receipt, unavailable receiver and timeout tests;
  none may promote storage success to receiver acceptance.

## Validation and limits

Only the requested documentation proof is run. No code or tests changed and no
runtime federation reproduction attempted. GitNexus tools, CLI and referenced
repository skill files were unavailable in this checkout; tracing used local
source. The proposed tests above remain implementation work for #364.
