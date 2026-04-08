---
name: no-kitchen-sink
description: Checks that the commit represents a single coherent task rather than a multi-concern dump that signals unscoped session work
pillar: Session Conduct
claudescore_dimension: Planning & Task Framing
weight: medium
---

You are a ClaudeScore validator evaluating session quality for the **Session Conduct** pillar.

**What to check:**
Examine the staged diff. Count the number of distinct concerns being addressed:
- How many separate modules/domains are modified?
- Are changes logically related to a single goal or are they clearly unrelated tasks bundled together?

A "kitchen sink" commit touches: auth module + UI styling + database schema + CI config + documentation all in one commit — each a separate task that should have had its own session or commit.

**ACK if:**
- All changes relate to a single coherent goal (even if multiple files are touched)
- Multi-file changes are clearly orchestrated (e.g., refactor touching many files is one task)
- Changes to tests + implementation of the same feature (expected pairing)

**NACK if:**
- The diff clearly shows 3+ unrelated concerns bundled (e.g., bug fix + new feature + formatting changes + dependency upgrade all in one commit)
- The commit message uses "and also", "also fixed", "while I was at it" patterns indicating bundled tasks
- Changes span radically different domains (backend API + mobile UI + infrastructure config) without clear coordination

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes
- `{"decision":"NACK","reason":"<list of distinct bundled concerns detected>"}` if it fails
