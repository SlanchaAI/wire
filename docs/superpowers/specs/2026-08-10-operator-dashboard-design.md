# Wire Operator Dashboard Design

## Product vision

Wire needs one local control surface for the sessions an operator owns. The
first release covers one machine and 10–20 concurrent agent sessions. It shows
only sessions backed by a live agent host, lets the operator link two sessions,
and creates a named Wire group from selected sessions.

The dashboard is a topology tool. It does not display or send messages. It does
not expose historical identities, retire old sessions, or accept network
connections. A later release can add other operator-owned machines without
changing the local interaction model.

## Confirmed product choices

- Scope: one machine, owned by the current operating-system user.
- Inventory: live agent-host sessions only; daemon-only and historical homes are
  hidden.
- Scale: 10–20 live sessions.
- Direct link: selecting two sessions creates a bilateral local pair after one
  confirmation. No second peer-acceptance step is required on the same machine.
- Group: selecting two or more sessions creates one shared Wire group room. It
  does not create a pairwise mesh.
- Surface: browser UI bound to `127.0.0.1` and launched by a Wire command.
- MVP actions: inspect, link two, and create a group. Messaging and retirement
  remain outside this build.
- Layout: compact operations list with row selection and an action bar.
- Visual direction: Wire's Open Band system—paper, burgundy frame, green dial,
  phosphor status, serif headings, and monospace operational labels.

## Root-cause repair before dashboard work

The live Codex process exposes `CODEX_THREAD_ID`. Wire 0.17.0 originally read
`CODEX_SESSION_ID` but ignored the current variable, so MCP fell through to a
machine-default identity. Commit `eaca903` on `main` adds the current Codex
adapter. The installed binary must be rebuilt from the dashboard branch so the
fix reaches the runtime that launches MCP servers.

Goose 1.45.0 injects `AGENT_SESSION_ID` into standard-input/output extensions
and Developer shell commands. Wire will use that key only when `AGENT=goose`.
The guard matters because `AGENT_SESSION_ID` is a cross-agent convention, not a
Goose-specific name. Resolution precedence remains:

1. `WIRE_SESSION_ID`
2. `CLAUDE_CODE_SESSION_ID`
3. `CODEX_SESSION_ID`
4. `CODEX_THREAD_ID`
5. guarded `AGENT_SESSION_ID` when `AGENT=goose`
6. existing Copilot and VS Code adapters
7. existing Claude PID-file fallback

The runtime repair will:

1. Build and install the branch binary through the repository's normal install
   path.
2. Restart or reconnect the active MCP host so its process loads the new binary
   and resolves the current thread identity.
3. Read supervisor state and process ownership.
4. Stop only daemon or monitor processes proven to be unmanaged manual starts
   for the same Wire home. The supervisor and its per-session children stay
   intact.
5. Verify one session identity across host environment, MCP `wire_whoami`,
   daemon state, and dashboard inventory.

No wildcard process kill is permitted.

## Architecture

`wire dash --web` extends the existing dashboard command. It starts an Axum
server on `127.0.0.1` with a kernel-selected port, prints the complete URL, and
opens the default browser unless `--no-open` is set. Axum already ships in the
Wire dependency graph.

The server and terminal dashboard share one inventory producer. The browser
does not invoke shell commands or parse terminal output. Mutation routes call
Rust functions that also remain available to command-line callers.

The first release uses server-owned HTML, CSS, and a small JavaScript file
embedded in the Wire binary. It adds no Node runtime, package manager, browser
framework, or persistent web service.

### Components

1. **Session inventory**
   - Reads registered session homes through the existing session registry.
   - Joins persona, project directory, lifecycle lease, runtime-role PID files,
     peer state, and health state.
   - Includes a row only when a non-expired lifecycle lease belongs to a live
     agent-host process.
   - Produces a stable JSON shape used by the terminal renderer and web API.

2. **Local topology operations**
   - `link_local_sessions(a, b)` validates two distinct live sessions owned by
     the current user, then materializes the bilateral local pair in both homes.
   - `create_local_group(name, creator, members)` validates the creator and at
     least one other selected live session, creates the signed group in the
     creator home, and materializes the group into each member home.
   - Both operations return a verified result. A partial write returns an error
     that names completed and failed homes; the UI never reports success from an
     attempted call alone.

3. **Loopback web server**
   - Serves the Open Band operations list and JSON mutation routes.
   - Binds only to `127.0.0.1`; no configurable network host ships in the MVP.
   - Generates a random launch token and places it in the initial URL. The
     browser sends it on every mutation request.
   - Rejects missing or incorrect tokens before reading a mutation body.

4. **Browser client**
   - Polls inventory every two seconds.
   - Preserves selection for rows that remain live and drops vanished rows.
   - Enables **Link selected** for exactly two rows.
   - Enables **Create group** for two or more rows.
   - Uses a confirmation step for linking and a name/creator dialog for groups.
   - Shows success only from the verified server response and leaves failures
     visible until dismissed or retried.

## Inventory and API contracts

Each session row contains:

- stable session-home identifier, never the raw host session key;
- DID, handle, persona emoji, and palette;
- agent host label;
- project directory;
- session start time and age;
- direct-link count;
- health summary;
- whether the row is eligible for local link and group actions.

Routes:

- `GET /` — embedded application shell.
- `GET /api/sessions` — current live-session inventory.
- `POST /api/links` — `{ "sessions": [a, b] }`.
- `POST /api/groups` — `{ "name": name, "creator": a, "members": [a, b, ...] }`.

Mutation responses use one shape:

```json
{
  "ok": true,
  "message": "Linked cobalt-nettle and warmer-cedar",
  "changed_sessions": ["session-a", "session-b"]
}
```

Errors set `ok` to `false`, return a non-2xx HTTP status, and include no secret,
relay slot token, raw session key, or private filesystem state.

## Error handling

- A session that exits between selection and confirmation yields `409 Conflict`
  and a refreshed inventory.
- Repeating an existing link succeeds as an idempotent no-op.
- A duplicate group name follows the existing group-name rules and returns the
  existing domain error without overwriting a group.
- Invalid selection cardinality returns `400 Bad Request`.
- Missing launch token returns `403 Forbidden` before mutation parsing.
- Failure in one member home stops further group materialization and reports the
  exact completed/failed boundary. It does not claim group-wide success.
- Browser-open failure leaves the server running and prints the complete URL.

## Security boundaries

- Loopback bind is fixed for this release.
- Mutation requests require the random launch token and a JSON content type.
- Session identifiers are opaque home IDs; API payloads never accept arbitrary
  paths.
- Server resolution maps every ID back through the registered-session inventory
  before filesystem access.
- Same-machine automatic pairing relies on the existing operating-system user
  and machine-fingerprint trust model.
- The dashboard cannot accept, reject, send, retire, stop, or delete.

## Verification

Deterministic checks:

- Unit tests for guarded Goose resolution, precedence, placeholder rejection,
  and distinct session-home mapping.
- Inventory tests proving live lifecycle leases appear while daemon-only,
  expired, retired, and historical homes do not.
- Local-link tests proving both homes gain the same bilateral relationship and
  repeated calls remain idempotent.
- Group tests proving one creator and selected members receive the same signed
  room while unselected sessions remain unchanged.
- HTTP tests for token rejection, content-type rejection, invalid selection,
  stale-session conflict, and successful mutations.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, focused test
  suites, then `cargo test`.
- Browser proof through the real `wire dash --web --no-open` server: rows render,
  selection gates actions, link completes, group completes, refresh shows the
  resulting topology, and console/network logs contain no errors.
- Installed-runtime proof: a Codex thread resolves from `CODEX_THREAD_ID`; a
  Goose standard-input/output extension resolves from guarded
  `AGENT_SESSION_ID`; MCP, daemon, and dashboard agree on identity.

The baseline full suite has one observed parallel-only failure in
`os_notify::tests::toast_dedup_public_api_suppresses_repeat`; it passes alone.
Any completion claim must report whether that baseline flake recurs and must
show a green focused run for the test.

## Success criteria

1. The installed Wire binary resolves current Codex and Goose sessions to
   stable, distinct Wire identities without machine-default fallback.
2. Supervisor diagnostics report bounded managed workers and identify no
   unmanaged daemon serving the active session home.
3. `wire dash --web` opens an Open Band dashboard bound to `127.0.0.1`.
4. The main view shows only live agent-host sessions and remains usable with 20
   rows.
5. The operator can select two sessions and create a verified bilateral local
   link.
6. The operator can select two or more sessions and create one shared Wire group
   room without creating a full mesh.
7. The real browser path observes both mutations and the refreshed inventory.
8. Relevant deterministic checks pass, with baseline flake evidence reported.

## Deferred work

- Historical-session archive and retirement controls.
- Messaging and conversation views.
- Network exposure, authentication, and remote browser access.
- Operator-owned machine enrollment and federated inventory.
- Cross-machine linking or group creation from this dashboard.
- Topology graph and split list/map views.

The structural next step after the local MVP is an operator-owned machine
registry that supplies the same inventory contract. It should replace, not
fork, the local inventory source.
