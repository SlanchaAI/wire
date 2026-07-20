# Group invite materialization

## Goal

Directly added, verified group members discover the room after sync without requesting a `wire-group:` join code.

## Change

- `wire group list` now runs the existing verified-invite intake when identity state exists.
- Intake accepts a direct invite only when the event targets and the signed roster includes the local identity.
- The group integration test proves pull → list materializes the direct invite before any send or tail call.

## Scope and assumptions

- Reused the existing creator-signed roster validation, trust pinning, and group persistence paths.
- Kept join codes as the admission path for unpaired/link recipients.
- Did not change daemon behavior, group credentials, trust tiers, or invite consent semantics.

## Evidence

- Regression reproduced: `cargo test --test e2e_group group_bidirectional_room_with_introduce_pin -- --nocapture` failed before the code fix because the recipient listed zero groups after pull.
- Focused verification passed: `cargo fmt --check`, `cargo test --test e2e_group -- --nocapture`, and `cargo test --test cli group_list_empty_reports_no_groups -- --nocapture`.
- Full verification passed: `cargo test -q`.
- GitNexus impact: `cmd_group_list` had one direct caller / LOW risk; shared `ingest_group_invites` was HIGH risk due to `send` and `tail` callers, so its validation logic was unchanged.
- Independent review found that automatic listing would otherwise persist a valid roster for an excluded recipient; added recipient and roster-membership gates plus e2e wrong-recipient and excluded-recipient regressions.

## Artifacts

- `docs/superpowers/specs/2026-07-19-group-invite-materialization-design.md` — approved narrow design.
- `SESSION_LOG_2026_07_19.md` — implementation and verification record.
