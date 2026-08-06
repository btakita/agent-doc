---
name: reference_agent_doc_rename_session_doc_midcycle_scrambles_exchange
description: "Renaming an agent-doc session document mid-cycle both deadlocks preflight/write on the new path AND scrambles exchange ordering (response body above the prompt, orphaned `### Re:` header)"
metadata: 
  node_type: memory
  type: reference
  originSessionId: b42cc175-cf2a-4e9a-8b1d-3060699facbe
  modified: 2026-07-31T15:45:32.731Z
---

Seen 2026-07-31 on `tasks/professional/mary-elle-zellerbach.md` → `mary-ellen-zellerbach.md`.
Renaming the session document while a cycle is in flight produces **two** distinct failures:

**1. Gate deadlock on the new path.** `preflight <new>` fails with
`found likely direct response patchback without agent-doc cycle: ### Re: <topic>` — the
`### Re:` block is in the working tree but the *new path's* HEAD predates it. `write --commit
<new>` then refuses with the already-committed pre-write gate. Same circle as
[[reference_agent_doc_gate_deadlock_reset_from_current_and_id_bearing_re_block]] case 1b, and the
same escape works: verify live == disk (`agent-doc read <FILE>` vs the file, full diff), then
`agent-doc reset --from-current --preserve-session <FILE>`, then `agent-doc commit <FILE>`,
then `session-check`.

**2. Scrambled exchange ordering — the part the deadlock recovery does NOT fix.** The write that
raced the rename logged `Current document changed since response baseline. Rebasing the captured
intent onto current authority...` plus a `force-disk current exchange fast path` boundary insert,
and a post-commit CAS race (`compare-and-swap raced for detached document`). Result: the response
**body** was appended ABOVE the user prompt and the `### Re:` header was left stranded alone at
the bottom of `agent:exchange`, immediately before the boundary marker. No text is lost — it is
purely an ordering break, and `session-check` reports **ok** on it, so the green check is not
proof the exchange reads correctly. **Read the exchange section after any rename-adjacent
recovery.**

Repair: hand-rewrite `agent:exchange` into prompt → `### Re:` header → body order (this is the
sanctioned "insert missing user prompt" exception, and it is safe here because the drift is
ordering-only), then `reset --from-current --preserve-session` + `preflight`. Preflight picks the
next prompt up normally and the cycle resumes through `agent-doc respond`.

Also: the old path stays staged as a deletion (`D `) in the shared superproject index — leave it
for whoever next commits that path rather than sweeping it in (see
[[reference_shared_superproject_index_contamination_commit_only]]).

**Recurs without any rename.** Seen twice in the same session on the same document. The reliable
tell is `[template] force-disk current exchange fast path inserted boundary <hash>` in the
`respond`/`write` log — whenever that line appears (with or without a preceding
`Current document changed since response baseline` rebase), assume the response block landed ABOVE
the pending prompts and check the exchange. In the second occurrence `session-check` even caught it
indirectly, erroring `INTERRUPTED: ... document still has unresolved prompt-bearing user changes`
for a prompt the response had in fact already answered — because the prompt now sat *after* the
response. Do not re-answer it; fix the ordering instead.

Repair loop that works every time: hand-move the prompt block above the `### Re:` header with two
`Edit` calls (delete from the bottom, re-insert before the header), then
`reset --from-current --preserve-session` + `agent-doc commit` + `session-check`.

Prevention: rename the session document only between cycles, after `session-check` is ok.
