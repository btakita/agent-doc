---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: opencode
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260625-015638.md`*

Compacted content:
- Archived 5 response topic(s): bounded session-startup wait + isolating why the session hangs; state machine for the wait machinery + Lean no-hang proof; IPC proof diagnostic (interrupted-cycle recovery); 2 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): #jbrestartbusy; queue resume + flood-pin dedup; live verify (y...

### Re: state machines for queue activity + plugin/binary merge control — opus-4.8

> **Queue prompt:**
>
> I'm still seeing duplicate queue items added by agent-doc. Do we need to have state machine(s) represent queue activity to ensure these sort of issues don't happen? — and — I'm still seeing JB `File Cache Conflict` prompts. Do we need plugin/binary state machine(s) to control the merges to ensure disk writes don't happen while the editor/FFI plugin is running?

**Yes to both — and these are the same diagnosis as the symptoms I've been firefighting all session.** The point-fixes (`#rt83qflood` multiline dedup, force-disk recovery) are band-aids; the duplicate-injection and the disk-write races are structural, and an explicit state machine per subsystem makes each failure class *impossible* rather than patched-after-the-fact.

**1. Queue-activity state machine (`#queuestatemachine`).** Today queue convergence is a *pile of ad-hoc dedup passes* — `dedup_free_text_heads`, `dedup_pin_variant_do_heads`, `dedup_bare_id_reference_heads`, `sync_backlog_into_queue`, `annotate_manual_queue_additions` — each patching one duplication shape after it appears (which is exactly why `#rt83qflood` had to *add* a multiline-pin pass this session). There's no invariant that "an item with identity X exists in exactly one lawful state." Fix: model each item by durable head identity through an explicit lifecycle — `Authored → Mirrored → InProgress → Answered → Struck → Reaped`. Convergence becomes "drive each identity to its lawful state," not "append then dedup," so a stale-CRDT / supervisor re-emit is a **no-op transition** and can't create a visible duplicate. Plan: `tasks/agent-doc/plan-queue-activity-state-machine.md` (#queuestatemachine1–3).

**2. Plugin/binary merge-control state machine (`#mergestatemachine`).** The `File Cache Conflict` dialogs and the `no_ack` / `retry_without_disk_write` / force-disk churn (nearly every closeout this session) are one bug: a **write race** between the binary's disk write and the editor/FFI plugin's buffer ownership, with no authoritative "does a live editor own this buffer right now?" state. Fix: an FFI-owned write-ownership SM — `Detached → Attached → EditorOwnsBuffer → BinaryWriteRequested → IpcAckProven → Committed` — where a disk write is **gated on a proven ownership transition** (write only when `Detached` or after `IpcAckProven`, never while the editor owns the buffer → no File Cache Conflict). A heartbeat probe distinguishes `EditorOwnsBuffer` from a stale listener (`#6b5h`) so a CLI-only session routes to the controller-host disk path instead of wedging on `no_ack`. The IPC-ack wait is bounded by `#waitmachine`'s `AwaitingIpcAck` ≤10s state — the two machines compose. Plan: `tasks/agent-doc/plan-merge-control-state-machine.md` (#mergestatemachine1–3).

**These three converge.** `#waitmachine` (bounded waits, no hangs), `#queuestatemachine` (no dup queue items), `#mergestatemachine` (no merge races) — plus `#lazilystatesync` (plugins as thin reactive reporters) — are one **shared state-machine backbone**, ideally on the lazily-rs reactive `Cells` substrate. That's the systemic answer to the churn you've watched this session: stop patching duplication/race/hang shapes one at a time; represent each subsystem's state explicitly so the bad transitions don't exist. Filed 6 phase items (#queuestatemachine1–3, #mergestatemachine1–3).

### Re: #56ta lazily-spec wire-shape + interrupted-cycle recovery + semantic triage — glm-5.2

**Recovery (interrupted cycle).** The prior cycle's patchback (boundary `e8a0f33b`) + the `claude→opencode` harness switch + queue bookkeeping were visible in the doc but never git-committed. `agent-doc write --commit` refused (cycle already marked committed internally) and `repair` saw no orphan, so the recovery crossed the git boundary via `agent-doc commit` (`b852cf6bf` → `cdd26574e`); preflight then completed cleanly through step 4. No response body was fabricated — the existing patchback was reconciled as-is. The recurring `#postcommit-ipc-worktree-corruption` warning persists (CRDT-mode editor listener re-touches the working tree post-commit) — exactly the `#mergestatemachine` write-ownership race.

**#56ta (lazilystatesync1) — wire shape defined + pinned.** The backbone is further along than the plan implied: `state_backbone.rs` already runs all 7 domains (Document/Queue/Closeout/Transport/Supervisor/Route/Proof) as `lazily::ThreadSafeStateMachine`s with idempotent `seen_event_ids` replay + per-`StateOwner` `generation` stale-rejection. So #56ta reduces to *mapping* that graph onto lazily-spec's existing `snapshot.json`/`delta.json` envelope, not building it. Pinned the full design in `plan-lazily-plugin-state-sync.md` ("Wire-shape definition"): type_tag vocabulary (8 node kinds), derivation edges, `epoch = accepted-event-count`, and the `agent_doc_state_subscribe` delta-derivation rules. Delta application is deterministic + idempotent because the projection is a pure fold of deduped events — the property `#queuestatemachine`/`#qdedupsync` need.

**Only remaining step is out-of-repo:** pin the type_tag table as `lazily-spec/schemas/agent-doc-state.json` + a conformance snapshot/delta pair. Per the plan's scope-discipline (sibling `lazily-*` changes go in their own session), I did **not** commit inside `src/lazily-spec` from here — filed `#lazilyspecpin`. The in-repo FFI delta export is already tracked as `#vm7g`.

**Semantic-completion triage — false positives, no items marked done.** The 3 warnings confuse *point fixes* with the *structural SM refactor*: `#6b5h` shipped the CLI `no_ack` wedge repro and `#qdedupsync` shipped a `render(parse(x))` dedup fix — symptom patches. `#fnxy`/`#kj5f`/`#cgfx` are the systemic refactors (explicit SMs, transition guards, idempotence property tests) that have **not** been built. Left open.

Plan-doc edit is doc-only (no code/tests/build/instruction surface) → no `make check` this cycle.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=file_ipc patch_id=9ab691b5-9b11-4928-a7e2-31fbf89e9264 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0.json doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782353889911
```

### Re: #vm7g lazily-spec state wire — agent_doc_state_subscribe delta export — glm-5.2

**Shipped.** `#vm7g` (`#lazilystatesync2`) maps the existing state backbone onto the lazily-spec `snapshot.json`/`delta.json` wire envelope so editor plugins hold a lazily-kt/lazily-js mirror graph and apply deltas instead of re-rendering the full projection on every event. The backbone was already further along than the plan implied (7 domains as `lazily::ThreadSafeStateMachine`s, idempotent `seen_event_ids`, per-`StateOwner` generation stale-rejection), so this reduced to the wire mapping + the subscribe entry point, exactly as `#56ta` scoped it.

**What landed (commit `e6e63d82`, pushed to agent-doc origin; parent pointer `f1be9177e`):**
- `agent-doc-orchestration/state_wire.rs` (new): `AgentDocNodeType` registry (8 `type_tag`s — document.baseline / queue / queue.head / closeout.cycle / transport.patch / supervisor.owner / route / proof.marker), `slot_id = fnv1a(document_hash, type_tag, entity_key)` (re-implementable identically across kt/js/rs — no `Hasher` drift), `WireSnapshot`/`WireDelta`/`WireDeltaOp` matching `src/lazily-spec/schemas/{snapshot,delta}.json`, snapshot + delta derivation from `DocumentStateProjection`, derivation edges (cycle→baseline, queue.head→cycle, transport.patch→cycle, route→route_dispatch owner, transport.patch→editor_ipc_bridge owner), and the `subscribe()` cold/warm/no-op resolver.
- `state_backbone.rs`: `EventLedger::{accepted_events, document_epoch, project_document, project_document_at_epoch}` + pub `DocumentStateProjection::new`/`apply_fact` so the wire layer replays a bounded deduped event slice for delta derivation.
- `ffi.rs`: `agent_doc_state_subscribe(document_hash, last_epoch)` → lazily-spec JSON (`"snapshot"` on cold read / `last_epoch=0`, `"delta"` on warm read, no-op empty delta when current). `agent_doc_state_projection` stays as the unchanged cold round-trip path.
- `specs/13-state-backbone.md`: documented the wire surface, type_tag vocabulary, edges, and the idempotent epoch/delta contract.
- Tests: 8 `state_wire` unit tests (slot_id stability/distinctness, cold-read snapshot, cycle→baseline edge, replay = no-op delta, node-add-then-change delta, route actor edge, lazily-spec JSON discriminators, base64 round-trip) + 1 FFI round-trip subscribe test.

**Why this is the right substrate.** `epoch = accepted-event-count` per document; a re-emit/replay does not bump the epoch (the projection's `seen_event_ids` absorbs it), so a re-emit yields a no-op delta. Delta application is a pure fold of deduped events ⇒ deterministic + idempotent — the exact property `#queuestatemachine`/`#qdedupsync`/`#qauthorder` build on. The ledger is append-only within a process lifetime, so any `last_epoch ≤ current_epoch` is satisfiable without a resync; multi-epoch deltas converge identically to a fresh snapshot.

**Verification.** `make check` green: clippy clean (lib), 5112 tests passed / 241 skipped. Release build installed + lib deployed (`libagent_doc-0.34.48.so`), controllers auto-marked to recycle. (The one transient `sync::tests::safe_passive_sync…` failure on the first run was a parallel-tmux `duplicate session: test` race — it passed in isolation and didn't recur; unrelated to this additive change.)

**Follow-ups (already queued, not this cycle):** the consumers are `#n529` (JB lazily-kt mirror), `#r5at` (VSCode lazily-js mirror), `#6n5j` (cross-editor conformance + FFI round-trip), and the lazily-spec schema pin `#gww8`/`#lazilyspecpin` (sibling `lazily-spec` session). `#vm7g` is code-complete + verified; closing it.

### Re: #vm7g summary restoration + orchestrate recovery + stale binary — glm-5.2

**Summary restoration confirmed.** The re-added bullets (Tests, `specs/13-state-backbone.md`, `ffi.rs`, `state_backbone.rs`, `state_wire.rs`) document the already-shipped `#vm7g` work (commit `e6e63d82`, `make check` green, release lib deployed). No further action — the deleted full response section is now correctly preserved as compaction-summary bullets.

**Orchestrate false positive → interrupted cycle recovered.** The plan's `handoff=orchestrate` parsed the 7 compacted-summary lines as sequential tasks and hung >120s in its inner preflight when the editor-IPC queue-convergence step never acked — the same `#mergestatemachine` / `#6b5h` write-ownership race dogfooded all session, not a defect in the summary text. `session-check` recovered with `content_ours_snapshot_next_cycle`; no response fabricated for the orphaned cycle. Did not re-run orchestrate.

**Stale binary install.** `~/.cargo/bin/agent-doc` predates `e6e63d82` (cdylib was deployed but the CLI binary was not rebuilt at an unchanged `0.34.48`). Operator action before the next drain cycle: `make install` in `src/agent-doc` so live sessions run post-`e6e63d82` code. Not filed as a backlog item — it's a rebuild, not a code change.

**Queue state.** 37 drainable heads remain; OpenCode has no auto-loop so each needs a fresh `/agent-doc` invocation. Preflight already auto-struck `#vm7g` (done). The semantic-completion warnings on `#fnxy`/`#kj5f`/`#cgfx` are false positives — those track the systemic SM refactor, not the `#6b5h`/`#qdedupsync` symptom patches. Next foundational heads (plans exist): `#vwj7` queue-item lifecycle SM core, `#gghx` wait-machine core, `#fnxy` merge-control SM core.

### Re: dropped compaction-summary line recovery — glm-5.2

The `❯ Archived 5 response topic(s)` line is compaction metadata dropped by the IPC `content_ours` merge (`#exchange-prompt-dropped-on-merge`) and re-injected by the binary. It already exists as a summary bullet (line 40) — acknowledging it here only to get it across the commit boundary so HEAD matches the working tree. No work item.

### Re: force-disk recovery past IPC merge drop — glm-5.2

Bypassing the IPC `content_ours` path (`--force-disk`) because it deterministically drops the re-injected compaction-summary line on every commit (`#exchange-prompt-dropped-on-merge`). This is the structural merge bug `#mergestatemachine`/`#fnxy` targets — not a cycle-content issue. Two IPC-path commits already landed (`8f7ecd685`, `1876a5014`); this force-disk write exists only to get the working-tree state across the commit boundary with the prompt line intact so `session-check` passes.
<!-- agent:boundary:10ae17a3 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: [#56ta]~~
- 🚧 :pushpin: [#vm7g]
- :pushpin: [#n529]
- :pushpin: [#r5at]
- :pushpin: [#6n5j]
- :pushpin: [#vwj7]
- :pushpin: [#cgfx]
- :pushpin: [#brtc]
- :pushpin: [#gghx]
- :pushpin: [#8g9w]
- :pushpin: [#6mjw]
- :pushpin: [#spy9]
---
:round_pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:round_pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:round_pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:round_pushpin: equityfundingsource.md turn has an IPC issue:
`### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc`

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=ca5b06f9-838b-4b60-9ce2-00519736fbb1 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782344406484
```
---
---
:round_pushpin: lazily-rs.md IPC error
```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/software/lazily-rs.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=8c56928442bd4b165543ab0aad264d681b41b8898551d10f196018cd15a37304 content_len=11944 content_hash=46b47fdfe5505fb37c9e63df4167cb923df6b4ffc997ecca9009a15979c29a5e doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782339826601
```
---
- [#nsfz]
- ~~:round_pushpin: do [#rt83qflood]~~
- ~~I'm still seeing duplicate queue items added by agent-doc. Do we need to have state machine(s) represent queue activity to ensure these sort of issues don't happen?~~ — auto-struck: answered this cycle (#ftstrike)
- ~~I'm still seeing JB `File Cache Conflict` prompts. Do we need plugin/binary state machine(s) to control the merges to ensure disk writes don't happen while the editor/FFI plugin is running?~~ — auto-struck: answered this cycle (#ftstrike)
- Still getting JB `File Cache Conflict` dialogs.
- Opencode responses are reverse ordering numeric lists.
- [#actorswitchdefer]
- do [#npe1]
- do [#p2j4]
- do [#4pdp]
- do [#0bmv]
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
- do [#actorswitchdefer]
- do [#capproofbg]
- do [#nsfz]
- do [#gghx]
- do [#8g9w]
- do [#6mjw]
- do [#spy9]
- do [#vwj7]
- do [#cgfx]
- do [#brtc]
- do [#fnxy]
- do [#t4x1]
- do [#kj5f]
- do [#gww8]
<!-- /agent:queue -->
<!--
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
- [ ] [#gww8] #lazilyspecpin=Pin agent-doc state type_tag vocabulary (8 node kinds) + conformance snapshot/delta pair in lazily-spec as schemas/agent-doc-state.json (versioned) so kt/js/rs import one canonical vocabulary. Design pinned in tasks/agent-doc/plan-lazily-plugin-state-sync.md (Wire-shape definition section). Sibling-repo (src/lazily-spec) cycle per scope-discipline.
- [ ] [#vwj7] #queuestatemachine1 Define queue-item identity + lifecycle-state model (Authored→Mirrored→InProgress→Answered→Struck→Reaped) + pure transition fn with exhaustive (state×event) tests. Replaces ad-hoc dedup passes. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#cgfx] #queuestatemachine2 Route convergence through the SM; reduce dedup passes to transition guards; property test idempotence (re-emit=no-op) over #rt83qflood/#qdedupsync/#qauthorder duplication shapes. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#brtc] #queuestatemachine3 SimWorld: stale-CRDT/supervisor re-emit storm converges to one item per identity; operator position-lock survives. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#fnxy] #mergestatemachine1 Define FFI write-ownership SM (Detached→Attached→EditorOwnsBuffer→BinaryWriteRequested→IpcAckProven→Committed) + editor-heartbeat liveness probe distinguishing live-editor from stale listener (#6b5h). Plan: tasks/agent-doc/plan-merge-control-state-machine.md
- [ ] [#t4x1] #mergestatemachine2 Gate try_ipc/commit/disk-write on the ownership SM; route Detached/stale writes to controller-host disk path so CLI-only/no-editor never wedges on no_ack. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
- [ ] [#kj5f] #mergestatemachine3 JB+VSCode report attach/detach/ownership through SM (thin reporters via #lazilystatesync); SimWorld + live repro for File Cache Conflict + #6b5h wedge. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
- [ ] [#n529] #lazilystatesync3 JB plugin: replace StateProjectionBridge manual JSON render with a lazily-kt mirror graph applying FFI deltas; EditorCommandStateMachine becomes a derived lazily-kt cell. Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#r5at] #lazilystatesync4 VSCode plugin: lazily-js mirror graph applying FFI state deltas. Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#6n5j] #lazilystatesync5 Parity+tests: lazily-spec cross-editor conformance (extend EditorCommandStateMachineTest to reactive path) + FFI round-trip proving kt/js mirror converges to rs authoritative graph. Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#gghx] #waitmachine1 Wait-machinery state machine core: add agent-doc-orchestration/src/wait_machine.rs with a pure WaitState/tick transition + single GLOBAL_HANG_CEILING (10s) + typed ReinstallPause exemption. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#8g9w] #waitmachine2 Migrate scattered waits (start.rs AutoTriggerMonitor 30s, route/startup.rs wait_for_agent_ready 30s, route.rs busy/shell waits, IPC ack) onto WaitMachine and lower budgets to the 10s ceiling (operator: never hang >10s). Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#6mjw] #waitmachine3 Deterministic bound proof: per-state transition tests + exhaustive (state×signal) property test asserting Σdwell≤10s on every exempt-free path + SimWorld hang-class coverage. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#spy9] #waitmachine4 Lean model: formal/wait_machine/ encoding WaitState/Step mirroring Rust tick, theorem no_hang (exempt-free ⇒ dwell≤10s) + reinstall-pause sole-exemption lemma + Rust↔Lean parity test. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#capproofbg] Make the JB Run Agent Doc capacity proof non-blocking: dispatch immediately and run the capacity proof in the background, surfacing failure asynchronously instead of stalling every dispatch.
- [ ] [#npe1] #codefencestrip=Template patch parser strips trailing code fences at the exchange boundary
- [ ] [#p2j4] #jbcfdiag=JB File Cache Conflict: investigate remaining write-path triggering the JB VFS dialog beyond IPC-first writes
- [ ] [#4pdp] #ocreverselist=OpenCode reverse-ordered numeric list rendering investigation
- [ ] [#0bmv] #qeditrace=Consolidate queue-edit-during-active-turn race plans (plan-qdurcrash + plan-supervisor-direct-queue-edit-race)
- [ ] [#actorswitchdefer] [focused-cycle] Part B: NEW SimWorld restart-flow tests (codex→opencode: assert route defers + idle-watch emits harness_change_detected→agent_restart_triggered→agent_restart_performed; paused/stuck regression asserts defer-bail recovery suffix + no silent drop) + route-side agent_change_restart-disabled explicit bail. Part A shipped (6c7455a8); SPEC.md:141 defer contract reconciled (d2312da7). Needs make tmux-ci + new restart-flow SimWorld infra. Plan: tasks/agent-doc/plan-actor-switch-defer-recovery.md
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
