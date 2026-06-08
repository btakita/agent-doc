# Plan — Operation-scoped drift (turn-affectedness model)

**Backlog id:** `#op-scoped-drift`
**Origin:** agent-doc-bugs2.md queue prompt (2026-06-08) — "use the AST + document model to understand where changes occurred and who made them … a change in an area that does not affect the current turn … should just be added to the model + durable sqlite, but the turn should not be affected."

## Problem

The drift gate is **coarse**: any snapshot↔document (or live-buffer↔disk) divergence is treated as blocking/affecting the current turn. Independent edits — a new queue item, a comment, a backlog note, a foreign-supervisor write, a lagging editor sidecar — trip the same gate that genuine input/output conflicts do. This produces:
- false-positive finalize blocks ("visible editor buffer differs", exit 75),
- churn when the user edits the queue while a *different* item is running (monsterrodholders case),
- lost output when a concurrent writer contends the exchange boundary.

## Model

### Document substrate (HAVE — `semantic_diff`)
```
Document → Component{ kind, occurrence, turn_role } → Node{ node_key, item_id, span, content }
```
`turn_role` ∈ { input (queue, exchange-tail), output (exchange-append, backlog, review, status), inert (icebox, done, comments) }.

### Operation (formalize existing `node_events`)
```
Op = { target: Address, kind: insert|remove|edit|move|replace,
       actor: agent|user|foreign_supervisor|live_buffer,
       payload: ContentDelta, causal: { lamport, origin_session } }
Address = component + occurrence + (node_key | sub-node span)
```
`node_events` already emit `{component, node_key, op, item_id, after_index, prev/next_node_key}`. Add **actor** + **causal clock** + **sub-node edit** payload.

### TurnScope (NEW — the current operation manifest)
```
TurnScope = { read_set: Set<Address>, write_set: Set<Address>, driver: Address }
```
Derived at turn start (e.g. respond to `queue[ft-…]` → read {that queue item, exchange.tail}; write {exchange.append, that item's strike, backlog, status}).

### Affectedness relation (NEW — the classifier)
```
affects(O, S) = conflict(O.target, S.read_set) ∨ conflict(O.target, S.write_set)
conflict(a, b) = ∃x∈b : overlaps(a, x)   // same node_key, span intersection, or remove/move of a depended node
```
`¬affects(O,S)` → **Independent**: CRDT-apply into the document model + persist op to durable sqlite, do NOT touch the turn.

## Taxonomy of operations (effect class on the current turn)

1. **Independent / commutative** — target ∉ scope → silent integrate + persist; no drift. (new queue item beside a running one, comment/backlog/icebox edit)
2. **Input-affecting (read conflict)** — target ∈ read_set → reconcile: re-read input. (user edits the queue item being answered)
3. **Output-contended (write conflict)** — target ∈ write_set, concurrent writer → CRDT merge at write-back; never lose committed output. (two supervisors append exchange boundary) — partly HAVE: `crdt::merge` + `guard_committed_responses` (c9d276b9).
4. **Structural dependency** — remove/move of a node in scope → invalidate/adapt; don't recreate a deleted driver. (user deletes the running queue item)
5. **Provenance-spoofed (false drift)** — non-user actor misattributed as a user edit → classify by actor + causal ts; suppress. HAVE: live-buffer mtime check (51b9ed40).

Actor × class disambiguates 3 vs 5 (the recent false-positive bug).

## Gap → implementation

- HAVE: `semantic_diff.{changed_components,node_events}` (op extractor); `crdt::merge` (class 3); live-buffer mtime/actor suppression (class 5).
- BUILD:
  1. **Actor + causal tagging** on extracted ops (who + Lamport/session origin).
  2. **TurnScope manifest** emitted at turn start (read/write set + driver) — derive from preflight `prompt_targets` / plan `write_scope`.
  3. **Affectedness classifier** replacing the coarse drift gate: intersect incoming ops vs TurnScope; route to the 5 classes; only classes 2/3/4 affect the turn.
  4. **Durable op log** in sqlite (extend `agent_doc_write: crdt`): persist every op + actor + causal clock; independent ops land here without disturbing the turn.

## Phasing (each its own actionable backlog id at implementation time)

- `#op-scoped-drift-1` — actor + causal tagging on `node_events`; sqlite op-log schema + writer.
- `#op-scoped-drift-2` — TurnScope manifest (read/write set, driver) from preflight/plan.
- `#op-scoped-drift-3` — affectedness classifier + the 5-class router; replace coarse gate with scope-intersection; independent ops integrate+persist without affecting the turn.
- Spec + tests + build/install + commit/push per phase (carries the queue's `#spec-test-build-install-commit-push` preset).

## Verification

- Deterministic SimWorld: turn on `queue[A]` while inserting `queue[B]` → assert turn unaffected, op-B persisted.
- Negative: user edits `queue[A]` (the driver) mid-turn → assert classified input-affecting / structural, turn reconciles.
- Provenance: foreign-supervisor / stale live-buffer write → classified Independent/spoofed, no finalize block.
