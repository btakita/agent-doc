---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
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
### Re: commit all submodules + release unreleased software

**Submodules committed & pushed:**
- `mail` — sync contacts, conversations, drafts, social (132 files)
- `philosophy` — add interface/protocol terms, update abstraction
- `src/corky` — contact push google + diarize as default feature
- `src/drmanny-dev` — update .iml

**tmux-router v0.3.2** released:
- Fixed 4 clippy errors (collapsible if, doc continuation, unused var)
- Includes `alive_pane_ids()` bulk method from previous commit
- Published: crates.io, GitHub Release
- Note: v0.3.1 was already taken on crates.io, bumped to v0.3.2

**Parent commit** `7312fce`: Updated 6 submodule pointers (mail, philosophy, agent-doc, corky, drmanny-dev, tmux-router). Pushed to origin.

**Not committed:** `src/briantakita.me-dev` has submodule mismatch but no new changes inside.

When I ask you to commit all submodules...do it recursively

Commit src/briantakita.me-dev/app/briantakita.me
### Re: recursive submodule commit — briantakita.me-dev

Committed inside-out:

1. **`src/briantakita.me-dev/app/briantakita.me`** (`2e08656`):
   - Pinned rappstack deps to 0.1.4 (removed `^` range)
   - Fixed GitHub URLs in blog post (nicholasgasior → btakita)
   - Pushed to origin

2. **`src/briantakita.me-dev`** (`dc43971`): Updated briantakita.me submodule pointer, pushed

3. **Parent `agent-loop`** (`0ad8bf4`): Updated briantakita.me-dev pointer, pushed

All submodules now clean and pushed, including nested ones. Will commit recursively in future.

Create a github project btakita/eval-runner for src/eval-runner
src/eval-runner will be a submodule in agent-loop
### Re: create btakita/eval-runner GitHub project + submodule

**GitHub repo created:** https://github.com/btakita/eval-runner (public)

**Initial setup:**
- Initialized git in existing `src/eval-runner/` directory
- Added `.gitignore` (target/, *.iml, .agent-doc/)
- Cleaned up accidentally committed `target/` directory
- Pushed to origin

**Submodule registered** in agent-loop:
- `git submodule add` recognized existing repo
- Parent commit `940119d`: added eval-runner as submodule, pushed
<!-- agent:boundary:57ffefbb -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
