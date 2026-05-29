---
agent_doc_session: plan-stale-capture-after-compaction-blocks-route
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
---

# Plan: stale capture sidecar after compaction blocks `Run Agent Doc` with baseline mismatch

**Bug id:** `#stale-capture-after-compaction-blocks-route`
**Status:** investigation + fix plan (triage capture)
**Severity:** medium — recoverable via a printed manual command, but it interrupts dispatch and the recovery is manual.

## Symptom (reported 2026-05-29, live)

Pressing `Run Agent Doc` on `tasks/agent-doc/agent-doc-bugs2.md` failed with:

```
[route] debounce OK — file idle for 0.5s and typing indicator idle
[route] target tmux session: 0
Error: authoritative actor generation 204 ... route could not drain the active closeout before dispatch: captured response baseline no longer matches current document for .../agent-doc-bugs2.md. Rebuild sidecars without clearing session state: `agent-doc reset --from-current --preserve-session .../agent-doc-bugs2.md`
```

## Root cause (traced)

1. The route's pre-dispatch "drain the active closeout" step runs `repair`, which loads a capture sidecar and calls `capture::validate_replay` (`repair.rs:1488`).
2. `validate_replay` (`capture.rs:444`) fail-closes when `file_hash` drifts **and** `response_body_intact_in_current` is false. That intactness check is exactly `repair::response_already_applied[_after_prefix_strip]` — so it returns false only when the captured response body is genuinely **absent** from the current document.
3. Earlier this session the exchange was **compacted** ("Archived N response topic(s)"), which removed older `### Re:` bodies from the live document. The capture sidecars for those archived responses still exist, but their `response_body` is no longer present in the doc.
4. So `validate_replay` correctly bails: it cannot distinguish "this response was deliberately compacted away (safe to drop the capture)" from "this response was lost (must not silently discard)". It fails closed and prints the manual `reset --from-current --preserve-session` recovery.

Net: compaction archives responses but leaves their capture sidecars behind; a later route drain finds a stale capture whose body is gone and blocks dispatch until the operator runs a manual reset.

## Why existing benign-drift handling doesn't cover it

`#adoc-baseline-drift-after-user-commit` (Phase 2, `capture.rs:460`) already refreshes the baseline when the captured response body is **still intact** in the working tree. The compaction case is the opposite: the body is intentionally **gone**, so the intactness gate fails and the bail fires.

## Proposed fix options

- **Option A (recommended) — compaction discards its own captures.** When `compact` archives response topics, mark the corresponding capture sidecars `Discarded` (or delete them) in the same operation. Compaction is the authority that decided to archive those responses, so this needs no guessing: a capture whose response was just archived is provably terminal. The route then finds no stale active capture to drain. Touches `compact.rs` + `capture.rs` (a `discard_captures_for_responses(...)` helper keyed by `response_sha256`).
- **Option B — route treats committed-cycle + archived-response captures as drained.** In the drain/repair path, if the cycle is `committed` and the capture's `response_sha256` is present in the compaction archive (or the capture `committed_at` predates the latest compaction), treat the capture as already-drained instead of bailing. Heavier correlation; Option A is cleaner because it removes the stale sidecar at the source.
- **Option C — auto-run the recommended reset.** Have the route auto-invoke `reset --from-current --preserve-session` when the only blocker is a baseline mismatch on a committed cycle. Removes the manual step but is the bluntest (rebuilds all sidecars), so reserve as a fallback.

Recommend **Option A**: smallest, provably-safe (compaction owns the archive decision), and removes the root cause instead of papering over it. Add a deterministic test: capture a response, compact it away, assert the capture is discarded and a subsequent `validate_replay`/drain does not bail.

## Out of scope / do not implement yet

Triage capture only — the replay-safety path is fail-closed for good reason. No code changes until Option A vs B is chosen and a deterministic compaction+capture test harness is in place.

## Related

- `#adoc-baseline-drift-after-user-commit` — benign-drift baseline refresh when the response body is still intact (the inverse case).
- `#duplicate-response-after-commit`, `#exchange-prompt-dropped-on-merge` — same IPC/capture lifecycle family.
