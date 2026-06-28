# Plan: Operator-vs-agent-doc provenance authority

**Operator directive (2026-06-26):** "Can we properly distinguish if text was from
an operator action or from agent-doc? All operator actions should never be reset
or treated as suspicious."

## Problem

agent-doc cannot reliably distinguish **operator-authored** text from
**binary/agent-doc-authored** text. It *infers* provenance from:

- snapshot ↔ working-tree diffing (conflates operator + binary edits in one window),
- the IPC source of a writeback,
- content heuristics — `content_ours` (editor content wins on drift),
  "looks like a manual cleanup", and the stale-editor resurrection guards.

Because provenance is *inferred from content*, not recorded as a durable per-edit
fact, three failure modes recur (all observed live in agent-doc-bugs2.md):

1. **Operator queue deletions get resurrected.** The backlog→queue mirror
   (`sync_backlog_into_queue`, Append/Sync) re-adds any active `queue`-attr backlog
   id that is *absent* from the queue. Absence is ambiguous — "never added" vs
   "operator deleted it" — so an operator delete is undone next preflight.
   ("I deleted items from the queue but they reappeared.")
2. **Operator manual edits risk being treated as suspicious / reset.** A manual
   edit (e.g. the operator grouping ids onto one line) can trip content-cleanup
   heuristics.
3. **Agent content splices into operator components.** Whole-document
   `crdt::merge` can merge agent console/response text into `agent:queue`
   (#qcellmerge live proof: a console line appeared as a queue item).

## Principle

Operator actions are **authoritative**. Operator-origin text is **never** reset,
**never** resurrected-after-delete, **never** flagged suspicious. Provenance is a
recorded per-edit fact, not a content guess.

## Design

1. **CRDT transaction-origin tagging.** Tag every yrs transaction in
   `crdt.rs`/`crdt_sync.rs` with an origin:
   - `Operator` — editor DocumentListener delta forwarded over IPC
     (`CrdtReplicaForwarder.forwardLocalDelta`, the #crdtauth5/#crdtauth6 seam),
   - `Binary` — finalize/write/preflight writeback,
   - `Mirror` — backlog→queue auto-sync.
   Persist origin alongside each delta in the replica store so any text span's
   author is recoverable after restart.
2. **Per-span / per-cell provenance surface.** Riding the #qcellmerge per-cell
   merge isolation, each cell/queue-item carries its last-author origin; expose to
   preflight / sync / recovery.
3. **Invariant enforcement at the two mis-classification sites:**
   a. **Recovery** (`content_ours` / manual-cleanup / resurrection guards): when a
      working-tree change is `Operator`-origin, it IS the authoritative prompt —
      never reset, never "manual cleanup". Replace content heuristics with origin
      checks where origin is known; fall back to content heuristics only when it
      is not.
   b. **Mirror** (`sync_backlog_into_queue`): an `Operator`-origin delete of
      `do [#id]` records a session **tombstone**; Append/Sync/Prepend skip
      re-adding tombstoned ids until the operator re-adds the id or the backlog
      item materially changes. Also dedup against multi-id lines and existing
      entries so the auto-add never produces duplicates.
4. **Operator-delete tombstones** for queue items: a durable per-session set of
   operator-deleted ids, honored by every sync mode.

## Phases

- **Phase 1 (#provauth1)** — origin enum; tag Binary/Operator/Mirror transactions
  in crdt.rs/crdt_sync.rs; thread editor IPC delta origin from the #crdtauth seam.
- **Phase 2 (#provauth2)** — operator-delete tombstone set; `sync_backlog_into_queue`
  consults it + dedups (no duplicate adds). Directly closes "deleted items
  reappear" + "don't add duplicates". Smallest standalone win; can ship without
  full origin tagging by sourcing tombstones from snapshot-vs-working-tree
  operator deletes of `do [#id]` lines.
- **Phase 3 (#provauth3)** — replace `content_ours`/manual-cleanup/resurrection
  content-heuristics in recovery with origin checks; operator-origin spans
  authoritative.
- **Phase 4 (#provauth4)** — per-cell provenance (rides #qcellmerge) so agent
  content can never splice into operator cells.

## Backlog ids

Filed 2026-06-26 in agent-doc-bugs2.md (auto-ids — the `--pending-add 'id=…'`
prefix needs a trailing space, which was missed, so ids were generated; phase
labels below are logical names, the text of each item carries its `provauthN=`
tag):

- Phase 1 → `#f693`
- Phase 2 → `#q9w3`
- Phase 3 → `#fpyh`
- Phase 4 → `#818s`

## Relationship to existing work

- `plan-crdt-authority-model.md` (#crdtauth*) — provides the editor→binary delta
  transport this rides on (origin source).
- `project_agent_doc_percell_crdt_merge_intent` (#qcellmerge) — per-cell merge
  isolation; Phase 4 depends on it.
- `plan-queue-edit-race-consolidated.md` (#qeditdup) — the symptom plan; this is
  the provenance-level root fix.
