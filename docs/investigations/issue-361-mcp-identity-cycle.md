# SlanchaAI/wire#361: MCP identity lifetime after upgrade or resume

A long-lived Model Context Protocol (MCP) server pins its session home during startup. Updating the executable or changing the host's live session does not rerun that startup path. Reconnect the host-owned MCP subprocess with the correct session context to refresh it; restarting a daemon alone does not change the MCP identity.

## Evidence and limits

Read-only source investigation at `42d4346` (origin/main). No Wire processes, relay calls, builds, identity mutations, or runtime reproduction were performed. References below are repository-relative file:line locations at that revision. The mechanism is established by source; whether a particular host actually preserves its subprocess across resume remains a host-specific reproduction question. GitNexus tools and its referenced local skill were unavailable; source inspection supplies the trace.

## Resolution trace

| Consumer | Resolution and lifetime |
| --- | --- |
| MCP server | `src/mcp.rs:74`, `src/mcp.rs:91`: `run` adopts a home before spawning threads. `src/session.rs:1906` implements adoption; `src/session.rs:2074` sets process-local `WIRE_HOME`. Subsequent requests retain that environment. |
| Session key | `src/session.rs:1057`: `resolve_session_key` prioritizes `WIRE_SESSION_ID`, then Claude, Pi, and Codex adapters, followed by other host adapters and Claude ancestor PID-file lookup (`src/session.rs:1094`). `src/session.rs:1549` maps the raw key to a deterministic by-key home. |
| Explicit pin | `src/session.rs:1917`: an existing by-key home pin wins; presence of `WIRE_HOME_FORCE` also preserves a legacy pin. Otherwise a resolved session key supersedes a legacy home. Re-running adoption against the already adopted by-key home would short-circuit, not discover a new session. |
| Missing key | `src/session.rs:1993`: MCP retries the Claude PID-file race, then mints a random per-process key. `src/session.rs:2027` deliberately avoids placing this minted key in `WIRE_SESSION_ID`. The home still remains pinned for this process. MCP does not fall back to a shared cwd identity. |
| Identity reads | `src/mcp.rs:938` reads the agent card for `wire_whoami`; `src/config.rs:31`, `src/config.rs:608` resolve/read the card under `WIRE_HOME`. This is a pinned directory, not necessarily a permanently cached card. `src/mcp.rs:1598` bootstraps only if that home is uninitialized. |
| Statusline | `assets/wire-statusline.sh:38` takes the current stdin `session_id`, exports `WIRE_SESSION_ID`, and invokes a fresh `wire whoami --json` at line 42. CLI adoption runs at `src/cli/mod.rs:1913`. A fresh CLI can therefore resolve a different home from a surviving MCP. Explicit inherited home pins still follow adoption precedence. |
| Daemon | CLI startup uses the same adoption helper. `src/cli/relay.rs:1087` additionally resolves `--session` and pins that home before config reads. MCP starts daemon orchestration at `src/mcp.rs:120`; `src/ensure_up.rs:573` spawns a child inheriting its environment. Supervisor workers instead clear `WIRE_*` and explicitly set the selected home at `src/daemon_supervisor.rs:970`. These are separate processes, with separate lifetimes. |

The statusline's green dot only checks the daemon PID under the home returned by its own `whoami` (`assets/wire-statusline.sh:50`). It does not prove that the MCP process serves that same home.

## Two independent stale states

1. **Old executable:** installation replaces the executable on disk (`src/cli/upgrade.rs:258`). An existing process retains its loaded code. Upgrade explicitly counts MCP processes separately from daemon replacement (`src/cli/upgrade.rs:685`); it does not reconnect their host-owned stdio transport. `wire_whoami` reports `server_version`, `daemon_version`, and a mismatch warning (`src/mcp.rs:994`, `src/mcp.rs:1019`). Missing daemon version or matching version does not prove the correct identity; equal-version rebuilds also evade a version-only comparison.
2. **Old home:** a startup race can pin a minted home before the host PID-file appears, or a retained process can outlive a host session change. Its children and tool calls continue using that home while a fresh statusline invocation resolves the live key. Resuming the *same* stable key and home should preserve identity; reuse itself is not a bug. A fixed launcher override can also intentionally or accidentally collapse distinct sessions, and reconnecting with that same override will reproduce the problem.

`detect_identity_split` (`src/session.rs:1228`) compares the operational card with the live Claude ancestor PID-file's session card. MCP exposes it through `identity_split_json` (`src/mcp.rs:1297`). It is diagnostic, not a repair: explicit home pins suppress it, and missing cards/session information return no split. It is not a general Codex/Goose resume detector. A null field therefore cannot establish universal agreement.

## Cycling and re-keying options

**Preferred: host-managed reconnect.** Have the host close the old stdio connection and spawn the installed Wire binary with the intended stable session key and appropriate home-pin policy. Wire handles stdin EOF by stopping and joining its watcher/writer (`src/mcp.rs:255`). Source messages recommend `/mcp` reconnect (`src/mcp.rs:1302`); exact host UI support needs verification. If reconnect does not refresh launch context, restart the host session with corrected configuration. Check both the MCP-reported identity and a fresh CLI/statusline identity afterward, plus reported binary versions. These are proposed operator actions, not actions taken by this investigation.

**Restart daemon only.** Useful for old sync code, but insufficient for either MCP-loaded code or its pinned environment. Keep daemon cycling scoped to the intended home; sibling sessions may still own their daemons and leases.

**New identity via a new stable key and a new MCP process.** Appropriate only when intentionally creating a different session identity. Launch against its distinct home and initialize through the normal lifecycle. A restart with the same key normally reuses the same existing card. Changing a shell variable cannot update a running child's environment. Preserve the old home's trust, inbox/outbox, and lifecycle state; deleting its key/config is not a safe reconnect procedure. `wire_init` is idempotent (`src/mcp.rs:1626`), not a session-switch operation.

**Future in-process rebind or self-reexec.** Neither is implemented by the traced adoption path. A rebind would need to drain requests, stop inbox subscriptions/watchers, release the old lease, choose and validate the new home, rebuild state, and coordinate daemon ownership before accepting more work. Mutating process environment after threads start violates the adoption helper's safety contract (`src/session.rs:2070`). A host-mediated restart is smaller and preserves transport ownership; automatic exit/reexec would require an explicit host restart contract and in-flight request policy. No implementation proposed in this doc-only row.

## Tests to add

Use temporary homes, synthetic cards/PID-file fixtures, controlled subprocess environments, and `WIRE_MCP_SKIP_AUTO_UP=1`; keep network disabled. No tests below were added or run in this investigation.

- **Retained MCP across session change:** initialize under key A, update a synthetic host PID-file to B, and request `wire_whoami` again. Assert operational identity remains A and split reports B when both cards exist. Close stdin, relaunch with B, and assert identity B with no split. Exercise the stdio request path, not just the pure helper.
- **Startup race:** cover PID-file arrival within retry and after retry exhaustion. In the latter case, establish that the minted operational home persists until reconnect and does not populate `WIRE_SESSION_ID`. Extend `retry_recovers_a_pidfile_that_arrives_late` (`src/session.rs:1284`).
- **Resume stability and isolation:** restart twice with one stable key and assert the same DID/home; use two distinct keys from the same cwd and assert different homes. Include Codex thread identifiers and explicit override precedence.
- **Statusline versus MCP:** feed a current session ID to the statusline using a local stub executable; assert the child receives that override. Pair with the retained-process fixture to show the split and reconnect convergence. Cover inherited explicit home pins.
- **Upgrade boundary:** simulate differing server/daemon version records and verify MCP response fields; missing/equal versions must not imply identity agreement. Existing pure coverage is `mcp_stale_binary_note_flags_only_real_mismatch` (`src/mcp.rs:2180`); add response-level coverage without running a real upgrade.
- **Pin and diagnostic limits:** preserve deliberate home pins, suppress their split warning, and cover unknown live cards plus non-Claude hosts. Extend `identity_split_only_when_both_known_and_differ` (`src/session.rs:1247`) and `deliberate_home_pin_suppresses_split` (`src/session.rs:1384`).
- **Lifecycle cleanup:** close old MCP stdin and assert process exit and lease cleanup, then start its replacement; verify no old watcher notifications cross the boundary. Daemon ownership assertions should use fixtures and exclude sibling homes.

## Deliverable proof

`test -s docs/investigations/issue-361-mcp-identity-cycle.md && grep -qiE 'identity' docs/investigations/issue-361-mcp-identity-cycle.md`

This checks the requested documentation artifact only; it does not claim runtime repair or a reproduced host incident.
