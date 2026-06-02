# Plan: route/preflight sibling-document commit sweep commits docs the actor does not own

**Pending:** `#xdoc-route-sweep-commits-sibling-docs`
**Status:** Planned + root-caused 2026-06-02. Not yet implemented.
**Class:** `#agent-doc-bug` — cross-document contamination (`#jb-tsift-pane-sync` family).

## Repro / observed (2026-06-02, agent-loop)

While a Claude session owned `agent-doc-bugs2.md` on pane `%70` (authoritative
actor generation 221), a **separate** `lazily-rs.md` Codex session (pane `%73`,
gpt-5.5 xhigh, supervisor `2341978`) was concurrently:

1. Running its own auto-queue (lazily-rs integration phases `#lr-head-8/9/10` =
   `RunContext` slots in `agent-doc-orchestration`).
2. Editing the **shared `src/agent-doc` submodule** (`graph.rs`, `ops_log.rs`,
   `template_io.rs`, `run.rs`, `stream.rs`, `template.rs`, `write.rs`,
   `preflight.rs`, `src/notify.rs`, `src/patch.rs`) — landed as `c85fe72f`.
3. **Committing `agent-doc-bugs2.md` cycles it did not own.** `bugs2.md` received
   6+ closeout commits (17:15–17:22) during the `%70` session that `%70` never
   authored; the `#manual-queue-head-loss` item was struck + reaped by a
   different actor than the one implementing it.

The `%70` owner's `finalize` then failed (`pending edit: no item with id` — the
item had been reaped out from under it), its baseline went stale, and it had to
HOLD. Classic "concurrent owner serviced the same doc prompt" stall.

## Root cause (two facets)

**A. The sibling-document commit sweep is owner-agnostic.** `preflight` (and the
route-owned reap) sweep every *open* session document in the project and commit
its drift, regardless of which pane/session is the document's authoritative
actor. Evidence — a single actor's pass in `.agent-doc/logs/ops.log`:

```
route_owned_reap_decision policy=auto ... active codex turn
commit_staging file=.../tasks/software/lazily-rs.md ...
commit_staging file=.../tasks/software/tagpath.md ...
commit_staging file=.../tasks/software/tsift.md ...
```

and my own `%70` preflight emitting `[preflight] sweep: clean .../tsift.md` /
`.../tagpath.md`. So the `lazily-rs` actor's preflight/closeout committed and
advanced `bugs2.md` (and siblings) — authoring cycles on a document owned by a
different live actor.

**B. Shared work-item duplication across documents.** `#lr-head-8`, `#lr-wire-9`,
`#lr-actor-10` live in **both** `agent-doc-bugs2.md`'s backlog **and**
`lazily-rs.md`, both pointing at `tasks/agent-doc/plans/lazily-rs-integration.md`
and the same `src/agent-doc` files. Two sessions can pick up the same work and
race the same submodule, so the sweep (facet A) is not just bookkeeping churn —
it bundles a sibling's shared-submodule edits into another doc's closeout.

## Desired behavior

1. The sibling-document commit sweep must only commit / advance a document whose
   **authoritative actor is the current pane/session**. For a foreign-owned
   document with drift, record a diagnostic and leave it for its owner; never
   author its cycle.
2. Route-owned reap must not closeout-commit documents it does not own.
3. A sibling session's shared-submodule edits must not be bundled into another
   document's closeout commit (commit ownership stays with the owning actor's
   turn — relates to `#submodule-response-commit-proof`).
4. (Optional, facet B) Duplicate-ownership guard: when the same `#id` is open in
   two documents' backlogs targeting the same plan/files, warn so the operator
   collapses it to a single owner.

## Implementation sketch

- `preflight.rs` sweep step + the route-owned reap path: before `commit_staging`
  a *sibling* document, gate on `is_authoritative_owner(current_session/pane,
  doc)` (registry `pane`/session + live-pane evidence). Skip + `ops_log` a
  `foreign_owned_sweep_skip` diagnostic for foreign-owned docs with drift.
- Keep the current-document commit path unchanged.
- `session-check`: surface a cross-document-authorship diagnostic when HEAD shows
  a commit authored by a non-owning session between the owner's preflight and
  finalize.

## Tests

- Two registered docs owned by different panes (A=`%70`, B=`%73`). Actor A's
  preflight sweep with B dirty must NOT commit B; assert B stays dirty and a
  `foreign_owned_sweep_skip` diagnostic is logged.
- Same-owner sweep still commits the actor's own sibling docs (no regression to
  legitimate single-owner multi-doc batching).
- Duplicate-`#id`-across-docs warning fixture (facet B), warn-only.

## Acceptance

- A session can no longer commit / strike / reap another live-owned document's
  cycle via the sweep.
- Foreign-owned drift is reported, not silently committed.
- The `bugs2.md` + `lazily-rs.md` shared-`#lr-*` repro no longer produces
  competing authorship on `bugs2.md`.
