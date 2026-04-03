---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Sync auto-start bug fixed. Unclaimed files no longer spawn sessions on tab navigation.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260403-150156.md`*
In tasks/finance/xdgboost.md, I typed in a prompt but the diff was not detected. Why?

Typed in:
> Tell me about xdgboost. What is it used for? How does it work?

```
❯ /agent-doc tasks/finance/xdgboost.md

● Bash(agent-doc --version 2>/dev/null || echo "not installed")
  ⎿  agent-doc 0.31.2

● Bash(agent-doc preflight tasks/finance/xdgboost.md)
  ⎿  [preflight] step 0: layout check
     [preflight] layout issue: stash window 'stash' has non-idle pane %386 running 'agent-doc'
     [preflight] layout issue: stash window 'stash' has non-idle pane %396 running 'agent-doc'
     … +39 lines (ctrl+o to expand)

● No changes detected since the last snapshot. The document was just committed for the first time — the question in the
   exchange component is now part of the baseline.

  Edit the document (e.g., add a space or newline in the exchange) and re-run /agent-doc tasks/finance/xdgboost.md so
  the diff picks up the question.
```
### Re: xdgboost.md diff not detected — snapshot includes the prompt

The snapshot (442 bytes) already contains the user's prompt. After comment stripping, snapshot and document are identical (409 bytes). The diff correctly reports no changes.

**What happened:** The user typed the prompt into the exchange, then ran `Claim for Tmux Pane`. The claim (`agent-doc claim`) scaffolded components and committed — the snapshot was saved with the prompt already present. So when `/agent-doc` ran next, the diff was empty.

**Fix:** The snapshot should be reset after claim so the next cycle sees the prompt as new content. Or the user can run `agent-doc reset tasks/finance/xdgboost.md` to clear the snapshot, then re-run `/agent-doc`.

For immediate fix:
```bash
agent-doc reset tasks/finance/xdgboost.md
```
Then re-run `/agent-doc tasks/finance/xdgboost.md`.

**Root cause:** `agent-doc claim` saves the full file content as the snapshot (line 196 in claim.rs: `sessions::register_with_pid`). When the user's prompt is already in the file at claim time, it becomes part of the baseline.

This is a design tension: claim needs a snapshot for the gutter boundary, but the snapshot should not absorb user input that hasn't been responded to yet.
<!-- agent:boundary:9d6063a4 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] v0.31.2 released
- [ ] Fix: claim should not absorb unresponded user input into snapshot
<!-- /agent:pending -->
