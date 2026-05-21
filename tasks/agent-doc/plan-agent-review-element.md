# Plan — `agent:review` document element

| Field | Value |
|---|---|
| **Status** | Proposed |
| **Filed** | 2026-05-21 |
| **Filed by** | Brian Takita |
| **Scope** | agent-doc template-mode documents |
| **Reference document** | `src/session-share/tasks/root.md` (session-share) — has Rob-review rules |

## Problem

session-share's [Backlog Closeout — review-then-done rule](https://github.com/ClaudeScore/SessionShare/blob/main/CLAUDE.md#backlog-closeout--reviewthendone) introduces a third lifecycle state for backlog items:

1. `[ ]` — not started (in `<!-- agent:backlog -->`)
2. `[/]` — code-complete, awaiting Rob's review (currently still in `<!-- agent:backlog -->`)
3. `[x]` — accepted (archived into `<!-- agent:done -->`)

Today agent-doc only models two visual containers — `agent:backlog` and `agent:done` — with `[/]` floating among `[ ]` items. This conflates "queued work" with "awaiting human review," which:

- Hides review-load from the user at a glance.
- Makes it hard for the agent to decide whether to surface a review-pending item ("this is code-complete; Rob, please look") versus a not-started one.
- Forces ad-hoc filtering by checkbox glyph in every backlog inspection.
- Doesn't give the binary a place to enforce "only Rob/Brian advance items from review → done" (today `--done` works on any item regardless of state).

## Proposal

Introduce a fourth canonical agent-doc component, `agent:review`, between `agent:backlog` and `agent:done` in the lifecycle.

### Document surface

```markdown
## Backlog

<!-- agent:backlog -->
- [ ] [#abc1] Not-started item
- [ ] [#def2] Another not-started item
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ghi3] Code-complete, awaiting Rob review — pushed 2026-05-21
- [/] [#jkl4] Code-complete, awaiting Rob review
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/root.done.md -->
<!-- /agent:done -->
```

### CLI surface

| Existing | Behavior change |
|---|---|
| `agent-doc write --pending-gate <id>` | Moves item from `agent:backlog` ([ ]) → `agent:review` ([/]), instead of staying in backlog with checkbox glyph change. |
| `agent-doc write --pending-ungate <id>` | Moves item from `agent:review` ([/]) → `agent:backlog` ([ ]). |
| `agent-doc write --done <id>` | Accepts either backlog or review source. Adds a `review_done_guard` (frontmatter-configurable, like `pending_done_guard`) — when set to `warn` or `error`, requires the item to live in `agent:review` before `--done` succeeds. Default `off` to avoid disruption. |
| `agent-doc write --pending-add` | Unchanged; always adds to `agent:backlog`. |
| New: `agent-doc write --review-add <text>` | Add an item directly to `agent:review` (rare; usually via `--pending-gate`). |
| New: `agent-doc write --review-edit "id=text"` | Edit a review item in place (parity with `--pending-edit`). |

### Migration

- Existing documents without `<!-- agent:review -->` should be auto-migrated by `agent-doc migrate`: items with `[/]` in `agent:backlog` get pulled into a newly-inserted `agent:review` block (positioned after `agent:backlog` and before `agent:done`).
- `agent-doc preflight` should emit a `warnings[]` entry (kind: `legacy_gated_in_backlog`) on legacy documents until migrated, similar to how `agent:backlog-done` migration is currently handled.

### Parser / doc-format updates

- `runbooks/document-format.md` adds `agent:review` to the component catalog.
- `runbooks/pending-ops.md` documents the new gate/ungate behavior and review-list mutations.
- `agent-doc preflight` JSON contract gains `review_count` and `review_gated_count` fields alongside the existing `pending_gated_count`.
- Diff/snapshot logic treats the `agent:review` block as a first-class component (granular patch flags, no full-replace).
- doc-lint (in session-share `scripts/doc-lint.sh`) becomes aware of `agent:review` so it does not flag the block as unknown HTML.

### Closeout / session-check

- `session-check` `pending_done_guard` is extended:
  - `pending_done_guard: warn` — current behavior (warns if response resolves an open pending item without `--done`).
  - New: `review_done_guard: warn|error|off` — frontmatter switch. With `error`, `--done` against an `agent:backlog` item fails unless first gated via `--pending-gate`.
- Existing `[/]` glyph semantics preserved in the component for visual continuity, but the binary derives state from container membership, not the glyph.

## Rules engine (project-specific)

session-share `CLAUDE.md` already says: *"The agent should not skip step 1 [pending-gate] and jump to `--done` unless the user has explicitly authorized self-acceptance for that specific item."*

With `agent:review`, that rule becomes mechanically enforceable. Other projects (e.g. agent-doc-bugs2.md) keep the default `off` and lose nothing.

## Non-goals (for v1)

- Multi-reviewer state (one item with multiple required approvers). Useful later but adds schema complexity; punt.
- Per-item reviewer attribution (who needs to approve). For session-share today this is implicit (Rob/Brian). A `reviewer:` field on the review item could come later if the rule applies across multiple projects.
- Worksheet-row hour tracking (Wed 5/20/26 sheet Column L). Stays a separate session-share convention; the agent-doc binary doesn't need to know about it.
- Auto-detection that "all queued work is done" → "promote to review." That's a workflow heuristic, not a tool primitive.

## Risks

- **Document churn during migration.** All existing session-share documents will gain a `## Review` section. Doc-lint must be updated in lock-step or strict mode will fail on every doc in the next pass.
- **Backwards-compat shim for `agent-doc finalize --pending-gate <id>`.** If `agent:review` isn't present, the binary must auto-insert it; otherwise legacy docs break the first time gate is called.
- **`--done` semantics change is potentially breaking** if a project enables `review_done_guard: error` without first gating its existing items. Mitigation: default `off`, document explicitly that opting in requires a one-time migration pass.

## Implementation order (suggested)

1. **Parser + storage:** recognize `<!-- agent:review -->` block, add `review_*` fields to preflight JSON, no behavior change yet.
2. **`migrate` helper:** auto-insert empty `agent:review` block + move `[/]` items from backlog.
3. **`--pending-gate` retarget:** move to `agent:review` instead of in-place glyph change.
4. **`--done` accepts both sources** + optional `review_done_guard`.
5. **doc-lint update** in session-share + parallel update in other consumer repos.
6. **Documentation:** `document-format.md`, `pending-ops.md`, `commit.md` review-then-done callout.

## Reference use case

session-share `tasks/root.md` currently has these `[/]` items that should move to `agent:review` on the first migration: `#vtvc`, `#vtas`, `#vtmgr`, `#wdmgr` (post-2026-05-21), `#lk2a` (post-2026-05-21), `#vtbs`, `#qew8`. After this feature ships, agent-doc auto-moves them and `session-check` enforces "Rob promotes to done."

## DO NOT IMPLEMENT YET

Per the `#agent-doc-bug` preset rule. This plan is filed for agent-doc backlog scheduling; implementation kicks off when the agent-doc maintainer accepts it.
