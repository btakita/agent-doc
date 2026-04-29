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
<!-- agent:boundary:ce9d6152 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ppct] Fail closed when a user prompt tagged with `#agent-doc-bug` is answered without executing its frontmatter `prompt_presets` contract. Repro from `src/boost-client/ta****sks/monsterrodholders.md` on Apr 29, 2026: the session frontmatter says `#agent-doc-bug` must "create a plan for agent-doc to fix this issue" and "Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md", but the response answered the questions directly and left the newly reported bugs untransferred. The fix should make preset-required side effects binary-owned during plan/closeout, not dependent on the responder remembering them.
- [ ] [#xfrr] Fail closed when an `#agent-doc-bug` response identifies multiple bug reports but transfers only a subset to `agent-doc-bugs2.md`. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: only `#zpc0` and `#lvak` were added, while the additional user-reported bugs about the uncommitted repair, missing backlog transfer, and ignored list-shape requirement were left out. The fix should compare the prompt contract against the response/body mutations and interrupt when the promised bug-transfer set is incomplete.
- [ ] [#mcrc] Surface and require explicit follow-through when a missing response patchback is repaired manually in a session doc but not committed afterward. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the `#asch1` response was manually inserted and visible in the working tree, but `git status` still showed `M tasks/monsterrodholders.md` and the user had to ask why the patchback was not committed. The fix should either commit the manual repair as part of the recovery flow or emit a blocking notice that the recovery is still uncommitted.
- [ ] [#lvls] Preserve explicit user formatting directives such as "Please organize the backlog into a 2-level list" across follow-up bug-handling and transfer flows. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the document carries a live 2-level-list instruction near the backlog request, but subsequent bug-handling responses ignored that requirement and fell back to flat output without acknowledging the constraint boundary. The fix should carry active formatting requirements forward in the prompt contract or make the responder explicitly state when chat-level formatting rules prevent exact reproduction.
- [ ] [#c3ps] The html-style comments should have coloring. The contents of Elements `<!-- agent:exchange -->` should have a muted background color.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->
