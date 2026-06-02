# Plan — `auto` on the `agent:backlog` marker misfires an auto-run that rewrites the backlog

> Status: IMPLEMENTED 2026-05-29 (defensive warn). Filed via `#agent-doc-bug` from `boost-client/tasks/monsterrodholders/mrh-project.md`.
>
> Implementation (submodule `fdc184f2`, superproject `f88ca1951`): validation (below) confirmed the destructive path (#2 auto-loop scope, #3 backlog rewrite) was already prevented by queue-scoping; only proposed fix #1 (warn on misplaced/unknown attrs) was genuinely open. Added a non-blocking `misplaced_component_attr` preflight warning in `agent-doc-orchestration/src/preflight.rs` for queue-only attrs on non-queue components (`auto` on `agent:backlog`) and unrecognized attribute keys (the `auot` typo), with guidance toward `<!-- agent:queue auto -->`. No mutation occurs. Tests + spec §2.4/§2.5 updated; live-verified against a throwaway doc. Proposed fixes #2/#3 needed no code change.
>
> **Fix #4 (recovery snapshot) IMPLEMENTED 2026-06-02** (`#misfire-recovery-snapshot`): `orchestrate` now drops a `agent-doc/<doc>/pre-auto-run-N` git tag at HEAD before a `--from-queue` auto-run (sequential/dag) executes its first task, mirroring `compact`'s `pre-compact-N` checkpoint via the shared `create_pre_mutation_tag` helper (`agent-doc-orchestration/src/compact.rs`). Best-effort/non-fatal; skipped for non-queue / `--dry-run` / `--plan` / `--no-git`. Tests: `create_pre_mutation_tag_auto_increments_ordinal_per_slug`, `create_pre_mutation_tag_honors_override_name`. Spec: `specs/07-orchestration-commands.md` § Pre-auto-run recovery tag. Note this guards the auto-run mutation surface defensively; the destructive backlog rewrite from the original report remains queue-scoped/inert in current code.

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

## Validation against current source (2026-05-29, v0.34.0 — static, read-only; no implementation)

Validated the three hypotheses against the actual code. Result: the **data-loss path is already prevented**; only the defensive warn (proposed fix #1) remains genuinely unaddressed.

- **Hypothesis #2 (auto-loop keys off `auto` wherever it appears) — DOES NOT HOLD.** Every one of the 14 production `has_auto_attr()` call sites (`run.rs`, `preflight.rs`, `route.rs`, `write.rs`, `codex_hook.rs`, `plan.rs`, `orchestrate.rs`) passes the attrs of a component resolved by `find(|c| c.name == "queue")` (or its snapshot equivalent). No path consults the `backlog` component's attrs for activation. The auto-loop is strictly queue-scoped.
- **Hypothesis #3 (auto-run destructively rewrites the backlog / mints `do [#…]` stubs) — DOES NOT HOLD for backlog.** The ID-minting task-expansion path (`orchestrate.rs::queue_task_batch`) reads only the `queue` component and bails with "agent:queue is not active" unless the queue is active. There is no path where a `backlog` `auto` attribute drives a backlog rewrite. (Note: the reported corruption shape — `preset #…` line + `- [ ] [#minted] do [#oldid]` — matches the orchestration task-batch backlog write-back output, so the historical trigger was an orchestration write-back; the current code only sources that batch from `queue`.)
- **Hypothesis #1 (marker-attr parser tolerates arbitrary/unknown attrs on `agent:backlog`) — HOLDS, still unaddressed.** `component::parse_attrs` (`agent-doc-core/src/component.rs:577`) splits any whitespace token into the attrs map with no per-component validation, rejection, or warning. So `<!-- agent:backlog auto -->` (and the `auot` typo) still parse silently — but the result is now **inert** because no consumer reads backlog attrs.

**Net:** Against v0.34.0 the destructive misfire does not reproduce statically (queue-scoping prevents both activation and ID-minting). The remaining value of the planned fix is purely defensive UX: warn/reject on a misplaced `auto`/unknown attr on `agent:backlog` (proposed fix #1), and optionally the recovery snapshot (proposed fix #4).

**Uncertainty / not proven:** This is a static read of current source only. The exact historical path that produced the original corruption was not pinned to a specific prior version/commit — it may already be closed by the queue-scoping (whether pre-existing or from the recent queue-auto convergence work), or it may have come from a path that has since changed. Confirming requires git archaeology of the orchestration write-back at the time of the report. Still **DO NOT IMPLEMENT** until authorized.
