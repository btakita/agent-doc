# Plan — stop `❯ ` prompt prefixes from leaking into assistant verification lists

**Bug source:** `tasks/software/tagpath.md`, 2026-05-07  
**Related:** `plan-skill-finalize-prompt-prefix-strip.md`, `plan-prompt-prefix-stripped-after-closeout.md`, `plan-sidecar-prefix-normalization-loss.md`

## Problem

During `agent-doc` closeout and subsequent repair/replay paths, assistant-owned bullet lists inside a response body can incorrectly receive the binary-owned `❯ ` prompt prefix. The observed shape was inside a `### Re: ...` response after `Verification:`:

```md
Verification:
❯ - Passed focused tests:
❯   - `cargo test ...`
❯ - `cargo test` is still red on pre-existing repo failures ...
```

Those lines are assistant prose, not prompt-target lines. The `❯ ` prefix should only be applied to user prompt lines in the exchange user region.

## Why This Matters

- It corrupts the visible transcript shape and makes assistant output look like fresh user prompts.
- It can trigger false `prompt_target` / `content_edit` interpretations on the next preflight cycle.
- It makes `session-check` / repair loops harder to reason about because prompt-prefix normalization is no longer scoped to actual prompt lines.

## Likely Root Cause Areas

1. **Over-broad `normalize_prefix_lines` targets**  
   The binary may be deriving normalization targets from repaired working-tree drift that already includes assistant list items, then reapplying `❯ ` to any trimmed line match.

2. **Editor/plugin normalization matches by raw trimmed text only**  
   The JetBrains / VS Code prefix normalization path may be applying prefixes to matching assistant lines because it does not prove the line still belongs to the prompt-bearing user region of the exchange.

3. **Repair/adopt-current replay is feeding assistant lines into prompt-prefix normalization**  
   Post-commit repair, `(HEAD)` cleanup, or replay adoption may be reusing comparison-only prompt normalization helpers on the full exchange block instead of limiting them to the prompt section before the corresponding `### Re:` response.

4. **Session-check/preflight repair heuristics are classifying assistant bullets as prompt-bearing edits**  
   Once a prefixed assistant bullet lands in the working tree, later repair passes may preserve or amplify it because the invariant no longer distinguishes prompt lines from assistant list items.

## Questions To Answer

- Where were the leaked `❯ ` prefixes first introduced:
  - during `finalize` / `write --commit`
  - in plugin-side IPC application
  - in post-commit working-tree repair
  - in preflight repair on the next cycle
- What exact `normalize_prefix_lines` payload was emitted for the `tagpath.md` reproduction?
- Does the plugin normalization algorithm scope matches to the user region structurally, or just to raw line text before the boundary marker?
- Which binary path allowed assistant lines under `Verification:` to be treated as prompt canonicalization targets?

## Implementation Steps

- [ ] Reconstruct the `tagpath.md` reproduction from logs, snapshot/HEAD diffs, and any persisted IPC payloads.
- [ ] Trace `normalize_prefix_lines` generation in `write.rs` and any post-commit repair paths in `git.rs` / `repair.rs`.
- [ ] Prove whether assistant bullet lines after `Verification:` can enter the normalization target set.
- [ ] Audit editor-side normalization in JetBrains and VS Code to ensure it cannot prefix assistant lines even if the payload is imperfect.
- [ ] Tighten the invariant so only actual prompt-target lines are eligible for `❯ ` repair.
- [ ] Add regression coverage for a response body containing:
  - `Verification:`
  - nested assistant bullet lists
  - nearby real prompt lines in the same exchange
- [ ] Add a fail-closed test so replay/repair paths reject any normalization pass that would mutate assistant-owned list items.

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt`
- `src/agent-doc/editors/vscode/src/extension.ts`
- `src/agent-doc/editors/PLUGIN-SPEC.md`

## Exit Criteria

- Assistant bullet lists after `Verification:` remain plain `-` bullets through finalize, replay, repair, and post-commit normalization paths.
- `❯ ` prefix repair still works for real user prompt lines in the same exchange.
- Regression coverage proves the `tagpath.md` shape cannot recur.
