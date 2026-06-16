# Plan - response recovery drift must not become a user prompt

## Problem

During the `tasks/professional/equityfundingsource.md` closeout on 2026-06-16,
part of the assistant response/proof text was left in the working tree during
fallback recovery and then re-entered the document as prompt-bearing text. The
visible symptom was a large duplicated response region prefixed with `❯`, which
agent-doc treated as a fresh user prompt instead of committed assistant-owned
closeout content.

This produced a second command-replay closeout even though the original work was
already completed, committed, pushed, deployed, and backfilled.

## Observed recovery path

1. MCP `agent_doc_finalize` transport closed during final closeout.
2. CLI fallback used `agent-doc write --commit` / `agent-doc finalize`.
3. The first response was committed, but a repair/session-check pass duplicated
   a large response/proof tail into the working tree with `❯` prompt prefixes.
4. `write --commit` reported an already-applied or no-op style recovery while
   `session-check` still saw a prompt-bearing closeout tail.
5. The duplicated assistant response was picked up as a user prompt, requiring a
   follow-up command-replay closeout to settle the document.

## Root-cause hypotheses

- Closeout repair/adoption is classifying duplicated response-owned patchback or
  proof lines as user-authored prompt text when HEAD, boundary markers, and
  live-editor/IPC snapshots drift.
- The already-applied recovery path can commit or preserve a repaired working
  tree whose prompt prefixes were applied after ownership was lost.
- Prompt-prefix normalization is running without a strong response-region
  invariant in one recovery path. Once the response tail receives `❯`, later
  prompt-tail detection trusts the prefix more than the response boundary.
- Stale supervisor or degraded IPC proof may widen the race by replaying an old
  snapshot after the binary has already committed the assistant response.

## Fix plan

1. Add a minimal regression fixture for the equityfundingsource failure shape:
   committed assistant response plus a duplicated working-tree response/proof
   tail after the latest exchange boundary, where the duplicate copy is
   `❯`-prefixed.
2. Teach the post-commit drift classifier to detect response-owned duplicate
   tails by comparing normalized text against the committed response body:
   strip `❯`, `(HEAD)`, boundary id changes, and known proof-label metadata
   before deciding whether the tail is new user input.
3. If the normalized tail is duplicate assistant-owned content, repair by
   dropping the duplicate or reattaching it as assistant content without prompt
   prefixes. Do not route it as a fresh queue/prompt item.
4. Fail closed when ownership cannot be proven. The diagnostic should say that
   response recovery drift is ambiguous, and include response hash, boundary id,
   snapshot source, and IPC/supervisor proof state.
5. Harden `write --commit` already-applied recovery so a successful no-op
   recovery cannot leave prompt-prefixed response lines in the working tree.
6. Add ops-log markers for the branch decisions:
   `response_recovery_duplicate_repaired`,
   `response_recovery_ambiguous_fail_closed`, and
   `already_applied_response_tail_rejected`.

## Acceptance criteria

- A committed assistant response duplicated with `❯` prefixes is either removed
  or restored as assistant-owned content; it is never surfaced as a user prompt.
- A truly new user prompt after closeout still interrupts the cycle and is not
  swallowed by duplicate-response repair.
- `agent-doc session-check` after fallback closeout returns clean for duplicate
  response/proof replay, and fails closed for ambiguous mixed user+assistant
  tails.
- Regression coverage includes socket-finalize failure, file-IPC fallback, and
  `write --commit` already-applied recovery.
- Targeted tests and full `make check` pass before implementation is marked
  done.

## Non-goals

- Do not change queue-drain behavior for unrelated active prompts.
- Do not weaken the prompt-tail guard for genuinely new user input.
- Do not rely on editor plugin behavior alone; the CLI/session-check path must
  be correct when IPC is degraded.
