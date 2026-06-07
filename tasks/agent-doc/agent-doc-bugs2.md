---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

Completed the editor/IPC node-patch phase of #md-ast-document-model: src/agent-doc a45121b0 applies node_patches through shared Rust markdown-AST mutations in VS Code and JetBrains, parent pointer 3b59c9320 is pushed, local verification/install passed, and GitHub CI 27097245198 is green. Remaining: realtime watcher convergence and replacing the legacy text CRDT merge base with structured overlay/AST state.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260607-152337.md`*

Compacted content:
- Archived 14 response topic(s): do [#recursion-guard-wedge-escape] -- gpt-5; do [#partial-staging-closeout-guard]; #codex-mcp-server-root-fix split; 11 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#md-ast-document-model] — gpt-5

Shipped the editor/IPC `node_patches` apply path for `#md-ast-document-model`.

Implemented shared Rust markdown-AST node patch application for insert/remove/replace/move/strike/unstrike by stable node key, preserving unrelated live-buffer items. Exported it as `agent_doc_apply_node_patches`, then wired VS Code and JetBrains to apply node patches before legacy component patches and skip legacy replacement for components already handled by node patches. Older native libraries now fall back to the legacy component path instead of throwing.

Verification passed: `make check`, focused AST/FFI/static-guard tests, VS Code `npm test`, JetBrains `./gradlew test buildPlugin`, `make install`, and GitHub CI `27097245198`. Commits: `src/agent-doc` `a45121b0`; parent submodule pointer `3b59c9320`.

Scheduled next steps under `#md-ast-document-model`: `#md-ast-realtime-watcher` and `#md-ast-crdt-merge-base`.
<!-- agent:boundary:9e6485af -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto priority preset="#spec-test-build-install-commit-push" go -->
- :round_pushpin: do [#md-ast-document-model]
- There are many mcp apis for agent-doc. Can we consolidate into a single mcp or is there a good reason to have many apis?
- Schedule #next-steps to finish [#md-ast-document-model].
<!-- /agent:queue -->
###
<!--
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
1. [ ] [#md-ast-document-model] #agent-doc-bug PARTIAL SHIPPED a45121b0: editor/IPC plugins now apply authoritative node_patches via shared Rust markdown-AST mutations, preserving unrelated live-buffer edits while skipping stale legacy component replacement for node-patched components. REMAINING: legacy text CRDT is still the merge base; realtime watcher convergence still needs the structured node-patch apply path. NEXT: #md-ast-realtime-watcher and #md-ast-crdt-merge-base. Plan: tasks/agent-doc/plan-md-ast-document-model.md
2. [ ] [#md-ast-realtime-watcher] #next-steps Finish #md-ast-document-model realtime watcher convergence for editor/IPC plugins against the structured node-patch document model. Plan: tasks/agent-doc/plan-md-ast-document-model.md
3. [ ] [#md-ast-crdt-merge-base] #next-steps Finish #md-ast-document-model by making structured overlay/AST state the authoritative merge base instead of legacy text CRDT. Plan: tasks/agent-doc/plan-md-ast-document-model.md
4. [ ] [#stale-busy-after-auto-inject-no-clear] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals a stale-busy actor over an idle pane (poll reconcile from has_busy_cue, ~2s debounce, persisted via mark_lifecycle). Re-listed to match the still-pending queue mirror; strike via the queue drain. Live-verify tracked as #stale-busy-self-heal-live-verify.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
