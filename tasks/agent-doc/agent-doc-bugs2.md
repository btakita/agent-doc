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

Compacted. This exchange archived a long agent-doc bug triage and fix session on April 29, 2026.

Implemented and committed in this exchange:
- `#ckpb` clarified that `compact --commit` only commits the compacted document state, not any later console explanation.
- `#pdwg` moved pending-closeout gates ahead of document mutation in `finalize`.
- `#lvak` required fresh routed-cycle acknowledgment for prompt-bearing drift on top of committed cycles.
- `#ntwk` added explicit Codex network policy handling and fail-closed launch checks.
- `#ppct`, `#xfrr`, and `#nbla` hardened `#agent-doc-bug` prompt-preset contracts so backlog transfer counts and plan proofs are enforced by `preflight` / `finalize` / `session-check`.
- `#rtxf`, `#smdq`, `#tmuxcrash`, `#oobabs`, `#oobpmt`, and `#scpd` tightened routed tmux dispatch, startup-miss diagnostics, prompt absorption, and duplicate retry repair behavior.

Open backlog still active after compaction:
- `#smst` clear or supersede stale startup-miss markers after newer session ownership starts.
- `#mcrc` require commit follow-through after manual response-patchback repair.
- `#lvls` preserve explicit user formatting directives across follow-up bug handling and transfer flows.
- `#ptdr` / `#tdrl` fail closed or provide granular ops for destructive `patch:todo` replacements.
- `#c3ps` style HTML-like comments and mute the `agent:exchange` block background.

Unanswered user report preserved as open context:
- After `compact agent:exchange`, routed triggers still failed with `no new document cycle started` for pane `%422`, and the JetBrains plugin later reported multiple unregistered stash panes while rerouting to `tasks/monsterrodholders.md` pane `%408`.
- The user also asked why the drafted startup-miss diagnostic `echo` text appeared in the bound Codex pane and was not Entered; that path was later addressed by `#smdq`, which switched the diagnostic to a tmux display overlay.
<!-- agent:boundary:5eede6a4 -->
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
- [ ] [#sgdr] Nested submodule gitdir writes can still fail under Codex with a misleading `index.lock` error when the working tree is writable but the real gitdir under the superproject `.git/modules/...` tree is not. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: a commit inside `src/boost-client/src/monsterrodholders-dev` failed with `fatal: Unable to create '/home/brian/work/btakita/agent-loop/.git/modules/src/boost-client/modules/src/monsterrodholders-dev/index.lock': Read-only file system`. Plan: (1) reproduce and resolve the full writable-root set for nested submodules, (2) fix agent-doc/Codex launch + resume paths so those gitdirs are writable whenever commit/pointer updates are expected, and (3) add regression coverage for nested `.git/modules/...` commit flows. Working notes: `tasks/agent-doc/plan-submodule-gitdir-permission-issue.md`.
- [ ] [#smst] Clear or supersede stale startup-miss markers when a later session start takes ownership of the same document. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: `agent-doc session-check tasks/monsterrodholders.md` warned that startup-miss marker `%401` still existed, but `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` later recorded `session_start ... pane=%408` and `src/boost-client/.agent-doc/sessions.json` already registered `%408` as the live owner. The fix should prevent an older routed-trigger marker from surviving past a newer ownership start, or make `session-check`/`route` clear it as stale before surfacing a fake current crash.
- [ ] [#mcrc] Surface and require explicit follow-through when a missing response patchback is repaired manually in a session doc but not committed afterward. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the `#asch1` response was manually inserted and visible in the working tree, but `git status` still showed `M tasks/monsterrodholders.md` and the user had to ask why the patchback was not committed. The fix should either commit the manual repair as part of the recovery flow or emit a blocking notice that the recovery is still uncommitted.
- [ ] [#lvls] Preserve explicit user formatting directives such as "Please organize the backlog into a 2-level list" across follow-up bug-handling and transfer flows. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the document carries a live 2-level-list instruction near the backlog request, but subsequent bug-handling responses ignored that requirement and fell back to flat output without acknowledging the constraint boundary. The fix should carry active formatting requirements forward in the prompt contract or make the responder explicitly state when chat-level formatting rules prevent exact reproduction.
- [ ] [#ptdr] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc and Spec Refinement, and Expansion Phases (8 items lost). Fix: add granular --todo-done flag analogous to --pending-done, or add validation guard rejecting todo patches that reduce total item count.
- [ ] [#tdrl] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc & Spec Refinement, and Expansion Phases. The fix should either (a) add granular --todo-done flag analogous to --pending-done, (b) make patch:todo do a merge/diff when existing items are missing from the patch, or (c) add a validation guard that rejects todo patches reducing total item count.
- [ ] [#c3ps] The html-style comments should have coloring. The contents of Elements `<!-- agent:exchange -->` should have a muted background color.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->
