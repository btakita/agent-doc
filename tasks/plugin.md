---
session: d4eed435-cbf3-44ae-a96c-048b0b2e8025
resume: null
agent: null
model: null
branch: null
---

## JetBrains Plugin — Resolved Issues

### Alt+Enter Keybinding
- **ActionPromoter required** for Alt+Enter — `promote()` puts Agent Doc first for `.md` files
- **"Replace with" is an IDEA bug** (per-file spellcheck issue, not our plugin)
- **Plugin caching** — structural changes require uninstall → restart → install → restart
- Commits: submodule `ccf03d7`, parent `59d702b`

### PromptPoller Race Condition
- **Root cause:** `currentPromptKey`, `answeredPromptKey`, `activePromptQueue` modified by EDT, read by executor thread — stale values caused re-show after answering
- **Fix:** `@Volatile` on all 3 cross-thread fields. `answeredPromptKey` grace period filters only the specific answered prompt (no blanket cooldown)
- Commits: submodule `46d617f` (initial), `f1e71f1` (removed cooldown per user feedback)

### Layout Non-Session Pane Swap
- **Root cause:** `layout.rs` broke out ANY unwanted pane, including non-Claude shells
- **Fix:** Load session registry, only break out registered session panes. Non-session panes (shells, tools) left untouched.
- Commit: submodule `46bf84c`

### PromptPanel Button Overflow
- **Root cause:** Long option text (100+ chars) pushed buttons off-screen — only [3] No visible
- **Fix:** `maxLabelLen` 80 → 45 chars (full text in tooltip), panel height capped at 30% of window
- Commit: submodule `5ab29be`

All pushed to `main` on origin.

## User

I ran Run Agent Doc twice on a document and got an error;

agent-doc route failed (exit 1): not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode Error: tmux send-keys failed (text)

## Assistant

**Fixed.** Double-click guard added to `SubmitAction.kt`.

**Root cause:** No concurrency guard on "Run Agent Doc". Two rapid invocations spawn two `agent-doc route` processes that both `tmux send-keys -l` to the same pane. The second hits while Claude Code's TUI is processing the first command — tmux rejects each of the 11 characters in `/agent-doc ` with "not in a mode".

**Fix:** `AtomicBoolean` guard in `SubmitAction` (same pattern as `EditorTabSyncListener`). While a route is in flight, subsequent clicks show "Route already in progress" hint and skip. The guard releases via `onComplete` callback when the background thread finishes.

**Changes:**
- `SubmitAction.kt` — `AtomicBoolean routing` guard in `actionPerformed()`
- `TerminalUtil.kt` — `sendToTerminal()` accepts optional `onComplete` callback, invoked in `finally` block

**Plugin rebuilt** (both zips). Simple reinstall + restart should suffice.

## User

Is agent-doc-jetbrains and agent-doc-vscode still being used?
agent-doc/editors/jetbrains and agent-doc/editors/vscode should be the canonical source code.

agent-doc-jetbrains and agent-doc-vscode should be deleted.

## Assistant

**Deleted.** Both `agent-doc-jetbrains/` and `agent-doc-vscode/` removed from disk. They were gitignored leftovers from the migration to `agent-doc/editors/` (session 3).

Also cleaned up `.gitignore` — removed the two entries and the "Editor plugins moved" comment since there's nothing left to ignore.

Canonical source: `agent-doc/editors/jetbrains/` and `agent-doc/editors/vscode/` (inside the agent-doc submodule).

## User

I just ran into a usage limit on my Claude Code session. I have an existing JetBrains AI plan.

Can we make the plugin act as a bridge to use the JB AI plan to reduce Claude Code account usage?

## Assistant

**No — JetBrains does not expose a public API for plugins to call the AI Assistant's LLM backend.**

There's no `com.intellij.ai.LlmService` or equivalent that our plugin could call to pipe prompts through the JB AI plan. JetBrains has acknowledged the demand (YouTrack LLM-2340) but hasn't shipped it and gives no ETA.

### What won't work

- **Calling JB AI from our plugin** — The AI Assistant's internals (`com.jetbrains.ai.*`, `ai.grazie.*`) are private, undocumented, and break across versions.
- **InlineCompletionProvider** — Real API, but only for ghost-text code completions, not general-purpose LLM calls.

### What could work

**Option A: Direct Anthropic API backend in agent-doc CLI**
Add a new agent backend (`agent/api.rs`) that calls the Anthropic API directly via HTTP. No Claude Code CLI involved — just `agent-doc run` sends the diff to the API and appends the response. Uses your own Anthropic API key. Simplest path, keeps architecture unchanged.

**Option B: ACP agent (JetBrains' official agent protocol)**
Build agent-doc as a standalone ACP agent process. JetBrains routes AI chat messages to it through their unified AI interface. This is how Claude/Codex integrate with JetBrains now. Downside: replaces our document-based workflow with JB's chat UI.

**Option C: Plugin-side HTTP client**
Plugin calls Anthropic/OpenAI API directly with OkHttp, does diff→prompt→response, writes back to document. Bypasses CLI for submit. How DevoxxGenie/ProxyAI work.

**Recommendation:** Option A — one new file (`agent/api.rs`, ~100 lines), uses `ureq` (already a dependency) to call `api.anthropic.com`. Set `ANTHROPIC_API_KEY`, add `agent: api` to frontmatter.

## User

I already spent money on a JB AI plan so I want to use JB with agent-doc if possible.

Does the ACP agent support routing the agent-doc commands/diffs to the JB AI servers?
Can skills and other context be supported?

## Assistant

**No — ACP does not give agents access to the JB AI models.**

ACP is strictly an IDE-tools protocol (file access, terminal, permissions). Agents must bring their own LLM. There's no sampling/completion/inference capability in the ACP spec — it's deliberately silent on "how agents think."

The ACP spec defines:
- Client→agent: `initialize`, `session/prompt`, `session/cancel`
- Agent→client: `fs/read_text_file`, `fs/write_text_file`, `terminal/create`, `session/request_permission`
- **Zero LLM inference methods.**

**Junie** (JetBrains' own agent) does use the JB AI service, but through a privileged first-party integration — not ACP. Third-party ACP agents don't get this access.

### Re: skills and context

ACP agents get IDE context (open files, terminal, file system) but **not** JetBrains AI features. An ACP agent can read your project files and execute commands, but it must call its own LLM for reasoning. There's no mechanism for an ACP agent to say "hey IDE, run this prompt through whatever model you have."

MCP (Anthropic's protocol) has a "sampling" capability where servers can ask the client to do inference — ACP has no equivalent. The MCP-over-ACP RFD enables MCP tool servers to work through ACP, but even that doesn't bridge LLM access.

### Bottom line

Your JB AI plan gives you Junie, AI Assistant chat, and inline completions — but there's no programmatic API to route custom prompts through it. The subscription is locked to JetBrains' own features.

**Most practical path:** Direct Anthropic API backend (`agent/api.rs`) in agent-doc. Even a small API budget goes far since agent-doc sends targeted diffs, not full codebases.

## User

Jetbrains IDEs have a MCP server. Can this agent-doc route via the MCP server? See screenshot.

## Assistant

**The JB MCP Server goes the opposite direction from what you need.**

It exposes IDE tools (file editing, run configs, inspections) **to** Claude Code — not JB AI models **from** Claude Code:

```
Claude Code (LLM + client) → MCP → JetBrains IDE (tools server)
```

You want `agent-doc → ??? → JetBrains AI (LLM)` — but that path doesn't exist. The MCP server doesn't proxy LLM requests through JB AI credits.

### What the JB MCP Server IS useful for

If you wire Claude Code as the MCP client (click "Auto-Configure"), then your Claude Code sessions gain IDE-aware tools:
- Run configurations and build tasks
- Code inspections and quick-fixes
- Refactoring operations
- Navigation and symbol resolution

Since agent-doc already routes to Claude Code via tmux, your agent-doc sessions would **indirectly** gain these IDE capabilities. That's valuable — but the LLM is still Claude (your Claude Code plan), not JB AI.

### The hard truth about JB AI plan

The JB AI subscription is a **closed ecosystem**: Junie, AI Assistant chat, inline completions, AI-powered inspections. No programmatic API, no MCP sampling, no ACP inference passthrough. It can't be used as an LLM backend by external tools.

### Practical options to reduce Claude Code usage

1. **Anthropic API backend** (`agent/api.rs`) — pay-per-token, much cheaper for diff-sized prompts
2. **Local models via Ollama** — free, works offline, good for routine tasks
3. **Use JB AI for what it does well** — Junie for standalone tasks, AI Assistant for quick questions — reserve Claude Code for agent-doc sessions

## User
