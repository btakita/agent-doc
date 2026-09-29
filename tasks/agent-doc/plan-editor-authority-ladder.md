# Plan: editor authority ladder (editor state is the source of truth)

## Why

`src/haiven-dev/tasks/api.md`, 2026-09-29 15:35. The operator typed ~468
characters. The controller acknowledged the typed state (`projectState
content_hash=90a06be…`), but quarantined the update pending a lazy canonical
projection (`crdt_replica_update_quarantined recovery=lazy_canonical_projection`)
and lost it across an install handoff. When the editor re-registered after the
plugin reload, it **adopted** the stale CRDT text over its own buffer:
`registration retained the controller canonical projection`. The operator's text
was gone. The editor was the source of truth and was rolled back.

Every "adopt canonical" outcome carries this failure class. A canonical copy that
lacks editor text looks the same whether the controller lost the text (a
regression) or the agent deleted it (for example, a queue strike). No
text-comparison rule can separate the two.

## Design (operator-specified)

Canonical state, in order of preference:

1. **Two or more editors open:** the reconciliation of their states.
2. **One editor open:** its buffer.
3. **No editor open:** disk.

The in-memory CRDT is a merge engine and copy of that authority, never an
authority over an open editor:

- Editors only **merge forward**. They receive operations (other editors', the
  agent's) and never a whole-text replacement.
- **Registration** publishes the editor state into the CRDT and then merges
  forward. It never adopts the CRDT over the editor.
- A **controller handoff or restart** reseeds the CRDT from the current rung
  (the open editors' states, else disk), never from an older checkpoint, and it
  folds quarantined editor updates before any handoff or checkpoint.

## Conflict reconciliation (operator-specified, to formalize next)

When the agent's incoming merge and the operator's live edit meet:

1. **Both appending at the same point:** put the agent's merged content *before*
   the operator's in-progress append, and keep the cursor at the end of the
   operator's edit.
2. **Operator editing existing text while the agent appends:** apply the agent's
   append; the regions are independent.
3. **Both editing the same existing text:** a true conflict. It must be resolved,
   never silently dropped on either side. The operator's text stays in the buffer
   (editor truth), and the agent's version is surfaced **in the buffer** as a
   compact conflict for the agent (or operator) to reconcile next.

   **Conflict rendering (operator decision 2026-09-29):** "can we have an inline
   conflict resolution text for this if the conflict is short? Multi-line
   conflicts are overly verbose...and long lines with a small conflict are
   difficult to reconcile." Implemented as a pure, lossless renderer in
   `agent-doc-merge/src/conflict_render.rs`:

   - **Inline** when the differing lines correspond one-to-one and each change is
     short (≤ 80 chars per side, ≤ 4 changes per line). Only the differing words
     are marked, in CriticMarkup, reading as the agent's proposed edit to your
     text: `{~~yours~>agent~~}` substitutes, `{++agent++}` adds, `{--yours--}`
     removes. Adjacent word changes separated only by whitespace merge into one
     substitution. Example: `pending a {~~lazy~>canonical~~} projection`.
   - **Block** otherwise, but only around the lines that differ, never the
     unchanged lines around them: `<<<<<<< yours` / `=======` / `>>>>>>> agent`.
     A side without a final newline carries `\ No newline at end of file`.
   - `resolve_conflicts(text, Keep::Yours | Keep::Agent)` returns either side
     byte-for-byte; `has_conflicts` detects both shapes. Rendering refuses input
     that already contains marker syntax, since it could not round-trip.

## Formal artifacts

- `formal/tla/EditorAuthorityLadder.tla`: bounded model check. The positive
  config holds `NoRollback` (37,054 states). `Wedge` (adopt-canonical plus
  reseed-from-disk) violates it, reproducing api.md. `Reach` shows agent writes
  still reach editors.
- `formal/authority_ladder/EditorAuthorityLadder.lean`: **proof for every
  instance**, any editors and any content:
  - `no_rollback`: every step preserves an open editor's undeleted text.
  - `handoff_never_regresses`, `canonical_reconciles_all_open_editors`,
    `canonical_is_disk_when_none_open`, `deleted_monotone`.
  - `adopt_can_roll_back`: non-vacuity counterexample for the shipped
    registration.
- `formal/authority_ladder/ConflictReconciliation.lean` (`#editorauth1`, done
  2026-09-29): content is a sequence, an edit replaces `[pos, pos+len)` of the
  shared base, and the operator has a cursor. For every base and every pair of
  edits:
  - `same_point_agent_first`: rule 1, agent content before the operator's
    append, cursor at the end of the operator's edit.
  - `agent_before_applies_both`, `operator_before_applies_both`: rule 2.
  - `conflict_resolves_to_either_side`, `conflict_is_surfaced`: rule 3, never
    dropped, and resolving reproduces either edit exactly.
  - `resolve_render`, `render_compact`: the rendering is lossless and keeps
    shared text outside the marks (the Lean `Seg.conflict` is the node that
    `conflict_render.rs` writes as inline CriticMarkup or a block).
  - `operator_text_never_lost`, `agent_insert_preserved`: every case.
  - Non-vacuity: `operator_first_violates_rule_one`, `conflict_reachable`.
- `formal/tla/ConflictReconciliation.tla`: the same merge over every pair of
  edits of a 3-atom base plus the operator typing on at the cursor (11,368
  states). `OperatorFirstWedge` violates `SamePointAgentFirst`, `DropWedge`
  (last writer wins) violates `AgentPreserved`, `Reach` shows conflicts are
  surfaced.
- The model does not choose between inline and block notation: that choice is
  presentation, and `conflict_render.rs` tests its round trip separately.
  `#editorauth4` must implement `merge` from the Lean model in `agent-doc-merge`.

## `#editorauth4` (done 2026-09-29, agent-doc f62cbd933)

- `agent-doc-merge/src/conflict_reconcile.rs`: `reconcile(base, yours, agent,
  cursor)` diffs each side against the base into edits and applies the Lean
  rules; overlapping edits group transitively into whole-line regions rendered
  by `render_conflict`. Identical edits apply once (a replayed delivery); the
  Lean `merge` and the TLA+ spec gained the same first case
  (`identical_edits_apply_once`, `IdenticalAppliesOnce`).
- FFI `agent_doc_reconcile_text` (JSON result, UTF-16 cursor), bound as
  `reconcileText` in JetBrains `NativeLib.kt` and VS Code `native.ts`.
- Conflict gates use `introduces_conflicts(merged, inputs)`: marks the merge
  added, counted only when well formed. A bare `contains("<<<<<<<")` refused
  every document that quotes the notation; the bare-token `has_conflicts`
  would flag `i++}` or a Gemfile `~> 1.2`.
- Not yet on a live path: the replica exchange is yrs. `#editorauth2`
  (registration merges forward) and `#editorauth3` (no whole-text replacement
  into a registered editor) are where `reconcileText` gets called.

## `#editorauth3` (done 2026-09-29)

Root cause of the api.md loss, on the controller side: a registration that
retained the controller projection queued a bootstrap *receipt*, and
`ensure_canonical_projection_receipt` set the same flag that quarantines a
member's updates. The editor had adopted that bootstrap, so its typing was in
the relay's lineage, yet every keystroke was dropped
(`crdt_replica_update_quarantined`) until the receipt ACK. A handoff before
the ACK lost the text for good.

- `agent-doc-document-realtime` `RelayHub`: new `replica_updates_fenced`
  source map, separate from `canonical_projection_required`. A receipt
  obligation no longer fences updates. Only an unproven lineage
  (`require_canonical_projection`: a first-contact incremental frame, a retained
  frontier against an unestablished relay, a quarantined document-op author, a
  structural/graph rejection) fences them. Both clear on the receipt ACK and on
  deregister.
- `agent-doc-crdt-relay-io`: `relay_holds_only_disk_rung` (no established
  projection, no live member, canonical equals disk; or an empty reseed
  fence). An unregistered editor's update against such a relay is
  `RequestEditorReseed`: nothing is registered or projected over the editor, an
  `editor_replica_reregister` is signalled
  (`crdt_replica_update_editor_reseed_requested`), and the re-registration with
  a retained frontier resets that relay to the empty reseed fence
  (`crdt_replica_register_retained_reseed recovery=cold_relay_editor_authority_ladder`),
  so the editor's state reseeds it. This is ladder rung 2 over rung 3. A relay
  whose canonical differs from disk may hold an agent write that isn't on disk,
  so it keeps authority and the old fenced full bootstrap applies.
- Tests: `relay_update_after_handoff_reseeds_from_the_editor_not_disk` (api.md
  replay; it replaced the test that asserted the rollback),
  `typing_relays_while_a_bootstrap_receipt_is_outstanding`,
  `relay_ahead_of_disk_keeps_authority_over_a_retained_registration`,
  `cold_start_decision_prefers_the_editor_only_over_the_disk_rung`. Mutating
  each fix back fails its test.
- Still open: two or more editors on a relay that already holds one editor.
  An unregistered editor there still gets the fenced canonical projection,
  because a foreign-lineage delta cannot be merged without its text. Reconciling
  it through `reconcileText` needs the plugin to publish its buffer on
  registration (`#editorauth2`).

## Phases

1. `#editorauth1`: Lean + TLA model of the conflict-reconciliation rules above.
   Content becomes a sequence with the operator's cursor. Prove: operator text
   never lost; agent inserts preserved; same-point appends ordered
   agent-before-operator with the cursor at the operator's end;
   same-span conflicts surfaced, not dropped, in the compact rendering above
   (resolving either side reproduces it exactly).
2. `#editorauth2`: plugin registration. Remove every adopt-canonical outcome
   (`ApplyCanonical` over a buffer that differs) and replace it with
   publish-editor-state then merge forward. Regression test: replay of the api.md
   sequence.
3. `#editorauth3`: controller. No whole-text canonical replacement into a
   registered editor. Fold quarantined editor updates before handoff/checkpoint.
   Reseed from the ladder on handoff/restart.
4. `#editorauth4`: implement the conflict-reconciliation merge from phase 1 in
   the native merge (`agent-doc-merge`), shared by JetBrains and VS Code through
   FFI. Same-span conflicts render through `conflict_render::render_conflict`,
   and every conflict detector that today checks only `<<<<<<<` (for example
   `agent-doc-document-realtime/src/write_policy.rs`,
   `agent-doc-write-converge-io`) switches to `conflict_render::has_conflicts`
   so inline markers are never committed as resolved text.
