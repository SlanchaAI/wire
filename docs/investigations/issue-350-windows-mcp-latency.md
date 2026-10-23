# Windows MCP process and identity probe latency

Investigation for SlanchaAI/wire#350. Source baseline: `42d4346` (origin/main).

The remaining recurring PowerShell cost in the Model Context Protocol (MCP)
server is the full daemon scan in `wire_status`. `wire_send` already avoids that
scan. Identity ancestry still has an expensive, unbounded first-use walk, but
it is cached for the lifetime of each process. Treat startup latency and warm
request latency separately.

## Evidence and limits

This is a static, read-only investigation on Dell/Linux: no build, Windows
benchmark, relay request, or live application network call was performed.
Git fetch and draft-PR delivery are workflow operations, not runtime probes.
Source references below are primary evidence at the baseline above
`[P, repository source at 42d4346, TBD: verify]`; API replacements are proposals,
not implemented or benchmarked claims. Actual per-call milliseconds remain
unmeasured. Do not mistake existing comments' healthy-host estimates for data.

Search coverage: all tracked Rust PowerShell/pwsh references and their callers,
plus repository-wide references to separate installer, documentation, plugin
shell-tool forwarding, and completion strings from runtime process probes.
There are **four production launch sites**, plus one timeout-test launch.

## Every PowerShell process/identity launch

| Launch site (file:line) | Operation and callers | Per-call cost and bound |
| --- | --- | --- |
| `src/platform.rs:242`, `find_processes_by_cmdline` | Starts `powershell.exe`; enumerates `Win32_Process`, filters wire image and role substring, returns PIDs. Used by collision detection (`src/session.rs:1620`), daemon liveness (`src/ensure_up.rs:185`), supervisor orphan discovery (`src/daemon_supervisor.rs:1141`), status/doctor role scans (`src/cli/status.rs:1299`), and upgrade's daemon/relay/MCP scans (`src/cli/upgrade.rs:679`). | One interpreter startup + full Common Information Model (CIM) process enumeration + filtering/output parsing per invocation. Source comment `src/platform.rs:38` says well under 500 ms on healthy hosts; **unverified estimate**. Default receive timeout 5 s, overridden by `WIRE_PLATFORM_TIMEOUT_SECS`. |
| `src/platform.rs:316`, `pid_cmdline` | Starts `powershell.exe`; enumerates the process table then filters one PID's command line. Supervisor retirement validates a recorded worker (`src/daemon_supervisor.rs:155`); not a direct MCP handler call. | One startup + full enumeration for each candidate, despite needing one PID. Same source estimate and configurable 5 s timeout; actual cost unmeasured. Scales with eligible supervisor retirement candidates and polling. |
| `src/session.rs:1526`, `parent_pid` | Starts `powershell` with a PID-filtered CIM query for `ParentProcessId`. Used by `ancestor_chain_cached` (`src/session.rs:1463`) for Claude PID-file session recovery and identity-split detection (`src/session.rs:1228`). | One startup + filtered CIM query per ancestry hop. Up to 16 sequential launches on first use, including the final parent lookup (`src/session.rs:1417`). **No timeout**: direct `.output()` at line 1534. No defensible numeric upper bound. A cache hit launches nothing. |
| `src/session_metadata.rs:516`, Windows `capture_process_snapshot` | Starts `powershell.exe`; captures the whole process table as JSON (PID, parent PID, executable, command line), then selects requested roots/ancestors in Rust. Lease creation (`src/session_lifecycle.rs:83`), lease heartbeat (`src/session_lifecycle.rs:113`), and operator inventory (`src/operator.rs:472`) use the shared wrapper. | One startup + full enumeration + JSON serialization/deserialization per cache miss. Fixed 5 s receive timeout at line 524, independent of the platform environment override. No measured healthy-host estimate for this larger payload. Cache at `src/session_metadata.rs:308` refreshes only when the sorted/deduplicated PID key changes; failures are cached too, with no time expiry. |

The fifth launch, `src/platform.rs:756`, runs `Start-Sleep -Seconds 60` only in
`run_with_timeout_returns_none_and_kills_on_slow_command`; its test timeout is
500 ms. It is not production overhead.

Timeouts above bound the receive wait, **not total wall time**: spawn precedes
that wait and cleanup calls synchronous `taskkill.exe` with no timeout
(`src/platform.rs:68`, `src/platform.rs:95`). Thus 5 s is not a strict complete
call deadline. CIM failures return empty/unknown results, not proof of absence.

## Which costs occur on MCP requests?

No process probe runs unconditionally in the request dispatcher
(`src/mcp.rs:287`) or tool dispatcher (`src/mcp.rs:841`).

| Entry point | PowerShell calls attributable to it |
| --- | --- |
| Server startup, before dispatch | Session adoption (`src/mcp.rs:91`) may initialize ancestry: 0–16 parent queries, once per process. Lease acquisition (`src/mcp.rs:112`) captures metadata once on a cold PID-key cache. Collision warning (`src/mcp.rs:148`) runs four role scans if `WIRE_HOME` is present (`src/session.rs:1606`, role list at line 1574). Daemon/bootstrap work is conditional and separate from warm request handling. |
| Every `wire_status` | One full scan via `tool_status` → `daemon_liveness` (`src/mcp.rs:1215`, `src/ensure_up.rs:185`). Also checks recorded PIDs through `tasklist`, scans session files, and resolves homes. Identity-split annotation at `src/mcp.rs:1287` can pay the ancestry walk if it has not run yet; warm calls only reread PID files. |
| Every `wire_whoami` | Identity-split annotation (`src/mcp.rs:987`) uses the same cached ancestry. Usually zero PowerShell calls after startup; first use can pay 0–16. An intentional home pin suppresses the split check (`src/session.rs:1228`). |
| `wire_send` | Its successful local and federated result paths use `daemon_pidfile_alive` (`src/mcp.rs:1416`, `src/mcp.rs:1434`): zero PowerShell scans, at most one `tasklist` if the daemon PID exists. The old full-scan penalty is already removed; see `src/ensure_up.rs:155`. Transport latency is outside this investigation. |
| `wire_group_*` | `group_cli_json` spawns a fresh wire CLI (`src/mcp.rs:1107`). Process-local caches do not carry into that child; identity recovery can therefore pay cold ancestry costs again when its environment requires PID-file fallback. Do not infer zero cold costs from the long-lived MCP cache. |
| Other dispatch methods/tools | No unconditional call to the four PowerShell sites. `initialize`, `ping`, catalogs, and resource subscription dispatch have no process-probe preamble. Stateful initialization and spawned workers can incur their own startup costs; this is not a claim that all tools are free of subprocesses or other latency. |
| Lease heartbeat | Background work (`src/mcp.rs:198`), not a per-request hook. Reuses the singleton-PID metadata cache through `heartbeat_lease_at`; same key means zero PowerShell launches. A different key in the same process causes refresh, including a return to the original key. |

Let `Cscan` be measured scan time, `Cparent` measured parent-query time, `Cmeta`
metadata capture time, and `Clive` tasklist time. Cold startup's identified
PowerShell component is up to `16*Cparent + 4*Cscan + Cmeta`, when all those
paths run; it is not a total startup bound. Warm status pays `Cscan` plus
PID checks and filesystem work; warm whoami's ancestry component is zero;
send's liveness component is at most `Clive`. The supervisor's per-candidate
command-line queries can add machine contention independently of requests.

### Other Windows shell-outs that must not be mislabeled PowerShell

`process_alive` uses `tasklist.exe` (`src/platform.rs:150`); termination uses
`taskkill.exe` (lines 105 and 371). Machine identity uses `reg.exe` for
MachineGuid (line 457), and user identity uses `whoami.exe` for the security
identifier (SID, line 501). These are separate remaining subprocess costs.
Windows notifications currently print to stderr (`src/os_notify.rs:225`),
despite older comments mentioning PowerShell. Windows home lookup uses
session-role PID files (`src/session.rs:1738`, line 1766), not PowerShell to
read another process's environment.

## Ranked native replacements and tests to add

Ranking prioritizes recurring request cost, then unbounded startup, then
background amplification. No dependency or implementation is selected here;
validate API signatures, supported Windows versions, permissions, and Rust
bindings against the Windows SDK before implementation.

1. **Replace repeated role scans and liveness subprocesses.** Use a single
   `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` enumeration with
   `Process32FirstW`/`Process32NextW` for PIDs, parent PIDs, and image names;
   match owned wire roles from validated PID records where possible. Toolhelp
   does **not** supply full command lines. Preserve orphan discovery with
   command-line retrieval for candidates using in-process Windows Management
   Instrumentation (WMI) through COM (`IWbemServices::ExecQuery`), rather than
   silently dropping unrecorded daemons. Replace tasklist with `OpenProcess`
   and process wait/exit-state queries, closing handles on every path.
   Tests: live/exited/recycled PID, access denied as unknown, unrelated image,
   all four roles, sibling sessions, missing/stale PID records, supervisor
   exclusion, and unregistered orphan. Verify a warm `wire_status` and send
   liveness check spawn neither PowerShell nor tasklist. Preserve existing
   role-matching behavior deliberately; parser changes need separate cases.
2. **Replace the per-hop identity walk with one native ancestry snapshot.**
   Reuse Toolhelp's parent map for the bounded 16-hop chain; keep cached
   ancestry and PID-file retry semantics. This removes the unbounded
   PowerShell `.output()` path, not just its warm cost.
   Tests: direct/deep ancestors, missing parent, cycles, PID 0, exit races,
   depth cap, delayed Claude PID-file arrival, deliberate home pin, and
   distinct session identities. Count one snapshot on cold resolution and
   zero new native snapshots on repeated status/whoami checks. Never recover
   an identity from an unrelated recycled PID without corroborating evidence.
3. **Unify metadata and supervisor command-line queries.** Toolhelp supplies
   ancestry; `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` plus
   `QueryFullProcessImageNameW` supplies an executable path. For full command
   lines, in-process WMI removes interpreter startup but retains provider
   latency: query only candidates and batch where possible. Avoid undocumented
   remote process-memory scraping. Preserve the supervisor's command-line
   validation before termination; PID alone cannot establish ownership.
   Tests: inaccessible/exited processes, Unicode and quoted executable paths,
   null command lines, zero/one/many query results, COM initialization across
   threads, provider failure/deadline, changed PID-key invalidation, failed
   capture caching, and no killing unrelated processes. Measure both provider
   time and serialization savings; do not promise native WMI cannot wedge.
4. **Remove adjacent identity/termination subprocesses after hot paths.**
   Candidate mappings: `RegGetValueW` for MachineGuid; `OpenProcessToken` and
   `GetTokenInformation(TokenUser)` with SID conversion for user identity.
   Preserve byte normalization because fingerprints depend on it. Process
   handles plus `TerminateProcess` do not reproduce `taskkill /T`; tree
   handling needs an explicit design (for example owned Job Objects).
   Tests: identical existing identity hashes without logging raw identifiers,
   missing registry values, denied token access, handle cleanup, and owned
   child-tree termination that never affects unrelated processes.

## Decisive follow-up measurement

On an authorized Windows test host, with networking disabled and an isolated
fixture home, measure each probe separately and cold startup versus repeated
JSON-RPC `wire_status`/`wire_whoami` calls in one persistent server. Disable
bootstrap via `WIRE_MCP_SKIP_AUTO_UP` and automatic identity creation via
`WIRE_AUTO_INIT=0`; use a prepared local fixture identity. Test send's liveness
helper directly rather than sending a message. Record OS/build, process and
session counts, sample count, median/p95, cache state, subprocess count, and
failure/timeout cleanup behavior. Repeat with many stale session records to
expose filesystem and tasklist scaling. Redact command lines and identifiers.

Acceptance for a future code change: existing semantic tests plus the cases
above pass; observed Windows request execution shows eliminated shell launches
and unchanged identity/orphan decisions. This document does not claim that
performance fix or Windows measurement has occurred.

Document validation: the fleet file-existence/PowerShell proof and `git diff
--check` pass. GitNexus detect-changes was attempted but no `wire` repository
index is registered locally; no code symbols were edited.
