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

## Assistant

### 1. Tmux idle notification — Done

Added hooks to `.claude/settings.json`:
- **`Stop`** → pane border turns **yellow** (colour226) — agent idle, waiting for input
- **`UserPromptSubmit`** → pane border turns **blue** (colour39) — agent working

Takes effect next session start. The `2>/dev/null` suppresses errors when running outside tmux.

I lost the binding of the selected Jetbrains editor and the claiming tmux pane.

### 2. Commit-doc linking — Done

Changed `agent-doc commit` format from `agent-doc: timestamp` to `agent-doc(doc-name): timestamp`.

```
# Before:
agent-doc: 2026-03-02 14:07:07

# After:
agent-doc(agent-doc): 2026-03-02 14:11:45
agent-doc(plan): 2026-03-02 14:12:00
```

Updated squash grep pattern (`--grep=^agent-doc`) to match both old and new formats. Committed (`e75e3b6`), pushed.

### 3. Deep exploration trigger

No — you wouldn't need to explicitly command it. The natural trigger is **design ambiguity in the document**. When the agent detects multiple viable approaches in a user request (e.g., "how should we handle X?"), it would autonomously spawn parallel subagents exploring different angles, then synthesize results.

But formalizing this as `agent-doc deep <file>` has a different value: **you** can trigger it when you want breadth. "I'm stuck on the right approach for auth — deep explore this." The command fans out 3-4 subagents with different framing prompts, each writes findings, then synthesizes.

Implementation would be: read the latest `## User` block, generate N variant prompts (e.g., "design X prioritizing simplicity", "design X prioritizing extensibility"), run them as parallel subagents, collect results into a `## Deep Exploration` section. Not complex to build — the skill infrastructure already supports subagents.

### 4. Per-feature docs and lifecycle

Yes — an agent doc can be both a **rolling conversation** (like plan.md) and a **feature spec** (like their FDs). The key difference is scope:

- **Rolling docs** (plan.md, plugin.md): topic-scoped journals. No lifecycle — they grow, get compacted, continue.
- **Feature docs** (tasks/codex.md, tasks/telegram.md): bounded scope, clear done state. These benefit from lifecycle tracking.

**Flow for per-feature docs:**
1. Create: `tasks/feature-name.md` with `status: planned` in frontmatter
2. Design phase: user + agent iterate until `status: designed`
3. Implementation: agent works, `status: in-progress`
4. Verification: tests pass, docs updated, `status: complete`
5. The doc **stays** as a decision record (like their 300+ FDs). Never deleted — searchable history.

The feature docs seem to be an extension of SPEC.md. Similar to how the ontology of philosophy nodes and broken out terms in the entire context.

Should each project have it's own feature directory? To track these specs?

**Coordination across agents:** Each agent claims a feature doc. `agent-doc prompt --all` already polls all sessions. Adding `status:` to frontmatter lets a dashboard view (`agent-doc status`) show all active feature docs and their states. The plan.md rolling doc becomes the orchestration layer — it references feature docs, not duplicates their content.

**An agent doc as an evolving feature:** Yes. A completed feature doc can reopen (`status: in-progress`) when the feature evolves. The conversation history IS the decision trail.

### 5. Compaction command

Two parts:

**`agent-doc compact <file>`** — CLI command that:
1. Reads the document, identifies completed `## User` / `## Assistant` exchanges
2. Summarizes them into a `## Session Log` section (like plan.md's existing format)
3. Removes the original exchanges, preserving the latest active conversation
4. Updates the snapshot

**Auto-suggestion** — The skill workflow could detect compactable state:
- If doc exceeds a threshold (e.g., 200 lines)
- If there are completed exchanges unrelated to the current topic
- Print: "This doc has N completed exchanges (~K tokens). Run `/agent-doc compact` to summarize them."

The suggestion is passive — never auto-compacts. The user decides. This is the same philosophy as `corky filter push` being manual.

## User

I saw the post at https://news.ycombinator.com/item?id=47218318

Is it worthwhile to comment in HN about agent-doc? Will we benefit in any way?

## Assistant

### Inline responses

**Re: "I lost the binding"** — The `sessions.json` claims are intact (`tasks/agent-doc.md` → pane `%4`, window `@1`). The tmux hooks shouldn't affect claim routing — they just set pane border colors, not session state. If the JetBrains "Run Agent Doc" isn't routing correctly, re-claim from terminal: `agent-doc claim tasks/agent-doc.md --window @1`.

**Re: Feature docs and SPEC.md** — Yes, exactly. SPEC.md is the **resolved definition** (canonical truth, concise). Feature docs are the **decision trails** (how you got there, alternatives considered). When a feature completes, key decisions get extracted into SPEC.md sections.

Each project with agent-doc sessions should have its own feature directory:

```
project/
  SPEC.md            # stable architecture decisions
  specs/             # feature specs (lifecycle-tracked agent docs)
    feature-a.md     # status: complete — becomes a decision record
    feature-b.md     # status: in-progress
  tasks/             # rolling session docs (journals)
    plan.md
```

This mirrors the philosophy ontology: SPEC.md = resolved entity, specs/ = evolution of understanding. `agent-doc status` would scan `specs/*.md` frontmatter across projects for lifecycle dashboards.

### HN comment — worth it, with conditions

**Read the thread.** It's the schipper.ai article. Discussion covers drift/rot concerns, resource limits (Max plan token caps), and competing approaches (CAS automated supervisor, single-agent preference).

**What agent-doc offers that the thread hasn't seen:**
- **tmux routing as first-class infrastructure** — `claim`, `route`, `focus`, `layout` with window scoping
- **JetBrains/VSCode plugin integration** — IDE-native hotkeys for submit/claim/layout
- **SKILL.md bundled workflow** — Claude Code skill that auto-updates from the binary
- **Snapshot-based diff** — comment-aware, minimal-payload document sessions
- **Superproject-aware git** — commits from submodule context resolve to correct repo

**Benefits of commenting:**
1. **Early users / testers** — someone might try it, file issues, give feedback
2. **Visibility** — agent-doc is a working open-source tool, not a pitch. HN respects that.
3. **Network** — the schipperai author and other parallel-agent builders could become collaborators

**Risks:**
- Low — HN comments on 4-hour-old posts with 30+ comments still get read
- agent-doc is niche (tmux + markdown + Claude Code) — won't appeal to everyone
- No polished landing page yet — README.md is the only onboarding

**Recommendation:** Comment with a brief "Show HN"-style description: what agent-doc does, how it differs from the FD approach, link to the repo. Keep it 3-4 paragraphs. Don't pitch — show.

## User

