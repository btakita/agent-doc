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

I have the binding. But the selected TMUX pane is no longer obvious. Can we have some sort of coloring to show which TMUX pane is selected?

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
The plugins are commit to the project but not released yet.
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

Should we have a separate posting. Which would give me some time to polish agent-doc. Like a website & releasing the editor plugins.

Should we wait for the separate posting or should we comment here as well?

## Assistant

**Do both.** They serve different purposes:

**Comment on this thread now** (5 min effort):
> "I built something similar — `agent-doc`: tmux session routing + IDE plugins for parallel Claude Code sessions. It routes `/agent-doc` slash commands to specific tmux panes, manages claims/layout across windows, and uses markdown docs as the agent interface. Repo: [link]"

This is a seed — people browsing the thread see it, some star the repo. No pressure to be polished.

**Show HN in 1-2 weeks** (polished launch):
- Landing page on briantakita.me
- JetBrains Marketplace + VSCode Marketplace submissions
- README overhaul with GIF demo
- Optional: 2-min demo video

The comment won't hurt the Show HN. Different audiences — HN thread readers vs front-page browsers. And having a few stars/issues before launch adds credibility ("people are already using this").

**Show HN timing:** Weekday morning US Pacific (Tuesday-Thursday, 8-10am). Avoid Monday/Friday.

## User

Please write the comment. Should it be a reply to another message in the thread or stand-alone?

## Assistant

**Reply to CloakHQ's comment** — they explicitly ask about context drift across parallel sessions. Perfect entry point.

Draft:

> I've been building agent-doc [1] to solve exactly this. Each parallel Claude Code session gets its own markdown document as the interface (e.g., tasks/plan.md, tasks/auth.md). The agent reads/writes to the document, and a snapshot-based diff system means each submit only processes what changed — comments are stripped, so you can annotate without triggering responses.
>
> The routing layer uses tmux: `agent-doc claim`, `route`, `focus`, `layout` commands manage which pane owns which document, scoped to tmux windows. A JetBrains plugin lets you submit from the IDE with a hotkey — it finds the right pane and sends the skill command.
>
> For context sync across agents, the key insight was: don't sync. Each agent owns one document with its own conversation history. The orchestration doc (plan.md) references feature docs but doesn't duplicate their content. When an agent finishes a feature, its key decisions get extracted into SPEC.md. The documents ARE the shared context — any agent can read any document.
>
> It's been working well for running 4-6 parallel sessions across corky (email client), agent-doc itself, and a JetBrains plugin — all from one tmux window with window-scoped routing.
>
> [1] https://github.com/btakita/agent-doc

**Why reply vs standalone:** Replying to CloakHQ frames it as answering their question (HN culture rewards this). Standalone carries more "look at my thing" energy. The reply will still be visible to everyone browsing the thread.

## User

Can we readd highlighting which TMUX pane is currently selected. I only see 1 yellow line between the 2 panes now.

## Assistant

**Fixed.** The hooks were overriding `pane-active-border-style` too, wiping out the active/inactive distinction.

Changed hooks to:
- Only set `pane-border-style` (inactive border) — yellow=idle, blue=working
- **Unset** `pane-active-border-style` (`-pu`) — falls back to tmux default (green/bright)

Active pane now shows the default tmux active border. Applied immediately to this pane.

## User

let's version bump and release the agent-doc changes.
 
Credit the article

Did you credit the article for these changes?

## Assistant

**Released agent-doc v0.5.6.**

- Bumped 0.5.5 → 0.5.6 in `Cargo.toml` + `pyproject.toml`
- Clippy clean, 21 tests pass
- Tagged `v0.5.6`, pushed — GH Actions building binaries
- **crates.io:** published
- **SKILL.md** bumped to 0.5.6, installed
- Parent pointer updated, pushed

**Credited the article** in the [GitHub Release notes](https://github.com/btakita/agent-doc/releases/tag/v0.5.6):

> The commit-doc linking feature was inspired by [Parallel coding agents with tmux and Markdown specs](https://schipper.ai/posts/parallel-coding-agents/) by @schipperai, where Feature Design (FD) numbers are referenced in every commit message.

## User

Can [workmux](https://github.com/raine/workmux) be beneficial to our development flow?

## Assistant

**Complementary, not competing.** workmux solves git worktree isolation for parallel agents; agent-doc solves document-based conversation with agents. Different problems.

**What workmux adds:** worktree management (create → work → merge → cleanup), dashboard TUI, agent status in window names, multi-terminal support (kitty, WezTerm, Zellij).

**What we have that it doesn't:** document-as-interface, snapshot diffs, SKILL.md workflow, IDE plugins, prompt detection, outline.

**Not needed for our current flow.** We use submodules — parallel agents touch different repos (corky, agent-doc, instruction-files), so code conflicts don't arise. workmux shines when multiple agents modify the same codebase (monorepo feature branches).

**Future integration:** If we adopt feature-branch-per-agent workflows, workmux creates the worktree, agent-doc manages the conversation inside it. Worktree = isolation, document = context.

## User

I happen to just run a session per project. But I am interested in the worktree on the same repo.

If we were to integrate, what would it look like?

## Assistant

**Worktree-per-feature with document sessions.** Here's the flow for a project like corky:

### Kickoff
```sh
# Create spec doc in main working tree
echo "## User\n\nAdd telegram integration" > specs/telegram.md

# workmux creates worktree + tmux window + Claude session
workmux add telegram -p "Implement per specs/telegram.md"
```
Creates `.worktrees/telegram/` (git worktree branched from main), tmux window with Claude running inside.

### The bridge
Spec doc lives in **main** (you edit it from IDE). Agent works in the **worktree** (code changes isolated).

```
main tree:          specs/telegram.md   ← human edits
worktree:           src/telegram/...    ← agent writes code
```

`agent-doc route specs/telegram.md` sends the command to the worktree's Claude pane. The agent reads the spec, implements in the isolated worktree. No branch switching needed.

### Parallel features
```sh
workmux add telegram     # worktree 1, tmux window 1
workmux add transcribe   # worktree 2, tmux window 2
workmux add filters      # worktree 3, tmux window 3
```
Three worktrees, three Claude sessions, zero merge conflicts. Each agent owns its branch. You edit all three specs from the same IDE window.

### Merge and cleanup
```sh
workmux merge telegram   # merge branch → main, delete worktree, close window
# specs/telegram.md stays as decision record (status: complete)
```

### What needs building
- `agent-doc worktree <spec>` — wraps workmux + session claim
- Session registry stores `worktree_path` alongside pane/window
- Spec frontmatter: `worktree: telegram` binds doc to worktree
- `agent-doc commit` already resolves git root correctly (works in worktrees)

**Key insight:** Documents stay in main (human-facing, IDE-editable). Code changes happen in worktrees (agent-facing, isolated). Clean separation.

## User

Can we have tmux windows stack vertically on the same side as the editor document?
