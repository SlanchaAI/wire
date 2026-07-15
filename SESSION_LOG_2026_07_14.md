# Session Log — 2026-07-14

## Codex Wire ergonomics

### What

Investigated whether Wire already ships a Codex plugin. Repository had a Claude Code plugin (`.claude-plugin/plugin.json`) and shared MCP config (`.mcp.json`), but no `.codex-plugin/plugin.json` or Codex marketplace entry. Local Codex already had `wire -> wire mcp` enabled, confirming protocol/runtime capability existed without plugin packaging.

### Why add a plugin

MCP supplies tools. Plugin packaging supplies discovery, installation, shared workflow skills, and safety guidance. Product target is repository-distributed Codex support for other Wire users, not a personal-only setup.

### Decision

Use a root-level Codex manifest alongside the Claude manifest. Reuse `.mcp.json` and `skills/`; do not create a nested copied plugin tree. Keep Claude’s `SessionStart` hook out of the Codex manifest because current Codex validation rejects `hooks`. Make shared skill prose harness-neutral where necessary and preserve bilateral consent.

Rejected:

- nested `plugins/wire`: duplicates or fragile-links package assets;
- MCP-only docs: capability works, but distribution and behavioral ergonomics remain weak.

Design: `docs/superpowers/specs/2026-07-14-codex-plugin-design.md`.

### Current phase

Implementation and verification complete: packaging, shared-skill modernization, install documentation, isolated Codex install smoke, plugin/skill validation, and full Rust regression suite.

### Implementation findings

- Codex packaging reuses the repository root through marketplace source `./`.
- Codex skill validation requires explicit `name` frontmatter.
- Shared skills advertised removed SAS/init commands; corrected before Codex distribution.
- `wire mcp` keeps data synchronized, while Codex ingests messages through `wire_pull` and `wire_tail` at task checkpoints.
- The local plugin validator requires PyYAML; `uv run --with pyyaml` provides it without changing project dependencies.
- Isolated `CODEX_HOME` smoke discovered `wire@wire` v0.17.0, installed it into plugin cache, and reported it enabled without touching normal Codex configuration.
- Final `cargo test -q` exited 0 across unit, integration, stress, and doctest batches; only pre-existing ignored tests remained ignored.

### Implementation artifacts

- `.codex-plugin/plugin.json` — Codex plugin manifest.
- `.agents/plugins/marketplace.json` — repository marketplace entry.
- `tests/plugin_contract.rs` — packaging and documentation contract tests.
- `skills/*/SKILL.md` — shared Claude/Codex workflows.
- `README.md` and `docs/PLUGIN.md` — install and operation guidance.

## Artifacts

- `docs/superpowers/specs/2026-07-14-codex-plugin-design.md` — product design, invariants, packaging, and verification criteria.
- `docs/superpowers/plans/2026-07-14-codex-plugin.md` — test-first implementation and verification plan.
- `SESSION_LOG_2026_07_14.md` — session decisions and artifact catalog.
