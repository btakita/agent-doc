# Plan: finalize strike heuristic wrongly strikes the NEXT open queue head

**Backlog id:** `#qmisstrike`
**Class:** clean-session (queue strike/consume heuristic in the finalize path)
**Related:** `#qpresetstrike` (preset-token head unstrikeable), `#pushpinaccum` (pushpin nesting),
`#qchurn` / `#qdedup` (queue churn family), `#0kjc`/`#mdastquote` (parser masking of fenced/quoted regions).

## Operator report (tsift.md) + LIVE reproduction (agent-doc-bugs2.md)

Operator report (tsift.md):

```
preset="#spec-test-build-install-commit-push" go
- ~~[#pcacheattr]~~
- ~~[#gdbwritelock]~~
- ~~[#pcacheregtrunc]~~
- [#loggatefidelity]
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
```
"I then manually deleted the strikethrough items ... but then agent-doc struck through the next open
item":
```
- ~~[#loggatefidelity]~~       <-- wrongly struck
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
```

**Live reproduction (this session, 2026-06-13):** a `finalize` that answered the free-text
`:pushpin: JB Run Agent Doc …` queue head consumed that head AND additionally struck the next open
id-backed head `~~[#qchurn]~~`, even though the response never addressed `#qchurn`. session-check
caught it (`runnable agent:queue head(s) #qchurn were removed … never consumed/completed/gated`) and
the head had to be manually un-struck + re-committed via `reset --from-current --preserve-session`.

## Root cause (hypothesis to confirm in code)

The finalize "strike the answered free-text head" heuristic mis-targets when the head it consumed is
not the literal first list item:

- After the consumed/deleted leading head(s), the heuristic advances to the next visible list item
  and strikes it, instead of striking only the head(s) the response actually answered.
- In the operator's case the trigger was *manual deletion* of already-struck items shifting the
  "next open" cursor; in the live case the trigger was consuming a free-text head positioned before
  the id-backed list. Both reduce to: **the strike cursor over-advances by one and strikes an
  unanswered open head.**

The consume heuristic already documents "only consumes one head per finalize" (hence the explicit
`queue consume` straggler-drain). The bug is the off-by-one *target*, not the count.

## Fix

1. **Strike only answered heads.** The finalize strike heuristic must strike exactly the queue
   head(s) that this turn's response sections answer (match by the consumed head text / the
   `### Re:` topic), never "the next open item" by position.
2. **Never strike an id-backed head as a side effect.** An id-backed `[#id]` head may only be struck
   via the `--done`/`--pending-gate`/`queue consume` (id-reap) paths, not by the free-text strike
   heuristic. If the answered head was free-text, the heuristic must stop at the free-text boundary.
3. **Manual-deletion stability.** When the operator deletes already-struck items, recomputing the
   queue must not re-anchor the strike cursor onto the new leading open item. Treat operator
   strike-deletions as terminal (those heads are done); do not re-strike forward.
4. **Proof + guard.** Emit `queue_strike target=<head> reason=answered|consume` and assert in
   session-check that every newly-struck id-backed head was matched to a `--done`/gate/consume action
   (the existing `#queue-clear-unrun-items` guard already catches the symptom — make finalize not
   produce it).

## Tests (SimWorld)

- Free-text head before an id-backed list → finalize answers the free-text head → only that head is
  struck; the id-backed head stays `[ ]` runnable.
- Operator deletes leading struck items, then a clean finalize → no forward re-strike of the new
  leading open item.
- Multi-head answer → only the answered heads strike (regression guard for the legit consume path).
- `make check` green; tmux-CI review for the queue/finalize surface.

## Note

CLEAN session required (finalize queue path; live-IPC supervisor races writes here). This item was
itself born from a live mis-strike during a degraded session — fix + verify under a freshly
`make install`-ed, recycled supervisor.
