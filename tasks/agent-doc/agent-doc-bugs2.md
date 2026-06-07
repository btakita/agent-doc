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

Completed default-fast focus behavior. agent-doc focus now uses the former fast handoff by default; --blocking / --synchronous preserves the old synchronous promote-and-select path for explicit manual use. Source commit 2f2e9327, parent pointer c8f7fd750; local verification passed. CI run 27098734151 was in progress when checked, and I did not wait.
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

### Re: There are many mcp apis for agent-doc. Can we consolidate into a single mcp or is there a good reason to have many apis? — gpt-5

> **Queue prompt:**
>
> There are many mcp apis for agent-doc. Can we consolidate into a single mcp or is there a good reason to have many apis?


Recommendation: keep multiple narrow public MCP tools, but keep consolidating the implementation behind them.

A single catch-all MCP tool is tempting, but it would make the important lifecycle boundaries less explicit. `agent_doc_read`, `agent_doc_preflight`, `agent_doc_plan`, `agent_doc_session_check`, and `agent_doc_finalize` each have different safety semantics, required arguments, and failure modes. Separate tool schemas make it harder for an agent to accidentally run a mutating closeout when it only meant to inspect, and they make client/tool UX easier to validate.

The useful consolidation is internal: one server, one JSON-RPC handler, shared argument parsing, and shared `CommandOptions` plumbing. That is already mostly what `src/agent-doc/src/mcp.rs` does. I would avoid adding a separate MCP tool for every CLI subcommand; if the MCP surface grows, group by lifecycle stage or add a deliberately named low-level escape hatch rather than replacing the safe lifecycle tools with one generic dispatcher.

So: no broad single-tool collapse for the primary API. Keep a small, stable set of purpose-specific MCP tools, and consolidate code paths underneath them.

### Re: [#md-ast-realtime-node-events] — gpt-5

> **Queue prompt:**
>
> [#md-ast-realtime-node-events]


Closed this as an already-shipped duplicate of the archived `#md-ast-realtime-watcher` phase.

The realtime watcher path is present in `src/agent-doc` commit `1cc8e2ad` (`Add node-keyed watch events`): `agent-doc-markdown-ast` exposes `events::diff_node_events`, the watch daemon seeds per-document markdown snapshots, and file changes log `document_node_events` JSON batches keyed by component/node id with insert/remove/replace/move/strike/unstrike operations. The docs and release notes already describe this as the watcher handoff for realtime enqueue work.

Verification passed this cycle:

- `cargo test -p agent-doc-markdown-ast diff_node_events`
- `cargo test -p agent-doc-orchestration node_events`
- `cargo test -p agent-doc-orchestration update_node_snapshot_emits_node_keyed_events_after_seed`

No new source change was needed for this queue head. The remaining `#md-ast-document-model` work is the CRDT merge-base phase, tracked by `#md-ast-crdt-merge-base`.

### Re: [#md-ast-crdt-merge-base] — gpt-5

> **Queue prompt:**
>
> [#md-ast-crdt-merge-base]


Shipped the structured-overlay CRDT merge-base phase for `#md-ast-document-model`.

Implemented `snapshot::crdt_merge_base_state`, which derives the text CRDT merge base from the structured `.agent-doc/crdt/<hash>.overlay.yrs` sidecar when its markdown projection matches the active cycle baseline. If the overlay sidecar is absent, corrupt, or stale relative to that explicit baseline, the merge falls back to the baseline text and logs the reason. The shared write paths now use that helper instead of constructing merge bases directly from text at each call site, while still persisting the legacy `.yrs` state for older binaries and editor plugins.

Updated docs/spec/release notes to make the new authority boundary explicit: overlay projection is preferred for merge-base selection, baseline text is the guarded fallback.

Verification passed:

- `cargo test -p agent-doc-orchestration crdt_merge_base_state`
- `cargo test -p agent-doc-orchestration crdt_merge`
- `cargo test -p agent-doc-markdown-ast crdt`
- `make check`
- `make install`
- GitHub CI `27097931595` green, including `make check` and `make tmux-ci`

Commits: `src/agent-doc` `5fd64b26` (`Use overlay CRDT as merge base`), parent submodule pointer `960fab435`. With the earlier IPC node-patch and watcher work, `#md-ast-document-model` is now complete.

### Re: When I navigate to lazily-rs.md, the tmux pane switch was not responsive. We need this to be responsive. The main loop should defer any long-running operations to avoid blocking the UI. — gpt-5

Implemented in `src/agent-doc` commit `cbd8ca78` and pushed to `origin/main`.

The immediate editor focus handoff is now a short best-effort operation instead of an unbounded CLI wait:

- JetBrains `focus-fast` now runs `agent-doc focus <file> --no-stash-promote` through `SyncLayoutAction.runCommandWithTimeout(..., timeoutMs = 750)` instead of directly starting a process and waiting indefinitely.
- VS Code now applies the same 750ms timeout to immediate tab-change focus and to any focus-plan execution path.
- Over-budget or missing-pane focus attempts are logged/ignored, and the debounced guarded `sync --no-autostart` reconciliation remains responsible for slower rescue/provisioning work.
- The shared editor specs and release notes now document that immediate focus must stay bounded and non-authoritative.
- JetBrains plugin version is bumped to `0.2.152`; VS Code extension version is bumped to `0.2.22`.

Verification passed:

- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin`
- `npm test`
- `make check`
- GitHub Actions CI run `27098225139` passed (`make check` + `make tmux-ci`).

### Re: When I navigate to lazily-rs.md, the tmux pane switch was not responsive. We need this to be responsive. The main loop should defer any long-running operations to avoid blocking the UI. / `focus-fast` should be the `focus` behavior — gpt-5

> **Queue prompt:**
>
> `focus-fast` should be the `focus` behavior. So make `focus` have the same behavior as `focus-fast` and retire the blocking slow `focus` behavior. The idea of `focus` is to quickly switch to the next pane and let the rest of the UI operations to catch up in a concurrent or parallel task. The main thing is the tmux pane must switch fast. Is there is a good reason to keep the blocking `focus` behavior? If there is, add a `--blocking`, `--synchronous`, or another more appropriate attribute name to trigger this behavior


> **Queue prompt:**
>
> When I navigate to lazily-rs.md, the tmux pane switch was not responsive. We need this to be responsive. The main loop should defer any long-running operations to avoid blocking the UI.


This closes both focus queue heads, including the follow-up prompt: `focus-fast` should be the `focus` behavior. So make `focus` have the same behavior as `focus-fast` and retire the blocking slow `focus` behavior. The idea of `focus` is to quickly switch to the next pane and let the rest of the UI operations to catch up in a concurrent or parallel task. The main thing is the tmux pane must switch fast. Is there is a good reason to keep the blocking `focus` behavior? If there is, add a `--blocking`, `--synchronous`, or another more appropriate attribute name to trigger this behavior

Implemented and pushed.

`agent-doc focus` is now the fast/default handoff: it selects an already visible pane immediately, skips foreground stash promotion, and leaves stash surfacing to the follow-up `sync --no-autostart` reconciler. The old synchronous promote-and-select path is still available as `agent-doc focus <file> --blocking`; `--synchronous` is accepted as an alias. I kept that path because manual/operator flows may explicitly want a stashed pane surfaced before the command returns, but editor navigation should not pay that cost. The old `--no-stash-promote` flag remains accepted as a hidden compatibility no-op for older plugin builds.

Updated JetBrains and VS Code to call plain `agent-doc focus <file>` for immediate focus, renamed JetBrains diagnostics from `focus-fast` to `focus`, bumped JetBrains to `0.2.153`, bumped VS Code to `0.2.23`, and updated the focus/editor specs.

Verification:
- `cargo test -p agent-doc-orchestration decide_focus_pane`
- `target/debug/agent-doc focus /tmp/agent-doc-focus-missing.md --blocking`
- `target/debug/agent-doc focus /tmp/agent-doc-focus-missing.md --synchronous`
- `target/debug/agent-doc focus /tmp/agent-doc-focus-missing.md --no-stash-promote`
- `target/debug/agent-doc focus --help`
- `npm test` in `editors/vscode`
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin` in `editors/jetbrains`
- `make check`

Commits: `src/agent-doc` `2f2e9327` (`Make focus default to fast handoff`), parent submodule pointer `c8f7fd750` (`Update agent-doc submodule`). CI check: `agent-doc` CI run `27098734151` was `in_progress` when checked; per instruction, I did not wait for completion. The parent repo has no workflow named `CI`.

### Re: JB `Run Agent Doc` on monsterrodholders.md stalled

> **Queue prompt:**
>
> JB `Run Agent Doc` on monsterrodholders.md stalled. We need to make `agent-doc tasks/monsterrodholders.md` not follow a deadlock path and do the work. Even if this means the invocation messages the current owner to continue to the next task and that's all. If we can detect `Codex pane already owns that agent-doc session`, we can message the owning supervisor or agent-doc session.
> ```
> • agent-doc tasks/monsterrodholders.md refused to run because this Codex pane already owns that agent-doc session, so a
>   direct recursive invocation would deadlock.
>
>   agent-doc session status tasks/monsterrodholders.md reports the session is healthy and the actor is ready, with the
>   last event recorded as recursive_direct_invocation_blocked. No agent-doc cancel is needed.
>
>   To run it as a new managed cycle, start it from outside this owned pane, or use:
>
>   agent-doc start tasks/monsterrodholders.md
> ```


Implemented and pushed the `agent-doc` fix so the supervisor idle-queue watch no longer injects recursive `agent-doc <FILE>` into a Codex owner pane. Codex drains now inject an in-pane queue continuation prompt containing the active head and closeout instructions; Claude and OpenCode keep their normal harness triggers.

Added regression coverage for Codex owner-continuation payloads and non-Codex trigger payloads, updated the supervisor and route specs, and added the Unreleased note.

Commits: `src/agent-doc` `9b6f4635` (`Avoid Codex idle queue self-invocation`), parent `9520d9a31` (`Update agent-doc submodule`).

Verification: `cargo test -p agent-doc-orchestration idle_queue_drain` passed; `make check` passed after the final edit. CI was checked without waiting: latest `agent-doc` CI run `27099096431` was `in_progress`; the parent repo has no workflow named `CI`.
<!-- agent:boundary:362122f0 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto priority preset="#spec-test-build-install-commit-push" go=true -->
- monsterrodholders.md queue items that are completed don't have the response written into the agent:exchange element.
- Now, when I navigate to lazily-rs.md, 3 panes appear, with the 3rd pane being the lazily-rs.md pane. But then the lazily-rs.md pane disappears and we are still left with the wrong agent-doc-bugs2.md pane instead of the correct lazily-rs.md pane.
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
