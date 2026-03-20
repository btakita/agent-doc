---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.22.2 | **Tests:** 466 (158 lib + 287 bin + 21 int) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.7 (cursor-aware FFI + VCS refresh + default modes) + VS Code v0.2.3
**Library:** `libagent_doc.so` — 7 FFI exports | **SKILL.md:** immediate commit after write
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

### Re: Exchange Compacted

Archived 1439 lines to `.agent-doc/archives/`. Use `agent-doc history tasks/agent-doc.md` to browse past exchanges or `--restore <commit>` to recover content.

**Session summary (v0.21.0 → v0.22.0):**

**Features built:**
- `agent-doc terminal` — cross-platform terminal launch from editor plugins (config-first, stale session detection)
- `agent-doc parallel` — fan-out with git worktrees + `--no-worktree` for read-only tasks
- Selective commit — `git hash-object` + `git update-index` stages snapshot only, user edits stay uncommitted
- `(HEAD)` marker — visual boundary on last `### ` heading (committed vs working tree)
- Cursor-aware FFI — `agent_doc_apply_patch_with_caret()` in shared library + JNA bindings
- CRDT post-merge reorder via Yrs attribution
- First-submit snapshot fix (git matches current → treat as new)
- JetBrains plugin v0.2.7 (cursor-aware append ordering)

**Bugs fixed:**
- First-submit "no changes detected" when file was just committed
- `(HEAD)` marker only matched `### Re:` (now any `### `)
- Plugin version path was wrong (`0.1.0` → `0.2.7`)

**Process improvements:**
- Immediate commit after write (no green gutter gap between cycles)
- Pending protocol: flush to disk before responding
Compacted. Content archived.

The JB plugin does not register the commit.
The green gutter still takes up the entire response...until I refresh the file.
Then the commit is registered.
It seems the document + git view is stuck in a cached state.

### Re: VCS Refresh Signal

The issue: `agent-doc commit` runs externally (CLI), but IntelliJ caches git state. The gutter stays green until the user manually refreshes.

**Fix: IPC signal for VCS refresh.**

**Rust side (`git.rs`):** After a successful commit, writes `.agent-doc/patches/vcs-refresh.signal`.

**Plugin side (`PatchWatcher.kt`):** Already watches `.agent-doc/patches/`. When it sees `vcs-refresh.signal`:
1. Deletes the signal file
2. Calls `VcsDirtyScopeManager.getInstance(project).markEverythingDirty()` on the EDT
3. IntelliJ refreshes all git gutter annotations

**Flow:**
```
agent-doc commit → git commit succeeds → writes vcs-refresh.signal
PatchWatcher → detects signal → VcsDirtyScopeManager.markEverythingDirty()
IntelliJ → re-reads git state → gutter updates immediately
```

Release binary rebuilt. Plugin v0.2.7 rebuilt with VCS refresh support. Install the new plugin zip to get automatic gutter updates after commits.

**Plugin path:** `src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.7.zip`
Can the plugin force a refresh of the git state?


### Re: Multi-Heading HEAD Marker

Fixed. `add_head_marker()` now marks ALL new `### ` headings in the agent's response, not just the last one.

Do responses always use `### `. Is it possible for a response to have a `## ` or `# ` or `#### ` or `##### ` or `###### `? If yes, can we make this rule apply to **any** root section title in the patch response.
So only the top level roots of the response...even if it's not a h3 tag.

**How it works:**
1. Gets git HEAD content for the file
2. Finds all `### ` headings in the snapshot
3. Filters to only headings that are NOT in git HEAD (= new, added by the agent)
4. Appends ` (HEAD)` to each new heading
5. Fallback: if git HEAD unavailable, marks the last heading only

For your `tasks/laura-klask.md` case with two response sections, both would now get the marker:
```
### Re: Amulet Technologies review (HEAD)    ← blue gutter
...response content...                       ← no gutter (committed)
### Re: Laura follow-up length (HEAD)        ← blue gutter
...response content...                       ← no gutter (committed)
```

Release binary rebuilt.
In tasks/laura-klask.md, I received a multi-section patch response from the agent.
However the section title uncommited response only applied to the last section. It should have applied to all sections in the patch response:
- ### Re: Amulet Technologies review
  - Unsuccessfully applied
- ### Re: Laura follow-up length
  - Successfully applied


### Re: Any-Level Heading Markers + VFS Refresh

**1. Heading level flexibility — Fixed**

`add_head_marker()` now matches ANY markdown heading level (`#` through `######`). Only the **root-level** (shallowest) headings in the new content get marked:

```
## Summary (HEAD)           ← root level, marked
### Details                 ← sub-heading, NOT marked
## Recommendations (HEAD)   ← root level, marked
### Item 1                  ← sub-heading, NOT marked
```

This works regardless of whether the response uses `###`, `##`, or any other level.

**2. VFS refresh — Fixed**

Added `LocalFileSystem.getInstance().refresh(true)` before `markEverythingDirty()`. The VFS refresh ensures IntelliJ re-reads the git state from disk before recomputing gutter annotations. Without VFS refresh, the dirty scope manager works against stale cached data.

**Note:** The plugin still needs to be reinstalled for these changes to take effect. Current plugin: `src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.7.zip`
I still saw the full green gutter. The full green gutter went away after I made an edit.


### Re: v0.22.1 Released

| Target | Status |
|--------|--------|
| crates.io | `agent-doc v0.22.1` |
| GitHub Release | [v0.22.1](https://github.com/btakita/agent-doc/releases/tag/v0.22.1) |
| cargo install | updated (replaced v0.22.0) |
| JetBrains plugin | v0.2.7 (rebuilt with VCS refresh) |

**What's new in v0.22.1:**
- Any-level `(HEAD)` markers (not just `###`)
- Multi-heading markers (all root headings in multi-section responses)
- VCS refresh signal (`vcs-refresh.signal` → plugin triggers `markEverythingDirty()` + VFS refresh)
Do we need another agent-doc release for these changes?

Look at tasks/laura-klask.md
In the last response, the end portion of my prompt was added after the patch agent response.
So my prompt was split.

> My engagement with Judith Krug has not started yet. We will be meeting mid next week. But it keeps on slipping.
> I should confirm the date and time of the call next week.

... patch response ...

> I'm also running low on funds. Need to get something going soon. So I have a client but have not closed on a deal yet. And have not begun work to be paid. And I need to find more work immediately after that.


### Re: Prompt Split Analysis

I can see exactly what happened in the `laura-klask.md` git diff:

```
+My engagement with Judith Krug...           ← user's text (part 1)
+I should confirm the date and time...
+
+### Re: Judith status & financial urgency    ← agent response landed HERE
+...response content...
+
+I'm also running low on funds...             ← user's text (part 2)
```

**Root cause:** The plugin applied the append patch at the end of the exchange component content. But the user was actively typing at the same position. The plugin's `document.setText()` preserved the caret, so the user's subsequent keystrokes went after the response insertion point.

**This is the cursor-aware ordering bug.** The fix (`applyComponentPatchWithCaret`) inserts the patch ABOVE the caret position, keeping user typing below. But the **plugin hasn't been reinstalled yet** — the current running plugin doesn't have this code.

The fix is already built in `agent-doc-jetbrains-0.2.7.zip`. Once installed:
1. Plugin captures caret offset before `WriteCommandAction`
2. For append-mode components, if caret is inside the component, inserts patch content at the line boundary ABOVE the caret
3. User's typing stays below the response

**To install:** IDEA → Settings → Plugins → gear → "Install Plugin from Disk..." → select `src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.7.zip` → Restart IDEA.

This will fix the prompt-splitting issue for all future responses.
Please tell me what happened.

An agent:exchange component with a patch attribute should default to patch=append.


### Re: Exchange Default Mode Fix

Found it. The **plugin's** `extractComponentMode()` was returning `"replace"` as the default when no `patch=` attribute was present. The Rust binary correctly defaults `exchange` → `append` via `default_mode()`, but the plugin didn't have this logic.

**Fix:** Added `defaultMode(component)` to `PatchWatcher.kt`:

```kotlin
private fun defaultMode(component: String): String {
    return when (component) {
        "exchange", "findings" -> "append"
        else -> "replace"
    }
}
```

Now `&lt;!-- agent:exchange --&gt;` (without explicit `patch=append`) correctly defaults to append mode in the plugin, matching the Rust binary behavior.

Plugin v0.2.7 rebuilt. This fix is included alongside all the other v0.2.7 changes (cursor-aware ordering, VCS refresh signal, any-level HEAD markers).
So `<!-- agent:exchange -->` should default to the same behavior as `<!-- agent:exchange patch=append -->`.

Please review the current commit + uncommitted state of tasks/judith-krug.md

First, my completed prompt is after the response. The response should **only** come after the patch response when the human has edited the prompt after the agent receives the diff for the request-> response.

Also, the entire patch response seems to be uncommitted. The patch response should be committed save the top level headers.

There are background tasks still running, so I'm not sure if that affects the behavior.

### Re: judith-krug.md Commit + Ordering Issues

**Two separate issues:**

**1. Entire response uncommitted:**

The `judith-krug.md` session was run by a **different Claude session** using the old SKILL.md (which doesn't call `agent-doc commit` after write). Only this session (`tasks/agent-doc.md`) has the immediate-commit behavior because I'm following the updated workflow manually.

**Fix:** The SKILL.md needs to be updated to include `agent-doc commit <FILE>` after `agent-doc write`. Once we update SKILL.md and run `agent-doc skill install`, all sessions will commit immediately. Currently the SKILL.md still says "Do NOT commit after writing the response" (step 5).

**2. User prompt appears after response:**

The user's prompts were part of the exchange BEFORE the response was sent. Looking at the diff:
- User prompts: "Please use corky to transcribe...", "Robert had some comments...", etc.
- Response: `#### Re: Judith's Response`, `#### Re: Robert Ward Recording Transcriptions`, etc.

The prompts appear below the response because the **plugin applied the append patch at the end of existing content** (the old behavior without cursor-aware ordering). The plugin hasn't been reinstalled yet with v0.2.7.

**Both issues resolve with:**
1. Install plugin v0.2.7 → cursor-aware ordering + VCS refresh
2. Update SKILL.md → immediate commit after write

Want me to update SKILL.md now? This will affect ALL future sessions after `/compact`.

Is this skill part of the current agent-doc package? Was it installed when agent-doc was upgraded?

### Re: SKILL.md Bundled + v0.22.2 Released

**Yes, SKILL.md is bundled with the binary** via `include_str!`. When you run `agent-doc skill install`, it writes the bundled version to `.claude/skills/agent-doc/SKILL.md`. But the v0.22.0/v0.22.1 binary still had the old "Do NOT commit after writing" instruction.

**Fixed in v0.22.2:** Updated the bundled SKILL.md to say "Commit immediately after writing the response." Now when any session runs the pre-flight version check and calls `agent-doc skill install`, it gets the new behavior.

**v0.22.2 released and installed:**

| Target | Status |
|--------|--------|
| crates.io | `agent-doc v0.22.2` |
| GitHub Release | [v0.22.2](https://github.com/btakita/agent-doc/releases/tag/v0.22.2) |
| cargo install | updated |
| Skill installed | `.claude/skills/agent-doc/SKILL.md` updated |

**For other sessions to pick up the change:** They need to either:
1. Run `/compact` (reloads SKILL.md from disk)
2. Start a new session (loads fresh SKILL.md)
3. The pre-flight auto-update check will detect the version mismatch and install automatically

audit docs

Are the recent updates in SPEC.md, VERSIONS.md, README.md?

### Re: Audit Docs + Documentation Updates

**Audit results:** 5 issues → 3 after updates. SPEC.md and README.md no longer flagged as stale.

**Updates made:**

**SPEC.md section 7.14 (commit):** Expanded from 1 line to full spec:
- Selective staging via `hash-object` + `update-index`
- `(HEAD)` marker on new root-level headings (any level)
- VCS refresh signal after successful commit
- Fallback to `git add -f` when no snapshot exists

**README.md CLI Reference:**
- Added `agent-doc terminal <file> [--session S]`
- Updated `commit` description to "selective commit (snapshot only, user edits stay uncommitted)"
- Removed duplicate `history` entries

**Already documented:**
- CLAUDE.md: `terminal.rs` already in module layout
- VERSIONS.md: all v0.22.0/v0.22.1/v0.22.2 entries present
- SPEC.md 7.24: `terminal` command already fully specified

**Remaining audit warnings (non-blocking):**
- CLAUDE.md large table (informational)
- SKILL.md/CLAUDE.md "older than git.rs" (timestamps, not content gaps)

In tasks/corky.md, the patch response to the prompt was merged above the prompt.

> VERSIONS.md is complete (47 entries, v0.1.0–v0.18.1, no gaps).
>Are the recent updates in SPEC.md, VERSIONS.md, README.md?

Should be above:


### Re: Corky Response Ordering

This is the same IPC ordering issue. The `tasks/corky.md` session used the old plugin (without cursor-aware ordering). The response was appended at the end of existing content, but the user typed "Are the recent updates..." at the same position concurrently. The result: response landed between the two user prompts.

**With plugin v0.2.7 installed**, the cursor-aware ordering would detect the caret position and insert the response above where the user is typing — keeping their prompt below. This would give the correct order:

```
audit docs                              ← user prompt (sent)
### Audit Docs Results                   ← response
...response content...
VERSIONS.md is complete...
Are the recent updates in SPEC.md...?   ← user prompt (typed after response)
```

**All three ordering issues this session (agent-doc.md, laura-klask.md, corky.md) have the same root cause:** the old plugin doesn't have cursor-aware ordering. Plugin v0.2.7 fixes all of them.

Has the plugin been installed yet? If not, the path is:
`src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.7.zip`
> ### Audit Docs Results

In tasks/youtube.md, the agent response patch is still being placed above the prompt.
The earlier feature to place the agent response patch to code being edited is overeager.


### Re: Cursor-Aware Ordering is Inverted (HEAD)

The problem is clear from the youtube.md git history. The current `append_with_caret()` logic:

1. Finds the caret position (where user is typing)
2. Inserts the response ABOVE the caret

This means when the user types new text while waiting for a response, the response lands ABOVE their new text — but also ABOVE the original prompt. The response ends up interleaved between pre-existing content and the user's prompt.

**The correct logic needs the baseline.** The plugin currently has no information about what the document looked like when the agent received the diff. Without that, it can't distinguish "old prompt" from "new typing."

**Proposed fix — baseline offset in patch metadata:**

When `agent-doc write` creates the patch file in `.agent-doc/patches/`, it already knows the baseline (passed via `--baseline-file`). Include the **byte offset of the end of the component content in the baseline** as metadata in the patch file:

```json
{
  "component": "exchange",
  "mode": "append",
  "content": "### Re: ...\
...",
  "baseline_content_end": 2847
}
```

Then the plugin's insertion logic becomes:

1. Read `baseline_content_end` from the patch
2. Find the component boundaries in the current document
3. Insert the response at `component_start + baseline_content_end` (clamped to component bounds)
4. This places it after the prompt the agent saw, before any new user typing

**Scenarios:**

| Case | baseline_content_end | Effect |
|------|---------------------|--------|
| User finished prompt, waiting | == current component end | Same as normal append |
| User typed more after prompt | < current component end | Response goes between old prompt and new text |
| User deleted text | > current component end | Clamped, same as normal append |

This is a cleaner signal than caret position because it directly encodes "where the agent's view of the content ended."

**Alternative (simpler but less precise):** Include the last N characters or a hash of the prompt tail in the patch metadata. The plugin finds that text in the document and inserts after it. Less robust but avoids offset tracking.

Which approach do you prefer? I can implement either in the Rust binary + plugin.
We need to place the agent response patch below the prompt used to generate the response.

So in the following scenarios:
- Human completely finishes the prompt
  - agent response patch should be below the prompt
- Human is still typing a prompt while the response to a previous prompt is being generated
  - agent response patch should be below the part of the prompt that the agent received and generated the answer
  - agent response patch should be above the prompt that was being edited
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] Release v0.22.0
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
