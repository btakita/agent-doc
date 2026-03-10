---
agent_doc_session: null
agent_doc_mode: template
---

# Lucas Onboarding — Recruiter Email Workflow

## Context

Lucas wants an easy process to reply to recruiter emails with rich context for detailed correspondence.

## Architecture

Lucas gets his own monorepo with submodules to Brian's context:

1. **Lucas's monorepo** (`lucas-agent-loop` or similar):
   - Submodule pointing to shared context (contact files, templates, recruiter threads)
   - His own `CLAUDE.md` with preferences, voice, and recruiter reply style
   - A `mail/` directory for his correspondence

2. **Claude Code as the interface**:
   - `/corky` skill handles all plumbing (git, threading, validation)
   - "Reply to the recruiter from Acme Corp" → gets a draft
   - Review → approve → Brian sends from his account

3. **Shared context via submodules:**
   - `src/shared-contacts/` — contact files with recruiter info, company notes
   - `src/mail-brian/` (read-only submodule) — synced threads to reply to
   - His own `drafts/` — where replies live

## Key Requirements

- Zero git commands (Claude handles commits/pushes)
- Rich context = better drafts (recruiter's company, role details, past correspondence)
- Review loop gives Brian control before anything sends
- Process must be easy & perceived as valuable enough to use

## Open Questions

- Does Lucas already use Claude Code? If not, that's the onboarding bottleneck.
- Should recruiter threads sync automatically or on-demand?
- How much of Brian's contact context should Lucas see?

## btakita/mailbox-lucas

Repo already exists with a README quick start guide. Lucas is not yet a collaborator.

**To onboard:**
1. Get his GitHub username
2. `gh api repos/btakita/mailbox-lucas/collaborators/HIS_GITHUB_USERNAME -X PUT -f permission=push`

<!-- agent:exchange -->
Extracted from tasks/corky.md. Ready for next steps.
<!-- /agent:exchange -->
