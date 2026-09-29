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
