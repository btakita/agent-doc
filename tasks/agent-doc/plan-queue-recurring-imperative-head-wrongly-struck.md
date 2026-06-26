# Plan: recurring imperative queue heads (`deploy`/`commit`/`push`/`build`) wrongly auto-struck instead of processed

## Problem

An operator added a free-text `deploy` head to the `agent:queue`. Instead of
being **executed** as a fresh directive, it was **auto-struck** (`~~deploy~~`)
and never processed. Two independent strike mechanisms fired on it:

1. **`#qftbklgstrike` (lexical done/backlog match)** —
   `agent-doc-orchestration/src/memory_cmd.rs`. The deterministic `rank_events`
   lexical scorer matched the head `deploy` to a completed `agent:done` item
   (observed: `auto-struck free-text head matched=done #do-id-closeout-open-backlog (2.050) (#qftbklgstrike): "deploy"`)
   above the conservative auto-strike threshold (`AUTO_STRIKE_THRESHOLD`, see
   `memory_cmd.rs` around the threshold/`best match per head` logic). A single
   common verb like `deploy` is lexically close to *many* prior done items
   ("...commit + push + deploy", "deploy:worker", etc.), so it clears the
   threshold and is struck as "already done."

2. **`#qheadresidue` (committed-exchange echo)** —
   `agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs`.
   Once a response quoted the head as a `> **Queue prompt:**` echo, the residue
   guard treated the still-active head as answered and struck it.

The shared root cause: **recurring imperative commands** (`deploy`, `commit`,
`push`, `build`, `install`, `release`, `test`, `sync`, `recycle`, …) are
**executable directives**, not one-time answerable tasks. They are valid every
time they are queued and must never be silently retired by fuzzy "done"/residue
matching. Striking `deploy` as done-equivalent drops the operator's instruction
with no deploy ever run — a silent loss of an executable directive.

This is distinct from the legitimate `#qftbklgstrike` case (a free-text head
that *restates* a specific, already-completed tracked task — e.g.
"add the lender email parity" matching a done `#lender-card-msg-email`). The
failure is the **generic recurring verb** subclass.

## Fix

1. **Recognize recurring-imperative heads and exempt them from auto-strike.**
   In `memory_cmd.rs`, before `#qftbklgstrike` strikes a free-text head, classify
   it: a head whose normalized text is (or is dominated by) a known recurring
   imperative command verb (`deploy`, `commit`, `push`, `build`, `install`,
   `release`, `test`, `sync`, `recycle`, `publish`, plus the preset tokens like
   `#spec-test-commit-push`) is an **executable directive**, never a
   "restatement of tracked work." Skip both the `Done` and `Backlog` match for it.
   Reuse / extend the existing imperative-directive detection the skill already
   relies on (the "imperative edits are executable directives" contract) so the
   verb list has one source of truth.

2. **Same exemption in the `#qheadresidue` residue guard.** In
   `queue_head_provenance_guards.rs`, a recurring-imperative head must not be
   treated as "answered residue" merely because a response echoed it — a
   completed deploy cycle does not retire a standing `deploy` directive. The
   guard should only strike free-text heads that are genuine one-time prompts.

3. **Process, don't strike.** A recurring-imperative head that reaches
   convergence un-actioned should remain a **drainable** head (so the in-session
   loop / supervisor dispatches it through the normal command path —
   `queue_dispatch.rs` classifies `deploy`-style heads as commands), not be
   retired. Only strike it once the directive has actually been dispatched /
   executed this cycle (the existing answered-this-turn consume path), not by
   lexical similarity to history.

4. **Raise the lexical threshold and/or require an id-anchored match for the
   `Done` kind.** As a defense-in-depth measure independent of the verb list,
   `#qftbklgstrike` should not strike a *short* free-text head (≤ N tokens) on a
   `Done` match unless the match is strongly anchored (high score AND shared
   distinctive tokens, not just a shared common verb). A one-word head should
   never be auto-struck by a multi-word done item.

## Tests

1. **SimWorld / unit (memory_cmd):** a queue with a free-text `deploy` head and a
   done `agent:done` item containing the word "deploy" (e.g.
   `#do-id-closeout-open-backlog ... commit + push + deploy`) must NOT produce a
   `QueueStrikeMatch` for the `deploy` head. Assert `select_queue_strike_matches`
   (or equivalent) returns no match for recurring-imperative heads.
2. **Unit (memory_cmd) — non-regression:** a free-text head that genuinely
   restates a specific done item (multi-token, distinctive overlap, e.g.
   "email-to-lender parity for account messages" vs done `#lender-card-msg-email`)
   STILL auto-strikes. The fix must not weaken the legitimate restatement case.
3. **Unit (queue_head_provenance_guards):** a committed exchange that echoes a
   `> **Queue prompt:** deploy` blockquote must NOT raise the `#qheadresidue`
   INTERRUPTED guard for a `deploy` head — the head stays active/drainable.
4. **Unit (short-head threshold):** a one-word free-text head never auto-strikes
   on a `Done` match regardless of score.
5. **Integration (convergence):** preflight convergence over a queue with an
   un-actioned `deploy` head leaves it active with
   `queue_drainable_head_count >= 1` and `queue_continuation_required = true`
   (dispatchable), rather than striking it.

## Evidence (this session, 2026-06-26, equityfundingsource.md)

- `[preflight] queue: auto-struck free-text head matched=done #do-id-closeout-open-backlog (2.050) (#qftbklgstrike): "deploy"`
- Later `session-check INTERRUPTED ... "deploy" ... already answered (#qheadresidue)` after a response echoed it.
- Net effect: the operator's `deploy` queue directive was retired (`~~deploy~~`)
  without the queue head itself being processed as a fresh deploy directive.
