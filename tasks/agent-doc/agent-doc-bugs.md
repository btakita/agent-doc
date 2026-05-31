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
queue_active: false
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
<!-- agent:boundary:40056e3b -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue -->
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
