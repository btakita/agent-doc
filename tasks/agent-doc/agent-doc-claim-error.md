---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.5 released. Commit-on-claim, auto-setup, VCS refresh, --diff-only, skill runbooks, .ai/runbooks convention.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. CI workflows idempotent (release.yml --clobber, pypi.yml skip-existing). `make release` added (CI-first: check→tag→push). Prerelease runbook updated. Gitignore fixed (.bin/ .cargo/ separate). All pushed.*

The jetbrains editor has performance issues navigating and editing tasks/agent-doc/agent-doc-bugs.md. Please look into why.
### Re: JetBrains performance with agent-doc-bugs.md

**Not the file** (125 lines / 6.5KB). It's the git repo state:

1. **Orphaned 3.9 GB blob** in git history — cleaned up via `git gc --prune=now --aggressive` (4.2 GB → 2.4 GB, remaining is submodule data)
2. **7,691 total commits**, 208 for this file — frequent `agent-doc commit` calls
3. **`git status` ~300ms** — JetBrains runs this + annotations on every VCS refresh signal

Each `/agent-doc` cycle triggers commit + VCS refresh → JetBrains re-indexes. With 7K+ commits, annotation/blame/log operations are heavier.

**Mitigations:**
- GC done (removed 1.8 GB orphaned blob)
- `agent-doc clean` could squash old task commits to reduce history depth
- JetBrains plugin could throttle VCS refresh signals
- Disable git annotations for `tasks/` directory in IDE settings

> - `agent-doc clean` could squash old task commits to reduce history depth

I would want to archive the repo with the old history. Are there options to do this?

> - Disable git annotations for `tasks/` directory in IDE settings

Wa can do this.
### Re: Archiving history before squash

Options:
1. **Archive branch**: `git branch archive/pre-squash` → preserves full history, push to remote for backup
2. **Tag**: `git tag archive/pre-squash-2026-04-04` — lighter, same effect
3. **Separate bare clone**: `git clone --bare . ~/archive/agent-loop-full-history.git`
4. **`agent-doc clean`** squashes only agent-doc task commits (not all), single squash commit remains

**Recommended:** Tag pre-squash + squash. Tag costs nothing, preserves full history if needed.

### Re: Disabling git annotations for tasks/

JetBrains options:
- Per-file: right-click gutter → Annotate → uncheck
- Directory: Settings → Version Control → Directory Mappings (heavy-handed)
- **Better: throttle VCS refresh in the agent-doc plugin** — the plugin fires `VirtualFileManager.getInstance().asyncRefresh()` on every commit. Could skip or debounce for task documents.

do
- [ ] Archive history + squash old task commits: add an agent-doc command
- [ ] Throttle VCS refresh in JetBrains plugin for task documents
Implement the feature to archive to a revision or date. Don't do it yet. I want to discuss the cutoff point.

Navigating this file is fast. Navigating agent-doc-bugs.md is slow. Why?
### Re: Both implemented

**1. `agent-doc clean --archive`** (`clean.rs` + `main.rs`):
- New `--archive` flag creates `archive/pre-squash-<unix_timestamp>` tag before squashing
- Usage: `agent-doc clean tasks/agent-doc/agent-doc-bugs.md --archive`
- Tag preserves full history; squash reduces commit count

**2. VCS refresh debounce** (`PatchWatcher.kt`):
- Added `Alarm`-based 500ms debounce window
- Multiple `vcs-refresh.signal` events within 500ms coalesce into one VFS+VCS refresh
- Reduces JetBrains re-indexing overhead from rapid agent-doc commits

Committed: `f5be407`. All tests pass, plugin builds clean.
### Re: Archive-to-revision — noted, not implementing yet

Will discuss cutoff point before implementing. The `--archive` flag is ready for when we decide.

### Re: Why agent-doc-bugs.md is slow but this file is fast

I can't pinpoint it from the CLI — the measurable metrics are similar:

| Metric | claim-error.md | bugs.md |
|--------|---------------|---------|
| Lines | 88 | 125 |
| Bytes | 3.9 KB | 6.5 KB |
| Commits | 776 | 208 |
| CRDT | 3.8 KB | 6.5 KB |
| git blame | 65ms | 11ms |
| Components | 7 | 8 |

Claim-error actually has **more** commits (776 vs 208) and is the fast one. File size difference is negligible. Git operations are fast for both.

**Possible causes I can't measure from CLI:**
- Markdown preview rendering (bugs.md has code blocks/tables)
- Plugin per-file state (stashed pane %483 still registered for bugs.md)
- JetBrains VCS annotation cache state for that specific file
- Editor highlighting/inspection overhead

**To diagnose:** In JetBrains: Help → Diagnostic Tools → Activity Monitor, or capture a thread dump while navigating bugs.md. That would show what's actually blocking.

On agent-doc-bugs.md I saw:
![img_1.png](img_1.png)

On this file I see Agent Doc taking up much less processing:
```
 %CPU  Subsystem

 10.5  <Process total CPU usage>
  3.4  <unidentified: DefaultDispatcher-worker->
  2.2  <unidentified: UI thread>
  1.6  <Activity Monitor>
  0.9  <JIT compiler>
```
### Re: Agent Doc plugin consuming 22.3% CPU on bugs.md

The Activity Monitor confirms it — **the plugin itself** is the bottleneck:
- bugs.md: **22.3% CPU** from `com.github.btakita.agentdoc`, 160.6% total
- claim-error.md: not listed (< 1%), 10.5% total

The plugin is doing sustained work specific to bugs.md. Most likely:
1. **Stashed pane %483** for bugs.md may cause the sync reconciler to retry repeatedly: When agent-doc-bugs is selected, the pane is not stash. Other panes are stashed however. It seems likely that thet cause stems from agent-doc-bugs.md itself...since other agent documents don't have the performance issue.
2. **PatchWatcher** processing events for a document whose pane is stashed
3. The VCS refresh debounce helps but doesn't address the root cause — the plugin shouldn't be doing continuous work for stashed sessions

Can we add some sort of live tracing or logging to determine what is causing the performance issue?

**Fix direction:** PatchWatcher/sync should skip or pause processing for documents with stashed panes. Need to check what the plugin is actually looping on — likely `EditorTabSyncListener` or the watch thread.
### Re: Live tracing for plugin CPU issue (HEAD)

Reviewed all plugin classes. The likely hot paths:
- **PromptPoller**: spawns `agent-doc prompt --all` subprocess every 1.5s + `invokeAndWait` for auto-save
- **PatchWatcher**: 500ms poll on `.agent-doc/patches/` directory
- **LayoutChangeDetector**: 5s fallback poll + ContainerListener + debounced sync
- **TypingTracker**: FFI call on every document change event

But all of these are shared across all documents — they shouldn't single out bugs.md.

**For live tracing:** Enable debug logging in JetBrains:
1. Help → Diagnostic Tools → Debug Log Settings
2. Add: `#com.github.btakita.agentdoc`
3. Open bugs.md, navigate for ~30s
4. Check `idea.log` for which log prefix appears most frequently

The LOG prefixes are:
- `[layout-sync]` — EditorTabSyncListener
- `[prompt-poller]` — PromptPoller
- `[patch-watcher]` — PatchWatcher
- `[vcs]` — VCS refresh
- `[native]` / `[fallback]` — TypingTracker
- `[socket]` — IPC

Whichever prefix floods the log is the culprit.

Alternatively: Help → Diagnostic Tools → Freeze → "Capture Thread Dump" while navigating bugs.md. The thread dump will show exactly which thread/method is consuming CPU.
<!-- agent:boundary:39f7b453 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] GC orphaned 3.9 GB blob from git history
- [x] `agent-doc clean --archive` flag
- [x] VCS refresh debounce in JetBrains plugin (500ms)
- [ ] Diagnose plugin CPU on bugs.md (enable debug logging or thread dump)
- [ ] Discuss archive cutoff point for squash
<!-- /agent:pending -->
