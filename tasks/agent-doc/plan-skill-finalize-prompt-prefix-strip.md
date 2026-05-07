# Plan — skill finalize strips prompt prefix on preset-expanded prompts

**Bug source:** `tasks/buildparty-investor-demo/dev.md`, 2026-05-05
**Related:** `plan-prompt-prefix-stripped-after-closeout.md` (same class, different code path)
**Prior fix:** `#spfxnorm` (`842c6bd`) repaired post-commit prefix drift in `write.rs` (IPC) and `git.rs` (no-listener) — but this reproduction shows the bug persists.

## Problem

In the `dev.md` session, the user wrote:
```
spec-test-news-commit-push
```
This is a prompt preset that expands to multi-step work. After the skill's `finalize --stream` committed the response, the working-tree file had the `❯ ` prefix stripped from the `spec-test-news-commit-push` line. The line appeared as bare `spec-test-news-commit-push` instead of the canonical `❯ spec-test-news-commit-push`.

Preflight's repair step fixed it on the next cycle, but the previous cycle's `session-check` (which runs inside `finalize`) did not fail closed on the stripped prefix.

## Reproduction Context

- Document: `tasks/buildparty-investor-demo/dev.md` (template mode, CRDT write, IPC)
- Harness: Claude Code skill (`/agent-doc`)
- Write path: `finalize --stream --origin skill` with `--baseline-file`
- The prompt line `spec-test-news-commit-push` appears inline (appended to the previous user prompt block, not as a standalone `## User` section)
- The response was committed successfully — the prefix was stripped in the post-commit working-tree state

## Root Cause Hypotheses

1. **IPC patch delivery strips prefixes before re-rendering**: the `normalize_prefix_lines` repair added in `#spfxnorm` runs after commit, but the IPC content delivery may write a pre-normalized version of the exchange that lacks the `❯ ` prefix on prompt-preset lines
2. **Sidecar normalization divergence**: the finalize log showed `sidecar normalization diverged — falling back to content_ours`. The `content_ours` fallback may lack prompt-prefix canonicalization that the sidecar path would have applied
3. **Boundary synthesis on inline prompts**: when a prompt-preset line like `spec-test-news-commit-push` is appended to the tail of an existing exchange block (not a new `## User` section), the boundary synthesis may not recognize it as a prompt-target line needing `❯ ` canonicalization
4. **`session-check` prefix guard gap**: the guard that "fails closed on likely bypassed patchbacks that leave bare prompt-target lines without the binary-owned `❯ ` prefix" may not fire when the prefix was present in the committed blob but stripped in the working tree after commit

## Questions To Answer

- Does the committed `HEAD` blob have the `❯ ` prefix, or was it stripped before commit?
- If `HEAD` has the prefix, which post-commit path stripped it in the working tree?
- Did `session-check` run on the working-tree state or the committed state?
- Is `content_ours` fallback missing the prefix-repair pass that the normal write/IPC path has?
- Does boundary synthesis treat inline prompt-preset lines differently from standalone `## User` blocks?

## Implementation Steps

- [ ] Reproduce: write a prompt-preset line inline at the end of an exchange block, run `finalize --stream`, check whether the working-tree file preserves `❯ ` after commit
- [ ] Trace the `content_ours` fallback path in `write.rs` — verify it applies `normalize_prefix_lines` before writing the working tree
- [ ] Trace the IPC `normalize_prefix_lines` repair — verify it covers inline prompt-preset lines (not just standalone user sections)
- [ ] Trace `session-check` — verify it compares the working-tree state (not just the committed blob) against the prompt-prefix invariant
- [ ] Fix: ensure every post-commit working-tree write path applies prompt-prefix canonicalization, including the `content_ours` sidecar fallback
- [ ] Add regression test: inline prompt-preset line at exchange tail → finalize → working tree must keep `❯ `
- [ ] Verify `session-check` fails closed if the working tree loses `❯ ` after a successful commit

## Files Likely To Change

- `src/agent-doc/src/write.rs` — `content_ours` fallback path, IPC normalization
- `src/agent-doc/src/git.rs` — post-commit working-tree cleanup
- `src/agent-doc/src/session_check.rs` — prefix guard scope (committed vs working tree)
- `src/agent-doc/src/template.rs` — boundary synthesis for inline prompt-preset lines

## Exit Criteria

- The `dev.md` reproduction (inline `spec-test-news-commit-push` prompt-preset) no longer strips `❯ ` after finalize
- `session-check` fails closed if the working tree loses prompt prefixes after commit
- Regression test covers the inline prompt-preset path
