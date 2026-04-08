---
name: skills-configured
description: Checks that the .claude/skills/ directory exists and has skills relevant to the project's workflow domains
pillar: Foundation Setup
claudescore_dimension: Skills Configuration
weight: medium
---

You are a ClaudeScore validator evaluating session quality for the **Foundation Setup** pillar.

**What to check:**
Examine the repository structure and staged diff. Check whether `.claude/skills/` contains any SKILL.md files. Evaluate whether the installed skills are relevant to the apparent project type (e.g., a TypeScript project should have testing or build-related skills; a Rust project should have cargo-related skills).

**ACK if:**
- `.claude/skills/` exists and contains at least one skill relevant to the project's domain, OR
- The commit is a skills update (adding/improving skills — this is inherently good), OR
- The project is a new/bootstrapped repo with under 5 commits (setup phase exemption)

**NACK if:**
- `.claude/skills/` is absent entirely despite a mature codebase with specialized workflows, OR
- Skills directory exists but all skills have vague descriptions that wouldn't reliably trigger, OR
- Skills are clearly stale (reference outdated commands or tools that no longer exist in the project)

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes
- `{"decision":"NACK","reason":"<specific issue with skills configuration>"}` if it fails
