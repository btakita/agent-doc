---
agent_doc_session: plan-duplicate-response-after-commit
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
---

# Plan: duplicate committed response re-inserted by next cycle

## Symptom

After a clean `finalize` cycle that reaches `committed` (commit hash printed, `session-check` OK), the **next** invocation of `agent-doc preflight` interrupts with:

```
found likely direct response patchback without agent-doc cycle:
### Re: <last-response-heading> — <model>;
tracked side-effect edits: src/boost-client, src/tmux-router,
tasks/agent-doc/agent-doc-bugs2.md (+2 more)
Use `agent-doc write --commit <FILE>` once the visible response body is final,
then re-run `agent-doc session-check <FILE>`.
```

Recovery via the suggested command fails with:

```
Error: empty response — nothing to write
```

Manual inspection of the working tree shows the **same response body** committed in HEAD also re-inserted as uncommitted content, fenced by **two identical** `<!-- agent:boundary:<id> -->` markers wrapping the duplicate. The exchange tail ends with the duplicate plus a redundant boundary marker before `<!-- /agent:exchange -->`.

## Repro (observed)

Session: `tasks/dreamable/separation.md`. agent-doc 0.34.0.

Cycle N (the prior cycle) finalized cleanly:

```
[main 231eafc83] agent-doc(separation): 2026-05-28 22:22:04
[commit] IPC reposition boundary signal sent
[commit] VCS refresh signal written
```

`session-check` passed and printed the standard recommendation-warn footer. No user edits between cycle N's success and cycle N+1's start, except the user typed the next agent-doc invocation.

Cycle N+1's preflight interrupted with the message above. `git diff tasks/dreamable/separation.md` showed the full cycle-N response body re-inserted at the tail with `<!-- agent:boundary:88409761 -->` wrapping it on both sides.

Manual remediation: Edit the duplicate block out (including both boundary markers), then re-run preflight — which reports `no_changes: true` and a clean tree.

## Likely root cause (hypothesis)

The `[commit] IPC reposition boundary signal sent` step in cycle N likely fired *after* the IPC listener / VCS refresh path had already re-applied the committed response to the working-tree file as if it were a fresh patch. The boundary signal then inserted a *second* boundary marker around the late-replayed content, leaving the tree with:

```
<original tail>
<!-- boundary:XXXX -->
<duplicate response body>
<!-- boundary:XXXX -->
<!-- /agent:exchange -->
```

The next cycle's `session-check` correctly identified the second copy as a "direct response patchback without agent-doc cycle" because (a) the body matches HEAD's commit content but (b) it sits *outside* HEAD as working-tree drift.

The "+2 more" in the tracked side-effect-edits diagnostic plus the listed `src/boost-client`, `src/tmux-router`, `tasks/agent-doc/agent-doc-bugs2.md` paths suggest the same IPC late-replay also touched unrelated paths whose own snapshots are still ahead of HEAD — those paths appear in every cycle's preflight `sweep` output, so they may share the same IPC reposition reentrancy issue.

## Why `agent-doc write --commit` can't recover

`write --commit` reports `empty response — nothing to write` because the duplicated body is already inside the live exchange — there is no new response delta to apply. The repair tool assumes "missing response needs to be written"; here the response is *over-applied*, not missing. There is no in-band repair for an over-applied response.

Manual edit + re-preflight is the only working recovery path today.

## Proposed fix directions

Two complementary changes:

1. **Detect over-application in `session-check`'s "direct response patchback" branch.** If the patchback delta's content matches the most recent commit's response content byte-for-byte (or after stripping `<!-- agent:boundary:* -->` markers), classify it as a **late-IPC duplicate** rather than a manual patchback. Auto-remediation: rewrite the working tree to drop the duplicate region between the two matching boundary markers and re-run preflight — cleanly, without operator intervention.

2. **Make the IPC reposition boundary signal idempotent and ordered.** Track the commit hash that owned each boundary marker. When the listener attempts to insert a marker for a hash already present in HEAD's exchange tail, skip the insertion instead of double-stamping. Investigate the race between `commit total` completing and the IPC listener's catch-up replay — the symptom suggests the listener replayed against a pre-commit snapshot view of the file.

Option A (recommended): both 1 and 2. Item 1 unblocks operators immediately; item 2 eliminates the root cause.

Option B: just 1 (recovery only). Cheaper but leaves the duplicate-insertion bug present and operators paying the cost when it triggers.

Option C: just 2. Better long-term but leaves any in-flight stuck sessions needing manual recovery.

## Acceptance criteria

- Reproducer: a cycle that exercises the IPC reposition path twice in quick succession produces a single boundary marker per response, never two.
- `session-check` on a tree with duplicated post-commit content recognizes it and auto-remediates (or at least produces a `write --commit --repair-duplicate` command that succeeds, instead of the current `empty response` dead end).
- The "+N more" side-effect-edit list in the interrupt message either disappears (preferred) or is explained in the diagnostic so operators know whether those listed paths need their own remediation.

## Out of scope

- Whether session-accretion `block` level should pause cycles independently — separate concern.
- The earlier `harness_mismatch` warning path — unrelated.
