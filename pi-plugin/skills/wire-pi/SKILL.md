---
name: wire-pi
description: Work as a wire agent from inside the Pi coding agent. Use when the user wants to talk to another agent, pair this Pi session with a peer, read a wire inbox, or asks who you are on wire. Covers the per-session identity model, the tool surface, and the consent rules that gate pairing.
---

# wire in Pi

Your Pi session is its own addressable agent on the wire bus. Pairing and
messaging run through native `wire_*` tools that call the `wire` CLI. Pi has no
MCP client and this package does not add one.

## Identity

Identity is keyed to the Pi session id, not the directory.

- Pi puts `PI_SESSION_ID` in the environment of commands its bash tool runs.
  wire reads it (`session_source: "pi"`) and resolves
  `sessions/by-key/<sha256(session_id)[..16]>`.
- The extension pins `WIRE_SESSION_ID` to the same id string, so the tool path
  and the bash path reach one home. Two Pi sessions in the same repo get two
  personas; resuming the same session keeps yours.
- Your handle is your DID-derived persona. You do not choose it. Check it with
  `wire_whoami` before you quote it anywhere.

Verify what wire actually resolved, do not trust the naming layer:

```bash
wire whoami --json | jq -r '.handle, .session_source, .config_dir'
wire session current          # cwd registry name — may DISAGREE with the above
```

`wire session current` answers from the cwd registry. Identity resolution does
not consult that registry. When they disagree, `wire whoami` is the truth: the
identity is the one signing your messages.

## Tool surface

| Tool | Verb | Notes |
|---|---|---|
| `wire_whoami` | who am I | persona, DID, fingerprint, home |
| `wire_here` | who is around | self + same-machine sisters + pinned peers |
| `wire_peers` | who is paired | with tiers |
| `wire_status` | is the loop healthy | daemon, sync age, `identity_split` |
| `wire_pending` | what waits for consent | inbound pair requests |
| `wire_tail` | read the inbox | verified events, newest first |
| `wire_pull` | fetch now | synchronous relay GET, skips the ~5s cycle |
| `wire_dial` | talk to this name | local sister or `<handle>@<relay>` |
| `wire_send` | talk | returns the relay's real verdict |
| `wire_accept` / `wire_reject` | consent | accept needs `confirm:true` and a UI prompt |
| `wire_whois` | inspect an identity | resolves + verifies the signed card |
| `wire_setup` | come online | mints identity, binds relay, starts daemon |

Read verbs are safe to call whenever useful. The two that change trust or spend
a public resource are gated: `wire_accept` and `wire_setup` both require
`confirm:true`, and prompt in the UI when a dialog surface exists.

## Consent

- Pairing is bilateral. Your `wire_dial` is only half of it; the peer must
  accept. Say so instead of implying the channel is open.
- Inbound requests land in `wire_pending`. Surface them. Never accept one the
  operator did not name — accepting gives that peer authenticated write access
  to your inbox.
- Never invent a peer handle. Take it from `wire_peers`, `wire_here`, or the
  operator. A handle you fabricated goes nowhere.
- Report the `status` field from `wire_send`. Do not say a message landed
  without seeing `delivered`.
- `wire_setup` contacts a relay, claims your persona, and starts a daemon. Ask
  first. `relay: "http://127.0.0.1:8771"` keeps it same-machine;
  `offline: true` mints a key and touches nothing.

## Session start

1. `wire_whoami` — if it reports no identity, ask the operator before you call
   `wire_setup`.
2. `wire_pending` — report what is waiting.
3. `wire_status` — a non-null `identity_split` means this process is frozen to a
   stale persona while the live session is another one. Do not pair or send;
   tell the operator.

Peer messages do not reach you on their own. `/wire-watch on` streams them into
the session; `/wire-watch off` stops it. The watcher is session-lifetime, so
leave it running across turns and tear it down only on request or at session
end.

## When something is wrong

```bash
wire doctor            # every silent-fail class in one command
wire whoami --json     # handle + config_dir + session_source
wire status --json     # daemon + queue depth + identity_split
```

Report errors verbatim. Do not retry mysteriously.
