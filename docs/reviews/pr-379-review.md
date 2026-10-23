# Review of SlanchaAI/wire#379

Reviewed https://github.com/SlanchaAI/wire/pull/379 at head
`08b45aa8a8a40bc8b093284ae1343ea5ce39735c` on 2026-10-22.
The proposed change adds twelve lines to `AGENTS.md`; no executable code changes.

## Findings

- **Low — AGENTS.md:256 — Cloud setup is not reproducible from these instructions.**
  The paragraph says the environment install selects stable Rust and installs
  `bc`, but supplies neither setup commands nor a reference to that install
  configuration. A later Cloud session without the same saved environment
  cannot repair the stated Rust 1.83 toolchain gap from this section alone;
  the listed Cargo gates fail before validating the project. Add explicit
  rustup toolchain/component setup and package installation instructions, or
  link the maintained environment setup, plus commands to verify the selected
  compiler and required utilities. This is a nonblocking documentation issue:
  the linked `CONTRIBUTING.md` already states the minimum Rust version.

## Evidence and limits

- Read the PR metadata and full diff using `gh pr view` and `gh pr diff`.
  Inspected supporting files directly from the reviewed Git object.
- `Cargo.toml:5-6` confirms edition 2024 and minimum supported Rust version 1.88.
  `.github/workflows/ci.yml` contains all three documented Cargo commands and
  builds the release binary for shell integration tests.
- `tests/it/README.md` gives the explicit `WIRE=./target/release/wire` invocation;
  `tests/it/lib.sh` checks `jq` and `curl` and manages temporary homes and relays.
  `scripts/hello-world-validate.sh` uses `bc` and supplies relay/daemon teardown.
  `src/cli/demo.rs` starts a loopback relay with a cleanup guard.
- `src/nuke.rs:83-90` records service-uninstall failures as warnings and proceeds
  to state removal. `tests/it/40-nuke-recovery.sh` asserts the resulting identity
  state, consistent with the documented missing-user-bus behavior.
- Observed passing GitHub checks for fmt, clippy, test, docs-lint, linux-e2e,
  and install-smoke-windows on the reviewed PR. These are existing CI results,
  not local reruns. No destructive lifecycle commands or messaging were run.
- The Cloud image version, saved install configuration, and exact VM bus error
  were not independently reproduced. The review checks repository consistency,
  not the current contents of the externally managed Cloud image.
- GitNexus tools and CLI were unavailable in this worker; scope was checked
  directly with Git. This review changes no symbols or execution flows.

## Verdict

SHIP. No blocking defect found in the documentation change at the reviewed
head. Make environment setup reproducible as a follow-up.
