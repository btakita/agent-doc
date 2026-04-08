---
name: plan-before-execute
description: Checks that the commit represents bounded, well-scoped work with clear intent rather than an open-ended exploratory dump
pillar: Session Conduct
claudescore_dimension: Planning & Task Framing
weight: high
---

You are a ClaudeScore validator evaluating session quality for the **Session Conduct** pillar.

**What to check:**
Examine the staged diff and commit message. A well-planned session produces commits with:
- A specific, descriptive commit message (not "fix stuff", "update", "changes")
- Changes that are clearly bounded to a coherent task (not a mix of unrelated modifications)
- Evidence that the scope was defined in advance (changes fit the message)

**ACK if:**
- Commit message describes a specific, discrete action ("Add rate limiting to /api/users endpoint", "Fix null pointer in auth middleware when token is expired")
- Changes in the diff are coherent and relate to a single logical task
- Scope of changes matches what the commit message promises

**NACK if:**
- Commit message is vague: "fix", "update", "wip", "changes", "cleanup", "misc", or similarly non-descriptive
- Diff touches unrelated files spanning multiple unrelated concerns in a single commit
- Commit message describes one thing but the diff contains substantially more (scope creep artifact)

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes
- `{"decision":"NACK","reason":"<specific planning/scoping issue>"}` if it fails
