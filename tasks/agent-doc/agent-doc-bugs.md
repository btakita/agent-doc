---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — CRDT compact fix released, all tests passing, live in production
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
CRDT compact fix released (v0.32.1). Root cause: stale CRDT state after compact operation. Fix: refresh CRDT from post-compact content. Automated tests: 262 pass (compact_preserves_pending, boundary_marker_preservation, working_tree_consistency). All deliverables shipped: crates.io, GitHub Release, binary installed.

❯ Read
❯ - /home/brian/work/btakita/agent-loop/tmp/context-audit/SKILL.md
❯ - /home/brian/work/btakita/agent-loop/tmp/context-audit/claude-code-context-cleanup-guide.md

❯ How can we convert this into a SKILL->runbook?
Where should this live? In its own package? In agent-doc?
### Re: Context audit → SKILL + runbook (HEAD)

**SKILL→runbook split:**
- **SKILL.md (~30 lines)** — description, trigger phrases, `/context` gate (requires user interaction, so stays in skill). Pointer to runbook for the procedure.
- **runbook/context-audit.md (~120 lines)** — Steps 2-4: audit checks (MCP, CLAUDE.md, skills, settings, permissions), scoring matrix, report template, fix offers.
- **The guide PDF** is educational/marketing content, not operational. It informed the skill's design but shouldn't be in the skill itself.

**Where it should live:**
- **Not agent-doc** — general Claude Code hygiene, not document-session-specific.
- **Not its own package** — pure skill, no binary or compiled logic. Just markdown.
- **Best fit: `.claude/skills/context-audit/`** — in agent-loop for personal use. For distribution, it's a standalone folder people drop into their `.claude/skills/`. The guide author already distributes it as a zip for this reason.

Structure:
```
.claude/skills/context-audit/
├── SKILL.md           # trigger + /context gate
└── runbooks/
    └── context-audit.md  # the full audit procedure
```

Want me to scaffold this?
<!-- agent:boundary:2ead1741 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- ✅ **Release v0.32.1 complete** (crates.io + GitHub)
- 🔲 **Context-audit skill** — scaffold SKILL.md + runbook from tmp/context-audit/ content (awaiting approval)
<!-- /agent:pending -->
