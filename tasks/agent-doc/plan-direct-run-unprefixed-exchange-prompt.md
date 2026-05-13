# Plan: direct run must prefix new user exchange prompts

## Problem

Direct `agent-doc <file>` runs can commit a new user prompt in `agent:exchange`
without the canonical `❯ ` prefix.

Evidence:

- Document: `src/boost-client/tasks/monsterrodholders.md`
- Commit: `a4ac31b agent-doc(monsterrodholders): 2026-05-12 22:40:26`
- Prompt block committed without prefixes:
  - `Read the image.`
  - `![img_7.png](img_7.png)`
- The block was inserted where a prompt belongs, immediately before the agent
  response and the new `agent:boundary`, but it remained indistinguishable from
  assistant prose.

This is related to older prefix-state bugs such as
`plan-prefix-strip-on-user-prompts.md`, but the failing shape is narrower: a
fresh direct-run prompt block was never normalized to the prompt marker at all.

## Desired Behavior

1. Every user-authored exchange prompt line that enters a committed agent-doc
   cycle must be represented with `❯ ` in the session document.
2. Multi-line prompts, including Markdown image/link lines, should be normalized
   as one user prompt block.
3. Assistant response lines must not receive `❯ `.
4. Boundary repositioning, CRDT/IPC patchback, and fallback file writes must all
   preserve the same prefix result.

## Investigation Steps

- [ ] Reproduce the Monster Rod Holders shape with a direct `agent-doc <file>`
      prompt containing a text line plus a Markdown image line.
- [ ] Trace whether prefix normalization is skipped in:
      - `run.rs` direct child-run writeback
      - template boundary synthesis
      - CRDT write path
      - IPC `reposition_boundary` / `normalize_prefix_lines`
      - no-listener fallback file write
- [ ] Compare direct-run prompt normalization with `finalize --stream`, where
      prompt targets are expected to receive `❯ ` before closeout completes.
- [ ] Determine whether the prefix target list omitted Markdown image lines or
      whether the whole prompt block was outside the user region seen by the
      normalizer.

## Implementation Steps

- [ ] Add a binary-owned normalization step for newly detected prompt-bearing
      exchange drift before the response body is merged and committed.
- [ ] Treat consecutive new lines between the previous boundary and the response
      insertion point as a user prompt block unless they are structural comments
      or already inside an assistant response block.
- [ ] Include Markdown image/link-only lines in prompt-block continuation
      handling.
- [ ] Make the editor IPC payload and fallback write use the same normalized
      prefix target list so editor-visible and committed content agree.
- [ ] Fail closed if prompt-bearing drift remains unprefixed after writeback.

## Regression Coverage

- [ ] Direct-run integration test: raw two-line prompt plus image line commits
      as `❯ Read the image.` and `❯ ![img_7.png](img_7.png)`.
- [ ] Regression test for one-line prompt, multi-line prose prompt, and
      Markdown-only prompt continuation.
- [ ] Negative test proving assistant response lines after `### Re:` do not gain
      `❯ ` during the same normalization pass.
- [ ] CRDT/IPC fallback test proving sidecar divergence cannot leave committed
      prompt lines unprefixed.

## Exit Criteria

- Direct `agent-doc <file>` closeout cannot commit a fresh user prompt block
  without `❯ `.
- `session-check` catches unprefixed prompt-bearing exchange drift before a run
  reports success.
- Existing response-prefix protections still prevent `❯ ` from leaking onto
  assistant closeout lines.
