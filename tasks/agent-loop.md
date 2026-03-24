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

### Re: submodule commits

Committed all dirty submodules from the inside out:

| Submodule | Action | Commit |
|-----------|--------|--------|
| `rappstack-dev/lib/domain--any--blog` | feat: video_url + playlist_url fields | `b901a39` |
| `rappstack-dev/lib/domain--server--blog` | feat: video_url + playlist_url schema/helpers | `4c6e539` |
| `src/rappstack-dev` | chore: update blog submodule pointers | `a934c87` |
| `mail` | chore: sync contacts, conversations, drafts, social (159 files) | `2c7b147` |
| Parent pointers | mail, rappstack-dev, agent-resume, briantakita.me-dev, brookebrodack-dev, tmux-router | `8b476aa` |

All submodules now clean — `git submodule status` shows no `+` or `m` prefixes.
<!-- agent:boundary:d0aae4b2 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
