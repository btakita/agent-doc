---
session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
resume: null
agent: null
model: null
branch: null
---

## Tmux Routing — Resolved

**Problem:** Flickering on tab switch, CWD drift in snapshots, claim routing to wrong terminal instance, layout pulling panes across windows.

**Fixes applied:**

1. **Tmux config** — Added `escape-time 10`, `focus-events on`, `terminal-overrides Tc` to `~/.config/tmux/.tmux.basic.conf`

2. **CWD drift** — Agent skill must use absolute paths for snapshot operations (no code change, behavioral)

3. **Window-scoped routing** — Full solution for multi-terminal claim routing:
   - `SessionEntry` now stores `window: String` alongside pane ID
   - `register_with_pid()` auto-queries window ID from tmux
   - `pane_by_position_in_window()` — position resolution scoped to specific window
   - `--window` and `--pane` flags on `claim`, `focus`, `layout` CLI commands
   - `TerminalUtil.projectWindowId()` reads window ID from sessions.json (matches by project CWD)
   - `ClaimAction`, `SyncLayoutAction`, `EditorTabSyncListener` all pass `--window`

4. **Layout safeguards** (`layout.rs`):
   - `--window` filtering (lines 80-95) — retains only panes within the specified window
   - Session registry check (lines 151-167) — only breaks out registered session panes, leaves non-session panes (git shells, tools) untouched

**JetBrains terminal integration** — Built (`AgentDocTerminalFactory.kt`, `TmuxSessionManager.kt`), then **removed** per user preference. User prefers external terminals. `--pane` CLI flag retained for future neovim/editor-agnostic use.

**Current flow:** User claims from terminal (establishes window binding) -> Plugin reads window ID from registry -> all subsequent IDE actions scope to that window. Recovery after reboot: `route` auto-starts dead panes, re-registers fresh IDs.

**Pending:** `TmuxBatch` command batching (collect multiple tmux ops into single process invocation) to further reduce tab-switch flicker. Not yet implemented.

## User

commit agent-doc. bump version. Let gh actions do the release

## Assistant

**Released agent-doc v0.5.5.**

- **Committed** (`93d077a`) — 12 files, 214 insertions: window-scoped routing, `--pane`/`--window` flags, layout safeguards
- **Bumped** 0.5.4 → 0.5.5 in `Cargo.toml` + `pyproject.toml`
- **Tests:** clippy clean, 93 tests pass (72 unit + 21 integration)
- **Tagged** `v0.5.5` and pushed to origin — GH Actions building binaries + GitHub Release → PyPI auto-publishes
- **crates.io:** published manually (not automated)
- **Parent submodule** pointer updated (`772a2ec`)

## User

Are there any plugins that can structure Markdown files?
Such as document structure. And summary of the text in a section?

## Assistant

**No existing plugin does this well.** Tree-sitter has a markdown parser but the AST is just headings/paragraphs/lists — not semantic. `smart_outline` on markdown would just be a table of contents.

**What you'd actually want:** A `doc-outline` tool that parses `##` headings, returns heading tree with token counts per section, optionally 1-line AI summary per section (cached), and lets you "unfold" a specific section by heading name. The markdown equivalent of `smart_outline`/`smart_unfold`.

**Simplest path:** Add `agent-doc outline <file>` to the CLI — split on `^## `, count tokens, optional summary. Or extend claude-mem plugin since it already has the `smart_*` infrastructure. No tree-sitter needed for markdown.

**Impact:** Session docs like plan.md (160 lines) → heading structure + summaries in ~200 tokens instead of ~2K tokens for full read.

## User

Implement. Add to docs and skill.
