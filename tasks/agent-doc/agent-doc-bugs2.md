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

<!-- normalization -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260430-114139.md`*

Active backlog:
- [ ] [#mcrc] Surface and require explicit follow-through when a missing response patchback is repaired manually in a session doc but not committed afterward. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the `#asch1` response was manually inserted and visible in the working tree, but `git status` still showed `M tasks/monsterrodholders.md` and the user had to ask why the patchback was not committed. The fix should either commit the manual repair as part of the recovery flow or emit a blocking notice that the recovery is still uncommitted.
- [ ] [#lvls] Preserve explicit user formatting directives such as "Please organize the backlog into a 2-level list" across follow-up bug-handling and transfer flows. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the document carries a live 2-level-list instruction near the backlog request, but subsequent bug-handling responses ignored that requirement and fell back to flat output without acknowledging the constraint boundary. The fix should carry active formatting requirements forward in the prompt contract or make the responder explicitly state when chat-level formatting rules prevent exact reproduction.
- [ ] [#splpend] finalize --stream with --pending-add drops pending mutations when sidecar normalization diverges. content_ours fallback lacks pending mutations because it is computed from baseline + patches, not from on-disk file. splice_pending_component() exists but is only called on IPC timeout path, not normalization divergence path. Repro: tasks/claudescore-3.md Apr 29, 2026 — 13 pending-add flags, all lost. Fix: call splice_pending_component(content_ours, on_disk_content) in normalization-divergence fallback branches. Plan: tasks/agent-doc/plan-sidecar-pending-mutation-loss.md
- 3 more active item(s)

Icebox:
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
<!-- agent:boundary:4169adf7 -->
<!-- /agent:exchange -->

<!--
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
- [ ] [#splpend] finalize --stream with --pending-add drops pending mutations when sidecar normalization diverges. content_ours fallback lacks pending mutations because it is computed from baseline + patches, not from on-disk file. splice_pending_component() exists but is only called on IPC timeout path, not normalization divergence path. Repro: tasks/claudescore-3.md Apr 29, 2026 — 13 pending-add flags, all lost. Fix: call splice_pending_component(content_ours, on_disk_content) in normalization-divergence fallback branches. Plan: tasks/agent-doc/plan-sidecar-pending-mutation-loss.md
- [ ] [#ptdr] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc and Spec Refinement, and Expansion Phases (8 items lost). Fix: add granular --todo-done flag analogous to --pending-done, or add validation guard rejecting todo patches that reduce total item count.
- [ ] [#tdrl] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc & Spec Refinement, and Expansion Phases. The fix should either (a) add granular --todo-done flag analogous to --pending-done, (b) make patch:todo do a merge/diff when existing items are missing from the patch, or (c) add a validation guard that rejects todo patches reducing total item count.
- [ ] [#c3ps] The html-style comments should have coloring. The contents of Elements `<!-- agent:exchange -->` should have a muted background color.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
<!-- /agent:icebox -->
