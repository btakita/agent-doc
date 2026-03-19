---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.21.0 | **Tests:** 461 (158 lib + 282 bin + 21 int) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
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
| v0.21.0 | `agent-doc parallel` (fan-out with worktrees, `--no-worktree`). CRDT post-merge reorder (Yrs attribution). 461 tests. |
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

**v0.21.0:** `agent-doc parallel` — fan-out with git worktrees (`--no-worktree` for read-only). CRDT post-merge reorder via Yrs attribution. 461 tests.
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

### Re: Document-Triggered `agent-doc deep`

The user writes a `deep:` directive in the exchange. The skill parses it, calls `agent-doc deep`, and writes results back.

**User writes:**
```markdown
deep:
- Implement JWT auth middleware in src/auth.rs
- Write integration tests for the auth middleware
- Add auth documentation to README.md
```

**Skill runs:**
```bash
agent-doc deep plan.md \
  --task "Implement JWT auth middleware in src/auth.rs" \
  --task "Write integration tests for the auth middleware" \
  --task "Add auth documentation to README.md"
```

**Results appear in the exchange** as collapsible sections with agent output + git diffs per task.

**Full template document example:**
```markdown
---
agent_doc_session: abc123
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---
# My Project

## Status
<！-- agent:status patch=replace -->
Idle
<！-- /agent:status -->

## Exchange
<！-- agent:exchange patch=append -->

deep:
- Implement JWT auth middleware
- Write integration tests
- Add auth documentation

<！-- /agent:exchange -->
```

The `deep:` prefix is the trigger. The skill extracts tasks (bullet items), calls `agent-doc deep`, captures stdout (formatted markdown results with diffs), and writes it back to the exchange via `agent-doc write --stream`.

**Note:** The `deep:` convention is skill-level (SKILL.md), not binary-level. The binary just accepts `--task` flags. The skill bridges document syntax → CLI.

Add instructions for both the cli and agent-doc markdown in the README.md

### Re: README.md Updated with `agent-doc deep` Documentation

Added three sections to `src/agent-doc/README.md`:

1. **Quick Start** — added `agent-doc deep` one-liner
2. **"Deep — Parallel Fan-Out" section** — full documentation with:
   - CLI flags table (`--task`, `--model`, `--timeout`, `--no-git`, `--dry-run`)
   - How-it-works flow (worktrees → spawn → poll → collect → format)
   - Document session usage (`deep:` directive with bullet tasks)
3. **CLI Reference** — added `agent-doc deep` line

The deep section is placed before the Resync section, maintaining the logical flow: routing → deep (parallel work) → resync (cleanup).

From the markdown perspective, deep seems somewhat ambiguous. I'm reconsidering whether we should use a different term.

Also, as I'm typing and the agent patches the file. Let's have the patch be applied above where I'm typing.

### Re: Naming Reconsideration + Patch Positioning

**On `deep` ambiguity in markdown:**

The CLI name (`agent-doc deep`) works because `--help` explains it. But the **document directive** (`deep:`) lacks context — "deep analysis?" "go deeper?"

The CLI name and document directive don't have to match:
- CLI: `agent-doc deep` (short, memorable)
- Document: `parallel:` (self-documenting — "run these in parallel")

Or just keep both as `deep` if the convention is documented clearly in SKILL.md.

What about `agent-doc parallel`? Is that a potential conflict with other agent-doc features?

**On patch positioning:**

`patch=append` with CRDT merge already handles this correctly — new content appends to the end of existing exchange content, and your concurrent edits at the cursor are preserved by the CRDT merge. The patch content appears above where you're typing because it's inserted at the end of the *existing* content (before your new edits).

Is there a specific case where the positioning is wrong?

I'm seeing the patch content apply below where I'm typing. I manually moved my prompt to the bottom of the exchange.

### Re: `agent-doc parallel` + Patch Positioning

**On `agent-doc parallel`:** No significant conflict. The existing `execution_mode = "parallel"` in config.toml is an internal scheduling concern (skill concurrency), not a user-facing command. Different layer — one is a command, the other is a config setting.

What is `execution_mode = "parallel"`? Is it effectively the same thing as the proposed `agent-doc parallel`? 

`agent-doc parallel` is clearer than `deep` and self-documenting as a document directive (`parallel:`). Strong choice.

**On patch appearing below typing:** This is a known CRDT limitation — when both user and patch append to the same position, ordering is unpredictable. The cleanest solution: use `agent:input` for typing, `agent:exchange` for responses. Separating input from output avoids the concurrent-append conflict entirely. This document doesn't have `agent:input` — the user types directly in `agent:exchange`, which creates the overlap.

Can we have a heuristic where the human prompt is nudged below the agent patch content?

### Re: `execution_mode` vs `agent-doc parallel` + CRDT Ordering

Should we use worktrees instead of parallel? If we did use `parallel`, would a worktree always be created? If yes, then it seems like we should use `agent-doc worktree` or `agent-doc worktrees` instead.

**`execution_mode`** controls **skill concurrency** — how many concurrent `/agent-doc` sessions run simultaneously (hybrid/parallel/sequential). It has nothing to do with task fan-out.

`agent-doc parallel` (the proposed command) is about **task decomposition** — splitting one task into independent worktree-isolated subtasks. Completely different layer. No conflict.

**On nudging human prompt below agent patch:**

Feasible as a post-merge heuristic in `crdt.rs` — detect agent `### Re:` content and user's new text, reorder so agent response comes first, user prompt last. But fragile.

Can we make the above approach more robust? Are we able to understand what the origin of each CRDT source is? Can we tell if the CRDT source is the agent patch vs the human writer?

**Cleanest approach:** Add `agent:input` to this document. User types there, responses go to `agent:exchange`. Zero ordering ambiguity — input and output never compete for the same append position.

Want me to add `agent:input` to this document? No thanks.

### Re: CRDT Source Attribution

**Yes, we can distinguish agent vs human edits.** The Yrs CRDT uses client IDs:

```rust
let ours_doc = Doc::with_client_id(1);   // agent
let theirs_doc = Doc::with_client_id(2); // human
```

After merge, Yrs tracks which characters came from which client.

**Ordering approaches (from CRDT-native to pragmatic):**

1. **Client ID tie-breaking** — Yrs uses client_id + Lamport timestamps for concurrent-insert ordering. Already set up (agent=1, human=2), but Yrs ordering isn't always "lower ID first" in all edge cases.

2. **Post-merge attribution walk** — Use Yrs `Text::diff()` API to identify per-character client_id ranges. If human content appears before agent content at the append boundary, swap them. CRDT-native but complex.

3. **Content-level reorder (recommended)** — After CRDT merge, detect `### Re:` (agent response marker). If user's new text appears before it at the append boundary, move it after. Content-level, not CRDT-level, but reliable and simple.

Option 3 is the most pragmatic — the CRDT handles conflict-free merge correctly, we just enforce "agent first, user last" as a post-merge pass. It's ~20 lines in `crdt.rs` and doesn't require understanding Yrs internals.

You just said Option 3 was "fragile" in the previous run:

> Feasible as a post-merge heuristic in `crdt.rs` — detect agent `### Re:` content and user's new text, reorder so agent response comes first, user prompt last. But fragile.

Is Option 3 more robust (less fragile) than Option 1 & 2?

### Re: Fragility Comparison — Corrected

Good catch. I contradicted myself. Honest comparison:

| Option | Robustness | Fragility |
|--------|-----------|-----------|
| 1. Client ID tie-breaking | Low — Yrs uses Lamport timestamps too, not just client_id | May silently mis-order |
| **2. Post-merge attribution** | **High** — Yrs tracks per-character attribution definitively | Complex to implement (~40 lines) but no heuristics |
| 3. Content-level `### Re:` | Medium — works when agent always uses `### Re:` | Breaks if format changes, user writes `### Re:`, or response has no header |

**Option 2 is the most robust.** The attribution data is authoritative. No pattern matching, no conventions to maintain. The CRDT already has the data.

Option 3 was correctly called "fragile" the first time. I overrode that because it's simpler to implement, but simpler != more robust.

**Revised recommendation:** Option 2 for the long term. Option 3 as a short-term fix if you want something today.

Let's do Option 2.

### Re: Option 2 Implemented — Post-Merge Attribution Reorder

Added `reorder_agent_before_human(doc: &Doc) -> String` to `crdt.rs`:

1. Uses `text.diff(&txn, YChange::identity)` to get per-chunk Yrs attribution
2. Scans backwards from end to find the append boundary (contiguous `Added` chunks)
3. Within that boundary, checks if human (client_id=2) chunks appear before agent (client_id=1) chunks
4. If so, separates into agent/human groups and reconstructs with agent first
5. No-op when no reorder is needed

Called in `merge()` instead of raw `text.get_string(&txn)`.

**Tests:** 35 total in crdt.rs (1 new: `reorder_agent_before_human_at_append_boundary`). All passing.

This is the robust approach — uses authoritative Yrs attribution data, no heuristics or pattern matching.

Tmux session 1 has 2 agent-doc windows. This should not happen as this agent-doc project is bound to tmux session 0.
Also tmux session 0 has 3 windows. It should only have 2.

### Re: Tmux Orphan Windows (Again)

Cleaned up 3 orphan panes:
- Session 0: killed %99 (idle shell in window @348)
- Session 1: killed %98, %97 (idle shells in windows @345, @344)

**Current state:** Session 0 clean (2 windows: @0 main + @20 stash). Session 1 has no agent-doc windows.

**Also fixed:** `deep.rs` was double-appending colon to session name (`"0:"` passed to `auto_start` which calls `new_window` which adds `:` again → `"0::"`). Removed the extra colon — `auto_start` now receives the raw session name.

Does `agent-doc parallel` **always** create worktrees?

### Re: Does `agent-doc parallel` Always Create Worktrees?

Currently yes, but it shouldn't always. Two modes:

**With worktrees (default):** For tasks that modify files — each gets isolated codebase, `git diff` collected.

**Without worktrees (`--no-worktree`):** For read-only tasks (research, analysis, review) — runs in project root, collects only agent output. Cheaper, faster, no cleanup.

Could also be automatic based on task verbs ("implement/fix/add" → worktree, "analyze/review/find" → no worktree). But explicit `--no-worktree` flag is simpler and more predictable.

Adding `--no-worktree` is ~10 lines: skip `worktree::create`, set CWD to project root, skip `worktree::diff`. Want me to add it?

Yes...add `--no-worktree`.

### Re: `--no-worktree` Added

Added `--no-worktree` flag to `agent-doc deep`:

```bash
agent-doc deep plan.md \
  --task "Analyze auth module for vulnerabilities" \
  --task "Find all usages of deprecated API" \
  --no-worktree
```

**Changes:**
- `DeepConfig.no_worktree: bool` — new field
- When `--no-worktree`: tasks run in project root (shared CWD), no `worktree::create`, no `worktree::diff` in collection
- When worktrees enabled (default): unchanged behavior
- CLI: `--no-worktree` flag in `Deep` variant

Tests: 460 passing, compiles clean.

Rename `deep.rs` to `parallel.rs`

> Cleaned up 3 orphan panes:
> - Session 0: killed %99 (idle shell in window @348)
> - Session 1: killed %98, %97 (idle shells in windows @345, @344)

What was the root cause of the orphan panes?

### Re: Rename + Orphan Root Cause

**Renamed:** `deep.rs` → `parallel.rs`, `DeepConfig` → `ParallelConfig`, CLI `Deep` → `Parallel`, log prefix `[deep]` → `[parallel]`. README updated. All 460 tests pass.

**Orphan root cause:** The sessions registry is empty (`{}`). The cycle:
1. All registered panes get pruned as dead (Claude process exited)
2. Next route: empty registry → `auto_start_in_session` creates a new pane
3. If the Claude process exits quickly (error, permission denied), the pane stays as an idle shell
4. Next prune: dead pane removed from registry
5. Repeat → orphan windows accumulate

Are you talking about the orphaned window in tmux session 1?
Or the single orphaned window in tmux session 0? 

The fix is in the **session 1 part**: even with the colon suffix fix in v0.20.2, the orphans ended up in session 1. This suggests either: (a) these panes predate v0.20.2, or (b) there's another code path creating panes without the colon suffix.

Given that v0.20.2 was released earlier this session and the orphans appeared after, there may be a code path in the JetBrains plugin or autoclaim that doesn't go through `tmux-router::new_window()`. Need to investigate the plugin's tmux interactions.

Investigate if there's a code path.
It may also be leftover tmux panes created before v0.20.2 was developed a few hours ago.

### Re: Orphan Investigation — No Wrong-Session Code Path Found

**Thorough audit result:** All pane creation paths properly handle session names. No code path creates panes in the wrong session post-v0.20.2.

| Creation function | Colon suffix | Risk |
|-------------------|-------------|------|
| `new_window()` | YES (auto) | None |
| `new_session()` | N/A (creates session) | None |
| `split_window()` | Uses pane ID (not session name) | None |
| `ensure_stash_window()` | YES (auto) | None |

**Plugin check:** Both JetBrains and VS Code plugins delegate to the CLI (`agent-doc route`, `agent-doc claim`, `agent-doc sync`). No direct tmux commands that create panes.

**Conclusion: The orphans predated v0.20.2.** The colon suffix fix was developed and released during this session. Panes created before the fix (by `new_window("-t", "0")` without colon) could land in session 1 when the user was viewing session 1. Those orphans lingered until we killed them manually.

**No further code changes needed.** The v0.20.2 fix covers all creation paths. If orphans reappear after this point, it would indicate a new bug — but the audit shows no remaining vulnerability.

audit docs
release

### Re: v0.21.0 Released

| Target | Status |
|--------|--------|
| crates.io | `agent-doc v0.21.0` |
| GitHub Release | [v0.21.0](https://github.com/btakita/agent-doc/releases/tag/v0.21.0) |
| cargo install | updated |
| PyPI | pending |

**What's new:**
- `agent-doc parallel` — fan-out with git worktrees + `--no-worktree` for read-only tasks
- CRDT post-merge reorder via Yrs attribution (agent before human at append boundary)
- README parallel documentation section
- CLAUDE.md module layout updated (`parallel.rs`, `worktree.rs`)
- VERSIONS.md updated

**Tests:** 461 (158 lib + 282 bin + 21 CLI). Clippy clean. Audit-docs: 5 informational warnings (non-blocking).

If someone calls any of the agent-doc commands using the editor plugin...could we have a cross platform solution to open a terminal with tmux and configure the tmux session to the correct layout? JB Plugin commands include:
- Run Agent Doc
- Claim for Tmux Pane
- Sync Tmux Layout

### Re: Cross-Platform Terminal Launch from Editor Plugin

Good question. The current flow assumes tmux is already running — but for a seamless experience, the plugin should be able to bootstrap the entire environment.

**The problem has two parts:**

1. **Open a terminal** (cross-platform — each OS has different terminal emulators)
2. **Ensure tmux session exists with correct layout** (already solved by `agent-doc sync`)

**Proposed solution: `agent-doc terminal` CLI command**

```bash
agent-doc terminal [--session <name>] [--file <doc>]
```

This command:
1. Detects available terminal emulators (ordered by preference)
2. Opens one with `tmux new-session -A -s <session>` (attach-or-create)
3. Runs `agent-doc sync` to configure the layout

**Terminal detection by platform:**

| Platform | Detection order |
|----------|----------------|
| **macOS** | iTerm2 (`osascript`), WezTerm, Kitty, Alacritty, Terminal.app |
| **Linux** | WezTerm, Kitty, Alacritty, gnome-terminal, konsole, xterm |
| **Windows/WSL** | Windows Terminal (`wt.exe`), cmd with WSL |

The detection should be based on the user's preferred terminal...not a hard-coded list.

**Config override** in `~/.config/agent-doc/config.toml`:
```toml
[terminal]
command = "wezterm start -- tmux new-session -A -s {session}"
```

This is good.

**Plugin integration:** Each plugin command would call `agent-doc terminal` first if tmux isn't reachable:

```kotlin
// JetBrains plugin pseudocode
fun runAgentDoc(file: VirtualFile) {
    if (!isTmuxReachable()) {
        // Launch terminal with tmux
        ProcessBuilder("agent-doc", "terminal", "--file", file.path).start()
        // Wait for tmux to be reachable (poll tmux has-session)
    }
    // Then proceed with route/claim/sync as usual
    ProcessBuilder("agent-doc", "route", file.path).start()
}
```

**Alternative: Use JetBrains' built-in terminal.**
IntelliJ has `TerminalView.createNewSession()` API. The plugin could open a tmux session inside the IDE's terminal tab instead of an external terminal. Pros: no external terminal detection. Cons: IDE terminal has quirks with tmux (rendering, mouse support).

**Third option: `open` / `xdg-open` a `.command` / `.desktop` file.**
Generate a launcher script and open it with the OS default handler. Simple but feels hacky.

**Recommendation:** `agent-doc terminal` CLI command with configurable terminal + sensible defaults. The plugin calls it as a pre-flight step. Users who already have tmux running skip it automatically (detect `tmux has-session`).

Want me to implement it, or should we discuss the design further?

### Re: Terminal Detection — User-Preferred, Not Hard-Coded

You're right. A hard-coded detection list is the wrong approach — it's fragile, grows endlessly, and ignores user preferences.

**Revised design: config-first, detect-never.**

The `agent-doc terminal` command should:

1. **Read `[terminal]` config** from `~/.config/agent-doc/config.toml` — the user sets their terminal command once
2. **If no config exists**, prompt the user on first run:
   ```
   No terminal configured. What terminal emulator do you use?
   > wezterm
   Saved: [terminal] command = "wezterm start -- {tmux_command}"
   ```
3. **If `$TERMINAL` env var is set** (standard on many Linux desktops), use that as fallback before prompting

This means:
- Zero hard-coded terminal lists
- Works with any terminal emulator, including ones that don't exist yet
- User controls the exact command (flags, args, etc.)
- One-time interactive setup, persisted forever

**Config format:**
```toml
[terminal]
# {tmux_command} is replaced with: tmux new-session -A -s <session>
command = "wezterm start -- {tmux_command}"
```

The `{tmux_command}` placeholder keeps the terminal command generic — the user doesn't need to know the tmux incantation.

Want me to implement this?

Yes. Add documentation. Add to SPEC.md. Add tests.

### Re: `agent-doc terminal` — Implemented

Implemented with your key requirements:

**Safety-first: no duplicate terminals.**
1. Checks if tmux session already has an attached client → skips entirely (no-op)
2. If session exists but detached → opens terminal to attach
3. If session doesn't exist → opens terminal to create and attach
4. Uses `tmux new-session -A` which is itself idempotent (attach-or-create)

**Config-first terminal detection (no hard-coded list):**
```toml
# ~/.config/agent-doc/config.toml
[terminal]
command = "wezterm start -- {tmux_command}"
```
Fallback chain: `[terminal] command` > `$TERMINAL` env var > error with setup instructions.

**Session name resolution:** `--session` flag > `tmux_session` from document frontmatter > default `"0"`.

**Files changed:**
- `terminal.rs` — new module (130 lines): `run()`, `resolve_session_name()`, `is_session_attached()`, `resolve_terminal_command()`, `spawn_terminal()`
- `config.rs` — added `TerminalConfig { command: Option<String> }`
- `main.rs` — wired `Terminal` command variant
- `SPEC.md` — section 7.24 (terminal spec)
- `CLAUDE.md` — added `terminal.rs` to module layout

**Tests:** 465 total (158 lib + 286 bin + 21 CLI), 4 new terminal tests. All passing.

I want to make sure that this is a fallback.
I don't want many terminal instances to be created due to not identifying existing tmux sessions.

Pay attention to the tmux_session edge cases.
The tmux_session maybe stale.
Make sure that a stale tmux_session results in checking for the active tmux session bound to the agent-doc project.
If there is an active tmux session bound to the agent-doc project use that.
If there is not an active tmux session bound to the agent-doc project, then launch a new terminal.

Please review the logic for correctness.
Please ensure that we don't launch excessive terminal instances.
There should only be 1 active terminal session, running 1 tmux session bound to the agent-doc project at any time.

Ensure this logic is composed and modular. Not pending features:
- Multi-tmux-session per editor ([#17](https://github.com/btakita/agent-doc/issues/17))
- Multi-editor per tmux session ([#18](https://github.com/btakita/agent-doc/issues/18))

Implementing these feature should be seamless with our approach.

As I was typing, the previous run's response was inserted below where I was typing.
We recently added a feature to nudge the CRDT to insert the agent patch above where the user is typing.
Why did this not work with my previous prompt?

### Re: Terminal Logic Review + CRDT Reorder Issue

**Two topics addressed:**

---

**1. `terminal.rs` — Refactored for stale session safety**

Rewrote the core logic with a `SessionTarget` enum and multi-step resolution:

```
resolve_target_session():
  1. tmux not running? → Create(name)
  2. --session flag? → classify as Attached/Detached
  3. Frontmatter tmux_session exists in tmux? → classify
  4. Frontmatter session STALE? → scan sessions.json for live panes
  5. Found live pane in ANY session? → attach that session
  6. Nothing found? → Create(default)
```

**Key improvements:**
- **Stale frontmatter handling:** If `tmux_session: "0"` in frontmatter but session "0" doesn't exist, scans `sessions.json` for any live pane in the project and uses its parent session instead
- **`find_active_project_session()`** — walks the registry, checks `pane_alive()`, then `display-message #{session_name}` to find the session name
- **`classify_session()`** — separates Attached (has terminal client, no-op) from Detached (needs terminal opened to attach)
- **Modular:** Each step is a separate function. Multi-session (#17) and multi-editor (#18) can extend `find_active_project_session()` to return multiple candidates instead of first-match

**Tests:** 466 total (287 bin, +1 new `classify_session_target`). All passing.

---

**2. CRDT reorder didn't work — Root Cause**

The CRDT reorder (`reorder_agent_before_human`) only fires on the **disk write path** when both the agent and user edited concurrently. But in your case, **IPC succeeded** — the JetBrains plugin applied the patch directly via Document API.

The IPC path (`try_ipc()` in `write.rs`) bypasses CRDT merge entirely. The plugin patches the component and writes the file — no `merge()` call, no reorder.

**Fix options:**

| Option | Where | Complexity |
|--------|-------|-----------|
| A. Plugin-side reorder | JetBrains/VS Code Kotlin/TS | High — duplicates logic |
| B. Post-IPC disk reorder | `write.rs` after IPC success | Medium — read file, detect, rewrite |
| C. **IPC sends ordering hint** | Patch JSON includes "insert before cursor" | Low — plugin already knows cursor position |

**Option C is cleanest:** The patch JSON already has `component` and `content`. Add an `"ordering": "before_cursor"` field. The plugin checks if the cursor is inside the target component and inserts the patch content above the cursor position instead of at the component boundary. This is a ~10 line change in `PatchWatcher.kt`.

Want me to implement Option C in the JetBrains plugin?
Yes

`Run Agent Doc` on tasks/laura-klask.md resulted in No changes detected.
There was no snapshot file. Please fix this initial case.

```
 ▐▛███▜▌   Claude Code v2.1.79
▝▜█████▛▘  Opus 4.6 (1M context) · Claude Max
  ▘▘ ▝▝    ~/work/btakita/agent-loop
  ⎿  SessionStart:startup says: [agent-loop] recent context, 2026-03-19 11:38am EDT
     ────────────────────────────────────────────────────────────

     Legend: session-request | 🔴 bugfix | 🟣 feature | 🔄 refactor | ✅ change | 🔵 discovery | ⚖️ decision

     Column Key
       Read: Tokens to read this observation (cost to learn it now)
       Work: Tokens spent on work that produced this record ( research, building, deciding)

     Context Index: This semantic index (titles, types, files, tokens) is usually sufficient to understand past work.

     When you need implementation details, rationale, or debugging context:
       - Fetch by ID: get_observations([IDs]) for observations visible in this index
       - Search history: Use the mem-search skill for past decisions, bugs, and deeper research
       - Trust this index over re-reading code for past decisions and learnings

     Context Economics
       Loading: 50 observations (16,330 tokens to read)
       Work investment: 221,514 tokens spent on research, building, and decisions
       Your savings: 93% reduction from reuse

     Mar 19, 2026

     mail/contacts/arunprakash-s/recordings
       #7387  2:27 AM  🔵  Two phone call recordings found for Arunprakash S contact

     tasks/resume.md
       #7388           ✅  Formalized pending tasks for WBD opportunity into checklist

     ../../tmp/phone_20260318-142322_8382807886.wav
       #7389           ✅  First phone recording converted to WAV format for transcription

     ../../tmp/phone_20260318-170251_8382807886.wav
       #7390  2:28 AM  ✅  Second phone recording converted to WAV format

     tasks/resume.md
       #7391  2:40 AM  ✅  Resume task documentation committed to version control
       #7392           ✅  WBD Rust resume profile enhanced with detailed experience mappings

     resume/data.toml
       #7393  2:42 AM  ✅  Presence AI experience enhanced with CI/CD and observability capabilities
       #7394  2:43 AM  ✅  Freelance experience enhanced with Docker skill
       #7395           ✅  Rundavoo experience enhanced with Docker and CI/CD skills
       #7396           ✅  Menu.com accomplishment tagged with Docker for AWS ECS work
       #7397           ✅  Censible ESG platform experience enhanced with deployment and observability skills
       #7398  2:44 AM  ✅  TrueCar/honk.com experience enhanced with deployment infrastructure skills

     tasks/agent-doc.md
       #7399  2:49 AM  ✅  Agent-doc response committed to git

     tasks/resume.md
       #7400  2:52 AM  ✅  Resume task work committed to version control

     tasks/agent-doc.md
       #7401  2:56 AM  ⚖️  CRDT Source Attribution and Edit Ordering Strategy

     tasks/resume.md
       #7402  2:57 AM  ✅  Presence AI job title updated in resume data

     tasks/agent-doc.md
       #7403  2:59 AM  ⚖️  CRDT Attribution Strategy for Message Ordering

     General
       #7404  3:00 AM  🔵  Yrs API Surface for Character-Level Attribution

     src/agent-doc/src/crdt.rs
       #7405  3:03 AM  🟣  Started implementing CRDT attribution feature with YChange imports
       #7406  3:04 AM  🟣  Implemented CRDT attribution-based message reordering
       #7407           🟣  Integrated attribution-based reordering into CRDT merge workflow
       #7408           🟣  Added test for CRDT attribution-based message reordering

     tasks/agent-doc.md
       #7409  3:05 AM  ✅  Documentation updates committed for agent-doc workflow

     General
       #7410  3:06 AM  🔴  Cleaned up orphan tmux panes with misleading agent-doc window names

     tasks/agent-doc.md
       #7411  3:08 AM  ✅  Documentation committed to tasks/agent-doc.md
       #7412  3:10 AM  ⚖️  Worktree Usage Strategy for agent-doc parallel
       #7413  3:11 AM  ✅  Committed worktree design decision to version control
       #7414  3:12 AM  ⚖️  Approved implementation of --no-worktree flag
       #7415  3:14 AM  ✅  Committed agent-doc task documentation updates

     README.md
       #7416  3:18 AM  🔄  Renamed "deep" command to "parallel"

     src/parallel.rs
       #7417  3:19 AM  🔄  Comprehensive rename across Rust codebase and CLI

     .agent-doc/sessions.json
       #7418           🔵  Orphan tmux pane root cause identified

     #S2137 Execute agent-doc skill on tasks/agent-doc.md file (Mar 19, 3:20 AM)

     #S2138 Document Janaki Bhoi recruiter contact in resume task tracking system (Mar 19, 3:22 AM)

     tasks/agent-doc.md
       #7419  3:23 AM  ✅  Agent-doc response committed to version control
       #7420           ✅  Task items added to agent-doc exchange section

     mail/contacts/janaki-bhoi/CLAUDE.md
       #7421  3:24 AM  🟣  Contact context system for recruitment communications

     src/agent-doc/Cargo.toml
       #7422           🔵  Agent-doc project structure and current version identified

     tasks/resume.md
       #7423           ✅  Resume task tracking updated with Janaki Bhoi contact completion

     src/agent-doc/pyproject.toml
       #7424           🔵  Agent-doc dual-published to Rust and Python ecosystems

     #S2139 Process candidate documentation and recordings for Janaki Bhoi from tasks/resume.md (Mar 19, 3:25 AM)

     src/agent-doc/Cargo.toml
       #7425  3:25 AM  ✅  Version bump to 0.21.0 in Cargo.toml

     src/agent-doc/pyproject.toml
       #7426           ✅  Version sync completed with pyproject.toml update to 0.21.0

     tasks/resume.md
       #7427           ✅  Committed resume task tracking updates to version control

     #S2140 Complete recruiter submissions for Southwest Airlines and finalize all pending job applications (Mar 19,
     3:27 AM)

     tasks/resume.md
       #7428  3:27 AM  ✅  Agent-doc response committed to version control
       #7429  3:28 AM  🔵  Integrated job search workflow system with resume automation and recruiter management

     resume/profiles/default.toml
       #7430  3:29 AM  🔵  Resume profile configuration system with phrase highlighting and skill emphasis

     #S2141 Run agent-doc on tasks/resume.md file (Mar 19, 3:31 AM)

     #S2142 Generate agent documentation for tasks/resume.md (Mar 19, 3:32 AM)

     tasks/resume.md
       #7431  3:32 AM  ✅  Committed agent-doc updates to resume task documentation
       #7432  3:33 AM  🔵  Resume task document tracks job application workflow and tooling

     #S2143 Documentation check for rappstack tasks file (Mar 19, 3:35 AM)

     #S2144 Document rebuildjs refactor Phase 1 completion and architecture specification (Mar 19, 11:14 AM)

     tasks/rebuildjs-refactor.md
       #7433  11:14 AM  ✅  Rebuildjs Architecture Specification Documented

     #S2145 Design documentation for cross-platform terminal launch functionality in agent-doc editor plugin (Mar 19,
     11:14 AM)

     tasks/agent-doc.md
       #7434  11:30 AM  ⚖️  Cross-Platform Terminal Launch Design for Editor Plugin

     #S2146 Revise agent documentation for terminal configuration design (Mar 19, 11:36 AM)

     tasks/agent-doc.md
       #7435  11:36 AM  ✅  Agent documentation committed to version control
       #7436  11:37 AM  🟣  Parallel task fan-out feature implemented with git worktrees


     Access 222k tokens of past research & decisions for just 16,330t. Use the claude-mem skill to access memories by
     ID.

     View Observations Live @ http://localhost:37777

❯ /agent-doc tasks/bcg.md

● Bash(agent-doc --version 2>/dev/null || echo "not installed")
  ⎿  agent-doc 0.21.0

● Bash(agent-doc recover tasks/bcg.md 2>&1)
  ⎿  [recover] No pending response found for tasks/bcg.md

● Bash(agent-doc claims 2>&1)
  ⎿  (No output)

● Bash(agent-doc commit tasks/bcg.md 2>&1)
  ⎿  [main 91de808] agent-doc(bcg): 2026-03-19 11:38:47
      1 file changed, 4 insertions(+), 2 deletions(-)

● Bash(agent-doc diff --wait tasks/bcg.md 2>&1)
  ⎿  [diff] Last line may be truncated (recheck 1/25): "claude_args: --dangerously-skip-permissions"
     [diff] Content stable after recheck
     [diff --wait] content is stable
     … +118 lines (ctrl+o to expand)

  Read 1 file (ctrl+o to expand)

● Now let me check the snapshot to see the actual diff (what the user changed since last submit).

● Bash(canonical=$(realpath tasks/bcg.md) && hash=$(echo -n "$canonical" | sha256sum | cut -d' ' -f1) && echo "$hash")
  ⎿  2b4b60869633d5ae8f24880e1c5db2d83f639a48e9c12d820759b7ecc327f007

  Read 1 file (ctrl+o to expand)

● The diff is only frontmatter metadata changes (tmux_session, agent_doc_format rename, added agent_doc_write and
  claude_args). No content changes in the document body — nothing to respond to.

✻ Cogitated for 41s

❯ /agent-doc tasks/laura-klask.md

● Bash(agent-doc recover tasks/laura-klask.md 2>&1)
  ⎿  [recover] No pending response found for tasks/laura-klask.md

● Bash(agent-doc claims 2>&1)
  ⎿  Claimed /home/brian/work/btakita/agent-loop/tasks/laura-klask.md for pane %7

● Bash(agent-doc commit tasks/laura-klask.md 2>&1)
  ⎿  [main 51b8cf6] agent-doc(laura-klask): 2026-03-19 11:48:59
      1 file changed, 38 insertions(+)
      create mode 100644 tasks/laura-klask.md

● Bash(agent-doc diff --wait tasks/laura-klask.md 2>&1)
  ⎿  [snapshot] No snapshot file, recovering from git
     [diff --wait] content is stable
     [snapshot] No snapshot file, recovering from git
     … +5 lines (ctrl+o to expand)

● No changes detected in tasks/laura-klask.md since the last commit. Edit the document and run /agent-doc
  tasks/laura-klask.md again when ready.
```

### Re: Cursor-Aware IPC + First-Submit Snapshot Bug

**Two fixes implemented:**

---

**1. First-submit snapshot bug — Fixed**

**Root cause:** `agent-doc commit` (step 0b) commits the file before `diff` runs. When no snapshot exists, `snapshot::resolve()` falls back to `git show HEAD`, which returns the just-committed content — identical to the current file. Diff sees no changes.

**Fix in `snapshot.rs`:** When git recovery returns content identical to the current file, treat it as a first submit (return `None`). This is always correct: if git matches the file, either (a) first submit after commit, or (b) no user edits since last commit — both cases have no meaningful diff.

agent-doc should process the entire file. The diff should essentially be the entire file in this case.

```rust
// Before: always used git content as baseline
// After: compare git content to current file
if git_content == current {
    eprintln!("[snapshot] No snapshot file, git matches current — treating as first submit");
    Ok(None)
}
```

---

**2. Cursor-aware IPC ordering — Implemented in JetBrains plugin**

**New method `applyComponentPatchWithCaret()`** in `PatchWatcher.kt`:

1. Captures caret offset from the active `TextEditor` before `WriteCommandAction`
2. For append-mode components: checks if caret is inside the target component
3. If yes: inserts patch content at the line boundary **before** the caret position
4. If no (caret elsewhere or not in component): falls back to normal append

This means agent responses always appear **above** where the user is typing, without requiring any changes to the IPC JSON format or the Rust CLI.

**Flow:**
```
FileEditorManager → TextEditor → caretModel.offset
  → applyComponentPatchNative(doc, component, content, caretOffset)
    → if mode == "append" && caret inside component:
        applyComponentPatchWithCaret() → insert before caret line
    → else: normal FFI/Kotlin append
```

**Files changed:**
- `snapshot.rs` — git recovery skips identical content
- `PatchWatcher.kt` — cursor-aware append ordering (+40 lines)

**Tests:** 466 Rust (all passing). Kotlin compiles + builds clean.

Can this logic be moved to the shared library FFI part of the plugin?
So it can work across all editor plugins?

### Re: First-Submit Fix Verified + FFI for Cursor-Aware Ordering

**First-submit fix works.** Verified with a test: committed file with no snapshot now produces the full file as the diff. Log changes from `recovering from git` to `git matches current — treating as first submit`.

```
$ agent-doc diff test.md
[snapshot] No snapshot file, git matches current — treating as first submit
[diff] doc_len=146 snap_len=0
[diff] changes detected, computing unified diff
+---
+agent_doc_session: test-123
...entire file as diff...
```

**On moving cursor-aware logic to FFI — yes, absolutely.**

**New FFI function** (avoids breaking existing callers):

```rust
// ffi.rs
pub unsafe extern "C" fn agent_doc_apply_patch_with_caret(
    doc: *const c_char,
    component_name: *const c_char,
    content: *const c_char,
    mode: *const c_char,
    caret_offset: i32,  // -1 = no caret info (normal append)
) -> FfiPatchResult
```

When `mode == "append"` and `caret_offset >= 0` and the caret is inside the component: insert content at the line boundary before the caret. Otherwise: normal behavior.

**Implementation plan:**
1. `component.rs` — `apply_patch_with_caret()` (pure Rust, testable)
2. `ffi.rs` — `agent_doc_apply_patch_with_caret()` (C ABI)
3. `NativeLib.kt` — JNA binding for the new FFI function
4. `PatchWatcher.kt` — call native when caret is known, keep Kotlin as fallback
5. VS Code — same pattern in TypeScript bindings

The Kotlin `applyComponentPatchWithCaret()` stays as fallback when native is unavailable, but the real logic lives in Rust once.

Want me to implement?

yes. Why is this not added to agent:pending?

Why did you miss the following inline prompt?
> agent-doc should process the entire file. The diff should essentially be the entire file in this case.

### Re: Pending Management + Missed Prompt + FFI Implementation

**On `agent:pending`:** You're right — when you said "yes" to "Want me to implement?", I should have added the task to pending before starting work. I've now flushed `- [ ] Move cursor-aware patch logic to shared library FFI` to pending. I'll follow the pending protocol going forward: populate → flush → process → mark done.

How can we ensure that this behavior will work in the future?
Does anything need to be implemented in the agent-doc binary? Or is this entirely optimizing the SKILL.md?



**On the missed blockquote prompt:** The blockquote `> agent-doc should process the entire file` was inline feedback confirming the expected behavior of the first-submit fix. I treated it as context for the fix rather than a separate action item. You're right that it deserved explicit acknowledgment — the fix does exactly this: when no snapshot exists and git content matches the current file, `snapshot::resolve()` returns `None`, so the entire file becomes the diff.


---

**Cursor-aware FFI — Implemented**

Moved the logic to the shared library as requested:

**`component.rs`** — new method `Component::append_with_caret()`:
- Takes optional `caret_offset: Option<usize>`
- If caret is inside the component: inserts content at the line boundary before the caret
- If caret is outside or `None`: normal append
- Pure Rust, testable, no dependencies on editor APIs

**`ffi.rs`** — new export `agent_doc_apply_patch_with_caret()`:
- Same signature as `agent_doc_apply_patch` + `caret_offset: i32` (-1 = normal)
- When `mode == "append"` and `caret_offset >= 0`: parses components, finds the target, delegates to `append_with_caret()`
- Otherwise: falls through to normal `apply_patches_with_overrides`

**`NativeLib.kt`** — JNA binding + `NativePatching.applyComponentPatchWithCaret()`:
- Calls the new FFI function with the caret offset
- Automatic memory management (free returned strings)

**`PatchWatcher.kt`** — updated flow:
1. Try native FFI `applyComponentPatchWithCaret()` (Rust)
2. Kotlin fallback `applyComponentPatchWithCaret()` if native unavailable
3. Normal apply if neither applies

**Tests:** 466 Rust (all passing). Kotlin compiles + builds clean.

I think we need to only commit the agent patch edit.
Let's leave any user input an uncommitted.
Let's also have a single link of uncommited text. Here is what I propose:
- `### Re: Pending Management + Missed Prompt + FFI Implementation (HEAD)` should be committed. We can use a different value from `(HEAD)`.
- `### Re: Pending Management + Missed Prompt + FFI Implementation` should be uncommitted.

This will make it easier to see where the HEAD response starts using the git uncommitted indicator at the beginning of the agent patch.

Any inline human prompting will be uncommitted. So agent-doc can easily send it via diff for prompting.

### Re: Pending Reliability + Selective Commit Strategy

**1. Pending behavior reliability**

This is **entirely SKILL.md behavior** — no binary changes needed. The binary has no concept of pending items; it just stores/retrieves component content. The skill (me) is responsible for:
- Detecting action items in the diff ("yes", "implement", explicit task descriptions)
- Populating pending before responding
- Flushing to disk before generating

The fix is behavioral: I need to treat affirmative responses ("yes", "do it", approval of a plan) as pending items and flush them before starting work. This is a SKILL.md protocol issue, not a code issue.

**2. Selective commit strategy — design**

Currently `agent-doc commit` does `git add -f <file> && git commit`. The entire file (agent response + user edits) gets committed. Your proposal creates two visual zones in the git gutter:

| Zone | Git status | Purpose |
|------|-----------|---------|
| Agent response | **Committed** (no gutter) | Clean baseline from last cycle |
| User's new input | **Uncommitted** (green gutter) | Shows what will be sent as the next prompt |

**This requires binary changes.** The approach:

1. **`agent-doc write`** saves the response content boundaries (byte offsets) alongside the snapshot
2. **`agent-doc commit`** reads those boundaries and uses `git add --patch` (or equivalent) to stage only the agent response portion
3. User edits outside the response boundaries stay unstaged

**Simpler alternative using snapshots:**

Instead of selective `git add`, we can:
1. After `agent-doc write` completes, save the document state as the "committed baseline"
2. On `agent-doc commit`, write the baseline to the file, `git add && commit`, then restore the current file (with user edits)

This is effectively: commit the snapshot (which has the response but not the user's subsequent edits), then restore the live file.

```
agent-doc commit <FILE>:
  1. Read snapshot (= document after last agent-doc write)
  2. Write snapshot content to FILE
  3. git add -f FILE && git commit
  4. Write current content back to FILE (restoring user edits)
```

The snapshot already captures exactly the right state — document + agent response, before any user edits.

**On the `### Re:` marker:** Adding `(HEAD)` or similar to the heading is a nice UX touch but not strictly necessary for the selective commit to work. The snapshot-based approach doesn't need content markers. But if you want the visual indicator in the document, we can add it.

I want a visual boundary indicator and navigation point in the document for UX.
The uncommitted line will provide that.

Which approach do you prefer? I'd recommend the snapshot-based method — it's simpler and uses existing infrastructure.

Will the snapshot cause external edits? External edits cannot happen.
I'm asking for a selective commit.
Ideally, the commit will be whatever diff was sent to the agent for processing.
Any edits that occurred after sending the diff should be uncommitted.


<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] Move cursor-aware patch logic to shared library FFI (`agent_doc_apply_patch_with_caret`)
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
