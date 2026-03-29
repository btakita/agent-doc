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

**Compacted** (details in git history, commits `f80f524`..`f8fcce2`):

1. **DEV.md files** created for agent-doc, corky, instruction-files (project-specific release gotchas, secret paths, post-release steps)
2. **Submodule commits** — two rounds of inside-out commits across all dirty submodules (instruction-files, tmux-router, rappstack-dev, briantakita.me-dev, mail, resume) + parent pointer updates
3. **Crate releases** published to crates.io:
   - `instruction-files` v0.1.5 (`87945e5`)
   - `tmux-router` v0.2.9 (`21b2033`) — includes swap-pane atomic transitions + test fix
   - `tagpath` v0.5.1 (`c73139d`)
4. **Parent commit** `f8fcce2`: Updated 6 submodule pointers

**Open question:** Should agent-doc and corky get releases too (they depend on tmux-router and instruction-files)?
yes
commit all submodules

### Re: release agent-doc & corky + commit all

**agent-doc v0.25.12** released:
- Bumped tmux-router dep `0.2.8` → `0.2.9` (swap-pane atomic transitions)
- Published: crates.io, PyPI, GitHub Release
- Binary installed via `cargo install` + release build

**corky v0.21.1** released:
- Bumped instruction-files dep lock to v0.1.5
- Published: crates.io (with GCP build-time vars), PyPI, GitHub Release

**Parent commit** `50bec72`: Updated agent-doc + corky submodule pointers.

All submodules clean, all releases published.

Commit and push all submodules.
Release any unreleased software.
<!-- agent:boundary:7a0a0be9 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
