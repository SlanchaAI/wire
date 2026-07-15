---
name: wire-monitor
description: Keep a Wire session synchronized and surface peer messages using the listener mechanism available in the current agent host. Use at session start, when the user asks to watch Wire, or during active peer collaboration.
---

# wire-monitor

Wire daemon and MCP server keep relay inboxes synchronized. How messages enter model context depends on host.

## Codex and generic MCP hosts

1. Call `wire_status` at session start.
2. Confirm `daemon_running: true`, recent sync, and `identity_split: null`.
3. Call `wire_pull` for an immediate relay fetch.
4. Call `wire_tail` at collaboration checkpoints or on operator request.

MCP does not inject unsolicited tool results into an active model turn. Wire can receive and notify in background, but Codex calls `wire_tail` to ingest messages into task context.

## Hosts with persistent command monitors

Arm once for session lifetime:

```bash
wire monitor --json --include-handshake
```

Filter heartbeat and handshake noise in host monitor layer. Keep listener across loop iterations; stop only when session ends or operator says stop everything.

## Inbound requests

Call `wire_pending`, surface requests, and wait for operator consent. Never auto-accept.

## Multiple sessions

Each session has its own identity, MCP process, daemon state, and inbox cursor. Check or monitor each independently.

## Reference

- MCP server instructions returned by `wire mcp`.
- Agent integration: `docs/AGENT_INTEGRATION.md`.
