---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Prior frontmatter bug fix remains complete. New work this cycle is planning only: added a concrete reliability plan for the “response patchback not committed / no patchback cycle ran” failure family. The current diagnosis is that two bugs can surface as the same symptom: (1) fresh Codex sessions can miss the initial route trigger entirely, so `preflight -> write -> commit` never starts; (2) a write/IPC + snapshot save can succeed, but the cycle can still die before `commit_*`, leaving a visible uncommitted assistant patchback in the working tree.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260531-235103.md`*

Compacted content:
- Archived 2 response topic(s): editor-buffer-state-ffi-debounce; editor-buffer-state
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d2...
❯ `project_root_for_doc` is called several times, each requiring disk access. Would lazily-rs be a good fit for caching this?

### Re: lazily-rs for caching project_root_for_doc — gpt-5

Short answer: **not a good fit here.** A `HashMap<PathBuf, Option<PathBuf>>` behind `std::sync::OnceLock` or a function-local `HashMap` is more proportionate if caching is even needed.

Why lazily-rs does not add value:

1. **lazily-rs is a reactive dependency-injection framework** (Context + Slot + Cell with invalidation tracking). `project_root_for_doc` is a pure-ish filesystem walk with no reactive consumers, no dependency graph, and no invalidation semantics needed within a single run.
2. **agent-doc is sequential per run** (no async, per AGENTS.md). There is no long-lived context object threaded across calls. Introducing a `Context` just to cache one `Option<PathBuf>` per document path is architectural overkill.
3. **The function is already cheap.** It walks up `file.parent()` calling `.is_dir()` on `.agent-doc` at each level — typically 1-5 `stat(2)` calls. The filesystem dentry cache makes repeated calls for the same path near-zero cost within a single process lifetime.

If you do see measurable overhead (e.g., in a hot `watch` loop), the proportionate fix is:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static PROJECT_ROOT_CACHE: Mutex<HashMap<PathBuf, Option<PathBuf>>> = Mutex::new(HashMap::new());

pub fn project_root_for_doc_cached(file: &Path) -> Option<PathBuf> {
    let key = file.to_path_buf();
    if let Some(cached) = PROJECT_ROOT_CACHE.lock().unwrap().get(&key) {
        return cached.clone();
    }
    let result = project_root_for_doc(file);
    PROJECT_ROOT_CACHE.lock().unwrap().insert(key, result.clone());
    result
}
```

Or even simpler: since the result for a given file is immutable within a run, just compute it once at the call site and pass `Option<PathBuf>` down.
I plan on having a dependency graph. This is only the first step towards optimizing cachable state.

### Re: lazily-rs dependency graph vision — gpt-5

That reframes the picture significantly. If the end goal is a dependency graph for cachable state — where `project_root_for_doc`, `load_project_for_doc`, `canonicalize`, config parsing, SSH context resolution, frontmatter parsing, etc. all participate as nodes with tracked dependencies and lazy invalidation — then lazily-rs is the right foundation.

The current code has several call chains that recompute downstream results from the same upstream inputs within a single run:

```
file path → canonicalize → project_root_for_doc → load_project_from → ProjectConfig
                                                            ↓
                                              resolve_ssh_context_inputs
                                                            ↓
                                              frontmatter parse → SSH targets / harness args
```

Each arrow is a potential lazily-rs **Slot**: compute once, cache, and automatically invalidate when upstream inputs change. A `Context` threaded through the run would let any function in the chain access cached results without passing them explicitly.

**Where to start:** `project_root_for_doc` is a reasonable first slot — it's a leaf (no deps beyond the file path), it's called from multiple places (`load_project_for_doc`, `resolve_ssh_context_inputs`), and its result is stable per file within a run. From there, build upward:

1. `Slot` for `project_root_for_doc(file: &Path) -> Option<PathBuf>`
2. `Slot` for `load_project_for_doc(file: &Path) -> ProjectConfig` (depends on #1)
3. `Slot` for `resolve_ssh_context_inputs(file: &Path)` (depends on #1, #2)

This gives you the graph incrementally without a big-bang refactor. Each slot is testable in isolation.

**Caveat:** agent-doc runs are sequential and short-lived (single CLI invocation). The dependency graph's value isn't reactive updates mid-run — it's **deduplication** (avoiding redundant `stat`/`read`/`parse` for the same file across `preflight → plan → write → commit`) and **structural clarity** (making the data flow explicit rather than scattered function calls). If the graph later spans multiple documents or a long-lived watch/stream daemon, lazily-rs's invalidation tracking becomes more compelling.
There are long lived Actor processes that share the logic. So the CLI runs are short lived and can have a single run Context while the long lived processes can have a shared Context that persists across the lifetime of the Actor.

### Re: Two Context lifetimes (CLI vs Actor) — gpt-5

That's the right split. lazily-rs's `Context` is designed for exactly this — you create a context per lifetime scope, and slots automatically share cached state within that scope.

**Two contexts, same slot definitions:**

| Context | Lifetime | Owner | Invalidation |
|---------|----------|-------|-------------|
| **Run Context** | Single CLI invocation (`preflight → write → commit`) | Created in `main.rs`, dropped at exit | Implicit — process exit clears everything |
| **Actor Context** | Long-lived process (watch daemon, stream session, controller, session actor) | Created at actor startup, held in actor state | Explicit — `slot.clear()` or `ctx.clear()` when the file/config changes |

The slot *functions* (`project_root_for_doc`, `load_project_for_doc`, etc.) are defined once and work against `&Context`. They don't care which lifetime they're in:

```rust
// slot definitions — lifetime-agnostic
slot!(project_root_for_doc, |ctx: &Context, file: &Path| -> Option<PathBuf> {
    // walk up from file to .agent-doc/
});

slot!(load_project_for_doc, |ctx: &Context, file: &Path| -> ProjectConfig {
    let root = project_root_for_doc(&ctx, file); // cached
    // ...
});

slot!(resolve_ssh_context, |ctx: &Context, file: &Path| -> SshContext {
    let config = load_project_for_doc(&ctx, file); // cached
    // ...
});
```

**CLI path** — one-shot context, maximum deduplication within the run:
```rust
fn main() {
    let ctx = Context::new();
    // preflight, plan, write, commit all share ctx
    run(&ctx, file);
    // ctx drops — no cleanup needed
}
```

**Actor path** — persistent context with selective invalidation:
```rust
struct WatchActor {
    ctx: Context,
    watched_files: HashSet<PathBuf>,
}

impl WatchActor {
    fn on_file_changed(&self, file: &Path) {
        // Invalidate only the slots that depend on this file
        self.ctx.clear(); // or selective: project_root_for_doc(&self.ctx, file).clear()
        // Re-run with fresh cache
        self.process(file);
    }
}
```

The watch daemon's reactive file-watching (zero debounce for CRDT docs) becomes a natural invalidation trigger — `inotify`/`poll` event → `slot.clear()` → next access recomputes. No redundant `stat` walks between events for the same path.

**Migration path:** Define the slot functions in `agent-doc-core` (no `std::fs` — slot bodies call the existing I/O wrappers). CLI `main.rs` creates a throwaway context. Actor structs hold a persistent one. The existing function signatures stay available as non-context entry points for backward compat during migration.
Add #next-steps to begin integration of lazily-rs. Find areas where lazily-rs can help with performance.

### Re: lazily-rs next steps — gpt-5

Codebase audit found significant redundancy. Key metrics:

| Operation | Estimated redundant calls per cycle | Type |
|-----------|-------------------------------------|------|
| `find_project_root()` (directory walk) | 129+ | stat walk per call |
| `canonicalize()` | 303+ | syscall per call |
| `doc_hash()` (SHA-256 + canonicalize) | 73+ | syscall + CPU |
| `frontmatter::parse()` / `parse_for_file()` | 124+ | YAML parse + I/O |
| `load_project_for_doc()` (TOML read + parse) | 18+ | file read + parse |
| `config::load()` (global config) | 5-10+ | file read + TOML parse |

The dependency chain is:

```
file: Path
  → canonicalize() → PathBuf
    → find_project_root() → Option<PathBuf>
      → load_project_config() → ProjectConfig
      → load_global_config() → Config
    → doc_hash() → String
      → snapshot_path, lock_path, crdt_path, baseline_path, pending_path (all derived)
  → read_to_string() → content
    → frontmatter::parse() → (Frontmatter, body)
      → resolve_mode(), ssh_targets, harness config
```

#### Recommended lazily-rs Context Shape

| Slot | Input | Value | Invalidation trigger |
|------|-------|-------|---------------------|
| `canonical_path` | `&Path` | `PathBuf` | Never (within run) |
| `project_root` | `&PathBuf` (canonical) | `Option<PathBuf>` | Never (CLI run); on file-tree change (Actor) |
| `doc_hash` | `&PathBuf` (canonical) | `String` | Never (path is immutable within run) |
| `global_config` | `()` | `Config` | Never (CLI); on SIGHUP (Actor) |
| `project_config` | `&PathBuf` (root) | `ProjectConfig` | Never (CLI); on config file change (Actor) |
| `frontmatter` | `&str` (content) | `(Frontmatter, &str)` | On file content change |
| `snapshot_path` | `(&str hash, &PathBuf root)` | `PathBuf` | Never |
| `ssh_context` | `(&Path, &ProjectConfig)` | `Vec<String>` | When config changes |

#### Priority Order

**P0 — Highest impact, leaf nodes (no upstream deps beyond file path):**

1. `canonical_path` — eliminate 303 redundant syscalls
2. `find_project_root` — eliminate 129 directory walks
3. `doc_hash` — eliminate 73 SHA-256 + canonicalize recomputations
4. `frontmatter::parse` — eliminate 124 YAML re-parses

**P1 — Mid-level derived slots:**

5. `load_project_for_doc` — depends on project_root
6. `config::load` — global, no deps
7. `resolve_ssh_context_inputs` — depends on canonical + project_root + project_config
8. `snapshot::path_for` — depends on doc_hash + project_root

**P2 — Actor/long-lived process integration:**

9. Watch daemon `discover_entries()` — persistent Context, invalidate on file-tree change
10. `SupervisorShared` — cache Config + ProjectConfig + Frontmatter across child restarts
11. `session_actor` operations — persistent Context for canonicalize + project_root
12. Route `run_with_tmux()` — Context scoped to tab-switch burst

#### Implementation Sequence

1. Add `lazily-rs` to `agent-doc-orchestration/Cargo.toml`
2. Define slot functions in a new `context.rs` module in `agent-doc-orchestration/src/`
3. Create a `Context` in `main.rs` / each CLI entry point (throwaway lifetime)
4. Thread `&Context` through the P0 hot path: `preflight → diff → write → finalize`
5. Deduplicate the 8+ copies of `find_project_root` into the single slot
6. Add persistent `Context` to `SupervisorShared` and `WatchEntry` (P2)
How can we detect any changes to the document that may invalidate the slots or cells?

### Re: detecting slot/cell invalidation — gpt-5

lazily-rs invalidation is driven by **Cells** (mutable inputs) and propagated automatically through the dependency graph. The question is what agent-doc events update those Cells.

#### Two invalidation models

**CLI runs — no invalidation needed.** The process is sequential and short-lived. A fresh `Context` is created in `main()`, all slots compute at most once, and the entire context drops on exit.

**Actor/watch processes — event-driven invalidation.** Long-lived processes detect changes and update Cells, which cascades to dependent Slots.

#### Cell → Slot graph

```
Cells (mutable inputs — updated by events):
  canonical_path_cell: Cell<PathBuf>
  file_content_cell: Cell<String>
  project_tree_version: Cell<u64>        // bumped on any file-tree change
  config_version: Cell<u64>              // bumped on config.toml change

Slots (computed — auto-invalidated when upstream Cells change):
  canonical_path ← canonical_path_cell
  project_root   ← project_tree_version, canonical_path
  doc_hash       ← canonical_path
  global_config  ← config_version
  project_config ← project_root, config_version
  frontmatter    ← file_content_cell
  ssh_context    ← canonical_path, project_config
  snapshot_path  ← doc_hash, project_root
```

When `file_content_cell.set(new_content)` is called, only `frontmatter` and its downstream slots are recomputed on next access. `doc_hash`, `project_root`, `global_config` remain cached.

#### What triggers each Cell update

| Event | Cell updated | Detection mechanism |
|-------|-------------|-------------------|
| File content edit | `file_content_cell` | `notify` crate inotify/poll → `wait_for_stable_content` → `cell.set(content)` |
| File rename | `canonical_path_cell` | `try_migrate_renamed` detects path change → `cell.set(new_path)` |
| `.agent-doc/` created/removed | `project_tree_version` | Watch on parent dirs via `notify` → `cell.bump()` |
| `config.toml` modified | `config_version` | Watch on `~/.config/agent-doc/` + `.agent-doc/config.toml` → `cell.bump()` |
| CRDT merge conflict | `file_content_cell` | CRDT merge produces new content → `cell.set(merged)` |
| SIGHUP | `config_version` | Signal handler → `cell.bump()` |

agent-doc already has the detection infrastructure — the `notify` crate in `watch.rs`, `wait_for_stable_content` debounce, and CRDT merge in `write.rs`. The integration point is connecting those existing event sources to lazily-rs Cells instead of (or in addition to) the current ad-hoc recomputation.

#### Concrete wiring example (watch daemon)

```rust
struct WatchActor {
    ctx: Context,
    cells: WatchCells,
}

struct WatchCells {
    file_content: Cell<String>,
    tree_version: Cell<u64>,
    config_version: Cell<u64>,
}

impl WatchActor {
    fn on_file_event(&self, event: FileEvent) {
        match event.kind {
            EventKind::Modify(_) => {
                let content = std::fs::read_to_string(&event.path).unwrap();
                self.cells.file_content.set(content);
            }
            EventKind::Create(_) | EventKind::Remove(_) => {
                self.cells.tree_version.set(self.cells.tree_version.get() + 1);
            }
        }
        // Slots are lazy — they recompute on next access, not eagerly
        self.process(&event.path);
    }
}
```

The key insight: **Cells are updated by the same events agent-doc already handles.** No new detection layer is needed — just wire existing event handlers to `Cell::set()` instead of invalidating local variables or re-calling functions directly.
Do Slots also have an api to be invalidated?

### Re: slot invalidation API — gpt-5

Currently **no.** `clear_slot()` on Context is private (`fn`, not `pub`). Slots can only be invalidated indirectly by changing an upstream Cell via `set_cell()`.

The cascade path from `context.rs:249-305`:

```
set_cell(handle, new_value)
  → PartialEq check: old != new?
    → yes: update value, collect dependents
    → for each dependent slot:
        → clear_slot(id): set value = None, recurse into its dependents
```

**If a Slot has no Cell dependencies**, it will never be invalidated — it computes once and caches forever. This is by design for the current lazily-rs API.

#### Forcing recomputation without a direct Slot API

Two patterns work today:

**Pattern 1: Version Cell as a dummy dependency**
```rust
let version = ctx.cell(0u64);
let my_slot = ctx.slot(|ctx| {
    let _ = ctx.get_cell(&version); // registers dependency
    expensive_computation()
});
// Force recompute:
ctx.set_cell(&version, version_val + 1);
```

This is what the watch daemon `tree_version` / `config_version` Cells from the previous response already do — they exist solely to trigger downstream recomputation.

**Pattern 2: Expose `clear_slot` publicly**

If the agent-doc integration needs direct slot invalidation (e.g., "I know this file's snapshot is stale, clear it"), the API addition is small:

```rust
// In context.rs, add:
pub fn invalidate<T: 'static>(&self, handle: &SlotHandle<T>) {
    self.clear_slot(handle.id);
}
```

This is safe to expose — `clear_slot` already handles the "already cleared" early return and recursive cascade. Whether to add it depends on whether version Cells cover all invalidation needs or whether some Slots genuinely have no logical Cell input but still need explicit refresh.
Can we have SlotHandle.clear(&ctx) instead?
<!-- agent:boundary:5a34e727 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue auto -->
- Can we have SlotHandle.clear(&ctx) instead?
<!-- /agent:queue -->

<!-- agent:backlog -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-31 [#pbc1] Commit-boundary reliability for Codex/IPC sessions: fail closed when fresh-start route trigger never fires, and auto-resume commit when write/snapshot succeeded but commit boundary was interrupted. Plan: `tasks/agent-doc/plans/commit-boundary-reliability-codex-ipc.md`
- 2026-05-31 [#dtmc] scan_text_source parallelized via std::thread::scope in diagnose_cycle
- 2026-05-31 [#editor-buffer-state-ffi-debounce] [recommended] Replace `wait_for_stable_content`'s last-added-line/truncation heuristic with editor-authoritative buffer stability when an IDE plugin is attached. JetBrains and VS Code plugins should report normalized editor state through the FFI bridge to the Session Actor: canonical doc path/session, monotonic editor document version, dirty flag, last edit timestamp, optional save timestamp, optional buffer hash, and ideally current buffer content. Run requests should include the editor version/hash observed at click time so the Session Actor can detect edits after invocation, debounce until the version/hash is stable, and compute the diff against the exact stable editor buffer when available. If no editor state exists, fall back to whole-file content + mtime stability polling. This removes reliance on `extract_last_added_line`, which misses edits above the last inserted line, and reduces disk/snapshot/editor-buffer race classes seen in live IPC/JB/VSCode flows.
<!-- /agent:done -->
