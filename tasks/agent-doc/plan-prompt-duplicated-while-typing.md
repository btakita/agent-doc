---
agent_doc_session: plan-prompt-duplicated-while-typing
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
---

# Plan: user prompt duplicated in editor while typing

## Symptom

User report (2026-05-29): *"As I was typing in the prompt, the prompt was duplicated."*

While the user is typing a new prompt into the live editor buffer (IntelliJ via the
agent-doc JB plugin), the in-progress prompt text appears **twice** in the document.
The duplication is observed in the editor buffer during/after typing, before the user
ever invokes a new `agent-doc` cycle. (Distinct from `#duplicate-response-after-commit`,
which duplicates the *assistant response body* after a commit, and from
`#adoc-queue-ipc-drift`, which is a queue/snapshot drift loop.)

This session (`tasks/software/tsift.md`, agent-doc 0.34.x) showed the correlated
IPC-write diagnostics on nearly every cycle, which is the suspected mechanism class:

```
[write] IPC proof insufficient ... invariant=live_prompt_drift_after_preflight
        recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar
[write] mid-turn cycle rotation detected ... cycle ... marked committed but the
        incoming response heading(s) are absent from HEAD — starting a fresh cycle
[write] socket IPC reported already_applied
```

These all indicate the live editor buffer and the committed snapshot diverge while
the user is concurrently editing — the exact precondition for a duplicated prompt.

## Repro (to confirm)

Not yet captured deterministically. Suspected repro shape:

1. Open a CRDT/template session doc (`agent_doc_write: crdt`) in IntelliJ with the
   agent-doc plugin + IPC patches dir present (`.agent-doc/patches/`).
2. While an agent-doc cycle's write-back / boundary reposition is in flight (or a
   queued fallback patch exists), begin typing a new prompt into `exchange` after the
   prior `<!-- agent:boundary:* -->` marker.
3. Observe the typed prompt rendered twice once the pending IPC apply lands.

A deterministic harness should drive `PatchWatcher.applyPatch` /
`repositionBoundaryViaDocument` against a document whose buffer has uncommitted
post-boundary keystrokes, asserting the post-apply text contains the typed prompt
exactly once.

## Likely root cause (hypothesis — not yet proven)

The IPC apply path replaces the whole document via the Document API and recomputes the
result from `sourceContent` + patches + boundary reposition + a normalize step:

- `PatchWatcher.kt:774-775` — "Normalize after patches and boundary reposition so
  prompts typed after the prior boundary are included in the user region seen by the
  sidecar." This step *deliberately re-incorporates* the just-typed prompt into the
  user region.
- `PatchWatcher.kt:801` (and the reposition path at `:474`) — `document.setText(result)`
  full-document replacement.
- `write.rs` `try_ipc()` synthesizes a **boundary-aware exchange patch automatically**
  "when no explicit patches exist but unmatched content targets `exchange`/`output`
  and a boundary marker is present" (per agent-doc CLAUDE.md / write contract).

The suspected duplication window: when a synthesized/boundary-aware exchange patch
*already carries* the post-boundary prompt text **and** the editor-side normalize step
*also* preserves the same post-boundary keystrokes, `setText(result)` writes the prompt
twice. The `await_idle` debounce guard (`agent_doc_await_idle`,
`awaitIdleBeforeDocumentMutation`, 5s timeout at `PatchWatcher.kt:368/518`) reduces but
does not eliminate the race: a transient idle can satisfy the guard, the result is
computed against a buffer snapshot, then the user resumes typing before/within the
`WriteCommandAction`, or the 5s timeout elapses and the apply proceeds anyway against a
stale `sourceContent`.

Two candidate root layers (need to confirm which fires):
- **(L1) Rust side** — `try_ipc()` boundary-aware synthesis includes live post-boundary
  user text in the patch payload that the editor will also retain locally.
- **(L2) Editor side** — `PatchWatcher` normalize + `setText` double-counts the
  post-boundary prompt (patch content ∪ buffer content) instead of treating the
  editor buffer as the single source of truth for the user region.

## Fix options

**Option A (recommended) — make the post-boundary user region single-source + idempotent apply.**
Treat the editor buffer's post-boundary user region as authoritative and never let an
IPC patch re-insert text that already exists there. Concretely:
- On the editor side, before `setText`, diff the computed `result`'s post-boundary user
  region against the current buffer's post-boundary region and collapse an exact
  duplicate prompt run (reuse / extend the existing `dedupe.rs` consecutive-duplicate
  logic, exposed via FFI so both editors share it — Shared Foundation pattern).
- On the Rust side, exclude live post-boundary user text from the synthesized
  boundary-aware exchange patch payload so the patch only carries agent-owned content;
  the editor retains the user region locally. This removes the patch∪buffer overlap.
- Add an idempotency guard keyed on (patch_id, post-boundary-user-region-hash) so a
  re-delivered or already_applied patch cannot re-append.

**Option B — strengthen the typing guard only.**
Make `awaitIdleBeforeDocumentMutation` fail-closed: on 5s timeout, skip the apply and
reschedule rather than applying against a stale `sourceContent`; re-read buffer content
*inside* the `WriteCommandAction` immediately before computing `result`. Lower-risk but
narrower — it shrinks the race window without removing the patch∪buffer double-count, so
a fast typist can still trip it.

Recommend **Option A**: B alone leaves the structural double-counting in place. A makes
the user region single-source and the apply idempotent, which also hardens the related
`#duplicate-response-after-commit` and `#adoc-queue-ipc-drift` paths.

## Verification

- Deterministic editor-side test (`PatchWatcherTest`) asserting a post-boundary typed
  prompt survives a synthesized exchange-patch apply exactly once.
- FFI/Rust test asserting `try_ipc()` boundary-aware synthesis excludes live
  post-boundary user text from the patch payload.
- Re-run the live IntelliJ repro and confirm no duplication while typing during an
  in-flight write-back.

## Related

- `#duplicate-response-after-commit` — response-body duplication via late IPC reposition.
- `#adoc-queue-ipc-drift` — IPC-buffer/snapshot divergence loop.
  Shared theme: IPC working buffer diverges from committed snapshot and is reconciled
  non-idempotently. A single idempotent-apply + single-source-user-region fix likely
  covers all three.
