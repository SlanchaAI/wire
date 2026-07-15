# Codex Wire Plugin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Package Wire's existing MCP server and shared workflows as a repository-distributed Codex plugin without duplicating runtime behavior or weakening pairing consent.

**Architecture:** Add a Codex manifest beside the Claude manifest and expose the repository root through a Codex marketplace entry. Both harnesses reuse `.mcp.json` and `skills/`; Rust integration tests guard manifest versioning, shared paths, marketplace policy, removed-command drift, and install documentation.

**Tech Stack:** Codex plugin JSON, MCP stdio configuration, Markdown skills, Rust integration tests with `serde_json`, Codex CLI, GitNexus CLI.

## Global Constraints

- Plugin version is exactly `0.17.0`, matching `Cargo.toml`.
- Rust runtime, protocol, identity, relay, and MCP tool implementations remain unchanged.
- Codex manifest lives at `.codex-plugin/plugin.json`; Claude manifest stays at `.claude-plugin/plugin.json`.
- Both manifests reuse `.mcp.json` and `skills/`; no copied `plugins/wire` tree.
- Codex manifest omits `hooks`; Claude manifest keeps its SessionStart hook.
- Marketplace installation does not install the `wire` binary.
- Inbound pair requests never auto-accept.
- Removed SPAKE2/SAS commands must not appear as executable guidance.
- Preserve all unrelated working-tree changes.

---

### Task 1: Package contract and manifests

**Files:**
- Create: `tests/plugin_contract.rs`
- Create: `.codex-plugin/plugin.json`
- Create: `.agents/plugins/marketplace.json`
- Modify: `.claude-plugin/plugin.json`

**Interfaces:**
- Consumes: `CARGO_PKG_VERSION`, `./skills/`, and `./.mcp.json`.
- Produces: root Codex plugin `wire`, repository marketplace `wire`, and static package-contract tests.

- [ ] **Step 1: Write failing package-contract tests**

Create `tests/plugin_contract.rs`:

```rust
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(relative: &str) -> Value {
    let path = repo_root().join(relative);
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn assert_shared_components(manifest: &Value) {
    assert_eq!(manifest["name"], "wire");
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest["skills"], "./skills/");
    assert_eq!(manifest["mcpServers"], "./.mcp.json");
    for field in ["skills", "mcpServers"] {
        let relative = manifest[field].as_str().expect("component path string");
        assert!(repo_root().join(relative).exists(), "missing {relative}");
    }
}

#[test]
fn plugin_manifests_share_components_and_track_release() {
    let claude = read_json(".claude-plugin/plugin.json");
    let codex = read_json(".codex-plugin/plugin.json");
    assert_shared_components(&claude);
    assert_shared_components(&codex);
    assert!(claude.get("hooks").is_some());
    assert!(codex.get("hooks").is_none());
    let mcp = read_json(".mcp.json");
    assert_eq!(mcp["mcpServers"]["wire"]["command"], "wire");
    assert_eq!(mcp["mcpServers"]["wire"]["args"], json!(["mcp"]));
}

#[test]
fn repository_marketplace_exposes_root_wire_plugin() {
    let marketplace = read_json(".agents/plugins/marketplace.json");
    assert_eq!(marketplace["name"], "wire");
    let plugins = marketplace["plugins"].as_array().expect("plugins array");
    assert_eq!(plugins.len(), 1);
    let wire = &plugins[0];
    assert_eq!(wire["name"], "wire");
    assert_eq!(wire["source"], json!({"source": "local", "path": "./"}));
    assert_eq!(wire["policy"]["installation"], "AVAILABLE");
    assert_eq!(wire["policy"]["authentication"], "ON_INSTALL");
    assert_eq!(wire["category"], "Developer Tools");
    assert!(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".codex-plugin/plugin.json")
        .is_file());
}
```

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test plugin_contract`

Expected: FAIL because Codex manifest and marketplace are absent; Claude manifest version is stale.

- [ ] **Step 3: Create Codex manifest**

Create `.codex-plugin/plugin.json`:

```json
{
  "name": "wire",
  "version": "0.17.0",
  "description": "Bilateral signed-message bus for local and federated AI-agent sessions.",
  "author": {
    "name": "Slancha AI",
    "email": "paul@slancha.ai",
    "url": "https://slancha.ai"
  },
  "homepage": "https://wireup.net",
  "repository": "https://github.com/SlanchaAi/wire",
  "license": "AGPL-3.0-or-later AND Apache-2.0 AND MIT",
  "keywords": ["agent", "p2p", "ed25519", "mailbox", "mcp", "identity"],
  "skills": "./skills/",
  "mcpServers": "./.mcp.json",
  "interface": {
    "displayName": "Wire",
    "shortDescription": "Secure agent-to-agent messaging",
    "longDescription": "Discover, pair, and exchange signed messages with local or federated AI-agent sessions through Wire.",
    "developerName": "Slancha AI",
    "category": "Developer Tools",
    "capabilities": ["Read", "Write"],
    "websiteURL": "https://wireup.net",
    "defaultPrompt": [
      "Show my Wire identity and nearby agents.",
      "Dial another agent over Wire.",
      "Send a message to a paired Wire peer."
    ]
  }
}
```

- [ ] **Step 4: Create repository marketplace**

Create `.agents/plugins/marketplace.json`:

```json
{
  "name": "wire",
  "interface": {"displayName": "Wire"},
  "plugins": [
    {
      "name": "wire",
      "source": {"source": "local", "path": "./"},
      "policy": {
        "installation": "AVAILABLE",
        "authentication": "ON_INSTALL"
      },
      "category": "Developer Tools"
    }
  ]
}
```

- [ ] **Step 5: Bring Claude manifest into release lockstep**

Change `.claude-plugin/plugin.json` fields:

```json
"version": "0.17.0",
"description": "Magic-wormhole for AI agents — bilateral signed-message bus over local and federated mailbox relays. Rust-native MCP server with Ed25519-rooted identity. Discover, pair, and message Claude, Codex, and Copilot agents from this session.",
```

Keep its `hooks` field unchanged.

- [ ] **Step 6: Confirm GREEN**

Run: `cargo test --test plugin_contract`

Expected: `2 passed; 0 failed`.

- [ ] **Step 7: Scope-check and commit**

```bash
git add tests/plugin_contract.rs .codex-plugin/plugin.json .agents/plugins/marketplace.json .claude-plugin/plugin.json
npx gitnexus detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "feat: package wire for Codex"
```

Expected: packaging/test files only; no Rust execution flow affected.

---

### Task 2: Modernize shared skills for Claude and Codex

**Files:**
- Modify: `tests/plugin_contract.rs`
- Modify: `skills/wire-enroll/SKILL.md`
- Modify: `skills/wire-init/SKILL.md`
- Modify: `skills/wire-monitor/SKILL.md`
- Modify: `skills/wire-pair/SKILL.md`
- Modify: `skills/wire-quiet/SKILL.md`
- Modify: `skills/wire-send/SKILL.md`

**Interfaces:**
- Consumes: v0.17 CLI/MCP surface and Task 1 manifests.
- Produces: one validator-compatible, harness-neutral skills tree.

- [ ] **Step 1: Add failing removed-command audit**

Append to `tests/plugin_contract.rs`:

```rust
#[test]
fn bundled_skill_command_audit_rejects_removed_pairing_surface() {
    let removed = [
        "wire pair-list-pending",
        "wire pair-confirm",
        "wire init <handle>",
        "/wire:wire-",
    ];
    for entry in fs::read_dir(repo_root().join("skills")).expect("read skills") {
        let skill_path = entry.expect("skill entry").path().join("SKILL.md");
        if !skill_path.is_file() {
            continue;
        }
        let body = fs::read_to_string(&skill_path).expect("read skill");
        for signature in removed {
            assert!(!body.contains(signature), "{} advertises {signature}", skill_path.display());
        }
    }
}
```

This is an exact syntax audit, not a semantic-output detector.

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test plugin_contract bundled_skill_command_audit_rejects_removed_pairing_surface`

Expected: FAIL on removed SAS/init commands and Claude-only slash-command references.

- [ ] **Step 3: Add valid skill names**

Add `name: <directory-name>` to each skill frontmatter. Exact names:

```yaml
name: wire-enroll
name: wire-init
name: wire-monitor
name: wire-pair
name: wire-quiet
name: wire-send
```

Each file receives only its matching line.

- [ ] **Step 4: Rewrite initialization workflow**

In `skills/wire-init/SKILL.md`, replace manual `wire init <handle>` flows with:

````markdown
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

- `wire` not found — install binary, then start a new agent task.
- relay unreachable — use offline mode or a reachable local/custom relay.
- `identity_split` non-null — restart stale MCP host.

## Organization identity

Use the `wire-enroll` skill.
````

Set the frontmatter description to:

```yaml
description: Bootstrap Wire with the canonical wire up flow, creating a per-session DID-derived identity and optionally binding local or federation relays. Use when the user says "wire init", "wire up", "set up wire", or asks how to start using Wire.
```

- [ ] **Step 5: Rewrite pairing workflow**

Replace `skills/wire-pair/SKILL.md` body with:

````markdown
# wire-pair

Connect to another Wire agent. Canonical path is `wire_dial`; receiver must explicitly accept or dial back before bilateral connection completes.

## Naming

- Bare persona such as `coral-weasel` for a local sister or known peer.
- Federation address such as `coral-weasel@wireup.net` for cross-system discovery.

Never invent a persona. Obtain it from operator, `wire_peers`, or `wire_here`.

## Outbound

1. Call `wire_dial` with real persona/address.
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

```bash
wire pending
wire accept <persona>
# or
wire reject <persona>
```

## Organization easing

Explicit receiver org policy may auto-pair a verified same-org peer at `ORG_VERIFIED`. Plugin installation creates no such policy and weakens no default consent gate.
````

Set frontmatter to:

```yaml
---
name: wire-pair
description: Connect this Wire session to another agent through bilateral dial and explicit inbound consent. Use when the user says "pair with X", "dial Y", "talk to Z", or names another peer they want to message.
---
```

Remove SAS commands, old tier ladder, and slash-command references.

- [ ] **Step 6: Rewrite listener workflow**

Replace `skills/wire-monitor/SKILL.md` with:

````markdown
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

Filter heartbeat/handshake noise in host monitor layer. Keep listener across loop iterations; stop only when session ends or operator says stop everything.

## Inbound requests

Call `wire_pending`, surface requests, and wait for operator consent. Never auto-accept.

## Multiple sessions

Each session has its own identity, MCP process, daemon state, and inbox cursor. Check or monitor each independently.
````

- [ ] **Step 7: Remove remaining harness-specific wording**

Apply these exact text replacements:

```text
skills/wire-quiet/SKILL.md
"every Claude tab's daemon" -> "every local agent session's daemon"
"Memory: `feedback_wire_upgrade_skips_mcp_servers` — sister Claude sessions' wire mcp subprocesses need `/mcp` reconnect to pick up the silenced binary if `wire upgrade` ran." -> "After `wire upgrade`, restart or reconnect each host-pinned `wire mcp` subprocess so it uses the new binary."

skills/wire-send/SKILL.md
"`mcp__wire__wire_send({peer: \"<nick>\", body: \"<body>\"})` from the assistant context. Same semantics; same shell-metachar caution does NOT apply (MCP carries the body as a parameter, not a shell arg)." -> "Call `wire_send` with `peer` and `body`. MCP carries the body as a parameter, so shell-metacharacter escaping does not apply."
```

Add `name` only to `wire-enroll`; its workflow is already harness-neutral.

- [ ] **Step 8: Confirm GREEN and validate manifest/skills**

```bash
cargo test --test plugin_contract
python3 /Users/laul_pogan/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py .
```

Expected: `3 passed; 0 failed`; `Plugin validation passed`.

- [ ] **Step 9: Scope-check and commit**

```bash
git add tests/plugin_contract.rs skills/*/SKILL.md
npx gitnexus detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "docs: make wire skills work across agent hosts"
```

---

### Task 3: Codex install documentation

**Files:**
- Modify: `tests/plugin_contract.rs`
- Modify: `README.md`
- Modify: `docs/PLUGIN.md`
- Modify: `SESSION_LOG_2026_07_14.md`

**Interfaces:**
- Consumes: marketplace/plugin name `wire` from Task 1.
- Produces: copyable plugin install, MCP-only fallback, duplicate-registration warning, and smoke workflow.

- [ ] **Step 1: Add failing install-signature audit**

Append to `tests/plugin_contract.rs`:

```rust
#[test]
fn codex_install_signatures_are_documented() {
    let signatures = [
        "codex plugin marketplace add SlanchaAi/wire",
        "codex plugin add wire@wire",
        "codex mcp add wire -- wire mcp",
    ];
    for relative in ["README.md", "docs/PLUGIN.md"] {
        let body = fs::read_to_string(repo_root().join(relative)).expect("read docs");
        for signature in signatures {
            assert!(body.contains(signature), "{relative} missing `{signature}`");
        }
    }
}
```

This audits exact copyable command syntax.

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test plugin_contract codex_install_signatures_are_documented`

Expected: FAIL because both documents lack Codex commands.

- [ ] **Step 3: Update README harness table and fallback**

Add row after Claude Code:

```markdown
| **Codex App / CLI / IDE** | `cargo install slancha-wire`, `codex plugin marketplace add SlanchaAi/wire`, then `codex plugin add wire@wire` | Start a new task; ask “Show my Wire identity” |
```

Add below table:

````markdown
Codex users who want only the tool surface can skip plugin packaging:

```bash
codex mcp add wire -- wire mcp
```

Plugin adds shared Wire skills and marketplace discovery; both routes run the same local `wire mcp`. Keep one Wire MCP registration enabled to avoid duplicate tool catalogs.
````

- [ ] **Step 4: Add Codex section to plugin guide**

Change title to `# Wire agent plugins`. Keep Claude instructions, then add:

````markdown
## Codex plugin

Codex manifest lives at `.codex-plugin/plugin.json` and reuses `.mcp.json` plus `skills/`. Binary remains separate:

```bash
cargo install slancha-wire
codex plugin marketplace add SlanchaAi/wire
codex plugin add wire@wire
```

Start a new task. Ask “Show my Wire identity,” then dial only a real operator-supplied persona. Inbound requests remain pending until explicit accept/reject.

### MCP-only Codex setup

```bash
codex mcp add wire -- wire mcp
```

MCP-only gives same tools without bundled skills/marketplace UX. Do not enable direct `mcp_servers.wire` and plugin-provided Wire MCP together unless testing duplicates.

### Local Codex development

```bash
codex plugin marketplace add /absolute/path/to/wire
codex plugin add wire@wire
```

Reinstall after manifest or skill changes, then start a new task.
````

Make later Claude-only headings explicit; retain hook instructions.

- [ ] **Step 5: Confirm GREEN**

```bash
cargo test --test plugin_contract
cargo test agent_docs_match_advertised_tools
```

Expected: four contract tests and MCP documentation consistency pass.

- [ ] **Step 6: Update session log**

Append to `SESSION_LOG_2026_07_14.md`:

```markdown
### Implementation findings

- Codex packaging reuses the repository root through marketplace source `./`.
- Codex skill validation requires explicit `name` frontmatter.
- Shared skills advertised removed SAS/init commands; corrected before Codex distribution.
- `wire mcp` keeps data synchronized, while Codex ingests messages through `wire_pull` and `wire_tail` at task checkpoints.

### Implementation artifacts

- `.codex-plugin/plugin.json` — Codex plugin manifest.
- `.agents/plugins/marketplace.json` — repository marketplace entry.
- `tests/plugin_contract.rs` — packaging and documentation contract tests.
- `skills/*/SKILL.md` — shared Claude/Codex workflows.
- `README.md` and `docs/PLUGIN.md` — install and operation guidance.
```

- [ ] **Step 7: Scope-check and commit**

```bash
git add tests/plugin_contract.rs README.md docs/PLUGIN.md SESSION_LOG_2026_07_14.md
npx gitnexus detect-changes --scope staged --repo wire
git diff --cached --check
git commit -m "docs: add Codex plugin installation"
```

---

### Task 4: Isolated install smoke and final verification

**Files:**
- Modify only when a verification failure identifies a defect in Tasks 1–3.

**Interfaces:**
- Consumes: complete plugin package and docs.
- Produces: local evidence that Codex discovers and installs the root plugin without touching normal user config.

- [ ] **Step 1: Run focused verification**

```bash
cargo fmt --check
cargo test --test plugin_contract
python3 /Users/laul_pogan/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py .
cargo test agent_docs_match_advertised_tools
```

Expected: every command exits zero.

- [ ] **Step 2: Smoke marketplace/install in isolated Codex home**

```bash
SMOKE_HOME="$(mktemp -d)"
CODEX_HOME="$SMOKE_HOME" codex plugin marketplace add "$PWD" --json
CODEX_HOME="$SMOKE_HOME" codex plugin list --available --json
CODEX_HOME="$SMOKE_HOME" codex plugin add wire@wire --json
CODEX_HOME="$SMOKE_HOME" codex plugin list --json
```

Expected: marketplace `wire` added; Wire appears available; install succeeds; final list reports `wire@wire`. Isolated home leaves `~/.codex/config.toml` unchanged.

If Codex rejects source path `./`, correct only marketplace source semantics and its test. Do not create duplicated plugin assets.

- [ ] **Step 3: Run full Rust suite**

Run: `cargo test`

Expected: zero failures.

- [ ] **Step 4: Run final scope checks**

```bash
npx gitnexus detect-changes --scope all --repo wire
git diff --check
git status --short
git log -4 --oneline
```

Expected: no Rust execution-flow changes; only intended feature files changed; unrelated dirty files untouched; design plus three implementation commits at tip.

- [ ] **Step 5: New-task product check**

After normal Codex install, start a new task and enter:

```text
Show my Wire identity and status.
```

Expected: Codex selects `wire_whoami` and `wire_status`. Dial only a real operator-supplied persona. Never fabricate peer or accept inbound request without explicit consent.
