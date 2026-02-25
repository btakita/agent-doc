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

## Assistant

### Fixed — JetBrains plugin was calling the old shell script name

The installed plugin was running `agent-doc-route` (old shell script) instead of `agent-doc route` (Rust subcommand).

**Root cause:** The installed plugin was stale — built before the shell-to-Rust migration. Two fixes applied:

1. **`SubmitAction.kt`** — was passing `/agent-doc $relativePath` as the path argument to `sendToTerminal`, which would produce `agent-doc route "/agent-doc tasks/plan.md"`. Fixed to pass just `relativePath`.
2. **`extension.ts`** (VSCode) — cosmetic fix to error message string.

**Plugin rebuilt.** Reinstall from the fresh unsigned zip:
- Settings > Plugins > gear icon > Install Plugin from Disk
- Select `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0.zip`
- Restart IDE

Also make sure `agent-doc` binary is v0.2.0:
```bash
agent-doc --version   # should show 0.2.0
# If not: cd ~/work/btakita/agent-loop/agent-doc && cargo install --path .
```

## User

Nothing seems to happen when I run Ctrl+Alt+Shift+a...or run the Submit to Agent Doc command...

How do I post images other than the cli...I will post a screenshot after to follow up this agent-doc call.

Can we always build the signed Jetbrains plugin after building the unsigned plugin?

## Assistant

### 1. Plugin fix — nothing happens because JetBrains can't find `agent-doc`

JetBrains doesn't inherit your shell PATH (`~/bin` isn't visible). The plugin was calling `ProcessBuilder("agent-doc", ...)` which silently fails when the binary isn't in PATH. No error shown because there was zero error handling.

**Fixed `TerminalUtil.kt`:**
- Added `resolveAgentDoc()` — checks `~/bin`, `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin` before falling back to bare `agent-doc`
- Added IDE balloon notifications — success shows "Routed plan.md", failures show the actual error
- Background thread reads process output so the EDT isn't blocked
- Registered `Agent Doc` notification group in `plugin.xml`

**Reinstall the plugin:**
```
agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip
```
Settings > Plugins > Install from Disk > restart IDE.

### 2. Posting images

For this CLI session: just paste or drag a screenshot path. Claude Code can read image files directly — use the file path (e.g., `/tmp/screenshot.png`).

For agent-doc documents: images aren't embedded in the markdown session. Reference them by path or paste the relevant text (stack traces, error messages) directly into the `## User` block, as you did with the IOException.

Does markdown support base64 encoded images? I wonder if I can copy/paste an image as base64.

### 3. Signed plugin always builds now

Updated `build.gradle.kts`:
- `signPlugin` reads certificate from `certificate/chain.crt` and `certificate/private.pem` files directly (env var fallback still works for CI)
- `signPlugin` task depends on `buildPlugin` — running `./gradlew signPlugin` always builds first, then signs

Single command to build + sign:
```bash
cd agent-doc-jetbrains && ./gradlew signPlugin
```
Thank you
## User

I got the Routed plan.md
But the session did not receive.

I keep on getting this request...can we change how we assign the timestamp to avoid this constant request in Claude

cd /home/brian/work/btakita/agent-loop && git add -f tasks/plan.md && git commit -m "agent-doc: $(date '+%Y-%m-%d
%H:%M:%S')" --no-verify 2>&1 || echo "nothing to commit"
Commit plan.md before responding

Command contains $() command substitution

Do you want to proceed?
❯ 1. Yes
2. No

## Assistant

### 1. Route fix — registered this session

The "Routed plan.md" balloon was working, but the command went to pane `%85` (an auto-started Claude session) instead of THIS session at pane `%73`. This happened because you started Claude manually, not via `agent-doc start`, so there was no registry entry.

**Fixed:** Updated `sessions.json` to point to `%73` (this pane). Next `Ctrl+Shift+Alt+A` should work.

**UX gap identified:** We need an `agent-doc register <file>` subcommand to register an existing Claude session's pane without having to edit JSON manually. Added to Outstanding.

### 2. Base64 images in markdown

Yes, markdown supports it: `![alt](data:image/png;base64,...)`. But the base64 string for a screenshot is typically 50-200KB of text — it would bloat the document badly. Not practical for agent-doc sessions.

Better options for sharing images:
- Reference by path: `See /tmp/screenshot.png`
- In the CLI: just provide the file path and Claude can read it
- Paste error text directly (as you've been doing — this is the best approach)

### 3. `$()` confirmation prompt — fixed

The `$(date ...)` command substitution triggers Claude Code's security prompt every time. I'll stop using that pattern. Going forward, agent-doc commits will use a pre-computed timestamp variable to avoid the prompt.

### 4. Outstanding items added

- `agent-doc register <file>` — register current tmux pane for a document without starting a new Claude session

## User

Saw a tmux window open with this error:

/home/brian/work/btakita/agent-loop/agent-doc/target/debug/agent-doc start /tmp/.tmp0vnmJ7/test.md
mise WARN  Current directory does not exist or is not accessible: /tmp/.tmp0vnmJ7
mise WARN  Current directory does not exist or is not accessible: /tmp/.tmp0vnmJ7
mise WARN  Current directory does not exist or is not accessible: /tmp/.tmp0vnmJ7
 /tmp/.tmp0vnmJ7                                                                                                                                                                                                                      00:24:15
❯ /home/brian/work/btakita/agent-loop/agent-doc/target/debug/agent-doc start /tmp/.tmp0vnmJ7/test.md
Error: file not found: /tmp/.tmp0vnmJ7/test.md
mise WARN  Current directory does not exist or is not accessible: /tmp/.tmp0vnmJ7
 /tmp/.tmp0vnmJ7                                                                                                                                                                                                                      00:24:16
❯

## Assistant

### 1. Stale tmux window from tests

That `/tmp/.tmp0vnmJ7/test.md` error was a side effect from `cargo test`. The integration tests triggered auto-start which created real tmux windows using temp file paths. After the test finished, the temp dir was cleaned up but the tmux window persisted.

**Fix needed:** Integration tests that trigger `route` (which can auto-start) should use `-L` isolated tmux servers so they don't pollute the user's tmux. Added to Outstanding.

Can we fix this now?

Safe to close that tmux window — it's an orphan from the test run.

I did.

### 2. send-keys Enter fix (already applied)

The `send_keys` was sending text + Enter in a single tmux call. Claude Code's TUI wasn't processing the Enter as a submit. Fixed to:
1. Send text with `-l` (literal mode)
2. Wait 50ms
3. Send Enter separately

Rebuilt and installed. Try `Ctrl+Shift+Alt+A` again — should fully work now.

### 3. Bugs fixed this session

| Bug | Root cause | Fix |
|---|---|---|
| Plugin "no such file" | Plugin called `agent-doc-route` (old name) | Fixed `SubmitAction.kt` |
| Plugin silent failure | JetBrains PATH doesn't include `~/bin` | Added `resolveAgentDoc()` with common paths |
| Route to wrong session | Manual Claude sessions not registered | Updated `sessions.json` manually |
| `tmux new-window` crash | Duplicate window index | Added `-a` flag (append) |
| send-keys Enter not submitting | Text + Enter in single call too fast for TUI | Split into literal text + 50ms delay + Enter |
| Test creates orphan tmux windows | Auto-start in tests uses real tmux | TODO: use `-L` isolation in tests |

## User

The Routed ... overlay is a bit invasive. Does Jetbrains support an inline notification.
Like can it be a info bubble near the cursor that is very short? There should be quick feedback and it goes away. Not a bubble in the lower right corner (away from the actual file) that sticks around a bit too long...

## Assistant

### Inline hint — done

Replaced the balloon notification with `HintManager.showInformationHint()` — a small hint that appears right at the cursor position and auto-dismisses in ~2 seconds. Errors still use balloon notifications (they need to persist and be readable).

Plugin rebuilt and signed. Reinstall:
```
agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip
```

### Test orphan fix — done

Added `AGENT_DOC_NO_AUTOSTART=1` env var check in `route.rs`. The two route integration tests now set this var, preventing them from creating real tmux windows. All 21 tests pass.

## User

The workflow is much better now. I would like to figure out how to get prompted, in a lightweight way, the questions & permissions that claude asks for...without having to jump to the terminal window. Is that possible or would require a fragile hack to pull off?

Hold on...the document didn't update. I see the following in the console:


## Assistant

### 1. Workflow improvement — glad it's working!

The route from JetBrains is now: `Ctrl+Shift+Alt+A` → inline hint → command arrives in this session within seconds. That's the target UX.

### 2. Document not updating

The previous submit was triggered by the hotkey but the `## User` block was empty (no new text). Per the skill spec, if there's no diff, I say "nothing changed" in console and don't write to the document. This is correct behavior but confusing UX.

**Possible improvement:** When a submit arrives with an empty `## User` block, append a brief acknowledgment anyway (e.g., "No new content — route confirmed working.") so the document always shows activity. Worth adding to the spec?

### 3. Permission prompts in JetBrains — feasible but fragile today

**What you want:** When Claude asks a question or needs permission (e.g., "Do you want to proceed?"), surface it in JetBrains as a dialog instead of requiring a terminal switch.

**How it could work:**
1. Poll `tmux capture-pane -t <pane> -p` every ~1 second
2. Parse the output for Claude Code's permission patterns (numbered choices, yes/no prompts)
3. Show a JetBrains dialog with the options
4. Send the user's choice back via `tmux send-keys`

**Why it's fragile:**
- Claude Code's TUI output has terminal escape codes — parsing is unreliable
- The prompt format could change between Claude Code versions
- Race conditions between polling and user input

**More robust paths (future):**
- Claude Code exposing a hooks API or MCP events for permission requests
- Claude Code supporting a `--headless` or `--json-events` mode
- A local WebSocket bridge that Claude Code pushes events t

**My recommendation:** Worth prototyping the `tmux capture-pane` approach as a proof-of-concept. If it works well enough for the common cases (yes/no, numbered choices), ship it. Refine later when Claude Code adds better extensibility.

## User

Ok...let's try the recommendation. commit the plugin first though...because it could be a bit risky...but it should work I guess...

What kind of ui would the Jetbrains plugin show? A dialog in the same document bottom right corner could work. But nothing invasive like a popup in the middle of the screen.
Hotkey support would make it worthwhile...because I can use hotkeys to answer the question in the console...though the hotkeys are not always a consistent pattern.

## User

Submit Agent Doc should also save the current markdown file before notifying the claude session.

## Assistant

### 1. Plugin committed

Committed as `37c29ff` — `feat: add agent-doc JetBrains plugin v0.1.0` with all session fixes (TerminalUtil.kt, SubmitAction.kt, plugin.xml, build.gradle.kts) plus agent-doc submodule (send-keys split, new-window append, test isolation).

### 2. Save-before-route — done

Added `FileDocumentManager.getInstance().saveAllDocuments()` to `SubmitAction.kt`. Now `Ctrl+Shift+Alt+A` always flushes unsaved editor changes to disk before calling `agent-doc route`.

### 3. Prompt polling — implemented

**Rust CLI** (`agent-doc prompt <file>`):
- Captures tmux pane content via `tmux capture-pane`
- Strips ANSI escape codes
- Parses for Claude Code's numbered-option prompt pattern
- Returns JSON: `{"active":true,"question":"...","options":[...],"selected":N}` or `{"active":false}`
- 8 unit tests passing

**Answer support** (`agent-doc prompt --answer <N> <file>`):
- Navigates to the selected option via arrow keys
- Sends Enter to confirm

**JetBrains plugin** (`PromptPoller` + `PromptPanel`):
- After each submit, starts polling `agent-doc prompt` every 1.5 seconds
- When a prompt is detected: shows a bottom-anchored bar with the question and numbered buttons
- **Hotkeys:** Press 1-9 to select an option, Esc to dismiss
- Auto-dismisses when the prompt clears (Claude continues processing)
- Non-invasive: anchored to the bottom of the IDE window, not a center popup

**To test:**
1. Reinstall plugin: `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip`
2. Restart IDE
3. Submit a document with `Ctrl+Shift+Alt+A`
4. When Claude asks a permission question, the prompt bar should appear at the bottom
5. Press 1/2/3 or click a button to answer

All 85 agent-doc tests pass (64 unit + 21 integration).

## User

cleanup this doc
