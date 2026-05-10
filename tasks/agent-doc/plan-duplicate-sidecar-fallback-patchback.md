# Plan: prevent duplicate response patchback after sidecar fallback

## Problem

The `#rspcmt2` closeout response was appended twice in
`tasks/agent-doc/agent-doc-bugs2.md`. The visible duplicate appeared after the
write path logged this sequence:

- socket IPC patch delivered
- CRDT merge successful
- sidecar normalization diverged
- fallback to `content_ours`
- full-content IPC re-delivery

That suggests the editor-side IPC patch and the fallback full-content repair can
both contribute the same exchange append, or the fallback snapshot can be built
from content that already includes one applied copy while the writer applies the
same patch again.

## Desired Behavior

1. A successful `finalize --stream` response appears exactly once in
   `agent:exchange`.
2. If sidecar normalization diverges after an IPC patch delivery, the fallback
   full-content payload must be a canonical complete document, not a second
   append on top of already-applied editor content.
3. The committed snapshot, working tree, CRDT state, and editor buffer converge
   to the same single-copy response.
4. `session-check` or the write path must fail closed if a response heading/body
   is duplicated during closeout.

## Investigation Steps

- [ ] Reproduce the `#rspcmt2` trace with a template + CRDT document, an active
      IPC listener, sidecar normalization divergence, and full-content
      re-delivery.
- [ ] Inspect the `try_ipc` fallback branches around
      `sidecar normalization diverged` and confirm whether `content_ours`,
      sidecar content, or live disk content already includes the response.
- [ ] Check whether the editor applies the initial component patch before the
      full-content fallback arrives, and whether the CLI-side snapshot then
      records a merged double-append.
- [ ] Compare this with existing duplicate-response detection in SimWorld and
      decide whether the production write path needs the same invariant before
      commit.

## Implementation Steps

- [ ] Add a regression that forces sidecar-normalization divergence after IPC
      patch delivery and asserts the final exchange contains one copy of the
      response heading/body.
- [ ] Make fallback full-content repair derive from a single authoritative
      complete document and re-run response deduplication before saving the
      snapshot or sending the full-content IPC payload.
- [ ] Add a post-merge/pre-commit duplicate-response guard for the active
      response block so the command fails closed instead of committing a double
      patchback.
- [ ] Ensure CRDT state and ack-content sidecars are refreshed from the same
      deduplicated final document.

## Regression Coverage

- [ ] Unit/integration test for IPC sidecar divergence with already-applied
      editor patch plus full-content fallback.
- [ ] Session-check or write-path test that rejects adjacent duplicate
      `### Re:` blocks with identical body in the active closeout.
- [ ] SimWorld seed promotion for the fallback sequence if the pure simulator can
      model editor-applied patch plus CLI full-content repair.

## Exit Criteria

- The `#rspcmt2` duplicate shape cannot be reproduced.
- Sidecar fallback closeout leaves exactly one response block in the document,
  snapshot, and committed `HEAD`.
- A future duplicate response patchback is caught before success is reported.
