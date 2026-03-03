---
session: fa2a5de1-dafd-421c-a264-844c0008e68b
resume: null
agent: null
model: null
branch: null
---

Help me draft a reply to https://news.ycombinator.com/reply?id=47224082&goto=threads%3Fid%3Dbriantakita%2347224082
> The "don't sync, own" model makes a lot of sense. We were thinking about it wrong - trying to push state out to a shared file, when the cleaner move is to pull it in on demand.
> The SPEC.md as the extraction target after a feature is done is a nice touch. In our case the tricky part is that browser automation state is partly external - you have sessions, cookies, proxy assignments that live outside the codebase. So the "ground truth" we needed wasn't just about code decisions but about runtime state too. Ended up logging that separately.
>
> Checking out agent-doc, the snapshot-based diff to avoid re-triggering on comments is clever. Does it handle cases where two agents edit the same doc around the same time, or is the ownership model strict enough that this doesn't come up?

So far I have:

> Does it handle cases where two agents edit the same doc around the same time, or is the ownership model strict enough that this doesn't come up?

A document is owned by one session...but one session can own many docs. It is possible to have a race condition where one agent claim a document, starts doing work, then another agent claims the document. Or I'm writing additional prompts while the agent is running. Worst case is the last writer could clobber the output. But I'm also working on that race condition.

## Assistant

Here's a draft reply:

---

The runtime state logging approach makes sense for browser automation — that's a domain where ground truth literally lives outside your repo. We have a similar dynamic with email state in our corky tool (IMAP sync, draft queues). Ended up with the same pattern: log the external state separately and let the document reference it rather than trying to capture it inline.

On concurrent editing — the ownership model is strict by default. Each document is claimed by one tmux pane (one agent session). The `agent-doc claim` command registers ownership, and the snapshot diff only fires for the owning session.

That said, it's not bulletproof yet. Two race conditions exist:

1. Agent A claims a doc, starts working, then Agent B claims the same doc (clobbering the claim)
2. I'm writing additional prompts while the agent is mid-response — last writer wins on the file

Both are "last writer wins" scenarios. In practice I avoid (1) through the routing layer (tmux pane → document mapping), and (2) hasn't bitten me badly because the Edit tool does surgical appends rather than full rewrites. But I'm actively working on proper locking to close both gaps.

The design philosophy is closer to "ownership prevents conflicts" than "handle conflicts gracefully." One session owns many docs, but each doc has exactly one owner at a time. Similar to how you'd avoid two people editing the same Google Doc section simultaneously — except enforced by tooling rather than social convention.

---

A few notes on the draft:

- **Opened with empathy** on their runtime state challenge, connected it to your corky experience (shows you've hit the same problem in a different domain)
- **Honest about limitations** — the race conditions are real, you're working on them
- **Framed the design intent** ("ownership prevents conflicts" vs "handle conflicts gracefully") which answers the spirit of their question
- **Kept it HN-appropriate length** — substantive but not a blog post

Feel free to edit the tone, cut sections, or adjust the corky reference if you'd rather not mention it publicly.

## User

The last writer wins scenario has been mitigated using git commits and diffs. Along with logic. Please verify by checking the logic in agent-doc.

## Assistant

You're right — I overstated the race condition. After checking the agent-doc source (`submit.rs`, `git.rs`), here's what actually happens:

**agent-doc uses a 3-way merge strategy, not "last writer wins":**

1. **Pre-submit git commit** — before sending to the agent, the file is committed to git (immutable baseline)
2. **Race detection** — after the agent responds, the file is re-read and compared to the pre-submit version. If the user edited during the response, it triggers a merge
3. **`git merge-file --diff3`** — merges the baseline, agent response, and user's concurrent edits. Non-overlapping changes merge cleanly; overlapping changes get conflict markers

So the concurrent editing question has a solid answer: within a single session, user edits during agent response are preserved via 3-way merge. Nothing is silently dropped.

The remaining edge case is claim clobbering (Agent A owns a doc, Agent B re-claims it). That's prevented by the routing layer — tmux pane-to-document mapping ensures each doc has exactly one owner.

Here's the **revised draft reply**:

---

The runtime state logging approach makes sense for browser automation — that's a domain where ground truth literally lives outside your repo. We have a similar dynamic with email state in corky (IMAP sync, draft queues). Same pattern: log the external state separately and let the document reference it.

On concurrent editing — it's handled at two levels:

**Ownership:** Each document is claimed by one tmux pane (one agent session). The routing layer prevents two agents from working the same doc simultaneously.

**3-way merge:** If I edit the document while the agent is mid-response, agent-doc detects the change on write-back and runs `git merge-file --diff3` — baseline (pre-commit), agent response, and my concurrent edits all merge. Non-overlapping changes merge cleanly; overlapping changes get conflict markers. Nothing is silently dropped.

The pre-submit git commit is the key — it creates an immutable baseline before the agent touches anything, so there's always a clean reference point for the merge.

---

## User

Is that the full reply?
