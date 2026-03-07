---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
---



## Status Summary (compacted 2026-03-06)

### Architecture

**tmux-router v0.2.0**: Standalone Rust crate (library + CLI). `Tmux` struct (30+ tmux ops + `capture_pane()` + `send_keys_raw()`), `Registry` (file-path-to-pane mappings), `reconcile()` (2D layout sync), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback, `prune()`. CLI: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`. File-name addressing (registry lookup). 48 tests. Published to crates.io + GitHub Releases.

**agent-doc v0.9.4**: Interactive document sessions with AI agents. Depends on tmux-router 0.2. Published to crates.io, PyPI, GitHub Releases.

**Core features:**
- **Session workflow** (`agent-doc run`): diff → send to agent → append response → 3-way merge-safe write → snapshot → git commit
- **Components** (`<!-- agent:name -->...<!-- /agent:name -->`): Bounded, named, re-renderable document regions. Paired markers, nested supported. Config-driven modes via `.agent-doc/components.toml`.
- **`agent-doc patch`**: Replace/append/prepend component content. Shell hooks (`pre_patch` stdin/stdout, `post_patch` fire-and-forget). Thread-safe project-root resolution.
- **`agent-doc watch`**: Daemon with `notify` crate, PID file, per-file debounce (500ms), bounded cycle limiting (default 3), convergence detection.
- **`agent-doc plugin install`**: Download + install JetBrains/VS Code plugins from GitHub Releases. Auto-detect IDE dirs. Aliases: `jb`/`idea`/`code`/`codium`.
- **Comment stripping**: `strip_comments()` — strips `<!-- -->` and `[//]: # (...)` but preserves component markers.
- **Auto-resync**: `prune()` runs before route/sync/claim operations.

**Tmux routing**: Window-scoped. Plugin passes `--window`. `route` auto-starts dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugin (JetBrains + VS Code)**: Lives in `agent-doc/editors/`. Both at 9/9 SPEC feature parity: Submit, Claim, Sync Layout, Tab Sync, Prompt Polling, Popup Menu, Notifications, File Filtering, CLI Resolution.

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

### Release History

| Version | Key Changes |
|---------|-------------|
| v0.5.5–v0.6.6 | Window-scoped routing, 2D sync, Junie backend, LayoutDetector |
| v0.7.0–v0.7.2 | Snapshot-diff-apply sync, ATTACH-first reconciliation |
| v0.8.0 | tmux-router extraction (-3133 lines) |
| v0.9.0 | Components, patch, watch daemon. 150 tests. |
| v0.9.1 | Colons in component names |
| v0.9.2 | `plugin install/update/list`, VS Code 9/9 parity |
| v0.9.3 | tmux-router 0.1.3 (stash window fix for numeric sessions) |
| v0.9.4 | tmux-router 0.2.0 (CLI binary, capture_pane, send_keys_raw) |

### Comparison with Agent of Empires

AoE is a **session manager** (TUI dashboard, multi-agent, Docker, worktrees). agent-doc is a **document workflow** (markdown is the UI). Complementary — could run agent-doc inside AoE-managed panes.

**Ideas borrowed from AoE (priority order):**
1. Multi-agent backends (Codex, Gemini CLI via `agent/` trait) — low effort, high value
2. Git worktree isolation for `agent-doc deep` fan-out
3. `agent-doc status` TUI (dashboard + interactive modes)
4. Sub-profiles per submodule in `.agent-doc/config.toml`
5. Docker sandboxing (`agent-doc run --sandboxed`)

### Pending / Not Built

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

### Key Lessons

1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"

## User



## Assistant

Compacted. Reduced from 567 lines to ~105 lines.

**What was preserved:**
- Full architecture summary (updated to v0.9.4 + tmux-router v0.2.0)
- Component config reference
- Domain ontology
- Release history (added v0.9.1–v0.9.4)
- AoE comparison and borrowed ideas (priority-ordered)
- Pending/not-built list (updated: tmux-router CLI moved to done)
- Key lessons (added stash window fixes)

**What was folded in:**
- All resolved Q&A threads (submodule conversion, stash window bugs, release steps, plugin publishing, AoE comparison annotations)
- Bug reports that resulted in fixes (now captured as key lessons)
- Release log details (compressed to version table)

## User

I just did agent-doc run on this file.
Nothing happened. Please analyze why not.
Note the most recent release.

I'm going to explicitly claim this file now. So this current run will work.

Please analyze agent-doc and tmux-router for any race conditions.

## Assistant

### Most Recent Release

- **agent-doc v0.9.5** (installed), **tmux-router v0.2.1** — both added `RegistryLock` (flock-based advisory locking via `fs2`) and `validate_file_claim` (stale claim auto-pruning).

### Why the Previous Run Did Nothing

Two compounding issues:

**1. No diff detected (primary cause).** The snapshot at `.agent-doc/snapshots/6242da3...md` ends with `## User\n\n` (empty user block, line 126-128). The document at the time of the previous run also ended with `## User\n\n` — the same empty block left after the compaction. `diff::compute()` stripped comments from both, compared them, found no changes, and the skill stopped with "nothing changed." The user's new text hadn't been written yet at that point.

I did make changes. So I'm curious why there is no diff.

**2. Possible claim staleness (secondary).** If the Claude Code session in pane %0 was restarted between compaction and the failed run, the session-to-pane mapping in `sessions.json` could have been pointing at a stale PID. The `resync::prune()` at the top of `route::run()` only prunes entries with *dead panes* — if the pane itself was still alive (same pane %0, new process), the entry would survive pruning even though the new Claude session had no SKILL.md loaded or no active conversation context. The `/agent-doc tasks/agent-doc.md` command would have been sent via `tmux send-keys` to a pane that received it as raw text input but didn't have the skill registered to process it.

**Root cause:** The empty `## User` block after compaction created a state where the document matched the snapshot. The skill correctly found no diff, but "nothing happened" is confusing UX. A better behavior would be to at least print "No changes detected since last submit" to the console.

### Race Condition Analysis

#### What v0.2.1/v0.9.5 Fixed

`RegistryLock` (flock) now protects the `sessions.json` read-modify-write cycle. Functions using it:
- `register_full()` in `sessions.rs` — acquire lock, load, mutate, save
- `prune()` in `registry.rs` — acquire lock, load, filter, save
- `update_window_for_entry()` — acquire lock, load, mutate, save
- `with_registry()` / `with_registry_val()` — transactional helpers in tmux-router

#### Remaining Race Conditions

**1. Snapshot read-modify-write (NOT protected)**

`snapshot.rs` has no locking. `snapshot::load()` and `snapshot::save()` use plain `fs::read_to_string` / `fs::write`. If two subagents process the same document concurrently (e.g., rapid re-submit while a subagent is mid-cycle), this sequence is possible:

```
Subagent A: reads snapshot → computes diff → responds
Subagent B: reads snapshot → computes diff → responds
Subagent A: writes snapshot (with A's response appended)
Subagent B: writes snapshot (with B's response appended, overwrites A's)
```

The document file itself has the 3-way merge safety net, but the snapshot does not. After this race, the next diff will re-detect A's response as "new" content.

**Severity:** Low in practice — the SKILL.md subagent model serializes processing per-pane (only one Claude session per pane). Would become a real issue with `agent-doc deep` (parallel subagents) or `agent-doc watch` (daemon auto-submit).

**Fix:** Add `SnapshotLock` analogous to `RegistryLock`, or use the same lock file.

Yes. Please implement.

**2. `validate_file_claim` double-lock potential**

In `claim.rs`, `validate_file_claim()` acquires `RegistryLock`, then later `register_full()` also acquires `RegistryLock`. Since `validate_file_claim` drops its lock before `register_full` runs (it returns, releasing the RAII guard), this is safe. BUT there's a TOCTOU window between them:

```
validate_file_claim: acquire lock → load → remove stale → save → release lock
                                                                     ↕ (window)
register_full:       acquire lock → load → insert new → save → release lock
```

Another process could register for the same pane in this gap. In practice this is benign — `register_full` already deduplicates stale entries pointing to the same pane. But if another process *claims the file for a different pane* in the gap, the stale removal in `validate_file_claim` was wasted work.

**Severity:** Negligible. Self-healing.

**3. `sessions.rs` load/save without lock**

The free functions `sessions::load()` and `sessions::save()` do NOT acquire `RegistryLock` internally — they're raw I/O helpers. Callers must remember to acquire the lock. `register_full()` does this correctly, but `validate_file_claim` calls `sessions::save()` while holding a separately-acquired lock, which works but is fragile — if someone later calls `save()` without a lock, there's no compiler-enforced protection.

**Recommendation:** Make `load()` and `save()` private (or `pub(crate)`) and expose only `with_registry()` as the public API, forcing all mutations through the lock.

Implement.

**4. `resync::prune()` calls `tmux_router::prune()` which acquires its own lock**

This is correct but worth noting: if `prune()` is called from within a context that already holds `RegistryLock`, it will deadlock (flock is not reentrant on the same thread on all platforms). Currently this doesn't happen because `prune()` is always called before lock acquisition, but it's a latent hazard.

Should there be a panic or a noop with a warning?

**5. Watch daemon concurrent file access**

`watch.rs` uses per-file debounce (500ms) and cycle limiting, which prevents most races. But if the user saves the file at exactly the moment the daemon is writing back the agent response, `fs::write` in `submit.rs` (line 113) could clobber the user's save. The 3-way merge mitigates this (re-reads the file before writing), but there's still a micro-window between the re-read (line 103) and the write (line 113).

**Severity:** Low. Editor auto-save intervals are typically longer than this window.

Can a lock protect against this?

#### Summary Table

| Race | Protected? | Severity | Fix |
|------|-----------|----------|-----|
| Registry read-modify-write | Yes (flock) | N/A | Done in v0.2.1 |
| Snapshot read-modify-write | No | Low (serial pane model) | Add SnapshotLock |
| claim validate→register gap | Benign TOCTOU | Negligible | Self-healing |
| load()/save() without lock | Caller discipline | Medium (API footgun) | Make private, expose with_registry() |
| Nested lock (prune + caller) | Not reentrant | Latent deadlock | Document or assert |
| Watch daemon write window | 3-way merge | Low | Atomic rename |

## User


