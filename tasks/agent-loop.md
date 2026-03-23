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

### Re: go

Created `DEV.md` files for the three most actively released submodules:

| File | Type | Key Overrides |
|------|------|---------------|
| `src/agent-doc/DEV.md` | rust-maturin | post-release `cargo install`, skill update reminder, lib target testing |
| `src/corky/DEV.md` | rust-maturin | GCP OAuth build-time injection, YouTube public default warning, two-repo model |
| `src/instruction-files/DEV.md` | rust-lib | No Makefile, no audit-docs, dependency chain note |

**What each DEV.md captures** (beyond SKILL.md defaults):
- Project-specific gotchas learned from past releases (e.g., stale binary, GCP creds)
- Secret paths including corky's extra GCP secrets
- Post-release steps (cargo install, release build)
- Dependency chain awareness

**Not yet created** (low release frequency, defaults suffice for now):
- `tmux-router/DEV.md` — simple Rust lib, no special patterns yet
- `tagpath/DEV.md` — same
- JS/TS app DEV.md files — deploy workflows not yet established

**Remaining future iteration: integration testing.** We could do a dry-run of `/dev agent-doc release` to validate the full workflow end-to-end. Want to try that, or move on?

Go through all of the submodules. Commit

### Re: submodule commits (HEAD)

Went through all submodules, committed everything from the inside out:

| Submodule | What was committed |
|-----------|-------------------|
| `src/instruction-files` | New `DEV.md` |
| `src/tmux-router` | Focus-steal diagnosis logging in sync/tmux |
| `src/rappstack-dev` | Blog submodule pointer updates (domain--any--blog, domain--server--blog) |
| `src/briantakita.me-dev` | Nested: `app/briantakita.me` (blog posts + yalc), `lib/ui--server--briantakita` (post doc html + yalc), then parent pointer + deps |
| `mail` | Full sync — contacts, conversations, drafts, social |
| `resume` | 4 new PDFs + profiles (andrew-curtis, benjamin-becker, eci-solutions, guidehouse-sba) |

**Parent commit** `8b476aa`: Updated all 8 submodule pointers (mail, resume, agent-resume, briantakita.me-dev, brookebrodack-dev, instruction-files, rappstack-dev, tmux-router).

All submodules are now clean.
<!-- agent:boundary:b55c7be5 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
