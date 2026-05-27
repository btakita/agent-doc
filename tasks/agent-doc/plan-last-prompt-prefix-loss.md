# Plan - last prompt should retain prompt prefix

## Problem

The latest prompt in an agent-doc exchange can be persisted without the `❯ `
prefix. The observed failure was the final user prompt landing as plain text
instead of a prompt-prefixed transcript line.

This is adjacent to the existing multi-line prompt prefix issues, but the
specific risk is sharper: when the last prompt is bare, later preflight/finalize
logic can still treat it as a prompt target while the document remains visually
ambiguous and inconsistent with the rest of the transcript.

## Classification

This is an agent-doc bug. The session document invariant should be that
user-authored exchange prompts are persisted with the `❯ ` prefix, including
the newest prompt at the tail of the exchange.

## Suspected Boundary

- `diff::prompt_prefix_normalization_targets` and
  `diff::first_bare_prompt_prefix_target` already detect bare prompt lines.
- `write::normalize_user_prompts_in_exchange_safe` and
  `write::normalize_exchange_prefixes_for_targets` apply the repair to the
  document/write path.
- Sidecar/IPC fallback paths carry `normalize_prefix_lines`; the final prompt
  may be escaping one of these paths or being restored from a stale sidecar/blob
  after normalization.

## Fix

- Add a regression that starts from a template exchange with an answered prompt,
  appends a final bare user prompt at the exchange tail, then runs the same
  preflight/finalize or write path that reproduced the issue.
- Assert both working tree and committed blob contain the final prompt as
  `❯ <prompt text>`.
- If the test fails before IPC, repair the normalization target extraction or
  `normalize_user_prompts_in_exchange_safe` tail handling.
- If it fails only after IPC/sidecar fallback, ensure `normalize_prefix_lines`
  is applied after sidecar/content_ours recovery and before the final write and
  commit snapshot are recorded.

## Regression

Add focused coverage in `tests/finalize_integration.rs` or the nearest existing
write-path integration suite:

1. baseline exchange contains a previous `❯ ` prompt and response boundary;
2. working tree appends a final unprefixed prompt line;
3. finalize/write applies a normal response patch;
4. the resulting document and HEAD blob preserve the last prompt with `❯ `.
