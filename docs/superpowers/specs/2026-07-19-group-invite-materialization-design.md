# Group invite materialization

## Goal

A verified peer added directly to a group sees that group after sync without requesting a `wire-group:` code.

## Scope

- Run existing verified-invite intake before `wire group list`.
- Materialize only an invite addressed to, and whose signed roster includes, the local identity.
- Cover the creator-adds-member, recipient-pulls, recipient-lists path in the group integration test.
- Preserve code-based joins for link-style, unpaired admission.

## Non-goals

- No change to group signatures, trust tiers, relay credentials, or daemon behavior.
- No new consent or pending-invite model.

## Verification

The integration test must prove that a recipient can list a directly added group after pulling its signed invite, before sending or tailing a group message.
