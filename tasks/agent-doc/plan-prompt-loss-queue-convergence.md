# Plan: prompt loss when queue-convergence / supervisor-recycle drops an uncommitted live-buffer edit

**Status:** plan only — DO NOT IMPLEMENT (filed via `#agent-doc-bug`).

## Symptom (live repro 2026-06-20, equityfundingsource.md)

A user typed a new queue line into the live session doc:

```
- re #cftk: Would `passage btak/equityfundingsource/cloudflare_api_token` work?
```

The next `/agent-doc` cycle silently **lost** it:

1. First `preflight` reported `no_changes:false`, `diff_type:"simple_question"`, but **`diff:null`** — preflight detected *that* something changed and even classified it as a question, yet produced an empty diff body.
2. A second `preflight` (same turn, moments later) reported **`no_changes:true`**.
3. The agent concluded "stale state settled, nothing changed" and stopped. The user's prompt was never answered and was **deleted from the document**.

The turn immediately prior had issued `agent-doc admin recycle` (stale-binary supervisor refresh) and preflight had logged:

```
[preflight] queue: converged live editor buffer (auto=false, queue_active=false)
[preflight] queue: stripped activation token (drained)
[commit] skipping working-tree boundary reposition — IPC listener active
```

## Root-cause hypothesis

The queue-convergence / live-editor-buffer convergence step (`converged live editor buffer`) overwrote the on-disk/live-buffer doc — which held the user's **uncommitted** new queue line — with the committed/HEAD-derived queue state (which was empty/drained). The user's pending edit was discarded rather than merged. The concurrent supervisor recycle at the idle boundary widened the window.

The `no_changes:false` + `diff_type:simple_question` + **`diff:null`** combination is the smoking gun: preflight's classifier saw the new prompt, but the diff renderer produced nothing (the convergence had already begun erasing the source line), and the re-run then saw the post-convergence empty state as `no_changes:true`.

This is **silent user-data loss**, distinct from `#6b5h` (finalize IPC wedge on a CLI-only actor) — here the loss happens in preflight queue-convergence, before any response is composed.

## Fix direction (do not implement yet)

1. **Convergence must merge, never overwrite.** `converge live editor buffer` / `strip activation token` must preserve any uncommitted user-authored queue/exchange line present in the live buffer or on disk. Converge by CRDT-merging the user edit forward, not by replacing the buffer with the HEAD/committed queue state.
2. **Fail closed on `no_changes:false` + `diff:null`.** A diff that is null/empty while the change classifier says `simple_question` (or any non-empty `prompt_changes`) must be treated as an unresolved prompt that needs capture — never allowed to resolve to `no_changes:true` on a re-run. Reconstruct the prompt from the semantic diff / live buffer instead of dropping it.
3. **Sequence recycle after prompt capture.** Do not run queue-convergence + boundary-reposition that can drop live-buffer content in the same window an `admin recycle` is pending. Capture/commit any pending user prompt before the idle-boundary recycle fires.
4. **Coverage.** Add SimWorld coverage for: live buffer has an uncommitted queue line → preflight convergence runs → the line survives into `prompt_changes` and is answerable; and the recycle-during-pending-prompt race.

## Evidence handles

- preflight #1: `no_changes:false diff_type:simple_question diff:null` (prompt present in `annotated_diff` as `[user+] - re #cftk: ...` only on the LATER re-entry, after the user re-typed it).
- preflight #2 (same turn): `no_changes:true`.
- prior-turn logs: `converged live editor buffer (auto=false, queue_active=false)`, `admin recycle: controller ... marked to recycle at next idle boundary`.
