# Plan — Preset-token queue head is unstrikeable by any agent path (#qpresetstrike)

## Symptom (live repro, 2026-06-13, equityfundingsource.md)

A queue head that is a **prompt-preset token** (a `#name` drawn from frontmatter
`prompt_presets`, e.g. `#advance-review`) cannot be struck/reaped through any
agent-facing command after its response is committed. The head stays in
`agent:queue`, keeps `queue_active=true`, and is re-served on every subsequent
`/agent-doc` invocation (and by the supervisor idle-watch under `go`).

Observed command results with the head `- #advance-review` in the queue:

```
$ agent-doc queue consume equityfundingsource.md
Error: queue head is an id-backed directive, not a free-text prompt.
  Reap it through the normal closeout with `--done <id>` / `--pending-gate <id>` ...
  `queue consume` only strikes free-text heads.

$ agent-doc write equityfundingsource.md --done advance-review
Error: id not found in backlog/icebox: advance-review
```

`finalize` also refuses to strike it: `kept head #advance-review because the
response did not record a completion outcome for #advance-review`.

Net: `consume` thinks it is id-backed (because of the leading `#`), so it routes
the operator to `--done`; `--done`/`--pending-gate` reject it because it is NOT a
backlog/icebox id — it is a preset token. The head is wedged in a classification
gap and re-serves indefinitely.

Workaround used to recover (the only one that worked): manually strike the head
in the doc (`- ~~#advance-review~~`) and drop `go` from the `agent:queue` marker,
then `reset --from-current --preserve-session` + `commit`. Editing the queue
directly races the live IPC writeback (`file modified since read`), so it must be
done in one shot and reconciled through reset, not a bare write.

## Root-cause hypothesis

The queue strike/reap router classifies a head as **id-backed vs free-text** by the
leading-`#` shape alone:

- `queue consume` is scoped to free-text heads; a `#`-prefixed head is deemed
  id-backed and refused (routed to `--done`).
- `--done <id>` / `--pending-gate <id>` look the token up in `agent:backlog` /
  `agent:icebox` and fail when it is not found.

A **preset token** (`#advance-review` exists in `prompt_presets`, NOT in the
backlog) is a third category the router does not handle: it matches the id shape
but has no backlog row. After `orchestrate` expands the preset and the response is
committed, nothing can record a completion outcome for the literal head, so it is
never struck.

Distinct from `#qchurn` / `#goqueuestall` (plan-go-queue-stall.md), which is about
go-mode not treating a queue of non-id free-text reports as *not-drainable*. This
bug is upstream of that: even with explicit operator intent to strike, no command
*can* strike a preset-token head.

## Fix direction (CLEAN session — touches the queue strike/reap classifier)

1. Classify a `#`-prefixed queue head against `prompt_presets` AND the
   backlog/icebox before routing: `id-backed` only when it matches a real
   backlog/icebox id; `preset` when it matches a `prompt_presets` key; `free-text`
   otherwise.
2. A `preset` head should be **strikeable like a free-text head** once this turn's
   response answered it: let `queue consume` strike it, and let the `finalize`
   strike-heuristic consume it (it was expanded via `orchestrate`/preset, not a
   `do #id` directive, so there is no backlog row to reap).
3. Keep id-backed (`do [#id]`) heads on the `--done`/`--pending-gate` reap path
   unchanged. A preset token that *also* happens to be a backlog id (unlikely, but
   possible) should prefer the backlog-id reap path and warn.
4. Confirm the fix also relieves the `#qchurn` re-dispatch loop for preset heads:
   once a preset head is struck, `queue_active` should drop without the manual
   reset workaround.

## Tests (add before/with the fix)

- SimWorld/unit: a queue whose head is a `prompt_presets` token, answered by a
  committed response, is struck by `queue consume` (and by `finalize`'s heuristic)
  and drops `queue_active`.
- A `do [#id]` head still requires `--done`/`--pending-gate` and is NOT struck by
  `queue consume`.
- A genuinely free-text head (no `#`) still strikes via `queue consume`.
- Regression: a `#`-token that is neither a preset key nor a backlog id is reported
  with an actionable error, not a silent wedge.

## Notes

- Live binary at repro: agent-doc 0.34.0.
- Related: `#qchurn` / `#goqueuestall` (plan-go-queue-stall.md), the `go`-mode
  no-op re-dispatch this bug feeds; `#compactdrift` (separate auto-adopt guard).
- DO NOT IMPLEMENT yet (operator `#agent-doc-bug` directive — plan only).
