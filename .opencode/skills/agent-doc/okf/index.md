---
okf_version: "0.1"
type: index
tags: [agent-doc, dynamic-context, okf]
---

# Agent Doc OKF Index

This bundle holds durable concept definitions for the agent-doc instruction
surface. `SKILL.md` stays the hot-path router, runbooks hold branch procedures,
and OKF files hold stable vocabulary that agents may load when a term needs a
precise shared meaning.

## Concepts

- [Session Cycle](session-cycle.md): one preflight, response, persistence, and
  closeout unit for a session document.
- [Instruction Surface](instruction-surface.md): the managed SKILL/AGENTS and
  runbook files installed for each harness.
- [Dynamic Context](dynamic-context.md): the router/resource/generated-pack
  pattern used to keep hot-path instructions small.
