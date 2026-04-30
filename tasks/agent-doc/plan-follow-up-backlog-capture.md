# Plan — enforce follow-up backlog capture for review flows

**Pending:** `#fuback`

**Observed in:** `tasks/software/tsift.md`

## Problem

`agent-doc` let a `#code-review` cycle commit three concrete findings without adding any new
`agent:backlog` items, even though the document frontmatter preset for `#code-review`
explicitly appends `#follow-up-backlog`.

That means the current guard surface is incomplete:

- preflight can already surface requested prompt presets
- prompt/planning paths can already emit `expect_add` for explicit backlog-oriented prompts
- finalize/session-check can already detect recommendation-like responses that omitted
  `--pending-add`

But none of those paths currently fail closed when the active prompt contract is "review this
and capture the follow-up backlog" while the response is phrased as findings-only review prose.

## Desired behavior

If the effective prompt requests backlog capture directly or through a preset such as
`#follow-up-backlog`, then a successful closeout must do one of these things:

1. record concrete `--pending-add` / `--pending-add-gated` mutations in the same cycle
2. explicitly prove there were no actionable follow-up items to capture
3. fail closed before commit

It should not silently commit review findings and leave the backlog unchanged.

## Plan

- [ ] Reconstruct the exact `tsift.md` failure chain, including the prompt preset expansion,
  preflight JSON, response shape, and whether any existing guard had enough information to stop
  the closeout.
- [ ] Audit how `prompt_presets_requested`, `pending_mutations.expect_add`, and the strict
  pending-capture gate are threaded through `preflight`, `plan`, `finalize`, and
  `session-check`.
- [ ] Define the invariant for backlog-required review flows:
  - preset-driven requests such as `#code-review` + `#follow-up-backlog`
  - explicit user prompts like "add the findings to the backlog"
  - findings-only review responses that still imply concrete follow-up work
- [ ] Tighten the closeout guard so backlog-required cycles fail before commit unless the cycle
  records backlog mutations or the response explicitly says there are no actionable follow-ups.
- [ ] Add regression coverage for:
  - tsift-style `#code-review` findings with no `--pending-add` -> closeout fails
  - the same response with backlog mutations -> closeout succeeds
  - a review that explicitly concludes there are no actionable follow-up items -> closeout
    succeeds without forced backlog growth

## Success criteria

- review flows that request backlog capture cannot silently commit findings-only prose
  without matching `agent:backlog` mutations
- the guard uses the effective prompt contract, not only recommendation-shaped response text
- the `tsift.md` repro class is covered by regression tests
