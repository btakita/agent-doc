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

## Session Log

### v0.5.5 release
- Released agent-doc v0.5.5 — 12 files, 214 insertions: window-scoped routing, `--pane`/`--window` flags, layout safeguards
- 93 tests pass, clippy clean. Tagged, pushed, published to crates.io. GH Actions built binaries.

### `agent-doc outline` implementation
- Implemented `agent-doc outline <file> [--json]` — parses markdown headings into section tree with line/token counts
- 220 lines in `src/outline.rs`, 5 unit tests. Added to SPEC.md, README, CLAUDE.md, SKILL.md.

### schipper.ai parallel agents article analysis
- Read [Parallel coding agents with tmux and Markdown specs](https://schipper.ai/posts/parallel-coding-agents/). Identified 5 improvements:
  1. **Tmux idle notification** — Done. `.claude/settings.json` hooks: Stop→yellow border (idle), UserPromptSubmit→blue border (working). Uses `-pu` to unset active border style so default active/inactive distinction preserved.
  2. **Commit-doc linking** — Done. `agent-doc commit` format changed to `agent-doc(doc-name): timestamp`. Squash grep updated to `--grep=^agent-doc`.
  3. **Deep exploration** (`agent-doc deep`) — Designed, not built. Fan-out parallel subagents with variant prompts, synthesize results.
  4. **Per-feature docs + lifecycle** — Designed. Rolling docs (journals) vs feature docs (bounded, lifecycle-tracked). `specs/` directory for feature specs, `status:` frontmatter.
  5. **Compaction** (`agent-doc compact`) — Designed, not built. Summarize completed exchanges, auto-suggest when doc exceeds threshold.

### HN engagement
- Drafted reply to CloakHQ's comment on HN thread about context drift. Recommended both: brief comment now + polished Show HN in 1-2 weeks (landing page, marketplace submissions, README overhaul).

### v0.5.6 release
- Released agent-doc v0.5.6 — commit-doc linking. Credited schipper.ai article in GitHub Release notes.
- Published to crates.io, SKILL.md updated, parent pointer pushed.

### workmux evaluation
- Evaluated [workmux](https://github.com/raine/workmux) — complementary but not needed. Our submodule + subagent setup provides sufficient isolation. workmux adds value for monorepo multi-agent scenarios we don't have.
- Designed worktree-per-feature integration: docs in main (human-editable), code in worktrees (agent-isolated). Not building yet.

### 2D tmux stacking → pane-groups spec
- Explored nested tmux splits for column-based layout (left stacked | right full-height).
- Dynamic pane groups: tmux windows as document-bound groups, window switching = visibility toggle.
- **Created `agent-doc/specs/pane-groups.md`** — first file in new `specs/` directory. Status: planned.

## User

commit agent-doc

## Assistant

**Committed and pushed.**

- `agent-doc` submodule: `c5e8973` — `specs/pane-groups.md` (new file, 80 lines)
- Parent pointer: `0d39066`

## User

I actually do want to support multiple agents per side...so 2d support.
I could start with the left or right side having a vertical stack in the editor...with the tmux window matching the layout.

left1 | right full
left2 | ...
