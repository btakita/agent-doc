---
agent_doc_session: 4ce06ad1-5c84-43b0-b32c-542e6c3d8ffb
tmux_session: '1'
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

I have a github PAT for lucas. Please track it by using the gh api. Make sure the PAT has edit access to btakita/resume

A few important things to clarify:

1. **GitHub PATs can't be "tracked" via the gh API** — PATs are personal tokens owned by Lucas. They aren't visible through the GitHub API from your account. The token is a secret Lucas manages.

2. **What you CAN do** is use Lucas's PAT to verify it works and grant him repo access.

**To grant push access to btakita/resume:**
```bash
gh api repos/btakita/resume/collaborators/LUCAS_GITHUB_USERNAME -X PUT -f permission=push
```

**I need from you:**
- Lucas's **GitHub username** (to add as collaborator on `btakita/resume` and `btakita/mailbox-lucas`)
- If you want to **test his PAT**, set it as `GH_TOKEN` and I can run API calls to verify its scopes

What did you mean by "track it"? Store it somewhere, test it, or use it to configure his access?
<!-- /agent:exchange -->
