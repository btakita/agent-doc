# Plan - agent:queue `preset` free-text attribute

**Pending:** `#queue-preset-attribute`
**Status:** Draft

## Request

2026-06-02, queued on agent-doc-bugs2.md:

> Add a preset attribute to agent:queue, where the value can be a free-text
> prompt that correctly applies `prompt_presets`.

Today `agent:queue` already accepts a leading `preset #name` line (the document
uses `preset #spec-test-build-install-commit-push`) which applies a single named
preset from frontmatter `prompt_presets` to the queue items. The request is to
let the `preset` attribute value be **free text** that is expanded the same way
`prompt_presets` expansion works — i.e., the attribute may contain inline preset
references (`#agent-doc-bug`, `#next-steps`, …) and/or literal prompt text, and
the binary resolves the `#name` tokens against `prompt_presets` before applying
the combined prompt to each queued item.

## Desired Behavior

- `agent:queue` `preset` attribute accepts a free-text value, not only a single
  bare `#name`.
- Any `#name` tokens in that value that match a `prompt_presets` key are expanded
  to their preset text; unmatched tokens stay literal.
- The resolved prompt is applied per queue item exactly as a named preset is today
  (same precedence and per-item application path), so a queue can carry, e.g.,
  `preset #spec-test-commit-push then print the top backlog id` and have both the
  preset expansion and the literal tail applied.
- Invalid/unknown `#name` tokens are preserved literally (no hard error), matching
  how free-text prompts already pass through.
- Round-trips through write/reset/commit queue convergence without mangling the
  attribute (coordinate with `#queue-user-edit-overwrite` preservation).

## Plan

1. Extend the `agent:queue` parser to capture the `preset` attribute as a
   free-text string (currently parsed as a single `#name` token).
2. Add a resolver that scans the value for `#name` tokens, expands those matching
   `prompt_presets`, and concatenates with surrounding literal text deterministically.
3. Reuse the existing per-queue-item preset application path so behavior matches
   the single-named-preset case; only the resolution step is new.
4. Preserve the attribute verbatim across `write`/`reset --from-current`/`commit`
   convergence (no drop/rewrite); add a regression with a multi-token free-text
   preset value.
5. Tests: parse a free-text `preset` value with mixed known/unknown `#name` tokens
   + literal text; assert correct expansion and per-item application; assert
   round-trip stability. `make check` green.

## Notes

- Sibling of the existing single-named-preset support and the
  `#backlog-queue-sync-attribute` / `#backlog-priority-attribute` queue-generation
  features. Composable: a synced queue could carry a free-text preset.
- DO NOT IMPLEMENT in this planning item.
