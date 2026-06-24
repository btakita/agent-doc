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
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-161232.md`*

Compacted content:
❯ - Archived 2 response topic(s): IPC proof issue dogfood log; IPC proof diagnostics and resurrected queue head
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: bug handling policy — gpt-5

> **Queue prompt:** This dogfood telemetry is good for my development environment. My goal is to distribute agent-doc to people who are not dogfooding the product. Should we add a configuration parameter to select if bugs should be directly fixed, added to gh issues, or just loggging?

Yes, but I would frame it as an action-routing policy, not just a bug toggle. Dogfood sessions can keep the aggressive direct-fix loop; general installs should default to bounded logging or explicit issue capture, with autonomous fixes only when the project opts in.

A good shape is:

- `agent_doc_bug_policy = "log" | "issue" | "fix"`
- Default distributed behavior: `log`, or `issue` only when GitHub repo/auth/dedupe are configured.
- Project/session override through `.agent-doc/config.toml` and frontmatter.
- Always capture bounded evidence plus a stable fingerprint first; then route the action according to policy.

That separation matters. Telemetry collection should be deterministic and low-surprise for every user, while actions that mutate repos or file external issues should be explicit policy choices. I added a recommended backlog item for this so the config, defaults, dedupe behavior, docs, and tests can be worked as a deliberate feature.

### Re: bug policy config — gpt-5

> **Queue prompt:** [#bugpolicy1]: In agent-loop, set the config to fix.

Set the local agent-loop project config to dogfood/fix mode by adding this top-level key to `.agent-doc/config.toml`:

```toml
agent_doc_bug_policy = "fix"
```

Verification: the file now contains the key, and a probe preflight completed without a config parse warning while keeping `#bugpolicy1` as the active queue head for this closeout. Git does not track `.agent-doc/config.toml` in this workspace, so there was no separate config commit to make; the session document closeout records the completed queue item.
Handled the cycle.

I set `.agent-doc/config.toml` to include:

```toml
agent_doc_bug_policy = "fix"
```

Then I finalized [agent-doc-bugs2.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md), marked `#bugpolicy1` done, and committed the session closeout at `aef5face6`. `agent-doc session-check` is OK and the queue is empty. The config file is local/untracked by Git in this workspace, so there was no separate config commit to make.

### Re: JB `Run Agent Doc` still does not submit for Codex. — gpt-5

> **Queue prompt:** JB `Run Agent Doc` still does not submit for Codex.

Fixed in `src/agent-doc` commit `d4219137` and included the parent submodule pointer in this closeout. The route path was still allowing dispatch-only JetBrains reroutes to treat Codex direct-pane text+Enter acceptance as success even when Codex hook tracking was visible, which meant Run Agent Doc could report `route_success` without hook-backed evidence that Codex actually started the submitted prompt.

Changed `agent-doc-orchestration/src/route/dispatch_only.rs` so dispatch-only proof policy is document-aware: Codex with visible `.codex/hooks.json` now waits for dispatch-start proof and fails closed with the existing route-dispatch bug report when only pane-input acceptance is observed. Codex installs without visible hooks keep the acceptance-only fallback. Updated coverage now locks both branches.

Verification:
- `cargo test -p agent-doc-orchestration dispatch_only_codex -- --nocapture`
- `cargo test -p agent-doc-orchestration accepted_only_dispatch_start_proof -- --nocapture`
- `cargo test -p agent-doc-orchestration dispatch_only_proof_policy -- --nocapture`
- `cargo test -p agent-doc-orchestration dispatch_only_direct_pane_accepted_submit_skips_optional_proof_wait -- --nocapture`
- `cargo test -p agent-doc-orchestration dispatch_only_submit_proof_gate_accepts_enter_delivery_without_codex_hooks -- --nocapture`
- `make check` via `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`
- `gh run list --workflow CI --limit 1` showed latest `CI` run on `main` completed successfully.

### Re: JB `Clear Session Context` on opencode does not get past the dropdown selection submission. — gpt-5

> **Queue prompt:** JB `Clear Session Context` on opencode does not get past the dropdown selection submission.

Fixed in `src/agent-doc` commit `07a2d446`; the parent repo now points at that submodule commit. The OpenCode clear path sends `/new`, but OpenCode can stop at the slash-command dropdown selection by replacing the draft text with `New session` or `session_new`; the old verifier treated that as no visible pending command and falsely accepted before sending the follow-up Enter.

Changed paths:
- `src/agent-doc/src/session_actor_cmd.rs`
- `src/agent-doc/agent-doc-orchestration/src/start/detection.rs`

The CLI-side clear verifier and the supervisor pending-payload detector now treat `New session` and `session_new` as still-pending `/new` submissions. That keeps the existing OpenCode split-submit retry path active, so it sends the second Enter instead of declaring success while the dropdown selection is still sitting in the composer. Added coverage for both selected forms.

Verification:
- `cargo test -p agent-doc session_actor_cmd::tests::clear_command_visible_detects_opencode_selected_new_session_command -- --nocapture`
- `cargo test -p agent-doc-orchestration start::detection::tests::supervisor_pending_payload_detects_opencode_selected_new_session_command -- --nocapture`
- `cargo test -p agent-doc clear_submit -- --nocapture`
- `cargo test -p agent-doc clear_command_visible -- --nocapture`
- `cargo test -p agent-doc-orchestration supervisor_pending_payload -- --nocapture`
- `make check` via `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`
- `make install`
- `gh run list --workflow CI --limit 1` showed latest `CI` on `main` completed successfully.

Supervisor refresh: `make install` updated the local binary and library. An immediate `agent-doc session restart-supervisor` refused because this pane is the active running turn, so I ran `agent-doc admin recycle`; the controller is marked to recycle at the next idle boundary without interrupting the live turn.

Confidence: high for the reported dropdown-selection stall, because both direct clear verification paths now fail closed while OpenCode shows the selected `/new` command state and the full local check suite passed. No force restart or destructive recovery was used.

### Re: FFI IPC and lazily state consistency — gpt-5

> **Queue prompt:** Look at the FFI IPC and lazily-kt/lazily-js. Will this help keep state consistent between the editor plugin, FFI plugin, and the CPC?

Yes, with one constraint: it helps only if Rust/CPC stays authoritative and the editor plugins remain thin event reporters plus projection renderers. It should not become a second Kotlin/TypeScript state machine.

The pieces already point that way:

- `src/lazily-rs/src/ipc.rs` defines a transport-neutral snapshot/delta protocol and explicitly keeps live Rust contexts, references, closures, and typed handles out of FFI payloads.
- `src/lazily-rs/src/ffi.rs` exposes opaque channel handles and canonical JSON/binary `IpcMessage` bytes; it is an ABI/transport adapter, not a second graph model.
- `src/agent-doc/src/ffi.rs` already has `agent_doc_state_projection` and `agent_doc_record_state_event` backed by a Rust-owned `EventLedger`.
- `src/agent-doc/agent-doc-orchestration/src/state_backbone.rs` already projects document, queue, closeout, transport, supervisor, route, and proof state, with owner-generation checks that reject stale editor/route events.
- `src/lazily-kt` and `src/lazily-js` already expose matching `StateProjectionClient` wrappers for projection reads and event reports.
- The JetBrains and VSCode native bridges already expose controller-backed repair/admin wrappers, so drift repair can go through the CPC boundary instead of local editor guesses.

Practical answer: yes, this is the right direction for keeping the editor plugin, FFI layer, and CPC consistent. The first useful increment is Phase 1 projection wiring: editor plugins read the binary-owned `DocumentStateProjection`, report observed facts as `StateEvent`s, and render UI/status from that projection. Use lazily IPC snapshot/delta transport later only if measured full-projection polling is too expensive.

I added a recommended backlog item for that wiring. No source code changed in this turn.

Changed paths: session document only.  
Verification: inspected the lazily IPC/FFI surfaces, agent-doc state backbone/FFI/controller repair paths, and Kotlin/JS projection clients; no tests run because this was an architecture answer with no code edits.  
Confidence: high for the architecture direction; medium for implementation effort because the editor plugins still need actual projection-read wiring.  
Escalation: none.

### Re: queue closeout repair — gpt-5

Repair closeout: the FFI IPC prompt was already answered in the prior committed section. I consumed that free-text queue head, preserved the remaining `do [#lzstatewire1]` queue item, and left the newly added equityfundingsource route-stall note in the queue for a later cycle.
<!-- agent:boundary:15c32825 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- Look at the FFI IPC and lazily-kt/lazily-js. Will this help keep state consistent between the editor plugin, FFI plugin, and the CPC?
- do [#lzstatewire1]
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
- [ ] [#lzstatewire1] [recommended] Wire agent-doc editor integrations to consume binary-owned state projections: expose/read `agent_doc_state_projection` in JetBrains and VSCode native bindings, emit `StateEvent`s for editor IPC/CPC observations, and render route/transport/proof state from `DocumentStateProjection` instead of local heuristics.
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
