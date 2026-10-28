# SlanchaAI/wire#352: v0.16.0 daemon pileup and doctor hangs

The release contains two concrete failure paths: non-atomic daemon ownership
checks permit duplicate starts, and unbounded process probes can block doctor
before it prints anything. A large count can also mean one daemon per session,
not duplicate ownership. Source inspection alone does not establish which path
caused the reported regression or identify its introducing commit.

## Evidence boundary

Read-only investigation of release `v0.16.0`, commit
`07b60f175e8349774a2456c90d58484b59bba225`, compared with `origin/main` at
`42d4346`. Unless explicitly marked current, every `file:line` below refers to
that release commit (retrieve with `git show v0.16.0:<file> | nl -ba`).
The issue symptom comes from the fleet row; no issue comments, host process
snapshot, or incident logs were fetched. No build, daemon, doctor, relay probe,
or live runtime network call was run. Git transport and draft-PR delivery are
separate from the investigation. Findings are code facts; hypotheses are ranked
inferences, not reproduced incident conclusions. Source trust: [P, repository
source at the pinned commits, TBD: verify].

## Spawn and detection trace

| Entry or operation | Release source | Behavior |
| --- | --- | --- |
| Model Context Protocol (MCP) startup | `src/mcp.rs:91`, `:106`, `:114–117` | Adopts session home, bootstraps identity, then ensures a daemon for an initialized home unless auto-up is skipped. |
| `wire up` | `src/cli/setup.rs:205` | Calls the same `ensure_daemon_running` helper. |
| Bootstrap helper | `src/ensure_up.rs:112–117`, `:521–575` | Reads home-local `daemon.pid`, skips if its PID is alive; otherwise spawns current executable with `daemon --interval 5` at line 559, waits for process existence, then writes child's PID at line 574. |
| Supervisor | `src/daemon_supervisor.rs:398–405`, `:471–497`, `:551–572` | Enumerates eligible sessions, skips tracked children and live pidfiles, then spawns a child with explicit `WIRE_HOME`. No overall worker cap in this loop. |
| Supervisor detection | `src/daemon_supervisor.rs:585–605` | Reads `<home>/state/wire/daemon.pid`; accepts JSON PID or legacy integer and checks liveness. |
| Child startup guard | `src/cli/relay.rs:1104–1124` | Checks singleton holder, returns if another PID is alive, otherwise claims pidfile. `--once` and the singleton test override bypass this guard. |
| Singleton detection and claim | `src/ensure_up.rs:403–430` | Reads PID, excludes self, checks process existence, then separately writes its own record. No ownership lock spans check and claim. |
| Diagnostic detection | `src/ensure_up.rs:157–216` | Reads own pidfile, scans command lines, enumerates sessions and supervisor, then filters orphan candidates by home. This is broader and costlier than bootstrap detection. |

The shared PID check delegates to `platform::process_alive`
(`src/ensure_up.rs:147–148`). Linux checks `/proc/<pid>` existence; other Unix
uses `kill -0`; Windows uses `tasklist.exe` (`src/platform.rs:29–57`). Despite
the singleton helper's description, it does not authenticate command line,
process start time, or home. PID reuse can suppress a needed spawn; process
probe errors can falsely allow one.

## How duplicate spawns can occur

1. Two parents read the same absent/dead pidfile at
   `src/ensure_up.rs:530` before either writes. Both call `spawn`.
2. Both children can check `daemon_singleton_holder` before either claims.
   Each then writes its own PID and proceeds. `claim_daemon_singleton` uses
   `std::fs::write` through `write_pid_record` (`src/ensure_up.rs:279–282`),
   not an exclusive lifetime lock. Last writer wins the record, not the work.
3. Parents also write child records after their liveness checks. A parent can
   overwrite the record of a child that has already claimed ownership. Some
   schedules cause one child to exit; that does not eliminate the schedule
   where both pass the check before either claims.
4. A read during a truncating pidfile write can see empty or partial JSON.
   The normal reader maps these to Missing/Corrupt with no PID
   (`src/ensure_up.rs:259–274`), enabling another spawn. A lost pidfile or
   false-negative process probe has the same effect. The supervisor's
   check-then-spawn can race an MCP or manual start too.

The drop guard checks whose PID it sees before removal
(`src/ensure_up.rs:448–463`); it does not serialize startup. Its read/remove
sequence is also not atomic. The 500 ms readiness budget
(`src/ensure_up.rs:510–518`, `:566`) checks process existence, not successful
singleton ownership or a completed sync. A blocking process probe can exceed
that nominal budget.

Not every pileup is this race: the supervisor starts one worker for every
eligible initialized home. Its seven-day idle filter reads `last_sync.json`
among activity files (`src/daemon_supervisor.rs:81–91`, `:130–136`), while each
successful daemon cycle refreshes that file (`src/ensure_up.rs:342–365`).
An otherwise abandoned session that keeps syncing can remain eligible. Count
workers by canonical home and owner before calling them duplicates.

## Why doctor can block

`cmd_doctor` evaluates its full check vector before either JSON or text output
(`src/cli/status.rs:929–956`). Both daemon health and PID consistency invoke
`daemon_liveness` (`:1047`, `:1158`), repeating the expensive scan.

On Windows, liveness executes unbounded `tasklist.exe .output()`
(`src/platform.rs:47–49`), and enumeration executes unbounded PowerShell
`Get-CimInstance Win32_Process` (`:127–134`). A stuck probe prevents the first
check from finishing. Even without a permanent wedge, `list_sessions` probes
liveness for each home (`src/session.rs:491–494`, `:595–598`, `:624–630`), so
many homes amplify delay. The global scan and per-session probes are repeated
by the second diagnostic check.

On Linux the PID check is a filesystem lookup, but the global `pgrep` still
uses unbounded `.output()` (`src/platform.rs:91–94`). macOS additionally shells
out for each candidate's home (`src/session.rs:1114–1118`). Thus Windows has
the clearest expensive-probe path; it is not evidence that Dell reproduced it.

Doctor also calls relay health synchronously (`src/cli/status.rs:1246–1276`)
and can heal local endpoint state (`:1399` onward). It is neither an offline
probe nor a safe read-only reproduction command for this row. A slow relay
check is a separate possible delay; it does not explain duplicate spawning.

## Ranked hypotheses and discriminating evidence

1. **Unbounded process enumeration explains the silent doctor hang.** Strong
   source mechanism, especially on Windows. A blocked stack in tasklist/CIM
   versus relay health distinguishes it. Later commit `00edb89` (#298) wraps
   Windows probes with timeouts; its existence supports the mechanism, not
   attribution of this incident.
2. **Session fanout explains a large persistent daemon population.** No cap
   in the release loop and sync-driven activity keeps homes eligible. Distinct
   homes with one worker each support this; multiple workers for the same home
   require hypothesis 3 or another ownership failure.
3. **Check/write races or unreadable pidfiles explain same-home duplicates.**
   The interleaving above is possible without network failure. Correlate spawn
   timestamps, parent IDs, home, and pidfile ownership to discriminate it from
   ordinary fanout. Missing live evidence prevents ranking this as the incident's
   confirmed root cause.
4. **The v0.16.0 session-layout change is a regression-specific candidate.**
   Commit `f4a933f` (#269) changes `session_dir` to a by-key hash and
   `list_sessions` to ignore legacy top-level homes
   (`src/session.rs:517–525`). Old processes can survive while discovery and
   new starts use a different home. Compare pre-upgrade and post-upgrade home
   resolution on a synthetic legacy tree; do not assume an automatic migration
   or claim that this commit introduced the older singleton race.
5. **Probe failure or PID-format mismatch permits an unnecessary restart.**
   Bootstrap requires full JSON while the supervisor accepts an integer.
   Missing/partial records or tool errors are treated as no owner. Conversely,
   PID reuse can produce false health. These require fixture-specific evidence.

## Tests to add in a follow-up change

- Barrier-controlled concurrent startup in one temporary home: two bootstrap
  callers plus a supervisor/manual caller; release both after ownership reads.
  Assert exactly one long-lived worker and one valid owner record. Fake sync
  and record worker entry locally; do not depend on a relay or timing sleeps.
- Inject a pause between pidfile truncation and write; race a reader. Cover
  missing, malformed, legacy integer, stale PID and unrelated live PID records.
  Assert explicit ownership behavior, not merely successful process spawn.
- Many synthetic by-key homes: distinguish one worker per home from duplicate
  owners; assert a worker budget and that idle sessions can age out despite
  background sync. Test bound/unbound sessions and recent real activity.
- Doctor with injected hanging tasklist/CIM/pgrep adapters: assert a global
  deadline, useful partial results and child cleanup in text and JSON modes.
  Count probes across hundreds of homes so a per-probe timeout multiplied by
  session count cannot masquerade as a bounded diagnostic.
- Legacy top-level and by-key fixtures across the layout transition: assert
  consistent home resolution and ownership reporting with surviving old
  workers. Include unreadable process-home metadata and sibling homes.
- Inject a stalled relay-health response separately; assert doctor identifies
  that check and does not erase already collected local findings.

Current main already includes Windows timeout tests
(`src/platform.rs:719`, `:745`, `:818`), a supervisor worker cap
(`src/daemon_supervisor.rs:81`, `:627`), and topology-reporting changes
(`b214738`). Extend those rather than proposing them as wholly absent today.
The release mechanisms above must not be presented as a verified current-main
reproduction. No tests were added or run beyond the row's documentation proof;
implementation and runtime reproduction remain follow-up work.
