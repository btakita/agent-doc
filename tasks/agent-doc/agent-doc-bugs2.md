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
<!-- agent:boundary:855cef3d -->
<!-- /agent:exchange -->

<!--
JB `Agent Doc Run` on claudescore-3.md has an error:
```
agent-doc route failed (exit 1): [route] configured tmux_session '0' is not alive, ignoring stale pin [route] target tmux session: 14 [sync] found alive agent-doc child (pid 1120613) in pane %412 for tasks/agent-doc/agent-doc-bugs2.md [route] registered pane %412 still proves live ownership for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md — clearing stale startup-miss marker [route] Pane %412 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md → pane %412 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %412 for tasks/agent-doc/agent-doc-bugs2.md — waiting for a new cycle ack for pending prompt_target: do #oobpmt. spec-test-build-install-commit-push Error: routed codex trigger for tasks/agent-doc/agent-doc-bugs2.md was accepted in pane %412, but no new document cycle started for pending prompt_target: do #oobpmt. spec-test-build-install-commit-push
```
---
Add `agent-doc fix`. `agent-doc resync --fix` is an alias to `agent-doc fix`. Support an optional file-path to agent doc that I want to fix `agent-doc fix <path-to-file.md>`.
---
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#mcrc] Surface and require explicit follow-through when a missing response patchback is repaired manually in a session doc but not committed afterward. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the `#asch1` response was manually inserted and visible in the working tree, but `git status` still showed `M tasks/monsterrodholders.md` and the user had to ask why the patchback was not committed. The fix should either commit the manual repair as part of the recovery flow or emit a blocking notice that the recovery is still uncommitted.
- [ ] [#lvls] Preserve explicit user formatting directives such as "Please organize the backlog into a 2-level list" across follow-up bug-handling and transfer flows. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the document carries a live 2-level-list instruction near the backlog request, but subsequent bug-handling responses ignored that requirement and fell back to flat output without acknowledging the constraint boundary. The fix should carry active formatting requirements forward in the prompt contract or make the responder explicitly state when chat-level formatting rules prevent exact reproduction.
- [ ] [#c3ps] The html-style comments should have coloring. The contents of Elements `<!-- agent:exchange -->` should have a muted background color.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->
