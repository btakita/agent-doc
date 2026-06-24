---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-174553.md`*

Compacted content:
- Archived 8 response topic(s): bug handling policy; bug policy config; JB `Run Agent Doc` still does not submit for Codex.; 5 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: editor state projection wiring — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#lzstatewire1]


Completed `#lzstatewire1`.

Wired the editor integrations to the Rust-owned state projection:

- JetBrains now binds `agent_doc_state_projection` and `agent_doc_record_state_event`, records route/transport/proof observations, renders a compact projection summary, and includes bridge tests.
- VS Code now has the same native projection/event bridge, records route and editor patch events, and includes matching tests.
- `specs/13-state-backbone.md` documents the editor projection bridge contract.
- `VERSIONS.md` notes the change, and the JetBrains plugin patch version is bumped to `0.2.188`.

Commits pushed:

- `src/agent-doc`: `6b105362` (`fix: wire editor state projections`)
- parent repo: `5ceac5053` (`agent-doc: update editor projection bridge`), followed by merge commit `370f156b3` to integrate `origin/main`

Verification run:

- `npm --prefix editors/vscode test` passed.
- `./gradlew test --tests com.github.btakita.agentdoc.StateProjectionBridgeTest` passed.
- `cargo test -p agent-doc state_projection_ffi_round_trip -- --nocapture` passed via `tsift digest-runner`.
- `make check` passed via `tsift digest-runner`.
- `make bump-plugin` passed and produced `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.188-signed.zip`.
- `make install-full` passed and installed the updated native library.

GitHub Actions note: `gh run list --workflow CI --limit 1` now reports no workflow named `CI` in the parent repo. The available `Agent Doc` workflow for push `370f156b3` failed before running any steps because GitHub reported account billing/spending-limit configuration issues.
- `make install-full` passed and installed the updated native library.
- `make bump-plugin` passed and produced `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.188-signed.zip`.
- `make check` passed via `tsift digest-runner`.
- `cargo test -p agent-doc state_projection_ffi_round_trip -- --nocapture` passed via `tsift digest-runner`.
- `./gradlew test --tests com.github.btakita.agentdoc.StateProjectionBridgeTest` passed.
- `npm --prefix editors/vscode test` passed.
- parent repo: `5ceac5053` (`agent-doc: update editor projection bridge`), followed by merge commit `370f156b3` to integrate `origin/main`
- `src/agent-doc`: `6b105362` (`fix: wire editor state projections`)
- `VERSIONS.md` notes the change, and the JetBrains plugin patch version is bumped to `0.2.188`.
- `specs/13-state-backbone.md` documents the editor projection bridge contract.
- VS Code now has the same native projection/event bridge, records route and editor patch events, and includes matching tests.
- JetBrains now binds `agent_doc_state_projection` and `agent_doc_record_state_event`, records route/transport/proof observations, renders a compact projection summary, and includes bridge tests.

### Re: JetBrains Codex Enter submit path — gpt-5

> **Queue prompt:**
>
> JB `Run Agent Doc` on Codex is STILL not submitting. Why is the text reliably being sent but sending Enter is so unreliable? Why not send the Enter with the text?

For Codex, the route layer already sends the trigger text and submit key together. The JetBrains action delegates to `agent-doc route --dispatch-only --plain-trigger`, and the binary’s shared live-pane helper uses the Codex submit profile `tmux_text_enter` with `submit_key=Enter`; when the profile has no split delay, it builds one `tmux send-keys` invocation containing both text and `Enter`.

The split text-then-Enter path exists for palette-style harness behavior, not Codex. The local comments call out OpenCode as the case where `/` can open a command palette and swallow an Enter sent in the same tmux call, so that path deliberately waits before sending Enter.

So if JB `Run Agent Doc` still leaves a typed-but-unsubmitted Codex trigger, the failure is not “JetBrains forgot to include Enter.” It means the route typed across a bad readiness boundary: stale supervisor, startup/recycle transition, active/blocked composer, or a proof path that accepted pane input without dispatch-start proof. The right fix is to keep tightening the binary route readiness/proof gates and surface the state projection, not to add a second Enter from the plugin. A second Enter is risky because it can submit the wrong future prompt after the composer catches up.

Relevant local checks already pin this contract: `simworld_jb_run_and_clear_share_codex_enter_submit_contract` expects Codex `tmux_text_enter`/`Enter`, and the dispatch-only direct-pane test asserts the route submits once without extra Enter retries.
<!-- agent:boundary:c49c71e7 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#ipcproofcloseout] [recommended] Investigate IPC proof closeout drift that appended patch_id=69d50165 as bare fenced text under an existing `(HEAD)` response instead of opening a clean exchange heading after interrupted finalize recovery.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
