---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.20.3 | **Tests:** 451 (157 lib + 273 bin + 21 int) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.6 (hand-written JSON + JNA FFI) + VS Code v0.2.3 (prepend fix)
**Library:** `libagent_doc.so` (1.7M) — 6 FFI exports | **Routing:** numeric session fix (colon suffix)
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**tmux-router v0.2.4**: Standalone Rust crate (library + CLI). `Tmux` struct, `Registry`, `reconcile()` (2D sync), `IsolatedTmux` (test infra), `FileResolution` callback, `prune()`, `acquire_or_skip()`. 56 tests. Published to crates.io + GitHub Releases.

**Core:** diff → agent → merge-safe write → snapshot → git. Components (`&lt;!-- agent:name --&gt;`), template mode, stream mode (CRDT), `agent-doc patch` (hooks), `agent-doc watch` (debounced + reactive), `agent-doc write --stream` (CRDT merge), `agent-doc recover`, `agent-doc plugin install`. Comment stripping, auto-resync, stale snapshot recovery, diagnostic logging.

**Library target (`libagent_doc`):** `[lib]` with `crate-type = ["cdylib", "lib"]`. Exports `component`, `crdt`, `ffi`, `frontmatter`, `merge`, `template` modules. C ABI via `ffi.rs` (6 functions: parse_components, apply_patch, crdt_merge, merge_frontmatter, free_string, free_state). Binary modules use `pub(crate) use agent_doc::*` re-exports. `agent-doc lib-path` prints the shared library location.

**Concurrency:** `SnapshotLock` + `RegistryLock` (flock), atomic rename (tempfile + persist), doc advisory lock, `pub(crate)` load/save.

**Routing:** Window-scoped. Plugin passes `--window`. Auto-start dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugins (JetBrains + VS Code):** 9/9 SPEC parity. `editors/` directory. `agent-doc commands` for autocomplete. JetBrains: JNA FFI bindings (`NativeLib.kt`) with Kotlin fallback — native-first patching for component patches and frontmatter merge.

**Config:** `~/.config/agent-doc/config.toml`. Multi-agent backend support. Execution mode per-skill via SKILL.md frontmatter (`hybrid | parallel | sequential`).

**Skill auto-upgrade:** SKILL.md bundled via `include_str!`. Pre-flight version check → `agent-doc skill install`.

### Race Conditions

| Race | Protection |
|------|-----------|
| Registry read-modify-write | flock (RegistryLock) |
| Snapshot read-modify-write | flock (SnapshotLock) |
| Nested lock deadlock | Thread-local flag + `acquire_or_skip()` |
| Watch daemon write window | Atomic rename + doc advisory lock |
| Stale snapshot (compaction) | `is_stale_snapshot()` auto-recovery |
| Compaction mid-write | Durable pending store + `recover` |

### Execution Model

| Mode | Behavior |
|------|----------|
| **`hybrid`** (default) | First doc direct, 2nd+ concurrent use subagent |
| **`parallel`** | Every `/agent-doc` spawns subagent |
| **`sequential`** | Fully sequential, cheapest |

### Domain Ontology

| Term | Derives From |
|------|-------------|
| **Session** | project + story |
| **Document** | entity + context |
| **Component** | scope + abstraction |
| **Project** | system + scope |
| **Pane** | focus + scope |
<!-- /agent:architecture -->

## Release History

<!-- agent:releases -->
| Version | Key Changes |
|---------|-------------|
| v0.5.5-v0.9.6 | Window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename. 176 tests. |
| v0.9.7-v0.9.10 | `agent-doc write`, template mode, compaction recovery, `recover`, `template-info`, code-span parser fix. 192 tests. |
| v0.10.0-v0.10.1 | Renamed `response_mode` → `agent_doc_mode`, bundled template instructions in SKILL.md. |
| v0.11.0-v0.11.2 | Append-friendly merge strategy, shared `merge.rs`, fixed duplicate headings. |
| v0.12.0-v0.12.2 | `compact`, `convert`, lazy claim, exchange default, exchange append mode default. 218 tests. |
| v0.13.0-v0.13.3 | `autoclaim` (SessionStart hook), git-based snapshot fallback, sync autoclaim, spare pane assignment (tmux-router v0.2.4). |
| v0.14.0-v0.14.9 | Route focus fix, `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, multi-backtick. 235 tests. |
| v0.15.0-v0.16.1 | CRDT stream mode (yrs), chain-of-thought, reactive file-watching, truncation detection, native compact. 309 tests. |
| v0.17.0-v0.17.30 | Format/write split, IPC write, JNA FFI (NativeLib.kt), VS Code prepend, immutable session binding, `lib-path`. 440 tests. |
| v0.18.0-v0.18.1 | `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3. 446 tests. |
| v0.19.0 | ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7). 446 tests. |
| v0.20.0-v0.20.3 | CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs. Session affinity enforcement. Numeric session fix (colon suffix). `claims` subcommand. 451 tests. |
<!-- /agent:releases -->

## Lessons

<!-- agent:lessons -->
1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity. This applies to ALL tmux `-t` targeting, not just stash. `new_window("-t", "0")` interprets "0" as window index; `new_window("-t", "0:")` forces session name. Root cause of persistent session bleedover bug fixed in v0.2.8.
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"
8. **YAGNI for speculative code** — `with_snapshot` was added speculatively, then deleted (no production callers)
9. **Subagents are expensive (~7x)** — use filesystem locking instead when possible; subagents only for true parallelism
10. **Stale snapshots from context compaction** — solved with `is_stale_snapshot()` auto-recovery in `diff.rs`
11. **Durable pending store** — save response before write, clear after success; survives compaction and crashes
12. **Distinct patch prefix** — patch markers vs agent markers prevents confusion between response directives and document components
13. **Skip code spans in ALL parsers** — any function scanning for `<!--` in user documents must use `find_code_ranges()`. Same fix needed twice: component.rs (v0.9.10) and diff.rs (v0.14.8). Check all `<!--` parsers when adding new ones.
14. **Immutable session binding** — `tmux_session` in frontmatter should never be silently overwritten by claim. This caused cross-session pane swapping when autoclaim or lazy-claim ran in a different session. Fix: refuse to overwrite unless `--force`. Track multi-step plans in `agent:pending` to survive context compaction.
15. **No Gson in JetBrains plugin** — Gson causes `ClassNotFoundException` at runtime in some IntelliJ builds. Use hand-written JSON parsing. This bug was found and fixed twice (SlashCommandCompletionContributor Mar 11, PatchWatcher Mar 18) — check git history before introducing "bundled" dependencies.
16. **Never fall back to current_tmux_session() when tmux_session is set** — `display-message #{session_name}` returns whichever session the user's terminal is viewing. If the user switches to session 1, all routes using `current_tmux_session()` as fallback will create panes in session 1. Fix: bail with error instead of fallback. Only use `current_tmux_session()` for first-claim (no frontmatter binding yet).
<!-- /agent:lessons -->

## Session History

<!-- agent:history -->
**v0.5.5-v0.9.6:** Foundation — window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename, stale snapshot recovery.

**v0.9.7-v0.12.2:** `agent-doc write`, template mode, compaction recovery, `recover`, `exchange` convention, append-friendly merge, `compact`/`convert`, lazy claim.

**v0.13.0-v0.14.9:** `autoclaim`, git-based snapshot fallback, `commands`, plugin autocomplete (v0.2.0), bidirectional `convert`, code-range-aware `strip_comments`, multi-backtick.

**v0.15.0-v0.16.1:** CRDT stream mode (yrs), chain-of-thought streaming, reactive file-watching, truncation detection, native compact. 309 tests.

**v0.17.0-v0.17.30:** Format/write split. IPC write + JetBrains PatchWatcher. JNA FFI (NativeLib.kt). VS Code prepend. Immutable `tmux_session` binding. `lib-path` + `merge_frontmatter()` FFI export.

**v0.18.0-v0.18.1:** `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3.

**v0.19.0:** ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7).

**v0.20.0-v0.20.3:** CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs, session affinity enforcement, numeric session fix (colon suffix), `agent-doc claims` subcommand. 451 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260319-041740.md`*

**Session recap (v0.17.27 → v0.20.3):**

**Releases:** v0.18.0, v0.18.1, v0.19.0, v0.20.0, v0.20.1, v0.20.2, v0.20.3

**Features built:**
- Library target with C FFI (6 exports) + `lib-path` + JNA bindings (NativeLib.kt) with Kotlin fallback
- VS Code prepend mode fix, JetBrains Gson revert (ClassNotFoundException)
- Immutable `tmux_session` binding + session affinity enforcement (bail on wrong-session)
- Pre-response snapshots + `undo`, `extract`, `transfer` commands
- ExecutionMode in `config.toml` (hybrid/parallel/sequential)
- TmuxBatch (tmux-router v0.2.7) — `select_pane()` uses batch
- CRDT conservative dedup (#15) + frontmatter patches on disk write path (#16)
- Binary-vs-agent responsibility docs in CLAUDE.md
- H2 headers before components (canonical pattern), claim scaffolding updated
- Numeric session name ambiguity fix (colon suffix in tmux-router v0.2.8)
- `agent-doc claims` subcommand (replaces shell one-liner, prevents zombie processes)
- Sub-profiles design proposal (#23)
- 13 GitHub issues created for pending roadmap items (#17-#29)

**Bugs fixed:**
- Gson ClassNotFoundException in JB plugin (lesson #15)
- Cross-session pane creation from `current_tmux_session()` fallback (lesson #16)
- Numeric session name ambiguity in `new_window()` (lesson #6 extended)
- Session 1 pane bleedover — cleaned up misplaced panes
- `resync --fix` gap: can't join to full stash (manual kill needed)
- 26 zombie bash processes from claims log check (solved with `agent-doc claims`)

**Issues closed:** #15 (CRDT dedup), #16 (frontmatter patches)

**Test count:** 451 (157 lib + 273 bin + 21 int), up from 440.

Tell me more about this feature:
- `agent-doc deep` — parallel subagent fan-out with worktrees ([#21](https://github.com/btakita/agent-doc/issues/21))

Consider different command names. Some possibilities:
- `agent-doc deep`
- `agent-doc worktrees`
- `agent-doc fan-out`
- `agent-doc parallel`
- `agent-doc background`
- `agent-doc offload`

Are there other alternatives that would be a better command name?

### Re: `agent-doc deep` — Feature Exploration + Command Naming

**What this feature does:**

When a document session encounters a complex task that can be decomposed into independent subtasks, this command would:

1. **Fan-out:** Spawn N parallel Claude sessions, each in its own git worktree. Each subtask gets an isolated copy of the repo — no merge conflicts during concurrent work.
2. **Execute:** Each subagent works independently on its assigned subtask (e.g., "implement auth middleware", "write tests for X", "research API design").
3. **Checkpoint:** Results from each subagent are collected — diffs, outputs, findings.
4. **Fan-in:** Present all results back in the parent document for user review before merging. The user reviews diffs, approves/rejects, and the approved changes are merged from worktrees into the main branch.

**Use cases:**
- Research multiple approaches to a design problem simultaneously
- Implement independent features in parallel (auth + tests + docs)
- Investigate multiple files/modules concurrently for a bug hunt
- Generate multiple candidate implementations for comparison

**Command name analysis:**

| Name | Pros | Cons |
|------|------|------|
| `deep` | Short, evocative of "deep work" | Ambiguous — could mean "deep analysis" |
| `worktrees` | Technically precise | Implementation detail, not user intent |
| `fan-out` | Describes the pattern | Hyphenated, jargon-y |
| `parallel` | Clear intent | Generic, clashes with ExecutionMode |
| `background` | Familiar concept | Implies single task, not fan-out |
| `offload` | Delegation metaphor | Implies fire-and-forget, no review |
| `spawn` | Clean verb, implies creating child processes | Could confuse with process spawning |
| `swarm` | Multiple agents working in concert | Trendy but clear |
| `branch` | Git metaphor, each subtask gets a branch | Overloaded with git branch concept |
| **`fork`** | Process/git metaphor, parallel divergence then merge | Short, Unix-familiar, no collisions |
| `split` | Divide work into parts | Already used for tmux splits |

**Recommendation: `agent-doc fork`** — clear "diverge then converge" lifecycle. Pairs naturally with review/merge phase. Git-native concept (worktrees are conceptually forks). No naming collisions with existing agent-doc concepts.

Runner-up: `agent-doc spawn` — also clean, but `fork` better captures the full lifecycle.

Would fork be misleading? We are not forking the process. Are we actually forking the context?

### Re: Fork Naming Concern

You're right. `fork` in Unix means "duplicate the current process" — but here we're not duplicating a process. We're:

1. **Forking the codebase** (git worktree = a branch of the repo) — accurate
2. **Spawning new agents** with distinct tasks (not clones of the parent) — not a fork
3. **Merging results back** — fork doesn't imply this

"Fork" is partially accurate (worktrees) but misleading about what's being forked. Users might expect the agent session context to be duplicated.

**What we're actually doing:** Dispatching work to parallel workers with isolated codebases.

Revisiting with that lens:

| Name | Fit |
|------|-----|
| **`dispatch`** | Sending tasks to workers. Clear, no overloaded meaning. Implies results collection. |
| `delegate` | Assigning work to subordinates. Natural language but passive. |
| `scatter` | Scatter-gather pattern. Technical but accurate. |
| **`deep`** | "Go deeper" — intensive parallel exploration. Memorable but doesn't describe mechanism. |
| `fan` | Short for fan-out. Minimal, technical. |

**Choice is between mechanism clarity (`dispatch`) and intent clarity (`deep`).**

`dispatch` describes what happens: send tasks to isolated workers, collect results. `deep` describes why: intensive multi-angle analysis. Both are valid — depends on which mental model you want users to have.

Could dispatch be used for other functionality that is not related to this pattern?

### Re: Could `dispatch` Be Overloaded?

Yes. `dispatch` could apply to:
- Sending a single task to a background agent (no worktree, no fan-out)
- Routing a document to a specific agent backend
- Event/message dispatch patterns

It's generic enough to be claimed by future features, which dilutes specificity for the parallel-worktree pattern.

**This argues for `deep` after all:**
- Specific — no other agent-doc feature would naturally claim it
- Short and memorable
- Intent-clear: "go deep on this problem with multiple parallel workers"
- Not overloaded by any existing concept
- The mechanism (worktrees, parallel agents) is an implementation detail — the user cares about "intensive multi-angle work"

Runner-up `explore` has the same overloading risk ("explore the codebase" = single agent).

**Recommendation: keep `agent-doc deep`.**

Compare with spawn. Is spawn also applicable to other potential features?

### Re: `spawn` vs `deep` — Overloading Risk

Yes, `spawn` is also applicable to other features:
- `agent-doc start` already spawns a Claude session — `spawn` could describe that
- Single background task — "spawn a worker" (no worktree, no fan-out)
- Watch daemon spawns a background process
- Any future subprocess creation

| Name | Specificity | Overloading risk |
|------|------------|-----------------|
| `spawn` | Low — any subprocess | High |
| `dispatch` | Medium — task routing | Medium |
| `deep` | High — only this feature | Low |

`deep` remains strongest because it's specific enough that no other feature would claim it. The slight ambiguity covers all use cases (parallel research, implementation, investigation) under one evocative name.

Describe the `agent-doc deep` feature. How would it be implemented and used?

### Re: `agent-doc deep` — Implementation Design

**Usage:**
```bash
# Explicit tasks
agent-doc deep plan.md \
  --task "Implement auth middleware" \
  --task "Write tests for auth" \
  --task "Update API docs"

# Or auto-decompose (agent splits the prompt)
agent-doc deep plan.md --tasks 3
```

**In-document usage:** Write `deep:` followed by bullet tasks in the exchange. The skill parses and calls `agent-doc deep`.

**Architecture:**

```
agent-doc deep <FILE> [--task "..."]* [--tasks N] [--parallel 3]
    │
    ├── 1. Parse tasks from FILE or --task flags
    ├── 2. Create git worktree per task:
    │     git worktree add .agent-doc/worktrees/<hash>-<n> -b deep/<session>/<n> HEAD
    ├── 3. Spawn Claude in each worktree (stashed tmux panes)
    ├── 4. Monitor completion (exit codes, output capture)
    ├── 5. Collect: git diff HEAD per worktree + agent output
    └── 6. Write results to parent doc for review
```

**Key data structure:**
```rust
struct DeepTask {
    id: usize,
    prompt: String,
    worktree_path: PathBuf,
    branch_name: String,     // deep/<session>/<n>
    pane_id: Option<String>,
    status: TaskStatus,      // Pending | Running | Done | Failed
    diff: Option<String>,
    output: Option<String>,
}
```

**Review UX:** Results presented in parent document with collapsible diffs, agent output, and approve/reject controls. Approved changes cherry-picked from worktree branches.

**Integration:** tmux-router (stashed panes, TmuxBatch), registry (deep: prefix), CRDT (result write-back), compact (post-merge cleanup).

**Files:** `deep.rs` (orchestration), `worktree.rs` (git worktree lifecycle), `deep_review.rs` (result presentation + merge).

**Limits:** Default `--parallel 3`. Failed tasks preserve worktree for inspection/retry.

Implement

### Re: `agent-doc deep` — Implemented

Two new modules added:

**`worktree.rs`** — git worktree lifecycle management:
- `create(project_root, session_id, index)` → `WorktreeInfo { path, branch }`
- `diff(worktree_path)` → unified diff string
- `remove()`, `cleanup_session()`, `list_session()` — lifecycle helpers
- 9 unit tests (all passing)

**`deep.rs`** — orchestration pipeline:
1. Finds project root, reads frontmatter for tmux/session config
2. Creates worktree per `--task` flag via `worktree::create`
3. Writes prompt to `.agent-doc-prompt.txt` in each worktree
4. Spawns `claude -p --output-format json` in stashed tmux panes (one per worktree)
5. Polls for pane death every 2s with configurable `--timeout`
6. Collects `.agent-doc-result.json` + `worktree::diff()` from each worktree
7. Formats results as markdown with collapsible diffs and stderr logs

**CLI:**
```bash
agent-doc deep plan.md \
  --task "Implement auth" \
  --task "Write tests" \
  --model opus \
  --timeout 600 \
  --dry-run
```

**Tests:** 460 total (9 new worktree tests + 451 existing). All passing, compiles clean.

Show me an example of using markdown agent doc to trigger agent-doc deep
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- Multi-tmux-session per editor ([#17](https://github.com/btakita/agent-doc/issues/17))
- Multi-editor per tmux session ([#18](https://github.com/btakita/agent-doc/issues/18))
- Plugin phases 5-7 — component folding, gutter icons, structure view ([#19](https://github.com/btakita/agent-doc/issues/19))
- Multi-agent backends — codex, gemini ([#20](https://github.com/btakita/agent-doc/issues/20))
- `agent-doc deep` — parallel subagent fan-out with worktrees ([#21](https://github.com/btakita/agent-doc/issues/21))
- `agent-doc status` — Ratatui TUI dashboard ([#22](https://github.com/btakita/agent-doc/issues/22))
- Sub-profiles for submodule workflows ([#23](https://github.com/btakita/agent-doc/issues/23))
- Docker sandboxing (`--sandboxed`) ([#24](https://github.com/btakita/agent-doc/issues/24))
- `agent/api.rs` — direct HTTP API backend ([#25](https://github.com/btakita/agent-doc/issues/25))
- `agent-doc init --dashboard` ([#26](https://github.com/btakita/agent-doc/issues/26))
- Capture/log skill workflow ([#27](https://github.com/btakita/agent-doc/issues/27))
- Blog post: document modes with screencast ([#28](https://github.com/btakita/agent-doc/issues/28))
- `agent-doc stream` — bridge terminal to components ([#29](https://github.com/btakita/agent-doc/issues/29))
<!-- /agent:pending -->
