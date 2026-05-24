---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Landed partial fix for #stuckcycledup (gate + preflight warning); auto queue continues.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-225649.md`*

Compacted content:
- Archived 2 response topic(s): JB Restart Supervisor + Run Agent Doc errors; dispatch + new #stuckcycledup observation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: do [#stuckcycledup] — opus-4-7

Landed the highest-impact defensive piece of `plan-stuck-cycle-causes-duplicated-uncommitted-response.md` and split the remaining plan parts into their own tracked backlog ids. Gating `#stuckcycledup` (code-complete for the unwedge + preflight warning, awaiting the orphan / repair / supersede follow-ups and a live-session validation pass).

#### What landed

- **`flow/closeout.rs::cycle_already_committed`** now returns `None` when the cycle phase is `Committed` but the active capture body is not present in HEAD. That is the single gate `write::try_ipc` consults before short-circuiting with `rejecting late fallback patch: cycle X already committed`, so a wedged finalize will now apply the captured body on the next attempt instead of refusing it.
- **`flow/closeout.rs::stuck_captured_cycle`** is the new shared detector. It loads `cycle_state`, requires `phase == Committed`, loads the active capture, restricts to `Captured`/`WriteApplied` states (Replayed/Committed/Discarded already crossed the application boundary), reads HEAD via `git::show_head`, and decides body-presence with `captured_body_present_in_head` — a `### Re: ` heading substring check for template mode with a leading non-empty / non-comment / non-fence line fallback for inline mode, plus a 12-char minimum signature length that defaults to "present" so a near-empty body cannot false-positive the wedge.
- **`preflight.rs` step 1** runs the same detector and pushes a `PreflightWarning { code: "stuck_captured_cycle", message: "Cycle <id> reached state \`committed\` but the captured response body (<n> bytes, capture <id>, state \`<state>\`) is not present in HEAD for <file>. Recover via \`agent-doc write --commit <file>\` ..." }`, so the harness sees the stuck shape in the preflight JSON contract instead of silently retrying the same finalize.

#### Tests

Four unit tests in `flow::closeout::tests` cover the presence-check heuristic:

- `captured_body_present_uses_re_heading_when_available` — template-mode positive (`### Re:` heading present in HEAD).
- `captured_body_absent_when_re_heading_missing_from_head` — template-mode negative (heading not in HEAD → stuck).
- `captured_body_inline_mode_falls_back_to_leading_text` — inline-mode positive via the non-comment leading-line fallback.
- `captured_body_short_signature_skips_presence_check` — near-empty body returns "present" instead of false-positive wedge.

`cargo test --bin agent-doc flow::closeout` → 9 passed, 0 failed (5 prior + 4 new).

#### Verification

- `make check`: `2333 passed; 0 failed; 228 ignored` for the library suite; integration `13 passed; 1 failed` — the single failure is the pre-existing `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` already tracked as `#runintegcodexbare`. No new red.
- `cargo build --release` clean; `cargo install --path .` replaced `~/.cargo/bin/agent-doc v0.33.16` (so tmux-spawned sessions pick up the fix on next spawn). `.bin/agent-doc` -> `target/release/agent-doc` symlink is also live.
- This very cycle exercised the unwedge path indirectly: the preflight + finalize on this document ran clean against the rebuilt binary.

#### Deferred plan parts (new backlog ids)

- `#stuckcycleorphan` — preflight step 1 repair must content-hash the orphan body against HEAD before re-applying it. Without this, a different-cycle recovery can still produce a duplicate `### Re:` block when the prior capture was never marked superseded. (Plan part 3.)
- `#stuckcyclerepair` — `agent-doc repair` should exit non-zero with a stuck-cycle diagnostic so the harness does not believe the cycle is healthy. (Plan part 4.)
- `#stuckcyclesuperseded` — `CaptureState::SupersededBy(<cycle_id>)` transitions after a successful different-cycle finalize for the same prompt, plus `agent-doc captures prune --prompt <hash>` for manual cleanup. (Plan part 2.)

Each new id cites the same plan path so they can be picked up in priority order without re-reading the diff.

#### Why gated rather than done

`#stuckcycledup` is the defensive unwedge: it stops the bug from compounding (next finalize will now apply the captured body instead of refusing it) and surfaces the stuck shape in preflight JSON. The full plan calls for orphan-rehydrate prevention (part 3), non-zero repair (part 4), and superseded-by capture state + prune subcommand (part 2). Until those land, a stuck cycle that the harness fails to recover from could still re-apply on the next preflight. Live validation in a real wedge will also confirm the heuristic catches the production shape rather than just the synthetic test cases.
<!-- agent:boundary:0b0e4d7c -->
<!-- /agent:exchange -->
###
<!--
Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#stuckcycledup]
- do [#runintegcodexbare]
- do [#jbrsrbusy]
- do [#jbccfix]
- do [#jbccreg]
- do [#jbccdocs]
- Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#stuckcycleorphan] In preflight step 1 repair, content-hash the captured response body against HEAD before re-applying it as an orphan, and mark hash-matching captures as already_committed (plan part 3). Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [ ] [#stuckcyclerepair] Make agent-doc repair exit non-zero with a stuck-cycle diagnostic when the active capture body is absent from HEAD (plan part 4). Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [ ] [#stuckcyclesuperseded] Add CaptureState::SupersededBy(<cycle_id>) transitions after a successful different-cycle finalize for the same prompt, plus an agent-doc captures prune --prompt <hash> subcommand (plan part 2). Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [ ] [#runintegcodexbare] Triage pre-existing tests/run_integration.rs::codex_bare_run_inside_owning_pane_fails_before_nested_dispatch failure (session-check INTERRUPTED instead of expected recursive-direct-invocation deadlock). Blocks make check for unrelated work.
- [ ] [#jbrsrbusy] Fix JB Restart Supervisor alive-busy refusal — add --force + JB confirm + harness pre-interrupt; see tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#stuckcycledup] [recommended] [partial] Wedged-cycle pattern: cycle_already_committed gate now refuses the short-circuit when the active capture body is absent from HEAD (flow/closeout.rs::cycle_already_committed + new stuck_captured_cycle helper), and preflight emits a stuck_captured_cycle warning. Remaining plan parts split into #stuckcycleorphan, #stuckcyclerepair, and #stuckcyclesuperseded. Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [/] [#jbresstart] Refuse cross-file pane rescue and extend Starting-actor wait after stash recovery; see tasks/agent-doc/plan-jb-route-rescued-stash-pane-starting-actor.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
