# Plan — Optional `do` directive + `re` reference form

## Problem

The user wants `do` to no longer be **required** to kick off a tracked task, while
preserving a way to **reference** a task without executing it:

> `do [task-id]` should not be necessary to kick off tasks. However, don't lose
> referencing the task such as `re [task-id]` which should not execute the task.

## Current behavior

- **Free-text queue items already execute** without `do` — any non-preset,
  non-completed `agent:queue` line becomes an executable prompt
  (`queue::prompts`). So `do` is not required for *free-text* kick-off today.
- The `do [#id]` / `do #id` form is special because it is **id-backed**: it
  targets a tracked `agent:backlog` item, and closeout reaps/gates that `#id`.
  Detection lives in:
  - `cycle_state::is_do_directive` (`do [#` / `do #` prefix) and
    `active_queue_directive_heads`.
  - `session_check::do_directive_target_ids_in_line` — strips list/checkbox
    markers, then requires a literal `do ` prefix before `extract_pending_hash_ids`.
  - `session_check::is_queue_directive_prompt`, and the closeout guards that key
    off directive targets (`pending_done_guard`, `expect_done_or_gate_ids`,
    queue-head provenance, `#do-id-closeout-open-backlog`).
- There is **no reference form**: a bare `[#id]` in prose is ignored by directive
  detection, and there is no `re [#id]` opt-out.

## The actual gap

1. A **bare `[#id]` / `#id`** in directive position should be treated as an
   id-backed execute (the `do` verb becomes optional).
2. A **`re [#id]` / `re #id`** form should *reference* the task — never execute,
   never reap, never trip a closeout guard.

## Design

### Grammar (directive position only)

Applied after the existing normalization (trim, strip `❯`, strip list marker
`- `/`* `/`+ `, strip a leading `[ ]`/`[x]`/`[/]` checkbox):

| Form | Meaning |
|------|---------|
| `do [#id]` / `do #id` | execute + id-backed (unchanged, back-compat) |
| `[#id]` / `#id` (bare, at directive start) | execute + id-backed (NEW — `do` optional) |
| `re [#id]` / `re #id` | reference only — no execute, no reap, no guard (NEW) |
| free text | execute as prose prompt (unchanged) |

### Disambiguation (the risk)

A bare `[#id]` must only execute when it is in **directive position** — a queue
line head, or an exchange prompt line — **not** an incidental id mention inside
prose. Rules:

- Only the **leading token** of a normalized directive/queue line is considered.
  `See [#foo] for context` does not start with `[#id]` as its whole head → not a
  directive. (The token must be immediately followed by end-of-line or a known
  separator, so `[#id]: note` prose stays inert — cf. the existing bracket-id
  prompt-guard false-positive class.)
- `re ` is matched the same way `do ` is (case-insensitive leading verb), and
  short-circuits to a **reference** classification that every downstream guard
  treats as non-executable (like a struck/commented line).

### Scope

Stage 1 — **queue lines** (lower risk; queue is already execution-intent):
`queue::prompts` / `is_do_directive` / `active_queue_directive_heads` recognize
bare `[#id]` as id-backed and `re [#id]` as a non-prompt reference.

Stage 2 — **exchange directives**: `do_directive_target_ids_in_line` and the
closeout guards accept the bare form and skip the `re` form. This is the
higher-blast-radius half (the guards) and should land with dedicated guard tests.

## Files to modify

| File | Change |
|------|--------|
| `queue.rs` | bare-`[#id]` → id-backed prompt; `re [#id]` → reference (excluded from `prompts`) |
| `cycle_state.rs` | `is_do_directive` + `active_queue_directive_heads` recognize bare/`re` forms |
| `session_check.rs` | `do_directive_target_ids_in_line`, `is_queue_directive_prompt`, and every directive-keyed guard accept bare form, skip `re` form |
| specs `07-*`, `SKILL.md`, runbooks | document the optional-`do` + `re` grammar |

## Risks

- **Accidental execution**: bare `[#id]` in prose mis-parsed as a directive. Mitigated
  by the leading-token + trailing-separator rule and by limiting Stage 1 to queue lines.
- **Guard regressions**: the closeout guards (`pending_done_guard`, queue-head
  provenance, `expect_done_or_gate`) all key off `do `. Each must learn the bare
  and `re` forms or risk false interrupts / missed reaps. Needs dedicated tests
  per guard before shipping Stage 2.

## Test plan

1. `queue::prompts` includes bare `[#id]`, excludes `re [#id]`.
2. `is_do_directive` / `do_directive_target_ids_in_line` accept bare, reject `re`.
3. Prose `See [#id] ...` and `[#id]: note` are NOT directives (no false execute).
4. Each closeout guard: bare form behaves like `do`; `re` form is inert.
5. Back-compat: every existing `do [#id]` test still passes unchanged.

## Open decision for the user

- **Stage 2 scope**: ship the exchange/guard half now, or land Stage 1 (queue
  only) first and gate Stage 2 behind live use? Recommend Stage 1 first — it
  delivers "`do` optional for kicking off queued tasks" with the smallest guard
  surface.
