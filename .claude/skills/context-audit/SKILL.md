---
name: context-audit
description: >
  Audit Claude Code setup for token waste and context bloat. Use when
  the user says "audit my context", "check my settings", "why is Claude so
  slow", "token optimization", "context audit", or runs /context-audit.
user-invocable: true
---

# Context Audit

Audit your Claude Code setup for token waste and context bloat. Scores your
setup out of 100 and tells you exactly what to cut.

## Step 1: Get /context data

Check the conversation history for `/context` output. If the user already
ran `/context` in this session, use that data. If not, ask:

> Run `/context` in this terminal and let me know when you're done. I can't
> run slash commands myself, but once I see the breakdown I'll audit everything.

**STOP HERE.** Do not proceed until the user has run `/context`. The context
breakdown determines what to audit and in what order.

## Step 2: Run the audit

Read and follow the runbook: [runbooks/context-audit.md](runbooks/context-audit.md)

Pass the `/context` output as input to the runbook procedure.
