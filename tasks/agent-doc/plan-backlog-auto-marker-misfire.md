# Plan — `auto` on the `agent:backlog` marker misfires an auto-run that rewrites the backlog

> Status: PLAN ONLY — do not implement. Filed via `#agent-doc-bug` from `boost-client/tasks/monsterrodholders/mrh-project.md`.

## Symptom (observed 2026-05-29)

User intended to add `auto` to the **queue** to enable the auto-loop, but typed it on the **backlog** marker — `<!-- agent:backlog auto -->` (actually `auot`, a typo). The result:

- An auto-run fired off the backlog and **rewrote the backlog component**, replacing its prior content with id-wrapped `do [#…]` stubs:
  ```
  <!-- agent:backlog auot -->
  preset #spec-test-news-commit-push-deploy
  - [ ] [#8b25] do [#na3x]
  - [ ] [#200g] do [#yqkg]
  - [ ] [#ry27] do [#t5vx]
  - [ ] [#vm09] do [#y994]
  ```
- New item IDs (`#8b25`, `#200g`, …) were minted wrapping references to already-`done` items, and the user reported their original task text as lost.
- Recovery required manual git/sidecar archaeology + `--pending-clear` + a hand edit to restore the empty backlog.

## Root-cause hypotheses (to confirm during implementation)

1. The marker attribute parser accepts arbitrary attributes on `agent:backlog` (it tolerated even the typo `auot`) instead of rejecting/ignoring unknown ones. `auto` is a **queue-only** attribute.
2. The auto-loop / queue-consume machinery keys off an `auto` attribute wherever it appears, so `auto` on the backlog was treated as an executable directive.
3. The backlog auto-run path mutates the backlog destructively (mints IDs / rewrites item text) rather than treating the backlog as a read-only holding list.

## Proposed fix (decide during implementation)

1. **Reject `auto` (and other queue-only attributes) on `agent:backlog`** — fail closed with a clear warning ("`auto` is a queue attribute; did you mean `<!-- agent:queue auto -->`?"), and do **not** mutate the backlog. Same for unknown attributes generally.
2. **Auto-loop must trigger only from `<!-- agent:queue auto -->`** (and the documented queue fields), never from the backlog marker.
3. **Never destructively rewrite committed backlog item text** in an auto-run. Backlog mutations must go through the granular `--pending-*` flags; an auto-run that wants to act on backlog items should copy them into the queue, not rewrite the backlog in place.
4. Add a guard/snapshot so a misfire is recoverable without git archaeology (e.g., tag `pre-auto-run` like compact does).

## Repro

1. In an agent-doc template doc, add `<!-- agent:backlog auto -->` (or `auot`) with some hand-typed backlog items.
2. Run `agent-doc <FILE>` (or let the auto-loop fire).
3. Observe the backlog rewritten with id-wrapped `do [#…]` stubs and original text gone.

## Regression test

- Assert `agent:backlog` with an `auto`/unknown attribute is rejected (warning, no mutation).
- Assert the auto-loop does not start from a backlog `auto` attribute.
- Assert an auto-run never replaces existing backlog item text.
