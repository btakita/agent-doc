---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
L1–L4 safeguards implemented. Root bug remaining: route.rs spawns new stash panes without evicting existing ones — stash pane eviction fix needed. Manual pane kills are whack-a-mole until then.
❯ In the src/session-share directory, why was the last patchback to tasks/claudescore.md uncommitted?

❯ Why was there no response patchback to this document?

❯ Investigation: the current `agent-doc-bugs2.md` Codex session started on April 22, 2026 at 21:55:08 EDT (`.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log` shows `session_start` + `codex_start`), but the root `.agent-doc/logs/ops.log` has no later `preflight_diff_start`, `write_origin`, or `commit_*` entries for this file after that start. So this was not a failed patchback; no patchback cycle ran at all. The evidence lines up with the current state: `.agent-doc/pending/` has no pending-response file for this document hash, `.agent-doc/patches/` is empty, and the snapshot for this doc still ends at the older stash-pane note while the live file contains the newer questions. In plain terms: the latest work happened outside the document's normal `agent-doc preflight -> write -> commit` loop, so nothing wrote a response back into the document.

❯ I did not ask you to make changes. I only asked why there was no response patchback.
❯ > Why was there no response patchback to this document?

❯ Why did this happen? I used the normal workflow if `Run Agent Doc` from the JB Editor. This should have run the "document's normal `agent-doc preflight -> write -> commit` loop". Are you drifting from the spec again? DO NOT CHANGE ANY FILES.
❯ > In plain terms: the latest work happened outside the document's normal `agent-doc preflight -> write -> commit` loop

❯ Findings:

❯ - The workflow was normal. The failure point is inside `agent-doc`, not in how `Run Agent Doc` was invoked from JetBrains.
❯ - My earlier "outside the normal loop" phrasing was too strong. The better statement is: the normal entrypoint was used, but the loop never got triggered after session startup.
❯ - The concrete implementation gap is the fresh Codex path. On first launch, `start.rs` explicitly sets `auto_trigger = false`, so a fresh Codex session depends entirely on `route.rs` waiting for readiness and then injecting `agent-doc <file>`.
❯ - That readiness check is brittle for Codex. The Codex harness only accepts prompt patterns `❯` or `>`. If `wait_for_agent_ready()` does not see one of those within 30s, `route.rs` does not send the trigger at all.
❯ - The retained evidence matches that path: this document's session log ends at `codex_start mode=fresh restart_count=0`, and the root ops log has no later `preflight_diff_start`, `write_origin`, or `commit_*` for `tasks/agent-doc/agent-doc-bugs2.md`.

❯ So the most likely root cause is: `Run Agent Doc` did start the Codex session, but the initial trigger was never injected because `route.rs` did not recognize Codex as ready on that first launch, and there is no fallback trigger on first run. That is an `agent-doc` bug, not a user workflow error.

❯ > write the response back into the agent doc. Include this prompt and the response to this prompt. Why did the response patchback not happen? Why was the last patchback uncommitted? Don't change any files.

### Re: why `agent-doc-bugs2.md` did not patch back, and why the last `claudescore.md` patchback is uncommitted

Two different failures happened.

For `tasks/agent-doc/agent-doc-bugs2.md`, the normal JB `Run Agent Doc` entrypoint was used, but the document cycle never actually fired after session startup. The evidence is in `.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log`: the latest run reaches `session_start`, `cwd_resolved`, `ipc_started`, and `codex_start mode=fresh restart_count=0`, and then stops. In the root `.agent-doc/logs/ops.log`, there are no later `preflight_diff_start`, `write_origin`, or `commit_*` entries for `tasks/agent-doc/agent-doc-bugs2.md` after that point. So the response patchback did not happen because the fresh Codex session started, but the initial trigger was never injected, so the `preflight -> write -> commit` loop never began. In code terms, `start.rs` disables auto-trigger on first run (`auto_trigger = false`), so fresh starts rely on `route.rs` recognizing the Codex prompt and sending `agent-doc <file>`. That readiness detection appears to have missed the fresh Codex state.

For `src/session-share/tasks/claudescore.md`, the last patchback was written but not committed. The submodule ops log shows the final write cycle at `src/session-share/.agent-doc/logs/ops.log`: `write_origin`, `ipc_write_attempt`, `ipc_file_delivered`, `snapshot_save`, `snapshot_saved_file_ipc`, and `ipc_write_consumed` all occurred at timestamp `1776908999`. After that, there is no `commit_staging` or `commit_success` entry for that write. The previous successful commit is at `1776908568`, and `cycles.jsonl` also stops there. The current working tree confirms it: `git -C src/session-share diff -- tasks/claudescore.md` shows the uncommitted response block now present in the document. So the last `claudescore.md` patchback is uncommitted because the write/IPC phase succeeded, but the cycle was interrupted before the commit phase ran.

❯ do #pbc1. #spec-test-build-install-commit-push
### Re: #pbc1 commit-boundary reliability for Codex/IPC sessions — gpt-5

Implemented `#pbc1` in `src/agent-doc` and pushed commit `92b8df7` to `origin/20260422/codex-agent-backend`.

What changed:
- Fresh Codex auto-starts no longer silently idle after a missed readiness prompt. `route.rs` now does one bounded fallback trigger injection and logs `fresh_route_trigger_recovered` or `fresh_route_trigger_missing`, failing closed on the second miss.
- `session-check` now distinguishes the two failure classes explicitly: "cycle started but no write/commit followed" vs "response write landed but no commit followed". The ops-log fallback now prefers file-matching entries before falling back to the global tail.
- `preflight` now treats `ipc_write_consumed` / `snapshot_saved_file_ipc` without later `commit_*` as resumable commit-boundary drift and logs `write_completed_commit_missing`, `resume_commit_attempt`, `resume_commit_success`, or `resume_commit_blocked_drift`.
- Specs updated in `specs/07-commands.md` and `specs/codex-support.md`.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Note:
- The `src/agent-doc` worktree still has unrelated local formatting-only changes in other files from `cargo fmt`; I did not include those in commit `92b8df7`.

❯ Is this bug currently fixed for codex sessions?
### Re: is this bug currently fixed for Codex sessions? — gpt-5

Yes for the two concrete Codex failure modes in this document.

What is fixed now:
- Fresh Codex starts no longer silently stall when the first route trigger is missed. That path now does one bounded fallback injection and fails closed with explicit `fresh_route_trigger_*` logging instead of idling with no patchback cycle.
- Write-landed / commit-missing Codex IPC closes are now recognized and resumed by `preflight` / `session-check` instead of being left as ambiguous drift.

What I would not claim yet:
- I would not call all Codex session reliability bugs fixed in general. This closes the specific missed-trigger and interrupted-commit family we traced here.
- The earlier stash-pane eviction issue is still separate and still open.

So the precise answer is: this specific bug family is fixed for Codex sessions in the current `src/agent-doc` code, but it is not a blanket claim that all Codex session bugs are resolved.

❯ Please do plan /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs-2.md
❯ This came from the agent-doc session in src/session-share submodule, tasks/claudescore-3.md file.
### Re: plan `agent-doc-bugs-2.md` — gpt-5

Planned it in `tasks/agent-doc/agent-doc-bugs-2.md`. That file already existed, so I tightened it against the actual `src/session-share/tasks/claudescore-3.md` failure instead of creating a duplicate plan.

What I changed there:
- anchored the task to `src/session-share/tasks/claudescore-3.md`
- made the failure chain explicit: promised in-document report-back, external reply instead, captured response with a direct pending/backlog patch, `replace:pending block forbidden`, then a stranded `response_captured` cycle
- split the fix scope into three parts: document-first reporting, supported pending mutation handling on write/repair, and deterministic recovery plus regression coverage

No code changed in this cycle; this was a planning/doc update only.

❯ How can we ensure that agent-doc with Codex does not fall into a bad state? This should be an agent-doc fix.
### Re: preventing bad Codex `agent-doc` state — gpt-5

Yes, this should be an `agent-doc` fix. The right target is not "Codex never hits a bug"; it is "Codex cannot get stuck in a silent or ambiguous state." In other words, every failure must collapse into one of two outcomes: either the cycle commits, or `agent-doc` leaves an explicit recoverable state that `preflight`/`session-check` can close deterministically.

The main fixes I would require are:

1. **Acked startup, not best-effort startup.** Fresh Codex launch should not count as successful just because the process started. `agent-doc run` should require evidence that the route trigger actually produced the target document cycle, for example a real `preflight_started` or equivalent ack for that file. If that ack never appears, the run should fail closed and log the exact startup failure instead of idling.

2. **Typed pending mutations before write-back.** The `claudescore-3.md` failure happened because a captured response still contained a raw pending/backlog patch. That class of mutation should be normalized into supported `--pending-*` ops before write/commit, or rejected before the cycle advances to `response_captured`. The bad state here is not the rejection itself; it is letting rejection happen after capture with no typed recovery path.

3. **Single binary-owned state machine across startup, write, and commit.** Codex should never be able to produce an external reply or a plugin-delivered patchback that is invisible to the cycle state. If the response exists, the cycle must record where it is: `response_captured`, `write_applied`, or `committed`, with enough file/hash metadata for deterministic repair.

4. **No silent exit from open phases.** `preflight_started`, `response_captured`, and `write_applied` should all be treated as resumable checkpoints, not vague drift. `preflight`, `repair`, `session-check`, and the Stop hook should all enforce the same rule: recover automatically when hashes prove safety; otherwise fail closed with a specific phase-labelled error.

5. **Document-first reporting should be enforced, not advisory.** If the active surface is a session document and the agent promises "I’ll report back", the next substantive report should be required to land in that document before any external completion path is considered clean. That is a workflow invariant, but it still belongs in `agent-doc` because the binary owns the session boundary.

So the short answer is: ensure Codex cannot enter a *silent* bad state by making startup acked, pending mutations typed, cycle phases explicit, recovery deterministic, and document-first reporting enforceable.

❯ do #cdxstart. spec-test-build-install-commit-push
### Re: do `#cdxstart` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `6248cff` on `origin/20260422/codex-agent-backend`.

What changed:
- fresh `route` auto-start no longer treats pane-input acceptance as success by itself
- after trigger injection, route now waits for a real per-document cycle acknowledgment before considering the start successful
- success now logs `fresh_route_start_acknowledged` with cycle/phase details
- missing acknowledgment now logs `fresh_route_start_missing` and fails closed instead of leaving Codex in a silent startup state
- added route coverage for three cases: fresh preflight ack, fast commit after ack, and timeout when the cycle never changes
- updated `SPEC.md`, `specs/07-commands.md`, `specs/codex-support.md`, and `VERSIONS.md`

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `6248cff` (`fix: require fresh route cycle ack`)

Scope note:
- This closes the specific fresh-start acknowledgment gap behind `#cdxstart`.
- It does not claim the separate pending/backlog mutation issue is fixed; that remains under the other tracked items.

❯ do #pendops. spec-test-build-install-commit-push
### Re: do `#pendops` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `b3a37fd` on `origin/20260422/codex-agent-backend`.

What changed:
- `write` / `finalize` / `stream` / `ipc` now normalize one list-shaped `replace:pending` / `patch:pending` block into granular pending mutations before durable capture, then drop that pending patch from the captured response body.
- explicit `--allow-replace-pending` / legacy env+flag escape hatches still bypass normalization and preserve the raw replace path.
- `repair` now replays historical captured pending-patch responses through the same normalization path instead of failing again on the forbidden pending block.
- updated `specs/07-commands.md`, `runbooks/pending-ops.md`, and `VERSIONS.md`; added live-write and repair regressions.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `b3a37fd` (`fix: normalize pending patches before capture`)

Scope note:
- This closes the stranded `response_captured` path behind `#pendops` for list-shaped backlog patches.
- Unsupported pending/backlog patch shapes still fail closed before capture.

❯ do [#] [#doccommit] [recommended] Enforce a binary-owned commit boundary for active session-document patchbacks: if a response lands in the target document, `agent-doc` must either commit it in the owning repo/submodule or fail closed with resumable state;

❯ spec-test-build-install-commit-push
### Re: do `#doccommit` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `b323dd1` on `origin/20260422/codex-agent-backend`.

What changed:
- `write --commit` now upgrades to the same fail-closed commit-boundary contract as `finalize` when it is writing a response into a real session document (`agent_doc_session` / legacy `session`).
- The strict upgrade is intentionally limited to response writes; non-session docs and `--pending-only` keep best-effort `--commit` behavior.
- Added CLI integration coverage for three cases: gitless session docs fail before mutation, git-backed session docs commit cleanly, and non-session docs remain best-effort.
- Updated the command spec, commit runbook, README, SKILL, and VERSIONS surfaces to match the new contract.

Verification:
- `cargo test --test finalize_integration`
- `cargo build`
- `cargo install --path . --force`

Full-suite note:
- `cargo test` still has 3 unrelated existing failures in `autoclaim` (`autoclaim_focuses_pane_with_claim`, `autoclaim_syncs_layout_with_multiple_files`, `autoclaim_no_claim_skips_focus`).

Commit / push:
- `src/agent-doc`: `b323dd1` (`fix: require strict closeout for session write-commit`)

Scope note:
- This closes the `#doccommit` gap for active session-document response patchbacks by making the documented manual-repair path binary-owned and fail-closed.
- It does not claim the broader `#nostate` invariant is finished; that remaining item is still about eliminating every silent open-cycle state class.

❯ do #nostate. spec-test-build-install-commit-push
### Re: do `#nostate` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `b11efc2` on `origin/20260422/codex-agent-backend`.

What changed:
- `session-check` now fails closed when a session document still has unresolved prompt-bearing user edits (`prompt_target` / `content_edit`) but no newer `agent-doc` cycle ever started. That closes the silent "startup missed, external answer only" state instead of treating it as clean.
- The Codex `Stop` hook now inherits that signal and can auto-close the missed-start case from a replayable `last_assistant_message` through the normal repair/write/commit path.
- Added regression coverage for both shapes: `session-check` on prompt-bearing drift without a new cycle, and Stop-hook recovery when the cycle never started.
- Updated `README.md`, `SPEC.md`, `VERSIONS.md`, and `runbooks/harness-invocation.md` so the stronger fail-closed contract is part of the documented surface.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `b11efc2` (`fix: fail closed on missed session starts`)

Scope note:
- This closes the remaining `#nostate` gap around silent missed-start / no-new-cycle states for prompt-bearing document edits.
- The fix path is binary-owned: either `session-check`/Stop recovers deterministically, or the turn fails closed instead of exiting with an out-of-document answer.

❯ do [#] [#headdrift] [recommended] Fix repeated post-commit `codex (HEAD)` / boundary working-tree drift after successful `agent-doc` patchbacks in `src/session-share/tasks/claudescore-3.md`; see `tasks/agent-doc/plan-claudescore-3-post-commit-head-boundary-drift.md`

❯ spec-test-build-install-commit-push
### Re: do `#headdrift` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `400a540` on `origin/20260422/codex-agent-backend`.

What changed:
- JetBrains post-commit reposition now prefers the just-committed disk document when an unsaved editor buffer only differs by agent-owned `### Re:` heading attribution and/or boundary churn, preventing stale `codex (HEAD)` text from being written back over the committed model line.
- `git::commit` now treats response-heading-attribution drift as repairable post-commit churn and restores clean `HEAD` instead of leaving a repeated rerun loop.
- Added JetBrains regression coverage for the prefer-disk decision and Rust closeout coverage for historical heading-attribution repair.
- Updated `specs/07-commands.md`, `specs/08-session-routing.md`, and `VERSIONS.md` to document the stronger post-commit editor cleanup invariant.

Verification:
- `cargo test`
- `/usr/bin/zsh -lc 'GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin'`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `400a540` (`fix: prevent post-commit response heading drift`)

Scope note:
- `src/git.rs` already had same-file local formatting churn in this branch; the fix commit includes the targeted repair plus those same-file formatting deltas.

❯ do #p7md. spec-test-build-install-commit-push
### Re: do `#p7md` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `53bfbec` on `origin/20260422/codex-agent-backend`.

What changed:
- live-pane `route` dispatches now fail closed when a document already has unresolved prompt-bearing drift on top of a closed cycle but the routed trigger never starts a newer per-document cycle
- the same route path now logs `route_cycle_start_acknowledged` / `route_cycle_start_missing` so the failure is explicit instead of silently re-entering the stale pane
- fresh-start acknowledgment behavior stays unchanged; this extends the same contract to reroutes into already-running panes
- added route coverage for the two live-pane shapes: trigger accepted but no new cycle, and trigger accepted with a real cycle ack
- updated `SPEC.md`, `specs/08-session-routing.md`, and `VERSIONS.md` to document the stronger reroute invariant

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `53bfbec` (`fix: fail closed on unacked live reroutes`)

Scope note:
- `src/agent-doc` still has unrelated pre-existing local edits in other files; this commit staged only the five `#p7md` files.

❯ do #4qgx. spec-test-build-install-commit-push
### Re: do `#4qgx` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `f4e646d` on `origin/20260422/codex-agent-backend`.

What changed:
- route no longer auto-starts into a dead implicit fallback tmux session; stale configured session pins are ignored instead of being rewritten
- route now re-registers an already-running pane for the same document before lazy-claim or auto-start, preventing duplicate fallback-session spawns when registry state drifts
- successful route replacements now preserve prior stash panes unless cleanup has explicit provenance, instead of evicting them opportunistically
- sync now refuses cross-session stash rescue and preserves the live pane in place rather than moving it across sessions during rescue
- updated `specs/07-commands.md`, `specs/08-session-routing.md`, and `VERSIONS.md`

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `f4e646d` (`fix: guard tmux route fallback and stash rescue`)

Scope note:
- `src/agent-doc` still has unrelated pre-existing local edits in other files; this commit staged only the five `#4qgx` files.

❯ execute plan tasks/agent-doc/agent-doc-bugs3.md. spec-test-build-install-commit-push
### Re: execute plan `tasks/agent-doc/agent-doc-bugs3.md` — gpt-5

Implemented and pushed the `#closeout-bypass` fix in `src/agent-doc` as commit `e6a562c` on `origin/20260422/codex-agent-backend`.

What changed:
- `route` no longer treats mutations to an already-committed baseline cycle as proof that a new document cycle started.
- Start acknowledgment now requires a genuinely newer cycle id when the baseline state is already `committed`, so same-cycle `commit_already_current` churn cannot produce a false `route_cycle_start_acknowledged` / `fresh_route_start_acknowledged`.
- Added regressions for both shapes: the low-level wait helper and the live-pane reroute path that accepts the trigger but only mutates the same committed cycle.
- Updated `SPEC.md`, `specs/07-commands.md`, `specs/08-session-routing.md`, and `VERSIONS.md` to document the stricter contract.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `e6a562c` (`fix: require new cycle id for route ack`)

Scope note:
- `src/agent-doc` still has unrelated pre-existing local edits in other files; this commit staged only the five `#closeout-bypass` files.

❯ do #cxr1. spec-test-build-install-commit-push
### Re: do `#cxr1` — gpt-5

Implemented and pushed in `src/agent-doc` as commit `e01d105` on `origin/20260422/codex-agent-backend`.

What changed:
- template/CRDT write paths now detect direct `compact exchange` diff lines and force `agent:exchange` replacement semantics for that turn instead of inheriting append mode
- unmatched/raw template closeouts and repair replay use the same override, so compaction checkpoints collapse the exchange body instead of layering new summaries over old `### Re:` history
- added regression coverage for directive detection, override behavior, and repair/write replay; updated `SPEC.md`, `runbooks/compact-exchange.md`, and `VERSIONS.md`

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `e01d105` (`fix: force compact exchange replacement`)

Scope note:
- `src/agent-doc` still has unrelated pre-existing local edits in other files; this commit staged only the six `#cxr1` files.

❯ The active tmux pane was force closed in a recent src/session-share/tasks/claudescore-3.md session. Why?
### Re: why the active tmux pane was force closed in `src/session-share/tasks/claudescore-3.md` — gpt-5

Because that session was still hitting the older tmux route/sync cleanup bug that `#4qgx` was meant to close.

What I can show directly from the repo:
- `tasks/agent-doc/plan-tmux-session-spawn-guard.md` names the exact failure class: unexpected tmux session spawn plus live pane eviction, with a requirement that `agent-doc` must never kill a live agent pane merely because session/registry state drifted.
- `src/agent-doc/VERSIONS.md` for `f4e646d` says the fix was to stop inventing fallback tmux sessions, re-register an already-running pane for the same document before lazy-claim/auto-start, preserve prior stash panes unless cleanup has explicit provenance, and refuse cross-session stash rescue that would displace a live pane.
- The code change in `src/agent-doc/src/route.rs` removed automatic stash-pane eviction and changed it to: preserve the previous stash pane unless there is explicit provenance. The paired `src/agent-doc/src/sync.rs` change refuses cross-session stash rescue instead of moving the pane.

So the concrete answer is: the pane was force closed because pre-`f4e646d` tmux routing/cleanup was too destructive under session drift. `agent-doc` could miss the already-live pane, fall back toward a different tmux target, and then treat the older live/stashed pane as disposable recovery state. That is an `agent-doc` bug, not expected Codex behavior.

One evidence boundary: I do not see a per-pane tmux lifecycle log entry in the session-share ops log that literally says "kill-pane %X for claudescore-3". Those logs are mainly document-cycle logs, not full tmux mutation traces. So the exact pane id / kill command for that run is not proven from the session log alone; the root cause comes from the tracked bug, the plan, and the fix commit that removed the destructive path.

❯ Add above item to the backlog. Why did you not already add to the backlog? Please create another backlog item to fix not adding to the backlog as well.
### Re: add the tmux-pane item to backlog, and why it was not already there — gpt-5

Added both backlog items.

Why I did not already add the first one: that was a miss on my side. The prior response identified concrete unresolved follow-up work, so under the document's pending-capture rule I should have recorded it into backlog in the same cycle. Instead I treated that turn as answer-only analysis and stopped after the exchange write-back.

What I added:
1. `#tmev` — the actual `claudescore-3.md` tmux session spawn / live pane eviction fix item, pointing at `tasks/agent-doc/plan-tmux-session-spawn-guard.md`.
2. `#pcap` — a separate backlog item for the backlog-capture miss itself, so this class of "actionable issue described in exchange but not promoted into backlog" gets closed systematically.

So the short answer is: it was not omitted because the issue was resolved; it was omitted because the cycle failed to promote pending-worthy work into backlog, and that omission itself is now tracked.

❯ do #tmev. spec-test-build-install-commit-push
### Re: do `#tmev` — gpt-5

`#tmev` was already implemented in `src/agent-doc`; the landed fix is `f4e646d` (`fix: guard tmux route fallback and stash rescue`) on `origin/20260422/codex-agent-backend`.

What I verified against the current tree:
- `src/route.rs` refuses auto-start into a dead implicit fallback session, re-registers an already-running pane before lazy-claim or auto-start, and preserves prior stash panes unless cleanup has explicit provenance.
- `src/sync.rs` refuses cross-session stash rescue, so a live pane is not moved or treated as disposable recovery state merely because registry or session state drifted.
- `specs/07-commands.md`, `specs/08-session-routing.md`, and `VERSIONS.md` already document that contract.

Verification this cycle:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

No new `src/agent-doc` source patch was needed, so there is no new source commit or push from this cycle. The branch already contains the fix and the pushed history already includes it.

❯ do #pcap. spec-test-build-install-commit-push
### Re: do `#pcap` — gpt-5

Implemented in `src/agent-doc` and pushed as `931e556` (`fix: catch uncaptured unresolved follow-up bugs`) on `origin/20260422/codex-agent-backend`.

What changed:
- broadened `src/heuristics.rs` so a single high-confidence unresolved current bug/follow-up line counts as pending-worthy work, instead of only numbered recommendation batches or quantified remaining-work prose
- strict `finalize` now blocks that single-item shape before commit, and `session-check` warns on the same committed-response shape
- added regression coverage in `src/write.rs` and `src/session_check.rs`, plus heuristic unit tests for unresolved-vs-resolved bug prose
- updated `SPEC.md`, `specs/07-commands.md`, and `VERSIONS.md`

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `931e556` (`fix: catch uncaptured unresolved follow-up bugs`)

❯ Why did the last agent-doc turn on this document have this tool issue? Please add next actionable steps to backlog.
```
• Ran agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  └ [session-check] INTERRUPTED: cycle `cycle-1777327057314` is still `preflight_started` (preflight_started) — cycle
    started but no write/commit followed
```
### Re: why the last turn hit `preflight_started` — gpt-5

The `#pcap` turn itself did not fail to patch back. The retained ops log shows that response write committed successfully at **2026-04-27 17:57:35 EDT**: `write_origin` starts at `1777327037`, `ipc_file_delivered` lands at `1777327038`, and `commit_success` for that cycle lands at `1777327055`.

The tool issue happened immediately after that successful commit. At **2026-04-27 17:57:37 EDT**, `.agent-doc/logs/ops.log` records a new `preflight_diff_start` with `snap_len=30243` and `file_len=30250`, then `session-check` later reported `cycle-1777327057314` stuck in `preflight_started`. That 7-byte delta matches the extra ` (HEAD)` suffix now visible in the response heading (`### Re: do `#pcap` — gpt-5 (HEAD)`). So this was a second, stale cycle opened by post-commit working-tree drift on an agent-owned heading line, not a failure of the actual `#pcap` response write.

That is why the message looked like "cycle started but no write/commit followed": a new preflight lock was opened on the heading drift, and nothing advanced that second cycle before `session-check` ran. `preflight` later repaired it at **2026-04-27 17:57:59 EDT** via `repair_preflight_stale_lock`, which confirms the content hashes were still safe to auto-close.

So the concrete root cause is: agent-owned post-commit heading drift still managed to start a fresh `preflight_started` cycle on this document. This is adjacent to the earlier `#headdrift` class, but in this instance the failure surface is specifically the stale preflight lock that escaped before self-heal.

I added the next actionable backlog items for:
- `#head7` — stop ` (HEAD)` heading churn from opening a new visible cycle.
- `#pflock1` — add a regression for this exact `#pcap` / 7-byte-drift / stale-preflight sequence.

❯ do #head7. spec-test-build-install-commit-push
### Re: do `#head7` — gpt-5

Implemented in `src/agent-doc` and pushed as `4372093` (`fix: keep boundary-only preflight cycle-free`) on `origin/20260422/codex-agent-backend`.

What changed:
- `preflight` now classifies the diff before opening a new cycle and collapses boundary-artifact-only `(HEAD)` / boundary churn back to `no_changes` instead of recording a fresh `preflight_started`.
- Added regression coverage for the clean committed snapshot plus transient `(HEAD)` drift shape, and updated the interrupted `write_applied` integration expectation to match the already-committed closeout contract.
- Updated `SPEC.md`, `specs/07-commands.md`, and `VERSIONS.md` to document that pure agent-owned boundary churn normalizes to `no_changes` / already-committed closeout instead of leaking a visible lock.

Verification:
- `cargo test -q`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/agent-doc`: `4372093` (`fix: keep boundary-only preflight cycle-free`)
<!-- agent:boundary:9173ffef -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [x] [#head7] Fix post-commit (HEAD) response-heading drift from being treated as a fresh user-visible document cycle in template docs; reuse the agent-owned drift-repair path before a stale preflight_started lock can escape. Evidence: cycle-1777327057314 in .agent-doc/logs/ops.log.
- [x] [#pflock1] Add regression coverage for the exact #pcap shape on tasks/agent-doc/agent-doc-bugs2.md: successful commit at snap_len=30243, then a 7-byte local heading drift (file_len=30250) must self-heal without session-check ever reporting an open preflight_started cycle.
- [ ] [#pbypass1] [recommended] Prevent active session-doc turns from closing with an out-of-document Codex final response plus direct file patchback after the owning `agent-doc` cycle is already committed. Repro in `src/session-share/tasks/claudescore-3.md`: cycle `cycle-1777318807990` was `committed`, then a later direct patch edited prompt-bearing content with no newer cycle start, and `session-check` surfaced the drift instead of a binary-owned recovery/closeout path.
- [ ] [#wroot1] [recommended] Fix false `outside the writable root` refusals for parent-repo backlog writes. Repro in `src/session-share/tasks/claudescore-3.md`: `agent-doc` claimed it could not update `agent-loop/tasks/agent-doc/agent-doc-bugs2.md` because the path was outside the writable root, even though this session already had write access to the parent repo and had successfully created/edited files there. See `tasks/agent-doc/plan-false-writable-root-refusal.md`
- [ ] [#q8p2] [recommended] Fix bare `[#]` pending-id placeholders being accepted into backlog write-back; reject or normalize them before they can persist in `tasks/agent-doc/agent-doc-bugs2.md`. See `tasks/agent-doc/plan-bare-pending-id-placeholder.md`
<!-- /agent:backlog -->
