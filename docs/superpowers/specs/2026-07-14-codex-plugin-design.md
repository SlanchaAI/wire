# Codex Plugin Design

**Date:** 2026-07-14
**Status:** Approved for implementation planning

## Problem

Wire already works in Codex through its stdio MCP server, `wire mcp`. That gives Codex the tool surface, but installation and behavior remain operator-configured: users must install the binary, register the MCP server, know which Wire tools to call, and remember pairing-consent and session-listener rules.

Claude Code already packages the same capabilities as a plugin. Codex users need equivalent product ergonomics without a second Wire implementation or a duplicated copy of the workflow assets.

## Goals

- Make Wire installable as a repo-distributed Codex plugin.
- Reuse the existing Rust binary, `.mcp.json`, and `skills/` directory.
- Make natural requests such as “dial grove-leaflet” and “send grove-leaflet …” route predictably to Wire MCP tools.
- Preserve per-session identity, bilateral pairing consent, and listener-lifetime rules.
- Keep Claude Code and Codex guidance on one shared workflow source where both harnesses support the same behavior.
- Provide deterministic validation and an end-user smoke path.

## Non-goals

- No new Wire protocol, relay, identity, or Rust behavior.
- No ChatGPT-hosted remote MCP app.
- No custom Codex status bar or unsupported hook emulation.
- No duplicate `plugins/wire` package tree.
- No automatic acceptance of inbound pair requests.
- No replacement for direct MCP configuration; MCP-only remains supported.

## Approaches Considered

### 1. Root-level dual-harness plugin — selected

Add `.codex-plugin/plugin.json` beside `.claude-plugin/plugin.json`. Both manifests point to the existing `.mcp.json` and shared `skills/` directory.

This keeps executable behavior in `wire mcp`, avoids copied skills, and lets each harness retain its own manifest. Codex-specific unsupported fields are omitted instead of forcing Claude hook semantics into Codex.

### 2. Nested `plugins/wire` package

Place a conventional Codex package below `plugins/wire`. Codex marketplace layout becomes straightforward, but `.mcp.json` and skills must be copied or linked into the archive. Copies drift; links are fragile across packaging and archive validation. Rejected.

### 3. MCP-only documentation

Document `codex mcp add wire -- wire mcp` and stop. This preserves capability but leaves discovery, behavioral guidance, and product distribution unresolved. Retained as fallback, rejected as primary Codex UX.

## Package Architecture

### Codex manifest

Create `.codex-plugin/plugin.json` at the repository root with:

- stable plugin name `wire`;
- version matching the released Wire crate version;
- product metadata for the Codex install surface;
- `skills` pointing to `./skills/`;
- `mcpServers` pointing to `./.mcp.json`;
- no `hooks` field while current Codex plugin validation rejects it;
- no app connector because Wire is a local stdio MCP server, not a hosted ChatGPT app.

The existing Claude manifest remains separate because its accepted schema and lifecycle hook support differ.

### Marketplace

Create `.agents/plugins/marketplace.json` as the repository marketplace. Its `wire` entry uses a Git-backed root-repository source for `https://github.com/SlanchaAi/wire`, with explicit installation policy, authentication policy, and category.

The marketplace is distribution metadata only. Wire continues to require the separately installed `wire` binary on `PATH`.

### Shared skills

Keep one `skills/` tree. Change only harness-specific prose that blocks Codex use:

- describe MCP tool intent before harness-specific command spelling;
- treat Claude’s persistent Monitor as one available listener implementation, not the universal mechanism;
- state that `wire mcp` provides the session listener/poll loop for Codex;
- retain operator consent before `wire_accept`;
- retain session-lifetime listener semantics;
- avoid claiming Codex supports Claude slash-command namespaces.

Claude-specific capabilities may remain as clearly labeled branches inside a shared skill when useful. Shared protocol and safety instructions must not fork.

### Documentation

Add Codex installation and smoke instructions without rewriting the Claude plugin guide. Document:

1. install `slancha-wire`;
2. add the Wire marketplace and plugin;
3. start a new Codex task so bundled MCP tools and skills load;
4. verify identity with `wire_whoami`;
5. dial or send to a real persona;
6. verify inbound pairing still requires operator approval.

Document MCP-only setup as the lean alternative for users who do not need bundled skills or marketplace discovery.

## Runtime Flow

1. Codex loads the installed plugin in a new task.
2. Plugin launches `wire mcp` from `.mcp.json`.
3. Wire resolves session identity through its existing session-key precedence, including `CODEX_SESSION_ID` where available.
4. MCP initialization advertises Wire tools and server instructions.
5. Bundled skills teach Codex the canonical dial, pending, accept/reject, send, and tail workflows.
6. User language maps to MCP calls; the Rust server remains the sole implementation of identity and transport behavior.

No plugin hook mutates trust state. No skill auto-accepts pairing.

## Failure Handling

- Missing `wire` binary: MCP startup must fail visibly; docs point to Cargo or release-binary installation.
- Plugin installed but tools absent: user starts a new task, then checks plugin enabled state and `codex mcp list`/plugin status.
- Duplicate direct MCP plus plugin MCP registration: docs tell users to keep one enabled source to avoid confusing duplicate tool catalogs.
- Missing session key: existing Wire fallback behavior applies; diagnostics use `wire_whoami`/`wire_status` rather than inventing plugin state.
- Offline relay: local identity and local-only workflows remain available; federation errors surface verbatim.

## Security Invariants

- `wire_accept` always requires operator consent for inbound peers.
- Plugin installation does not create trust relationships.
- Signing keys remain owned and read by the local Wire binary.
- Marketplace authentication policy describes installation behavior; it does not replace Wire’s cryptographic pairing.
- Shared skills must not weaken existing consent or identity instructions.

## Verification

Automated checks must cover:

- Codex manifest validation with the bundled `plugin-creator` validator;
- JSON validity for both manifests, MCP config, and marketplace metadata;
- all manifest-referenced paths exist and remain inside the plugin archive;
- shared skill frontmatter validates;
- existing documentation/tool-catalog consistency tests remain green;
- no Rust symbol or execution-flow changes appear in GitNexus change detection.

Product smoke must cover a clean local Codex install:

- marketplace discovers `wire`;
- plugin installs and enables;
- a new task exposes Wire tools;
- `wire_whoami` returns a session identity;
- a dial/send request selects Wire tooling;
- inbound pairing remains pending until explicit approval.

## Success Criteria

- A fresh Codex user can install Wire from repository marketplace metadata without manually editing `config.toml`.
- Codex loads `wire mcp` and the shared Wire skills from the plugin.
- Natural-language dial/send requests use the canonical Wire tool flow.
- Claude plugin behavior remains intact.
- MCP-only users remain supported.
- Plugin and marketplace validators pass from a clean checkout.

