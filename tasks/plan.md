---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.2.0 — crates.io + GitHub Releases. 77 tests passing (all 16 edge cases covered).
  - Tmux session routing: `agent-doc start` and `agent-doc route` subcommands with auto-start cascade
  - `Tmux` struct with `-L` socket isolation for testing (9 tmux isolation tests)
  - Session registry (`sessions.json`) — UUID → tmux pane mapping, liveness checks, auto-start
  - Upgrade check + self-update via GitHub Releases
  - Auto-frontmatter: `ensure_session()` generates UUID v4 for documents without one
  - Separated `session` (routing UUID) from `resume` (Claude conversation ID) in frontmatter
- **corky**: v0.10.0 — crates.io + GitHub Releases. 220 tests passing.
  - Social media posting: LinkedIn OAuth2, draft creation/publishing, 6 CLI subcommands, 54 social-specific tests
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge
  - Upgrade check + self-update via GitHub Releases
- **instruction-files**: v0.1.0 — crates.io. 45 tests passing.
  - Shared audit crate: staleness, tree paths, line budget, actionable content checks
  - `AuditConfig::agent_doc()` / `AuditConfig::corky()` presets (planned: collapse to single polyglot default)
  - README with usage, config reference, consumer links
- **agent-doc-jetbrains**: v0.1.0 — signed plugin. Hotkey (`Ctrl+Shift+Alt+A`) + right-click context menu. Calls `agent-doc route` via `ProcessBuilder`.
- **agent-doc-vscode**: v0.1.0 — extension. Same hotkey, calls `agent-doc route` via `child_process.exec`.
- **agent-loop**: Root Makefile with `make release`, `make update`, `make check`, `make precommit`, `make publish`, `make clean`.

### Session Routing (tmux)

```
agent-doc start <file>   # register pane in sessions.json, exec claude
agent-doc route <file>   # look up session from frontmatter, send-keys to pane, auto-start if needed
```

**Frontmatter:**
```yaml
---
session: cf853a21-...    # permanent routing UUID (for tmux pane lookup)
resume: abc123...        # Claude conversation ID (for --resume)
---
```

**Auto-start cascade** (when pane is dead/missing):
1. tmux not running → `tmux new-session -s claude`
2. "claude" session missing → create it
3. "claude" session exists → `tmux new-window`
4. Start `agent-doc start` in new pane

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### instruction-files

- [ ] Collapse `AuditConfig` presets to single polyglot `Default` (always audit both AGENTS.md + CLAUDE.md)
- [ ] `.instruction-files.toml` project-root config file (override extensions, root markers, skip dirs)

### corky — Topics

- [ ] `TopicConfig` struct + wire into `CorkyConfig` deserialization
- [ ] Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
- [ ] Topic sync: eligibility check (label/contact match OR `shared_with`), delegate to shared merge
- [ ] Thread tagging: auto-add `**Topic**: {name}` to matching conversations during sync
- [ ] `corky topics suggest` — scan untagged threads, propose assignments
- [ ] `corky topics list` — show topics with sync status per mailbox

### corky — Telegram

- [ ] Phase 2: Live sync via `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Phase 3: Watch mode (real-time push)
- [ ] Phase 4: Send via `draft push`

### corky — Slack

- [ ] Phase 2: API sync via `slack-morphism` — plan at `corky/tasks/integrations/slack.md`
- [ ] Phase 3: Socket Mode (real-time push)

### Future

- [ ] Streaming responses (agent-doc)
- [ ] Document-only permission flow — no terminal (agent-doc)
- [ ] Desktop notifications on completion (agent-doc)
- [ ] Multi-agent documents — sections routed to different agents (agent-doc)
- [ ] Email transport for async human-agent loops — bridge with corky (agent-doc)
- [ ] Personal data separation — nested git repo inside `tasks/` (agent-loop)

## Design Decisions

### Frontmatter: session vs resume

`session` is the permanent document routing UUID (generated once, never changes). `resume` is the Claude conversation ID (set after first submit, cleared by `agent-doc reset`). Separating them prevents tmux routing breakage when Claude overwrites the session ID.

### instruction-files: path + version dependency

```toml
instruction-files = { path = "../instruction-files", version = "0.1" }
```

Local builds use path. `cargo publish` uses crates.io version. Publish order: instruction-files → agent-doc → corky.

### instruction-files: polyglot default + .instruction-files.toml

Collapse to a single `AuditConfig::default()` that always discovers and audits both `AGENTS.md` and `CLAUDE.md`. Convention: `CLAUDE.md -> AGENTS.md` symlink so both names resolve.

Config at `.instruction-files.toml` in project root (optional). Resolution: config file → `AuditConfig::default()`. Consuming binaries read the TOML — zero new deps in the library crate.

### tmux routing: direct, not broadcast

Each document has its own session UUID → tmux pane. Direct `send-keys -t <pane>` routing. Broadcasting (`synchronize-panes`) would send the same command to all panes. Extract as separate crate when a second consumer exists.

### Topics — `.corky.toml` Config

```toml
[topics.project-alpha]
labels = ["project-alpha", "alpha-dev"]
contacts = ["alice", "bob"]
shared_with = ["alice", "bob"]
```

### Context Routing

Topics, contacts, and threads share the same sync primitive — content-hash-based 3-way merge with eligibility rules.

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `CLAUDE.md`

## Assistant

### Cleaned up plan.md

Collapsed 710 lines of conversation history. Updated Status to current versions (agent-doc v0.2.0, corky v0.10.0). Added new design decisions from the session (polyglot default, `.instruction-files.toml`, tmux direct routing).

### agent-doc is ready for tmux + JetBrains integration

Everything is implemented and released. Your installed binary is v0.1.4 — needs upgrading.

**Step 1: Upgrade agent-doc binary**
```bash
cd ~/work/btakita/agent-loop/agent-doc && cargo install --path .
# Verify: agent-doc --version → 0.2.0
```

**Step 2: Ensure tmux is available**
```bash
tmux -V   # should return tmux 3.x
```

**Step 3: Install JetBrains plugin**
- Settings > Plugins > gear icon > Install Plugin from Disk
- Select `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip`
- Restart IDE

**Step 4: Test the flow**
1. Open any `.md` file with `session:` frontmatter in JetBrains
2. Press `Ctrl+Shift+Alt+A` (or right-click > "Submit to Agent Doc")
3. This calls `agent-doc route <path>` which:
   - Reads session UUID from frontmatter (generates one if missing)
   - Looks up tmux pane in `.agent-doc/sessions.json`
   - If no pane → auto-start cascade creates tmux "claude" session, starts Claude
   - If pane exists → `tmux send-keys` sends `/agent-doc <path>` to the pane

**Alternative (terminal-only):**
```bash
agent-doc route tasks/plan.md
```

## User

java.io.IOException: Cannot run program "agent-doc-route" (in directory "/home/brian/work/btakita/agent-loop"): Exec failed, error: 2 (No such file or directory)
at java.base/java.lang.ProcessBuilder.start(ProcessBuilder.java:1170)
at java.base/java.lang.ProcessBuilder.start(ProcessBuilder.java:1089)
at com.github.btakita.agentdoc.TerminalUtil.sendToTerminal(TerminalUtil.kt:32)
at com.github.btakita.agentdoc.SubmitAction.actionPerformed(SubmitAction.kt:20)
at com.intellij.openapi.actionSystem.ex.ActionUtil.doPerformActionOrShowPopup(ActionUtil.kt:437)
at com.intellij.openapi.keymap.impl.ActionProcessor.performAction(ActionProcessor.kt:26)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcher$actionProcessor$1.performAction(IdeKeyEventDispatcher.kt:499)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcherKt.doPerformActionInner$lambda$0$0(IdeKeyEventDispatcher.kt:824)
at com.intellij.openapi.application.TransactionGuardImpl.performActivity(TransactionGuardImpl.java:109)
at com.intellij.openapi.application.TransactionGuardImpl.performUserActivity(TransactionGuardImpl.java:98)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcherKt.doPerformActionInner$lambda$0(IdeKeyEventDispatcher.kt:823)
at com.intellij.openapi.actionSystem.impl.ActionManagerImpl.performWithActionCallbacks$lambda$0(ActionManagerImpl.kt:1164)
at com.intellij.concurrency.ThreadContext.installThreadContext(threadContext.kt:305)
at com.intellij.openapi.actionSystem.impl.ActionManagerImpl.performWithActionCallbacks(ActionManagerImpl.kt:1162)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcherKt.doPerformActionInner(IdeKeyEventDispatcher.kt:821)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcherKt.access$doPerformActionInner(IdeKeyEventDispatcher.kt:1)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcher.processAction(IdeKeyEventDispatcher.kt:554)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcher.processAction(IdeKeyEventDispatcher.kt:512)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcher.processActionOrWaitSecondStroke(IdeKeyEventDispatcher.kt:451)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcher.inInitState(IdeKeyEventDispatcher.kt:444)
at com.intellij.openapi.keymap.impl.IdeKeyEventDispatcher.dispatchKeyEvent(IdeKeyEventDispatcher.kt:306)
at com.intellij.ide.IdeEventQueue.dispatchKeyEvent(IdeEventQueue.kt:555)
at com.intellij.ide.IdeEventQueue._dispatchEvent$lambda$3(IdeEventQueue.kt:509)
at com.intellij.platform.locking.impl.NestedLocksThreadingSupport.doRunWriteIntentReadAction(NestedLocksThreadingSupport.kt:736)
at com.intellij.platform.locking.impl.NestedLocksThreadingSupport.runPreventiveWriteIntentReadAction(NestedLocksThreadingSupport.kt:710)
at com.intellij.ide.IdeEventQueue._dispatchEvent(IdeEventQueue.kt:509)
at com.intellij.ide.IdeEventQueue.dispatchEvent$lambda$0$0$0$0(IdeEventQueue.kt:337)
at com.intellij.openapi.progress.impl.CoreProgressManager.computePrioritized(CoreProgressManager.java:901)
at com.intellij.ide.IdeEventQueue.dispatchEvent$lambda$0$0$0(IdeEventQueue.kt:336)
at com.intellij.ide.IdeEventQueueKt.performActivity$lambda$1(IdeEventQueue.kt:1065)
at com.intellij.openapi.application.TransactionGuardImpl.performActivity(TransactionGuardImpl.java:117)
at com.intellij.ide.IdeEventQueueKt.performActivity(IdeEventQueue.kt:1065)
at com.intellij.ide.IdeEventQueue.dispatchEvent$lambda$0(IdeEventQueue.kt:331)
at com.intellij.ide.IdeEventQueue.dispatchEvent(IdeEventQueue.kt:371)
at java.desktop/java.awt.EventDispatchThread.pumpOneEventForFilters(EventDispatchThread.java:207)
at java.desktop/java.awt.EventDispatchThread.pumpEventsForFilter(EventDispatchThread.java:128)
at java.desktop/java.awt.EventDispatchThread.pumpEventsForHierarchy(EventDispatchThread.java:117)
at java.desktop/java.awt.EventDispatchThread.pumpEvents(EventDispatchThread.java:113)
at java.desktop/java.awt.EventDispatchThread.pumpEvents(EventDispatchThread.java:105)
at java.desktop/java.awt.EventDispatchThread.run(EventDispatchThread.java:92)
Caused by: java.io.IOException: Exec failed, error: 2 (No such file or directory)
at java.base/java.lang.ProcessImpl.forkAndExec(Native Method)
at java.base/java.lang.ProcessImpl.<init>(ProcessImpl.java:295)
at java.base/java.lang.ProcessImpl.start(ProcessImpl.java:225)
at java.base/java.lang.ProcessBuilder.start(ProcessBuilder.java:1126)
... 39 more
