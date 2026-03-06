---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
---



## Status Summary (compacted 2026-03-04)

### Architecture

**tmux-router crate**: Extracted standalone Rust library (`tmux-router` v0.1.1) from agent-doc. Contains `Tmux` struct (30+ tmux operations), `Registry` (persistent key-to-pane mappings), `reconcile()` (2D layout sync algorithm), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback, `prune()` (dead pane removal + dedup). Published to crates.io. 46 tests (unit + integration + proptest). GitHub Pages docs at https://btakita.github.io/tmux-router/.

**agent-doc v0.9.0**: Interactive document sessions with AI agents. Depends on tmux-router. Published to crates.io, PyPI, GitHub Releases.

**Core features:**
- **Session workflow** (`agent-doc run`): diff → send to agent → append response → 3-way merge-safe write → snapshot → git commit
- **Components** (`<!-- agent:name -->...<!-- /agent:name -->`): Bounded, named, re-renderable document regions. Always paired markers (closing marker required for idempotent re-rendering). Nested supported (innermost-first). Parsed in `component.rs`.
- **`agent-doc patch`**: Replace/append/prepend component content. Config-driven modes via `.agent-doc/components.toml`. Shell hooks (`pre_patch` transforms via stdin/stdout, `post_patch` fire-and-forget). Thread-safe project-root resolution via `find_project_root()`.
- **`agent-doc watch`**: Daemon with `notify` crate, PID file, per-file debounce (500ms default), bounded cycle limiting (default 3), convergence detection via content hashing.
- **Comment stripping**: `strip_comments()` in diff.rs — strips `<!-- -->` and `[//]: # (...)` but preserves component markers. Comments are user scratchpad, never trigger responses.
- **Auto-resync**: `prune()` runs before route/sync/claim operations. Delegates to `tmux_router::prune()`.

**Tmux routing**: Window-scoped routing. Plugin passes `--window` on all sync/claim/layout ops. `route` auto-starts dead panes via `join-pane` into active window. 2D sync (`agent-doc sync --col`) with ATTACH-first reconciliation order.

**Plugin (JetBrains + VS Code)**: Lives in `agent-doc/editors/`. Plugin detects editor splits via `LayoutDetector`, sends `sync --col --focus <active-file>`. Inline hints at cursor for sync notifications.

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

Component names support colon-separated namespacing: `agent:type:scope` (e.g., `agent:log:my-section`). Parser change: `is_valid_name()` regex from `[a-zA-Z0-9][a-zA-Z0-9-]*` to `[a-zA-Z0-9][a-zA-Z0-9:-]*`.

**Three naming layers:**
1. **Simple**: `agent:status` — single component
2. **Namespaced**: `agent:log:my-section` — full string is the name, CLI treats it opaquely
3. **Capture semantics** — skill-level intelligence linking `capture:X` ↔ `log:X`

**Chosen pattern: Sibling capture + log** — capture and log are siblings linked by shared scope name (not nesting). Most flexible: log can live anywhere in the document.

```markdown
<!-- agent:capture:my-section -->
<!-- agent:status -->42<!-- /agent:status -->
<!-- /agent:capture:my-section -->

<!-- agent:log:my-section -->...<!-- /agent:log:my-section -->
```

**Implementation:** v0.9.1 parser allows colons in names (CLI stays dumb). Capture/log relationship lives in skill workflow only.

### Domain Ontology (overlay of base kernel)

| Term | Derives From | Description |
|------|-------------|-------------|
| **Session** | project + story | Bounded interaction with temporal arc; unit of agent-doc work |
| **Document** | entity + context | Markdown file holding conversational state |
| **Component** | scope + abstraction | Bounded, named, re-renderable region (`<!-- agent:name -->...<!-- /agent:name -->`). Configurable mode and shell hooks. |
| **Project** | system + scope | Bounded working context; identified by `.agent-doc/` at root. Contains documents, registry, snapshots, daemon. |
| **Pane** | focus + scope | Tmux viewport — finite attention applied to a single document |

### Release History

| Version | Key Changes |
|---------|-------------|
| v0.5.5–v0.5.6 | Window-scoped routing, commit-doc linking |
| v0.6.0–v0.6.6 | 2D sync, Junie backend, LayoutDetector, `--focus` |
| v0.7.0–v0.7.2 | Snapshot-diff-apply sync, convergent reconciliation, ATTACH-first order |
| v0.8.0 | tmux-router extraction (-3133 lines), frontmatter rename to `agent_doc_session` |
| v0.8.1 | Auto-resync via `tmux_router::prune()` |
| v0.9.0 | **Components**, `patch` (modes + hooks), `watch` daemon, comment stripping. README updated with component/dashboard docs. 150 tests. |

### Documentation

- `docs/guide/components.md` — Component reference (syntax, config, hooks, nesting)
- `docs/guide/dashboard.md` — Dashboard-as-document tutorial (end-to-end flow, multi-component, loop prevention)
- `docs/guide/commands.md` — All CLI commands including `patch` and `watch`
- `README.md` — Components and Dashboard-as-Document sections added

### Pending / Not Built

- **v0.9.1**: Allow colons in component names (parser change)
- `TmuxBatch` — command batching to reduce flicker
- `agent-doc deep` — fan-out parallel subagents
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend (replace junie bridge)
- tmux-router CLI binary — `tmux-router sync`, `tmux-router register`, etc.
- `agent-doc init --dashboard` — scaffold standard template with log + status components
- Capture/log skill workflow — prompt extraction from `capture:X` to `log:X`

### Key Lessons

1. **Always paired markers** — single markers unworkable for re-rendering (no boundary after first render)
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD. Thread-safe for parallel tests.
3. **Component terminology** — "component" (not "range template") aligns with web components/React
4. **CLI is dumb, skill is smart** — `patch` replaces content; prompt extraction lives in skill workflow
5. **ATTACH before DETACH** — join new panes first, then stash unwanted
6. **Log every tmux command** — essential for debugging multi-step operations
7. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc

## User

Commit the agent-doc README

## Assistant

Done. Committed and pushed the agent-doc README (`e084ff3`) with components, dashboard, and patch/watch CLI docs. Updated the parent submodule pointer (`6c8b825`) and pushed both.

## User

Commit agent-doc pane-group

## Assistant

Done. Committed and pushed `specs/pane-groups.md` frontmatter update (`ffb5c53`) in agent-doc. Updated parent submodule pointer (`921a898`) and pushed both.

## User


Please look at the current tmux pane layout being out of sync with the editor layout. Create an automated test for what happened and fix.

## Assistant

**Root cause**: `stash_pane()` in tmux-router uses `join_pane` to move unwanted panes into the stash window. When the stash window is too small (4 panes in 15 rows), tmux rejects with "pane too small" and the error propagated — leaving unwanted panes in the target window.

**Fix** (`tmux-router/src/tmux.rs:455-461`): Added fallback to `break_pane` when `join_pane` fails in `stash_pane()`. The pane gets its own window instead of being stuck in the target.

**Test** (`tmux-router/src/sync.rs:test_reconcile_stash_full_falls_back_to_break`): Creates a cramped stash window (4 rows, pre-filled with splits), then reconciles from 4 panes down to 2. Verifies the evicted panes survive via break_pane fallback and the target window has exactly the desired panes.

**Results**: 47/47 tmux-router tests pass. Rebuilt and installed agent-doc — live sync now works correctly (2 panes matching editor layout).

## User

Do we need to have a window? What if we just detach the inactive panes?

## Assistant

The stash window exists purely for cosmetics — grouping idle panes into one window so `tmux list-windows` isn't cluttered. But it introduces complexity and caused this bug.

**Tradeoffs:**
- **Stash window**: Clean window list, but size limits and this "pane too small" bug
- **break_pane only**: Simple, always works, no size limits. Each detached pane gets a solo window (more window clutter)

Reconcile already handles re-joining panes from any window, so scattered solo windows don't affect correctness. We could simplify by removing the stash entirely and always using `break_pane`. Want me to do that, or keep the current fix (stash with break_pane fallback)?

## User

I don't want to see extra windows in the session.
It would be better to hide the stashed panes.

## Assistant

Updated the fix to keep panes in the stash window instead of breaking them out. The root cause was that `stash_pane()` tried to join into the stash window but it was too small (15 rows, 4 existing panes). tmux enforces per-pane minimum heights that vary by content.

**Fix** (`tmux-router/src/tmux.rs:stash_pane`): Before each join, resize the stash window to 200 rows. Since the stash is never displayed, this costs nothing. Falls back to `break_pane` only if the join still fails after resize.

**Verified live**: Sync from 4 panes to 2 — both evicted panes go into the stash (now 6 panes), no extra solo windows. 47/47 tests pass.

Also discovered the `.bin/agent-doc` symlink points to the local `target/release/` build, not `~/.cargo/bin/`. Used `cargo build --release` instead of `cargo install`.

## User

release tmux-router and agent-doc

## Assistant

Released both:

**tmux-router v0.1.2** — [crates.io](https://crates.io/crates/tmux-router/0.1.2) | [GitHub](https://github.com/btakita/tmux-router/releases/tag/v0.1.2)
- Fix: resize stash window before join to prevent "pane too small"
- New test: `test_reconcile_stash_full_falls_back_to_break`

**agent-doc v0.9.1** — [crates.io](https://crates.io/crates/agent-doc/0.9.1) | [PyPI](https://pypi.org/project/agent-doc/0.9.1/) | [GitHub](https://github.com/btakita/agent-doc/releases/tag/v0.9.1)
- Bumps tmux-router to 0.1.2 for stash fix

Parent submodule pointers updated and pushed.

## User

Let's release the jetbrains plugin to the store. Should the plugin install agent-doc and ensure that the latest version is being used?

## Assistant

### Publishing the JetBrains Plugin

The plugin (`agent-doc-jetbrains` v0.1.0) is fully built and signed. To publish:

1. **Get a JetBrains Marketplace token** — sign in at https://plugins.jetbrains.com/ > Hub profile > Authentication > create a permanent token
2. Run: `PUBLISH_TOKEN=<token> ./gradlew publishPlugin` from `agent-doc/editors/jetbrains/`

JetBrains review takes 1-2 business days for first submission.

### Should the plugin install/update agent-doc?

**Recommendation: No auto-install.** Detect and warn instead.

**Why not auto-install:**
- CLI installs via `cargo install`, `pip install`, or prebuilt binary — plugin doesn't know user's preference
- Auto-installing binaries from a JetBrains plugin is a security concern (reviewers may reject)
- Version management belongs to user's package manager

**Better approach — version check on startup:**
- On project open, run `agent-doc --version`
- If missing: notification balloon with install instructions (cargo/pip/binary links)
- If outdated: "agent-doc X.Y.Z available, you have A.B.C" with update link
- Store minimum compatible version as a constant in the plugin

Publish as-is (v0.1.0) or add version check first (v0.1.0 with check)?

## User

Is there a way to make agent-doc install the JetBrains plugin without having to go through the JetBrains marketplace?

## Assistant

### Installing Without the Marketplace

Yes, several options:

**1. `agent-doc plugin install` CLI command (best for automation)**
- Download the signed zip from GitHub Releases
- Copy to the IDE's plugins directory (auto-detect from `~/.local/share/JetBrains/*/` or `~/Library/Application Support/JetBrains/*/`)
- Prompt user to restart IDE
- Could also handle updates: `agent-doc plugin update`

**2. Custom Plugin Repository**
Host `updatePlugins.xml` on GitHub Pages. Users add the URL once in `Settings > Plugins > Manage Plugin Repositories`. Plugin then appears in the Plugins tab with auto-updates — no marketplace needed.

**3. Direct zip install**
`Settings > Plugins > Install Plugin from Disk...` — manual, but always works.

**Recommended strategy:**
- Ship plugin zip as a GitHub Release asset alongside the CLI binary
- Add `agent-doc plugin install` command for one-liner setup
- Also publish to Marketplace for discoverability (users who find it there get auto-updates)
- Both channels coexist — marketplace for discovery, CLI for automation

Want me to implement the `agent-doc plugin install` command?

## User

I would need a separate Jetbrains and vs-code plugin install

## Assistant

### `agent-doc plugin` Subcommands

```bash
agent-doc plugin install jetbrains   # Download + install JetBrains plugin
agent-doc plugin install vscode      # Download + install VS Code extension
agent-doc plugin update jetbrains    # Update to latest
agent-doc plugin update vscode
agent-doc plugin list                # Show installed editor plugins + versions
```

**JetBrains install:**
- Download signed zip from GitHub Releases
- Auto-detect IDE plugins dir (`~/.local/share/JetBrains/IntelliJIdea*/plugins/` on Linux, `~/Library/Application Support/JetBrains/IntelliJIdea*/` on macOS)
- If multiple IDEs found, prompt user to choose
- Extract zip to plugins dir
- Print "Restart your IDE to activate"

**VS Code install:**
- Download `.vsix` from GitHub Releases
- Run `code --install-extension <path>` (VS Code CLI handles it)
- No restart needed (VS Code hot-reloads extensions)

**Both:**
- Version check via GitHub Releases API
- Cache check: skip if already at latest version

Want me to implement this? ~200 lines of Rust in a new `src/plugin.rs` module.

## User

Yes...implement. Need to test VSCode integration as well. Does VSCode have the same SPEC and features as the JetBrains plugin?

## Assistant

### VS Code Extension Status — Far Behind

The VS Code extension only has **1 of 9 features** (submit command, ~45 lines). JetBrains has all 9.

| Feature | JetBrains | VS Code |
|---------|-----------|---------|
| Submit | Yes | Yes |
| Claim | Yes | No |
| Sync Layout | Yes | No |
| Tab Sync (auto) | Yes | No |
| Prompt Polling | Yes | No |
| Popup Menu | Yes | No |
| Junie Agent | Yes | No |
| Auto-save | Yes | No |
| Split Detection | Yes | No |

### Two Independent Tasks

1. **`agent-doc plugin install`** — Rust module (~200 lines). Useful now for JetBrains.
2. **VS Code feature parity** — ~500-600 lines TypeScript to match the shared SPEC (`editors/SPEC.md`).

Options:
- **A)** Plugin install first (quick win)
- **B)** VS Code parity first (bigger effort)
- **C)** Both in parallel via subagents

## User

C

## Assistant

Launched both in parallel:

1. **Plugin install** — `src/plugin.rs` + Cargo deps + main.rs wiring. Uses `ureq` for HTTP, `zip` for extraction.
2. **VS Code parity** — Full rewrite of `extension.ts` + `package.json` updates. All 9 SPEC features.

Will report results when both complete.

### Results — Both Complete

**1. `agent-doc plugin install/update/list`** — `src/plugin.rs` (307 lines)
- `agent-doc plugin install jetbrains` — downloads signed zip from GitHub Releases, auto-detects IDE plugins dir, extracts, prompts restart
- `agent-doc plugin install vscode` — downloads `.vsix`, runs `code --install-extension`
- `agent-doc plugin update <editor>` — skips if already at latest version
- `agent-doc plugin list` — shows installed plugins + versions
- Supports aliases: `jb`/`idea`/`code`/`codium`
- Dependencies added: `ureq`, `zip`, `tempfile`
- All 21 tests pass, compiles clean

**2. VS Code Extension — Full Feature Parity** — `extension.ts` (570 lines)
- All 9 SPEC features implemented (was 1/9):

| Feature | Implementation |
|---------|---------------|
| Submit | `Ctrl+Shift+Alt+A` → save + `agent-doc route` |
| Claim | `Ctrl+Shift+Alt+C` → `agent-doc claim --position` with split detection |
| Sync Layout | `Ctrl+Shift+Alt+L` → `agent-doc sync --col` with focus |
| Tab Sync | Auto on tab/editor change, 500ms debounce, signature dedup |
| Prompt Polling | 1.5s interval, `prompt --all`, QuickPick UI, auto-save tracked files |
| Popup Menu | `Alt+Enter` → QuickPick with Run/Claim/Sync |
| Notifications | Status bar hints (auto-dismiss) + error messages (persistent) |
| File Filtering | `.md` only on all actions |
| CLI Resolution | `~/bin`, `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin` |

- `package.json` updated with 4 commands + 4 keybindings + `onLanguage:markdown` activation
- Compiles clean with `tsc`

**Next steps:**
- Build VS Code `.vsix` package for GitHub Release
- Publish JetBrains plugin (needs marketplace token)
- Test both plugins live in their respective editors

## User

release

## Assistant

Released **agent-doc v0.9.2**:

- [crates.io](https://crates.io/crates/agent-doc/0.9.2)
- [PyPI](https://pypi.org/project/agent-doc/0.9.2/)
- [GitHub Release](https://github.com/btakita/agent-doc/releases/tag/v0.9.2) — with 4 assets:
  - `agent-doc-x86_64-linux` (prebuilt binary)
  - `agent-doc-jetbrains-0.1.0-signed.zip`
  - `agent-doc-jetbrains-0.1.0.zip`
  - `agent-doc-0.1.0.vsix` (VS Code extension)

**What's new:**
- `agent-doc plugin install jetbrains|vscode` — download + install editor plugins from GitHub Releases
- `agent-doc plugin update/list` — version check + inventory
- VS Code extension: full 9/9 SPEC feature parity

Parent submodule pointer updated (`a18b4df`). Local binary installed (`~/.cargo/bin/agent-doc` v0.9.2).

## User

Does `agent-doc plugin install vscode` work with vscodium?

## Assistant

Yes. `detect_code_cmd()` in `plugin.rs:192-202` checks for `codium` first — if `codium --version` succeeds, it uses `codium` instead of `code`.

So `agent-doc plugin install vscode` will:
1. Download the `.vsix` from GitHub Releases
2. Detect if `codium` is available (prefers it over `code`)
3. Run `codium --install-extension` or `code --install-extension`

Aliases `vscode`, `code`, `vscodium`, `codium` all work.

## User

Compare agent-doc and tmux-router with https://github.com/njbrake/agent-of-empires

Is there anything we can learn from this project?

## Assistant

### agent-doc/tmux-router vs Agent of Empires (1,008 stars)

Both are Rust + tmux tools for AI agent session management, but with fundamentally different philosophies.

| Aspect | agent-doc + tmux-router | Agent of Empires |
|--------|----------------------|------------------|
| **Philosophy** | Document-centric (markdown is the UI) | Agent-centric (TUI dashboard) |
| **tmux usage** | Library crate for routing/layout | Direct wrapper for session lifecycle |
| **Agent support** | Claude Code | Claude, OpenCode, Mistral Vibe, Codex, Gemini |
| **Isolation** | Per-pane via tmux | Git worktrees + optional Docker |
| **UI** | Editor plugins (JetBrains, VS Code) | Ratatui TUI + CLI |
| **Async** | Sync (sequential per-run) | Tokio async |

**AoE has, we don't:**
1. **Multi-agent backends** — detects/manages Claude, Codex, Gemini, etc.
2. **TUI dashboard** — Ratatui visual session manager with real-time status
Should we adopt a TUI interface to fix some of our syncing issues?
3. **Docker sandboxing** — containerized agent execution
4. **Git worktree isolation** — parallel agents on different branches
5. **Diff viewer** — built-in git change review UI
6. **Profile management** — separate workspaces per project/client

**We have, AoE doesn't:**
1. **Document-as-UI** — markdown file IS the conversation (diff → respond → write)
2. **Component system** — re-renderable regions with config-driven modes/hooks
3. **Editor integration** — JetBrains + VS Code plugins with split detection, tab sync
4. **Prompt polling** — permission prompt forwarding to editor
5. **2D layout sync** — reconcile tmux panes to mirror editor splits
6. **Watch daemon** — auto-submit on file change with loop prevention

### What We Can Learn

1. **Multi-agent backends** — AoE's `agents.rs` auto-detects available agents. Our `agent/` trait could support Codex CLI, Gemini CLI, etc. Low effort, high value.

Let's do it.

2. **Git worktree isolation** — parallel agents on separate branches via `git worktree`. Our planned `agent-doc deep` (fan-out subagents) should use worktrees instead of sharing the working directory.

Ok. How do we manage the fan out task? When will the work fan in? At the end of all of the parallel tasks?

3. **TUI status dashboard** — real-time session monitoring. A lightweight `agent-doc status` could show all active sessions, prompt states, and agent status in a Ratatui TUI.

Yes. But also allow for direct cli interaction with the agent loop.

4. **Docker sandboxing** — optional containerized execution for untrusted agent actions (`agent-doc run --sandboxed`).

Interesting. Let's plan for this. If we can implement, go ahead.

5. **Profile system** — grouping sessions by project/client. Our per-project `.agent-doc/` config could be extended with named profiles.

Is the project effectively already a named profile? Are there sub profiles as well? Like in the case of agent-loop, there are many submodules with different workflows. Would the named profile help with managing the workflows? 

### Key Differentiator

AoE is a **session manager** — it launches agents and monitors them. agent-doc is a **document workflow** — the document IS the interface. These are complementary rather than competing: you could run agent-doc sessions inside AoE-managed tmux panes.

True. What would it take to propertly integrate?
For now let's iterate on the existing tumx-manager.

## User

Add btakita/brookebrodack-site as a submodule


## Assistant

Addressing each annotation:

### TUI for Syncing

A TUI won't fix sync bugs (those are in tmux reconciliation logic), but a **status view** would make problems visible faster. `agent-doc status` showing live pane/file/sync state — you'd spot issues immediately instead of after the fact. Editor plugins stay primary; TUI is for monitoring/debugging.

### Multi-Agent Backends — Starting

The `agent/` trait is ready. Each backend is ~50-100 lines wrapping a CLI: `agent/codex.rs`, `agent/gemini.rs`, etc. Select via frontmatter (`agent: codex`) or flag (`--agent gemini`). Auto-detect available agents via `which`.

### Fan-Out / Fan-In

**Fan-out:** `agent-doc deep <file>` creates N worktrees + parallel agent branches.
**Fan-in — recommended: review checkpoint.** All tasks complete → diff viewer shows changes per branch → user approves/cherry-picks → merge. A `## Deep Status` component updates as subtasks complete.

Options: (A) wait-for-all merge, (B) progressive merge, (C) review checkpoint. C gives human-in-the-loop control.

### TUI with Direct Interaction

`agent-doc status` TUI with two modes:
- **Dashboard** — real-time session table (files, agents, status, prompts)
- **Interactive** — select session → attach to tmux pane or type commands to the agent

### Docker Sandboxing

`agent-doc run --sandboxed`: Docker container with project mounted, agent CLI installed, auth volumes shared. Medium effort — prioritize after multi-agent and worktrees.

### Profiles vs Sub-Profiles

`.agent-doc/` is already a project-level profile. For agent-loop's submodules, **sub-profiles** would help:

```toml
# .agent-doc/config.toml
[profiles.agent-doc]
agent = "claude"
watch = true

[profiles.corky]
agent = "claude"
watch = false
```

Files auto-inherit the matching profile by directory. Override per-document via frontmatter.

### AoE Integration

Simplest path: keep building our tmux-router and borrow AoE's ideas (worktrees, multi-agent, status detection) rather than integrating directly. If needed later, AoE could launch `agent-doc run` as its agent command.

**Priority order:**
1. Multi-agent backends (low effort, high value)
2. Git worktree isolation for `agent-doc deep`
3. `agent-doc status` TUI
4. Sub-profiles
5. Docker sandboxing

Want me to start on #1?

## User

There a 3 tmux panes and 2 editor windows. Please add the functional tests to reproduce this issue and fix.
