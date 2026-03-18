---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->

### Re: Human-driven SPEC evolution + borrowable skill repos

Added **§11 SPEC Evolution — Human-Driven, AI-Assisted** to SPEC.md. Five subsections:

**§11.1 Bootstrapping a New Dev Skill:**
1. Start from an existing SPEC (copy/reference)
2. Strip workspace-specific content
3. Create your SKILL.md adapted to your workspace
4. Iterate via conversation — AI assists, human decides
5. Add per-project DEV.md as needed

**§11.2 Borrowing from Other Skill Repos:**
- Copy sections, reference upstream, diverge freely
- AI discovery pattern: search for published SPECs → present relevant sections → human cherry-picks
- Never auto-merge entire SPECs

**§11.3 SPEC Sections as Modules:**
Each numbered section is independently adoptable. Required sections (Commands, Project Detection, Checklist, Workflows, Secrets) vs optional (Meta-Cognition, Composability, SPEC Evolution). A minimal workspace might only adopt §3-§7.

**§11.4 Version History:**
Changelog comment in SPEC for fork lineage tracking.

**§11.5 Anti-Patterns:**
- AI-generated SPEC without human review
- Monolithic SPEC covering everything
- Blind upstream merge
- SPEC drift from practice (dead sections)

The core philosophy: **the human drives, the AI assists**. The SPEC is a curated document, not an auto-generated one. People should borrow sections from skill repos they admire, not adopt entire frameworks wholesale.

Are we complete?
<!-- /agent:exchange -->

<!-- agent:input -->
<!-- /agent:input -->

<!-- agent:pending patch=replace -->
- [x] Make SPEC creation process configurable + human-driven + borrowable
<!-- /agent:pending -->
