# Plan — Manageable review-gated items

## Problem

`agent:review` lists have grown unmanageable on long-running docs (62 gated items
on `agent-doc-bugs2.md`, similar on `monsterrodholders.md`). The user wants:

1. **Actively reduce** review-gated items by splitting actionable work into the backlog.
2. **Token-efficient NEXT-step annotations** on review items so a quick pass can
   determine next actions.
3. A **token-efficient way to query** gated review items to address them.
4. **Feed review items back into the backlog pipeline** (simplify management).

## What already exists

- `agent-doc review ungate-tasks` — adds a backlog follow-up task per gated review
  item (drives it ungate → done). Idempotent. **This is the feed-back primitive (4).**
- `--pending-gate` / `--pending-ungate` — move items between backlog `[ ]` and
  review `[/]`.
- Typed gates: `--pending-set-gate-type id=release`, `--pending-resolve-gate [/release]`.
- Convention dogfooded this session: `[blocked: <reason>] <summary>. NEXT: (1)… (2)…`
  on a gated item — a token-efficient annotation, but **not enforced or parsed**.

## Gaps

| Ask | Status | Gap |
|-----|--------|-----|
| (1) reduce / split actionable | partial | no triage view to decide what to split; `ungate-tasks` is all-or-nothing (would add 62 backlog tasks at once) |
| (2) NEXT annotations | convention only | no schema/lint; not machine-parsed |
| (3) token-efficient query | **missing** | no `review list` / query command — must read the whole component |
| (4) feed back to backlog | exists | `review ungate-tasks`, but bulk; no selective / filtered ungate |

## Design

### Stage 1 — `agent-doc review list` (recommended first)

A read-only, token-efficient query of `agent:review`:

```
agent-doc review list <FILE> [--gate-type <t>] [--tag <#tag>] [--has-next] [--json]
```

- Default: one compact line per gated item — `id  [gate-type]  first-line/NEXT summary`.
- `--has-next` / `--no-next`: filter by presence of a `NEXT:` annotation (surfaces
  items that still need triage).
- `--tag #foo`: filter by hashtag (e.g. `#lazily-rs-tmux-crash` clusters).
- `--json`: structured output for agents to triage programmatically.

This directly answers ask (3) and feeds (1) — you can see at a glance which items
have actionable NEXT steps vs. which are stale.

### Stage 2 — NEXT-annotation schema + lint

Define the annotation grammar (`[<gate-type|blocked:reason>] <summary>. NEXT: …`)
and a non-blocking lint (warn) that flags gated items with no NEXT and no recent
activity — the "stale review" set that should be split, ungated, or dropped.
`review list --no-next` is the query that surfaces them.

### Stage 3 — selective feed-back

Extend `review ungate-tasks` with `--ids <a,b,...>` / `--tag` / `--has-next` so the
feed-back is selective (split the actionable subset into backlog) instead of bulk.

## Files to modify

| File | Change |
|------|--------|
| `review_cmd.rs` (new sibling to the `review` subcommand) | `list` subcommand: parse `agent:review`, filter, compact/JSON output |
| `main.rs` | wire `review list` |
| `pending`/component parser | expose review-item fields (id, gate-type, tags, NEXT) for the query |
| `session_check.rs` | optional Stage-2 stale-review lint (warn-only) |
| specs `07-*`, `SKILL.md`, pending-ops runbook | document `review list` + the NEXT convention |

## Risks

- Low for Stage 1 (read-only query). Stage 2 lint must be **warn-only** (never block
  closeout on review hygiene). Stage 3 selective ungate must stay idempotent like
  the existing bulk form.

## Test plan

1. `review list` compact + `--json` output on a fixture with mixed gate-types/tags/NEXT.
2. Filters: `--gate-type`, `--tag`, `--has-next` / `--no-next`.
3. Stage 2 lint warns on no-NEXT stale items, never blocks.
4. Stage 3 selective `ungate-tasks --ids/--tag` matches the bulk form per-item.

## Open decision for the user

- Stage 1 (`review list` query) is the smallest high-value piece — recommend
  shipping it first. Want the NEXT-annotation **lint** (Stage 2) to be warn-only,
  or skipped entirely for now?
