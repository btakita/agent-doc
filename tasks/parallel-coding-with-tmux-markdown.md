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
