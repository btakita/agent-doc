---
type: concept
tags: [agent-doc, skill, harness]
---

# Instruction Surface

An instruction surface is the managed set of files that tells an agent how to
run agent-doc in its harness. The source surface is `SKILL.md` plus bundled
runbooks and OKF concepts. Installed surfaces are generated mirrors such as
`.claude/skills/agent-doc/SKILL.md`, `.codex/AGENTS.md`,
`.opencode/skills/agent-doc/SKILL.md`, and managed root `AGENTS.md`.

The source surface owns policy. Installed mirrors must render from that source
and differ only where harness invocation requires it.

## Use When

Load this concept when changing skill install behavior, audit-docs behavior,
or any guidance that must remain aligned across Claude Code, Codex, OpenCode,
Cursor, and generic installs.
