# Stable queue prompt identity and relations

## Architecture contract

**Invariant:** Every executable `agent:run` / `agent:loop` queue node has one
persisted stable identity. An operator-authored semantic id is authoritative;
otherwise the binary admits exactly one canonical `[#q-<stable>]` id through
the editor-authoritative queue-maintenance CAS. Generated response metadata is
inert and never receives a queue id. `Related` edges are navigation metadata,
not ordering edges.

**Policy owner:** `agent-doc-queue::prompt_identity` owns the pure identity and
relation projection. `agent-doc-markdown-ast::overlay` owns the executable vs
reserved-inert classification. `agent-doc-preflight-io` only observes the live
authority, invokes the projection, and persists it through the existing
queue-maintenance compare-and-swap path.

**Transition table:**

| Input | Decision |
|---|---|
| Executable node with one authored id | Preserve it byte-for-byte. |
| Executable node with no id | Mint one deterministic `q-` id and insert it in the same node slot. |
| Executable node with a persisted generated id | Preserve it across moves, strikes, retries, compaction, claims, echoes, and queue sync. |
| Exact slash control such as `/clear` | Preserve exact command bytes; it is a supervisor/harness handoff, not prompt/work identity. |
| Generated Response / Choices / Next actions / Action / Related row | Keep inert and id-free. |
| Operator-authored child outside a reserved generated row | Treat as ordinary executable work and apply the identity rules. |
| Duplicate executable id | Diagnose; never silently reassign either item. |
| Relation to a known document-local id | Resolve a forward edge and computed backlink without editing the target row. |
| Duplicate, dangling, self, or cyclic relation | Diagnose; do not reinterpret it as `after=` or reorder work. |
| Legacy no-id document on a read-only path | Continue legacy text identity; offer authoritative migration on the next maintenance admission. |
| Concurrent editor advance during admission | Reobserve/rebase through the queue-maintenance CAS; do not mutate disk heuristically. |

**Evidence inputs:** The editor-authoritative document cut, resolved queue
component ids, overlay item nodes (including source slots and authored ids),
reserved generated-row classification, and parsed `Related` references.

**Reactive topology:** Editor/current-text Source -> queue document projection
Computed -> identity/relation projection Computed -> queue-maintenance CAS Effect
-> accepted/visible current-text receipt Source. The projection is a pure
one-shot derivation inside the existing document lifecycle; no polling or new
parallel state owner is introduced.

**Imperative extraction audit:** Identity extraction, generated-id selection,
relation resolution, backlink computation, and diagnostics live in one pure
projection instead of being recomputed by claim/strike/sync callers. Existing
claim and closeout APIs remain one-shot consumers because they operate on the
persisted id and hold no separate long-lived derived state.

**Allowed edit surfaces:** Queue Markdown overlay, the focused queue policy
crate, the preflight maintenance adapter, queue-facing specs/runbooks and their
installed mirrors, and focused regression tests. No live session document,
release, install, or unrelated controller surface changes.

**Verification:** Pure policy tests cover identity and relation decisions.
Maintenance/integration tests cover authoritative admission, CAS races,
concurrent editor edits, lifecycle preservation, same-text siblings, child
prompts, compaction, and legacy migration. Run focused crate/integration suites
and workspace clippy; do not run the full `make check`.

**Out of scope:** Cross-document relations, relation-driven scheduling,
external graph persistence, and a second identity sidecar/state store.

**Git trailer dependency:** The current commit path has only the specialized
`Session-Doc:` sibling trailer and no generic affected-item-ID seam. This change
therefore does not invent a queue-specific trailer. A follow-up should add one
generic affected-ID collection/trailer contract and then feed admitted or
consumed queue IDs into it.
