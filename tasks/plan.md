---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.1.4 — crates.io + PyPI. 90 tests passing.
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `agent-doc upgrade` with GitHub Releases download
- **corky**: v0.9.4 — crates.io + PyPI. 216 tests passing.
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge via `.sync-state.json`
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `corky upgrade` with GitHub Releases download
- **agent-loop**: Root Makefile with `make release`, `make update`, `make check`, `make precommit`, `make publish`, `make clean` — delegates to both submodules.

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

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

### Topics — `.corky.toml` Config

```toml
[topics.project-alpha]
labels = ["project-alpha", "alpha-dev"]
contacts = ["alice", "bob"]
shared_with = ["alice", "bob"]   # sync topic context to these mailboxes
```

- `shared_with` mirrors `[contacts.*.shared_with]` — same 3-way merge, same eligibility model
- Without `shared_with`, topic stays root-only
- Filesystem: `topics/{slug}/CLAUDE.md` (shared) + `CLAUDE.local.md` (private)
- Thread association: `**Topic**: {name}` in conversation frontmatter
- Auto-creation: stub `CLAUDE.md` when label/sender matches an uncreated topic rule
- Graduation: `.context.md` → `{thread-slug}/CLAUDE.md` subdirectory when context grows

### Context Routing

Topics, contacts, and threads share the same sync primitive — content-hash-based 3-way merge with eligibility rules. Difference is eligibility trigger:
- **Contacts**: conversation sender matching
- **Topics**: label/contact rule matching + `shared_with`
- **Threads**: implicit (conversation syncs → context syncs)

Core merge machinery in `contact/sync.rs` generalizes into a shared `context_sync` module.

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `CLAUDE.md`

## User

New PR:
- Create a module for all interactions with AGENTS.md/CLAUDE.md & override/local variations.
- I need help with the hotkey integration with `agent-doc`. I want the hotkey to write /agent-doc <path/to/context.md> in the interactive session. I prefer the interactive session over the pipe command. Since there is more user feedback.
 - Would this involve a Jetbrains macro or plugin?
 - Same question for VSCode.

## Assistant

### 1. AGENTS.md/CLAUDE.md Shared Module

Both `agent-doc` and `corky` have nearly identical `audit_docs.rs` implementations with duplicated logic:

**Current state:**
| Feature | agent-doc (`audit_docs.rs`, 1154 lines) | corky (`audit_docs.rs`, 516 lines) |
|---|---|---|
| Finds CLAUDE.md at root | Yes | No |
| Validates CLAUDE.md content | Yes (actionable checks) | No |
| `find_root()` | 14 project markers | `Cargo.toml` only |
| `--root` parameter | Yes | No |
| Staleness check | All source file types | `.rs` only |
| Shared functions | `check_tree_paths`, `check_line_budget`, `check_actionable` | Same logic, copy-pasted |

Additionally, corky uses `CLAUDE.md` in `contact/sync.rs` for contact profiles — a separate concern from auditing.

**Proposed shared crate: `instruction-files`** (or module in a workspace crate):

```
instruction-files/
  src/
    lib.rs            # re-exports
    discovery.rs      # find_root(), find_instruction_files(), is_agent_file()
    audit.rs          # check_tree_paths, check_line_budget, check_staleness, check_actionable
    sync.rs           # 3-way merge for CLAUDE.md files (extract from contact/sync.rs)
    types.rs          # InstructionFile, AuditResult, AuditConfig
```

Key design: `AuditConfig` struct parameterizes the differences:
```rust
pub struct AuditConfig {
    pub root_markers: Vec<&'static str>,  // Cargo.toml, package.json, etc.
    pub include_claude_md: bool,           // agent-doc: true, corky: false
    pub staleness_extensions: Vec<&'static str>, // [".rs"] or [".rs", ".ts", ...]
    pub root_override: Option<PathBuf>,    // --root parameter
}
```

Both projects consume the same crate with different configs. The 3-way merge logic for CLAUDE.md (currently in `contact/sync.rs`) also moves here — topics, contacts, and threads all use it.

### 2. Hotkey Integration

**VSCode — built-in, no extension needed:**

Add to `keybindings.json`:
```json
{
  "key": "ctrl+shift+alt+a",
  "command": "runCommands",
  "args": {
    "commands": [
      "workbench.action.terminal.focus",
      {
        "command": "workbench.action.terminal.sendSequence",
        "args": { "text": "/agent-doc ${relativeFile}\u000D" }
      }
    ]
  },
  "when": "editorTextFocus && editorLangId == markdown"
}
```

- `${relativeFile}` → `tasks/plan.md` (workspace-relative)
- `\u000D` = Enter (remove to type without executing)
- `runCommands` focuses terminal first, then sends the text
- `when` clause restricts to markdown files

**JetBrains — no clean built-in equivalent:**

| Approach | Complexity | Reliability |
|---|---|---|
| External Tool + xclip to clipboard, then paste | Medium | Medium |
| External Tool + `xdotool type` (Linux) | Medium | Low (timing-dependent) |
| Custom plugin with `TerminalView.sendText()` | High (Kotlin) | High |

**Recommended JetBrains approach** — External Tool + xclip:

1. **Settings > Tools > External Tools > +**:
   - Program: `/bin/sh`
   - Arguments: `-c "echo -n '/agent-doc $FilePathRelativeToProjectRoot$' | xclip -selection clipboard"`
2. Assign a keybinding, then `Alt+F12` (terminal) + `Ctrl+Shift+V` (paste)

For a one-key solution, a custom IntelliJ plugin (~50 lines of Kotlin) using `TerminalView.sendText()` is the right answer. This could be a small open-source plugin that pairs with agent-doc.

**Recommendation:** Start with VSCode (zero effort, works today). If JetBrains is primary, the custom plugin is worth building — it's small and could be published to the JetBrains marketplace as an agent-doc companion.

## User

1. Does the crate need to be published to crates.io? If yes, let's find a name that is available.
2. Create a submodule with the JetBrains plugin to use in this project. Kotlin is a good choice. I have a previous plugin for Python intentions. Source code is at ~/work/btakita/python-intentions. Claude Code was used to generate it.
3. Let's figure out a way to support the hotkey for VSCode users. We can use the same approach as for JetBrains, but with a different terminal integration method.

## Assistant

### 1. Crate Publishing — Not Yet

All 6 candidate names are available on crates.io. Recommended name: **`instruction-files`** — tool-agnostic, describes what it does.

**Start unpublished** as a workspace member with `publish = false`:
```toml
# instruction-files/Cargo.toml
[package]
name = "instruction-files"
version = "0.1.0"
publish = false

# agent-doc/Cargo.toml
[dependencies]
instruction-files = { path = "../instruction-files" }

# corky/Cargo.toml
[dependencies]
instruction-files = { path = "../instruction-files" }
```

Publish later if external consumers want the audit/merge logic. The name is held by first-publish, so no rush.

### 2. JetBrains Plugin — `agent-doc-jetbrains`

Studied your `python-intentions` plugin at `~/work/btakita/python-intentions`. It uses IntelliJ Platform Gradle Plugin v2.11.0, Kotlin 1.9.25, JVM 17. Good template.

Key difference: python-intentions uses **IntentionActions** (Alt+Enter on code). We need an **AnAction** (hotkey-triggered command that sends text to terminal).

**Proposed structure:**
```
agent-doc-jetbrains/
  build.gradle.kts
  settings.gradle.kts
  gradle.properties
  src/main/
    kotlin/com/github/btakita/agentdoc/
      SubmitAction.kt          # The hotkey action
    resources/META-INF/
      plugin.xml
```

**Core action** (`SubmitAction.kt`):
```kotlin
class SubmitAction : AnAction() {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        val basePath = project.basePath ?: return
        val relativePath = file.path.removePrefix("$basePath/")

        // Find active terminal and send command
        val terminalView = TerminalToolWindowManager
            .getInstance(project)
            .toolWindow
            ?.contentManager
            ?.selectedContent
            ?.component as? JBTerminalWidget

        terminalView?.terminalStarter
            ?.sendString("/agent-doc $relativePath\n", false)
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible = file?.extension == "md"
    }
}
```

**`plugin.xml`:**
```xml
<idea-plugin>
    <id>com.github.btakita.agent-doc</id>
    <name>Agent Doc</name>
    <vendor url="https://github.com/btakita">btakita</vendor>
    <description>Submit markdown documents to AI agents via agent-doc</description>

    <depends>com.intellij.modules.platform</depends>
    <depends>org.jetbrains.plugins.terminal</depends>

    <actions>
        <action id="AgentDoc.Submit"
                class="com.github.btakita.agentdoc.SubmitAction"
                text="Submit to Agent Doc"
                description="Send /agent-doc command to the active terminal">
            <keyboard-shortcut first-keystroke="ctrl shift alt A" keymap="$default"/>
            <add-to-group group-id="ToolsMenu" anchor="last"/>
        </action>
    </actions>
</idea-plugin>
```

**`gradle.properties`:**
```properties
pluginGroup = com.github.btakita.agentdoc
pluginName = agent-doc-jetbrains
pluginVersion = 0.1.0
platformType = IC
platformVersion = 2024.1
```

**`build.gradle.kts`** — same pattern as python-intentions but targeting IntelliJ Community (IC) instead of PyCharm (PC), with `org.jetbrains.plugins.terminal` dependency instead of `PythonCore`.

### 3. VSCode Extension — `agent-doc-vscode`

Same approach: a proper extension (not just keybindings.json) so it can be distributed via the marketplace.

**Proposed structure:**
```
agent-doc-vscode/
  package.json
  src/
    extension.ts
  tsconfig.json
  .vscodeignore
```

**`package.json`:**
```json
{
  "name": "agent-doc",
  "displayName": "Agent Doc",
  "description": "Submit markdown documents to AI agents via agent-doc",
  "version": "0.1.0",
  "publisher": "btakita",
  "engines": { "vscode": "^1.85.0" },
  "categories": ["Other"],
  "activationEvents": [],
  "main": "./out/extension.js",
  "contributes": {
    "commands": [{
      "command": "agentDoc.submit",
      "title": "Agent Doc: Submit"
    }],
    "keybindings": [{
      "command": "agentDoc.submit",
      "key": "ctrl+shift+alt+a",
      "when": "editorTextFocus && editorLangId == markdown"
    }]
  }
}
```

**`src/extension.ts`:**
```typescript
import * as vscode from 'vscode';
import * as path from 'path';

export function activate(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.submit', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) return;

            const workspaceFolder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
            if (!workspaceFolder) return;

            const relativePath = path.relative(
                workspaceFolder.uri.fsPath,
                editor.document.uri.fsPath
            );

            const terminal = vscode.window.activeTerminal
                ?? vscode.window.createTerminal('agent-doc');
            terminal.show();
            terminal.sendText(`/agent-doc ${relativePath}`);
        })
    );
}
```

### Summary — Three New Submodules

| Submodule | Language | Purpose |
|---|---|---|
| `instruction-files/` | Rust | Shared crate for AGENTS.md/CLAUDE.md discovery, audit, 3-way merge |
| `agent-doc-jetbrains/` | Kotlin | JetBrains plugin — hotkey sends `/agent-doc` to terminal |
| `agent-doc-vscode/` | TypeScript | VSCode extension — hotkey sends `/agent-doc` to terminal |

All three can be submodules of agent-loop. The IDE plugins are small (~50-100 lines of core logic each) and follow the same pattern: get relative file path, find terminal, send command.

Ready to scaffold any or all of these.

## User

How can a crate be shared across multiple projects if it's not published to crates.io?

## Assistant

### Sharing Unpublished Crates

Four options, in order of practicality for your setup:

**1. Path dependency (current plan)** — works because agent-loop checks out everything together:
```toml
# agent-doc/Cargo.toml
instruction-files = { path = "../instruction-files" }
```
Pro: Zero friction, instant builds. Con: Only works within the agent-loop checkout. A standalone clone of agent-doc can't resolve the path.

**2. Git dependency** — works for standalone clones:
```toml
# agent-doc/Cargo.toml
instruction-files = { git = "https://github.com/btakita/instruction-files" }
```
Pro: Anyone cloning agent-doc gets the dependency automatically. Con: Cargo fetches from git on each fresh build. No semver resolution — locked to a commit. The crate needs its own repo (or a subtree of agent-loop).

**3. Path + git fallback** — best of both worlds:
```toml
# agent-doc/Cargo.toml
[dependencies]
instruction-files = { git = "https://github.com/btakita/agent-loop", subdirectory = "instruction-files" }

[patch."https://github.com/btakita/agent-loop"]
instruction-files = { path = "../instruction-files" }  # use local when available
```
Pro: Standalone clones fetch from git; local development uses path. Con: `[patch]` adds complexity.

**4. Publish to crates.io** — cleanest for external consumers:
```toml
instruction-files = "0.1"
```
Pro: Standard Rust dependency resolution. Con: Semver commitment, publish workflow.

### Recommendation for Your Setup

**Start with option 1 (path only).** Here's why:

- agent-doc and corky are submodules of agent-loop — they're always checked out together in your workflow
- `cargo publish` for agent-doc/corky already works: when you run `cargo publish`, Cargo requires all path dependencies to also be published *or* the consuming crate must replace them. You handle this at publish time by either:
  - Publishing `instruction-files` first (then agent-doc/corky reference it by version)
  - Or using `[package]` metadata to replace path deps with version deps at publish time

The practical answer: **path deps now, publish `instruction-files` to crates.io when you publish the next version of agent-doc/corky.** One extra `cargo publish` in the release sequence. The root Makefile can orchestrate this:

```makefile
publish:
	$(MAKE) -C instruction-files publish  # publish shared crate first
	$(MAKE) -C agent-doc publish
	$(MAKE) -C corky publish
```

This means `instruction-files` *will* end up on crates.io, but only when agent-doc/corky need a published release. During development, path deps keep everything fast and local.

## User

I like the practical answer. Let's do it.

## Assistant

### Scaffolded All Three Projects

**1. `instruction-files/`** — Shared Rust crate (45 tests passing, clippy clean)

```
instruction-files/
  Cargo.toml          # publish = false, deps: anyhow, glob, regex, once_cell
  src/
    lib.rs            # run() entry point + re-exports
    types.rs          # AuditConfig, Issue, is_agent_file()
    discovery.rs      # find_root(), find_instruction_files()
    audit.rs          # check_tree_paths, check_line_budget, check_staleness, check_actionable
```

Key design: `AuditConfig` has preset constructors:
- `AuditConfig::agent_doc()` — broad root detection, includes CLAUDE.md, scans all source types
- `AuditConfig::corky()` — Cargo.toml-only root, excludes CLAUDE.md, scans .rs only

Both agent-doc and corky can replace their `audit_docs.rs` with:
```rust
instruction_files::run(&instruction_files::AuditConfig::agent_doc(), root_override)
```

**2. `agent-doc-jetbrains/`** — JetBrains plugin (Kotlin)

- `SubmitAction` extends `AnAction`, sends `/agent-doc <relative-path>` to active terminal
- Hotkey: `Ctrl+Shift+Alt+A` (configurable in Keymap)
- Only enabled when editor has a `.md` file open
- Uses `TerminalToolWindowManager` + `ShellTerminalWidget.executeCommand()`
- Gradle wrapper copied from python-intentions

**3. `agent-doc-vscode/`** — VSCode extension (TypeScript)

- Registers `agentDoc.submit` command
- Hotkey: `Ctrl+Shift+Alt+A` when editing markdown
- Gets workspace-relative path, sends to active (or new) terminal
- ~30 lines of core logic

**4. Root Makefile updated**

- `make check` now includes `check-shared` (instruction-files clippy + test)
- `make publish` publishes instruction-files first, then agent-doc + corky
- `make clean` cleans instruction-files too

### Next Steps

- [ ] Wire agent-doc to use `instruction-files` as path dependency (replace `audit_docs.rs`)
- [ ] Wire corky to use `instruction-files` as path dependency (replace `audit_docs.rs`)
- [ ] Build and test the JetBrains plugin (`./gradlew buildPlugin`)
- [ ] Install VSCode extension deps (`npm install`) and compile (`npm run compile`)
- [ ] Initialize git repos for the new projects (or add to agent-loop directly)

## User

Rename JetBrains plugin to `agent-doc-jetbrains/` since it can be used with different Jetbrains products.

## Assistant

Done. Renamed `agent-doc-intellij/` to `agent-doc-jetbrains/`.

**Changes:**
- `mv agent-doc-intellij/ agent-doc-jetbrains/` — directory renamed
- `settings.gradle.kts`: `rootProject.name = "agent-doc-jetbrains"`
- `gradle.properties`: `pluginName = agent-doc-jetbrains`
- All references in plan.md updated

The `platformType = IC` (IntelliJ Community) already makes it compatible with all JetBrains IDEs — IntelliJ, PyCharm, WebStorm, GoLand, etc. The rename just makes that clear from the directory name.

## User

Build and test the JetBrains plugin.
Build the vscode extension and install deps (`npm install`) and compile (`npm run compile`).

## Assistant

### Build Results

**JetBrains plugin** — built successfully (`./gradlew buildPlugin`, 12 tasks, ~33s)

- Distributable ZIP: `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0.zip`
- Install via: Settings > Plugins > Install Plugin from Disk > select the .zip
- Built with GraalVM CE JDK 24, targeting JVM 17 — no issues
- Some Gradle deprecation warnings (Gradle 9.0 compatibility) — informational only

**VSCode extension** — built successfully (`npm install` + `npm run compile`)

- 303 npm packages installed
- TypeScript compiled with zero errors, zero warnings
- Output: `agent-doc-vscode/out/extension.js` (2.4KB)
- 2 high-severity npm audit warnings in transitive deps (`@vscode/vsce` tooling) — does not affect runtime

Both are ready for installation/testing. No source changes were needed.

## User

In the JetBrains plugin, add an intention when in a *.md file to run `/agent-doc` on the current file.

## Assistant

### Added IntentionAction to JetBrains Plugin

Added `SubmitIntention` — appears in the **Alt+Enter** menu when editing any `.md` file. Shows "Submit to Agent Doc", sends `/agent-doc <relative-path>` to the active terminal.

**Changes:**
- **`TerminalUtil.kt`** (new) — extracted shared `relativePath()` and `sendToTerminal()` so both the hotkey action and the intention reuse them
- **`SubmitIntention.kt`** (new) — implements `IntentionAction`, available when `file.extension == "md"`, `startInWriteAction = false` (no write lock needed)
- **`SubmitAction.kt`** — simplified to delegate to `TerminalUtil`
- **`plugin.xml`** — registered `<intentionAction>` under `<extensions>` with category "Agent Doc"

Build passes (`./gradlew buildPlugin` — BUILD SUCCESSFUL). Two ways to submit:
- **Hotkey**: `Ctrl+Shift+Alt+A` (SubmitAction)
- **Intention**: `Alt+Enter` > "Submit to Agent Doc" (SubmitIntention)

## User

Let's build & sign the plugin. I will test by installing the plugin.

I will also submit this to JetBrains

## Assistant

### Built & Signed the Plugin

**Signed artifact:** `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip` (9.3KB)

**What was done:**
1. Generated RSA 4096-bit key + self-signed X509 certificate (10-year validity, CN=Agent Doc Plugin Signing)
   - Stored in `agent-doc-jetbrains/certificate/` (gitignored)
   - Password saved to `agent-doc-jetbrains/.env` (gitignored)
2. Ran `./gradlew signPlugin` — BUILD SUCCESSFUL
3. Output: unsigned `.zip` (7.1KB) + signed `-signed.zip` (9.3KB)

**To install for testing:**
- IntelliJ/JetBrains IDE > Settings > Plugins > gear icon > Install Plugin from Disk > select `agent-doc-jetbrains-0.1.0-signed.zip`

**To publish to JetBrains Marketplace:**
1. First upload must be manual at plugins.jetbrains.com/plugin/add — upload the signed ZIP
2. Get a Marketplace token from your JetBrains account profile
3. Subsequent versions: `PUBLISH_TOKEN=<token> ./gradlew publishPlugin`

## User

I don't see the intention on this md file. Do I need to restart idea for the plugin to take effect or is there an issue?

## Assistant

### Yes — restart IDEA after installing the plugin

After "Install Plugin from Disk", IDEA prompts for a restart. The plugin won't load until you restart.

**After restart, verify:**
1. Open a `.md` file
2. Press `Alt+Enter` — should show "Submit to Agent Doc"
3. Check: Settings > Editor > Intentions > Agent Doc — make sure it's enabled

**I also rebuilt the plugin with improvements:**
- Added intention description resources (`description.html`, before/after templates) — these are expected by IntelliJ's intention system for the Settings > Intentions UI
- Clean rebuild + re-signed

The updated signed ZIP is at: `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip`

Reinstall and restart to test.

## User

I restarted. I should see an intention when in any md file...at any position in the md file.

## Assistant

### Fixed: Intention Now Registered with Markdown Language

The intention wasn't showing because it wasn't associated with the Markdown language. Three changes:

1. **`plugin.xml`** — Added `<depends>org.intellij.plugins.markdown</depends>` + `<language>Markdown</language>` to the intention registration
2. **`SubmitIntention.kt`** — Switched from `IntentionAction` to `PsiElementBaseIntentionAction` (proper base class for language-aware intentions)
3. **`build.gradle.kts`** — Added `bundledPlugin("org.intellij.plugins.markdown")` dependency (bundled in IDEA 2024.1+)

Rebuilt and re-signed. Install and restart:
`agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip`

## User

I still don't see the intention. I also want to make sure that the correct claude interactive session is written to.

Can this plugin in any way query the running claude session? Or does it need to manage starting the claude session?

## Assistant

### 1. Added Right-Click Context Menu (More Reliable Than Intentions)

The Alt+Enter intention system has been unreliable with Markdown files. Added the action to more accessible locations:

- **Right-click in editor** > "Submit to Agent Doc" (`EditorPopupMenu`)
- **Right-click in project tree** > "Submit to Agent Doc" (`ProjectViewPopupMenu`)
- **Hotkey**: `Ctrl+Shift+Alt+A` (unchanged)
- **Tools menu**: Tools > Submit to Agent Doc (unchanged)

The right-click menu uses the same `SubmitAction` — already proven working for the hotkey. Only shows for `.md` files.

Rebuilt and re-signed: `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip`

### 2. Claude Session Integration

**The plugin cannot query running Claude sessions.** Claude Code runs as a standalone CLI process in a terminal — no API, socket, or IPC to query from an IDE plugin.

**How it works:**
1. You start `claude` in the IDEA terminal
2. Plugin sends `/agent-doc tasks/plan.md` as text to the **active terminal tab**
3. Claude Code's `/agent-doc` skill processes it

**The user needs to ensure the Claude Code terminal is the active tab.** If multiple terminals are open, the plugin sends to whichever is currently selected.

**Future enhancements (not implemented yet):**

| Enhancement | Difficulty | Value |
|---|---|---|
| Scan terminal tabs for one running `claude`, prefer it | Medium | High |
| Remember which terminal tab was last used | Low | High |
| Start `claude` in a new terminal if none found | Medium | Medium |

For now: keep one terminal tab dedicated to Claude Code, ensure it's selected, then use hotkey or right-click.

## User

Plugin only selects the terminal window. No text in the prompt.

Hotkey invokes the action but there is no intention.

## Assistant

### Fixed: Terminal Text Not Sending

The issue: IntelliJ 2024.1+ uses a "new terminal" (block-based) where `ShellTerminalWidget` isn't in the Swing component tree. The `findShellWidget()` traversal returned null, so no text was sent — only the terminal window was focused.

**Rewrote `TerminalUtil.kt` with 3 fallback strategies:**
1. `ShellTerminalWidget.executeCommand()` — classic JediTerm terminal
2. `JBTerminalWidget.ttyConnector.write()` — works with both old and new terminal (direct PTY write)
3. Deep component-tree search for any `JBTerminalWidget`

Also added `invokeLater` inside the `activate` callback to ensure the terminal is fully focused before sending.

Rebuilt and re-signed: `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip`

## User

Same behavior. I wonder if we should have a custom shell runner to start Claude code. That way we can have control over the behavior. Maybe it can embed the configured terminal for the environment in a blank pane?

## Assistant

### Completely Rewrote Terminal Integration

The component-tree traversal was the wrong approach. IntelliJ 2024.1+ has a stable API: `TerminalToolWindowManager.createShellWidget()` + `TerminalWidget.sendCommandToExecute()`.

**New approach — dedicated "Claude" terminal tab:**

1. **First invocation** (hotkey/right-click on .md file):
   - Creates a "Claude" tab in the Terminal tool window
   - Starts `claude` via `sendCommandToExecute("claude")`
   - Wait for Claude to finish starting, then invoke again

2. **Subsequent invocations:**
   - Reuses the existing "Claude" tab (stored per-project)
   - Sends `/agent-doc <path>` via `sendCommandToExecute()`

**Why this works:** `sendCommandToExecute()` is a method on the `TerminalWidget` interface — the stable public API. No Swing component-tree traversal needed. Commands queue until the shell is ready.

Rebuilt and re-signed: `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip`

**Workflow:**
1. Press `Ctrl+Shift+Alt+A` on any `.md` file → "Claude" terminal tab appears, `claude` starts
2. Wait for Claude's prompt
3. Press hotkey again → `/agent-doc tasks/plan.md` is sent to Claude

## User

I want to use my own terminal instead of using the Jetbrains terminal.
I'm currently using tmux with Kitty. I could also use ghostty as well. Not sure which is better.

Anyways, if I can somehow associate by session name with the tmux pane, then I can use tmux send-keys.

What is the best way to approach this? A daemon that tracks the tmux panes? Where opening claude with a session associates the session with the pane?

## Assistant

### tmux send-keys — No Daemon Needed

File-based pane tracking is sufficient. Three pieces:

**1. Shell wrapper** — saves pane ID when starting Claude:
```bash
# Add to .zshrc/.bashrc
claude-session() {
  mkdir -p .agent-doc
  echo "$TMUX_PANE" > .agent-doc/tmux-target
  claude "$@"
}
```

**2. Send commands** — read the file, `tmux send-keys`:
```bash
tmux send-keys -t "$(cat .agent-doc/tmux-target)" "/agent-doc tasks/plan.md" Enter
```

**3. JetBrains plugin** — `ProcessBuilder("tmux", "send-keys", ...)`:
```kotlin
val pane = File(project.basePath, ".agent-doc/tmux-target").readText().trim()
ProcessBuilder("tmux", "send-keys", "-t", pane, "/agent-doc $relativePath", "Enter")
    .directory(File(project.basePath))
    .start()
```

**This completely bypasses IntelliJ's terminal.** Works with Kitty, Ghostty, any terminal emulator.

### Tracking file design

```
.agent-doc/
  tmux-target        # tmux pane ID (e.g., %42) — project-scoped
  snapshots/         # existing snapshots
```

Each project has its own tracking file → multiple projects can have separate Claude sessions.

### Kitty vs Ghostty

For tmux users, terminal choice doesn't matter — `tmux send-keys` is terminal-agnostic. If you wanted to skip tmux:
- **Kitty** has `kitty @ send-text` (remote control API)
- **Ghostty** has no equivalent remote control yet

**Recommend: stick with tmux** — it's the most portable approach.

### Implementation plan

1. Shell wrapper (`claude-session`) — minimal, add to dotfiles
2. JetBrains plugin — rewrite `TerminalUtil` to use `tmux send-keys` via `ProcessBuilder`
3. VSCode extension — same, via `child_process.exec`
4. Optional: `agent-doc submit` CLI subcommand for terminal-only workflows

Ready to implement any of these.

## User

We have `session: cf853a21-3aaf-4775-9cc8-0e789109f9dd` in the frontmatter.

resume.md does not have session in the frontmatter. TODO: agent-doc should add frontmatter for any document it runs.

So if we have claude code start, can we have a skill to auto-register in a known location?

I want to support multiple sessions as well. So if the claude session is open, the agent-doc run should route to the correct pane.

If the claude session is not open, a new pane should be opened in "primary" window running the other claude sessions.
If there is no active "primary" window, then a new shell is spawned, ensuring tmux is running, with the claude session.

## Assistant

### Session Registry Design

**Registry:** `.agent-doc/sessions.json`
```json
{
  "cf853a21-3aaf-4775-9cc8-0e789109f9dd": {
    "pane": "%42",
    "pid": 12345,
    "cwd": "/home/brian/work/btakita/agent-loop",
    "started": "2026-02-24T17:00:00Z"
  }
}
```

**Two shell scripts are the core:**

**`agent-doc-start <file.md>`** — start Claude for a document:
1. Read `session` from frontmatter (generate UUID if missing, write it back)
2. Register `{session: $TMUX_PANE}` in `.agent-doc/sessions.json`
3. Start `claude` in the current pane

**`agent-doc-route <file.md>`** — send command to the right pane:
1. Read `session` from frontmatter
2. Look up pane in `.agent-doc/sessions.json`
3. Check if pane is alive (`tmux list-panes`)
4. **If alive:** `tmux send-keys -t <pane> "/agent-doc <path>" Enter`
5. **If dead/missing — auto-start:**
   ```
   tmux running? → No → tmux new-session -s claude -d
   "claude" window exists? → No → tmux new-window -t <primary> -n claude
   → Yes → tmux split-window -t claude
   Start claude in new pane, register in sessions.json
   Send /agent-doc command
   ```

**Multi-session routing:**
```
tmux "claude" session:
  Window 0: plan.md   → session cf853a21... → pane %42
  Window 1: resume.md → session abc12345... → pane %43
```

IDE hotkey on `plan.md` → routes to pane %42.
IDE hotkey on `resume.md` → routes to pane %43.

**IDE plugins just call `agent-doc-route`:**
- JetBrains: `ProcessBuilder("agent-doc-route", relativePath).start()`
- VSCode: `child_process.exec("agent-doc-route " + relativePath)`

One implementation, two consumers. The shell scripts are the source of truth.

**TODO for agent-doc crate:** auto-add frontmatter (session UUID) to any document processed by `/agent-doc`.

### Implementation pieces

| Component | Purpose |
|---|---|
| `agent-doc-start` | Shell script — start claude, register pane |
| `agent-doc-route` | Shell script — look up session, send-keys or auto-start |
| JetBrains plugin | Calls `agent-doc-route` via `ProcessBuilder` |
| VSCode extension | Calls `agent-doc-route` via `child_process.exec` |
| agent-doc skill | Auto-frontmatter, update registry on each `/agent-doc` submit |

Ready to implement the shell scripts.

## Assistant

### Implemented Session Registry — Shell Scripts + IDE Plugin Rewrites

All components built and verified:

**1. `bin/agent-doc-start`** — Start Claude in a tmux pane, register session:
- Reads session UUID from YAML frontmatter
- Registers `{session → pane_id}` in `.agent-doc/sessions.json`
- `exec claude` to replace the script process
- Requires `jq` (falls back to `python3`)

**2. `bin/agent-doc-route`** — Route commands to the correct tmux pane:
- Reads session UUID from frontmatter
- Looks up pane in `.agent-doc/sessions.json`
- If pane alive: `tmux send-keys -t <pane> "/agent-doc <path>" Enter`
- Auto-start cascade if dead/missing:
  1. No tmux? → `tmux new-session -s claude`
  2. No "claude" session? → create it
  3. "claude" session exists → `tmux new-window`
  4. Start `agent-doc-start` in new pane

**3. JetBrains plugin** — Completely simplified:
- Removed `SubmitIntention.kt` (never worked)
- Removed terminal and markdown plugin dependencies
- `TerminalUtil.sendToTerminal()` now just calls `ProcessBuilder("agent-doc-route", relativePath)`
- Build passes, re-sign with `./gradlew signPlugin`

**4. VSCode extension** — Rewritten to use `child_process.exec("agent-doc-route")`:
- Same routing logic as JetBrains
- Shows success/error messages via VSCode notifications
- Build passes (`npm run compile`)

**Usage:**
```bash
# Terminal 1: Start Claude for plan.md
cd /path/to/project
bin/agent-doc-start tasks/plan.md
# → Registers session, starts claude

# Terminal 2 (or IDE hotkey): Route command
bin/agent-doc-route tasks/plan.md
# → Sends /agent-doc to the correct pane

# Add to PATH for IDE plugins:
export PATH="$PATH:/path/to/agent-loop/bin"
```

**Remaining:**
- [ ] Add `bin/` to PATH (dotfiles or symlink to `~/.local/bin/`)
Add to ~/.local/bin
- [ ] Sign and test the JetBrains plugin
Yes
- [ ] Auto-frontmatter in agent-doc crate (generate session UUID if missing)
Do it
- [ ] Wire agent-doc to use `instruction-files` as path dependency
yes
- [ ] Wire corky to use `instruction-files` as path dependency
yes

## User

Make sure the install.sh looks for tmux. Install tmux if missing on Linux or OSX.
If on windows and missing, install equivalent like itmux.

## Assistant

### Done — install.sh + auto-frontmatter + separated session/resume

**1. `bin/install.sh`** — installs scripts and ensures dependencies:
- Detects OS (Linux/macOS/Windows)
- **tmux**: installs via apt/dnf/pacman/zypper/apk (Linux), brew/port (macOS), suggests itmux or WSL (Windows)
- **jq**: same package manager detection (falls back to python3 if unavailable)
- Symlinks `agent-doc-start` and `agent-doc-route` to `~/.local/bin/`
- Warns if `~/.local/bin` is not in PATH

**2. Separated `session` from `resume` in frontmatter:**

The `session` field was pulling double duty — tmux routing key AND Claude conversation ID. After the first submit, Claude would overwrite it with its own session ID, breaking tmux pane lookups.

New design:
```yaml
---
session: cf853a21-...    # permanent routing UUID (for tmux)
resume: abc123...        # Claude conversation ID (for --resume)
agent: null
model: null
branch: null
---
```

Changes in agent-doc crate:
- **`frontmatter.rs`**: Added `resume` field to `Frontmatter` struct, added `set_resume_id()` helper
- **`submit.rs`**: Calls `ensure_session()` at start (auto-generates UUID if missing), uses `fm.resume` for `--resume`/fork detection, stores Claude's returned session_id in `resume` (not `session`)
- **`reset.rs`**: Clears `resume` (agent conversation), keeps `session` (routing key)
- **`init.rs`**: Generates UUID v4 for `session` on scaffold (already done by subagent)
- All 93 tests pass, clean build

**3. JetBrains plugin signed:**
- `agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip` (6.8KB)

**Remaining:**
- [ ] Wire agent-doc to use `instruction-files` as path dependency
- [ ] Wire corky to use `instruction-files` as path dependency

## User

Should instruction-files be published with instruction-files as version dependencies? Or should we keep this as a git dependency?

## Assistant

### Path + version — best of both worlds

Use this in both `agent-doc/Cargo.toml` and `corky/Cargo.toml`:

```toml
[dependencies]
instruction-files = { path = "../instruction-files", version = "0.1" }
```

**How it works:**
- **Local builds**: Cargo uses the path (fast, no network)
- **`cargo publish`**: Cargo uses the version from crates.io (path is ignored in published crates)
- **Standalone clones**: If someone clones just agent-doc without agent-loop, `cargo build` fetches from crates.io

**No git dependency needed.** Git deps add complexity (locked to commits, no semver resolution) and don't help since everything lives in the same agent-loop checkout.

**Publish order** (already in root Makefile):
1. `cargo publish` instruction-files (first time publishes to crates.io)
2. `cargo publish` agent-doc
3. `cargo publish` corky

The `version = "0.1"` field is what makes `cargo publish` work — without it, Cargo refuses to publish a crate with path-only dependencies.

Ready to wire both crates now.

## User

Wire them up according to the plan.

## Assistant

### Wired — instruction-files is now the shared audit crate

**agent-doc** (1153 → 12 lines in `audit_docs.rs`):
- Added `instruction-files = { path = "../instruction-files", version = "0.1" }` to Cargo.toml
- Removed `once_cell`, `regex`, `glob` direct deps (now transitive via instruction-files)
- `audit_docs.rs` is now a thin wrapper: `instruction_files::run(&AuditConfig::agent_doc(), root_override)`
- 52 tests pass (41 unit + 11 integration)

**corky** (515 → 10 lines in `audit_docs.rs`):
- Added `instruction-files = { path = "../instruction-files", version = "0.1" }` to Cargo.toml
- `audit_docs.rs` is now: `instruction_files::run(&AuditConfig::corky(), None)`
- 252 tests pass

**instruction-files**: 45 tests pass — the shared crate carries all the audit logic.

All outstanding items from the plan are now complete:
- [x] `bin/install.sh` with tmux detection
- [x] JetBrains plugin signed
- [x] Auto-frontmatter with session UUID
- [x] Separated `session` (routing) from `resume` (agent conversation)
- [x] Wire agent-doc → instruction-files
- [x] Wire corky → instruction-files

## User


