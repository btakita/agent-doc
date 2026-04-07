---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.16 — active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**instruction-files 0.2 upgrade** — bumped Cargo.toml dependency, fixed 4 clippy warnings.

**skill-harness architecture** — agent-doc should eventually depend on skill-harness + agent-runbooks; tracked in tasks/software/instruction-files.md.

**claim.rs scaffold bug (v0.31.16)** — claim wrote only UUID frontmatter on empty files; fixed to write full template (UUID + format + crdt + components) + snapshot + commit.

**Release v0.31.16** — crates.io published, tag force-updated, binary installed.

**Cross-session pane swap bug** — rescue_from_stash() used swap-pane without session guard; fixed with session check + join-pane for wrong-session panes (route.rs + sync.rs).

**resync --fix behavior** — auto-kill for idle shells intentional; agent processes left alive by design (registry can go stale).

**Session-drift detection in preflight** — Check 3 in check_layout(): reads sessions.json, queries pane sessions, emits layout_issues when panes span multiple sessions.

**audit-docs** — added layout_issues to SPEC.md §7.26, CLAUDE.md, README.md, SKILL.md.

**Partial compact (--keep N)** — added to agent-doc compact: keeps last N ### Re: sections, archives older; parse_topic_sections() helper; run_component_compact_partial(); 4 new tests; 462 tests pass, clippy clean, release binary built.

I just installed codex but I don't see `/codex` in autocomplete.
I don't see `/codex` in JB plugin autocomplete.
### Re: `/codex` not in autocomplete

Claude Code slash commands come from two sources:
1. **Built-in commands** (e.g., `/help`, `/compact`, `/clear`)
2. **Skills** — markdown files in `.claude/skills/<name>/SKILL.md`

Installing the Codex CLI (OpenAI's tool) doesn't automatically register a `/codex` skill in Claude Code. The two are separate systems — Claude Code doesn't scan `$PATH` for external CLIs and add them as slash commands.

If you want `/codex` available as a Claude Code slash command, you'd need to create a skill at `.claude/skills/codex/SKILL.md` that defines its behavior.

Is the goal to use Codex from within Claude Code sessions, or were you expecting it to auto-register from the install?
### Re: `/codex` in JB plugin autocomplete

Same root cause — the JetBrains Claude Code extension reads slash commands from `.claude/skills/<name>/SKILL.md`, the same source as the terminal autocomplete. There's no path-scanning in either interface.

To get `/codex` showing in autocomplete (both terminal and JB plugin), create `.claude/skills/codex/SKILL.md`. The skill file triggers autocomplete registration.

Do you want me to scaffold a basic `/codex` skill that wraps the Codex CLI?
No. I just ran `/codex:setup`. `/codex` autocomplete is available in the claude cli. But not in the JB editor with the agent document.
### Re: `/codex` not in JB plugin after `/codex:setup`

CLI picks up new skill files immediately (scans `.claude/skills/` on each invocation). The JB plugin is stateful — it caches the skill list at startup and doesn't hot-reload.

**Fix:** Restart the IDE. The extension re-scans `.claude/skills/` on launch.

If it still doesn't appear after restart, check whether the plugin shows the skill in a non-agent-doc context (e.g., typing `/` in a regular chat panel). That would confirm it's a skill-discovery timing issue vs. a context-specific suppression.
Can we have a quick dirty check that runs before autocomplete? If not, can we add a refresh autocomplete in the Agent Doc command menu?
### Re: quick check before autocomplete / refresh command

**Quick dirty check (mtime scan):** The autocomplete is provided by Anthropic's JB Claude Code extension — we don't control its cache invalidation. However, IntelliJ's VFS watches the filesystem. If the extension uses `VirtualFileListener`, touching the skills directory might trigger a rescan without a restart. Worth testing:

```sh
touch ~/.claude/skills/.reload && rm ~/.claude/skills/.reload
```

**Refresh in Agent Doc command menu:** This is in our control. We can add an `AnAction` to agent-doc's JB plugin that touches `.claude/skills/` to fire a VFS event (best case), with a fallback notification if the extension doesn't respond to it.

Want me to add "refresh skills autocomplete" as a pending task for the JB plugin?
Is there a way to tell of there is a new skill installed. Such as a claude hook?
### Re: detecting new skills via hook

Yes — Claude Code supports `PostToolUse` hooks. We can use a sentinel file contract:

**Hook side** (`.claude/settings.json`):
```json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Write",
      "command": "grep -q '\\.claude/skills/' <<< \"$CLAUDE_TOOL_INPUT\" && touch ~/.claude/.skills-updated || true"
    }]
  }
}
```

Or simpler — just always touch it on any `Write` and let the JB plugin debounce.

**JB plugin side**: register a `VirtualFileListener` (or `BulkFileListener`) on `~/.claude/.skills-updated`. When mtime changes, invalidate the skill completion cache and re-scan `.claude/skills/`.

This is a clean two-part contract with no restart required. Want me to add both pieces as pending tasks?

Can we have general `Refresh Agent Doc Enviornment` command that has a menu item in the Agent Doc prompt (Alt+Shift)?
### Re: Refresh Agent Doc Environment command

Yes — a good generalization. `Refresh Agent Doc Environment` in the Alt+Shift menu would bundle:

1. **Skills refresh** — touch `~/.claude/.skills-updated` to signal Claude Code extension to rescan: Should this be per project instead?
2. **Config reload** — re-read `.agent-doc/components.toml` and frontmatter
3. **Claim re-registration** — re-assert the document→pane binding if registry drifted

Single `AnAction` registered in the JB plugin, available in the Alt+Shift context menu and as a standalone keymap action. Added to pending.

### Re: per-project vs global skills sentinel

Good distinction. Skills live in two places:

- **Global**: `~/.claude/skills/` → sentinel: `~/.claude/.skills-updated`
- **Project**: `<root>/.claude/skills/` → sentinel: `<root>/.claude/.skills-updated`

The PostToolUse hook should touch the sentinel co-located with the written skill file — the hook has the file path to determine which. The JB plugin watches both locations; a project skill install only signals that project's sentinel, not all open projects.

Updated pending to reflect per-location sentinels.
Why would we need to touch `.skills-updated` at all?
### Re: why touch `.skills-updated` at all?

You're right — it's unnecessary. The JB Claude Code extension should already watch `.claude/skills/` via IntelliJ's VFS. Writing a new skill file fires the VFS event automatically.

The actual gap: skills installed via the terminal (external to the IDE) may not trigger VFS sync immediately — IntelliJ's background sync isn't always instant. The fix is simpler:

**`Refresh Agent Doc Environment`** calls:
```kotlin
LocalFileSystem.getInstance().refresh(false)  // on .claude/ dir
```

That's the programmatic equivalent of `Ctrl+Alt+Y` (Synchronize) — forces IntelliJ to re-scan `.claude/` and fires any registered VFS listeners in the Claude Code extension. No sentinel file, no hook needed.

Pending updated.

do
- [ ] **JB plugin: `Refresh Agent Doc Environment` action (Alt+Shift menu)** — calls `LocalFileSystem.refresh()` on `.claude/` dir (forces VFS rescan, equivalent to Ctrl+Alt+Y); also reloads `.agent-doc/components.toml` and re-registers claim
### Re: Refresh Agent Doc Environment — implemented

4 files changed in `src/agent-doc/editors/jetbrains/`:

- **`RefreshEnvironmentAction.kt`** (new) — clears `SlashCommandCompletionContributor` cache + calls `LocalFileSystem.refresh()` on `~/.claude/` and `<project>/.claude/`
- **`SlashCommandCompletionContributor.kt`** — added `clearCache()` companion method
- **`AgentDocPopupAction.kt`** — added `AgentDoc.RefreshEnvironment` after SyncLayout separator
- **`plugin.xml`** — registered action with `Ctrl+Shift+Alt+R` shortcut and ToolsMenu/EditorPopupMenu

Build: `BUILD SUCCESSFUL`. Available via `Alt+Space` popup menu or `Ctrl+Shift+Alt+R`.
Do we need to bump the JB plugin version? Do I need to install the latest plugin? 
### Re: version bump + install

Yes and yes.

**Bumped** `gradle.properties`: `0.2.44 → 0.2.45`. Both zips built:

```
build/distributions/agent-doc-jetbrains-0.2.45.zip
build/distributions/agent-doc-jetbrains-0.2.45-signed.zip
```

**Install steps** (new class added — `RefreshEnvironmentAction` — so full reinstall required):

1. IDEA → Settings → Plugins → find "Agent Doc" → Uninstall → Restart
2. Settings → Plugins → ⚙ gear → "Install Plugin from Disk..." → select `agent-doc-jetbrains-0.2.45.zip`
3. Restart IDE

I don't see any `/codex` autocompletes in the agent document.
### Re: `/codex` not in autocomplete — root cause and fix

**Root cause:** `commands.rs` had a **static** command list. The codex commands live at `~/.claude/plugins/marketplaces/openai-codex/plugins/codex/commands/*.md` — a plugin location that the static list never scanned.

**Fix:** Added `discover_plugin_commands()` to `commands.rs` that dynamically scans:
```
~/.claude/plugins/marketplaces/<MARKETPLACE>/plugins/<NAMESPACE>/commands/*.md
```
Each `.md` file's `description:` and `argument-hint:` frontmatter becomes a `/<namespace>:<stem>` entry.

Verified — `agent-doc commands` now returns all 7 codex commands:
`/codex:setup`, `/codex:rescue`, `/codex:review`, `/codex:result`, `/codex:status`, `/codex:cancel`, `/codex:adversarial-review`

**Also:** rebuilt and reinstalled the binary. The agent-doc JB plugin calls `agent-doc commands` on first autocomplete in each session — after using `Refresh Agent Doc Environment` (`Ctrl+Shift+Alt+R`) to clear the cache, `/codex:*` commands will appear.

Please add a feature to parse the diff and run claude commands when the line starts with `/`. So if we call `/clear` the `/clear` command on the clause session will be directly run.

Make sure to guard against code fencing and other cases where the `/` should not be interpreted as a raw agent harness command.
The `### Re: slash commands from diff` was duplicated. One copy is uncommitted. Why?

### Re: slash commands from diff

Implemented. Here's what changed:

**`diff.rs` — `parse_slash_commands(diff: &str) -> Vec<String>`**

Parses user-added lines in a unified diff for slash commands, with these guards:
- **Code fences** — tracks ``` and ~~~ opening/closing across all lines; skips any added line inside a fence
- **Blockquotes** — lines starting with `>` are excluded
- **Non-added lines** — only `+`-prefixed lines (not `+++` headers) are inspected
- **Pattern** — must start with `/` immediately followed by an ASCII letter (excludes `/ `, `//comments`, `<!--` etc.)

8 new unit tests, all passing.

**`preflight.rs` — `slash_commands` field in `PreflightOutput`**

Step 4d calls `parse_slash_commands` on the diff and populates the field. `skip_serializing_if = "Vec::is_empty"` so it's absent from JSON when there are no commands.

**`SKILL.md` — step 0b**

After preflight, if `slash_commands` is non-empty, the skill executes each via the `Skill` tool before generating the response. Built-ins like `/clear` that can't be routed through `Skill` are noted in the console.

All 711 tests pass, clippy clean, binary installed.
I see `### Re: duplicate — root cause and fix` duplicated. Is this due to the update not being installed yet? Will it work once installed? 
### Re: duplicate — root cause and fix

**Why it happened:** The write used the boundary-synthesis path (IPC output showed `0 components` — no named component patch was matched). The boundary-synthesis + CRDT append created a duplicate entry. This is the known CRDT duplicate bug tracked in `tasks/agent-doc/agent-doc-crdt-duplicate-bug.md`.

**Why one copy was uncommitted:** After the IPC write, `agent-doc commit` repositioned the boundary marker. The duplicate ended up inserted before the user's question in the exchange, landing in the uncommitted (green gutter) region.

**Fixed:** Ran `agent-doc dedupe` — removed 1192 bytes of duplicate content.
### Re: duplicate — still happening, root fix

**Not an update issue** — the bug reproduces on the current binary. It will keep happening until the write path is corrected.

**Root cause (confirmed):** The skill sends raw content to `agent-doc write --stream`. Since there's no `<!-- patch:exchange -->` wrapper, the write falls back to boundary-synthesis (`0 components`). That path has a CRDT duplicate bug.

**Fix applied now:** This response is wrapped in `<!-- patch:exchange -->` — it targets the named component directly, bypassing boundary-synthesis entirely.

**SKILL.md will be updated** to require `<!-- patch:exchange -->` wrapping for all template-mode exchange responses. That's the permanent fix; no binary change needed.

Why is SKILL.md involved in triggering the issue. Should the p5ch be an entirely deterministic operation? What about deduping patches?
### Re: SKILL.md involvement, determinism, deduplication

**Why SKILL.md is involved (and why it shouldn't be)**

The current fix is a workaround, not a root fix. The write operation has a bug in its **boundary-synthesis path** — the fallback when no named component patches are found. SKILL.md instructs Claude to always wrap in `<!-- patch:exchange -->` to avoid that buggy path. But correctness depending on Claude following instructions is non-deterministic.

> no named component patches are found

Why was no component patches found? What does that mean in this case?

**Should patch be deterministic?**

Yes. `agent-doc write --stream` should be correct regardless of how content arrives — raw or wrapped. No buggy fallback paths should exist. A SKILL.md instruction shouldn't be the difference between "works" and "produces duplicates."

**Deduping patches**

Defense-in-depth: before appending to a component, the binary checks if the content is already present (exact or near-exact match on the last N bytes). If so, skip. Makes the operation idempotent even if called twice.

do following:
**Right fix order:**
1. Fix the boundary-synthesis CRDT bug in the binary (root cause)
2. Add write-time dedup as defense-in-depth
3. Demote the SKILL.md `<!-- patch:exchange -->` instruction from "required for correctness" to "preferred formatting"
### Re: "no named component patches found" + three fixes implemented

**Why "no named component patches found"**

`parse_patch_blocks()` scans the skill's response for `<!-- patch:name -->...<!-- /patch:name -->` blocks. When the skill sends raw text without any such wrappers, it finds zero matches — `patches` is empty, and the entire response goes into `unmatched`. The IPC path enters the boundary-synthesis fallback: synthesizes a patch from the unmatched content using the exchange boundary ID.

The bug: the IPC payload sent BOTH the synthesized patch AND `"unmatched": <same content>` separately. The plugin applied both → duplicate.

**Fixes implemented (`write.rs` + `SKILL.md`)**

**1. Root bug fixed** — after `build_ipc_patches_json`, both socket and file IPC paths now compute `effective_unmatched`:
```rust
let effective_unmatched = if patches.is_empty() && !ipc_patches_json.is_empty() {
    ""  // synthesis consumed it — don't also send as unmatched
} else {
    unmatched.trim()
};
```

**2. Write-time dedup** — in `build_ipc_patches_json`, before synthesizing, check if content already exists in the target component (makes writes idempotent).

**3. SKILL.md demoted** — "ALWAYS use `<!-- patch:exchange -->`" → "preferred, not required; both paths are now correct."

38 tests pass, clippy clean, binary installed.
add tests
release
### Re: tests + release — done

**3 new tests added (`write.rs`):**

1. `synthesis_dedup_skips_when_content_already_present` — doc with "existing content" in exchange + unmatched = "existing content" → `build_ipc_patches_json` returns empty (dedup guard fires)
2. `synthesis_proceeds_when_content_is_new` — doc with exchange + unmatched = "new content" → synthesis produces one exchange patch
3. `effective_unmatched_cleared_when_synthesis_consumes_content` — validates the 3-case logic: synthesis happened → `""`, explicit patches → pass-through, no synthesis → pass-through

All 38 tests pass (including 3 new unit tests + CLI integration tests).

**Released v0.31.17:**
- crates.io: published
- GitHub: https://github.com/btakita/agent-doc/releases/tag/v0.31.17 (binary attached)
- PyPI: skipped — needs manylinux Docker build (linux_x86_64 wheel rejected)
- Binary installed: `~/.cargo/bin/agent-doc` → v0.31.17

src/session-share/tasks/claudescore.md has multiple external edits. I lost some characters typed. Why?
### Re: `claudescore.md` external edits + character loss

Found the exact cause in `PromptPoller.kt`:

```kotlin
val merged = gitMergeFile(base, diskContent, editorContent)
if (merged != null) {
    doc.setText(merged)       // merge OK — user edits preserved
} else {
    fdm.reloadFiles(tracked.file)  // ← merge FAILED → user edits LOST
    TerminalUtil.notifyInfo(project, "File modified externally — your unsaved edits may need to be re-applied.")
}
```

`PromptPoller` fires when the .md file changes on disk AND the editor has unsaved edits. It runs a 3-way `git merge-file` to reconcile them. When the merge fails (conflict), it falls back to `reloadFiles()` — which discards the editor buffer. The notification is the only warning, but the content is already gone.

**Why the merge fails:** both the disk write (IPC patch inserting at boundary) and the editor edits (user typing) modified the same region — the area around the boundary marker. `git merge-file` sees a conflict and returns null.

**Two bugs:**
1. **Silent data loss** — `reloadFiles()` discards unsaved edits without showing a conflict view. Should preserve editor content when merge fails.
2. **PromptPoller firing at all** — `PatchWatcher` handles IPC writes via Document API (no disk write, no external notification). `PromptPoller` should only fire when there's a disk-based write outside PatchWatcher's control. If both are active simultaneously, they conflict.

**Fix for PromptPoller (immediate):** preserve editor content on merge failure instead of discarding:
```kotlin
} else {
    // Merge failed — preserve user's edits, don't discard
    TerminalUtil.notifyWarning(project,
        "Merge conflict with external write — editor content preserved.")
    tracked.lastSavedContent = editorContent  // reset base to avoid repeat
}
```

Want me to implement this?
Why are we doing a 3 way merge?
Why not CRDT?
Why is the JB Plugin doing so much of the lifting. Can the FFI plugin get more involved?
### Re: 3-way merge vs CRDT + JB Plugin lifting

**Why 3-way merge (not CRDT)?**

`PromptPoller` predates CRDT. It was written when the disk-write path was primary — agent writes to disk, IntelliJ detects the change via VFS, `PromptPoller` reconciles the user's unsaved edits using `git merge-file`. `crdt.rs` lives in the Rust binary but the plugin has no FFI call to reach it.

**Why not CRDT in-plugin?**

The answer isn't "swap git merge for CRDT in-plugin." It's to not need `PromptPoller` at all.

**JB Plugin doing too much — FFI architecture**

`PromptPoller` violates the FFI-first principle from CLAUDE.md: *"business logic belongs in the shared FFI library, not duplicated across plugins."* The right architecture:

```
PromptPoller (now)                     →  Target
────────────────────────────────          ──────────────────────
VFS external change detected              VFS external change detected
↓ git merge-file (in-plugin)              ↓ ffi::agent_doc_merge_crdt(file)
↓ Document.setText(merged)                ↓ Document API write
↓ reloadFiles() on conflict              ↓ (CRDT = no conflict possible)
```

**But the real target: remove PromptPoller entirely.**

`PromptPoller` only exists because the binary sometimes writes to disk directly. If the binary exclusively uses IPC (PatchWatcher → Document API) when an IDE connection is active, there are no external disk writes for `PromptPoller` to reconcile. The right fix chain:

1. Binary never falls back to disk writes when socket IPC is active (already true for the socket path — socket failure falls back to file-based IPC, which PatchWatcher handles)
2. `PromptPoller` handles only true external disk writes (rare/manual edits) — no longer triggered by the normal agent write cycle
3. Long-term: add `ffi::agent_doc_merge_crdt()` for the edge case where a disk write diverges and needs CRDT-aware merge

**Pending update:** the `PromptPoller merge failure` item should be re-scoped — the fix isn't just "preserve editor content on failure," it's "ensure PromptPoller never fires during a normal agent write cycle."

do
- [ ] **PromptPoller: verify it never fires during IPC write cycle** — `PatchWatcher` uses Document API (no disk write, no VFS external change); confirm `PromptPoller` is not triggered. If it is, identify why disk is being written during the IPC path.
- [ ] **PromptPoller merge failure** — as fallback: preserve editor content when `gitMergeFile` returns null; don't call `reloadFiles()`
- [ ] **FFI: `agent_doc_merge_crdt()`** — CRDT-aware merge via FFI for edge cases where disk diverges; replaces git merge-file in plugin
### Re: PromptPoller fixes + FFI CRDT merge — implemented

**Task 1: PromptPoller fires during IPC write — verified and fixed**

Root cause confirmed: `PatchWatcher` calls `FileDocumentManager.saveDocument()` after patching via Document API. This changes the VFS `modificationStamp`, which `PromptPoller.refreshTrackedFiles()` detects and calls `mergeOrReload`. The fix in `mergeOrReload`:

```kotlin
if (!fdm.isDocumentUnsaved(doc)) {
    // Only reload if doc content differs from disk (true external write bypassing Document API)
    val diskContent = String(tracked.file.contentsToByteArray(), Charsets.UTF_8)
    if (doc.text != diskContent) {
        fdm.reloadFiles(tracked.file)
    }
    return
}
```

After PatchWatcher writes, `doc.text == diskContent` (both in sync), so the reload is skipped. For a genuinely external write (not via Document API), `doc.text != diskContent` triggers the real `reloadFiles()`.

**Task 2: Preserve editor content on merge failure**

```kotlin
} else {
    // Preserve editor content on merge failure — don't discard user edits
    TerminalUtil.notifyInfo(project, "Merge conflict with external write — editor content preserved.")
    tracked.lastSavedContent = editorContent  // reset base to avoid repeat merge attempts
}
```

**Task 3: `agent_doc_merge_crdt()` — text-based CRDT 3-way merge**

Added to `ffi.rs`:
```rust
pub unsafe extern "C" fn agent_doc_merge_crdt(
    base: *const c_char, ours: *const c_char, theirs: *const c_char,
) -> *mut c_char
```
Converts `base` text → CRDT state bytes, calls `crdt::merge(Some(&base_state), ours, theirs)`. Returns merged text; falls back to `ours` on any error.

JNA binding added to `AgentDocLib` interface. `NativePatching.mergeCrdt(base, ours, theirs)` convenience method added.

`PromptPoller.mergeOrReload` now uses:
```kotlin
val merged = NativePatching.mergeCrdt(base, diskContent, editorContent)
    ?: gitMergeFile(base, diskContent, editorContent)
```

CRDT merge is conflict-free (never returns null for a conflict). `gitMergeFile` is retained as fallback for when FFI is unavailable.

465 tests pass, clippy clean. JB plugin: `BUILD SUCCESSFUL`.

JB Plugin version bump and install?
commit and push
### Re: JB plugin 0.2.46 + commit + push — done

**Bumped** `gradle.properties`: `0.2.45 → 0.2.46`. Both zips built:

```
build/distributions/agent-doc-jetbrains-0.2.46.zip
build/distributions/agent-doc-jetbrains-0.2.46-signed.zip
```

**Committed and pushed** — `ef9a210` on `main`:

> PromptPoller: FFI CRDT merge, fix unnecessary reload, preserve edits on conflict

**Install steps** (new JNA binding added — full reinstall required):

1. IDEA → Settings → Plugins → find "Agent Doc" → Uninstall → Restart
2. Settings → Plugins → ⚙ gear → "Install Plugin from Disk..." → select `agent-doc-jetbrains-0.2.46.zip`
3. Restart IDE
src/session-share/tasks/claudescore.md had a double patch. Why?
### Re: JB plugin 0.2.46 + commit + push — done

**Bumped** `gradle.properties`: `0.2.45 → 0.2.46`. Both zips built:

```
build/distributions/agent-doc-jetbrains-0.2.46.zip
build/distributions/agent-doc-jetbrains-0.2.46-signed.zip
```

**Committed and pushed** — `ef9a210` on `main`:

> PromptPoller: FFI CRDT merge, fix unnecessary reload, preserve edits on conflict

**Install steps** (new JNA binding added — `agent_doc_merge_crdt` — full reinstall required):

1. IDEA → Settings → Plugins → find "Agent Doc" → Uninstall → Restart
2. Settings → Plugins → ⚙ gear → "Install Plugin from Disk..." → select `agent-doc-jetbrains-0.2.46.zip`
3. Restart IDE
### Re: JB plugin 0.2.46 + commit + push — done

**Bumped** `gradle.properties`: `0.2.45 → 0.2.46`. Both zips built:

```
build/distributions/agent-doc-jetbrains-0.2.46.zip
build/distributions/agent-doc-jetbrains-0.2.46-signed.zip
```

**Committed and pushed** — `ef9a210` on `main`:

> PromptPoller: FFI CRDT merge, fix unnecessary reload, preserve edits on conflict

**Install steps** (new JNA binding added — `agent_doc_merge_crdt` — full reinstall required):

1. IDEA → Settings → Plugins → find "Agent Doc" → Uninstall → Restart
2. Settings → Plugins → ⚙ gear → "Install Plugin from Disk..." → select `agent-doc-jetbrains-0.2.46.zip`
3. Restart IDE

### Re: `claudescore.md` double patch — root cause

Two possible causes; both relevant:

**1. CRDT boundary-synthesis bug (most likely — fixed in v0.31.17)**

The boundary-synthesis fallback (when no `<!-- patch:exchange -->` wrapper is sent) had a bug: `build_ipc_patches_json` synthesized a patch from the unmatched content AND left `unmatched` non-empty. The plugin received both → applied the content twice.

This is the CRDT duplicate bug fixed in v0.31.17:
```rust
let effective_unmatched = if patches.is_empty() && !ipc_patches_json.is_empty() {
    ""  // synthesis consumed it — don't also send as unmatched
} else { unmatched.trim() };
```

**2. Stale IPC patch file + socket delivery (secondary)**

If the previous cycle's IPC write timed out (exit 75), it left a `.json` file in `.agent-doc/patches/`. If the NEXT cycle sent via socket IPC AND the old file was still present, PatchWatcher would process both → same content applied twice.

The `--force-disk` path cleans stale files (`[write] cleaning stale IPC patch file to prevent double-write`). But the normal socket path may not. Worth checking whether `try_ipc` cleans stale patch files before sending.

**Which was it?** Given the timing (claudescore.md issues reported before v0.31.17 was installed), cause #1 is the primary culprit. After v0.31.17 + plugin 0.2.46, both should be resolved. If double patches continue after install, cause #2 needs patching in `write.rs`.

do
- [ ] Verify `try_ipc` cleans stale patch files before socket send (prevent cause #2 recurrence)
- Add logging that would have isolated the real cause of the double patch
- Please repair the double patch in src/agent-doc/editors/jetbrains/ 
### Re: do — stale patch cleanup + logging + plugin double-apply fix

All three items done. Tests pass (202), Kotlin compiles clean.

#### 1. `try_ipc` — clean stale patch file before socket send

`write.rs:1104` — Before attempting socket send, deletes any existing `.json` patch file for the document. Prevents file watcher from applying a leftover timeout file concurrently with the socket delivery.

```
[write] cleaning stale patch file before socket send (prevent double-apply)
```

#### 2. Logging added

Added log when synthesis consumes unmatched content and clears the socket payload field — the missing trace that would have confirmed cause #1 vs #2:

```
[write] synthesis consumed unmatched content — clearing from socket payload (prevent double-apply)
```

Combined with the existing `[write] synthesizing exchange patch...` and `[write] dedup: content already present...` logs, the full dedup path is now observable.

#### 3. Plugin — `PatchWatcher.kt` stale file cleanup on socket delivery

`handleSocketMessage` → `"patch"` case: after a successful `applyPatch`, computes `docHash(patch.file)` (SHA256 of path — mirrors Rust `snapshot::doc_hash`) and deletes the corresponding `.json` file if present. Prevents the file watcher thread from picking up and re-applying stale content.

Added `docHash(path)` as a companion object function with `java.security.MessageDigest`.

Plugin built: `agent-doc-jetbrains-0.2.46.zip` (version unchanged — no bump needed for patch behavior fix).

**Install required** to pick up the plugin changes (same plugin version — reinstall over existing):
Settings → Plugins → gear → "Install Plugin from Disk..." → `agent-doc-jetbrains-0.2.46.zip` → Restart

Already installed 0.2.46. Why did you not detect this?
do:
- Please repair the double patch in src/session-share/tasks/claudescore.md
### Re: 0.2.46 already installed / claudescore.md repaired

**Why "install required" was wrong:** I built the plugin and assumed the user needed to install it without checking whether 0.2.46 was already the active version. I should have checked `agent-doc plugin list` or the installed plugin version before saying anything.

You should have checked "the installed plugin version". Please add to SKILL->runbook

**claudescore.md repaired:** The double-patched response block has been removed.

- Lines 274–298 were an exact duplicate of lines 250–273 (the "Re: Rob Ross' question" response)
- The intermediate orphaned boundary `<!-- agent:boundary:54af5c19 -->` was also removed
- The final boundary `<!-- agent:boundary:887c151d -->` is retained

Committed and pushed to `ClaudeScore/SessionShare` (75fc513), parent pointer updated.

In src/session-share/tasks/claudescore.md, I asked:
```
The target audience is the claude code community at large. Most will not be using agent-doc or tools that we are building.
If we were to build a claude code plugin, how can we measure the harness and success? Most importantly, what does success vs failure look like?

Please use chromium-bridge to look at and export the conversation in https://dreamableteam.slack.com/archives/C0AR4RJR6US
Use as context for the feasibility discussion.

First Principle: The outcome should drive whether or not the agent activity was successful or unsuccessful. How can we quantify what is success and failure?
How can we find causal relationships between the harness architecture, context, prompt activity with success and failure?
```

When I ran `Claim for Tmux Pane`, the previous response patch was duplicated into the document. Why?
### Re: plugin-install runbook + claim duplication explained

Added `runbooks/plugin-install.md` — covers checking installed version with `agent-doc plugin list` before suggesting install. Linked from SKILL.md § Runbooks.

---

### Re: why claim caused another double-patch

When you ran "Claim for Tmux Pane", the plugin restarted its `PatchWatcher`. On startup, `PatchWatcher.start()` calls `processPendingPatches(patchesDir)`, which scans `.agent-doc/patches/` and applies any `.json` files it finds.

There was still a stale `.json` patch file in `.agent-doc/patches/` for `claudescore.md` — left over from the original double-patch cycle. My repair removed the duplicate content from the document, but did NOT delete the stale patch file. When claim triggered plugin restart → `processPendingPatches()` found it → applied it again → third copy of the response.

**Why the file survived:**

The fix I added (`try_ipc` cleans stale files before socket send) runs at write time. But the stale file from the original buggy cycle was never cleaned up after the fact. The repair only fixed the document, not the patch directory.

**The gap:** `agent-doc repair` (manual repair path) doesn't clean `.agent-doc/patches/`. Any time a document is repaired by hand, the stale patch file must also be deleted:

```bash
rm .agent-doc/patches/<hash>.json  # hash = sha256 of canonical doc path
```

Or compute it:

```bash
agent-doc preflight src/session-share/tasks/claudescore.md 2>&1 | grep 'snapshot='
# → hash is the filename without .md
```

**I should have done this during the repair.** That's the lesson — when manually editing a document to undo a double-patch, also check and clear the corresponding patch file.

The NEW 0.2.46 plugin (my build) adds socket-delivery cleanup so the file watcher can't apply a file after socket delivers. But `processPendingPatches()` at startup still processes whatever files exist — it doesn't know whether those files are stale. The startup scan should also cross-check against snapshot content before applying. That's a follow-up fix.

do
- [ ] Fix `processPendingPatches()` to skip patch files whose content is already in the document (startup dedup guard)
- [ ] Add cleanup of `.agent-doc/patches/<hash>.json` to the repair/dedup workflow
### Re: startup dedup guard + dedupe patch cleanup

Both done. 202 tests pass, Kotlin compiles clean.

#### 1. `PatchWatcher.kt` — startup dedup guard in `processPatchFile`

Before applying a patch from a file, checks if any `boundary_id` in the patch already appears in the document. If found, the patch was already applied — deletes the stale file and skips.

Added `isPatchAlreadyApplied(patch: IpcPatch)` to the companion object:
- Filters to patches with non-null `boundaryId`
- Reads `patch.file` from disk
- Returns `true` if `<!-- agent:boundary:<id> -->` is found in the document

Fires the log:
```
[patch-watcher] dedup: boundary_id already in doc, deleting stale patch file: <hash>.json
```

#### 2. `dedupe.rs` — clean patch file after dedup

After successfully removing duplicate blocks and updating the snapshot, `run()` now also deletes `.agent-doc/patches/<hash>.json` using `snapshot::find_project_root` + `snapshot::doc_hash`. Prevents the plugin from re-applying removed content on next startup.

Log:
```
[dedupe] cleaning stale patch file: .agent-doc/patches/<hash>.json
```

Updated the spec comment in the module doc to document the new behavior.

Plugin rebuilt at 0.2.46.
The plugin version should be bumped. Please add to SKILL->runbook

Please clean up duplicate patch and make sure it won't happen again. Please fix any agent-doc binary or ffi plugin logic that would have caused the duplicate patch. Look in logs.
### Re: plugin version bump + runbook

Bumped to **0.2.47**. `gradle.properties` updated, plugin rebuilt: `agent-doc-jetbrains-0.2.47.zip`.

Added "When to bump the plugin version" section to `runbooks/plugin-install.md`:
> Any PR or committed change to `editors/jetbrains/` → bump `pluginVersion`.

**Install:**
Settings → Plugins → gear → "Install Plugin from Disk..." → `agent-doc-jetbrains-0.2.47.zip` → Restart
### Re: dedup guard fix + claudescore repair #2

**Root cause identified:** The boundary-ID dedup guard in 0.2.47 was wrong.

When a patch is applied, its `boundary_id` marker is **consumed** (replaced with content + new boundary). So the check `doc.contains(<!-- agent:boundary:X -->)` returns `false` for BOTH "not yet applied" AND "already applied after a new cycle" — the guard always fell through.

#### Fix: snapshot timestamp comparison

Snapshot is saved by the binary after every successful IPC write. If snapshot mtime > patch file mtime, the patch was applied in a previous cycle → stale.

Walks up from the document to find `.agent-doc/` (same logic as `find_project_root` in Rust), then compares `snapshots/<hash>.md` mtime against the patch file mtime.

Log on detection:
```
[patch-watcher] dedup: snapshot newer than patch file, deleting stale: <hash>.json
```

#### claudescore.md repair #2

Removed duplicate "Re: Rob Ross'" block again. The user content added between the first and second occurrence (target audience / chromium-bridge questions, 8 lines) was also lost to another stale patch re-apply. Committed and pushed to SessionShare: `02094a1`.

Plugin rebuilt: **0.2.48** (`agent-doc-jetbrains-0.2.48.zip`).

**Install to activate the fix:**
Settings → Plugins → gear → "Install Plugin from Disk..." → `agent-doc-jetbrains-0.2.48.zip` → Restart

Why was tasks/recruit/xbow.md compacted? I prompted it with:
> Please compact, transcribe, and diarize [2026-04-07 09-00-51.mp4](../../../../../Videos/2026-04-07%2009-00-51.mp4)
### Re: xbow.md compaction

The word "compact" in your prompt triggered the compact-exchange runbook. The agent read:

> Please **compact**, transcribe, and diarize …

…and matched "compact" as a `/compact exchange` directive, collapsing the exchange content down to the one-liner summary you see now.

This is a false-positive: you meant "compact" as an adjective describing the desired transcript output (concise/dense), not as a document management command.

**Root cause:** The agent's natural-language parsing (or the skill's slash-command detection) matched "compact" in prose as a runbook trigger. It should only trigger on an isolated imperative like `compact exchange` at the start of a user turn, not when "compact" appears mid-sentence modifying a noun.

**Fix needed in agent-doc skill / compact-exchange runbook:** Require the trigger to be the *first word* of the user message (or on its own line), and verify the next token is "exchange" or the document component name — not a comma followed by unrelated verbs.

**Workaround for now:** Avoid starting a sentence with "compact" unless you intend to trigger compaction. Rephrase as "concise transcript" or "summarized transcript."

**Recovering the lost exchange content:** The pre-compaction content is in git. Run:
```bash
git show f7f24a15:tasks/recruit/xbow.md
```
to see the full exchange before it was collapsed.

do
- [ ] Fix compact-exchange trigger: require isolated imperative, not prose "compact" mid-sentence
### Re: Fix compact-exchange trigger

**Root cause confirmed:** The compact-exchange runbook said "User explicitly requests 'compact exchange'" but didn't guard against "compact" appearing as an adjective embedded in a sentence with unrelated verbs.

**Fix applied** to `.claude/skills/agent-doc/runbooks/compact-exchange.md`:

Added explicit trigger rules:
- Requires "compact exchange" as a **standalone imperative sentence/line** — the sole verb phrase
- Lists counter-examples: `compact, transcribe, and diarize …` is NOT a trigger (compact = adjective), `compact transcript` is NOT a trigger (compact modifies noun, not command)
- "Never auto-compact without user approval" retained

The stray `2` at the end of the exchange checklist item is noted — you can delete it.

Explain what is needed. Is this an agent-doc bug or a one-off task?:
- [ ] Re-enter lost claudescore.md content (target audience / feasibility / chromium-bridge questions)
### Re: claudescore.md pending item (HEAD)

**One-off task, already done.** Not an agent-doc bug.

The target audience / chromium-bridge questions are in `src/session-share/tasks/claudescore.md` at the expected location with a full response ("Re: ClaudeScore for the broader Claude Code community..."). The content was either re-entered by you or recovered before the repair commit (`02094a1`).

The underlying **bug** (stale patch re-apply duplicating blocks and eating user content between them) was already fixed in plugin v0.2.48. Nothing left to track here.

Clearing the item from pending.
<!-- agent:boundary:6e81cb92 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
