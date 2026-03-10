---
agent_doc_session: d4eed435-cbf3-44ae-a96c-048b0b2e8025
tmux_session: '1'
agent_doc_mode: template
---

## agent-doc Plugin — Summary

**Architecture:** Plugin is a thin layout reporter — 500ms debounce, concurrency guard, layout detection → `agent-doc sync`. All sync intelligence in CLI. Source: `agent-doc/editors/{jetbrains,vscode}/` inside agent-doc submodule.

**JB AI — Dead End:** JB AI plan is a closed ecosystem (Junie, AI Assistant only). No inference API for external tools. Alternatives: direct Anthropic API, Ollama, or separate Claude Code sessions.

### Resolved Bugs

- **Alt+Enter keybinding** — ActionPromoter `promote()` puts Agent Doc first for `.md` files
- **PromptPoller race** — `@Volatile` on 3 cross-thread fields + per-prompt grace period
- **Layout pane swap** — only break registered session panes, not any unwanted pane
- **Panel button overflow** — `maxLabelLen` 45, tooltip for full text, 30% height cap
- **Double-click guard** — `AtomicBoolean` in `SubmitAction`
- **Overeager claiming** — skip files without `session:` UUID in frontmatter
- **3 panes / 2 tabs** — removed plugin dedup cache; CLI FAST_PATH handles dedup
- **Stash failures** — `break_pane` fallback instead of `join-pane`
- **Stale registry** — `register_full()` deduplicates; `resync` prunes stale entries

### Infrastructure

- Global tmux state logging at sync start/end
- Single session per pane enforced at registration
- 142 tests (121 unit + 21 integration)

<!-- agent:exchange -->
Compacted. Merged two bug tables into one list, trimmed architecture/JB AI prose, removed redundant headers. Same info, ~40% fewer lines.
<!-- /agent:exchange -->
