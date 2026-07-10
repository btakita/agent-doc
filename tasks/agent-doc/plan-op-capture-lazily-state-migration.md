# Plan: Migrate the op-capture sidecar onto the lazily state backbone

`#opcapturelazily` · folds into `#sidecarstate`

## What op-capture is

`.agent-doc/op-capture/<doc-hash>.json` (crate `agent-doc-op-capture-io`,
`#qnodemerge4`) is a **bespoke JSON sidecar source of truth**: it durably holds
the ordered `EditorOp`s the editor reported plus the `base_hash` of the buffer text
they were captured against. The CRDT merge (`merge::merge_contents_crdt_with_ops`
→ `merge_with_editor_ops`) and preflight repair **replay** those ops as authority
("replaying captured CRDT patchback"), then consume + clear them. It is
base-hash-guarded (a stale/advanced base disqualifies the ops → merge falls back to
the diff guess), so it is *safe*, but it is still a **separate replay source of
truth** living outside the lazily state backbone.

## Why it matters (this-turn evidence)

During the 2026-07-10 stuck-cycle churn on `agent-doc-bugs2.md`, clearing
`.agent-doc/captures/` + `.agent-doc/state/cycles/` aside did **not** stop preflight
repair from re-replaying — because op-capture is a *distinct* sidecar that also
drove the CRDT-patchback replay. Recovery had to hunt and clear a third location by
hand. The root refusal was `#stale-lease-cpc-authority` (fixed in 0.34.75), but the
churn showed op-capture is one more uncoordinated sidecar authority, exactly what
`#sidecarstate` targets.

## Migration

1. Model the captured editor-op epoch (ops + `base_hash` + epoch counter) as a
   lazily state-backbone fact for the document, co-located with the cycle phase
   state (`agent-doc-cycle-state-io` already emits `state_backbone_response_captured`
   / `state_backbone_*`), so capture authority and cycle authority are one
   reconcilable model rather than two JSON files.
2. Keep the base-hash guard semantics exactly: `editor_ops_for_base` returns ops
   only when the stored base hash matches the supplied base; recording against a
   new base starts a fresh epoch; consume clears the epoch. These invariants move
   onto the backbone unchanged.
3. Make recovery/clear **atomic**: clearing a stranded cycle clears the captured
   op epoch in the same operation (no third hand-cleared location).
4. Preserve the append-only-within-epoch discipline and the never-silently-drop-an-op
   contract.

## Surface / risk

96 references across `agent-doc-op-capture-io` + ~9 crates (`agent-doc-merge-io`,
`agent-doc-write-runtime-io` run_entry/ipc, `agent-doc-run-io`, `agent-doc-stream-io`,
`agent-doc-codex-stop-io`, `src/op_capture_verify.rs`, `src/commands.rs`, …). This is
a `#sidecarstate`-class refactor on the live merge/replay path — its own focused
cycle with SimWorld coverage (base-match replay, stale-base fallback, epoch reset,
atomic clear) and a live editor-attached proof. Do not bundle with unrelated hot-path
changes.

## Related

- `#sidecarstate` (parent: retire hot-path snapshot/cycle/live-buffer sidecars for the
  lazily state backbone). Plan: tasks/agent-doc/plan-lazily-state-backbone-sidecar-retirement.md
- `#stale-lease-cpc-authority` (0.34.75) — the refusal root cause that first exposed
  the multi-sidecar replay churn.
