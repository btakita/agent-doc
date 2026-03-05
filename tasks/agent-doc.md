---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '1'
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
