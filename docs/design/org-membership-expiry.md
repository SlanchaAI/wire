# Membership expiry, re-keying, and atomic trust writes

Design for SlanchaAI/wire#246. Documentation only; no runtime changes or live
Wire network calls. Source baseline: `42d4346` (2026-10-23 inspection).
Paths and lines below refer to that baseline. Source evidence is primary
repository code [P, SlanchaAI/wire@42d4346, TBD: verify trust-prior score];
proposals below are decisions for a future implementation, not shipped behavior.

## Current producer and consumer paths

| Concern | Source evidence |
| --- | --- |
| Issue | `src/cli/identity.rs:461` handles `enroll org-add-member`, loads the org key, calls `enroll::issue_member_cert`, stores a membership, and prints a transferable bundle. `src/enroll.rs:32` signs only the operator DID (decentralized identifier), using `src/identity.rs:50`. |
| Persist and emit | `src/config.rs:569` replaces the existing entry for the same org DID in `memberships.json`. `src/enroll.rs:43` builds claims; `src/agent_card.rs:258` defines the membership fields and `src/agent_card.rs:339` emits them. No expiry field exists in this path. |
| Import | `src/cli/identity.rs:791` ingests a bundle; `src/cli/identity.rs:852` verifies its certificate against the local operator before storing it. |
| Verify | `src/identity.rs:78` verifies the bare operator-DID signature. `src/org_membership.rs:134` verifies the operator anchor, org DID/key commitment, and each certificate; invalid entries are skipped, and at least one valid entry is needed. |
| Apply policy | `src/pair_invite.rs:868` and `src/pair_invite.rs:890` call membership evaluation for auto-pin and notify decisions. `src/pair_decision.rs:55` matches verified orgs against local policy. `src/trust.rs:287` records `ORG_VERIFIED`; it does not verify certificates itself. |
| Read cached trust | `src/trust.rs:76` reads the stored tier. `src/trust.rs:107` returns non-`VERIFIED` tiers without rechecking membership age. Adding an admission check alone would not expire a cached org grant. |
| Rotate | `src/cli/identity.rs:673` replaces the operator key after logging succession and instructs manual certificate reissue. `src/cli/identity.rs:722` creates a new org DID/key, retains the old key file, and instructs reissue and binding changes. Neither rotation automatically migrates receiver trust. |
| Write trust | `src/config.rs:680` takes `trust.lock`; `src/config.rs:704` already serializes, writes `trust.json.tmp`, then renames it over `trust.json`. `src/config.rs:729` locks the entire read-modify-write transaction. |

## Proposed certificate format and expiry check

Introduce membership format `cert_version: 2`, carrying `org_did`,
`org_pubkey`, `op_did`, `issued_at`, `expires_at`, and `member_cert`.
Timestamps are nonnegative integer UTC Unix seconds, with
`issued_at < expires_at`. Require an explicit expiry on issuance; propose a
maximum lifetime of 30 days. That duration is a design choice, not existing
policy. Reject non-integers, overflow, unknown versions, and invalid DID syntax.

Sign an unambiguous payload rather than appending unsigned metadata to today's
signature. Define signed bytes as ASCII `wire/member-cert/v2` followed by a NUL,
then, in order: org DID and operator DID encoded as a four-byte unsigned
big-endian UTF-8 byte length followed by their UTF-8 bytes, then `issued_at`
and `expires_at` as unsigned eight-byte big-endian integers. The org key signs
these bytes with Ed25519. The org DID commits to the inline public key as today.
This separates membership signatures from operator and succession signatures;
changing any identity or timestamp invalidates the signature. JSON whitespace
and field order have no effect. Enforce length bounds before allocating.

Add a pure verifier taking explicit `now` for deterministic tests. It checks
format, DID/key commitments, signature, lifetime, `issued_at <= now`, and
`now < expires_at`; equality at expiry rejects. No grace interval extends expiry.
Production samples the local clock once per evaluation; unavailable/pre-epoch
clock fails closed for org privileges. A wrong or rolled-back local clock remains
an offline trust limitation; this proposal does not promise trusted time.

Carry all fields through `MemberOf`, `OrgMembership`, claim serialization, local
storage, printed bundles, and import. Use the same verifier at import and at
card evaluation. Import must also match the local operator; card evaluation
must retain the operator-to-session binding. One expired membership must not
hide another valid org membership. An invalid v2 entry must never retry as v1.

At rollout, legacy bare-DID certificates cannot prove expiry. Updated receivers
reject them for org-derived privileges; members need fresh v2 issuance. Do not
dual-sign or silently allow timeless certificates. Deploy readers and arrange
renewal before enforcing the cutover; older receivers cannot enforce this
contract and must be upgraded for expiry guarantees. Ordinary bilateral trust
remains independent of membership validity.

## Expiry after pairing

Store the verified grant's org DID, operator DID, signed certificate, and expiry
with the pinned card, preserving multiple eligible memberships. At each
org-authorized decision, reevaluate the card and current org policy with current
time; no valid eligible grant means effective `UNTRUSTED` for org-only access.
Do not erase the peer or downgrade independently earned `VERIFIED` trust.

The implementation must connect this check to `trust::effective_tier` and audit
callers of `get_tier` and direct tier reads used for authorization, including
`trust::project_recipients` (`src/trust.rs:327`). Admission via auto/notify and
any later pending acceptance must recheck, not reuse an old eligibility result.
A fresh verified card renews the cached grant through `update_trust`; a rejected
renewal must not overwrite a still-valid grant. Legacy cached `ORG_VERIFIED`
records without verifiable expiry fail closed until refreshed. No background
scheduler or network renewal dependency is required: issuance remains explicit,
and authorization is the consumer of expiry.

## Re-keying without invalidating other members

Distinguish member renewal, operator rotation, and org issuer rotation:

1. **Renew one member:** issue a v2 cert for the same operator and org with a new
   validity interval. Verify before replacing its stored entry. Keep the old
   cert usable until its original expiry; renewal does not revoke other members.
2. **Rotate one operator:** stage a new operator key/DID and obtain new org certs
   before making that key active. Keep existing sessions and their old operator
   anchor valid through their original certificate expiry. Activate the new key
   and rebuild/sign new session cards only once required memberships are ready.
   Old certs must never be copied onto the new operator DID. The present immediate
   overwrite command needs a prepare/activate split in the future implementation.
   Failure before activation leaves the old key and cards usable. Other operators'
   certs are unaffected. Succession is evidence of key handoff, not org approval
   of a new member identity; do not transfer memberships using succession alone.
3. **Rotate the org key:** key commitment means the new key has a new org DID.
   Keep old certificates verifiable against their original inline key/DID.
   Receivers explicitly configure both old and new org DIDs during a bounded
   overlap, after checking the existing old-key-signed succession certificate
   and approving the new policy entry. No arbitrary card may install an alias.
   Issue new certificates under the new DID, retaining both memberships while
   members migrate; current storage keyed by org DID supports that overlap.
   Set the old policy's retirement deadline to cover outstanding finite certs,
   at most the chosen 30-day maximum after rotation, and stop old-key issuance.
   Effective authorization requires both certificate validity and an unretired
   policy entry, so use of a retained old private key cannot extend the overlap.
   Retire the old policy/key only after that deadline and migration. A domain
   binding change alone does not update already-pinned receiver policy.

Offline or unrenewed members lose org privileges at expiry; continuity requires
renewal before that deadline. For compromised keys, explicitly revoke the old
policy immediately: preserving old validity would preserve the attacker's
access. Automatic succession-chain trust migration is outside this design.

## Atomic `write_trust`: preserve the shipped solution

The requested write-temp-then-rename transition has already landed. Preserve
`write_trust` -> lock -> `write_trust_unlocked` -> serialize -> write sibling
temporary file -> rename -> unlock. Same-directory rename publishes a complete
old or new JSON document; readers do not consume the temporary file. Keep the
lock on the stable `trust.lock` file, not the inode being replaced. The fixed
temporary filename is safe only while every writer holds that lock.

Serialization, temporary write, or rename failure must return an error and leave
the previous destination intact; remove a failed temporary file while holding
the lock, or overwrite it on the next successful attempt. Do not truncate or
unlink the destination first. Atomic visibility is distinct from power-loss
durability: the current helper has no file/directory synchronization. If durable
acknowledgment becomes required, sync the temporary file before replacement and
the parent directory afterward, with explicit platform handling and errors.

Whole-file `write_trust` calls can still overwrite newer state from a stale
snapshot. Membership grant updates must use `update_trust`, which reads and
modifies under the same lock and invokes the unlocked helper without recursively
locking. Existing callers such as `src/pull.rs:364` and
`src/cli/group.rs:485` show why rename alone is not a lost-update solution;
changing those callers is a separate implementation task, not this doc change.

## Tests for the implementation

All proposed checks use fixture keys, injected time, and temporary directories;
no relay, domain lookup, or live accounts are needed.

- **Signature/format:** fixed v2 byte-vector round trip; tampered org/operator,
  timestamp or version fails; wrong key, malformed lengths, integer overflow,
  missing expiry, v1 fallback, and cross-protocol signature replay fail.
- **Time:** valid before expiry, invalid exactly at and after expiry; future
  issuance, inverted interval, lifetime over limit, and clock failure reject.
- **Real producer/consumer seam:** issue bundle -> import -> stored membership
  -> rebuilt signed card -> evaluate -> auto/notify policy decision. Verify expiry
  survives every serialization step. Mixed expired/valid org entries still work.
- **Cached authorization:** pin while valid, advance fake time without receiving
  another card, assert org-only access and project recipients lose eligibility;
  pending accept rechecks; independent bilateral trust survives. Renew before
  expiry without a gap; failed renewal leaves the prior valid grant intact.
- **Rotation:** stage/activate a member key and show old sessions remain valid
  while new sessions use new certs; other members unchanged; wrong-operator
  reuse fails. For org rotation, verify both policies during overlap, then old
  policy retirement even if a later-expiring old-key cert is presented. Reject
  unapproved aliases and forged succession; test interrupted staging/retry.
- **Atomic persistence:** retain existing round-trip/no-temp and concurrent
  no-tear tests at `src/config.rs:1225` and `src/config.rs:1243`, plus transaction
  lost-update coverage at `src/config.rs:1275`. Add injected write/rename failure,
  interrupted writer, stale-temp recovery, first-write failure, and concurrent
  readers of alternating large/small documents; assert old-or-new complete
  content and no silently successful failed update. Test replacement on each
  supported operating system. Add synchronization failure cases only if durable
  acknowledgment is implemented.

Doc acceptance: `test -s docs/design/org-membership-expiry.md && grep -qiE 'write_trust' docs/design/org-membership-expiry.md`.
The runtime tests above are proposed, not executed or claimed by this draft.
