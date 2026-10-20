# Claude Code handoff: agent-doc status sync interval and issue #316

Generated: 2026-10-19T22:22:00-04:00

Last updated: 2026-10-19T22:59-04:00

## Live continuation checkpoint

- The status-interval subagent has been resumed in its preserved worktree.
- Issue #316 commit `188fad6374b1d1cd2f79dac9fc708b4d77a1fcfb` has now been cherry-picked into the integration worktree as `7431a565b3`.
- The integration branch is not yet tested or pushed; it is waiting for the interval commit before the one combined verification run.
- Repository verification confirmed that Claude Code and Codex use the installed unfiltered `PostToolUse` bridge after completed tool calls. OpenCode, Grok, and Cursor use polling/command boundaries. No hook can interrupt a tool call still in progress.
- Interval checkpoint: the obsolete SQLite periodic-claim helper and its tests have been removed; docs now identify the controller Lazily graph as the live timing authority. The refactored hook integration test and controller cadence test pass. Focused parser/RPC tests, clippy, and broader checks remain in progress.
- Interval implementation is now committed and pushed as `7d67a33ccb105e66b9370c3f70b8d5b4280def37`; focused status-sync tests, targeted Clippy with warnings denied, `cargo build -p agent-doc`, and `git diff --check` passed.
- The interval commit has been cherry-picked into the integration branch as `38976ca0ee`. The combined integration HEAD now contains both queue tasks; authoritative `make check` and `make tmux-ci` are next.
- First combined `make check` attempt reached 6,843/11,116 tests before stopping on one deterministic failure: `component_attr_warning_accepts_valid_schedule_and_flags_bad_values` still expected the old `queue-only attribute` wording for misplaced `interval`; the implementation now correctly reports `queue-or-status attribute`. The interval worker is preparing the minimal expectation fix. This failure is not being treated as flaky or bypassed.
- The stale assertion was fixed in pushed follow-up `b180f2a4d2efe30070ca92a057ce57626ccf554f` and integrated as `9646a2b934`.
- The corrected combined `make check` is green: 11,116 Rust tests passed (173 skipped), 202 VS Code tests passed, cross-editor simulation passed, all JetBrains variants built/tested, 8 bootstrap tests passed, docs audit had no blockers, and the cached TLA proof was valid.
- `make tmux-ci` is also green, including the previously failing `two_ide_lane_claims_start_panes_in_their_own_lanes_end_to_end` case and all live clear/layout/resync suites. Install, installed-version verification, and push remain.
- `make install` completed once. The CLI/native library and VS Code extension were installed; JetBrains artifacts were already byte-identical. Runtime reload was scheduled for idle boundaries, with one editor correctly deferred because its document was mid-cycle.
- Installed verification: `agent-doc 0.35.525`; integration worktree clean.
- Integration HEAD `9646a2b934a6eb58906394b01ec37b4e89cd2a2d` was pushed to `origin/main` (`6aa595da2d..9646a2b934`). CI, Book, and Pages runs were queued at handoff update time; local verification is complete and no release was published.

## Objective

Continue the active work from `tasks/agent-doc/agent-doc-bugs.md` and finish both queue heads:

1. `#gh-fix https://github.com/btakita/agent-doc/issues/316`
2. `Should we have a \`interval=...\` attribute on the \`agent:status\` to preserve tokens?`

The operator clarified the desired behavior:

- `interval` defaults to off. `sync` without `interval` runs at preflight only.
- An explicit `interval` enables mid-turn status-sync reminders.
- The scheduling decision must be owned by agent-doc's Lazily state graph, not by a SQLite polling/claim loop.
- A hook may carry an observation or deliver a due edge at a safe in-turn boundary, but it must not own scheduling.
- This should keep `agent:status` current during very long turns that may contain one or more releases.

## Required workflow

1. Read the repository `AGENTS.md` and `.claude/skills/agent-doc/SKILL.md` completely before acting.
2. Use `tsift` for code navigation.
3. Enter the session with the harness-native command for `tasks/agent-doc/agent-doc-bugs.md` and follow its preflight/claim instructions. The previous Codex cycle is being closed with this handoff; do not assume its in-memory ownership continues.
4. Do not overwrite unrelated dirty state in `/home/brian/work/btakita/agent-loop`.
5. Finish implementation in the external worktree(s), integrate serially, run the required checks once on the combined result, install once, then persist the final agent-doc response and run `agent-doc session-check`.

## Completed issue #316 fix

Subagent work is complete:

- Worktree: `/home/brian/worktrees/agent-doc-issue316`
- Branch: `fix/issue-316-status-closeout`
- Commit: `188fad6374b1d1cd2f79dac9fc708b4d77a1fcfb`
- Pushed to origin.
- GitHub issue #316 closed at 2026-10-20T02:21:22Z: https://github.com/btakita/agent-doc/issues/316
- Worktree is clean.

The fix adds exact ledger-derived ownership proof for a late reported-handoff `agent:status` projection, gated to `WriteApplied`/`Committed`. Unrelated status edits still fail closed.

Verification already passed in that worktree:

- Focused regression tests.
- `cargo test -p agent-doc-commit-io`.
- Full `make check`: 11,117 Rust tests passed, 173 skipped; 202 VS Code tests; JetBrains classic/262/263 builds and tests; 8 bootstrap tests; cached TLA proof.

The agent-doc queue completion was reported for generation 1, but it is still `awaiting integration`. Integrate this commit before retiring/consuming the queue item.

## In-progress status interval work

The prior subagent was stopped after producing this checkpoint so Claude can take over without concurrent edits.

- Worktree: `/home/brian/worktrees/agent-doc-status-sync-interval-20261019`
- Branch: `fix/status-sync-interval-20261019`
- Base: `6aa595da2d feat: checkpoint persistent status sync mid-turn`
- State: uncommitted worktree, intentionally preserved.

Current design:

- `agent:status sync="..."` remains preflight-only by default.
- A status-only `interval` attribute opts into repeated checkpoints.
- Duration syntax is queue-compatible, with a minimum of 60 seconds.
- `agent-doc-controller-io/src/project_controller.rs` now contains a keyed ProcessScope Lazily graph:
  - Sources: `status_sync_observation`, `status_sync_receipt`
  - Computed: `status_sync_due`
  - An atomic controller transition consumes a due edge and advances the receipt from the observed safe boundary.
- `agent-doc-controller-io/src/project_controller/rpc.rs` adds the typed observation RPC.
- `agent-doc-session-check-io/src/midturn_steering.rs` is intended to be owner-only hook ingress plus prompt rendering.
- SQLite must not arbitrate the live due claim.

Currently modified files:

- `agent-doc-controller-io/src/project_controller.rs`
- `agent-doc-controller-io/src/project_controller/rpc.rs`
- `agent-doc-document/src/status_projection.rs`
- `agent-doc-preflight-command-io/src/run.rs`
- `agent-doc-queue/src/component_attrs.rs`
- `agent-doc-session-check-io/src/midturn_steering.rs`
- `agent-doc-sqlite/src/state_store.rs`
- `docs/reference/tags-and-attributes.md`
- `specs/02-document-format.md`
- `Cargo.lock` (local path-resolution noise; restore only this known generated change after confirming its diff)

Checks already passed during the rewrite:

- `cargo check` for controller-io and session-check-io.
- New controller graph cadence test.
- Parser/attribute/preflight/session focused tests passed before the Lazily rewrite.

Remaining interval work:

1. Finish refactoring the session hook test to inject the checkpoint decision. Unit tests do not have a live controller. The initial compile error was only the removed `status_sync_checkpoint_key`; helper functions were added, but the test body still needs conversion.
2. Ensure `midturn_steering.rs` no longer calls `claim_periodic_project_runtime_state_in_db`.
3. Rewrite docs that still describe SQLite atomic claiming so they accurately describe controller-owned Lazily state and safe hook-boundary delivery.
4. Decide whether the now-unused `claim_periodic_project_runtime_state_in_db` helper and tests introduced by base commit `6aa595da2d` should be removed. If retained, keep the two Rust 1.99 clippy fixes: `.and_then(parse_due)` rather than `.and_then(&parse_due)`.
5. Add/finish tests proving:
   - missing `interval` creates no periodic state and no reminder;
   - explicit interval becomes due at the expected safe boundary;
   - duplicate observations do not emit duplicate reminders;
   - changing cycle or interval creates a new generation;
   - non-owner hooks cannot consume the due edge;
   - a captured/closed cycle does not emit reminders;
   - ordinary mid-turn steering remains additive with the sync reminder.
6. Run formatting carefully, focused tests, clippy, then the repository checks.
7. Commit and push the interval branch, then report the queue completion for generation 1 using the current queue text.

## Integration worktree

Use the existing clean integration worktree unless its state has changed:

- `/home/brian/agent-doc-extract-projection-integ`
- Branch: `integrate/extract-projection-fix-20261019`
- Last known HEAD: `6aa595da2d`

Recommended order:

1. Confirm the integration worktree is clean and fetch origin.
2. Cherry-pick `188fad6374b1d1cd2f79dac9fc708b4d77a1fcfb`.
3. Finish, commit, and push the interval work; cherry-pick that commit.
4. Review the combined diff, especially the exact ownership proof for #316 and the absence of SQLite live arbitration for status cadence.
5. Run exactly one authoritative `make check` on the combined result and run `make tmux-ci`.
6. Run `make install` once after integration. Let the normal idle reload mechanism handle the installed binary; do not use `admin recycle`.
7. Verify the installed `agent-doc --version`, repository cleanliness, and issue #316 closure.
8. Push the integrated HEAD to `origin/main` if all checks pass.

Known CI context:

- Hosted Rust 1.99 previously flagged two `needless_borrows_for_generic_args` instances in `agent-doc-sqlite/src/state_store.rs`; the interval worktree contains the `.and_then(parse_due)` fixes.
- A prior unrelated hosted tmux run failed `two_ide_lane_claims_start_panes_in_their_own_lanes_end_to_end`; local `make tmux-ci` should determine whether the combined change is healthy.

## Release/status facts

Last verified release:

- Agent-doc release: `v0.35.525`
- Published: `2026-10-19T22:23:24Z`
- JetBrains assets included plugin versions `0.2.720`, `0.2.721`, and `0.2.722`.

No newer release was observed during this cycle. Do not claim that the unintegrated work is published.

## Final agent-doc closeout

After integration, testing, install, and push:

1. Report interval queue completion with generation 1 and exact current item text.
2. Ensure both queue items are retired/consumed only after their commits are integrated.
3. Respond through `agent-doc respond tasks/agent-doc/agent-doc-bugs.md --stream --origin skill`.
4. Include all operator chat prompts from this turn verbatim as `> **Chat prompt (#chatprompt):** ...` in `agent:exchange`, including the request that produced this handoff.
5. Run `agent-doc session-check tasks/agent-doc/agent-doc-bugs.md` exactly once after `respond` completes.

Do not resend or force a retained closeout if the controller reports that it is already handling it; obey the returned recovery instruction.
