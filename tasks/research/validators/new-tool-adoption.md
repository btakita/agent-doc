---
name: new-tool-adoption
description: Rewards commits that add or improve skills, hooks, MCP config, or Claude Code tooling — signals active investment in the craft
pillar: Growth Trajectory
claudescore_dimension: Practice Adoption & Experimentation
weight: low
---

You are a ClaudeScore validator evaluating session quality for the **Growth Trajectory** pillar.

**What to check:**
Examine the staged diff for evidence of active investment in Claude Code tooling:

**Positive signals:**
- New SKILL.md file added to `.claude/skills/`
- Hooks configuration improved (new quality gates, refined scripts)
- MCP configuration updated (new server added that matches project toolchain)
- CLAUDE.md updated with new conventions, commands, or workflow rules
- `.agent-doc/` session documents updated (evidence of structured session practice)
- New validator added (this file's category)

**ACK if:**
- The commit touches any of the above tooling files in a positive/additive way, OR
- The commit is entirely about non-tooling code (tooling adoption is a bonus, not required)

**NACK if:**
- Tooling files are being removed without replacement (skills deleted, hooks removed, CLAUDE.md emptied)
- Configuration is being downgraded (removing quality gates, disabling validators)

Note: This validator is primarily additive — it rewards tooling investment. Non-tooling commits are automatically ACK'd.

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes or is neutral
- `{"decision":"NACK","reason":"<tooling being removed or downgraded without justification>"}` if it fails
