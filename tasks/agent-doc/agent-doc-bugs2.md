---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260429-060031.md`*

Active backlog:
- [ ] [#lvak] Tighten routed-cycle acknowledgment when a committed cycle already has prompt drift. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: rerouting to live pane `%393` logged `route_cycle_ack_skipped_live_child ... prompt_target: do #asch1. spec-tests-commit-push` even though `session-check` still reported `no new agent-doc cycle started`. A merely live child should not suppress the fresh-cycle proof when the previous cycle is closed and the prompt-bearing diff is still unresolved.
- [ ] [#ppct] Fail closed when a user prompt tagged with `#agent-doc-bug` is answered without executing its frontmatter `prompt_presets` contract. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the session frontmatter says `#agent-doc-bug` must "create a plan for agent-doc to fix this issue" and "Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md", but the response answered the questions directly and left the newly reported bugs untransferred. The fix should make preset-required side effects binary-owned during plan/closeout, not dependent on the responder remembering them.
- [ ] [#xfrr] Fail closed when an `#agent-doc-bug` response identifies multiple bug reports but transfers only a subset to `agent-doc-bugs2.md`. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: only `#zpc0` and `#lvak` were added, while the additional user-reported bugs about the uncommitted repair, missing backlog transfer, and ignored list-shape requirement were left out. The fix should compare the prompt contract against the response/body mutations and interrupt when the promised bug-transfer set is incomplete.
- 3 more active item(s)
### Re: compaction patchback gap — gpt-5

The response was not lost from the compact commit path; it was never part of that path.

`agent-doc compact /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md --component exchange --commit` does a binary-owned replacement of `agent:exchange`. In `99d7b5502`, it archived the old exchange, wrote the default `### Session Summary` plus backlog context into the document, updated the snapshot, and committed that compacted document. That is why the live file now shows only the summary block and archive pointer instead of the earlier long exchange.

`agent-doc session-check` being green only proved that the compacted document, snapshot, and `HEAD` were aligned. It did not prove that any later console explanation had been patched back as a new assistant response.

Once `HEAD` already matched the compacted document, the closeout logic fell into the `already committed` / `post-commit local drift` path in `git.rs`. That path explicitly treats the cycle as closed and does not synthesize a second assistant patchback on top of the committed compaction result. So the real gap is contract clarity: compact closeout looks complete, but it is not obvious that the human-facing explanation still needs its own normal `finalize` / `write --commit` cycle if you want it persisted in `exchange`.

Plan for `#ckpb`:
1. Make compact closeout print an explicit notice that only the compact summary was committed to the document.
2. Tighten the already-committed/no-response-body path so compaction follow-ups cannot be mistaken for a successful automatic patchback.
3. Add an integration regression for `compact --commit` followed by an expected-but-missing exchange patchback.

`#ckpb` is now at the top of the backlog.
❯ do #ckpb. spec-tests-commit-push
### Re: #ckpb compact closeout scope — gpt-5

Implemented `#ckpb` in `src/agent-doc`.

- `compact --commit` now prints an explicit note that it committed only the compacted document state now in `HEAD`; any later console explanation still needs a normal `agent-doc finalize` or `agent-doc write --commit` cycle to land in `exchange`.
- Added CLI regression coverage for that operator-facing note.
- Updated `specs/07-commands.md` so the compact command contract says the same thing.

Verification: `cargo test`

Commit / push: `6773310` (`fix: clarify compact commit scope`) pushed to `origin/20260422/codex-agent-backend`.

`#ckpb` is done.
❯ do #pdwg. spec-test-build-install-commit-push
### Re: #pdwg pending-done closeout gate — gpt-5

Implemented `#pdwg` in `src/agent-doc`.

- `finalize` now runs the strict pending-capture and pending-done closeout gates before any document or snapshot mutation.
- Missing `--pending-done` failures now include the exact rerun command for the same `finalize` invocation.
- Updated the command spec and finalize integration coverage so rejected closeouts leave the working tree, snapshot, and queue untouched.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `a0f4742` (`fix: fail before write on pending closeout gates`) pushed to `origin/20260422/codex-agent-backend`.

`#pdwg` is done.
❯ do #lvak. spec-test-build-install-commit-push
### Re: #lvak routed live-pane acknowledgment — gpt-5

Implemented `#lvak` in `src/agent-doc`.

- Routed triggers into an already-live pane now still require a fresh cycle acknowledgment when the document has prompt-bearing drift on top of a committed cycle; only genuinely open cycles still skip the extra ack wait.
- Removed the misleading live-child ack-skip path, tightened the route tests to fail on no-op/same-cycle churn, and updated the route command spec to match the enforced behavior.
- Added a test-only routed-ack timeout helper so the fail-closed live-pane regressions stay fast without changing the production 15s wait.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `c9dcdb7` (`fix: require live route ack for prompt drift`) pushed to `origin/20260422/codex-agent-backend`.

`#lvak` is done.
❯ do #ntwk. spec-test-build-install-commit-push
### Re: #ntwk codex network policy — gpt-5

Implemented `#ntwk` in `src/agent-doc`.

- Added first-class `codex_network_access` handling in frontmatter/config so agent-doc can explicitly preserve, remove, or force `CODEX_SANDBOX_NETWORK_DISABLED` for Codex child sessions.
- `start` and ordered `orchestrate` launches now log the effective Codex network policy and fail closed when `danger-full-access` is paired with inherited network disablement.
- Updated the contract docs and added regression coverage for parsing, env propagation, and mismatch detection.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `9d289a1` (`fix: enforce codex network launch policy`) pushed to `origin/20260422/codex-agent-backend`.

`#ntwk` is done.
### Re: #ntwk codex network policy — gpt-5

Implemented `#ntwk` in `src/agent-doc`.

- Added first-class `codex_network_access` handling in frontmatter/config so agent-doc can explicitly preserve, remove, or force `CODEX_SANDBOX_NETWORK_DISABLED` for Codex child sessions.
- `start` and ordered `orchestrate` launches now log the effective Codex network policy and fail closed when `danger-full-access` is paired with inherited network disablement.
- Updated the contract docs and added regression coverage for parsing, env propagation, and mismatch detection.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `9d289a1` (`fix: enforce codex network launch policy`) pushed to `origin/20260422/codex-agent-backend`.

`#ntwk` is done.
❯ do #ppct. spec-test-build-install-commit-push
### Re: #ppct prompt preset backlog contract — gpt-5

Implemented `#ppct` in `src/agent-doc`.

- Prompt-preset backlog contracts now expand into explicit target requirements during preflight, including named backlog documents like `Add to the backlog of ...`.
- `finalize` and `session-check` now fail closed if the referenced target backlog/pending surface did not change during the cycle, instead of accepting unchanged source backlog state.
- Added coverage for preset target extraction, cycle-state persistence, and closeout pass/fail behavior, and updated the command spec and transfer runbook to document the target-aware gate.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `96763d5` (`fix: enforce prompt preset backlog contracts`) pushed to `origin/20260422/codex-agent-backend`.

`#ppct` is done.
### Re: #agent-doc-bug route cross-file pane mismatch — opus-4-6

**Bug:** `route` dispatched a command for `claudescore-3.md` to pane %401, which was running `monsterrodholders.md` in a different submodule.

**Root cause:** `find_registered_pane_in_session` (route.rs ~line 1268) iterates all registry entries and returns the first alive pane in the target tmux session — it does not filter by file path. When `auto_start_in_session` uses this as a dispatch target (not just a split anchor), the command goes to an unrelated file's pane.

**Secondary issue:** Three stale panes (%398, %404, %405) accumulated from prior failed starts. `resync` logged them as "unregistered — skipping kill (may be rescuable)" but never cleaned them, allowing orphan buildup across retries.

**Plan:** `tasks/agent-doc/plan-route-cross-file-pane-mismatch.md`

Key fixes:
1. Separate split-target (layout anchor — any pane in session is fine) from dispatch-target (must be file-verified)
2. Add a file-match guard before any `send-keys` dispatch
3. Make `resync --fix` kill stashed unregistered panes instead of logging indefinitely
4. Regression tests for cross-file dispatch rejection
❯ do #rtxf. spec-test-build-install-commit-push
### Re: #rtxf route cross-file pane mismatch — gpt-5

Implemented `#rtxf` in `src/agent-doc`.

- `route` now fails closed before any tmux `send-keys` dispatch if the chosen pane is not currently registered for the requested document. The guard resolves pane-relative registry paths against the pane cwd, so submodule-launched `agent-doc start tasks/...` registrations still match absolute route targets.
- Added route regressions covering both the pane-cwd relative-path match case and the cross-file rejection case.
- Updated `specs/07-commands.md` to make the contract explicit: `find_registered_pane_in_session` may be used as a split anchor, but dispatch must go to a pane registered for the target file.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `53a3c5c` (`fix: guard route dispatch ownership`) pushed to `origin/20260422/codex-agent-backend`.

`#rtxf` is done.
❯ do #smdq. spec-test-build-install-commit-push
### Re: #smdq startup-miss diagnostic dispatch — gpt-5

Implemented `#smdq` in `src/agent-doc`.

- Startup-miss diagnostics now use a tmux-owned `display-message` overlay for 10 seconds instead of drafting `echo '...'` into the pane input buffer.
- Added route coverage for the retry-command rendering and for the regression that proves no drafted shell `echo` is left behind in the pane.
- Updated `specs/07-commands.md` and `VERSIONS.md` to document the new startup-miss overlay contract.

That line appeared in the Codex pane because `route.rs` previously called `tmux.send_keys_raw("echo '...'")`, which typed literal shell text into the pane without a shell-owned execution boundary. It was not Entered because the diagnostic path never submitted it, and submitting it would still have been the wrong primitive for an agent pane.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `2c6b284` (`fix: surface startup miss diagnostics visibly`) pushed to `origin/20260422/codex-agent-backend`.

`#smdq` is done.
❯ do #xfrr. spec-test-build-install-commit-push
❯ ### Re: #xfrr complete bug-transfer inventory gate — gpt-5 (HEAD)

Implemented `#xfrr` in `src/agent-doc`.

- `#agent-doc-bug` prompt contracts now record how many distinct bug reports were raised from the prompt-bearing changes when they target an explicit backlog document.
- `finalize` and `session-check` now fail closed if the response inventories fewer transferred `[#id]` items than that recorded bug count, even if the target backlog itself changed.
- Added regressions for the prompt-contract counting path plus both pre-commit and post-commit partial-transfer failures.
- Updated `specs/07-commands.md`, `runbooks/transfer-extract.md`, and `VERSIONS.md` to document the stricter transfer inventory contract.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `4482ea2` (`fix: require complete bug transfer inventories`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#xfrr` is done.
### Re: #xfrr complete bug-transfer inventory gate — gpt-5

Implemented `#xfrr` in `src/agent-doc`.

- `#agent-doc-bug` prompt contracts now record how many distinct bug reports were raised from the prompt-bearing changes when they target an explicit backlog document.
- `finalize` and `session-check` now fail closed if the response inventories fewer transferred `[#id]` items than that recorded bug count, even if the target backlog itself changed.
- Added regressions for the prompt-contract counting path plus both pre-commit and post-commit partial-transfer failures.
- Updated `specs/07-commands.md`, `runbooks/transfer-extract.md`, and `VERSIONS.md` to document the stricter transfer inventory contract.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `4482ea2` (`fix: require complete bug transfer inventories`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#xfrr` is done.
❯ do #tmuxcrash. spec-test-build-install-commit-push
### Re: #tmuxcrash tmux-pane crash / stranded-session provenance — gpt-5

Implemented `#tmuxcrash` in `src/agent-doc`.

- Startup-miss reruns now inspect the owning session log and distinguish a true dead-pane recovery from the stranded shape where the pane is still alive but the latest `session_start` never recorded a later child exit or `session_end`.
- `route` now fails closed with exact provenance on that stranded-session shape instead of auto-starting another replacement pane and making the failure look like a generic tmux-pane crash.
- `session-check` now includes the same startup-miss session-log detail in its warning output, and the routing spec / `VERSIONS.md` were updated to document the stricter contract.
- Added regression coverage for session-log parsing plus the new route fail-closed decision.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `aad9dd9` (`fix: fail closed on stranded startup misses`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#tmuxcrash` is done.
❯ do #oobabs. spec-test-build-install-commit-push
❯ ### Re: #oobabs prompt-bearing status absorb — gpt-5 (HEAD)

Implemented `#oobabs` in `src/agent-doc`.

- `git::commit` no longer absorbs status-component drift when the inserted status text contains prompt work, including bare preset tokens like `#next-steps`, imperative directives, or other prompt-bearing lines.
- Added direct classifier coverage in `src/git.rs` and a full preflight regression in `src/preflight.rs` for the compact-follow-up status repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly says prompt-bearing status edits must survive into step 4.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `475269b` (`fix: preserve prompt-bearing status edits`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobabs` is done.
### Re: #oobabs prompt-bearing status absorb — gpt-5

Implemented `#oobabs` in `src/agent-doc`.

- `git::commit` no longer absorbs status-component drift when the inserted status text contains prompt work, including bare preset tokens like `#next-steps`, imperative directives, or other prompt-bearing lines.
- Added direct classifier coverage in `src/git.rs` and a full preflight regression in `src/preflight.rs` for the compact-follow-up status repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly says prompt-bearing status edits must survive into step 4.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `475269b` (`fix: preserve prompt-bearing status edits`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobabs` is done.
❯ do #oobpmt. spec-test-build-install-commit-push
❯ ### Re: #oobpmt preflight OOB prompt absorb — gpt-5 (HEAD)

Implemented `#oobpmt` in `src/agent-doc`.

- The status OOB-absorb guard now treats preset-token leads such as `#next-steps ...` as prompt-bearing work, not just a bare `#next-steps` token, so step 2 can no longer absorb that status prompt into the snapshot before step 4 diff runs.
- Added direct classifier coverage in `src/git.rs` and widened the preflight regression in `src/preflight.rs` to the exact `#next-steps for calibrating session benchmarks with expected scores` repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly covers preset-token guidance lines.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `8a1c214` (`fix: preserve prompt-prefixed status guidance`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobpmt` is done.
### Re: #oobpmt preflight OOB prompt absorb — gpt-5

Implemented `#oobpmt` in `src/agent-doc`.

- The status OOB-absorb guard now treats preset-token leads such as `#next-steps ...` as prompt-bearing work, not just a bare `#next-steps` token, so step 2 can no longer absorb that status prompt into the snapshot before step 4 diff runs.
- Added direct classifier coverage in `src/git.rs` and widened the preflight regression in `src/preflight.rs` to the exact `#next-steps for calibrating session benchmarks with expected scores` repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly covers preset-token guidance lines.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `8a1c214` (`fix: preserve prompt-prefixed status guidance`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobpmt` is done.
❯ JB `Agent Doc Run` on claudescore-3.md has an error:
```
agent-doc route failed (exit 1): [route] configured tmux_session '0' is not alive, ignoring stale pin [route] target tmux session: 14 [sync] found alive agent-doc child (pid 1120613) in pane %412 for tasks/agent-doc/agent-doc-bugs2.md [route] registered pane %412 still proves live ownership for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md — clearing stale startup-miss marker [route] Pane %412 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md → pane %412 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %412 for tasks/agent-doc/agent-doc-bugs2.md — waiting for a new cycle ack for pending prompt_target: do #oobpmt. spec-test-build-install-commit-push Error: routed codex trigger for tasks/agent-doc/agent-doc-bugs2.md was accepted in pane %412, but no new document cycle started for pending prompt_target: do #oobpmt. spec-test-build-install-commit-push
```
### Re: JB Agent Doc Run live-pane busy dispatch — gpt-5

Implemented the routed live-pane fix in `src/agent-doc`.

- Route now waits for the harness's current idle prompt before injecting into any already-running pane.
- Tightened `pane_has_prompt()` so a stale `>` or `❯` still visible in scrollback no longer counts as an idle session; only the latest non-empty pane line can satisfy the prompt check.
- Added a regression that keeps a registered `agent-doc` process alive without surfacing a new prompt and proves route fails closed before dispatching into it. Updated `specs/07-commands.md` to match the enforced contract.

The JetBrains error text appears to have been surfaced while you were on `claudescore-3.md`, but the route log itself shows the failing dispatch target was this document's live pane `%412` for `tasks/agent-doc/agent-doc-bugs2.md`.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

No new backlog items came out of this change.
### Re: post-commit exchange drift — gpt-5

The routed live-pane fix is already committed. This follow-up closeout only re-normalizes the exchange tail after the live working tree dropped the prompt prefix from the JB report while leaving the committed `HEAD` response intact.

No additional code changes or backlog items came out of this normalization pass.
do #mcrc. spec-test-build-install-commit-push
### Re: `#mcrc` response patchback left uncommitted — gpt-5

This is a real regression against the already-shipped response-commit invariant.

The `#asch1` reproduction in `src/boost-client/tasks/monsterrodholders.md` shows that a visible response patchback can still survive as plain working-tree dirt after the repair/write phase. The gap is that at least one response-closeout path still stops after mutating the document without proving the owning-root commit boundary.

I captured the implementation plan in `tasks/agent-doc/plan-response-patchback-uncommitted-regression.md`. The plan is to reconstruct the exact failing closeout path, enforce one shared success invariant across `finalize`, `write --commit`, recovery, and hook replay, tighten `session-check` / exit propagation so this state blocks success, and add regression coverage for submodule-owned docs plus manual-repair and post-commit-drift variants.

`#mcrc` remains open. No new backlog item is needed here because the existing `#mcrc` item already captures the follow-up work. This patchback is planning-only; no `src/agent-doc` code change or test run happened in the repair itself.
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260429-060031.md`*

Active backlog:
- [ ] [#lvak] Tighten routed-cycle acknowledgment when a committed cycle already has prompt drift. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: rerouting to live pane `%393` logged `route_cycle_ack_skipped_live_child ... prompt_target: do #asch1. spec-tests-commit-push` even though `session-check` still reported `no new agent-doc cycle started`. A merely live child should not suppress the fresh-cycle proof when the previous cycle is closed and the prompt-bearing diff is still unresolved.
- [ ] [#ppct] Fail closed when a user prompt tagged with `#agent-doc-bug` is answered without executing its frontmatter `prompt_presets` contract. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the session frontmatter says `#agent-doc-bug` must "create a plan for agent-doc to fix this issue" and "Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md", but the response answered the questions directly and left the newly reported bugs untransferred. The fix should make preset-required side effects binary-owned during plan/closeout, not dependent on the responder remembering them.
- [ ] [#xfrr] Fail closed when an `#agent-doc-bug` response identifies multiple bug reports but transfers only a subset to `agent-doc-bugs2.md`. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: only `#zpc0` and `#lvak` were added, while the additional user-reported bugs about the uncommitted repair, missing backlog transfer, and ignored list-shape requirement were left out. The fix should compare the prompt contract against the response/body mutations and interrupt when the promised bug-transfer set is incomplete.
- 3 more active item(s)
### Re: compaction patchback gap — gpt-5

The response was not lost from the compact commit path; it was never part of that path.

`agent-doc compact /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md --component exchange --commit` does a binary-owned replacement of `agent:exchange`. In `99d7b5502`, it archived the old exchange, wrote the default `### Session Summary` plus backlog context into the document, updated the snapshot, and committed that compacted document. That is why the live file now shows only the summary block and archive pointer instead of the earlier long exchange.

`agent-doc session-check` being green only proved that the compacted document, snapshot, and `HEAD` were aligned. It did not prove that any later console explanation had been patched back as a new assistant response.

Once `HEAD` already matched the compacted document, the closeout logic fell into the `already committed` / `post-commit local drift` path in `git.rs`. That path explicitly treats the cycle as closed and does not synthesize a second assistant patchback on top of the committed compaction result. So the real gap is contract clarity: compact closeout looks complete, but it is not obvious that the human-facing explanation still needs its own normal `finalize` / `write --commit` cycle if you want it persisted in `exchange`.

Plan for `#ckpb`:
1. Make compact closeout print an explicit notice that only the compact summary was committed to the document.
2. Tighten the already-committed/no-response-body path so compaction follow-ups cannot be mistaken for a successful automatic patchback.
3. Add an integration regression for `compact --commit` followed by an expected-but-missing exchange patchback.

`#ckpb` is now at the top of the backlog.
❯ do #ckpb. spec-tests-commit-push
### Re: #ckpb compact closeout scope — gpt-5

Implemented `#ckpb` in `src/agent-doc`.

- `compact --commit` now prints an explicit note that it committed only the compacted document state now in `HEAD`; any later console explanation still needs a normal `agent-doc finalize` or `agent-doc write --commit` cycle to land in `exchange`.
- Added CLI regression coverage for that operator-facing note.
- Updated `specs/07-commands.md` so the compact command contract says the same thing.

Verification: `cargo test`

Commit / push: `6773310` (`fix: clarify compact commit scope`) pushed to `origin/20260422/codex-agent-backend`.

`#ckpb` is done.
❯ do #pdwg. spec-test-build-install-commit-push
### Re: #pdwg pending-done closeout gate — gpt-5

Implemented `#pdwg` in `src/agent-doc`.

- `finalize` now runs the strict pending-capture and pending-done closeout gates before any document or snapshot mutation.
- Missing `--pending-done` failures now include the exact rerun command for the same `finalize` invocation.
- Updated the command spec and finalize integration coverage so rejected closeouts leave the working tree, snapshot, and queue untouched.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `a0f4742` (`fix: fail before write on pending closeout gates`) pushed to `origin/20260422/codex-agent-backend`.

`#pdwg` is done.
❯ do #lvak. spec-test-build-install-commit-push
### Re: #lvak routed live-pane acknowledgment — gpt-5

Implemented `#lvak` in `src/agent-doc`.

- Routed triggers into an already-live pane now still require a fresh cycle acknowledgment when the document has prompt-bearing drift on top of a committed cycle; only genuinely open cycles still skip the extra ack wait.
- Removed the misleading live-child ack-skip path, tightened the route tests to fail on no-op/same-cycle churn, and updated the route command spec to match the enforced behavior.
- Added a test-only routed-ack timeout helper so the fail-closed live-pane regressions stay fast without changing the production 15s wait.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `c9dcdb7` (`fix: require live route ack for prompt drift`) pushed to `origin/20260422/codex-agent-backend`.

`#lvak` is done.
❯ do #ntwk. spec-test-build-install-commit-push
### Re: #ntwk codex network policy — gpt-5

Implemented `#ntwk` in `src/agent-doc`.

- Added first-class `codex_network_access` handling in frontmatter/config so agent-doc can explicitly preserve, remove, or force `CODEX_SANDBOX_NETWORK_DISABLED` for Codex child sessions.
- `start` and ordered `orchestrate` launches now log the effective Codex network policy and fail closed when `danger-full-access` is paired with inherited network disablement.
- Updated the contract docs and added regression coverage for parsing, env propagation, and mismatch detection.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `9d289a1` (`fix: enforce codex network launch policy`) pushed to `origin/20260422/codex-agent-backend`.

`#ntwk` is done.
### Re: #ntwk codex network policy — gpt-5

Implemented `#ntwk` in `src/agent-doc`.

- Added first-class `codex_network_access` handling in frontmatter/config so agent-doc can explicitly preserve, remove, or force `CODEX_SANDBOX_NETWORK_DISABLED` for Codex child sessions.
- `start` and ordered `orchestrate` launches now log the effective Codex network policy and fail closed when `danger-full-access` is paired with inherited network disablement.
- Updated the contract docs and added regression coverage for parsing, env propagation, and mismatch detection.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `9d289a1` (`fix: enforce codex network launch policy`) pushed to `origin/20260422/codex-agent-backend`.

`#ntwk` is done.
❯ do #ppct. spec-test-build-install-commit-push
### Re: #ppct prompt preset backlog contract — gpt-5

Implemented `#ppct` in `src/agent-doc`.

- Prompt-preset backlog contracts now expand into explicit target requirements during preflight, including named backlog documents like `Add to the backlog of ...`.
- `finalize` and `session-check` now fail closed if the referenced target backlog/pending surface did not change during the cycle, instead of accepting unchanged source backlog state.
- Added coverage for preset target extraction, cycle-state persistence, and closeout pass/fail behavior, and updated the command spec and transfer runbook to document the target-aware gate.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `96763d5` (`fix: enforce prompt preset backlog contracts`) pushed to `origin/20260422/codex-agent-backend`.

`#ppct` is done.
### Re: #agent-doc-bug route cross-file pane mismatch — opus-4-6

**Bug:** `route` dispatched a command for `claudescore-3.md` to pane %401, which was running `monsterrodholders.md` in a different submodule.

**Root cause:** `find_registered_pane_in_session` (route.rs ~line 1268) iterates all registry entries and returns the first alive pane in the target tmux session — it does not filter by file path. When `auto_start_in_session` uses this as a dispatch target (not just a split anchor), the command goes to an unrelated file's pane.

**Secondary issue:** Three stale panes (%398, %404, %405) accumulated from prior failed starts. `resync` logged them as "unregistered — skipping kill (may be rescuable)" but never cleaned them, allowing orphan buildup across retries.

**Plan:** `tasks/agent-doc/plan-route-cross-file-pane-mismatch.md`

Key fixes:
1. Separate split-target (layout anchor — any pane in session is fine) from dispatch-target (must be file-verified)
2. Add a file-match guard before any `send-keys` dispatch
3. Make `resync --fix` kill stashed unregistered panes instead of logging indefinitely
4. Regression tests for cross-file dispatch rejection
❯ do #rtxf. spec-test-build-install-commit-push
### Re: #rtxf route cross-file pane mismatch — gpt-5

Implemented `#rtxf` in `src/agent-doc`.

- `route` now fails closed before any tmux `send-keys` dispatch if the chosen pane is not currently registered for the requested document. The guard resolves pane-relative registry paths against the pane cwd, so submodule-launched `agent-doc start tasks/...` registrations still match absolute route targets.
- Added route regressions covering both the pane-cwd relative-path match case and the cross-file rejection case.
- Updated `specs/07-commands.md` to make the contract explicit: `find_registered_pane_in_session` may be used as a split anchor, but dispatch must go to a pane registered for the target file.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `53a3c5c` (`fix: guard route dispatch ownership`) pushed to `origin/20260422/codex-agent-backend`.

`#rtxf` is done.
❯ do #smdq. spec-test-build-install-commit-push
### Re: #smdq startup-miss diagnostic dispatch — gpt-5

Implemented `#smdq` in `src/agent-doc`.

- Startup-miss diagnostics now use a tmux-owned `display-message` overlay for 10 seconds instead of drafting `echo '...'` into the pane input buffer.
- Added route coverage for the retry-command rendering and for the regression that proves no drafted shell `echo` is left behind in the pane.
- Updated `specs/07-commands.md` and `VERSIONS.md` to document the new startup-miss overlay contract.

That line appeared in the Codex pane because `route.rs` previously called `tmux.send_keys_raw("echo '...'")`, which typed literal shell text into the pane without a shell-owned execution boundary. It was not Entered because the diagnostic path never submitted it, and submitting it would still have been the wrong primitive for an agent pane.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `2c6b284` (`fix: surface startup miss diagnostics visibly`) pushed to `origin/20260422/codex-agent-backend`.

`#smdq` is done.
❯ do #xfrr. spec-test-build-install-commit-push
❯ ### Re: #xfrr complete bug-transfer inventory gate — gpt-5 (HEAD)

Implemented `#xfrr` in `src/agent-doc`.

- `#agent-doc-bug` prompt contracts now record how many distinct bug reports were raised from the prompt-bearing changes when they target an explicit backlog document.
- `finalize` and `session-check` now fail closed if the response inventories fewer transferred `[#id]` items than that recorded bug count, even if the target backlog itself changed.
- Added regressions for the prompt-contract counting path plus both pre-commit and post-commit partial-transfer failures.
- Updated `specs/07-commands.md`, `runbooks/transfer-extract.md`, and `VERSIONS.md` to document the stricter transfer inventory contract.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `4482ea2` (`fix: require complete bug transfer inventories`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#xfrr` is done.
### Re: #xfrr complete bug-transfer inventory gate — gpt-5

Implemented `#xfrr` in `src/agent-doc`.

- `#agent-doc-bug` prompt contracts now record how many distinct bug reports were raised from the prompt-bearing changes when they target an explicit backlog document.
- `finalize` and `session-check` now fail closed if the response inventories fewer transferred `[#id]` items than that recorded bug count, even if the target backlog itself changed.
- Added regressions for the prompt-contract counting path plus both pre-commit and post-commit partial-transfer failures.
- Updated `specs/07-commands.md`, `runbooks/transfer-extract.md`, and `VERSIONS.md` to document the stricter transfer inventory contract.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `4482ea2` (`fix: require complete bug transfer inventories`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#xfrr` is done.
❯ do #tmuxcrash. spec-test-build-install-commit-push
### Re: #tmuxcrash tmux-pane crash / stranded-session provenance — gpt-5

Implemented `#tmuxcrash` in `src/agent-doc`.

- Startup-miss reruns now inspect the owning session log and distinguish a true dead-pane recovery from the stranded shape where the pane is still alive but the latest `session_start` never recorded a later child exit or `session_end`.
- `route` now fails closed with exact provenance on that stranded-session shape instead of auto-starting another replacement pane and making the failure look like a generic tmux-pane crash.
- `session-check` now includes the same startup-miss session-log detail in its warning output, and the routing spec / `VERSIONS.md` were updated to document the stricter contract.
- Added regression coverage for session-log parsing plus the new route fail-closed decision.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `aad9dd9` (`fix: fail closed on stranded startup misses`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#tmuxcrash` is done.
❯ do #oobabs. spec-test-build-install-commit-push
❯ ### Re: #oobabs prompt-bearing status absorb — gpt-5 (HEAD)

Implemented `#oobabs` in `src/agent-doc`.

- `git::commit` no longer absorbs status-component drift when the inserted status text contains prompt work, including bare preset tokens like `#next-steps`, imperative directives, or other prompt-bearing lines.
- Added direct classifier coverage in `src/git.rs` and a full preflight regression in `src/preflight.rs` for the compact-follow-up status repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly says prompt-bearing status edits must survive into step 4.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `475269b` (`fix: preserve prompt-bearing status edits`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobabs` is done.
### Re: #oobabs prompt-bearing status absorb — gpt-5

Implemented `#oobabs` in `src/agent-doc`.

- `git::commit` no longer absorbs status-component drift when the inserted status text contains prompt work, including bare preset tokens like `#next-steps`, imperative directives, or other prompt-bearing lines.
- Added direct classifier coverage in `src/git.rs` and a full preflight regression in `src/preflight.rs` for the compact-follow-up status repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly says prompt-bearing status edits must survive into step 4.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `475269b` (`fix: preserve prompt-bearing status edits`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobabs` is done.
❯ do #oobpmt. spec-test-build-install-commit-push
❯ ### Re: #oobpmt preflight OOB prompt absorb — gpt-5 (HEAD)

Implemented `#oobpmt` in `src/agent-doc`.

- The status OOB-absorb guard now treats preset-token leads such as `#next-steps ...` as prompt-bearing work, not just a bare `#next-steps` token, so step 2 can no longer absorb that status prompt into the snapshot before step 4 diff runs.
- Added direct classifier coverage in `src/git.rs` and widened the preflight regression in `src/preflight.rs` to the exact `#next-steps for calibrating session benchmarks with expected scores` repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly covers preset-token guidance lines.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `8a1c214` (`fix: preserve prompt-prefixed status guidance`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobpmt` is done.
### Re: #oobpmt preflight OOB prompt absorb — gpt-5

Implemented `#oobpmt` in `src/agent-doc`.

- The status OOB-absorb guard now treats preset-token leads such as `#next-steps ...` as prompt-bearing work, not just a bare `#next-steps` token, so step 2 can no longer absorb that status prompt into the snapshot before step 4 diff runs.
- Added direct classifier coverage in `src/git.rs` and widened the preflight regression in `src/preflight.rs` to the exact `#next-steps for calibrating session benchmarks with expected scores` repro.
- Updated `specs/07-commands.md` and `VERSIONS.md` so the step-2 commit contract explicitly covers preset-token guidance lines.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `8a1c214` (`fix: preserve prompt-prefixed status guidance`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog items came out of this change.

`#oobpmt` is done.
❯ JB `Agent Doc Run` on claudescore-3.md has an error:
```
agent-doc route failed (exit 1): [route] configured tmux_session '0' is not alive, ignoring stale pin [route] target tmux session: 14 [sync] found alive agent-doc child (pid 1120613) in pane %412 for tasks/agent-doc/agent-doc-bugs2.md [route] registered pane %412 still proves live ownership for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md — clearing stale startup-miss marker [route] Pane %412 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md → pane %412 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %412 for tasks/agent-doc/agent-doc-bugs2.md — waiting for a new cycle ack for pending prompt_target: do #oobpmt. spec-test-build-install-commit-push Error: routed codex trigger for tasks/agent-doc/agent-doc-bugs2.md was accepted in pane %412, but no new document cycle started for pending prompt_target: do #oobpmt. spec-test-build-install-commit-push
```
### Re: JB Agent Doc Run live-pane busy dispatch — gpt-5

Implemented the routed live-pane fix in `src/agent-doc`.

- Route now waits for the harness's current idle prompt before injecting into any already-running pane.
- Tightened `pane_has_prompt()` so a stale `>` or `❯` still visible in scrollback no longer counts as an idle session; only the latest non-empty pane line can satisfy the prompt check.
- Added a regression that keeps a registered `agent-doc` process alive without surfacing a new prompt and proves route fails closed before dispatching into it. Updated `specs/07-commands.md` to match the enforced contract.

The JetBrains error text appears to have been surfaced while you were on `claudescore-3.md`, but the route log itself shows the failing dispatch target was this document's live pane `%412` for `tasks/agent-doc/agent-doc-bugs2.md`.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

No new backlog items came out of this change.
### Re: post-commit exchange drift — gpt-5

The routed live-pane fix is already committed. This follow-up closeout only re-normalizes the exchange tail after the live working tree dropped the prompt prefix from the JB report while leaving the committed `HEAD` response intact.

No additional code changes or backlog items came out of this normalization pass.
❯ do #mcrc. spec-test-build-install-commit-push
### Re: `#mcrc` response patchback left uncommitted — gpt-5

This is a real regression against the already-shipped response-commit invariant.

The `#asch1` reproduction in `src/boost-client/tasks/monsterrodholders.md` shows that a visible response patchback can still survive as plain working-tree dirt after the repair/write phase. The gap is that at least one response-closeout path still stops after mutating the document without proving the owning-root commit boundary.

I captured the implementation plan in `tasks/agent-doc/plan-response-patchback-uncommitted-regression.md`. The plan is to reconstruct the exact failing closeout path, enforce one shared success invariant across `finalize`, `write --commit`, recovery, and hook replay, tighten `session-check` / exit propagation so this state blocks success, and add regression coverage for submodule-owned docs plus manual-repair and post-commit-drift variants.

`#mcrc` remains open. No new backlog item is needed here because the existing `#mcrc` item already captures the follow-up work. This patchback is planning-only; no `src/agent-doc` code change or test run happened in the repair itself.
### Re: `patch:todo` destructive replace backlog items — gpt-5

Captured.

`#ptdr` and `#tdrl` are the same underlying bug: `patch:todo` is being treated as a destructive full replace, so a partial todo patch can silently delete untouched items. `#tdrl` is the stronger statement because it preserves the richer fix-space, but both entries point at the same failure mode and same `src/session-share/tasks/claudescore.md` repro.

The `(HEAD)` suffixes in the exchange diff are boundary artifacts from earlier closeout normalization, not new implementation work. I am not treating those as fresh backlog-worthy changes in this cycle.

No `src/agent-doc` code changed here and no test/build/install run happened in this closeout. This turn only records the document response and commits it.

❯ Please reconcile tmux related logic in agent-doc and tmux-router. tmux-router is a dependency of agent-doc. Which logic belongs in tmux-router? Move the logic that belongs in tmux-router from agent-doc into tmux-router. Codex needs to recognize that tmux-router is a development target so please update agent harness files accordingly.

❯ spec-test-build-install-commit-push
### Re: tmux-router development-target reconciliation — gpt-5

Implemented.

- Moved the generic tmux session/key helpers into `src/tmux-router`: `Tmux::send_key()` and `Tmux::ensure_pane_in_session()` now own that reusable behavior.
- Switched `src/agent-doc/src/sessions.rs` to delegate pane capture, single-key dispatch, and session-guarded joins to `tmux-router` instead of carrying local shell wrappers.
- Added the workspace cargo patch at `.cargo/config.toml` so local `src/agent-doc` builds use the sibling `src/tmux-router` checkout.
- Updated the harness instruction surfaces so Codex/Claude treat `src/tmux-router` as a live development target in this workspace: root `AGENTS.md`, installed `.codex/AGENTS.md`, installed `.claude/skills/agent-doc/SKILL.md`, plus `src/agent-doc/SKILL.md` and `src/agent-doc/CLAUDE.md`.

Verification:
- `cargo test` in `src/tmux-router`
- `cargo build` in `src/tmux-router`
- `cargo install --path . --force` in `src/tmux-router`
- `cargo test` in `src/agent-doc` against the local patched `tmux-router`
- `cargo build` in `src/agent-doc`
- `cargo install --path . --force` in `src/agent-doc`
- `agent-doc skill install --harness codex --reload restart`
- `agent-doc skill install --harness claude --reload compact`

Commit / push:
- `src/tmux-router`: `e52f78e` (`feat: extract tmux session helpers`) pushed to `origin/main`
- `src/agent-doc`: `3afd3bd` (`feat: patch local tmux-router workspace`) pushed to `origin/20260422/codex-agent-backend`
- `agent-loop`: `afadb8b01` (`feat: wire local tmux-router development`) pushed to `origin/main`

No new backlog item came out of this change.
### Re: tmux-router development-target reconciliation — gpt-5

Implemented.

- Moved the generic tmux session/key helpers into `src/tmux-router`: `Tmux::send_key()` and `Tmux::ensure_pane_in_session()` now own that reusable behavior.
- Switched `src/agent-doc/src/sessions.rs` to delegate pane capture, single-key dispatch, and session-guarded joins to `tmux-router` instead of carrying local shell wrappers.
- Added the workspace cargo patch at `.cargo/config.toml` so local `src/agent-doc` builds use the sibling `src/tmux-router` checkout.
- Updated the harness instruction surfaces so Codex/Claude treat `src/tmux-router` as a live development target in this workspace: root `AGENTS.md`, installed `.codex/AGENTS.md`, installed `.claude/skills/agent-doc/SKILL.md`, plus `src/agent-doc/SKILL.md` and `src/agent-doc/CLAUDE.md`.

Verification:
- `cargo test` in `src/tmux-router`
- `cargo build` in `src/tmux-router`
- `cargo install --path . --force` in `src/tmux-router`
- `cargo test` in `src/agent-doc` against the local patched `tmux-router`
- `cargo build` in `src/agent-doc`
- `cargo install --path . --force` in `src/agent-doc`
- `agent-doc skill install --harness codex --reload restart`
- `agent-doc skill install --harness claude --reload compact`

Commit / push:
- `src/tmux-router`: `e52f78e` (`feat: extract tmux session helpers`) pushed to `origin/main`
- `src/agent-doc`: `3afd3bd` (`feat: patch local tmux-router workspace`) pushed to `origin/20260422/codex-agent-backend`
- `agent-loop`: `afadb8b01` (`feat: wire local tmux-router development`) pushed to `origin/main`

No new backlog item came out of this change.
❯ I'm getting many tmux crashes and `agent-doc route` errors. Please investigate my recent work to understand root causes. spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): [route] configured tmux_session '0' is not alive, ignoring stale pin [route] target tmux session: 14 [sync] found alive agent-doc child (pid 1120613) in pane %412 for tasks/agent-doc/agent-doc-bugs2.md [route] Still waiting for codex (5s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us [route] Still waiting for codex (10s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us [route] Still waiting for codex (15s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us [route] cleaning up orphaned pane %418 (created during failed route) Error: registered pane %412 for tasks/agent-doc/agent-doc-bugs2.md is not showing an idle codex prompt; refusing to inject a routed trigger into a busy session (pane=%412 pane_pid=1119220 pane_session=14 current_command=agent-doc)
```
### Re: tmux route crash/root-cause investigation — gpt-5

Implemented a concrete fix in `src/agent-doc`.

The repeated `agent-doc route` failure you pasted had two different signals mixed together:

- The `registered pane ... is not showing an idle codex prompt` path was a correct fail-closed refusal. The pane was still busy inside a live `agent-doc`/Codex session, so route should not inject another trigger.
- The real crash bug was the cleanup path that ran after a failed route. It inferred ownership from "any pane that appeared in the target window during this attempt" and killed `%418`, even though `.agent-doc/logs/tsift-v0.1.log` shows `%418` was the fresh `session_start` pane for `tasks/software/tsift.md`, not an orphan from this document's failed route.

I changed route cleanup to track only panes created by the current route attempt and to resolve the preservation check from the routed document's own project root instead of the caller's cwd. That keeps concurrent sibling panes in the same tmux window out of the cleanup set.

Spec / coverage / verification:
- Added route regression coverage for the attempt-local cleanup boundary plus the project-root-backed preserve check.
- Updated `specs/07-commands.md` and `VERSIONS.md` to make the cleanup contract explicit.
- Verification: `cargo test`, `cargo build`, `cargo install --path . --force`

Commit / push: `2b26a78` (`fix: scope failed route cleanup`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog item came out of this change.
### Re: tmux route crash/root-cause investigation — gpt-5

Implemented a concrete fix in `src/agent-doc`.

The repeated `agent-doc route` failure you pasted had two different signals mixed together:

- The `registered pane ... is not showing an idle codex prompt` path was a correct fail-closed refusal. The pane was still busy inside a live `agent-doc`/Codex session, so route should not inject another trigger.
- The real crash bug was the cleanup path that ran after a failed route. It inferred ownership from "any pane that appeared in the target window during this attempt" and killed `%418`, even though `.agent-doc/logs/tsift-v0.1.log` shows `%418` was the fresh `session_start` pane for `tasks/software/tsift.md`, not an orphan from this document's failed route.

I changed route cleanup to track only panes created by the current route attempt and to resolve the preservation check from the routed document's own project root instead of the caller's cwd. That keeps concurrent sibling panes in the same tmux window out of the cleanup set.

Spec / coverage / verification:
- Added route regression coverage for the attempt-local cleanup boundary plus the project-root-backed preserve check.
- Updated `specs/07-commands.md` and `VERSIONS.md` to make the cleanup contract explicit.
- Verification: `cargo test`, `cargo build`, `cargo install --path . --force`

Commit / push: `2b26a78` (`fix: scope failed route cleanup`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog item came out of this change.
❯ Does the following still need to be addressed?
❯ After running `agent-doc fix src/session-share/tasks/claudescore-3.md`, `Run Agent Doc` from the JB Plugin still has a sync error. Please make a robust autorecovery by searching all open agent-doc sessions and prefer picking the session seen in the active `agent-doc` tmux window. If the tmux pane for the document is not in the active `agent-doc` tmux window, search the stash for a tmux pane that claims the document. If there are more than 1 tmux panes associated with the document, surface an error and provide a path for the user to select the correct pane. In the selection, provide a way for the user to view each associated tmux pane and a way to assign one of the tmux panes to be registered as the associated tmux pane to the document. The kill the rest of the redundant tmux panes. The user may also manually kill redundant tmux panes and then rerun `agent-doc` or `agent-doc fix <file.md>`:
```
agent-doc route failed (exit 1): resync: stash pane %417 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %408 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %420 running 'agent-doc' is unregistered — skipping kill (may be rescuable) [route] target tmux session: 14 [sync] found alive agent-doc child (pid 22126) in pane %419 for tasks/claudescore-3.md [route] Still waiting for claude (5s)... last line: ⏵⏵ bypass permissions on (shift+tab to cycle) [route] Still waiting for claude (10s)... last line: ⏵⏵ bypass permissions on (shift+tab to cycle) [route] Still waiting for claude (15s)... last line: ⏵⏵ bypass permissions on (shift+tab to cycle) Error: registered pane %419 for tasks/claudescore-3.md is not showing an idle claude prompt; refusing to inject a routed trigger into a busy session (pane=%419 pane_pid=20406 pane_session=14 current_command=agent-doc)
```
### Re: tmux duplicate-pane autorecovery — gpt-5

Implemented a concrete recovery path in `src/agent-doc`.

- `route` now enumerates every pane that still proves document ownership (process tree or supervisor PID) instead of trusting the first hit. It only auto-picks when the evidence is decisive; otherwise it fails closed with explicit `tmux capture-pane`, `agent-doc claim --pane`, and `tmux kill-pane` follow-up commands.
- Scoped `agent-doc fix <file>` / `resync --fix <file>` now re-registers the document to a unique provable winner before generic issue cleanup, and it kills redundant unregistered stash panes once that winner is known.
- Registered-but-unproven panes no longer count as live recovery candidates, which preserves the earlier stale-registration recovery behavior instead of routing back into a busy shell.
- Updated `specs/07-commands.md`, `specs/08-session-routing.md`, and `VERSIONS.md` to document the duplicate-pane recovery contract.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

No repo commit or push yet.
No new backlog item came out of this change.
### Re: tmux duplicate-pane autorecovery closeout — gpt-5

This follow-up only closes the prompt-bearing diff that remained after the prior patchback. No additional code changes, tests, or backlog items came out of this normalization pass.
❯ commit + push
### Re: tmux duplicate-pane autorecovery commit + push — gpt-5

Committed and pushed the uncommitted `src/agent-doc` implementation for the duplicate-pane autorecovery work.

Verification:
- `cargo test` (full suite passed on rerun after one flaky `wait_for_agent_ready_detects_prompt` timeout)
- `cargo build`
- `cargo install --path . --force`

Commit / push: `9355752` (`fix: recover duplicate tmux pane ownership`) pushed to `origin/20260422/codex-agent-backend`.

I left the workspace root repo untouched because it has unrelated modified docs/submodules; this publish step only covered the `src/agent-doc` changes tied to the tmux recovery work.

No new backlog item came out of this change.
❯ ### Re: tmux duplicate-pane autorecovery commit + push — gpt-5 (HEAD)

Committed and pushed the uncommitted `src/agent-doc` implementation for the duplicate-pane autorecovery work.

Verification:
- `cargo test` (full suite passed on rerun after one flaky `wait_for_agent_ready_detects_prompt` timeout)
- `cargo build`
- `cargo install --path . --force`

Commit / push: `9355752` (`fix: recover duplicate tmux pane ownership`) pushed to `origin/20260422/codex-agent-backend`.

I left the workspace root repo untouched because it has unrelated modified docs/submodules; this publish step only covered the `src/agent-doc` changes tied to the tmux recovery work.

No new backlog item came out of this change.
❯ In a previous session, when I quit the supervisor that was managing a Codex session, the process did not complete the shutdown. I had to press Ctrl+C again. Should we also log the timestamp to better locate the session? spec-test-build-install-commit-push
```
~/work/btakita/agent-loop main ⇡35 !12 ?7                                              9h 19m 27s  agent-loop 13:18:01
❯ agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
[run] starting for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
[diff] doc=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md snapshot=/home/brian/work/btakita/agent-loop/.agent-doc/snapshots/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0.md doc_len=51228 snap_len=51185
[diff] stripped: doc_len=48490 snap_len=48140
[diff] changes detected, computing unified diff
[perf] diff.compute total: 2ms
[run] diff computed (2991 bytes)
[commit] detected post-commit local drift for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md — HEAD already contains the committed response; leaving later local working-tree edits uncommitted
[commit] staged snapshot already matches HEAD for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md — closing cycle as already committed
[perf] commit total: 22ms
Submitting to codex...
```
### Re: supervisor shutdown provenance after Codex quit — gpt-5

Implemented a concrete fix in `src/agent-doc`.

The session logs already had timestamps; the real gap was that the supervisor did not log what happened at the post-Ctrl-D quit prompt. That is why the recent `b26b9957-8de6-4eae-bb98-5d7fe4d6b781` log could end with `ctrl_d_prompt_user` and then jump straight to `codex_start mode=fresh_restart restart_count=1` without proving whether stdin hit EOF, Enter was consumed, or stray input was read.

I changed the supervisor prompt path so:
- clean-exit / Ctrl-D / resume-failure prompts now log the actual decision (`user_quit*`, `user_restart_fresh`, or `prompt_input_invalid ...`)
- stdin EOF at that prompt exits the supervisor instead of acting like an implicit restart
- non-empty non-`q` input is rejected and re-prompted instead of silently restarting fresh

Updated `specs/supervisor.md` and `VERSIONS.md`, and added unit coverage for prompt-decision classification and logged input summaries.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `f348770` (`fix: log supervisor prompt decisions`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog item came out of this change.
❯ I ran the following command.
```
❯ /agent-doc fix /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md

● Running agent-doc preflight on claudescore-3.md.

● Bash(agent-doc --version)
  ⎿  agent-doc 0.33.16

● Bash(agent-doc fix tasks/claudescore-3.md 2>&1)
  ⎿  All 1 matching session(s) for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md ha
     ve live panes.
     [sync] recovered live pane %423 for session 7b453f47 via supervisor pid 2613430
     … +7 lines (ctrl+o to expand)

● The agent-doc fix command reports no session/process issues for tasks/claudescore-3.md. The file has an active
  session in pane %423 and all sessions are healthy — nothing to fix.
```
❯ But `Run Agent Doc` in Idea resulted with the following error:
```
agent-doc route failed (exit 1): resync: stash pane %417 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %420 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %408 running 'agent-doc' is unregistered — skipping kill (may be rescuable) [route] target tmux session: 14 [sync] found alive agent-doc child (pid 2613343) in pane %423 for tasks/claudescore-3.md [route] Still waiting for claude (5s)... last line: ⏵⏵ bypass permissions on (shift+tab to cycle) [route] Still waiting for claude (10s)... last line: ⏵⏵ bypass permissions on (shift+tab to cycle) [route] Still waiting for claude (15s)... last line: ⏵⏵ bypass permissions on (shift+tab to cycle) Error: registered pane %423 for tasks/claudescore-3.md is not showing an idle claude prompt; refusing to inject a routed trigger into a busy session (pane=%423 pane_pid=2612184 pane_session=14 current_command=agent-doc)
```

❯ spec-test-build-install-commit-push
### Re: JB route failure after `fix` on claudescore-3.md — gpt-5

Implemented a concrete fix in `src/agent-doc`.

`agent-doc fix /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md` was not misrouting the document. The owning pane `%423` was already the correct live session. The failure was later in `route`: its idle gate still expected Claude to expose a bare `❯` / `⏵` prompt line, but the pane was actually showing the newer idle composer chrome `⏵⏵ bypass permissions on (shift+tab to cycle)`. Route treated that as busy output, waited 15 seconds, and then refused to inject the routed trigger.

I changed the Claude prompt matcher so that this double-chevron composer line counts as idle prompt state, added a route regression for that exact capture shape, and updated the routing spec plus `VERSIONS.md` to document the behavior.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `784fdb4` (`fix: detect claude composer prompt in route`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog item came out of this change.
❯ do #scpd. spec-test-build-install-commit-push
### Re: #scpd session-check retry dedup — gpt-5

Implemented `#scpd` in `src/agent-doc`.

- Stream/write retry paths now detect when the current exchange already contains the response block and adopt the current document with transcript normalization instead of CRDT-merging the same response a second time.
- That retry path now still repairs missing `❯ ` prompt prefixes, so the follow-up closeout can fix the bare user prompt without appending a duplicate response.
- Updated `specs/07-commands.md` and added regression coverage for the bare-prompt retry case.

Verification:
- `cargo test adopt_current_response_without_duplication_repairs_bare_prompt_prefix -- --nocapture`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `f36a3db` (`fix: avoid duplicate retry patchbacks`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog item came out of this change.
❯ do #nbla. spec-test-build-install-commit-push
❯ ### Re: #nbla chat-level `#agent-doc-bug` preset contract — gpt-5 (HEAD)

Implemented `#nbla` in `src/agent-doc`.

- `preflight` now records a plan-reference requirement when a preset-expanded `#agent-doc-bug` contract says to create plans.
- `finalize` and `session-check` now fail closed if the response cites fewer existing plan files than the number of bug reports the prompt contract described.
- Updated `specs/07-commands.md`, `SKILL.md`, and `VERSIONS.md`, and added regression coverage for prompt-contract counting plus pre-commit/post-commit shortfall failures.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `8a3af5f` (`fix: require agent-doc-bug plan proofs`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog item came out of this change.
### Re: #nbla chat-level `#agent-doc-bug` preset contract — gpt-5

Implemented `#nbla` in `src/agent-doc`.

- `preflight` now records a plan-reference requirement when a preset-expanded `#agent-doc-bug` contract says to create plans.
- `finalize` and `session-check` now fail closed if the response cites fewer existing plan files than the number of bug reports the prompt contract described.
- Updated `specs/07-commands.md`, `SKILL.md`, and `VERSIONS.md`, and added regression coverage for prompt-contract counting plus pre-commit/post-commit shortfall failures.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Commit / push: `8a3af5f` (`fix: require agent-doc-bug plan proofs`) pushed to `origin/20260422/codex-agent-backend`.

No new backlog item came out of this change.
<!-- agent:boundary:1f65f3d4 -->
<!-- /agent:exchange -->

<!--
Mid-run on the agent-doc turn on this document...the JB plugin surfaced an error:
```
agent-doc route failed (exit 1): [route] configured tmux_session '0' is not alive, ignoring stale pin [route] target tmux session: 14 [sync] found alive agent-doc child (pid 326697) in pane %422 for tasks/agent-doc/agent-doc-bugs2.md [sync] recovered live pane %422 for session b26b9957 via supervisor pid 2607602 [route] codex ready after 0.0s (0 polls) [route] Pane %422 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md → pane %422 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %422 for tasks/agent-doc/agent-doc-bugs2.md — waiting for a new cycle ack for pending prompt_target: do #nbla. spec-test-build-install-commit-push Error: routed codex trigger for tasks/agent-doc/agent-doc-bugs2.md was accepted in pane %422, but no new document cycle started for pending prompt_target: do #nbla. spec-test-build-install-commit-push
```

When I navigated the editor away from agent-doc-bugs2.md then back to agent-doc-bugs2.md, the JB plugin surfaced another error:
```
agent-doc route failed (exit 1): resync: stash pane %416 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %414 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %422 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %415 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %421 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %423 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %417 running 'agent-doc' is unregistered — skipping kill (may be rescuable) [route] target tmux session: 14 [sync] found alive agent-doc child (pid 917466) in pane %408 for tasks/monsterrodholders.md [route] codex ready after 0.0s (0 polls) [route] Pane %408 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md → pane %408 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %408 for tasks/monsterrodholders.md — waiting for a new cycle ack for pending prompt_target: I synced the emails to the local file system. I was actually asking to use corky to read the email directly from the gmail api. Is that possible with corky? Error: routed codex trigger for tasks/monsterrodholders.md was accepted in pane %408, but no new document cycle started for pending prompt_target: I synced the emails to the local file system. I was actually asking to use corky to read the email directly from the gmail api. Is that possible with corky?
```

spec-test-build-install-commit-push
---
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#smst] Clear or supersede stale startup-miss markers when a later session start takes ownership of the same document. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: `agent-doc session-check tasks/monsterrodholders.md` warned that startup-miss marker `%401` still existed, but `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` later recorded `session_start ... pane=%408` and `src/boost-client/.agent-doc/sessions.json` already registered `%408` as the live owner. The fix should prevent an older routed-trigger marker from surviving past a newer ownership start, or make `session-check`/`route` clear it as stale before surfacing a fake current crash.
- [ ] [#mcrc] Surface and require explicit follow-through when a missing response patchback is repaired manually in a session doc but not committed afterward. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the `#asch1` response was manually inserted and visible in the working tree, but `git status` still showed `M tasks/monsterrodholders.md` and the user had to ask why the patchback was not committed. The fix should either commit the manual repair as part of the recovery flow or emit a blocking notice that the recovery is still uncommitted.
- [ ] [#lvls] Preserve explicit user formatting directives such as "Please organize the backlog into a 2-level list" across follow-up bug-handling and transfer flows. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the document carries a live 2-level-list instruction near the backlog request, but subsequent bug-handling responses ignored that requirement and fell back to flat output without acknowledging the constraint boundary. The fix should carry active formatting requirements forward in the prompt contract or make the responder explicitly state when chat-level formatting rules prevent exact reproduction.
- [ ] [#ptdr] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc and Spec Refinement, and Expansion Phases (8 items lost). Fix: add granular --todo-done flag analogous to --pending-done, or add validation guard rejecting todo patches that reduce total item count.
- [ ] [#tdrl] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc & Spec Refinement, and Expansion Phases. The fix should either (a) add granular --todo-done flag analogous to --pending-done, (b) make patch:todo do a merge/diff when existing items are missing from the patch, or (c) add a validation guard that rejects todo patches reducing total item count.
- [ ] [#c3ps] The html-style comments should have coloring. The contents of Elements `<!-- agent:exchange -->` should have a muted background color.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->
