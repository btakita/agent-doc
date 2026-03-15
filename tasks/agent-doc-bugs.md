---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

In tasks/briantakita.me.md, the CRDT created duplicate data. Please add tests to replicate. Fix bug.
```
Why were the videos not public to begin with?
Cork

Why were the videos not public to begin with?
Corky should always publish public videos...unless explicitly to private.
```

<!-- agent:exchange -->

### Re: brookebrodack-dev CRDT duplicate

**New bug found** — different from the v0.17.1 exchange append mode fix.

**Root cause:** CRDT `merge()` in `crdt.rs` — when the base state is stale (from previous write cycle) and both `ours` and `theirs` share text added since the base was saved, the CRDT treats each side's addition as independent and includes both, duplicating shared content.

**Scenario:**
1. CRDT base from previous cycle: exchange ends at "Committed and pushed."
2. User types prompt: "Opening a video a shows video a..."
3. Agent generates `content_ours` = previous + user prompt + agent response
4. File on disk = previous + user prompt
5. CRDT merge sees user prompt added by BOTH sides relative to stale base → duplicates it

**Fix:** Added "shared prefix advancement" in `merge()` — detects when ours and theirs share a common prefix longer than base, advances base to that shared prefix (line-boundary snapped). Shared content is then treated as "already in base" rather than two independent insertions.

**Tests:** 2 regression tests in `crdt.rs`. `make check` passes (337 tests, 0 failures).

Ready to publish when you want.

publish
<!-- /agent:exchange -->
