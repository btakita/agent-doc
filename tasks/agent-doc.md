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

## Assistant

**Implemented `agent-doc outline <file> [--json]`** — committed (`26b72c6`), pushed.

**What it does:** Parses markdown headings into a section tree with line counts and approximate token counts (bytes/4). Two output modes:

```
$ agent-doc outline tasks/plan.md
Plan: agent-doc & agent-loop                2 lines  ~    0 tokens
  Status                                     20 lines  ~  345 tokens
    Session Routing (tmux)                     15 lines  ~  167 tokens
  Outstanding                                 2 lines  ~    0 tokens
    ...
  User                                        7 lines  ~   43 tokens
  Assistant                                  20 lines  ~  290 tokens
---
Total                                     178 lines  ~ 2226 tokens
```

`--json` outputs `[{"heading","depth","line","lines","tokens"}, ...]` for tooling integration.

**Files:**
- `src/outline.rs` — 220 lines, 5 unit tests (heading parsing, sections, preamble, empty, JSON)
- `src/main.rs` — wired as `Commands::Outline { file, json }`
- **SPEC.md** — §7.16 outline spec
- **README.md** — Quick Start + CLI Reference entries
- **CLAUDE.md** — module layout entry
- **SKILL.md** (bundled) — version bumped to 0.5.5

All tests pass (98 total), clippy clean, audit-docs clean.

## User

Read https://schipper.ai/posts/parallel-coding-agents/

Anything that we can learn? Any improvements to our process?

## Assistant

**Read it.** They run 8 parallel Claude Code agents via tmux with a Feature Design (FD) markdown lifecycle — similar to our setup but with a few patterns worth stealing.

**Already doing:**
- tmux session routing (ours is more sophisticated — window-scoped, plugin-integrated)
- Markdown session docs (their FDs ≈ our `tasks/*.md`)
- Slash commands (their `/fd-*` ≈ our `/agent-doc`)
- Subagent exploration, commit tracking

**Worth adopting:**

1. **Tmux idle notification** — They change window color when agents go idle (Claude Code notification hook → tmux bell). Low effort, high signal for monitoring multiple agents. Could add to `agent-doc` as a hook.

Do it.

2. **Commit-doc linking** — Their commits reference FD numbers (`FD-049: Implement...`). Our `agent-doc commit` just timestamps. Adding the doc name would create a searchable trail: `agent-doc(plugin): 2026-03-02 13:56`.

Add the doc name to the commits.

3. **`/fd-deep` fan-out** — 4 parallel Opus subagents exploring different design angles. We do this ad-hoc; they formalized it as a command. Could be `agent-doc deep <file>`.

Ok...could you specify what would trigger this action from the agent doc? Do I need to explicitly command to explore multiple options?

4. **Lifecycle frontmatter** — 8-stage status tracking (Planned → Designed → Open → In Progress → Complete). A `status:` field in our session doc frontmatter would help track many concurrent docs.

This seems to be explicit to one doc per feature? Our docs are rolling atm. But it may make sense to break out features as well. So the status tracking could be useful to coordinate parallel agents. If each task were to write it's own agent doc, what would the flow look like? Would an agent doc stick around until deleted? Is an agent doc essentially a feature of the system that can evolve over time?

5. **Context checkpointing** — They explicitly compact FD progress to avoid context burn. Our long docs (plan.md at 300+ lines) could benefit from periodic summarization of completed sections.

Yes. I do explicitly request to compact a doc. Perhaps we could have a command. Also it may make sense to suggest the doc be compacted. Particularly if parts of the doc are compactable...unrelated to the current topic and complete.

**Not worth adopting:** Their alias system (we have `route`/`focus`), Cursor cross-checking (we use subagents), 8-agent limit discussion (already handled by window scoping).

## User

