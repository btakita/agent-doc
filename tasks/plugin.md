---
agent_doc_session: d4eed435-cbf3-44ae-a96c-048b0b2e8025
tmux_session: '1'
agent_doc_mode: template
---

## agent-doc Plugin — Summary

### Architecture Decision
**Plugin is a thin layout reporter.** All sync intelligence lives in the `agent-doc` CLI. Plugin responsibilities: 500ms debounce, concurrency guard, layout detection → `agent-doc sync`. No caching, no dedup logic.

Canonical source: `agent-doc/editors/jetbrains/` and `agent-doc/editors/vscode/` (inside agent-doc submodule). Old standalone dirs (`agent-doc-jetbrains/`, `agent-doc-vscode/`) deleted.

### JetBrains AI Integration — Dead End
**JB AI plan cannot be used as an LLM backend by external tools.** Investigated ACP (IDE-tools protocol only, zero inference methods), JB MCP Server (exposes IDE tools TO Claude, not JB AI FROM Claude), and direct plugin API (private, undocumented internals). JB AI subscription is a closed ecosystem: Junie, AI Assistant chat, inline completions only.

**Practical alternatives:** (1) Direct Anthropic API backend in `agent/api.rs`, (2) Local models via Ollama, (3) Reserve JB AI for Junie/AI Assistant, Claude Code for agent-doc.

### Resolved Plugin Bugs

| Bug | Root Cause | Fix |
|-----|-----------|-----|
| **Alt+Enter keybinding** | ActionPromoter needed for `.md` files | `promote()` puts Agent Doc first |
| **PromptPoller race condition** | Cross-thread fields without volatile | `@Volatile` on 3 fields, per-prompt grace period |
| **Layout non-session pane swap** | `layout.rs` broke out ANY unwanted pane | Only break registered session panes |
| **PromptPanel button overflow** | 100+ char labels pushed buttons off-screen | `maxLabelLen` 45, tooltip for full text, 30% height cap |
| **Double-click route error** | No concurrency guard on Run Agent Doc | `AtomicBoolean` guard in `SubmitAction` |
| **Overeager .md claiming** | `sync` called `ensure_session()` on all `.md` files | Skip files without `session:` UUID in frontmatter |

### Resolved Sync Bugs

| Bug | Root Cause | Fix |
|-----|-----------|-----|
| **3 panes when 2 tabs open** | Plugin dedup cache blocked re-sync on tab close | Removed plugin-side cache entirely; CLI FAST_PATH handles dedup |
| **Stash pane failures** | `tmux join-pane` fails when stash too small or pane already there | `break_pane` fallback; ANCHOR/JOIN phases use `break_pane` instead of `stash_pane` |
| **Stale registry entries** | Multiple sessions mapped to same pane | `register_full()` deduplicates; `resync` prunes stale entries |

### Sync Infrastructure Added
- **Global tmux state logging** — `log_global_state()` logs `list-panes -a` and `list-windows -a` at sync start/end
- **Single session per pane** — enforced at registration time
- **142 tests** (121 unit + 21 integration) covering: stash recovery, 3→2 pane eviction, crowded stash, dedup registration

## User



<!-- agent:exchange -->
Acknowledged switch to `agent_doc_mode: template`. The document currently uses append-style `## User` sections but has no template components defined yet.

Ready for your next prompt — add content to `<!-- agent:input -->` or `<!-- agent:exchange -->` when you're ready, or let me know if you'd like me to restructure the document with template components.

<!-- /agent:exchange -->
