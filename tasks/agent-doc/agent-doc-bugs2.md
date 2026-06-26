---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260626-015838.md`*

Compacted content:
- Archived 29 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); #lazilycrdtspec; #lazilycrdtspec; 26 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: lazily CellGraph reactive document substrate — opus-4-8

Your queue prompt — *use lazily Cell Family / CellGraph to reactively model the document and improve fine-grained sync* — is the per-cell CRDT-merge intent (`#qcellmerge1`), and it already has a sequenced plan: `tasks/software/plan-lazily-agent-doc-reactive-substrate.md`. Good news from checking the code — the keystone lazily-rs primitives the plan depends on **already exist**:

- **`CellTree<Id, V>`** (`src/lazily-rs/src/cell_tree.rs`) — the ordered keyed tree (Phase 1 `#lzordtree`): `(kind, stable_id, value_cell, ordered children)` with `insert_child` / `attach_child` / `children` / `resolve_path`. Per-node value edits invalidate only that node's readers.
- **`reconcile.rs` `apply_to_map`** — keyed reconciliation / per-item `DiffOp` application (Phase 2 `#lzkeyrecon`): the step that turns a structural edit into minimal per-item ops instead of a whole-subtree replace.
- **`CellFamily` / `CellMap`** (`src/lazily-rs/src/cell_family.rs`) — the reactive keyed collection underneath.

(There's no literal `CellGraph` type — that name maps to the `CellTree` + `CellFamily` reactive graph.)

**The real gap is the agent-doc side, not lazily-rs.** agent-doc still merges with the whole-document `crdt::merge`, so the doc isn't yet *projected* onto `CellTree` and a queue/backlog edit can't be reconciled to minimal per-item ops — exactly what produces the recurring cross-cell splices you keep hitting (this very cycle included). "Fine-grained sync" concretely = model `root → component → item` as a `CellTree` keyed on the **existing** `component:occurrence:item-id:index` anchors, run `reconcile` per level to emit `{insert, remove, move, update}`, and merge each item's value cell independently instead of merging the whole document.

**Why I'm not landing it in this drain cycle:** it replaces agent-doc's live merge path, which is supervisor-critical — the established safe path is an isolated worktree + focused cycle, not a live edit to the merge core mid-session-drain. The plan sequences the remaining phases (`#lzseqcrdt` move-aware order, `#lzstableid` manufactured identity, `#lzsemtree` memoized semantic tree, `#lztextcrdt` free-text CRDT) each as its own plan-first / test / commit unit.

**Concrete next step** (its own focused cycle): build the agent-doc `document → CellTree` projection keyed on the existing anchors + a `reconcile`-based per-item differ as an **additive** module (leaving `crdt::merge` in place behind it), with deterministic tests, in an `src/agent-doc-*-wt` worktree — no install/recycle/push. Say "go" and I'll kick that off in an isolated worktree.

### Re: CellTree per-cell merge — Phase 1 groundwork landed — opus-4-8

Built per your "go". This is the root fix for the `#qftloss` / `#qeditdup` / `#qcellmerge1` corruption class — and first, the diagnosis you should have: the supervisor is **not** stale. Pid 2967818 runs inode `95839507` = the installed binary (21:20) → `freshness=controller:fresh,supervisor:fresh`. So the IPC churn this session (duplicate-prompt, deferred writes, post-commit drift) is *this very whole-document-merge problem*, not a stale binary — which is exactly why fixing it is worth more than another live re-test.

**Landed** — isolated worktree `src/agent-doc-celltree-wt`, branch `celltree-percell-merge`, commit `6c225edd`. New **additive** module `agent-doc-core/src/cell_doc.rs` (636 lines). `crdt::merge` is untouched (only `pub(crate)` visibility widening so the new module reuses the existing splitters):

- `project_document(doc) -> Vec<ComponentOccurrence>` — projects the doc onto the existing `component:occurrence:item-id:index` keys (reuses `component::parse` + `split_list_children` / `split_exchange_children` — no new parser).
- `diff_document(old, new) -> Vec<ComponentDiff>` — lazily `reconcile` per component level → minimal `{Insert, Remove, Move, Update}` ops. **This is the per-cell merge enabler.**
- `CellDocTree` — reactive `CellTree`/`CellMap` representation; per-item value cells, so a single item edit invalidates only that item's reader.

**9 tests, all green, prove the anti-corruption properties:** reorder→`Move`s (not replace), insert/remove→single op, single-item edit→one `Update` with siblings untouched, and cross-component isolation (a queue edit yields **zero** backlog ops — the splice that keeps corrupting these sessions). `make check` green: **5320 passed**, clippy clean. No install / push / recycle — the live supervisor is undisturbed; branch is ready for your review/merge.

**Next phase (its own cycle):** the cutover. This is a 2-way diff; the live merge is 3-way — compose base→ours + base→theirs per-component diffs, resolve per-`node_key` conflicts (mirror `reconcile_component_body` in `crdt.rs`), then route `crdt::merge` through it behind a flag. Tracked in `tasks/software/plan-lazily-agent-doc-reactive-substrate.md`. Say "go" for the cutover and I'll take it in the same worktree.

### Re: CellTree cutover — flag-gated 3-way per-cell merge landed — opus-4-8

"go cutover" done. The lazily per-cell merge now has a **3-way** path wired into `crdt::merge_by_component` behind a **default-OFF** flag — safe, reversible, zero production behavior change until you flip it.

**Landed** — same worktree `src/agent-doc-celltree-wt`, branch `celltree-percell-merge`, commit `df0b330f`:

- `cell_doc::merge_3way(base, ours, theirs) -> CellMergeOutcome` — composes `reconcile(base→ours)` + `reconcile(base→theirs)` per `node_key` into a merged item list. Different items / different components → both land, no splice. **Same item edited on both sides → a real conflict surfaced in `CellMergeOutcome.conflicts` (ours-wins policy), never a fabricated blend** (the `#qnodemerge5` safety property).
- Flag-gated seam in `crdt.rs::merge_by_component` (L492) behind `AGENT_DOC_CELL_MERGE` (default OFF). Structural divergence → falls straight back to the legacy path. **Flag OFF ⇒ byte-identical to current behavior** (tested).
- 17 module tests (9 new): different-item / different-component isolation, the console-in-queue splice repro blocked, same-item→conflict, reorder+edit compose, structural fallback, flag-OFF byte-identical. `make check` green: **5328 passed**, clippy clean.

**Before the flag can flip in production** (honest gaps — all safe because the flag is OFF):
- The 3-way base is **text-only** — it decodes the Yrs base to text and discards CRDT structure / `MultiNodeState` op-clocks.
- The **`Live<Struck` anti-resurrection lifecycle** (`reconcile_list_item_lifecycle`) is **NOT yet mirrored** — a struck item could resurrect under the cell path. This is the #1 port needed before enabling.
- Op-capture replay not wired (touched sets inferred from text diff); conflicts only logged to stderr (no editor surface yet); merged text not yet persisted through the snapshot/CRDT path.

So: groundwork (`6c225edd`) + cutover (`df0b330f`) both on `celltree-percell-merge`, fully tested, **default-off and reversible** — two commits ready for your review/merge. To make it production-ready next: port the strike-lifecycle join + decide conflict UX, then a live zero-drift proof before flipping the default. Say the word for that rung.

### Re: Production-ready rung — Live<Struck anti-resurrection ported into cell merge — opus-4-8

Done. The two correctness gaps that gated flipping `AGENT_DOC_CELL_MERGE` are closed (commit `ec38a973`, branch `celltree-percell-merge`):

- **Anti-resurrection lifecycle ported** — `merge_3way` now routes each both-sides list item through the SAME `QueueItemLifecycle::classify` / `join` lattice (`Live < Struck`) that `crdt::reconcile_list_item_lifecycle` uses (reused, not re-derived). For queue/backlog/review/done, if either side is `Struck` the struck text wins — a stale un-strike from the persisted base can no longer resurrect a struck/deleted head. `exchange` is excluded (not strike-bearing).
- **Honest conflicts** — strike-vs-unstrike is a lawful lifecycle join (struck wins), **NOT** a conflict. Only a genuine same-level disagreement (both Live or both Struck, text differs vs base) is recorded in `CellMergeOutcome.conflicts` (now carries `ConflictKind` + structured per-conflict stderr log). The conflict list is the single honest source.
- 9 new tests (cell_doc 17→26): id-backed + free-text struck heads not resurrected by a stale un-strike (both directions), multi-cycle no-churn (persist-struck-as-base), backlog/review parity, strike-vs-unstrike-not-a-conflict, genuine-conflict-recorded. **Full `make check` green: 5337 passed.**

**Three commits now on `celltree-percell-merge`, all default-off + reversible:** `6c225edd` groundwork → `df0b330f` cutover (3-way + flag seam) → `ec38a973` production-ready (anti-resurrection + honest conflicts).

**What still blocks flipping the default to ON:**
1. **Persistence round-trip** — `merge_3way` is pure text→text; the seam must persist the merged `.yrs`/snapshot so the next cycle's base reflects the per-cell result. Multi-cycle convergence is proven at the fn level but not yet in the live snapshot path.
2. **Yrs op-clock / op-capture** — the cell path composes text revisions via keyed LIS diff, not Yrs operation clocks; genuine concurrent op histories need reconciling against the existing CRDT substrate.
3. **Live zero-drift proof** — flag ON, edit-during-finalize, `ops.log` zero drift — needs your live editor (can't be agent-done).

(1) and (2) are the next agent-doable rung; (3) is yours. Conflict editor surface stays nice-to-have (the list is honest; no UI consumes it yet). Say **"go persistence"** for the next rung.

### Re: Merged celltree-percell-merge into agent-doc main — opus-4-8

Merged (fast-forward) — agent-doc submodule `main` now at `ec38a973`, all three commits landed:

- `6c225edd` groundwork (projection + keyed diff)
- `df0b330f` cutover (3-way merge + flag seam)
- `ec38a973` production-ready (anti-resurrection + honest conflicts)

`cell_doc.rs` + the flag-gated `crdt.rs` seam are on main. **Runtime behavior is unchanged** — `AGENT_DOC_CELL_MERGE` defaults OFF, so the legacy merge path stays authoritative until you flip the flag. Release build of `agent-doc-core` verified green (13.9s), so the supervisor's idle-boundary auto-install won't stall on it.

**Not pushed, and the superproject pointer is not bumped** — both stay under your release/push gate ([[stop-cutting-releases]]). The agent-loop superproject now shows `src/agent-doc` as a modified gitlink (`16092173 → ec38a973`); bump + push it whenever you want it recorded. The worktree `src/agent-doc-celltree-wt` is left in place (`git worktree remove` it when done).

**Still open before the flag can default-ON:** the persistence round-trip + Yrs op-clock/op-capture (next agent rung — say **"go persistence"**), then your live zero-drift proof with the flag ON. Until then this is safe, dormant infrastructure on main.

### Re: Persistence round-trip rung landed — opus-4-8

"go persistence" done — commit `00b07925` on `celltree-percell-merge` (now 1 ahead of merged main).

**The gap it closed:** the cell seam lived only in `merge_by_component` (text path). The live persistence path merges through `MultiNodeState::merge`, which returns merged text **and** the per-node Yrs state persisted as next cycle's base. With the flag ON, `merge_by_component` returned cell-merged text but `MultiNodeState::merge` still persisted the **legacy** merge's state — so a conflict/strike could resolve one way in the doc and another in the persisted base → next-cycle drift.

**Fix:** routed `MultiNodeState::merge` through the same cell path via a shared `try_cell_merge_text` helper both seams call (so they can't diverge), rebuilding the persisted state from the cell-merged text via `from_text`. Confirmed against `snapshot.rs::save_document_crdt` — next cycle's base = `from_text(returned_text)`, so returned text, persisted state, and next base are all the one cell-merged text. Default-OFF preserved (flag-OFF byte-identical, tested).

**4 new tests:** conflict persists the cell winner (`state.to_text() == merged`), 3-cycle no-churn (strike stays struck across persisted bases, idempotent), flag-OFF parity, both seams produce identical text. Full `make check` green: **5341 passed**.

**Honest status — this is NOT the last rung.** Before the flag can default-ON:
1. **Yrs op-clock / op-capture (the real remaining code blocker)** — the cell path composes from a 3-way *text* diff then re-encodes deterministically; it doesn't replay genuine concurrent Yrs *op histories*. For true offline op streams (esp. interleaved char-level edits within one cell), the text-composition result can differ from an op-level merge. Reconciling the cell diff with real op capture is the next rung.
2. **Conflict editor surface** — conflicts resolve ours-wins silently (logged only); a losing concurrent edit needs an operator-visible marker before default-ON.
3. **Live zero-drift proof** — yours (live editor, flag ON, edit-during-finalize, `ops.log` zero drift).

Persistence is now round-trip-consistent for the text-composition model. The op-capture reconciliation (1) + conflict UX (2) are the remaining agent-doable rungs, then your live proof (3). Five commits on the branch, all default-off. Say **"go opcapture"** for the next one.
<!-- agent:boundary:365f4a5b -->
<!-- /agent:exchange -->

## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 go [#qftloss] [#qeditdup] [#crdtauth6] [#nsfz]
- Use lazily Cell Family, CellGraph, and other new features to reactively model the document. Use CellGraph to improve syncing documents on a fine grained level.
- :pushpin: do [#qftloss]
- :pushpin: do [#qeditdup]
- :pushpin: do [#crdtauth6]
- :pushpin: do [#nsfz]
- :pushpin: do [#qftloss]
- :pushpin: do [#qeditdup]
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
- [ ] [#qftloss] [operator-verify] Free-text prompt loss (mode-6) — CODE-COMPLETE on main (8b900bf4): binary now journals operator prompts from the plugin live-buffer sidecar (.agent-doc/live-buffer/<hash>) into the #qdurcrash store before any convergence/recovery can clobber the unsaved buffer, replayed if absent. Remaining: live IDE eyeball that an unsaved prompt typed during a concurrent finalize survives end-to-end. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#qeditdup] [operator-verify] Queue items duplicated + manually-struck/deleted items resurrected when the operator edits the queue during an active turn. #queuestatemachine idempotent convergence + the Live<Struck lattice (merged, 0.34.48) should resolve this — VERIFY on the recycled binary; if it persists live, a deeper editor-edit-during-cycle guard is needed. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#crdtauth6] [operator-verify] crdtauth5 live hookup: thin JB DocumentListener calling CrdtReplicaForwarder.forwardLocalDelta + a Kotlin Unix-socket ReplicaTransport client (writes NDJSON register/deregister/update, delivers inbound update_b64 to applyRemoteUpdate); plus the deferred VS Code koffi replica seam (editors/vscode/src/native.ts out-param marshaling). Rust fan-out + Kotlin seam already merged + tested. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#kj5f] [operator-verify] #mergestatemachine3 remaining: JB+VSCode thin reporters emit attach/detach/ownership facts through the merge-ownership SM (via #lazilystatesync) + live editor-attached/detached repro for File Cache Conflict + #6b5h wedge. DONE: SM core Phases 1-2 + deterministic scenario repros for both wedge classes (agent-doc 34d7023d). Remaining needs the operator-paused plugin install + a live editor to verify. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
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
