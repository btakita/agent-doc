# Compound Task Steering

Use this runbook when one user directive mixes a primary task with clear follow-up clauses that should be tracked as explicit steps instead of one opaque prose blob.

## Goal

Keep execution and recovery deterministic by normalizing compound prose into explicit orchestration before running it.

## When To Use It

- The user writes one task line with multiple imperative clauses, for example `do #ntoc. Add to today's news. commit + push`.
- A clause clearly targets a different follow-up surface or lifecycle step, such as a news update, separate document update, notification, or final `commit + push`.
- The work would be clearer as sequential or dependency-ordered steps than as one free-form prompt.

## Steering Rule

- Preserve the user's intent, but do not force the entire line through execution as one opaque task when the follow-up clauses are operationally distinct.
- Normalize the work into explicit steps first, then execute those steps through the existing `agent-doc` lifecycle or `agent-doc orchestrate`.
- Keep this steering in the skill/runbook layer unless the user already supplied explicit orchestration metadata. Do not invent binary-owned prose grammar.

## Normalization Pattern

1. Identify the primary task.
2. Split clear follow-up clauses into explicit ordered steps.
3. Use `agent-doc orchestrate <FILE> --mode sequential` for straight-line follow-up work.
4. Use `agent-doc orchestrate <FILE> --mode dag` only when the user already expressed real dependencies.
5. Keep `commit + push` as the last explicit step, not an ambiguous sentence fragment buried inside the primary task prompt.

## Examples

- `do #ntoc. Add to today's news. commit + push`
  Normalize to:
  - primary `do #ntoc`
  - follow-up `Add #ntoc result to today's news`
  - final `commit + push`

- `do #bench. Run benchmarks. Add to today's news. commit + push`
  Normalize to:
  - primary `do #bench`
  - same-task verification `Run benchmarks`
  - follow-up `Add #bench result to today's news`
  - final `commit + push`

- Explicit metadata already present:
  ```md
  - [id=ntoc] do #ntoc
  - [id=news-ntoc after=ntoc] Add #ntoc result to today's news
  - [id=push after=ntoc,news-ntoc] commit + push
  ```
  Preserve it. Do not rewrite explicit orchestration into free-form prose.

## Constraints

- Do not silently drop any user-requested clause.
- Do not fan out same-document patchback steps concurrently; shared-document follow-ups stay sequential/DAG.
- If the clause is ambiguous and you cannot infer a safe explicit step, surface the ambiguity in the response instead of inventing hidden execution rules.
