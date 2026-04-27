> Extracted from SPEC.md — see [index](../SPEC.md)

# Session Routing

## Registry

`sessions.json` maps document session UUIDs to tmux panes:

```json
{
  "cf853a21-...": {
    "pane": "%4",
    "pid": 12345,
    "cwd": "/path/to/project",
    "started": "2026-02-25T21:24:46Z",
    "file": "tasks/plan.md",
    "window": "1"
  }
}
```

Multiple documents can map to the same pane (one Claude session, multiple files). The `window` field (optional) enables window-scoped routing — `claim --window` and `layout --window` use it to filter panes to the correct IDE window.

## Use Cases

| # | Scenario | Command | What Happens |
|---|---|---|---|
| U1 | First session for a document | `agent-doc start plan.md` | Creates tmux pane, launches Claude, registers pane |
| U2 | Submit from JetBrains plugin | Plugin `Ctrl+Shift+Alt+A` | Calls `agent-doc route <file>` → sends to registered pane |
| U3 | Submit from Claude Code | `/agent-doc plan.md` | Skill invocation — diff, respond, write back |
| U4 | Claim file for current session | `/agent-doc claim plan.md` | Skill delegates to `agent-doc claim` → updates sessions.json |
| U5 | Claim after manual Claude start | `/agent-doc claim plan.md` | Fixes stale pane mapping without restarting |
| U6 | Claim multiple files | `/agent-doc claim a.md` then `/agent-doc claim b.md` | Both files route to same pane |
| U7 | Re-claim after reboot | `/agent-doc claim plan.md` | Overrides old pane mapping (last-call-wins) |
| U8 | Pane dies, plugin submits | Plugin `Ctrl+Shift+Alt+A` | `route` detects dead pane → auto-start cascade |
| U9 | Install skill in new project | `agent-doc skill install` | Writes bundled SKILL.md to `.claude/skills/agent-doc/` |
| U10 | Check skill version after upgrade | `agent-doc skill check` | Reports "up to date" or "outdated" |
| U11 | Permission prompt from plugin | PromptPoller polls `prompt --all` | Shows bottom bar with numbered hotkeys in IDE |
| U12 | Claim notification in session | Skill reads `.agent-doc/claims.log` | Prints claim records, truncates log |
| U13 | Clean up dead pane mappings | `agent-doc resync` | Removes stale entries from sessions.json |

## Claim Semantics

`claim` binds a document to a **tmux pane**, not a Claude session. The pane is the routing target — `route` sends keystrokes to the pane. Claude sessions come and go (restart, resume), but the pane persists. If Claude restarts on the same pane, routing still works without re-claiming.

Last-call-wins: any `claim` overwrites the previous mapping for that document's session UUID.

**Prompt-bearing rerun acknowledgment:** When `route` dispatches to an already-running pane and the document already has prompt-bearing user drift on top of a closed cycle, pane-input acceptance is not sufficient proof of success. Route must observe a newer per-document cycle state (`preflight_started` or later) before returning success; otherwise it fails closed instead of silently leaving the prompt stranded in the document.

**Dead fallback session guard:** Route may fall back to the current tmux session or an already-alive harness fallback session, but it must not auto-start a brand-new implicit fallback session such as `"claude"` or `"codex"` just because no explicit target survived resolution. If the only remaining target is a dead implicit fallback name, route fails closed.

**Duplicate-pane guard:** When a document's registry entry is stale but there is still a live pane whose process tree is already running that document, route re-registers and reuses that pane before attempting lazy-claim or auto-start.

## Stash Window Routing

The stash system preserves running Claude sessions when the user switches editor tabs. Panes are moved to a hidden stash window rather than killed, keeping the Claude session alive for later reuse.

**Window-scoped routing:** Each editor split maps to a tmux pane in the primary window (`@0`). When the user switches files, `reconcile()` swaps panes by detaching unwanted ones into the stash and attaching needed ones back.

**Stash lifecycle:**

| Phase | Operation | Detail |
|-------|-----------|--------|
| DETACH | `stash_pane()` | Moves an unwanted pane into the stash window via `tmux join-pane` |
| — | target selection | Targets the LARGEST pane in the stash (by height) to avoid "pane too small" errors |
| — | overflow | If join fails, `break_pane_to_stash()` creates an overflow stash window (also named `"stash"`) |
| ATTACH | `reconcile()` | Joins a stashed pane back into `@0` when needed again |
| RESCUE | `sync` pre-resolution | Rescues same-session stashed panes back to agent-doc window via `swap-pane`/`join-pane` before layout; cross-session stash rescue fails closed and preserves the live pane in place |

**Discovery:** `find_all_stash_windows()` returns all stash windows — both the primary stash and any overflow windows. All windows named `"stash"` or matching `"stash-*"` (tmux auto-deduplication) are treated as stash windows by `is_stash_window_name()`.

**Invariants:**
- Stashed panes keep running — the Claude session remains alive inside
- Stash windows are named `"stash"` for consistent discovery
- The stash window is resized to 200 rows before join operations to prevent minimum-size failures
- Focus never leaves window `@0` during stash operations (`-d` flags are always set)
- Successful route replacements do not kill older stash panes unless the cleanup path has explicit provenance that the older pane is a throwaway artifact from the current recovery attempt

**Commit write contract:** `commit()` always stages a clean snapshot copy (no `(HEAD)`, no stale boundaries) and normalizes the boundary to end-of-exchange. The committed blob and snapshot converge to the same clean shape. The working tree and editor buffer preserve `(HEAD)` annotations so the user can see which response headings are new.

**Snapshot boundary cleanup:** After committing, `commit()` keeps the snapshot in the same clean shape as the committed blob: ALL stale boundaries are stripped, heading-level transient ` (HEAD)` markers are removed, and a single fresh 8-char boundary is inserted at end-of-exchange.

**Working-tree boundary cleanup:** The working tree uses `reposition_boundary_to_end_preserve_head()`, which removes stale boundaries and inserts a fresh one but keeps `(HEAD)` annotations. Preflight classifies `(HEAD)` differences as `boundary_artifact`, so they do not cause false-positive change detection.

**Editor cleanup invariant:** The editor-side reposition helpers (JetBrains and VS Code) must collapse stale boundaries. When the IPC reposition signal includes `preserve_head: true`, the plugin should call `agent_doc_reposition_boundary_to_end_preserve_head_with_id()` for the normal case, but it must not overwrite the just-committed disk response with a stale unsaved buffer that only differs by agent-owned response-heading attribution and/or boundary churn. Boundary-only cleanup must not create a follow-up diff that is only old boundary IDs, and stale harness-label headings such as `codex (HEAD)` must not be written back over the committed model attribution.

**Boundary reposition lifecycle:**
1. **Before IPC patch JSON (clean boundary reposition):** All IPC write paths (`run_ipc`, `try_ipc`, IPC-timeout fallback) read the on-disk document and normalize boundaries in memory before extracting `boundary_id` values. This removes ALL stale boundaries and inserts a single fresh one. The repositioned document is used solely to extract `boundary_id` values — never written to disk. This ensures the `boundary_id` points to end-of-exchange (after the user's prompt), not the stale mid-exchange position.
2. During `agent-doc write`: the `reposition_boundary: true` IPC flag tells the plugin to move the boundary after applying the response patch. The plugin should call `agent_doc_reposition_boundary_to_end()` via FFI to ensure identical cleanup logic.
3. During `agent-doc commit`: (a) the snapshot is cleaned via the clean reposition helper, and (b) a standalone IPC signal with `preserve_head: true` is sent when a live listener exists so the plugin preserves `(HEAD)` in the editor buffer. Without a live listener, the working tree is rewritten with the preserve-head variant.
4. If no plugin is active, the file is rewritten locally with `reposition_boundary_to_end_preserve_head()`, preserving `(HEAD)` annotations while removing stale boundaries.

## Pane Lifecycle — Binding Invariant

**The editor-selected document drives pane resolution. It either finds an existing pane that already claims that document, or provisions a new one. It NEVER commandeers another document's pane.**

This is the **Binding invariant** — the foundational rule of pane management.

### Resolution Path

When the user navigates to a document in the editor:

1. **Sync fires** — JB plugin sends `agent-doc sync --col <file1> --col <file2> --focus <focused_file>`
2. **Initialization** — `ensure_initialized()` runs for each file in `col_args`:
   - If file is empty (no frontmatter, no content) → auto-scaffold as template with frontmatter + exchange component
   - If file has `agent_doc_format` but no `agent_doc_session` → assigns a UUID
   - If no snapshot exists → creates snapshot + `git add` + `git commit`
3. **File resolution** — `resolve_file()` reads frontmatter. Files with `agent_doc_session` → `FileResolution::Registered`. Non-`.md` files or files with content but no frontmatter → `Unmanaged`.
4. **Reconciliation** — `tmux_router::sync` matches the declared layout to tmux panes:
   - Pane exists for this session → **focus it** (Binding found)
   - Pane in stash → **rescue it** (swap-pane back to agent-doc window)
   - No pane exists → trigger **Provisioning**
5. **Provisioning** — `route::provision_pane()` creates a new tmux pane:
   - Splits alongside an existing pane in the agent-doc window
   - Registers the session→pane **Binding** in `sessions.json`
   - Starts Claude asynchronously in the new pane

### Invariants

| Invariant | Enforcement |
|-----------|-------------|
| One document per pane | Registry check in `claim::run()` (line 142-156) |
| Document drives, pane follows | Sync resolves files first, then matches to panes |
| Never commandeer another document's pane | `auto_start` creates new panes; `claim` validates pane isn't already bound |
| Stashed panes stay alive | `join-pane` moves to stash, doesn't kill |
| Initialization is idempotent | `ensure_initialized()` checks snapshot existence first |

### Terminology (Domain Ontology)

| Term | Definition | Module |
|------|-----------|--------|
| **Binding** | Document→pane association in `sessions.json` | `claim.rs`, `sessions.rs` |
| **Reconciliation** | Matching editor layout to tmux layout | `sync.rs` |
| **Provisioning** | Creating a new pane + starting Claude | `route.rs` (`auto_start`) |
| **Initialization** | Assigning UUID + snapshot + git tracking | `snapshot.rs` (`ensure_initialized`) |
