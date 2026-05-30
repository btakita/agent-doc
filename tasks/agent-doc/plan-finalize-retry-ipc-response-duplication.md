# Plan — multi-retry finalize + live IPC duplicates the response and `❯`-prefixes its body lines

> Status: PLAN ONLY — do not implement. Filed via `#agent-doc-bug` from a live repro on `tasks/agent-doc/agent-doc-bugs2.md`.

## Symptom (observed 2026-05-29, live)

During a single `agent-doc` turn whose closeout took **several finalize/write retries** (each forced by a `lint-gate` block or a post-commit `session-check` interruption), with the IDE IPC listener active (`ipc.sock`) and the user editing the document concurrently, the assistant response in `agent:exchange` became corrupted in the working tree:

- The response block was **duplicated** — a stale earlier-retry copy (`### Re: … — opus-4-8`) plus the current `### Re: … (HEAD)` copy.
- Several **agent response body lines were prefixed with `❯ `** (the user-prompt prefix), e.g. `❯ - New deterministic regression …`, `❯ **Scope/honesty:** …`, `❯ **Commits:** …`. Body prose is not a prompt, so the prefix is wrong.
- The corruption was **working-tree/live-buffer only** — HEAD held a single clean committed copy. The on-disk file thrashed between samples (heading count flipped 2→0 while `❯` lines persisted), indicating the IDE live buffer was rewriting the file across retries.
- Net effect: `session-check` fail-closed (`bare prompt target missing ❯: **Verification:**` / `direct response patchback without agent-doc cycle`), blocking closeout until a manual `git checkout HEAD -- <doc>` repair (HEAD was clean, so the restore was lossless).

## Repro shape

1. Open a template doc in the IDE (IPC listener active); keep typing/edit concurrently.
2. Drive a finalize whose first attempt fails a gate (lint-gate false positive, `pending_done_guard`, or `live_prompt_drift_after_preflight`), forcing 2–4 `finalize` / `write --commit` re-runs in quick succession.
3. Each retry pushes the same response through IPC and receives `already_applied`; combined with the live buffer's prior copy and a fresh pre-patch boundary, the buffer accumulates a duplicate response copy and mis-prefixes body lines with `❯ `.

## Root-cause hypotheses (confirm during implementation)

1. **Retry non-idempotence against the live buffer.** A finalize/write retry that gets IPC `already_applied` should *converge* the buffer to the single canonical response, not append/compound. Repeated `pre-patch boundary … inserted` + `already_applied` across retries is the likely compounding path.
2. **`❯ ` mis-attribution of agent body lines.** The boundary reposition / prompt-region detection treats response body lines below a (mis-placed or duplicated) boundary as user-prompt region and prefixes them with `❯ `. Agent response body must never receive the `❯ ` prompt prefix.
3. **`live_prompt_drift_after_preflight` across retries.** The `content_ours_snapshot_next_cycle` recovery leaves disk/buffer diverged; a subsequent retry re-bases on a diverged buffer and duplicates.

## Proposed fix (decide during implementation)

1. **Idempotent retry convergence:** when IPC reports `already_applied` (or the response body is already present), the write path must converge the live buffer to exactly one canonical response block (dedupe duplicate `### Re:` headings sharing a topic) instead of leaving/adding a second copy.
2. **Never `❯ `-prefix agent response body:** the prompt-prefix normalization must be scoped to genuine user-prompt lines (above the active boundary), never to agent response body lines.
3. **Repair/session-check auto-dedup:** detect duplicate response headings with the same topic and/or `❯ `-prefixed response body lines and auto-dedupe in `repair` (the recovery that currently requires a manual `git checkout`).
4. Relates to and may share root cause with `#prompt-duplicated-while-typing`, `#duplicate-response-after-commit`, `#ipcfullprompt-recur2`, and the PromptPoller double-tracking fix `#ipc-dup-tracking-double-merge` (plugin 0.2.138, not yet installed during this repro). Confirm whether installing 0.2.138 eliminates the live-buffer compounding before adding new CLI-side machinery.

## Regression test

- SimWorld/unit: two consecutive `write` retries with `already_applied` IPC acks for the same response produce exactly one response block (no duplicate heading, no `❯ `-prefixed body lines).
- Assert prompt-prefix normalization never prefixes a line inside the agent response region.
- Assert `repair` dedupes a pre-corrupted doc (duplicate heading + `❯ `-prefixed body) without a manual `git checkout`.
