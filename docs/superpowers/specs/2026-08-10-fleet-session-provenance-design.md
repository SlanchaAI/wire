# Fleet-ready session provenance

Date: 2026-08-10  
Status: approved design  
Implementation scope: one machine

## Goal

Make each live dashboard row explain what the agent is, where it runs, and what work it owns. Preserve enough provenance for a future cross-machine dashboard without adding remote collection now.

Success means an operator can distinguish the harness, identity source, machine, repository, branch, worktree, and working directory. Missing facts remain unknown. Wire never presents an inference as an observed fact.

## Boundaries

This slice:

- enriches active local MCP sessions;
- records richer metadata for new leases;
- recovers safe facts from old live processes;
- adds compact rows with expandable details;
- keeps the server bound to loopback.

This slice does not:

- collect from another machine;
- spawn, stop, supervise, or budget agents;
- add task boards or workflow state;
- retire old identities;
- create permanent boss identities;
- depend on the public relay.

Remote aggregation, role delegation, and fleet control require a separate design after relay staleness and `daemon_seen: false` pass a sustained pressure test.

## Correct the data model

`session_source` records how Wire resolved an identity. It does not identify the agent harness. The dashboard must stop using it as the Agent column.

Each live session gains four separate descriptors.

### Machine

- stable machine fingerprint already available to Wire;
- operator-facing hostname;
- operating system;
- architecture;
- Wire version.

The local collector emits this descriptor now. A future fleet collector can merge reports by machine fingerprint without changing the session shape.

### Harness

- normalized kind: `codex-cli`, `chatgpt-codex`, `claude-code`, `claude-desktop`, `goose`, `cursor`, `vscode`, or `unknown`;
- display label;
- launch mode when observed: interactive, resume, app-server, or MCP host;
- confidence: `explicit`, `inferred`, or `unknown`;
- evidence class, never a raw command line.

Harness inference may inspect a bounded parent-process chain. It must match executable boundaries, not arbitrary command substrings.

### Identity

- Wire persona and DID fingerprint;
- identity source from the existing session resolver;
- classification: session-keyed, explicit override, registry fallback, or machine-default;
- warning when a live agent uses machine-default.

Machine-default stays visible because it marks an identity propagation defect. It must never appear as a harness name.

### Project

- repository name;
- repository root;
- process working directory;
- path relative to repository root;
- version-control branch;
- worktree name and worktree path;
- remote repository name or URL when present;
- confidence and evidence class.

Project discovery walks from the working directory to the nearest Git root. It reads `.git`, `HEAD`, worktree metadata, and repository config from the filesystem. It does not run one `git` process per session and does not compute dirty state.

## Source precedence

For each field, prefer:

1. explicit lease metadata written by the current MCP process;
2. session registry metadata;
3. cached live-process inference;
4. unknown.

New leases record the machine, harness, and project snapshot at acquisition. Heartbeats preserve acquisition time and refresh facts that were unknown.

Old live leases need immediate value. The collector takes one bounded process snapshot only when the active PID set changes, then caches results by PID. It must not spawn `ps` or `lsof` once per row or on every two-second browser poll.

Platform adapters may recover different fields:

- Linux reads `/proc` for parentage and working directory.
- macOS uses one bounded process snapshot and one bounded working-directory snapshot.
- Windows uses one bounded process snapshot; unavailable working directories remain unknown.

Probe failure leaves fields unknown and never blocks the inventory.

## API contract

The live-session report advances to a new schema version. It keeps existing topology and health fields and adds structured `machine`, `harness`, `identity`, and `project` objects.

The API exposes no raw thread ID, environment value, launch token, private key, slot token, or complete process command line. Full local paths remain available because this dashboard is operator-owned and loopback-only. A remote fleet endpoint must add explicit operator authentication and field policy before reusing them.

## Interface

The compact row shows:

- persona and handle;
- harness label and confidence marker;
- repository and branch;
- machine label;
- identity warning when needed;
- topology count and health.

Selecting the row does not toggle details. A separate detail control expands:

- full DID fingerprint and identity source;
- process and launch mode;
- repository root, relative directory, branch, worktree, and remote;
- machine fingerprint, operating system, architecture, and Wire version;
- provenance for inferred fields.

Unknown values render as `Unknown`, not a dash that could mean empty, unavailable, or not applicable.

## Future role and fleet model

Wire session DIDs stay disposable. A future conductor may assign a short-lived, signed lease that lets one session act for a logical role such as `ws2-critic`. The role is an operator-owned address, not a shared session private key.

ACP or a harness supervisor owns process lifecycle. Wire carries role requests, decisions, acknowledgements, and results. Short-lived workers join an operator-approved room or star topology instead of forming an all-to-all mesh.

No role or remote-control code lands in this slice.

## Verification

- Unit fixtures cover explicit, inferred, and unknown harnesses without substring false positives.
- Lease compatibility tests read old records and round-trip new metadata.
- Project fixtures cover a normal repository, linked worktree, repository subdirectory, detached HEAD, missing remote, and non-Git directory.
- Collector tests prove explicit metadata beats inference and probe failure fails open to unknown.
- A process-probe test proves work is bounded by snapshot, not session count.
- The dashboard end-to-end test includes Codex, Claude, Goose, machine-default identity, full project metadata, and unknown fields.
- Playwright verifies compact and expanded rows on desktop and mobile with zero console errors.
- Installed proof compares displayed harness and working directory against live process ancestry for sampled sessions.

## Fleet reliability gate

Before Wire becomes a fleet message bus, run at least twelve roles for forty-five minutes, restart the supervisor during active work, and prove:

- no lost work orders;
- idempotent duplicate handling;
- bounded acknowledgement latency;
- recovery after supervisor restart;
- role takeover by a replacement session;
- healthy sync state and `daemon_seen: true`.

Until that gate passes, files remain the durable work artifact and Wire remains an optional coordination channel.
