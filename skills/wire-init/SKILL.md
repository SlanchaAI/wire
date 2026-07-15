---
name: wire-init
description: Bootstrap Wire with the canonical wire up flow, creating a per-session DID-derived identity and optionally binding local or federation relays. Use when the user says "wire init", "wire up", "set up wire", or asks how to start using Wire.
---

# wire-init

Set up Wire on a fresh machine or in a fresh per-session home. `wire up` is idempotent and owns identity creation, relay binding, persona claim, and daemon startup.

## When to use

- User says "set up wire", "wire init", "wire up", or "install wire".
- `wire_whoami` reports that no session identity exists.
- A new agent session needs its own DID-derived persona.

## Pre-flight

Verify the binary is on `PATH`:

```bash
command -v wire
```

If absent, install `slancha-wire` with Cargo or use a prebuilt release from https://github.com/SlanchaAi/wire/releases.

## Workflow

### Public relay

```bash
wire up @wireup.net
```

This mints the session identity, binds federation, claims the DID-derived persona, opportunistically adds local routing, and starts the daemon. Operator never chooses a separate handle.

### Offline identity

```bash
wire up --offline
```

Bind a relay later with `wire bind-relay <url>`.

### Custom relay

```bash
wire up https://relay.example.com
```

## Verify

Prefer `wire_whoami` and `wire_status` through MCP. CLI equivalent:

```bash
wire whoami --json
wire status --json
```

Verify DID-derived `persona`, `config_dir`, endpoints, daemon health, and `identity_split: null`.

## Common errors

- **`wire` not found** — install binary, then start a new agent task.
- **Relay unreachable** — use offline mode or a reachable local/custom relay.
- **`identity_split` non-null** — restart stale MCP host.

## Organization identity

Use the `wire-enroll` skill for operator and organization enrollment.

## Reference

- README: https://github.com/SlanchaAi/wire#pick-your-harness
- Agent integration: `docs/AGENT_INTEGRATION.md`.
