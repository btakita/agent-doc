# Plan: Response tail line misclassified as user prompt (❯ prefix leak)

## Problem

During boundary synthesis / finalize, the last line of an assistant response can be incorrectly stamped with the `❯ ` user-prompt prefix. This is a variant of the `content_ours` prefix leak documented in `plan-content-ours-prefix-and-commit-churn.md`.

### Reproduction

In `buildparty-investor-demo/dev.md`, commit `2302c9b6`:

1. Commit `0dc98b12` added the response tail: `All 506 assertions pass. Committed + pushed buildparty-investor-demo and session-share.` (no prefix — correct, this is agent output)
2. In the next cycle, a new user prompt replaced the old one on line 39
3. Commit `2302c9b6` changed the response tail to `❯ All 506 assertions pass...` — incorrectly adding the prompt prefix

The boundary synthesis logic misidentified the response tail as a prompt boundary when processing the new user prompt insertion.

### Relation to #pfxleak2

Same root cause as `plan-content-ours-prefix-and-commit-churn.md` Phase 1: `content_ours` reconstruction does not preserve the prefix/no-prefix state of existing committed lines. When rebuilding the exchange content, lines that existed in HEAD without `❯ ` should never receive it.

## Proposed Fix

Follow Phase 1 of `plan-content-ours-prefix-and-commit-churn.md`:

1. In the `content_ours` fallback path, read the committed HEAD version of the exchange
2. For each line present in HEAD without `❯ `, preserve that state in the reconstruction
3. Only apply `❯ ` to genuinely new user-prompt lines (not present in HEAD)

### Additional check for boundary synthesis

When the write path inserts a new prompt boundary and the preceding line is agent content (no `### Re:` header follows), ensure the preceding line does not receive the `❯ ` prefix.

## Test Cases

1. Finalize with new user prompt after a response tail line → tail line retains no prefix
2. `content_ours` fallback on a document with committed agent response → no `❯ ` on agent lines
3. Multiple successive prompts replaced → only the actual prompt line gets `❯ `
