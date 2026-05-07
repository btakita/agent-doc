# Plan - prevent prefixed response labels from becoming prompt targets

**Pending:** `#respfx` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`agent-doc session-check` can fail after a committed cycle when an assistant response
label is accidentally prefixed with the user prompt glyph:

```text
[session-check] INTERRUPTED: cycle `cycle-1778181134648` is `committed`
...
prompt_target: ❯ **Verification:** Both redirects confirmed via `curl`.
```

The line is assistant-owned closeout prose, not a new user prompt. Because it begins
with `❯`, the prompt classifier reports it as unresolved prompt-bearing drift even
though the response was already committed.

## Likely Cause

The Rust classifier has the right response-label vocabulary but applies it too late
for prefixed labels:

- `diff::normalized_prompt_preview_line()` calls
  `text_line_looks_like_prompt_target()` first.
- `text_line_looks_like_prompt_target()` treats any non-heading line starting with
  `❯` as a prompt target.
- `line_looks_like_plain_response_after_prompt()` recognizes labels such as
  `Verification`, `What changed:`, `Follow-up:`, and `Commit / push:`, but a line
  like `❯ **Verification:** ...` is rejected before label detection can strip the
  prompt glyph and markdown emphasis.

The JetBrains prefix-repair helper has the same ordering hazard:

- `startsPromptRunAfterResponseForPrefixRepair()` treats prefixed text as a fresh
  prompt unless `lineLooksLikePlainResponseAfterPromptForPrefixRepair()` wins.
- That helper checks `lineLooksLikeFreshPromptAfterResponseForPrefixRepair()` before
  checking response labels, so prefixed response labels are also classified as fresh
  prompts.

## Fix

1. Add a shared response-label normalizer in Rust that strips an optional leading
   `❯`, list marker, markdown emphasis, and whitespace before testing known
   assistant closeout labels.
2. Make `line_looks_like_plain_response_after_prompt()` run that normalized
   response-label check before `normalized_prompt_preview_line()`.
3. Keep true user prompts protected: lines like `❯ verify deploy` or
   `❯ Verification failed; what next?` should still classify as prompts when they
   are question/directive-shaped rather than assistant label-shaped.
4. Mirror the same ordering and normalization in the JetBrains prefix-repair helper
   or expose the Rust decision through the existing native normalization boundary.
5. Add regressions for:
   - `❯ **Verification:** Both redirects confirmed via \`curl\`.` is assistant
     response prose, not a prompt target.
   - `❯ Commit / push:` and `❯ **Commit / push:**` do not reopen a cycle.
   - a real prefixed user follow-up after a response still starts a new prompt run.
   - JetBrains `normalizeExchangePrefixes` does not prefix assistant closeout labels
     inside an existing `### Re:` response block.

## Notes

This is related to the earlier prefix-leak fixes, but the failure mode is narrower:
even when the leaked prefix already exists in the document, `session-check` should
not let a known assistant closeout label masquerade as unresolved user work.
