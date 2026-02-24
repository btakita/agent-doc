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
