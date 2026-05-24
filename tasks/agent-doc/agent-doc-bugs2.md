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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-210841.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ What are #next-steps to fix agent-doc bugs?
❯ JB `Compact Exchange` had a `File Cache Conflict` issue. #next-steps
### Re: next steps — gpt-5

Top backlog item id: `jbccache`.

I captured the JetBrains `Compact Exchange` `File Cache Conflict` track in the backlog. The priority order is reproduce/isolate, fix the write/cache boundary, add deterministic regression coverage, then update recovery docs.

Changed paths: session document only. Commands run: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, and `agent-doc finalize`. Verification: this response is closing through `finalize --stream`, including the session write/commit guard. Confidence: medium-high for the backlog ordering. Escalation: none.
### Re: jbccache — gpt-5

I isolated `#jbccache` to the JetBrains Compact Exchange action path and the post-0.33.16 write strategy.

The exact IDE path is: `CompactExchangeAction.actionPerformed` waits for typing idle, runs `FileDocumentManager.saveAllDocuments()`, then calls `TerminalUtil.compactExchange`; `TerminalUtil.buildCompactExchangeCommand` runs `agent-doc compact <relative file> --component exchange --commit` from the resolved project root.

The conflicting boundary is not `PatchWatcher`: current `compact.rs` intentionally bypasses editor IPC for template exchange compaction and calls `write::atomic_write_if_current_pub(..., "compact_exchange_direct_write")`. That performs the CLI-side idle/live-buffer/current-content CAS checks, then persists by atomic temp-file rename and updates the snapshot. This protects content from stale full-document replacement, but it is still an external disk mutation from JetBrains' perspective, so an already-open IDE buffer can surface IntelliJ's File Cache Conflict/reload path after the binary writes and commits.

The existing deterministic coverage already reproduces the important stale-cache shapes:

- `component_compact_uses_guarded_direct_write_when_patches_dir_exists` confirms Compact Exchange creates no IPC JSON patch even when `.agent-doc/patches` exists.
- `component_compact_direct_fallback_rejects_late_visible_edit` confirms the CAS guard rejects disk drift typed after compact planning.
- `component_compact_direct_fallback_rejects_idle_unsaved_editor_buffer` confirms live-buffer sidecar drift is rejected before disk write.
- `TerminalUtilTest` confirms the JetBrains action wires to `agent-doc compact ... --component exchange --commit`.

So `#jbccache` is reproduced and isolated as: JetBrains action -> external `agent-doc compact` process -> guarded direct disk rename -> JetBrains VFS/document cache observes an outside change. The next fix item should decide whether Compact Exchange needs an editor-mediated component replacement or an explicit post-compact VFS/document reload/refresh handshake after the direct write, without returning to unsafe full-document `fullContent` IPC.

Changed paths: session document and done archive only; no code changes. Commands run: `tsift status`, `tsift status --fix`, `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, targeted `tsift source-read` / `rg` inspection, `cargo test component_compact_direct_fallback_rejects_idle_unsaved_editor_buffer -- --nocapture`, `cargo test component_compact_uses_guarded_direct_write_when_patches_dir_exists -- --nocapture`, `cargo test component_compact_direct_fallback_rejects_late_visible_edit -- --nocapture`, `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.TerminalUtilTest`, `gh workflow list`, `gh run list --workflow "Agent Doc" --limit 1`, and `gh run view 26372808421 --json ...`. Verification: targeted Rust and JetBrains tests passed. Latest GitHub Actions workflow is `Agent Doc`, not `CI`; latest run `26372808421` failed before producing job steps/logs (`check` and `tmux-ci` both empty), so I classify that as an external CI-start/log-allocation blocker rather than a local tmux/code regression. Confidence: high for the isolated path and cache-conflict boundary; medium for the exact IDE dialog timing because this environment cannot display JetBrains' live file-cache UI. Escalation: none.
❯ The agent:queue stalled. #spec-test-build-install-commit-push
### Re: queue stall — gpt-5

Fixed the stalled `agent:queue` activation path.

Root cause: preflight was treating a newly reactivated queue as if it were already in-flight work. When the current document changed an inactive queue into an `auto` queue with a different head item, preflight compared that head against the stale inactive snapshot, halted with `item_modified`, stripped `auto`, and left inactive queue residue instead of dispatching the first queued item.

Changed paths:

- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/specs/02-document-format.md`
- `src/agent-doc/specs/07-orchestration-commands.md`
- `src/agent-doc/VERSIONS.md`
- parent gitlink: `src/agent-doc`

Behavior now:

- Head-item modification halts only apply when the queue was already active in the snapshot.
- A newly activated queue is treated as operator-authored input for the current cycle.
- Preflight snapshots the newly activated queue body and `queue_active: true`, so closeout can prove and consume the same head prompt.
- The old active-queue edit guard is still covered and still halts fail-closed.

Verification:

- `cargo test preflight_new_auto_queue_from_inactive_snapshot_does_not_halt_on_changed_head -- --nocapture`
- `cargo test preflight_halts_when_already_active_queue_head_changes -- --nocapture`
- `make check`
- `cargo install --path .`
- `agent-doc --version` -> `agent-doc 0.33.16`
- `git diff --check`

CI/tmux evidence:

- Latest pushed GitHub Actions run `26374290084` failed before runner steps were available: both `check` and `tmux-ci` jobs have empty `steps`, and `gh run view 26374290084 --log-failed` returned `log not found: 77631742498`.
- I classified that as an external CI-start/log-allocation blocker, not a code or tmux regression, and used the local full verification above as the actionable evidence.

Commits and pushes:

- Submodule commit pushed to `btakita/agent-doc` branch `20260422/codex-agent-backend`: `ec932657 fix: avoid stalling newly activated queues`
- Parent repo commit pushed to `btakita/agent-loop` branch `main`: `8fce4c220 agent-doc: update submodule for queue activation fix`

Escalation: none.

Confidence: high. The regression test recreates the stalled inactive-snapshot-to-auto-queue transition and proves the resulting head item can be consumed by closeout; the companion test preserves the fail-closed halt for an already active queue whose head was edited.
<!-- agent:boundary:48927d18 -->
<!-- /agent:exchange -->
###
<!--
JB `Restart Supervisor Process` error
```
agent-doc command failed (exit 1): Error: session_restart refused for /home/brian/work/btakita/agent-loop/tasks/software/tsift.md because pane %13 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 30% used"). Run `agent-doc session status /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.
```
---
JB `Run Agent Doc` on tsift.md error:
```
resync: stash pane %11 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 26.2s and typing indicator idle
[route] target tmux session: 0
[route] Pane %33 is in stash window 'stash', rescuing to agent-doc window
[route] Rescued pane %33 via join-pane
Error: authoritative actor generation 9 for tasks/software/tagpath.md owns pane %33 but route will not inject a new trigger because the authoritative actor is still starting (waited 8s for codex startup). Wait for the pane to show a dispatch-ready prompt (`prompt_ready=true`), then rerun `agent-doc tasks/software/tagpath.md`. If the pane stays stuck, restart the owner with `agent-doc start tasks/software/tagpath.md`.
```
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#jbccache]
- do [#jbccfix]
- do [#jbccreg]
- do [#jbccdocs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
