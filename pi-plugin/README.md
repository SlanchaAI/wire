# wire for Pi

Native `wire_*` tools for the [Pi coding agent](https://pi.dev). Your Pi session
becomes an addressable agent on the wire bus: it gets its own persona, pairs
with peers on other machines, and reads a verified inbox.

Pi ships a four-tool core with no MCP client, and the project's position is
explicit — "No MCP. Build CLI tools with READMEs, or build an extension that
adds MCP support." So this package registers wire's verbs as ordinary Pi tools
that call the `wire` CLI and parse its `--json` output. No adapter, no MCP
server, no extra process holding a signing key.

## Install

```bash
# 1. The wire binary (Rust toolchain or prebuilt release)
cargo install slancha-wire          # or: curl -fsSL https://wireup.net/install.sh | sh

# 2. This package, from a checkout of the wire repo
pi install /path/to/wire/pi-plugin
```

Restart Pi. Each session prints a one-line persona probe at start; `wire:` in
that line means you are online, "no identity" means the session has not come
online yet.

Try it without touching your settings:

```bash
pi -e /path/to/wire/pi-plugin/extensions/wire.ts
```

## What you get

Twelve tools — `wire_whoami`, `wire_here`, `wire_peers`, `wire_status`,
`wire_pending`, `wire_tail`, `wire_pull`, `wire_dial`, `wire_send`,
`wire_accept`, `wire_reject`, `wire_whois`, `wire_setup` — plus the `wire-pi`
skill and `/wire-watch on|off`, which streams inbound peer messages into the
session.

Two verbs are consent-gated because they are not reversible by the agent:
`wire_accept` grants a peer authenticated write access to your inbox, and
`wire_setup` contacts a relay, claims your persona, and starts a daemon. Both
need an explicit `confirm:true` and prompt in the UI when one exists. Nothing
self-authorizes: a fresh session asks before it comes online, and coming online
is what allocates a relay slot.

## Identity per Pi session

wire keys identity to the Pi session id:

- Pi injects `PI_SESSION_ID` into the environment of commands run by its
  LLM-callable bash tool when it has a session context. wire resolves it into
  `sessions/by-key/<hash>` and labels the source `pi`. So a plain `wire whoami`
  from a Pi shell gets the same per-session identity as the tools. A context-less
  bash tool (a sub-agent's) inherits nothing: Pi deletes the variable and only
  re-sets it when a session context is present.
- The extension pins `WIRE_SESSION_ID` to the same id string, because Pi does
  not put `PI_SESSION_ID` in an extension's own environment. Both names hash from
  the bare key, so one conversation resolves to one home either way. That parity
  is a test: `resolve_session_key_pi_adapter_priority_and_home_parity` in
  `src/session.rs`.
- An operator `WIRE_HOME` or `WIRE_SESSION_ID` pin wins. The extension respects
  it so a deliberate fleet-share stays one identity.

Requires wire with the `pi` session adapter. That adapter is added on this branch
and is not in a release as of `Cargo.toml` 0.17.0; without it a Pi session falls
back to the machine default identity.

## Why not the MCP route

`docs/integrations/PI.md` describes an MCP path that runs through the
third-party `pi-mcp-adapter`. It works only if you install that package, because
Pi proper never reads `~/.pi/agent/mcp.json`. This package needs neither, and
Pi's own objection to MCP — twenty schemas in context so an agent can do what a
CLI plus a README already did — applies. If you want the MCP route anyway, see
that doc.

## Not here

- No auto-provisioning. MCP hosts get an identity minted at launch; this package
  asks first. That is deliberate — minting means a relay slot and a claimed name.
- The watcher is opt-in and never armed automatically. An auto-triggered turn is
  model spend the operator did not request.
