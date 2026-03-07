---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_mode: template
---

# agent-doc

<!-- agent:status -->
**Version:** v0.10.0 | **Tests:** 179 passing | **Dependencies:** tmux-router v0.2.2, instruction-files v0.1
**Published:** crates.io, PyPI, GitHub Releases
**Plugins:** JetBrains + VS Code at 9/9 SPEC feature parity
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**tmux-router v0.2.2**: Standalone Rust crate (library + CLI). `Tmux` struct (30+ tmux ops + `capture_pane()` + `send_keys_raw()`), `Registry` (file-path-to-pane mappings), `reconcile()` (2D layout sync), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback, `prune()`. CLI: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`. File-name addressing (registry lookup). Thread-local `REGISTRY_LOCK_HELD` flag for nested flock deadlock prevention. `acquire_or_skip()` for non-blocking lock attempts. 56 tests. Published to crates.io + GitHub Releases.

**Core features:**
- **Session workflow** (`agent-doc run`): diff → send to agent → append response → 3-way merge-safe write → snapshot → git commit
- **Components** (`<!-- agent:name -->...<!-- /agent:name -->`): Bounded, named, re-renderable document regions. Paired markers, nested supported. Config-driven modes via `.agent-doc/components.toml`.
- **Template mode** (`response_mode: template`): In-place response documents. Patch-block response parser + component applier. Auto-create output component for unmatched content.
- **`agent-doc patch`**: Replace/append/prepend component content. Shell hooks (`pre_patch` stdin/stdout, `post_patch` fire-and-forget). Thread-safe project-root resolution.
- **`agent-doc watch`**: Daemon with `notify` crate, PID file, per-file debounce (500ms), bounded cycle limiting (default 3), convergence detection.
- **`agent-doc write`**: Atomic response write-back with 3-way merge. `--template` flag for template-mode. Durable pending store for compaction recovery.
- **`agent-doc recover`**: Detect and apply orphaned responses from `.agent-doc/pending/<hash>.md`. Auto-detects append vs template mode.
- **`agent-doc template-info`**: JSON output of document template structure for plugin rendering.
- **`agent-doc plugin install`**: Download + install JetBrains/VS Code plugins from GitHub Releases. Auto-detect IDE dirs. Aliases: `jb`/`idea`/`code`/`codium`.
- **Comment stripping**: `strip_comments()` — strips `<!-- -->` and `[//]: # (...)` but preserves component markers.
- **Auto-resync**: `prune()` runs before route/sync/claim operations.
- **Stale snapshot recovery**: `is_stale_snapshot()` in `diff.rs` auto-detects when context compaction interrupted a previous cycle.
- **Diagnostic logging**: `[diff]`, `[submit]`, `[snapshot recovery]` prefixed stderr lines.

**Concurrency safety (v0.9.5+):**
- `SnapshotLock` — flock-based advisory locking for snapshot read/write (`fs2`)
- `RegistryLock` — flock-based advisory locking for `sessions.json` (in tmux-router)
- Atomic rename — `tempfile::NamedTempFile` + `persist()` for document and snapshot writes
- Document advisory lock — `.agent-doc/locks/<hash>.lock` (relocated from sibling files in v0.9.8)
- `sessions::load()`/`save()` — `pub(crate)` to force callers through `with_registry()`

**Tmux routing**: Window-scoped. Plugin passes `--window`. `route` auto-starts dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugin (JetBrains + VS Code)**: Lives in `agent-doc/editors/`. Both at 9/9 SPEC feature parity: Submit, Claim, Sync Layout, Tab Sync, Prompt Polling, Popup Menu, Notifications, File Filtering, CLI Resolution.

**Config**: `~/.config/agent-doc/config.toml` — multi-agent backend support (claude, codex, etc.). Execution mode configurable per-skill via SKILL.md frontmatter (`hybrid | parallel | sequential`).

**Skill auto-upgrade**: SKILL.md bundled in binary via `include_str!`. Step 0 (pre-flight) compares `agent-doc --version` against frontmatter `agent-doc-version`, runs `agent-doc skill install` if newer.

### Race Condition Summary

| Race | Protection | Fix |
|------|-----------|-----|
| Registry read-modify-write | flock (RegistryLock) | v0.2.1 |
| Snapshot read-modify-write | flock (SnapshotLock) | v0.9.5 |
| claim validate→register gap | Self-healing (benign TOCTOU) | N/A |
| load()/save() API footgun | `pub(crate)` visibility | v0.9.5 |
| Nested lock deadlock | Thread-local flag + `acquire_or_skip()` | v0.2.2 |
| Watch daemon write window | Atomic rename + doc advisory lock | v0.9.6 |
| Stale snapshot (context compaction) | `is_stale_snapshot()` auto-recovery | v0.9.6 |
| Compaction mid-write | Durable pending store + `recover` | v0.9.9 |

Full analysis: `agent-doc/docs/reference/race-conditions.md`

### Execution Model (SKILL.md)

| Mode | Behavior | Cost |
|------|----------|------|
| **`hybrid`** (default) | First doc direct, 2nd+ concurrent docs use background subagent | Optimal |
| **`parallel`** | Every `/agent-doc` spawns a background subagent | Higher (~7x) |
| **`sequential`** | Never use subagents, fully sequential | Cheapest |

Key insight: filesystem locking makes the document the shared blackboard — no context duplication needed. Subagents only needed for true parallelism (multi-document concurrent processing).

### Component Config (`.agent-doc/components.toml`)

```toml
[log]
mode = "append"        # replace (default) | append | prepend
timestamp = true       # auto-prefix with ISO timestamp
max_entries = 100      # auto-trim in append/prepend (0 = unlimited)

[status]
mode = "replace"
pre_patch = "scripts/validate.sh"   # stdin: content, stdout: transformed
post_patch = "scripts/notify.sh"    # fire-and-forget, env: COMPONENT, FILE
```

### Namespaced Components & Capture (designed, not built)

Component names support colon-separated namespacing: `agent:type:scope`. Sibling capture + log pattern: `capture:X` ↔ `log:X` linked by shared scope name.

### Domain Ontology (overlay of base kernel)

| Term | Derives From | Description |
|------|-------------|-------------|
| **Session** | project + story | Bounded interaction with temporal arc |
| **Document** | entity + context | Markdown file holding conversational state |
| **Component** | scope + abstraction | Bounded, named, re-renderable region |
| **Project** | system + scope | Bounded working context; identified by `.agent-doc/` at root |
| **Pane** | focus + scope | Tmux viewport — finite attention applied to a single document |

### Comparison with Agent of Empires

AoE is a **session manager** (TUI dashboard, multi-agent, Docker, worktrees). agent-doc is a **document workflow** (markdown is the UI). Complementary — could run agent-doc inside AoE-managed panes.

**Ideas borrowed from AoE (priority order):**
1. Multi-agent backends (Codex, Gemini CLI via `agent/` trait) — low effort, high value
2. Git worktree isolation for `agent-doc deep` fan-out
3. `agent-doc status` TUI (dashboard + interactive modes)
4. Sub-profiles per submodule in `.agent-doc/config.toml`
5. Docker sandboxing (`agent-doc run --sandboxed`)
<!-- /agent:architecture -->

## Release History

<!-- agent:releases -->
| Version | Key Changes |
|---------|-------------|
| v0.5.5-v0.6.6 | Window-scoped routing, 2D sync, Junie backend, LayoutDetector |
| v0.7.0-v0.7.2 | Snapshot-diff-apply sync, ATTACH-first reconciliation |
| v0.8.0 | tmux-router extraction (-3133 lines) |
| v0.9.0 | Components, patch, watch daemon. 150 tests. |
| v0.9.1 | Colons in component names |
| v0.9.2 | `plugin install/update/list`, VS Code 9/9 parity |
| v0.9.3 | tmux-router 0.1.3 (stash window fix for numeric sessions) |
| v0.9.4 | tmux-router 0.2.0 (CLI binary, capture_pane, send_keys_raw) |
| v0.9.5 | RegistryLock, SnapshotLock, flock advisory locking, stale claim auto-pruning |
| v0.9.6 | Atomic rename, doc advisory lock, stale snapshot recovery, diagnostic logging, 26 new tests |
| v0.9.7 | `agent-doc write` command, atomic response write-back with 3-way merge |
| v0.9.8 | Lock relocation to `.agent-doc/locks/<hash>.lock` |
| v0.9.9 | Template-mode, compaction recovery, `agent-doc recover`, `agent-doc template-info` |
| v0.9.10 | Code-span parser fix, template input/output component convention |
| v0.10.0 | Renamed `response_mode` → `agent_doc_mode` with aliases (mode, response_mode) |
<!-- /agent:releases -->

## Pending / Not Built

<!-- agent:pending -->
- Add `input` component to template-mode document convention (user writes in `input`, agent responds in `output`)
- Response diff storage for undo/extract workflows
- `agent-doc undo` — revert last response by applying inverse diff
- `agent-doc extract` — move request+response pair to another document
- `agent-doc convert` — auto-convert append-mode documents to template mode (creates `input` + `output` components)
- Document binary-vs-agent responsibility split in README/CLAUDE.md (and AGENTS.md)
- Fix flaky `recover_append_response` test (parallel race condition)
- Plugin phases 5-7 — component folding, gutter icons, structure view (depends on `template-info`)
- Multi-agent backends (`agent/codex.rs`, `agent/gemini.rs`)
- `agent-doc deep` — fan-out parallel subagents with git worktrees, review checkpoint for fan-in
- `agent-doc status` — Ratatui TUI with dashboard + interactive pane attachment
- Sub-profiles for submodule workflows in `.agent-doc/config.toml`
- Docker sandboxing (`--sandboxed`)
- `TmuxBatch` — command batching to reduce flicker
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend
- `agent-doc init --dashboard` — scaffold template with log + status components
- Capture/log skill workflow — prompt extraction from `capture:X` to `log:X`
- Execution mode in `config.toml` (currently only in SKILL.md frontmatter)
<!-- /agent:pending -->

## Lessons

<!-- agent:lessons -->
1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"
8. **YAGNI for speculative code** — `with_snapshot` was added speculatively, then deleted (no production callers)
9. **Subagents are expensive (~7x)** — use filesystem locking instead when possible; subagents only for true parallelism
10. **Stale snapshots from context compaction** — solved with `is_stale_snapshot()` auto-recovery in `diff.rs`
11. **Durable pending store** — save response before write, clear after success; survives compaction and crashes
12. **Distinct patch prefix** — patch markers vs agent markers prevents confusion between response directives and document components
13. **Skip code spans in parser** — never invent custom escape syntax; follow the universal convention of treating code blocks as literal
<!-- /agent:lessons -->

## Session History

<!-- agent:history -->
**v0.9.7 session:** Lock relocation from sibling files to `.agent-doc/locks/<hash>.lock`. SHA256-hashed paths mirror snapshot scheme.

**v0.9.8 session:** Released lock relocation. Cleaned up stale `tasks/*.agent-doc.lock` files.

**In-place response documents session:** Explored dashboard-as-document concept, TUI editor integration (Ki Editor), designed 7-phase implementation plan. Implemented Phases 1-4: `response_mode: template` frontmatter, `template-info` CLI, patch-block parser, `agent-doc write --template`, auto-create output fallback.

**Compaction recovery session:** Diagnosed write-back failure after context compaction. Implemented durable pending response store (`.agent-doc/pending/<hash>.md`), `agent-doc recover` command, SKILL.md pre-flight integration.

**v0.9.9 release:** Template-mode + compaction recovery. Published to all targets. 192 tests.

**Template conversion session:** Converted this document from append-mode to template-mode. Assessed `agent-doc convert` command feasibility.

**Code-span parser fix:** Fixed `component.rs` to skip markers inside fenced code blocks and inline backtick spans. Released as v0.9.10.

**v0.10.0 session:** Renamed `response_mode` → `agent_doc_mode` (canonical) with `mode` (shorthand) and `response_mode` (deprecated) as aliases. Discussed in-place conversation model and blog post.
<!-- /agent:history -->

## User

<!-- agent:input -->
Let's hold off on the release
~~version bump (minor version?) and release~~
Add documentation. We should write a blog post as well explaining the different response_modes.

Actually, is response_mode the correct name?
The interaction is a bit more than a response_mode. I wonder if there is a clearer name. 
<!-- /agent:input -->

<!-- agent:output -->
### Single conversation component: `exchange`

Yes — a single component is cleaner than separate `input`/`output`. If inline discussion is the primary interaction, splitting creates friction.

**Name candidates considered:**

| Name | Verdict |
|------|---------|
| `exchange` | Balanced, mutual, implies back-and-forth. **Recommended.** |
| `dialog` | Clear but feels append-mode-ish |
| `workspace` | Good spatial feel, but vague |
| `surface` | Novel — "where interaction happens" |
| `canvas` | Overloaded (Obsidian) |
| `pad` | Simple, collaborative (Etherpad vibes) |
| `thread` | Overloaded, implies chronological |
| `prompt` | Too narrow — it's bidirectional |

**`exchange` wins because:**
- Implies mutual interaction (not one-directional)
- Not overloaded by other tools
- Short, clear, descriptive
- Works for both user and agent writing in the same space

```markdown
<!-- agent:exchange -->
User writes here. Agent responds inline. Annotations happen naturally.
<!-- /agent:exchange -->
```

Should we keep input and output as well? There may be legitimate reasons for it.
Although I would imagine that a user could write inside an output component but the user's input would be routed to the input component.

### Keeping input/output alongside exchange

Yes — keep all three as conventions, each serving a different purpose:

| Component | Use case |
|-----------|----------|
| `exchange` | Shared conversation surface. User and agent write in the same space. Best for interactive discussions, annotations, iterative refinement. |
| `input` + `output` | Structured separation. User writes prompts in `input`, agent responds in `output`. Better for formal requests, task delegation, or when you want clear request/response boundaries. |

The routing idea is good: user text written inside `output` gets lifted to `input` on next cycle. This keeps the conversation legible — agent output stays clean, user additions are explicitly tracked.

**When to use which:**
- **`exchange`**: This document. Living discussions where the boundary between request and response blurs.
- **`input`/`output`**: Task documents, dashboards, batch operations. "Do X" → "Done. Here's the result."

Neither is mandatory. The agent-doc binary doesn't enforce conventions — components are just named regions. The SKILL.md and documentation establish patterns.

**Threading: not needed.** The `exchange` component is flat — user annotates, agent responds, content evolves. Git provides the history timeline. Threading adds complexity when the document IS the thread.

Agreed.

**Other components** (status, architecture, releases, etc.) remain agent-managed via patch blocks. Only `exchange` is the shared conversation surface.

I requested that tasks/contabo.md be converted to template mode. But it did not happen. Should I try again?

Yes, try again. The conversion didn't happen because the previous session ran out of context. You can edit `tasks/contabo.md` frontmatter to add `agent_doc_mode: template` and restructure manually, or run `/agent-doc tasks/contabo.md` with a prompt requesting conversion. The claim was just logged (pane %32), so it's ready.

briantakita.me...we can syndicate the blog post.
Let's track this current discussion because it will be interesting to talk about in the blog post.

I want to have a screencast as well.

### Blog post plan

- **Platform**: briantakita.me, syndicated
- **Content**: Document modes (append vs template vs exchange), the design discussion that led here, architectural decisions
- **Source material**: This discussion — the naming evolution (`response_mode` → `agent_doc_mode`), the `exchange` component proposal, the in-place annotation model
- **Screencast**: Demo the workflow — editing a template-mode document, watching agent-doc pick up changes, seeing patch blocks applied in real time
- **Good demo flow**: Create a fresh template document → write in exchange component → submit → show inline response → annotate → re-submit → show git diff timeline
<!-- /agent:output -->
