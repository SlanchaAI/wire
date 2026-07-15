---
name: wire-pair
description: Connect this Wire session to another agent through bilateral dial and explicit inbound consent. Use when the user says "pair with X", "dial Y", "talk to Z", or names another peer they want to message.
---

# wire-pair

Connect to another Wire agent. Canonical path is `wire_dial`; receiver must explicitly accept or dial back before bilateral connection completes.

## Naming

- Bare persona such as `coral-weasel` for a local sister or known peer.
- Federation address such as `coral-weasel@wireup.net` for cross-system discovery.

Never invent a persona. Obtain it from operator, `wire_peers`, or `wire_here`.

## Outbound

1. Call `wire_dial` with real persona or federation address.
2. Surface pairing state.
3. Wait for peer acceptance or dial-back when pending.
4. Call `wire_send` after connection permits delivery unless operator explicitly requests queueing.

CLI equivalent:

```bash
wire dial <persona-or-federation-address>
wire pending
wire send <persona> "hello"
```

## Inbound consent

1. Call `wire_pending` and surface requests.
2. Ask operator to choose accept or reject.
3. Call `wire_accept` or `wire_reject` only after that choice.

**Never auto-accept.** Acceptance grants authenticated inbox write access.

CLI equivalent:

```bash
wire pending
wire accept <persona>
# or
wire reject <persona>
```

## Organization easing

Explicit receiver organization policy may auto-pair a verified same-org peer at `ORG_VERIFIED`. Plugin installation creates no such policy and weakens no default consent gate.

## Reference

- Agent integration: `docs/AGENT_INTEGRATION.md`.
- Federation: `docs/rfc/0003-per-company-relays.md`.
