---
name: feedback_concurrent_agent_doc_refactor_session_build_coordination
description: "A concurrent agent-doc session runs crate-extraction refactors + `make clear`; don't fight its churning working tree — commit selectively and build from a clean commit."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e78c804c-247c-4dca-be12-9d7670c6ee81
---

During some sessions a **concurrent** agent-doc session is actively running a
large crate-extraction refactor (extracting `agent-doc-agent-io`,
`agent-doc-supervisor-process`, `session_liveness`, `ops_log`, `capture`,
`session-accretion` into their own crates) and periodically runs `make clear`.
This makes `src/agent-doc`'s working tree flap between compilable and broken
(dangling `pub mod X` with the file moved, `crate::agent`/`ops_log`/`capture`
unresolved) every few minutes — **not caused by your edits.**

**Why:** the refactor commits working increments to `HEAD` continuously, but its
in-progress edits sit uncommitted in the shared working tree, so any build/test
you run against the working tree can fail on their half-applied state.

**How to apply:**
- Don't patch their in-flight files to "unblock" — you'll collide. The breakage
  is transient; `HEAD` itself compiles (they commit working increments).
- Commit **only your files** selectively (`git add <my files>`); their
  uncommitted changes stay untouched. Your commit = clean `HEAD` + your delta.
- To build a reproducible binary, either build in-place only when the tree
  currently compiles, or build from your commit in a git worktree placed **under
  `src/`** (e.g. `src/agent-doc-focusfix-build`) so the `../../tmux-router` path
  dep still resolves to the real `src/tmux-router`. Remove the worktree after.
- **Do not create a worktree or start a build while the other session is running
  `make clear`** — it can sweep untracked dirs / wipe artifacts mid-build. The
  operator will say when it's running; wait for it to finish. [[feedback_concurrent_sessions_force_push_shared_superproject]]
- Install the built release binary by copying to `~/.cargo/bin/agent-doc` via
  atomic rename (`cp … .new && mv -f`) — a live process holds it `Text file
  busy`. The JB plugin resolves `<root>/.bin/agent-doc` first (→
  `target/release/agent-doc`), so refresh that too.
- To verify a **self-contained new module** (std-only + a crate like `libc`) while
  the workspace is mid-break, don't fight the tree: drop the file into a throwaway
  scratch cargo crate in the scratchpad with just its real deps and `cargo test`
  it there. A `/tmp` git worktree does NOT work for this submodule — its relative
  `../tmux-router` / `../.cargo/config.toml` path deps only resolve when the
  worktree is under `src/` (see the worktree-under-`src/` note above).
- Mid-crate-extraction the tree can show a **cyclic package dependency** (e.g.
  `agent-doc-supervisor-io depends on itself`) or unresolved `agent_doc_supervisor*`
  — transient WIP, not your bug. **Defer any edit that must touch the crates they
  are actively restructuring** (supervisor/controller/session-registry) to a fresh
  focused cycle after their refactor lands; you cannot build to verify against a
  moving crate graph. [[feedback_deep_supervisor_refactor_fresh_focused_cycle]]
