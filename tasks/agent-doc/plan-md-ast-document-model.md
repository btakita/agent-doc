# Plan: `#md-ast-document-model` — structured AST + CRDT document model

**Status:** design / dependency decision (this is the root-cause fix for the queue/dedup/live-buffer data-loss family).

## Problem

agent-doc mutates the session document as **text**. Every data-loss bug this session traces to that:

- **queue-consume** strikes "the leading free-text *line*" → struck the wrong item (`File Cache Conflict`) when the answered item was a different one, and errored (`queue consume: snapshot head prompts ... do not match document head prompts`) when the sorted-snapshot head disagreed with the document head.
- **dedup** (`dedup_live_prompts`) collapses same-*text* lines → destroys intentional duplicates (`#queue-dedup-destroys-intentional-duplicates`).
- **IPC** ships *text* patches that fail on live-buffer drift (`live_prompt_drift_after_preflight`) → stuck `response_captured`/`preflight_started` cycles, forcing `reset --from-current` recovery.

None of these operations know *which item* they act on — only what the text looks like — so a concurrent edit makes them act on the wrong node.

## Fix

Model the document as a **structured AST + structured CRDT**: queue items, backlog items, exchange turns are **typed nodes with stable ids**, not lines.

- `consume` strikes the dispatched node *by id* (the "struck the wrong head" bug becomes structurally impossible).
- `dedup` compares node identity/provenance, not text.
- `reorder` / `enqueue` are node ops; preflight detects reorder as id-permutation, not a diff.
- **IPC** patches become `{node-id, operation}` instead of text matches → drift-resilient (the failure that wedged finalize twice this session).
- yrs CRDT already exists (`crdt.rs`) but operates on **text**; the gap is a **structured schema** (yrs `Map`/`Array` of typed item nodes), not text in a yrs `Text`.

## Dependency decision (answers the queue question)

**Question:** Should tsift be a hard dependency for agent-doc, or extract a separate crate for the markdown AST?

**Decision: extract a separate, focused crate. Do NOT make tsift a hard dependency of agent-doc.**

Rationale:
1. **Weight.** tsift is a 20-crate workspace (tree-sitter for 7+ languages, vector search, graph DB, embeddings, sim-world). agent-doc is a lean, sequential, per-cycle hot path ("No async — sequential per-run"). A hard tsift dep bloats build, binary, and startup for every agent-doc cycle.
2. **No clean unit exists today.** tsift's markdown AST is `tree-sitter-md` embedded in `tsift-graph` (heavy) behind a `lang-markdown` feature, alongside the code-graph substrate — there is no standalone `tsift-markdown-ast` to depend on.
3. **Layering.** tsift is the heavier *analysis* tool used to navigate agent-doc's code; making agent-doc depend on tsift inverts that and risks build cycles in the agent-loop superproject (both are sibling submodules sharing `../.cargo/config.toml`).
4. **Narrow need.** agent-doc needs: parse markdown blocks + the `<!-- agent:name -->` component overlay into typed nodes with stable ids, backed by CRDT. That is a focused crate, not all of tsift.

**Shape:** a small `markdown-ast` crate (its own publishable crate / agent-loop workspace member), `tree-sitter-md`-based to match tsift's parser choice so node semantics align.
- agent-doc depends on it for the document model.
- tsift migrates `tsift-graph`'s markdown path to depend on it too (or layers its richer graph on top) — giving the *shared, consistent* AST the user wants, which is what makes an IPC-shared model coherent across both tools.
- Keeps agent-doc's deps to `tree-sitter-md` + the small crate, not the tsift stack.

## Phases (one backlog id per actionable phase)

1. **`#md-ast-crate-extract`** — scaffold the `markdown-ast` crate: `tree-sitter-md` block parse → typed block tree (heading, list, list_item, fenced_code, html_comment) with byte spans + stable ids. Pure, unit-tested, no agent-doc coupling.
2. **`#md-ast-component-overlay`** — layer the `<!-- agent:name -->` component model on the block tree: queue/backlog/review/exchange as typed item collections; each item a node with a stable id (existing `[#hash]` ids reused; free-text items get a content-hash id with provenance).
3. **`#md-ast-crdt-schema`** — back the overlay with a structured yrs CRDT (`Map`/`Array` of item nodes) replacing text-in-`Text`; migration path from the current `.yrs` text state.
4. **`#md-ast-mutations-node-keyed`** — reimplement consume / dedup / reorder / enqueue as node-id operations; delete the text-line heuristics. Subsumes `#queue-dedup-destroys-intentional-duplicates` and the wrong-head consume bug.
5. **`#md-ast-ipc-node-patches`** — IPC patches become `{node-id, op}`; drift-resilient apply. Subsumes the `live_prompt_drift_after_preflight` failure class and overlaps `#ipc-listener-dewedge-direct-disk`.
6. **`#md-ast-realtime-watcher`** — document watcher emitting node-keyed CRDT events; unblocks `#queue-enqueue-action` realtime enqueue.

## Quick-win stopgap (independent of the rewrite): `#queue-consume-pushpin-normalization`

Observed 3× this session, blocking `finalize` every time:

```
Error: queue consume: snapshot head prompts ["do [#md-ast-document-model]. ..."]
do not match document head prompts [":pushpin: do [#md-ast-document-model]. ..."]
```

The queue-consume head-equality check compares the **sorted snapshot head** against the **document head** without normalizing the `:pushpin:` / `:round_pushpin:` annotation prefix (and without normalizing sort order). The *same item* differs only by the `:pushpin:` prefix, so the equality check errors out and the cycle cannot commit (forcing `reset --from-current` recovery). An earlier variant instead **struck the wrong head**.

Cheap fix in `queue.rs`: normalize the annotation prefix + sort order on **both** sides before the head-equality comparison in queue consume. This is the text-keyed-mutation symptom that `#md-ast-document-model` removes structurally (node-id equality), but the normalization is a low-risk stopgap that should land first so the queue is committable in the meantime.

## Bugs this subsumes / overlaps

`#queue-dedup-destroys-intentional-duplicates`, `#monsterrod-queue-items-deleted-live`, `#ipc-listener-dewedge-direct-disk`, `#ipc-typing-duplication-family`, the queue-consume wrong-head/head-mismatch failures observed this session, and (via the watcher) `#queue-enqueue-action`.
