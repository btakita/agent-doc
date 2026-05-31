# Plan: repair-orphan-prefix-bug

## Bug

When `agent-doc repair` applies an "orphaned response" (found via `Found orphaned response for <FILE> (N bytes). Applying...`), it prefixes every body line with `❯ ` — the user-prompt marker. This corrupts the assistant response content.

## Reproduction

1. Session document in template mode with `<!-- agent:exchange patch=append -->`.
2. Direct Edit-based patchback of a response (e.g., editing the file to replace a test placeholder with the real response body).
3. Run `agent-doc write --commit <FILE>` — it detects the orphaned response and applies it.
4. Result: every paragraph/line of the response body gets `❯ ` prefix.

Live repro: `tasks/software/tsift.md` 2026-05-31 cycle for `#health-dedup-scc`. The edit tool replaced a test "Hello world" response with the real content, then `agent-doc write --commit` repaired it with `❯ ` on every line.

## Root Cause Hypothesis

The repair path in `write.rs` (or the template normalization step) applies `strip_user_prompt_prefix` or equivalent logic that treats the response body as user-prompt content and adds the `❯ ` prefix. The orphan-detection code may be reading the response from the document diff and classifying it as a user prompt block rather than an assistant response.

The prefix `❯ ` matches the IntelliJ/JetBrains terminal prompt character and is used by agent-doc's `prompt_prefix` stripping logic. The repair code path likely runs `normalized_response_lines` or similar with the wrong prompt-side flag.

## Fix Direction

1. **Trace the orphan-repair code path** in the agent-doc binary (likely `write.rs` or `repair.rs`): find where the response body is read from the document diff and where `❯ ` gets injected.
2. **Ensure the repair path preserves the response body verbatim** — it should not apply prompt-prefix transformation to assistant response content.
3. **Add a deterministic regression test**: construct a document with an orphaned response in the exchange (no boundary), run repair, assert no `❯ ` prefix on body lines.

## Related

- `#jb-compact-malformed-response-commit` — compact summary committed with `❯` prefix (similar prefix injection, different trigger)
- `#finalize-retry-ipc-response-duplication` — multi-retry finalize prefixes body lines with user-prompt marker
- `#prompt-duplicated-while-typing` — `❯` prefix mismatch in prompt dedup

## Scope

Binary-only fix in the agent-doc Rust crate. No spec changes needed.
