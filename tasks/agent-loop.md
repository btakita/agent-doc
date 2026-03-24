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

Go through all of the submodules. Commit. Release anything unreleased.

### Re: submodule commits

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
commit all submodules
release anything pending
commit agent-loop

### Re: commit & release

**Submodule commits done:**

| Submodule | Action | Commit |
|-----------|--------|--------|
| `mail` | sync contacts, conversations, drafts (32 files) | `912be4c` |
| `resume` | update resume + andrew-curtis profile | `78a2a07` |

**Pending releases identified** (unreleased commits since last tag):

| Submodule | Current Tag | Unreleased Commits | Bump To |
|-----------|-------------|-------------------|---------|
| `src/instruction-files` | v0.1.4 | 4 (DEV.md, .gitignore, JetBrains module, Cargo.lock) | v0.1.5 |
| `src/tmux-router` | v0.2.8 | 2 (swap_pane feat, focus-steal logging) | v0.2.9 |
| `src/tagpath` | v0.5.0 | 2 (.gitignore, Claude Code skill) | v0.5.1 |

`src/agent-doc` (v0.25.11) and `src/corky` (v0.21.0) are already at HEAD.

Publishing all three to crates.io now, then committing agent-loop parent pointers.
Should we release agent-doc and corky due to tmux-router and instruction-files releases?

**Release progress:**

All three crates are committed, tagged, and pushed to GitHub:
- `instruction-files` v0.1.5 → `87945e5`, tag pushed
- `tmux-router` v0.2.9 → `21b2033`, tag pushed (fixed failing test from swap-pane fast path)
- `tagpath` v0.5.1 → `c73139d`, tag pushed

**Blocked:** crates.io token in `~/.cargo/credentials.toml` is expired (403 Forbidden). Need a new token from https://crates.io/settings/tokens — run `cargo login` to set it, then I'll publish all three.

try again

### Re: try again

Root cause: `CARGO_REGISTRY_TOKEN` was set as a **literal** `(pass btak/CARGO_REGISTRY_TOKEN)` in the environment (not shell-expanded). Fixed by passing `"$(pass btak/CARGO_REGISTRY_TOKEN)"` explicitly.

**All three crates published to crates.io:**
- `instruction-files` v0.1.5
- `tmux-router` v0.2.9
- `tagpath` v0.5.1

**Parent commit** `f8fcce2`: Updated 6 submodule pointers (mail, resume, agent-doc, instruction-files, tmux-router, tagpath).

All done — submodules committed, releases published, parent pointers updated.
<!-- agent:boundary:35d90669 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
