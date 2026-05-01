> Extracted from SPEC.md — see [index](../SPEC.md)

# Commands

## run

`agent-doc [run] <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

1. Compute diff → 2. Resolve document mode from frontmatter (`resolve_mode()`, default template) → 3. Build the matching append/template prompt (diff + full doc) → 4. Branch if `-b` → 5. Send to agent → 6. Durably capture the final parsed response in `.agent-doc/captures/<doc-hash>/<cycle-id>.json` → 7. Apply the response through the matching append/template write path → 8. Update session resume ID → 9. Persist the final post-write cycle state as `write_applied` → 10. Save snapshot → 11. Run the shared strict post-write closeout helper for git-backed runs: commit, require the cycle to be closed, retry once if the snapshot still differs from `HEAD`, then enforce `session-check`

First run prompt wraps full doc in `<document>` tags. Subsequent wraps diff in `<diff>` tags + full doc in `<document>`.

`agent-doc <FILE>` and `agent-doc run <FILE>` are equivalent. Both dispatch by document mode from frontmatter, with template as the default when no explicit format is present.

**Imperative-directive guard:** if the pending user diff contains executable directives like `do #id`, `run tests`, `build + install`, `commit + push`, a one-word approval such as `go`, or natural-language pending-item task text that begins with an imperative verb (for example `[#n8q4] Fix the cross-repo ...`), `run` rejects status-only/meta-only agent replies. The response must include either concrete execution evidence (for example commands, verification/commit sections, or file-path evidence) or a concrete blocker.

**Compaction-directive guard:** if the pending diff contains a bare `compact exchange` request, `run` must not proceed as an ordinary agent response cycle. It fails closed and instructs the caller to use the binary compaction path instead: `agent-doc compact <FILE> --commit` (optionally with `--message ...`).

**Interrupted-run contract:** if `run` writes the final response to disk but stops before the post-write commit finishes, the recorded cycle state must already be `write_applied` with the final file/snapshot hashes. That lets `agent-doc preflight` or `repair` finish the pending commit deterministically instead of misclassifying the cycle as stale `response_captured` drift.

**Cycle-phase monotonicity:** once a cycle records `committed`, later repair/replay bookkeeping for that same cycle must not rewind the persisted cycle-state phase back to `response_captured` or `write_applied`. Lower-rank bookkeeping may be ignored or logged separately, but the on-disk cycle state that powers `session-check` and the Codex Stop hook must remain terminal.

**Run pre-commit boundary:** `run` may still do a user-only pre-commit before contacting the agent, but it must open a fresh `preflight_started` cycle immediately after that pre-commit and before the response write path begins. Otherwise an interrupted run can inherit the older committed state and skip the required `write_applied` recovery surface.

## init

Two modes:

**No-arg (project init):** `agent-doc init` — checks prerequisites, creates `.agent-doc/snapshots/` and `.agent-doc/patches/` directories, and installs `.claude/skills/agent-doc/SKILL.md`. Idempotent. Run once per project before creating session documents.

**With file (document scaffold):** `agent-doc init <FILE> [TITLE] [--agent NAME]` — scaffolds frontmatter + `## User` block. Fails if file already exists. Lazily runs project init first if `.agent-doc/` does not exist.

## install

`agent-doc install [--editor jetbrains|vscode] [--skip-prereqs] [--skip-plugins]` — system-level setup.

1. **Prerequisite check** (unless `--skip-prereqs`): verifies `tmux` and `claude` are on `PATH`; prints ok or MISSING with install hint for each. Does not fail — only warns.
2. **Editor plugin install** (unless `--skip-plugins`):
   - If `--editor` is given, installs only that editor's plugin.
   - Otherwise, auto-detects installed editors: JetBrains (checks `~/.local/share/JetBrains/` on Linux, `/Applications/IntelliJ*` on macOS) and VS Code family (`cursor`, `codium`, `code`).
   - If no editors detected, prints a hint to use `--editor` and exits without error.
   - Calls `crate::plugin::install(editor)` for each detected editor.
   - Prints a summary of installed and failed editors.

## diff

`agent-doc diff <FILE>` — prints unified diff to stdout.

## reset

`agent-doc reset <FILE>` — clears session ID, deletes snapshot.

## clean

`agent-doc clean <FILE>` — squashes all `agent-doc:` commits for file into one via `git reset --soft`.

## gc

`agent-doc gc [--root DIR] [--dry-run]` — garbage-collect orphaned files in `.agent-doc/`.

**What gets cleaned:**

| Category | Criteria | Artifact |
|----------|----------|----------|
| Snapshots, CRDT, pre-response, baselines, annotations | Hash doesn't match any existing `.md` document | File removed |
| Capture ledgers | Hash directory doesn't match any existing document | Directory removed |
| Lock files | Modified >1 hour ago | File removed |
| Hook events | Modified >24 hours ago | File removed |
| Typing indicators | Modified >7 days ago | File removed |
| Status files | Modified >24 hours ago | File removed |
| Repair-blocked diagnostics | Modified >7 days ago | File removed |
| Supervisor sockets | PID dead + socket won't connect | `.sock` file removed |
| Session entries | PID dead + no socket file | Pruned from `sessions.json` |

**Orphaned socket GC algorithm:**

1. Scan `.agent-doc/supervisor/*.sock` files
2. For each socket, extract the session UUID from the filename
3. Find the `sessions.json` entry whose `session_id` matches the socket filename — if the registered supervisor PID is alive, keep the socket
4. If the PID is dead or no matching registry entry exists, try connecting to the socket as fallback
5. If connect succeeds, keep the socket (the supervisor is alive but the registry is stale or was rewritten)
6. If connect fails, remove the socket file and mark the sessions.json entry for pruning
7. Also scan sessions.json for entries whose PID is dead and socket file doesn't exist — prune those entries

Registry pruning acquires `RegistryLock` before modifying `sessions.json`.

**Preflight integration:** `preflight` step 0a runs `gc::run()` automatically at most once per day (controlled by `.agent-doc/gc.stamp`). Orphaned socket cleanup is included in this daily GC pass.

`--dry-run` shows what would be deleted without deleting. `--root DIR` overrides project root auto-detection.

## audit-docs

`agent-doc audit-docs [--root DIR]` — checks CLAUDE.md/AGENTS.md/README.md/SKILL.md for tree path accuracy, line budget (1000), staleness, and actionable content. Exit 1 on issues.

`--root DIR` overrides auto-detection of the project root directory. Without it, the root is resolved via project markers (Cargo.toml, package.json, etc.), then `.git`, then CWD fallback.

When `agent-doc audit-docs` is launched from an outer repo via a nested crate checkout (for example `cargo run --manifest-path src/agent-doc/Cargo.toml -- audit-docs` from a monorepo root), the default scope prefers the running `src/agent-doc` crate root over the outer repo root. That keeps discovery aligned with the crate being audited instead of the caller's larger checkout.

## start

`agent-doc start <FILE>` — start the configured harness in a new tmux pane and register the session.

1. Parse YAML frontmatter. If the frontmatter is malformed, fail closed with a file-targeted error that names the document and tells the user to fix the `--- ... ---` block before retrying.
2. Ensure session UUID in frontmatter (generate if missing)
3. Read `$TMUX_PANE` (must be inside tmux)
4. If another tmux pane still proves live ownership of that same document session, focus and reuse that pane immediately; if it lives in another tmux session, switch the caller's current client to that session before selecting the target window/pane. Once live-owner proof exists, missing or stale supervisor IPC state does not authorize a fresh replacement pane for the same document.
5. If `sessions.json` points at a different **alive** pane but no live owner can still be proven for the document, consult the supervisor before treating it as stale:
   - **Healthy** (supervisor IPC returns `running=true, state="healthy"`) → focus and reuse that pane
   - **Restartable** (supervisor reachable but child not running, or state is degraded) → send `restart` via supervisor IPC, focus pane on success; clear the stale registration only if restart fails
   - **Halted** (supervisor state is `halted` after repeated crashes) → do not revive that supervisor in place; if the session log still shows the registered pane as the latest open run with no later child exit or `session_end`, fail closed instead of replacing it, otherwise clear the crashed registration and start fresh in the current pane
   - **Unreachable** or **No socket** → before any fail-closed/replacement decision, consult the session log's latest open pane provenance. If the log still shows an alive latest-open pane for that session, focus and reuse that pane even when process-tree ownership proof is currently missing. If that same alive pane still owns the active startup-miss marker for the document, fail closed instead of rebinding to a fresh pane. Only when neither live-owner proof nor alive latest-open session-log provenance exists may start treat the registration as stale and continue in the current pane
6. Only when start is falling through to a fresh session in the current pane, auto-relocate that pane into the configured project tmux session if needed. Reuse/focus of an already-running owner must leave the launcher's pane in its original session. If the configured project session is dead and start must keep the current pane in another live tmux session, it must update `.agent-doc/config.toml` so `tmux_session` follows that new live binding instead of the dead session name.
7. Register session → pane in `sessions.json`. If the same session UUID was previously bound to a different pane, the registry write must first append `session_superseded ...` plus `session_end origin=registry_rebind ...` to that session log before the new pane binding lands, so pane replacement leaves explicit provenance instead of two apparently open pane eras in one log. Any later session-log parser that decides whether the previous pane era is still open must treat `session_end` as the closing event by first token, not by exact whole-line match, so recovery metadata after `session_end` still counts as a real closeout.
8. **Validate snapshot integrity** — call `ensure_initialized` before the IPC listener starts. If the file was moved (e.g., JB plugin respawn after rename), migrates orphaned state files from the old path hash to the new one, or bootstraps a fresh snapshot. Prevents CRDT corruption from stale state.
9. Resolve harness args from `agent_args` / harness-specific aliases, then auto-append `--add-dir` entries for any extra writable roots needed by submodule documents: the superproject working tree for parent-repo patchback targets plus any external git metadata directories (including nested child submodule gitdirs under `.git/modules/...` and the superproject `.git` when applicable). Both Claude Code and Codex `exec` accept `--add-dir`; however, `codex exec resume` does not, so the Codex backend strips `--add-dir` entries from resume args (the resumed session inherits writable roots from the original `exec`)
10. Exec the configured harness (replaces process). For Codex clean exits, the supervisor still auto-restarts in resume mode by default, but if a forwarded `Ctrl-D`/stdin-EOF prompt fires immediately after a fresh start (within the early-start grace window) and the follow-up prompt itself sees EOF, treat that as a fresh restart instead of an intentional quit so transient pane-move/input races do not close the claimed tmux pane.

## route

`agent-doc route <FILE> [--pane P]` — route a `/agent-doc` command to the correct tmux pane.

1. Prune stale entries from `sessions.json`
2. Ensure session UUID in frontmatter (generate if missing)
3. Look up pane in `sessions.json`
4. If the registered pane is alive, route first tries **path provenance** from `sessions.json`: the canonical document-path entry must still point at that pane, the pane process tree must still contain the recorded supervisor PID, and the supervisor socket must report the same `supervisor_instance_id`. Only when that top-down proof fails does route fall back to tmux process-tree file-path matching and supervisor-PID recovery. Route only auto-picks a winner when that evidence is decisive: a single provable owner overall, or a single provable owner in the active tmux window while every other candidate is already stashed. Otherwise it fails closed with an explicit ambiguity report that lists every candidate pane plus direct `tmux capture-pane`, `agent-doc claim --pane`, and `tmux kill-pane` follow-up commands. When a winner is found, route must first see the harness's idle prompt in that pane; only then does it send the harness trigger via `send_keys`, run the Enter verification loop (polls for command text disappearance every 300ms, retries Enter on each poll, up to 5s timeout), and focus the pane. For routed dispatch, "idle prompt" means an empty composer: Codex/Claude prompt lines that already contain drafted user text are not safe dispatch targets, even if they still begin with the prompt glyph. Trigger verification must also treat a visually wrapped `agent-doc <FILE>` line as still pending input; route must not declare success merely because the absolute path wrapped across multiple physical tmux lines. Claude routes send `/agent-doc <FILE>`. Codex routes also send only the bare `agent-doc <FILE>` reopen, even when the document still has unresolved prompt-bearing drift on top of a closed cycle. The document diff remains the source of truth for that follow-up work; route must not paste extra prompt lines into the Codex composer because a multiline payload can be interpreted as ordinary chat instead of re-entering the binary-owned document cycle. Route must validate that final Codex payload immediately before `send-keys` and fail closed if it is multiline or otherwise differs from the bare reopen command, rather than surfacing a later ambiguous startup-miss timeout. Prompt-bearing rerun gating is body-only: YAML frontmatter metadata churn such as `agent: codex`, session ids, and model-tier annotations must not be treated as `content_edit` / `prompt_target` work that blocks a routed reopen. The same gate must still catch a plain prose user follow-up appended at the live `agent:exchange` tail immediately before `<!-- /agent:exchange -->`, even when that line has no `❯ ` prefix yet, does not end in `?`, and does not start with an imperative verb; route must not silently downgrade that shape to "already running" just because the prompt classifier missed it. Claude idle detection must treat the newer double-chevron composer chrome (for example `⏵⏵ ... (shift+tab to cycle)`) as prompt-ready state, not as busy output. If the pane never becomes idle and the document still has unresolved prompt-bearing drift, route must run the same scoped repair pass as `agent-doc fix <FILE>` once, then fully re-resolve and retry the route before surfacing a fail-closed busy-session error. That no-op scoped fix must not restart an otherwise healthy same-document supervisor just to "unstick" a busy Codex/Claude pane; healthy authoritative sessions degrade to focus/already-running success, while only restartable/degraded supervisors are eligible for a one-shot in-place restart before retry. If the pane is proven live for the same document and there is no prompt-bearing drift to dispatch, route may focus that already-running session and return success without injecting a duplicate reopen.
5. If the registered pane is alive but no live-owner proof succeeds, route still probes the pane's supervisor health before declaring it stale:
   - **Healthy** supervisor → reuse the registered pane and send the routed command there, but only after the same idle-prompt gate passes
   - **Restartable** (reachable supervisor, child degraded/not running) → send supervisor `restart`, refocus the same pane, and require a fresh routed-cycle ack instead of spawning a second pane
   - **Halted** (reachable supervisor state `halted`) → fail closed with the pane id and restart count; route must not auto-restart that pane or auto-start a replacement on top of the crash loop
   - **Unreachable** or **No socket** → clear the stale registration and continue to lazy-claim / auto-start
6. If pane dead (previously registered) → lazy-claim only to an explicit `--pane P` override, and only if that pane is not already claimed and is running an agent process (`agent-doc`, `claude`, `node`). Route does **not** adopt the tmux session's current active pane implicitly. Non-agent panes (corky, shells, etc.) are skipped — falls through to auto-start. Unregistered files skip lazy-claim entirely.
7. If no active pane available → before any auto-start, consult the document session log for recent synthetic `supervisor_exit code=missing_pane` recoveries. If the same session recorded two or more unexpected pane-loss recoveries inside the last 10 minutes, route must fail closed with a manual-recovery diagnostic instead of spawning yet another replacement pane. Otherwise continue with the normal auto-start cascade (see below), register, wait up to 30s for Claude `❯` prompt via `pane_has_prompt()` with ANSI stripping, then send command
8. If route fails after creating replacement panes, cleanup must be scoped to panes created by that specific route attempt. Panes that appeared concurrently for other documents in the same tmux window are out of scope and must not be killed as inferred orphans.

**Session validation:** If `tmux_session` references a non-existent tmux session, route logs a warning, ignores the stale pin, and resolves a live target from the current tmux session or an already-alive harness fallback session. If the only remaining target is a dead implicit fallback name (for example `"claude"` / `"codex"`), route fails closed instead of creating that session implicitly.

> **Deprecation note:** `tmux_session` in frontmatter is deprecated. The tmux session is now determined at runtime: `--window` argument (sync), `current_tmux_session()` (route/start), or future `.agent-doc/config.toml` settings. The field is still read for backward compatibility and auto-repaired by sync. It will be removed in a future version.

**Auto-start algorithm (`auto_start_in_session`):**
1. **Startup locks:** Acquire flock-backed locks under `.agent-doc/starting/` for both the document hash and the target tmux session (`session-<hash>.lock`). This serializes same-document retries and same-session provisioning races. After the locks are held, re-check the registry; if another route already registered a live pane for this document, reuse it instead of spawning again.
2. Read `tmux_session` from the document's frontmatter (fall back to default `claude` session name)
3. Find a split target pane:
   - **Sync path** (`skip_wait=true`): pick the split target by column position — first pane in the agent-doc window for left-column files, last pane for right-column files. This places the new pane adjacent to its column neighbors.
   - **Route path** (`skip_wait=false`): search `sessions.json` for any registered pane alive in the target session. This pane is a layout anchor only, not a dispatch target for the new document.
4. If found → `tmux split-window` alongside that pane (`-dbh` for left-column, `-dh` for right-column)
5. If split-window fails → fall back to creating a new window
6. If no split target found → create a new window via `tmux new-window` (the session may not exist yet, in which case a new session is created)
7. Any pane created by this auto-start path must enable tmux pane-local `remain-on-exit on` on that pane before the harness launch begins, so later pane death preserves `pane_dead_status` and recent pane output for sync/resync diagnostics even after the pane is moved into stash or rescued back out.
8. After registration, every routed send in this fresh-pane path must still pass the same file-bound dispatch guard as existing-pane routing. Route may split beside another document's pane in the same session, but it must only send the routed command to the newly created pane registered for the target file.

## claim

`agent-doc claim <FILE> [--position left|right|top|bottom] [--window W] [--pane P]` — claim a document for a tmux pane.

1. Ensure session UUID in frontmatter (generate if missing)
2. **Resolve effective window** (see Window Resolution below)
3. Determine pane: `--pane P` overrides, else `--position` resolves via tmux pane geometry, else `$TMUX_PANE`
4. If the resolved pane is in a different tmux session than the configured project session, fail closed unless the configured session is stale or `--force` was passed
5. Register session → pane in `sessions.json`, including window ID

Unlike `start`, does not launch Claude — the caller is already inside a Claude session. `--position` is used by the JetBrains plugin to map editor split positions to tmux panes.

**Binding invariant enforcement:** If the target pane is already claimed by a different session (and the pane is alive), `claim` provisions a new pane for this document instead of erroring. This enforces the Binding invariant (§8.5): "never commandeer another document's pane." Use `--force` to explicitly overwrite the existing claim (discouraged — breaks the Binding invariant unless the old document is abandoned).

**Cross-session claim guard:** `claim` must reject a pane that lives in another tmux session while the configured project session is still alive. The only allowed exceptions are an explicitly stale configured session (post-reboot recovery) or an explicit `--force` override.

**Default components on claim:** For new template documents, `agent-doc claim` scaffolds `<!-- agent:status patch=replace -->` and `<!-- agent:exchange patch=append -->` components by default.

**Window Resolution:**

When `--window W` is provided:

1. Check if window `W` is alive (`tmux list-panes -t W`)
2. If alive → use `W` (no change)
3. If dead → scan `sessions.json` for entries with matching project `cwd` and non-empty `window` field. For each, check liveness. Use first alive match.
4. If no alive windows found → fall through to no-window behavior (position detection without window scoping)

This prevents the JetBrains plugin from hitting persistent error balloons when a tmux window dies. The same fallback pattern is used in `sync.rs` for dead `--window` handling.

**Snapshot initialization:** After registration, saves a snapshot with empty exchange content (via `strip_exchange_content`). This ensures existing user text in the exchange becomes a diff on the next run, rather than being absorbed into the baseline.

**Notifications:**
- `tmux display-message` — 3-second overlay on the target pane showing "Claimed {file} (pane {id})"
- `.agent-doc/claims.log` — appends `Claimed {file} for pane {id}` for deferred display by the SKILL.md workflow on next invocation

## focus

`agent-doc focus <FILE> [--pane P]` — focus the tmux pane for a session document.

1. Read session UUID from file's YAML frontmatter (or use `--pane` override)
2. Look up pane ID in `sessions.json`
3. Run `tmux select-window -t <pane-id>` then `tmux select-pane -t <pane-id>`

Exits with error if the pane is dead or no session is registered.

## layout

`agent-doc layout <FILE>... [--split h|v] [--window W]` — arrange tmux panes to mirror editor split layout.

1. Resolve each file to its session pane via frontmatter → `sessions.json`
2. If `--window` given, filter to panes registered for that window only
3. Pick the target window (the one containing the most wanted panes; tiebreak: most total panes)
4. Break out only registered session panes that aren't wanted (shells and tool panes are left untouched)
5. Join remaining wanted panes into the target window (`tmux join-pane`)
6. Focus the first file's pane (the most recently selected file)

`--split h` (default): horizontal/side-by-side. `--split v`: vertical/stacked. Single file falls back to `focus`. Dead panes and files without sessions are skipped with warnings.

## resync

`agent-doc resync [FILE] [--fix]` — validate sessions.json against live tmux panes.

`agent-doc fix [FILE]` is the canonical fix surface. `agent-doc resync --fix [FILE]` is an alias to the same behavior.

**Always (dry-run and --fix):**
1. Load `sessions.json`, prune entries with dead panes (delegates to `tmux_router::prune()`)
2. Purge idle stash windows: kill `stash`-named windows where all panes run idle shells (`zsh`, `bash`, `sh`, `fish`) and last activity was >30s ago
3. Log orphaned `claude`/`stash` windows (all panes unregistered) for diagnostics

**Scoped mode (`FILE` provided):**
- Resolve `FILE` through the normal cwd/git-root path resolver and require the document to exist.
- Before generic issue detection, gather all provable owner panes for that document. If there is a unique winner, re-register the document to that pane immediately; when the winner is in the active tmux window, redundant unregistered stash panes for the same document are killed during this scoped fix pass. If multiple candidates remain, fail closed with the same actionable ambiguity report used by `route`.
- Limit dead-pane pruning, issue detection, and fix application to registry entries whose stored document path resolves to that same file.
- Do not mutate unrelated documents' registry entries, stash windows, or orphan cleanup state during a scoped run.

**Issue detection (alive panes only):**
4. **Wrong-process:** Pane is running a process not in the allowlist (`agent-doc`, `claude`, `node`) and not an idle shell (`zsh`, `bash`, `sh`, `fish`)
5. **Wrong-session:** Pane is in a different tmux session than the document's `tmux_session` frontmatter field. Skipped if no `file` path or no `tmux_session` in frontmatter. Wrong-process panes are not also checked for wrong-session.
6. **Wrong-window:** Pane is in a different non-stash window from the majority of panes sharing the same tmux session. Majority-window is computed by count; ties broken arbitrarily. Panes already in a stash window (`stash`, `stash-2`, etc.) are excluded from this check.

**Without `--fix`:** Reports issues to stderr with "run with --fix to resolve".

**With `--fix`:**
- Wrong-session panes: kills the pane via `tmux kill-pane`, removes registry entry. Next `route` auto-starts in the correct session. With `--session <name>`: uses `join-pane` to relocate the pane to the named session instead of killing it; registry entry is preserved (pane ID is stable). Falls back to deregister if no active pane found in target session.
- Wrong-process panes: removes registry entry only (does not kill the foreign process). Next `route` auto-starts a new pane.
- Wrong-window panes: moves the pane into the stash window via `stash_pane` (does not deregister). The pane stays alive; the next `sync` or `layout` rejoins it into the correct window.

**Non-agent process guard (route):** `is_agent_process()` gates both the wrong-session recovery path and the lazy-claim path (Strategy 2). A pane running corky, a shell, or any non-agent process is never stashed, rescued, or claimed — agent-doc provisions a fresh pane instead. Prevents foreign processes from being dragged across tmux sessions during route/sync.

**Stash window naming:** Stash windows are named `stash`. When tmux auto-deduplicates a name collision the window becomes `stash-2`, `stash-3`, etc. All names matching `stash` or `stash-*` are treated as stash windows (checked by `is_stash_window_name`). `resync` purges stash windows where all panes are idle shells and last activity was >30s ago.

**Auto-start stash overflow (route):** When `auto_start_in_session` tries `split-window` alongside a registered pane and the split fails (e.g. minimum pane size constraint), it falls back to `tmux new-window` then immediately calls `stash_pane` to move the new pane into the stash window — avoiding a visible throwaway window in the session.

**Automatic pruning:** `resync::prune()` (step 1 only — no issue detection or fixing) runs automatically before `route`, `sync`, and `claim` operations. Uses bulk metadata fetching (2 subprocess calls: `list-windows -a` + `list-panes -a`) instead of per-pane queries. Stranded panes (no valid return target) are deregistered on first failure to prevent repeated expensive lookups. **Stash pane safety:** unregistered agent processes (`agent-doc`, `claude`, `node`) in stash windows are purged only when they are truly orphaned: if the pane still proves ownership of a registered document or still hosts a live supervisor socket, prune must preserve it. That proof is scoped to the pane's own nearest project root as well as the caller's current root, so a stashed sibling-repo pane is not killed merely because the current workspace registry/supervisor set no longer mentions it. Retained dead stash panes (`remain-on-exit` with `pane_dead=1`) are not treated as live ownership and should be reaped once they are unregistered, so preserved crash diagnostics do not become permanent stash clutter. Automatic prune also reaps unregistered retained-dead panes in non-stash windows when another pane remains in that same window; it must not auto-kill the last pane in a window, so a lone dead pane can still be inspected manually. The scoped `fix <FILE>` duplicate-owner recovery path may still kill redundant unregistered stash panes after it has already re-bound the document to a unique provable winner.

## fix

`agent-doc fix [FILE] [--session <name>]` — apply the same fix path as `agent-doc resync --fix`, either globally or for one document.

- Without `FILE`, behavior matches the existing global `resync --fix` workflow, including stash/orphan cleanup and post-fix session listing.
- With `FILE`, behavior is scoped to that document only; unrelated registry entries and stash cleanup are left untouched.
- `--session <name>` preserves the existing `WrongSession` relocate-via-`join-pane` behavior instead of killing the pane.

## prompt

`agent-doc prompt <FILE>` — detect permission prompts from a Claude Code session.

- Captures tmux pane content, strips ANSI, searches bottom-up for footer containing `"to cancel"`
- Supports two option formats: bracket `[N] label` (legacy) and numbered list `N. label` (Claude Code v2.1+)
- Returns JSON: `{ "active": bool, "question": str, "options": [...], "selected": int }`
- `--answer N` navigates to option N via arrow keys and confirms with Enter
- `--all` polls all live sessions, returns JSON array of `PromptAllEntry` objects
- Debug: `AGENT_DOC_PROMPT_DEBUG=1` logs last 5 non-empty lines of each captured pane to stderr

## commit

`agent-doc commit <FILE>` — selective commit with auto-generated timestamp.

1. Load the snapshot for the file (the document state after the last `agent-doc write`)
2. If snapshot exists:
   a. Strip any transient `(HEAD)` suffixes from the snapshot copy used for staging
   b. Write the clean snapshot to git's object database via `git hash-object -w --stdin`
   c. Stage via `git update-index --add --cacheinfo 100644,<hash>,<file>`
   d. Result: snapshot content (agent response) is committed; plain user edits in the working tree stay uncommitted
   e. Narrow repair path: if the live document is ahead of the snapshot due to a missed agent-doc mutation, `commit` first refreshes the snapshot from the live file, then stages it. The repair only triggers when the redacted component structure is unchanged and the drift looks like an agent-owned `status` change and/or an appended `### Re:` block and/or a `pending` stable-ID superset. Plain user-prompt drift is not absorbed. Status drift also fails closed when the inserted status text contains prompt work such as a preset token at line start (`#next-steps`, including `#next-steps ...` guidance), an imperative directive, or another prompt-bearing line; those edits belong in the next diff, not the previous-cycle commit.
   f. Historical lower bound: if `HEAD` already contains a safe historical agent-owned response growth beyond the stale snapshot, and the current working tree differs from that committed `HEAD` only by a new user follow-up in `exchange`, `commit` must repair the snapshot up to `HEAD` before the no-op `HEAD` comparison. This repair is exchange-only: it may adopt already-committed `exchange` prompt+response growth, and it may also heal committed exchange-only prompt-prefix normalization on already-answered prompts (`❯ do ...` in the stale snapshot vs bare `do ...` directly above a real `### Re:` block in `HEAD`), but it must fail closed if the historical bypass also mutated typed components such as `status`, backlog, or pending. This prevents stale snapshots from staging an older blob and rewinding an already-committed response.
   g. Post-commit local drift classification: if the stripped snapshot already matches `HEAD` but the working tree still differs, `commit` must classify that state as later local drift on top of the committed document before closing as `commit_already_current`. Safe follow-up prompts and arbitrary later working-tree edits both stay uncommitted; the operator-facing explanation must say this is post-commit local drift, not a missed patchback. For the safe follow-up case, the diagnostic must explicitly say the prior response is already committed in `HEAD`, no new assistant response body was supplied, and `commit` will not synthesize a second assistant patchback.
   g1. Direct assistant patchback guard: when `snapshot == HEAD` but the working tree has a likely direct response patchback (`### Re:` / `## Assistant`) that did not come through a newer `agent-doc` cycle, `commit` must fail closed instead of downgrading the document to ordinary post-commit working-tree drift. This prevents a binary-owned response from stopping at "already committed" while the live session doc still contains an unowned assistant write.
   g2. Bare prompt-target guard: even when `HEAD` proves a historical response marker, `commit` must not auto-adopt that history if the repaired tail would still leave a bare prompt target or another prompt-bearing line that was never canonicalized through the binary-owned transcript path.
   h. Extreme drift guard: when the file is vastly larger than the snapshot, `commit` may auto-resync only for bootstrap scaffold snapshots on files with no `HEAD` entry yet. Tracked documents still do NOT wholesale re-sync from the live file, because that would risk absorbing unanswered user prompts.
   i. Relative-path resolution must prefer an existing cwd-local document before falling back to a superproject root. This prevents submodule sessions like `src/boost-client` from accidentally staging an outer-repo shadow file that happens to share the same relative path (for example `tasks/monsterrodholders.md`).
3. Acquire a blocking advisory commit lock keyed by the resolved git dir (`git rev-parse --absolute-git-dir`), so different docs in the same repo or submodule serialize the short staging+commit critical section instead of racing on one shared index. For submodule documents, workspace-write harness sessions rely on the auto-added external gitdir access from `start` / fresh-agent launch so this lock path and the parent-pointer update remain writable.
4. If no snapshot: fall back to `git add -f <file>` (stages entire file)
5. If the fully staged index already matches `HEAD`, close the cycle as `commit_already_current` and return success without creating a duplicate git commit. If that no-op closeout also rewrites transient working-tree drift back to committed `HEAD`, it must refresh every live sidecar that can resurrect or continue showing stale session state: snapshot, working-tree file, CRDT state (for `agent_doc_write: crdt` docs), and the editor/VCS refresh signal when the plugin patch directory exists.
6. Otherwise run the full staging+commit transaction. If git reports `index.lock` contention during `update-index`, `git add`, or `git commit`, retry the whole transaction with backoff instead of retrying only the final `git commit` call.
7. On successful commit: keep the on-disk snapshot / visible document in the same clean post-commit shape as the committed blob, refresh CRDT state from that final cleaned document for CRDT-backed sessions, then either
   - rewrite the working tree locally to that clean single-boundary shape when no live editor IPC listener exists, or
   - send reposition + VCS-refresh IPC signals so the plugin applies that same clean rewrite via the Document API

**HEAD marker:** `(HEAD)` is transient with respect to the committed blob and snapshot. `agent-doc commit` strips it before staging, and post-commit cleanup strips it from the snapshot. However, `(HEAD)` markers are **preserved in the working tree** (and editor buffer via IPC) so the user sees which response headings are new. Preflight classifies `(HEAD)` differences as `boundary_artifact`, and a boundary-artifact-only turn is normalized back to `no_changes` / already-committed closeout before `preflight_started` is recorded, so working-tree `(HEAD)` markers do not create a false-positive visible cycle.

**Post-commit cleanup:** After a successful commit, the **snapshot** is normalized to the same clean single-boundary shape as the committed blob (no `(HEAD)`, single boundary). The **working tree** is repositioned (stale boundaries removed, fresh boundary inserted) and may retain `(HEAD)` annotations on response headings, but editor helpers must prefer the just-committed on-disk document over any stale unsaved buffer when the only drift is agent-owned response-heading attribution and/or boundary churn. In that stale-buffer case, preserving the committed response text takes priority over preserving `(HEAD)` markers. The IPC reposition signal includes `preserve_head: true` for the normal best-effort case.

**FFI variants:**
- `agent_doc_reposition_boundary_to_end()` / `_with_id()` — clean variant (strips `(HEAD)`). Used for snapshot cleanup.
- `agent_doc_reposition_boundary_to_end_preserve_head()` / `_with_id()` — preserves `(HEAD)`. Used for working-tree and editor-buffer cleanup.

## compact

`agent-doc compact <FILE>` — archive old exchange/component content and rewrite the document + snapshot atomically.

- Default `template` target is `exchange`; `--component <name>` overrides it.
- `--message <text>` replaces the default archive pointer text. `--message -` reads the message from stdin (standard Unix convention).
- Full compaction of the `exchange` component without `--message` now emits a default session summary instead of a bare archive pointer. That summary must preserve the archive location plus compact live-state context from `agent:backlog` / `agent:pending`, `agent:queue`, and `agent:icebox` so unresolved work survives even when the old exchange prose is removed.
- Frontmatter `prompt_presets` are not the source of compaction truth. They may tune summary policy (for example, how many backlog items to mention), but the substantive state carried forward must come from the live backlog/queue/icebox components.
- `--tag <name>` sets the pre-compact checkpoint tag; `--tag skip` disables tagging.
- `--commit` closes out the compacted file through the same `agent-doc commit` path described above, rather than relying on an external raw `git commit`.
- `--commit` only proves that the compacted document state itself reached `HEAD` (for `exchange`, typically the summary/archive-pointer rewrite). It does not also persist any later human console explanation; that follow-up still needs its own normal `finalize` / `write --commit` response cycle if it should appear in `exchange`.
- When `--commit` is used and the project exposes `.agent-doc/patches/vcs-refresh.signal`, the closeout must verify that the refresh signal write succeeded; a failed signal write is a hard closeout failure, not a warning.

## skill

`agent-doc skill install` — write the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. Idempotent (skips if content matches).

`agent-doc skill check` — compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.

The bundled skill instructions always render `agent-doc-version` from the running binary version at install time. Harness-specific auto-update steps should call `agent-doc skill install --harness <harness>` rather than relying on shell env detection: Claude Code uses `--harness claude --reload compact`, while Codex uses `--harness codex --reload restart`.

## outline

`agent-doc outline <FILE> [--json]` — display markdown section structure with line counts and approximate token counts.

1. Read file, skip YAML frontmatter
2. Parse `#`-prefixed headings into a section tree
3. For each section: heading text, depth, line number, content lines, approximate tokens (bytes/4)
4. Content before the first heading appears as `(preamble)`

Default output: indented text table. `--json` outputs a JSON array of section objects (`heading`, `depth`, `line`, `lines`, `tokens`).

## upgrade

`agent-doc upgrade` — check crates.io for latest version, upgrade via GitHub Releases binary download → cargo install → pip install (cascade).

> **Startup version check:** On every invocation (except `upgrade` itself), `warn_if_outdated` queries crates.io (with a 24h cache at `~/.cache/agent-doc/version-cache.json`) and prints a one-line stderr warning if a newer version is available. Errors are silently ignored so normal operation is never blocked.

## plugin

`agent-doc plugin install <EDITOR>` — download and install the editor plugin from the latest GitHub Release.

`agent-doc plugin update <EDITOR>` — update an installed plugin to the latest version.

`agent-doc plugin list` — list available editor plugins and their install status.

Supported editors: `jetbrains`, `vscode`. Downloads plugin assets from GitHub Releases (`btakita/agent-doc`). Prefers signed assets (`*-signed.zip`) when available, falling back to unsigned. Auto-detects standard plugin directories for each editor (e.g., JetBrains plugin dir via `idea.plugins.path` or platform defaults, VS Code `~/.vscode/extensions/`).

## sync

`agent-doc sync --col <FILES>,... [--col <FILES>,...] [--window W] [--focus FILE]` — declarative 2D layout sync.

Mirrors a columnar editor layout in tmux. Each `--col` is a comma-separated list of files. Columns arrange left-to-right; files stack top-to-bottom within each column.

**Pre-sync file resolution:** Before the layout algorithm runs, sync parses file paths from `--col` args and resolves each file. Files without a session UUID in frontmatter are treated as **unmanaged** and skipped (no auto-initialization of frontmatter). Only `agent-doc claim` adds session UUIDs. Files with session UUIDs are always treated as **registered**, even if the registry entry was pruned (dead pane). This enables the declarative layout flow: navigating to a file in a split creates a tmux pane regardless of registry state. When the visible columns span multiple nested project roots, sync must first synthesize a per-run tmux-router registry keyed by session id and populated from each document's own nearest `.agent-doc/sessions.json`; it must not ask tmux-router to resolve every file through the caller's current project root. That synthetic registry keeps cross-repo layouts stable when the editor shows documents from both the workspace root and a child repo. If the synthetic registry finds the same pane id claimed by multiple visible documents, it must fail closed before handing that registry to tmux-router: keep the duplicate pane only when exactly one claimant still proves live ownership, or, if none prove live ownership, only when exactly one claimant uniquely matches the pane's project root. Ambiguous duplicates such as `claudescore.md` and `claudescore-3.md` both pointing at the same child-repo pane must be dropped from the synthetic registry entirely so tmux-router rehydrates a distinct pane instead of aliasing two visible documents onto one live pane. Sync also maintains a per-run pane reservation set: once a live pane is accepted for one visible file in the current reconciliation pass, later files in that same pass must treat that pane as unavailable, even if their registry or associated-pane heuristics still point at it. A duplicate claimant falls back to recovery / auto-start instead of silently aliasing two documents onto one pane. The same fail-closed rule applies after tmux-router returns its layout result: if post-sync registration sees one pane assigned to multiple documents, it must refuse to mirror that duplicate into `sessions.json` unless exactly one claimant matches the pane's own nearest project root or already proves live ownership, and it should prune any losing stale duplicate binding instead of preserving a second cross-root alias. That post-sync registration step must also fail closed for single-document geometry-only assignments whenever the document is already under startup-miss or repeated recent `missing_pane` recovery protection: if sync intentionally skipped auto-start for a fail-closed document, it must not immediately rebind whatever spare pane tmux-router left in that column unless that pane now proves live ownership for the document. Before sync provisions a replacement pane for a managed file, it must first try the shared associated-pane recovery path: collect any panes that still prove ownership for that document session via tmux process-tree match on the file path plus supervisor child-PID fallback, re-register a unique winner, and rescue that winner from stash when needed. This closes the claimed-pane reroute shape where the long-lived supervisor is still alive but the foreground process tree no longer mentions the file, so a pure argv/file-path check would have spawned a duplicate replacement session. If multiple associated panes remain, sync must fail closed for that file (skip auto-start and log the ambiguity) instead of creating yet another pane on top. Any associated pane already reserved earlier in the same sync run is filtered out before ambiguity resolution so a sibling document cannot keep stealing it back. For managed files whose registered pane is in a stash window, sync **rescues** the pane back to the agent-doc window via guarded `join-pane`, choosing the left or right edge from the requested column so the recovered pane becomes visible without swapping another live pane back into stash. If the registered pane is no longer alive but tmux still retains it as a dead pane, sync must capture tmux's `pane_dead_status`, persist a recent pane tail for provenance, and record the current cycle phase before it treats that pane as lost. If the registered pane cannot be found anywhere and sync is about to replace it, sync then records a synthetic `supervisor_exit code=missing_pane` (or the dead-pane equivalent) event in the owning session log and attempts the narrow stale-`preflight_started` repair path so abandoned cycles do not stay wedged open behind the replacement start. Before sync auto-starts any replacement pane, it must consult that same session-log history: if two or more `missing_pane` recovery events landed within the last 10 minutes, sync must skip auto-start and leave the document in a stable manual-recovery state instead of cascading more replacement panes. Only after those recovery/fail-closed checks pass, or if no alive pane exists at all, does sync auto-start a fresh Claude session (via `route::auto_start()`).

Malformed YAML frontmatter is not silently dropped during resolution or auto-start. `frontmatter::parse_for_file()` now includes the file path, the parser error, and when YAML reports a location, a compiler-style excerpt of the frontmatter with a caret on the reported line/column before the `--- ... ---` repair hint. Sync logs that same warning, mirrors it into the document's `agent:status` component when present so editor-driven auto-starts have a visible in-document error, then skips that file for the current pass. Once the file parses again, sync clears only the status text it previously wrote for that warning.

**Build stamp:** On each sync invocation, the binary compares its embedded build timestamp (`AGENT_DOC_BUILD_TIMESTAMP` from `build.rs`) against `.agent-doc/build.stamp`. On mismatch (new build detected), all startup locks (`.agent-doc/starting/*.lock`) are cleared and the stamp is updated. This prevents stale locks from old binary instances from blocking auto-start.

**Empty col_args filtering:** Before processing, empty strings in `col_args` are filtered out. The JetBrains plugin sometimes sends phantom empty columns when editor splits change rapidly.

**JetBrains column order:** When the JetBrains plugin reports a multi-split editor layout, column order must be derived from the visible editor-window screen positions, not from the raw `FileEditorManagerEx.windows` iteration order. Focusing the right split can reorder that array, and sync must still preserve left-to-right tmux pane placement while keeping empty columns for non-markdown tabs.

**Blank `--window` safety:** Empty or whitespace-only `--window` values are normalized to "unset" before layout repair, auto-start session scoping, stash rescue, and `tmux_router::sync`. A blank scope must never be treated as a real target window or session override, because that can widen reconcile scope beyond the intended IDE window.

**Column memory:** `.agent-doc/last_layout.json` persists a column→agent-doc mapping across syncs. When a column has no agent doc (user switches to a non-session file), sync substitutes the last known agent doc for that column index. This preserves the 2-pane tmux layout when one editor column temporarily shows a non-agent file. The state file is written after each successful sync for columns that contain an agent doc.

**No early exits:** The full reconcile path always runs regardless of how many panes resolve (0, 1, or 2+). The DETACH phase stashes excess panes from previous layouts. Previous versions had early exits for `resolved < 2` that bypassed stashing, leaving orphaned panes visible.

**Busy pane guard (`layout.rs` only):** The `layout.rs` break_pane path checks `is_pane_busy()` before breaking panes. The sync reconciler's DETACH phase does NOT use a busy pane guard — the `SyncOptions.protect_pane` callback exists in tmux-router but agent-doc passes default options (no guard). This was changed because the guard caused 3-pane accumulation when users switched documents in the same column. Column memory + stash rescue handle session preservation without the guard.

**Reconciliation algorithm** (attach-first order):
1. **SNAPSHOT** — query current pane order in target window
2. **FAST PATH** — if current order matches desired, done
3. **ATTACH** — `join-pane` missing desired panes into target window (isolate from shared windows first, then join with correct split direction: `-h` for columns, `-v` for stacking)
4. **SELECT** — select focus pane before stashing (prevents tmux auto-selecting an unintended pane)
5. **DETACH** — stash unwanted panes out of target window (panes stay alive in stash)
6. **REORDER** — if all panes present but wrong order, break non-first panes out and rejoin in order
7. **VERIFY** — confirm final layout matches desired order

## patch

`agent-doc patch <FILE> <COMPONENT> [CONTENT] [--mode replace|append|prepend]` — replace content in a named component.

1. Read the document and parse component markers (`<!-- agent:name -->...<!-- /agent:name -->`)
2. Find the named component (error if not found)
3. Read replacement content from the positional argument or stdin
4. Load component config from `.agent-doc/components.toml` (if present)
5. Apply `pre_patch` hook (stdin: content, stdout: transformed content; receives `COMPONENT` and `FILE` env vars)
6. Apply mode: `replace` by default. `append` / `prepend` are only used when the caller passes `--mode append` / `--mode prepend`; the component's configured `patch=` mode does not silently change `agent-doc patch` into an append/prepend operation.
7. If `timestamp` is true, prefix entry with ISO 8601 UTC timestamp
8. If `max_entries > 0` (explicit append/prepend only), trim to last N non-empty lines
9. Write updated document
10. Save snapshot relative to project root
11. Run `post_patch` hook (fire-and-forget; receives `COMPONENT` and `FILE` env vars)

**Component markers:** `<!-- agent:name -->...<!-- /agent:name -->`. Names must match `[a-zA-Z0-9][a-zA-Z0-9-]*`.

**Component config** (`.agent-doc/components.toml`):
```toml
[component-name]
mode = "replace"       # "replace" (default), "append", "prepend"
timestamp = false      # Auto-prefix with ISO timestamp
max_entries = 0        # Trim old entries (0 = unlimited)
max_lines = 0          # Trim to last N lines (0 = unlimited)
pre_patch = "cmd"      # Shell command: stdin→stdout transform
post_patch = "cmd"     # Shell command: fire-and-forget
```

## write

`agent-doc write <FILE> [--baseline-file PATH] [--stream] [--ipc] [--force-disk] [--origin ORIGIN]` — apply patch blocks from stdin to a template document.

1. Read response (patch blocks) from stdin
2. Parse `<!-- patch:name -->...<!-- /patch:name -->` blocks
3. Read document and baseline (from `--baseline-file` or current file)
4. Apply patches to baseline:
   - Mode resolution chain applies normally: inline attribute > `components.toml` > built-in default (`replace`)
   - All components use their resolved mode (no hardcoded overrides for exchange)
5. CRDT merge: if the file was modified during response generation, merge `content_ours` (baseline + patches) with `content_current` (file on disk) using Yrs CRDT
6. After any template/CRDT merge or adopted-current replay, re-run exchange prompt-prefix normalization on the final merged transcript before writing so a concurrently bare `do #...` line cannot survive the merge and fail post-commit `session-check`
6. Atomic write + snapshot save + CRDT state save

**`--stream` flag:** Enables CRDT write strategy. Required for template/CRDT documents.

**`--ipc` flag:** Writes a JSON patch file to `.agent-doc/patches/` for IDE plugin consumption instead of modifying the document directly.

**`--force-disk` flag:** Bypasses IPC and writes directly to disk, even when `.agent-doc/patches/` exists (plugin installed).

**`--origin` flag:** Write-origin identifier for tracing (e.g., `skill`, `watch`, `stream`). Logged to `ops.log` as `write_origin file=<path> origin=<value>`. Used with the commit drift warning to trace which process wrote to a file.

**IPC-first behavior (v0.17.5):** When `.agent-doc/patches/` exists (plugin installed) and `--force-disk` is not set, IPC is tried first. `try_ipc()` handles component patches; `try_ipc_full_content()` handles full-document replacement (inline mode). Both check for `.agent-doc/patches/` directory existence first — if absent (no plugin active), they return immediately without delay. On IPC timeout (2s), stream writes complete a local closeout, then exit with code 75 (`EX_TEMPFAIL`) so the caller knows IPC was degraded. Any timeout patch file left behind for editor recovery must carry the original `patch_id`, and once the local closeout lands the binary writes `.agent-doc/claimed-patches/<patch_id>` so later plugin pickup deletes the stale patch instead of replaying it against an advanced baseline. On IPC success with socket delivery, snapshot is saved from the ack-content sidecar written by the plugin after it applies the patch. On IPC success via file delivery (patch file consumed), snapshot is read from the sidecar if present, otherwise from the post-flush disk state.

**`normalize_prefix_lines` IPC field:** When user-prompt normalization adds `❯ ` prefixes to exchange lines, the IPC payload includes a non-empty `normalize_prefix_lines` array listing those lines (un-prefixed). The plugin must apply the same `❯ ` prefixes in the editor buffer using `trimEnd()` matching (trailing-whitespace resilience). See `editors/PLUGIN-SPEC.md` § "Exchange prompt prefix normalization" for the full algorithm.

**Sidecar normalization verification (`verify_sidecar_normalization`):** After receiving the ack-content sidecar from the plugin, the binary checks that each non-blank `normalize_prefix_lines` target appears with a `❯ ` prefix in the sidecar (using `trimEnd()` on both sides). If any target is missing its prefix — indicating the plugin's buffer-side normalization failed (e.g., trailing-whitespace divergence) — the binary falls back to `content_ours` as the snapshot source. Before saving that fallback snapshot, the binary re-splices the tracked backlog/pending component from the current on-disk file so earlier `--pending-*` mutations applied before IPC are not lost just because the editor-side normalization diverged. The fallback is logged as `sidecar_normalization_fallback snap_source=content_ours reason=prefix_divergence` in `ops.log`.

**Durable response capture (v0.33.13):** Once the response survives strict pre-write closeout gates, the final parsed response is persisted to `.agent-doc/captures/<doc-hash>/<cycle-id>.json` before any document mutation or hooks. The capture ledger stores the cycle ID, session/agent/model metadata, response SHA-256, and the exact snapshot/document hashes the response was generated against. It also preserves lifecycle provenance (`write_applied_at`, `replayed_at`, `committed_at`, `discarded_at`) so later recovery closeout remains distinguishable from an original same-turn patchback. `pending/<hash>.md` remains the short-lived queue, but it is now a projection of that durable capture rather than the only durable copy.

**Pending/backlog patch normalization:** Agents must still use granular `--pending-*` flags in normal response cycles, but the binary now has a compatibility path for a single list-shaped `replace:pending` / `patch:pending` block that slips into the response anyway. Before durable capture, `write` / `finalize` / `stream` / `ipc` parse that block, rewrite the live pending component through the same granular pending primitives, drop the pending patch from the captured response body, and only then persist the response. A lone legacy `- [ ] [#] ...` placeholder may be normalized into a generated id during that rewrite, but active add-time parsing and normalized backlog patches now fail closed on bare `[#]` custom-id input and on stacked leading prefixes such as `[#a] [#b] ...` or `id=a [#b] ...`. Unsupported shapes (multiple backlog patches, non-list mutations, or pending prelude/postlude edits) still fail closed **before** `response_captured` so the cycle is not stranded by a forbidden patch after capture.

**Todo patch data-loss guard:** Template write paths now also fail closed on destructive `patch:todo` replacements. When the live `agent:todo` component already contains Markdown checklist items, the binary counts those checklist rows and rejects any incoming `patch:todo` whose replacement body contains fewer checklist items than the current component. This is a narrow data-loss backstop for the `src/session-share/tasks/claudescore.md` repro: a partial todo patch intended to check Phase 1 items must not silently delete untouched Phase 2 / Doc / Expansion entries. Full same-size rewrites still pass; reductions require a deliberate manual edit or a future granular todo mutation surface.

**Manual-repair contract:** For documented manual repair across Claude Code and Codex, once the user prompt already exists in the document the assistant response path is `agent-doc write --commit <FILE>`. Do not document or rely on a repair flow that stops after bare `agent-doc write`; that leaves the cycle open on the wrong side of the response-commit boundary.

**`--commit` behavior:** `agent-doc write --commit <FILE>` has two modes. For non-session documents and `--pending-only` maintenance, it remains a best-effort convenience: it runs the normal write path, then tries `git::commit()`. Outside git it warns and skips commit; inside git it warns on commit failure but still reports the underlying write result. For real session documents (`agent_doc_session` / legacy `session`) that are writing a response, `write --commit` upgrades to the same strict closeout contract as `finalize`: non-git documents are rejected before mutation, commit failure is a command failure, and success means the cycle reached `committed`.

**Response-commit invariant:** Every appended response must cross a commit boundary unless the user explicitly asks to leave it uncommitted. The default happy-path command for normal response cycles is `agent-doc finalize <FILE>`; bare `agent-doc write` is for explicit no-commit exceptions or intermediate checkpoints, not for the final response.

**Write dedup (v0.28.2):** All four write paths skip the actual write when the merged/patched content is identical to the current file on disk. On dedup, pending state is cleared and the function returns early. Events are logged to stderr and appended (with backtrace) to `/tmp/agent-doc-write-dedup.log`. For stream retries with an explicit baseline, if the current exchange already contains the response block, the binary must adopt the current document and re-run transcript normalization (including `❯ ` prefix repair) instead of CRDT-merging the same response a second time.

**Pane ownership verification (v0.28.2):** `verify_pane_ownership()` is called at the top of `run`, `run_template`, and `run_stream`. It reads the document's `session` frontmatter field, looks up the owning pane in the session registry, and compares it to the current tmux pane. If a different pane definitively owns the session, the write is rejected. The check is lenient: it passes silently when not in tmux, when there is no session ID, or when the pane is indeterminate.

**Snapshot persistence invariant:** Write paths normally persist the snapshot from the final merged on-disk content after the response write completes. Narrow exception: when the caller supplied an explicit `--baseline-file` and the live file diverged during the write merge, the snapshot and CRDT state persist from `content_ours` instead of merged `final_content`. That keeps concurrent user edits typed during `finalize` visible in the next diff rather than silently absorbing them into the new baseline. Non-baseline writes still treat the merged disk state as authoritative.

**Replay dedup invariant:** If a strict retry (`finalize`, session-document `write --commit`, or template recovery) sees that the exact visible response block is already present in the live document, the binary must adopt and canonicalize that current transcript instead of merging the same response body again. This check must run before the template write path takes its no-edit / `content_current == base` fast path, otherwise a retry against an unchanged baseline can still append a second `### Re:` block. The adopted-current path must keep prompt-prefix cleanup active so replay repair still fixes bare `❯ ` targets without duplicating the response.

**Exchange prompt normalization:** For append-mode `agent:exchange`, the write path prefixes newly added user lines with `❯ ` based on the `snapshot -> baseline` diff, but the required prefix targets come from the canonical `prompt_bearing_changes` classifier rather than a separate prompt-shape heuristic. This normalization must ignore synthetic heading-only churn from binary-owned commit markers (for example a response heading gaining ` (HEAD)` in the committed snapshot). A heading replacement may preserve existing agent content, but it must not suppress `❯ ` prefixing for the next genuine prompt-bearing user block.

**Carried-forward formatting requirements:** The agent-facing prompts built by `run`, `stream`, and sequential `orchestrate` must not rely only on the latest diff for structure-sensitive response constraints. Before dispatch, the binary scans historical `❯ ...` prompt blocks in the current document for explicit formatting / organization directives that still read as active document-level requirements (for example "Please organize the backlog into a 2-level list" or "Use a numeric list where appropriate"). Matching directives are rendered back into the agent prompt as a separate carried-forward requirement section, and that section must instruct the responder to say so explicitly when harness/chat formatting rules prevent an exact structural match instead of silently flattening the user's requested shape.

**Template structure guard (tail-patch):** After patch application and again after any merge, template writes fail closed if live conversation content (`## User`, `## Assistant`, `### Re:`, `❯ ` prompts) would land outside `<!-- agent:exchange -->`. The binary does NOT silently repair these escapes in the write path — it rejects the write with a precise error identifying the first escaped line. Comment-only notes or other non-conversation scratch content without escaped conversation headings are allowed outside `agent:exchange`. Explicit `agent-doc repair` can still move safe escaped conversation tails back inside exchange for recovery purposes.

**Boundary marker lifecycle (binary-owned):** Boundary management is fully deterministic and handled by the binary — never by the SKILL workflow. The `apply_patches()` function manages the complete lifecycle:

1. **Pre-patch cleanup:** Remove ALL stale boundary markers from the entire document (not just the target component)
2. **Fresh insertion:** Insert a new boundary at the END of the exchange component (after all user text)
3. **Patch application:** Response content is inserted at the boundary position via `append_with_boundary()`
4. **Post-patch re-insertion:** A new boundary is inserted at the END of exchange (after the response)

**Boundary marker format:** `<!-- agent:boundary:{id} -->` where `{id}` is an 8-character hex string (first 8 chars of a UUID v4 with hyphens removed). Short IDs reduce visual noise while maintaining negligible collision probability (~4.3 billion values, self-correcting on collision via next cycle's cleanup).

**Invariants:**
- At most ONE boundary marker exists in the document at any time (outside of code blocks)
- User prompts typed while idle always appear before the response because the fresh boundary is placed after all user text
- The boundary is the dividing line — content before boundary = before response, content after boundary = after response
- Boundaries inside fenced code blocks are excluded from all scanning and cleanup operations

**Cleanup scope:** `remove_all_boundaries()` scans the ENTIRE document (not just the exchange component) and removes every `<!-- agent:boundary:... -->` line that is not inside a fenced code block. This prevents stale boundary accumulation from interrupted cycles or plugin bugs. A single fresh boundary is then inserted at end-of-exchange.

**Design principle:** Boundary insertion was initially implemented in the SKILL workflow (step 1b) but moved to the binary because: (1) it's deterministic (unit-testable with fixed inputs), (2) ALL write paths need it (SKILL, run, stream, watch), (3) non-SKILL paths bypassing step 1b caused stale boundary bugs. **Rule: when adding deterministic operations, ask "will ALL write paths need this?" If yes, it belongs in the binary.**

**IPC boundary:** Before building the IPC patch JSON, all IPC write paths call the clean boundary reposition helper on the current document in memory. This removes stale boundaries and strips transient heading-level ` (HEAD)` markers before inserting a fresh boundary at the end of the exchange. The repositioned document is used only for `boundary_id` extraction (never written to disk by this step). Without this, the IPC path would read the old boundary position (above the user's new prompt), causing responses to be inserted before the prompt. When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware exchange patch automatically. If that synthesized target is append-mode (`exchange`), the binary must also bake `normalize_prefix_lines` into the synthesized patch content itself, because editor plugins normalize prefixes before applying patches and therefore cannot recover a newly appended bare `do #...` line afterward.

**Exchange prompt-order binding:** When an append-mode `patch:exchange` response contains a real `### Re:` heading and the prior document still had an unresolved prompt tail below the old boundary, the write path must not blindly append the response after every unresolved prompt now sitting ahead of the freshly repositioned boundary. Instead it must inspect that old prompt tail in document order, bind the response to the oldest compatible unresolved prompt block (matching pending ids like `#wcup1` / `[#wcup1]` when present), insert the response immediately after that block, and move the new boundary behind the response so later unresolved prompts stay after the boundary for future turns. If the response would skip an older unresolved prompt in that same tail, fail closed instead of silently reversing chronology.

**FFI exports:**
- `agent_doc_reposition_boundary_to_end(doc)` — clean variant. Returns document with stale boundaries removed, `(HEAD)` markers stripped, and a single fresh boundary at end-of-exchange. Used for snapshot cleanup.
- `agent_doc_reposition_boundary_to_end_with_id(doc, id)` — clean variant with explicit boundary ID. Used by post-commit editor refresh.
- `agent_doc_reposition_boundary_to_end_preserve_head(doc)` — preserves `(HEAD)` annotations. Stale boundaries removed, fresh boundary inserted, but `(HEAD)` markers remain. Used for working-tree and editor-buffer post-commit cleanup.
- `agent_doc_reposition_boundary_to_end_preserve_head_with_id(doc, id)` — preserve-head with explicit ID. Editor plugins should call this on post-commit reposition when the IPC signal has `preserve_head: true`, unless the open buffer is stale relative to the just-committed disk document and only differs by agent-owned response-heading attribution / boundary drift, in which case the committed disk text wins.

## finalize

`agent-doc finalize <FILE> [write flags...]` — strict happy-path response write for session documents.

1. Validate that `<FILE>` lives in a git repository before mutating the document.
2. Reuse the same mutation surface as `write`: pending/status mutations are applied first, then the response write path is selected from `--ipc` / `--stream` / `--template` / auto-detect.
3. Run the normal write pipeline (`write`, `run_template`, `run_stream`, or `run_ipc`) with the same snapshot/capture/merge semantics as `agent-doc write`.
4. Invoke `git::commit(<FILE>)` even if the write path returned an error, because the write may have partially succeeded after persisting snapshot/cycle state.
5. Fail unless the final persisted cycle state for the document is `committed`.

**Contract:** `finalize` is the binary-owned happy path for normal session responses. Session-document `write --commit` response closeouts share the same strict contract:

- non-git documents are rejected before any write
- `--pending-only` is rejected because `finalize` is for response cycles, not standalone pending maintenance
- **response-body proof gate:** template / CRDT response writes must contain at least one non-empty non-backlog, non-frontmatter patch or a non-empty unmatched response body that the write path can synthesize into `exchange` / `output`. Empty `patch:exchange` shells, frontmatter-only payloads, or normalization-only template responses fail before IPC / commit instead of silently reaching strict closeout as a zero-patch turn.
- **pre-write pending capture gate** (strict mode only): after parsing and normalizing the response but before any document or snapshot mutation, `finalize` inspects the response text with the same `detect_uncaptured_recommendations()` heuristic used by `session-check`. When `pending_capture_guard` is `strict` and the response contains recommendation-like items or a single high-confidence unresolved follow-up item (for example, a current bug still needing tracking) without any `--pending-add` flags, `finalize` exits non-zero before writing. When `pending_capture_guard` is `warn` (default) or `off`, only the post-commit `session-check` fires. The `<!-- no-pending-capture -->` marker and `had_pending_mutations` cycle state suppress the gate as they do for `session-check`.
- **backlog-required closeout gate:** if the pre-response prompt contract requested backlog capture directly or through prompt-preset expansion (for example `#code-review` => `#follow-up-backlog`), `finalize` must fail before writing unless the cycle recorded backlog mutations or the response explicitly says there were no actionable follow-up items to capture. When the expanded prompt contract names a specific backlog document (for example `Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md`), the gate becomes target-aware: `finalize` / `session-check` must verify that the referenced target's tracked-work surface actually changed during the cycle instead of accepting an unchanged source backlog. If the response body itself lists new tracked backlog items (for example `- [ ] [#id] ...` under "Planned backlog items"), the gate is stronger: every newly listed `[#id]` absent from the target baseline must exist in the live target backlog before closeout succeeds. For `#agent-doc-bug`, the gate is stronger again: when the prompt-bearing changes describe multiple distinct bug reports, the response must enumerate at least that many explicit tracked backlog items for the target backlog before closeout succeeds, and if the prompt contract also says to create plans then the response must cite that many existing plan file paths (for example `Plan: tasks/agent-doc/plan-foo.md`). This gate is contract-driven, not heuristic-only, so findings-only review prose cannot silently commit against an unchanged backlog even when `pending_capture_guard` is merely `warn`.
- **pre-write pending-done gate** (strict mode only): before any document or snapshot mutation, `finalize` also compares the normalized response text against still-open tracked-work ids from `agent:backlog` / `agent:pending` and `agent:icebox`. When the response clearly completes an existing `#id` item but the cycle recorded no matching `--pending-done <id>`, `finalize` exits non-zero before writing and emits the exact rerun command with the required `--pending-done` flags. Session documents default this guard to `strict`; frontmatter or project config may downgrade it to `warn` or `off`. The `<!-- no-pending-done-guard -->` marker suppresses the gate for intentional exceptions.
- **compaction-directive gate:** if the current baseline→document diff contains a bare `compact exchange` request, `write` / `finalize` reject the ordinary response path before mutation and direct the caller to `agent-doc compact <FILE> --commit` (optionally `--message ...`) instead. Normal finalize success is reserved for actual response turns, not binary-owned exchange compaction.
- success means the cycle closed in `.agent-doc/state/cycles/<hash>.json` as `committed`
- a write error plus a commit error is still a command failure even if some recovery work ran
   - imperative directive diffs (`do #id`, `run tests`, `build + install`, `commit + push`, `go`, or pending-item prose like `[#id] Fix ...`) reject status-only/meta-only responses unless they contain concrete execution evidence or a concrete blocker

## repair

`agent-doc repair <FILE>` (legacy alias: `agent-doc recover`) — repair orphaned pending/captured response state and close git-backed repairs through the normal commit boundary.

1. Run the same recovery engine as `preflight` step 1:
   - replay a pending/captured response when the response still needs to be written
   - dedup and clean stale pending/capture state when the response is already present in the document
   - already-applied detection must match the response's normalized visible lines as one contiguous block; do not treat scattered matching phrases elsewhere in the document as a replay hit just because the first few lines happen to recur
   - for template docs, that `AlreadyApplied` dedup path still runs transcript/tail canonicalization before cleanup, including restoring required `❯ ` prompt prefixes from the prompt-bearing classifier
   - when the cycle is open at `response_captured` / `write_applied`, or the logs show a write-complete/no-commit tail, `repair` may recover the missing commit boundary without replaying a response only when the current file/`HEAD` state proves the patchback already landed as exchange-only history
   - respect safe manual removal of an escaped template conversation tail
   - repair a stale `preflight_started` cycle when the persisted snapshot/file hashes still match exactly
   - fail closed for that missing-commit-boundary recovery when the historical bypass mutates typed components (`status`, backlog, pending, etc.) or when the repaired tail still contains a bare prompt target
   - narrow historical replay exception: when there is no active pending/capture artifact, `repair` may replay the newest committed durable capture only if the current exchange still contains a matching orphan prompt target for that response topic and the response heading itself is absent. Generic committed captures without that prompt match stay inert.
   - when replaying a historical template capture that still contains a single list-shaped `replace:pending` / `patch:pending` block, normalize that pending/backlog patch through the granular pending primitives before applying the assistant response; unsupported shapes still fail closed
   - fail closed before replay when a template-mode pending/captured payload looks like a transcript or full document dump rather than one assistant closeout; save the blocked payload under `.agent-doc/repair-blocked/` for diagnostics instead of appending it into `agent:exchange`
2. If recovery work happened and `<FILE>` lives in git, immediately run `agent-doc commit <FILE>`
3. If no pending/captured repair path exists, repair still checks for stale completed backlog items:
   - if the live `agent:backlog` / `agent:pending` still contains `- [x]` items, reap them immediately, mirror the same reap into the snapshot, and archive them into `agent:pending-done` when that component exists
   - snapshot sync for that reap must stay surgical to the backlog/archive components; it must not absorb unrelated live exchange/user prompt edits into the snapshot
   - if prompt-bearing exchange drift already exists (for example plain text inserted immediately before `agent:boundary`), that drift must remain visible to the next preflight diff after the reap; `repair` must not convert it into `no_changes`
   - otherwise stop without committing

**Commit-boundary contract:** For git-backed docs, `repair` must not stop after only updating the live document / pending ledger. A recovered or deduped response should cross the same snapshot+commit boundary in the same command so the next prompt does not inherit repaired-but-uncommitted assistant content. For template docs that means `AlreadyApplied` is still a document-mutation-capable repair outcome when transcript canonicalization is needed.

**Already-applied reopened closeout:** When a recovery path reopens a cycle (for example from durable capture or a Codex `Stop` replay) but finds that the assistant response is already present in the live document, `repair` must still advance the snapshot / `write_applied` state if the snapshot is missing that response. Otherwise the subsequent commit step can misclassify the turn as post-commit local drift and leave the direct patchback outside the binary-owned closeout.

## rename

`agent-doc rename <OLD_PATH> <NEW_PATH>` — migrate session state after a document rename.

Moves all hash-keyed state files (snapshots, baselines, locks, pending, CRDT, pre-response)
from the old path hash to the new path hash. Updates session registry entries.

- Old path may not exist (rename already happened); uses `doc_hash_from_str` as fallback.
- New path must exist.
- Missing source files are silently skipped (idempotent).
- Existing destination files cause an error (no overwrite).

**Auto-migration:** `ensure_initialized` (called from `preflight`, `claim`, `sync`) also
detects renames automatically by scanning `.agent-doc/snapshots/` for orphaned files whose
frontmatter session UUID matches the current document. This makes `rename` a fallback for
cases where the old path is known but auto-detection failed (e.g., symlink resolution
differences).

## watch

`agent-doc watch [--stop] [--status] [--debounce MS] [--max-cycles N]` — watch session files for changes and auto-submit.

- Watches files registered in `sessions.json` for modifications (via `notify` crate)
- On file change (after debounce), runs `submit::run()` on the changed file
- **Reactive mode:** CRDT-mode documents (`agent_doc_write: crdt`) are discovered with `reactive: true` and use zero debounce (`Duration::ZERO`) for instant re-submit on file change. Reactive paths are tracked in a `HashSet<PathBuf>`.
- **Loop prevention:** changes within the debounce window after a submit are treated as agent-triggered; agent-triggered changes increment a cycle counter; if content hash matches previous submit, stop (convergence); hard cap at `--max-cycles` (default 3)
- **Busy guard:** Before submitting, checks `is_busy(file)` via the debounce status signal. If the file has an active agent-doc operation (skill write, stream), the watch daemon skips the file. This prevents the watch daemon from competing with skill writes and causing duplicate responses.
- `--stop` sends SIGTERM to the running daemon (via `.agent-doc/watch.pid`)
- `--status` reports whether the daemon is running
- `--debounce` sets the debounce delay in milliseconds (default 500)

## history

`agent-doc history <FILE>` — list exchange versions from git history.

1. Scan git log for commits touching `<FILE>`
2. Extract the `<!-- agent:exchange -->` component content at each commit
3. Display a list of commits with timestamps and content previews

`agent-doc history <FILE> --restore <COMMIT>` — restore a previous exchange version.

1. Read the exchange content from the specified commit
2. Prepend the old exchange content into the current document's exchange component
3. The restored content appears above the current exchange, preserving both

## transfer

`agent-doc transfer <SOURCE> <TARGET> <COMPONENT> [--bypass-claim] [--items ID1,ID2,...] [--referral]` — move entire component content from source to target document.

1. **Argument validation:** if `--items` is set and component is not `pending`, `backlog`, or `icebox`, reject immediately.
2. Validate source exists; auto-create target if missing (template format with status/exchange/queue/backlog/icebox scaffold)
3. **Pane ownership check:** unless `--bypass-claim` is set, verify the current tmux pane owns the target document's session. If a different pane owns it, reject with an error suggesting `--bypass-claim`.
4. **If `--items` is set (selective mode):** match backlog or icebox parent items by `[#id]` pattern on their flush-left tracked parent line (`- ...` or `1. ...`), move the entire matching item block (including indented nested continuation lines) to the same component in target, leave the rest in source. Report unmatched IDs as warnings.
5. **Otherwise (full transfer):** read named component from source; bail if empty or absent. Clear source component (single newline). Append content to target's matching component with `> **[TRANSFER from <source>]** (timestamp)` annotation.
6. If the transferred component is not backlog/pending or icebox, also merge both backlog and icebox items from source → target
7. Commit the target so transferred headings appear in git HEAD (prevents `(HEAD)` marking on next cycle)
8. Save snapshots for both files

When a source document has an open cycle whose prompt contract explicitly requires backlog capture in `<TARGET>`, changing `<TARGET>`'s backlog/pending surface via `transfer` satisfies that closeout proof. A later `finalize` / `session-check` on the source document should then see the target drift instead of failing the contract gate.

**`--bypass-claim`:** Explicitly opt into cross-pane transfer. Required when the target document is owned by a different tmux pane. Without it, transfer refuses to write to another pane's document. This flag exists because transfers are deliberate user actions (not concurrent writes) and should not be blocked by session ownership.

**`--items`:** Selective backlog or icebox transfer. Only moves items whose flush-left tracked parent line (`- ...` or `1. ...`) contains `[#id]` for each comma-separated ID, together with any indented nested continuation lines that belong to that parent block. IDs may include or omit the `#` prefix (both `--items "#abc,#def"` and `--items "abc,def"` work). Valid with `component=pending`, `component=backlog`, or `component=icebox`. Mutually exclusive with `--referral`.

**`--referral`:** Instead of moving content, inserts a structured referral pointer in the target's component:
```html
<!-- agent:referral src="<relative-path>" component="<name>" created="<timestamp>" -->
*Context from [<path>](<path>) — read source <component> for full history.*
<!-- /agent:referral -->
```
The source content stays in place. When preflight sees `<!-- agent:referral -->` tags, it can optionally resolve and inject the referenced content as context. Mutually exclusive with `--items`.

## extract

`agent-doc extract <SOURCE> <TARGET> [--component NAME]` — move the last exchange entry from source to target.

1. Validate both files exist
2. Find last `### Re:` header in named component (default: `exchange`)
3. Split at that position: extracted (last entry) + remaining (everything before)
4. Update source with remaining content
5. Append extracted content to target's matching component with `> **[EXTRACT from <source>]** (timestamp)` annotation
6. Save snapshots for both files

## backlog

`agent-doc backlog <FILE> <ACTION>` — manage the document backlog component.

Canonical entrypoint is `agent-doc backlog`. The legacy spelling `agent-doc pending` remains accepted as a deprecated alias for the same command surface, but it must emit a deprecation warning directing callers to `agent-doc backlog`.

Supported actions include `add`, `add-gated`, `remove`, `prune`/`reap`, `backfill`, `done`, `edit`, `clear`, `reorder`, `list`, `resolve-gate`, and `set-gate-type`. These actions operate on the canonical `agent:backlog` component while continuing to accept legacy `agent:pending` markers at the document-parsing layer.

Backlog and icebox bodies may include markdown headings or blank separator lines between item groups. The backlog mutation helpers and accidental-patch normalizer must preserve those non-item lines; only the item slots are mutated/reordered.

Tracked backlog and icebox parents may use either flush-left unordered `- ...` or ordered `1. ...` list markers. If any tracked parent in the component uses ordered style, the binary canonicalizes the whole tracked surface as a sequential ordered list in current item order so numeric priority stays valid after mutations.

Nested lists are also supported inside backlog and icebox items. The canonical tracked item is the flush-left parent entry (`- ...` or `1. ...`); any indented continuation lines that follow it belong to that same item and must move/reap/edit with the parent block rather than being parsed as standalone tracked work. When an indented continuation line itself looks like a child task item, backfill canonicalizes it with a checkbox and a nested id shaped like `[#parentid-abcd]` while still keeping it subordinate to the parent block. Multiline `edit` payloads replace that continuation block wholesale; they must not leave stale child lines appended behind the new sublist, and duplicate existing child ids inside the block must be reassigned during canonicalization.

## terminal

`agent-doc terminal <FILE> [--session NAME]` — open an external terminal with tmux attached to the session.

Intended as a fallback for editor plugin commands when no terminal with tmux is open. Prevents duplicate terminal instances by checking for existing attached clients.

1. Resolve tmux session name: `--session` flag > `tmux_session` in document frontmatter > default `"0"`
2. Check if session exists and has an attached client — if so, print message and exit (no-op)
3. If session exists but is detached, open terminal to attach
4. If session does not exist, open terminal which creates and attaches
5. Build tmux command: `tmux new-session -A -s <session>` (attach-or-create)
6. Resolve terminal command (priority order):
   a. `[terminal] command` in `~/.config/agent-doc/config.toml` — template with `{tmux_command}` placeholder
   b. `$TERMINAL` env var — used as `$TERMINAL -e {tmux_command}`
   c. Error with configuration instructions
7. Spawn terminal process (detached)

**Config example:**
```toml
[terminal]
command = "wezterm start -- {tmux_command}"
```

**Safety:** The `{tmux_command}` uses `tmux new-session -A` which attaches to an existing session if it exists, or creates a new one. This means multiple calls to `agent-doc terminal` are idempotent — they either no-op (client already attached) or attach to the existing session.

## preflight

`agent-doc preflight <FILE>` — run all pre-agent steps and output JSON.

Combines interrupted-cycle enforcement, repair, commit, claims-log check, diff, and document HEAD read into a single call. The SKILL workflow consumes the structured JSON output instead of making separate CLI calls.

**Steps (in order):**
0. Enforce previous-cycle completion using persisted per-document cycle state in `.agent-doc/state/cycles/<doc-hash>.json`
   - If the prior cycle is `response_captured` or `write_applied`, preflight first auto-attempts `repair` + `commit`
   - That recovery may close the missing commit boundary directly when the current file/`HEAD` already prove the response landed as an exchange-only patchback; it must not synthesize a second response write in that case
   - If that `commit` path finds the staged snapshot already matches `HEAD`, it closes the cycle as already committed instead of logging `commit_failed`
   - If the prior cycle is `preflight_started` and the persisted snapshot/file hashes still match (including normalized transient-marker-only churn), `repair` repairs that stale preflight lock as a no-op closeout
   - If the prior cycle is `preflight_started`, no pending/capture artifact exists, and `HEAD` proves the visible response patchback was already committed, `repair` repairs the historical snapshot drift and closes the stale cycle before diffing
   - Historical self-heal remains exchange-only: if the visible committed patchback also mutated typed components (`status`, backlog, pending, etc.) or still leaves a bare prompt target, preflight fails closed instead of auto-adopting it
   - If the prior cycle is `preflight_started`, a visible response patchback is present, no pending/capture artifact exists, and `HEAD` cannot prove the patchback was already committed, preflight fails closed before running `agent-doc commit <FILE>` so the tool does not silently commit ambiguous flushed content
   - Even when no open cycle remains, preflight still fails closed before diffing if the current snapshot/file pair already looks like an uncommitted assistant closeout: either a visible bypassed `### Re:` block, or a hidden `snapshot != HEAD` state with no recoverable cycle left to explain it. The failure message names `agent-doc write --commit <FILE>` and, when present, tracked side-effect files outside the document.
   - Otherwise, preflight still attempts the normal snapshot-only `agent-doc commit <FILE>` closeout before failing. That closeout stages the prior snapshot only, so later live working-tree edits stay uncommitted. If the cycle is still open after `repair` + `commit`, preflight fails closed instead of diffing again
   - If the cycle still has no terminal committed state after that attempt, preflight fails closed instead of silently diffing again
1. Repair orphaned pending/captured responses (`agent-doc repair`, legacy alias: `agent-doc recover`)
   - If a template document's current file matches the captured snapshot except that the user manually removed a safe escaped `## User` / `## Assistant` / `### Re:` tail, `repair` respects that edit: it discards the stale capture, updates the snapshot to the repaired file, and closes the cycle instead of failing hash validation or replaying the removed tail
   - If a template document has no pending capture to replay but still shows a stale `agent:boundary` marker above a prompt/response pair that is already complete, `repair` repositions that existing boundary to the true end of the completed turn and advances the snapshot through the same binary-owned repair path. This normalization is deterministic-only: unanswered prompts below the boundary are left untouched and remain prompt-bearing drift.
   - Run backlog maintenance before the commit boundary:
     - lazy-backfill missing backlog ids / normalize checkboxes
   - reap completed `[x]` tracked-work items from backlog and icebox; if a done line is still missing an id, backfill it first so repair/archive paths can reference it instead of dropping it silently
     - archive reaped items into the backlog-done surface when present
     - detect user-authored reorder and surface `pending_reordered: true` in JSON so the skill skips reordering that cycle
     - scan for open backlog-shaped `[#id]` items outside the live `agent:backlog` block (excluding code fences and `agent:icebox`)
       - if the same open id still exists in the live backlog, emit a warning about the shadow copy
       - if an open id exists only in shadow/commented form outside the live backlog, fail closed before commit/diff so unfinished work cannot silently fall out of active tracking
     - compare current backlog against `git show HEAD` to detect open items that were present in recent committed history but are now completely absent from the document (not in live, icebox, or shadow sections); fail closed when any are found, excluding items in the cycle's `pending_done_ids`
2. Commit previous cycle (`agent-doc commit`)
3. Read and truncate `.agent-doc/claims.log`
3c. Check linked docs: inspect `links` from frontmatter — local files compared by git commit time, URLs fetched via `ureq` with HTML-to-markdown conversion (htmd), cached in `.agent-doc/links_cache/`
4. Compute diff between snapshot and current document
   - If the diff classifies as `boundary_artifact` only (transient `(HEAD)` / boundary churn with no real user-authored prompt drift), preflight reports `no_changes: true` and does **not** open a new cycle
   - If the file diff is empty but the active harness prompt for this exact document still has a non-empty body after stripping the leading `agent-doc <file>` invocation, preflight synthesizes an in-memory added-lines diff from that body and continues through the normal prompt classifiers instead of stopping at `no_changes`
5. Read document HEAD from disk

**Steps (in order, pre-step 1):**
0. Layout check — `check_layout()` inspects the current tmux session:
   - Check 1: Window 0 exists (base-index compliance)
   - Check 2: Stash windows have no non-idle (running) panes
   - Check 3: Registered panes all belong to the same tmux session (session-drift detection)
   Returns empty outside tmux. Issues are reported in `layout_issues` (informational).

**Output (JSON to stdout):**
```json
{
  "layout_issues": [],
  "recovered": false,
  "committed": true,
  "claims": [],
  "diff": "unified diff text or null",
  "no_changes": false,
  "document": "full document content",
  "slash_commands": ["/clear", "/agent-doc foo.md"],
  "linked_changes": [{"path": "https://example.com", "summary": "content changed (1234 bytes)", "exists": true}]
}
```

- `layout_issues` — array of tmux health warnings (empty = healthy); always present
- `no_changes` is `true` only when there is neither a real snapshot/file diff nor a synthesized harness-prompt diff for the current document
- `diff` is `null` when `no_changes` is `true`
- `document` always contains the current HEAD content
- `slash_commands` — slash commands extracted from user-added lines in the diff via `parse_slash_commands()`; omitted when empty. Guards: code fences (``` / ~~~), blockquotes (`>`), non-added lines, and removed lines are excluded. Pattern: `/` followed immediately by an ASCII letter.
- `linked_changes` lists changes in linked docs/URLs since last cycle (omitted when empty)
- Progress/diagnostic messages go to stderr

Harness prompt source for the synthesized no-diff path:
- Codex: the `UserPromptSubmit` hook stores the last prompt text keyed by `CODEX_THREAD_ID` / session id, and preflight/plan load it only when the tracked doc path matches the current file exactly
- Other harnesses/tests: `AGENT_DOC_HARNESS_PROMPT` can provide the same prompt text explicitly
- Only the body after `agent-doc <file>` counts as prompt content; bare invocation with no trailing body is not actionable

## session-check

`agent-doc session-check <FILE>` — verify that the previous cycle reached a terminal committed state and that no likely assistant patchback bypassed `agent-doc write` / `finalize`.

- Primary source of truth: `.agent-doc/state/cycles/<doc-hash>.json`
- Fallback for older repos: the last matching non-empty `.agent-doc/logs/ops.log` line for that file (falling back to the global tail only when no file-qualified line exists yet)
- Exit `1` when the current cycle state is still open (`preflight_started`, `response_captured`, or `write_applied`)
- Interrupted-state messaging distinguishes:
  - `preflight_started` / `preflight_diff_start`: the cycle started but no write/commit followed
  - `response_captured`: the response was captured but no write/commit followed
  - `write_applied` / `ipc_write_consumed` / `snapshot_saved_file_ipc`: the response write landed but no commit followed
- Exit `1` when the snapshot→file diff contains a likely direct assistant patchback marker such as `### Re:` or `## Assistant` without a corresponding `agent-doc` cycle
- Exit `1` when the current Codex session still owns the file, the most recent cycle is already committed, and the live document changed again after that closeout without reopening the binary-owned write/commit path. That state is not "clean post-commit drift"; `session-check` must fail closed so Stop-hook recovery or a new explicit closeout can persist the turn.
- If an open `preflight_started` cycle already has a visible `### Re:` patchback in the working tree but `HEAD` still does not prove it, `session-check` must fail with an explicit manual-repair / commit-boundary message instead of a generic open-cycle note. The closeout is still incomplete at that point: the operator must either re-run the response through `agent-doc write --commit <FILE>` or manually commit the repaired document once the visible response is confirmed correct.
- When that bypassed patchback leaves bare `prompt_target` lines in the same changed exchange tail, report the missing-`❯ ` prompt target in the failure marker so repair can route through the binary path instead of silently accepting transcript drift
- Narrow self-heal: if that marker is only exchange-only historical drift already committed in `HEAD`, and the working tree matches `HEAD` modulo transient boundary / `(HEAD)` markers or adds only a newer local user follow-up, `session-check` repairs the stale snapshot or missing commit boundary first and exits `0`
- Historical exchange repair must treat already-committed prompt+response pairs as safe drift too, not only bare `### Re:` insertions. If `HEAD` contains a committed user directive or question immediately followed by its `### Re:` block, snapshot repair advances to that committed pair before classifying any newer working-tree prompt as the unresolved tail.
- Prompt-bearing drift classification must also ignore an already-answered exchange tail that sits below a stale `agent:boundary` marker. If the current exchange tail's oldest prompt-bearing line is immediately followed later in that same tail by a real `### Re:` / `## Assistant` response block and no newer prompt-bearing tail follows it, `session-check` must not keep reporting that prompt as unresolved work; the remaining closeout handling is commit-boundary repair, not `pending prompt_target`.
- Typed-component fail-closed rule: `session-check` must not self-heal committed historical bypasses that also mutate typed components such as `status`, backlog, or pending, and it must not report success when the repaired tail would still contain a bare prompt target.
- Pending-capture guard: after a committed cycle, `session-check` inspects the committed response capture for recommendation-like batches that were not accompanied by any `--pending-add` / `--pending-add-gated` flags in that cycle
- Pending-done guard: after a committed cycle, `session-check` also inspects the committed response capture against still-open tracked-work ids in `agent:backlog`/`agent:pending` and `agent:icebox` and warns or errors when the response appears to complete an existing `#id` task but the cycle recorded no matching `--pending-done <id>`
- Completed-tracked-work reap guard: after a committed cycle, `session-check` fails closed if the live document still contains stale `- [x]` tracked-work items in backlog or icebox that were not newly marked done by that same committed cycle. Fresh `--pending-done` items are allowed to survive until the next preflight reap; anything older must either be reaped through both the working tree and snapshot or stop with an explicit failure instead of silently carrying completed items forward.
- Backlog-shadow guard: after a committed cycle, `session-check` scans for open backlog-shaped `[#id]` items outside the live `agent:backlog` block (excluding code fences and `agent:icebox`). Duplicate shadow copies warn; shadow-only open items fail closed so the cycle cannot report success while active work is stranded outside tracked backlog scope.
- Backlog-replay guard: after a committed cycle, `session-check` compares the current document's backlog against the pre-cycle baseline (`.agent-doc/baselines/`, falling back to `git show HEAD`). Open items present in the baseline but completely absent from the current document — not in live backlog, not in icebox, not in shadow/commented sections, and not in the cycle's `pending_done_ids` — fail closed. This detects items silently dropped during a response cycle when no shadow copy survives.
- Default guard mode is `warn` for non-session docs; session documents default the pending-done guard to `strict` so closeouts fail before commit unless the cycle records the matching `--pending-done <id>` (or the document/project explicitly downgrades the mode)
- `pending_capture_guard: strict` in document frontmatter or `.agent-doc/config.toml` `[guards] pending_capture = "strict"` upgrades that condition to exit `1`
- `pending_done_guard: strict` in document frontmatter or `.agent-doc/config.toml` `[guards] pending_done = "strict"` upgrades the missing-`--pending-done` condition to exit `1`
- `pending_capture_guard: off` disables the guard; `<!-- no-pending-capture -->` in the response suppresses it for that cycle (the marker is stripped from the committed blob and from the snapshot/working-tree file post-commit — it is ephemeral signaling only)
- `pending_done_guard: off` disables the missing-`--pending-done` guard; `<!-- no-pending-done-guard -->` in the response suppresses it for that cycle (same strip-after-check behavior as `<!-- no-pending-capture -->`)
- Snapshot-committed guard: after a committed cycle, `session-check` verifies that the current snapshot matches `git show HEAD:<file>` in the owning git root (narrowed to submodule when applicable). If the snapshot differs from HEAD, the response patchback is visible in the file but was never committed — `session-check` exits `1` with a specific message. This catches cases where `cycle_state` says `committed` but the git commit never landed or committed the wrong content.
- Uncommitted-closeout diagnostics: when `session-check` detects either a direct bypassed `### Re:` patchback or the hidden `snapshot != HEAD` shape with no matching open cycle, the error must include the exact `agent-doc write --commit <FILE>` follow-through command. If tracked side-effect files are dirty in the same repo, list those paths too so the operator can see what would otherwise be lost outside git history.
- Submodule no-op pointer update: when `commit` closes as `commit_already_current` for a file in a submodule, it also checks whether the parent repo's submodule pointer is stale and updates it if needed. This handles the case where a previous submodule commit landed but the parent pointer was never updated.
- A cycle closed by `agent-doc commit` as `commit_already_current` counts as terminal / committed: it means the staged snapshot was already identical to `HEAD`, so no duplicate git commit was necessary
- Exit `0` when the cycle state is committed or no state/log file exists
- Intended skill/runbook use: the Codex/direct-exec path runs `agent-doc session-check <FILE>` immediately after `agent-doc finalize <FILE> ...` or manual `agent-doc write --commit <FILE> ...`; if the check exits nonzero, the cycle is still open and the agent must fail closed instead of reporting success. Historical snapshot repair may advance the snapshot to already-committed `HEAD` before classifying later local drift, but only when that later drift did not add a newer assistant patchback beyond `HEAD`.

`preflight` also treats the write-completed/no-commit case as recoverable commit-boundary drift. When the last relevant state for a file is `write_applied`, `ipc_write_consumed`, or `snapshot_saved_file_ipc` with no later `commit_*`, preflight records `write_completed_commit_missing`, attempts `resume_commit_attempt`, and logs either `resume_commit_success` or `resume_commit_blocked_drift`.

**URL link processing:**
- URLs (`http://`/`https://`) in `links` frontmatter are fetched with a 10s timeout
- HTML responses are converted to markdown via `htmd` (stripping script, style, nav, footer, noscript, svg)
- Content is cached at `.agent-doc/links_cache/<sha256(url)>.txt`
- Changes detected by comparing fresh fetch against cached content

## Preflight Mtime Debounce

The `preflight` command applies a 500ms mtime debounce gate: if the document's filesystem mtime is less than 500ms old, preflight waits until the file has been idle for at least 500ms. This prevents duplicate preflight runs caused by rapid sequential file saves from the editor.

## Unified Diff Context Radius

Diff output now uses a 5-line context radius (unified diff with 5 lines of surrounding context around each hunk). This gives the agent better surrounding context to understand changes.

## Route --debounce

`agent-doc route <FILE> [--debounce MS]` — optional debounce flag to coalesce rapid editor triggers. When set, route will skip execution if another route call for the same file completed within the debounce window.

For fresh auto-starts, route now fails closed instead of silently idling when prompt readiness is missed. If the initial 30s readiness wait times out, route performs one bounded fallback trigger injection. Success logs `fresh_route_trigger_recovered`; failure logs `fresh_route_trigger_missing` and returns an error so the caller can surface the missed start explicitly.

Fresh auto-start also requires a real per-document cycle acknowledgment after trigger injection. Accepting the typed command in the pane is not sufficient. After the trigger is sent, route polls the target document's persisted cycle state and only treats the start as successful once it observes a new cycle for that file (for example `preflight_started`, `response_captured`, `write_applied`, or `committed`). If the baseline state was already `committed`, route requires a newer cycle id; same-cycle `commit_already_current` churn does not count. Because a fresh pane can immediately recycle through a supervisor restart or missing-pane recovery before the first real document cycle becomes visible, this path now uses the same longer bounded acknowledgment window as the live-child reroute path (30s in production, 2s under tests). Success logs `fresh_route_start_acknowledged`; absence of a new cycle within that bounded wait logs `fresh_route_start_missing` and fails closed.

If that fresh-start acknowledgment fails after route already created and registered a new pane, cleanup must preserve the pane when it is still the live registered owner for the document. Route may fail closed with `fresh_route_start_missing` / `fresh_route_trigger_missing`, but it must not convert that startup-ack miss into a visible tmux pane crash by killing the new live pane as "orphaned cleanup".

### Startup-miss tracking

When a fresh-start or routed-trigger acknowledgment times out, route now records a startup-miss marker at `.agent-doc/state/startup-miss/<doc-hash>.json` with pane provenance (pane id, session id, harness, origin, cycle baseline, timestamp). Route also shows a tmux-owned startup-miss diagnostic overlay on the target pane for 10 seconds, including the reason, retry command, and the recorded startup-miss timestamp. The fail-closed routed error surface must include that same timestamp so plugin/UI wrappers can correlate the failure to the persisted marker without re-reading the state file. The diagnostic must not be injected into the harness input buffer or left as drafted shell text.

On the next route invocation, if the registered pane matches a persisted startup-miss marker, route first re-runs the live-owner proof. If that same pane still proves live ownership of the document, the marker is no longer cleared just because ownership matches: route may clear it only when the session log proves a newer open harness run for that pane after the recorded miss. A harness run boundary is any `*_start` / `*_restart` event inside the owning supervisor session, not only a brand-new `session_start`, so clean child restarts in the same pane count as fresh provenance. If the same pane still owns the document but the session log only shows a closed/timeout restart loop with no newer run after the miss, route must deregister it and auto-start fresh instead of repeatedly reusing and re-clearing the same broken pane. If the marked pane is still alive but the session log for its latest live harness run never recorded a later child exit or `session_end`, and the supervisor socket is missing/unreachable, route must fail closed and report that stranded-session provenance instead of auto-starting a replacement pane on top of it. Only when the marked pane no longer proves live ownership and the recorded session already closed (or there is no stranded-session evidence) does route deregister it, clear the marker, and auto-start fresh. Separately, if `sessions.json` and the session log already prove a newer open start on a different registered pane for the same document, the older marker is stale and must be cleared before route reasons about the current owner. This prevents startup-miss reruns from masquerading as a generic tmux-pane crash.

Successful cycle acknowledgment (both `fresh_route_start_acknowledged` and `route_cycle_start_acknowledged`) clears the startup-miss marker. `session-check` also reports a warning when a startup-miss marker exists for the inspected document, except when the marker is provably stale because a newer registered open start has already superseded it; in that case `session-check` clears the stale marker instead of surfacing a fake current crash warning.

Editor-driven sync and manual `start` must apply the same supersession rule before any fresh-pane provisioning path. If the startup-miss marker is already stale because a newer registered owner is open, clear it first. If the marker still belongs to an alive registered pane, those entrypoints must fail closed or refocus the existing owner instead of creating another replacement pane and making a registry rebind look like a tmux-pane crash. More generally, `start` must not treat an alive registered pane with missing ownership proof plus an unavailable supervisor as permission to clear the binding and replace it, and a halted supervisor is still not sufficient proof to replace the pane when the session log shows that pane's latest run never recorded a child exit or `session_end`. When `start` does intentionally replace an alive pane after an explicit crashed/halted determination, it must preserve the old binding until the new pane registers so the normal `session_superseded` and `session_end origin=registry_rebind` provenance is appended to the session log.

### Live-child ack suppression

When route detects a live agent-doc child process for the target file in the resolved pane (via `find_live_owner_pane`), it still sends the routed trigger, but prompt-bearing drift on top of a closed cycle must still produce a real per-document cycle acknowledgment. A merely live child is not proof that the new routed prompt was consumed into a fresh cycle. Route therefore waits for a newer cycle state and fails closed if none appears, even when the pane already owns the document. This guard only considers prompt-bearing drift from the document body; frontmatter-only metadata edits do not qualify, and neither does a stale-boundary tail whose oldest prompt is already visibly answered by a later `### Re:` / `## Assistant` block in that same tail. Because a healthy live owner can take longer than a fresh pane to surface the next `preflight_started` boundary after a routed prompt, this path uses a longer bounded acknowledgment window (30s in production, 2s under tests) before it records `route_cycle_start_missing`. The only suppression remains the open-cycle case: if the baseline cycle is already in flight, route does not require another start ack for that same pending work.

## is_tracked FFI Export

`agent_doc_is_tracked(path)` — C ABI export for editor plugins. Returns whether the given file path is tracked in `sessions.json` (has a registered session). Plugins use this via JNA/FFI to conditionally show UI elements for tracked documents.

## Sync provision_pane

The sync path uses `provision_pane` instead of the standard auto-start. This variant accepts `col_args: &[String]` and computes `split_before` via `is_first_column(file, col_args)`, so new panes split in the correct direction for their column position (left-column files split before, right-column files split after). It does not block waiting for the `❯` prompt to appear (unlike `route` which waits up to 30s), avoiding sync blocking on slow Claude startup when arranging multiple panes. The call site in `sync.rs` passes the `col_args` slice through from the CLI arguments.

## Sync Swap-Pane Atomic Reconcile

The sync path uses swap-pane atomic transitions via tmux-router. When reconciling pane layout, `provision_pane` spawns sessions without blocking on prompt detection. A `context_session` parameter allows cross-session override — sync knows which session it's managing and passes that context to `auto_start`, which takes priority over the document's `tmux_session` frontmatter field.

Before lazy-claim or auto-start, route also scans tmux for an already-running pane whose process tree is executing the same document and re-registers that pane when found. This prevents stale-registry drift from spawning a parallel fallback session for a document that is already live somewhere else. The same ownership proof now applies to an *alive* registered pane: route first tries path provenance from the canonical document-path entry (`pane + supervisor PID + supervisor instance id`), then falls back to the tmux process-tree match on the document path, accepting equivalent pane-relative start-path forms for the same file (for example `tasks/docs.md` in a submodule pane versus `src/session-share/tasks/docs.md` in the root registry), then falls back to the per-session supervisor socket by asking it for the live supervisor PID and mapping that PID back to the owning tmux pane. Utility-side commands that merely mention the file path, such as `agent-doc route <FILE>` or `agent-doc claim <FILE>` running in a control pane, do **not** count as live-owner proof; the process-tree proof is reserved for the long-lived `agent-doc start <FILE>` supervisor path (plus harness-owned matches). If another pane does prove ownership, route re-registers there first and dispatches to the recovered live owner. That re-registration must preserve authoritative supervisor metadata when the registry or supervisor socket can still prove it; recovering a live pane must not rewrite `sessions.json` back to the short-lived `route` / `sync` utility PID with an empty `supervisor_instance_id`. If neither path provenance, the tmux process tree, nor the supervisor PID can prove that the registered pane still owns the file, route deregisters that stale binding and continues with lazy-claim / auto-start recovery instead of dispatching into the ambiguous pane. Any later `send-keys` dispatch must still re-check the registry entry for the chosen pane and refuse the send if that pane is currently registered for a different file.

Cross-session stash rescue is intentionally non-destructive: if sync finds a live stashed pane for the document in another tmux session, it logs the mismatch and preserves that pane in place instead of moving it across sessions or killing it during rescue.

## Sync tmux_session Auto-Repair (Deprecated Field)

> **Note:** `tmux_session` in frontmatter is deprecated. This auto-repair mechanism exists for backward compatibility during the deprecation period and will be removed when the field is removed.

When `context_session` (from `sync --window`) differs from the document's `tmux_session` frontmatter value, both `auto_start` and the sync loop automatically repair the frontmatter via direct string replacement. This avoids frontmatter round-trip issues (extra newlines) and ensures the document reflects the actual session assignment after cross-session moves.

## Sync Resync Report-Only

The post-sync `resync` call runs with `--fix` disabled (report only). `auto_start` with `context_session` intentionally places panes in a different session than the frontmatter originally specified — `resync --fix` would incorrectly kill these cross-session panes. The resync still reports anomalies for operator awareness.

`resync` uses that same live-owner proof for registered documents: path provenance from the canonical document-path entry first, then direct tmux process-tree match on the document path, including equivalent pane-relative start-path forms for the same file, then supervisor PID fallback. The proof must resolve back to the registered pane itself; if some other pane owns the file, the current registration is treated as stale and reported as `NoLiveOwner`. `resync --fix` deregisters that stale binding without killing the pane. When a pane still proves live ownership of its bound document, `resync --fix` must preserve it even if foreground-command heuristics or tmux layout heuristics would otherwise classify it as `WrongSession` or `WrongWindow`; active bound sessions are never killed or stashed just because registry/layout cleanup is running. During stash cleanup, an unregistered agent pane is preserved when either (a) live-owner proof still ties it to some registered document or (b) the pane still hosts a live supervisor socket that route/sync can later recover by session id. Only panes with neither proof are treated as orphaned and eligible for purge, which closes the earlier stash-loss shape where a recoverable stashed session could be silently killed after its registry entry drifted away.

For routed dispatch into an existing pane, readiness must mean more than "a prompt glyph exists somewhere near the footer." Route ignores panes that still show an active permission prompt, and for Codex it must also treat the `tab to queue message` footer as busy rather than idle. Current Codex builds may expose an idle composer as a canned suggestion line such as `› Run /review on my current changes` or `› Find and fix a bug in @filename`; those known placeholder lines count as ready even though they contain text, while real drafted user input still does not. Prompt readiness is sampled across consecutive polls before route injects a new trigger, so a pane that is mid-run or queue-only fails closed instead of accepting a trigger that never starts a new document cycle.

## Sync Visible-Window Split

When the sync path (`skip_wait=true`) creates new panes, it prefers splitting in the visible `agent-doc` window of the target session rather than falling back to any registered pane (which may be in a stash window). This ensures new panes appear where the user can see them. Falls back to `find_registered_pane_in_session` if no panes exist in the agent-doc window. That fallback pane is only a split anchor; route must register and dispatch to the newly created pane, never to the unrelated anchor pane itself.

## Repair Layout

`repair_layout` normalizes the tmux window layout before every sync. It receives the tmux handle, session name, and target window name (always `"agent-doc"`). The plugin always passes `--window agent-doc` as a fallback so the target window name is known.

**Phase 1 — Stash consolidation:** Merges all secondary stash windows (`stash-*` and duplicate `stash` windows) into a single primary stash window. For each secondary, all panes are joined into the primary via `join-pane -dv`, targeting the largest pane to avoid "pane too small" errors. Empty secondary windows are killed after pane migration.

**Phase 2 — Window rescue:** If the target `agent-doc` window does not exist, attempts to recreate it by finding an alive registered pane in the stash using each requested file's own nearest `.agent-doc` registry, breaking it out with `break-pane`, and renaming the resulting window to `agent-doc`.

**Phase 3 — Index normalization:** Re-lists windows after Phases 1+2 and moves the `agent-doc` window to index 0 via `move-window` if it is not already there. This phase always runs.

**Fast path:** When the target window already exists and there is at most one stash window, Phases 1 and 2 are skipped entirely. Only Phase 3 (index normalization) executes, making the common case a lightweight check.

## session

`agent-doc session` — show the configured tmux session.
`agent-doc session set <name>` — update config.toml and migrate panes to the new session.

**Show:** Reads `.agent-doc/config.toml` `tmux_session` field and prints it (or "(none)").

**Set:** Updates config.toml, then moves the `agent-doc` window and `stash` window from the old session to the new one via `tmux move-window`. If the move fails (target session doesn't exist), config is still updated — subsequent route/claim operations will target the new session.

**Session resolution (`resolve_target_session`):** Single function in route.rs that all session-targeting code paths use. Priority: (1) non-empty `context_session` from sync --window, (2) config.toml if alive, (3) fallback to current session. Empty/whitespace-only overrides are ignored. Config is auto-updated only when the configured session is dead.

## migrate

`agent-doc migrate [FILES...] [--all] [--dry-run]` — migrate documents from deprecated component names and attributes to canonical forms.

Renames `<!-- agent:pending ... -->` to `<!-- agent:backlog ... -->` (both open and close tags) and strips deprecated `patch=`/`mode=` attributes from backlog component tags. Skips files already using canonical markers. Updates the snapshot after each successful migration.

`--all` scans the project root for `.md` files containing deprecated `agent:pending` markers. `--dry-run` previews which files would be migrated without writing. Idempotent — running twice produces the same output.

Markers inside fenced code blocks or inline code spans are not transformed.

## dedupe

`agent-doc dedupe <FILE>` — remove consecutive duplicate response blocks.

Detects consecutive `### Re:` blocks with identical content (after stripping boundary markers) and removes the duplicate. Updates the snapshot after removal. Idempotent — running twice produces the same result.

After removing duplicates and updating the snapshot, `dedupe` also deletes the corresponding stale patch file at `.agent-doc/patches/<hash>.json` (if present). Without this cleanup, `processPendingPatches()` on plugin restart would re-apply the removed content, creating another duplicate.

## plan

`agent-doc plan <FILE>` — derive a structured post-preflight planning/dispatch record for the current document.

The command computes the current diff against the saved snapshot and emits JSON with:

- `prompt_targets`
- `execution_scope`
- `repo_actions`
- `required_commands`
- `pending_mutations`
- `handoff`
- `blockers`

The intent is to give the skill/orchestrator an explicit, binary-owned execution contract before repo work starts. The implementation is deterministic and reuses the same diff classifiers that back preflight (`prompt_bearing_changes`, imperative directives, slash-command parsing, orchestration detection) rather than hiding planning in skill prose.

`pending_mutations` includes two kinds:

- `resolve_existing` — emitted when a `do #id` directive in `repo_actions` matches an open tracked-work item in backlog or icebox. Tells the skill the finalize command must include `--pending-done <id>` if the work completes.
- `expect_add` — emitted when prompt targets contain signals that the response will likely generate recommendations (e.g., user asks for "tasks", "todo", "backlog", "what's next", "recommendations", "next steps", "action items"). Tells the skill the finalize command should include `--pending-add` flags for any actionable items in the response.

`execution_scope` is binary-owned dispatch policy:

- `normal` — execute `repo_actions` normally before response persistence.
- `plan_backlog_only` — the active prompt is a report/planning contract such as `#agent-doc-bug`. In this scope the planning record must suppress repo actions for the current cycle even if the raw prompt included imperative wording, because the contract requires creating plan/backlog artifacts first and defers implementation to a later explicit `do #id ...` turn.

## orchestrate

`agent-doc orchestrate <FILE> --mode sequential|parallel|dag [--task TEXT ...] [--from-file TASKS.md] [--from-exchange] [--agent NAME] [--model MODEL] [--dry-run] [--plan]`

**Skill-side dispatch:** the bundled skill/runbook treats natural-language orchestration requests as aliases for this command. Ordered phrases like `run these in order`, `chain these`, `one by one`, `orchestrate`, `synchronous orchestra`, and the already-seen malformed variants `synchronous orcestra` / `synchronous opera` map to `--mode sequential`; concurrency phrases like `fan out`, `concurrent`, and `simultaneously` map to `--mode parallel`; dependency phrases like `after #a do #b`, `depends on`, and `fan in` map to `--mode dag`.

**Compound single-line directives:** the bundled skill/runbook may also normalize one prose task with distinct follow-up clauses into explicit orchestrate steps before invoking this command. Example: `do #ntoc. Add to today's news. commit + push` can be steered into primary work plus explicit follow-up news/update/push steps. This remains skill-side steering rather than binary-owned free-form parsing; the CLI still expects explicit `--task` entries or explicit DAG metadata.

Shared task-source resolution:

1. Collect tasks from repeated `--task` flags, preserving order.
2. If `--from-file` is provided, read the file and extract tasks from the last fenced code block or markdown list that contains list items; otherwise fall back to non-empty trimmed lines.
3. If `--from-exchange` is provided, read the document's `agent:exchange` component. Before extracting tasks, scope the exchange text to only the user's latest additions by comparing against the snapshot — lines matching the snapshot (modulo `(HEAD)` markers and boundary comments) are excluded, and only the newly-added tail is searched. This prevents stale task lists in prior response content from being selected when the user's directive is a bare line at the exchange tail. Falls back to the full exchange text when no snapshot exists. Then apply the same task extraction rule to the scoped text. List items with the `❯ ` prompt prefix (added by the binary write path for user prompts) are normalized by stripping the prefix before parsing, so `❯ - do #task` is treated identically to `- do #task`. Preflight orchestration detection applies the same normalization before deciding whether a multi-item exchange batch should route through `agent-doc orchestrate ... --from-exchange`, so prefixed batches are routed deterministically instead of depending on skill prose.
4. Independently scan `--from-file` / `--from-exchange` text for batch-level `preset <name>` or `presets <a>, <b>` directives. These are not tasks; they request frontmatter `prompt_presets` by name. Preserve request order and de-duplicate repeated names.
5. Validate requested presets against the document frontmatter `prompt_presets` map. Missing preset references fail closed.
6. Concatenate all resolved tasks in source order. Error if the final task list is empty.

### `--dry-run` and `--plan`

Both flags exit without executing any agent tasks.

- `--dry-run`: prints task labels only, before preset expansion. Stops after the initial task-list summary.
- `--plan`: resolves tasks fully (including preset expansion via `apply_prompt_preset_block`), then prints each task's fully expanded prompt. Use this to verify what the agent will receive before committing to execution:

```
[orchestrate] plan — 2 task(s) (no execution)
[orchestrate] step 1/2: do #prep
[orchestrate] --- prompt ---
[orchestrate] (preset #spec-test-commit-push)
[orchestrate] update spec + tests. commit + push
[orchestrate] do #prep
[orchestrate] --- end prompt ---
[orchestrate] step 2/2: do #report
...
```

`--plan` is the recommended harness verification step before issuing a live `orchestrate` call. It applies to all three modes (`sequential`, `parallel`, `dag`).

### Frontmatter args forwarding

Orchestrate subprocesses inherit permission/sandbox settings from the document frontmatter using the same precedence chain as `agent-doc start`:

- **Claude:** `fm.agent_args` > `fm.claude_args` > `config.agent_args` > `config.claude_args`
- **Codex:** `fm.agent_args` > `fm.codex_args` > `config.agent_args` > `config.codex_args`

When resolved args exist, the subprocess command is built from structural base args (Claude: `-p --output-format json`; Codex: `exec --json`) plus the resolved frontmatter/config args. When no args are resolved, default base args apply (Claude: `--permission-mode acceptEdits`; Codex: `-s workspace-write`).

### Subprocess stderr surfacing

Streaming agent subprocesses (`send_streaming` in both Claude and Codex backends) drain stderr in a background thread. When the subprocess exits non-zero, the iterator yields a final `Err` containing the exit status and stderr content — e.g. `"claude subprocess exited with exit status: 1: permission denied"`. This replaces the previous behavior where stderr was silently discarded and failures surfaced only as `"empty response from streaming orchestrate step"` with no diagnostics.

When the subprocess exits successfully but stderr is non-empty (warnings, deprecation notices), the content is logged to the parent's stderr with an `[agent]` prefix and does not produce an error.

### Streaming args construction

`build_streaming_args()` transforms the base args for streaming: it strips any existing `--output-format` value and replaces it with `--output-format stream-json`, then unconditionally adds `--verbose`. The `--verbose` flag is required by the Claude CLI when `-p` and `--output-format stream-json` are combined.

### Queue item classification and command dispatch

Orchestration items are classified before execution. Items starting with `/` are **commands** dispatched through a priority chain; all other items are **prompts** dispatched through the normal agent lifecycle.

**Classification:** `queue_dispatch::classify(text)` returns a `QueueItem` with `kind` (`Prompt` or `Command`), the parsed `command` name (without `/`), and `args`.

**Command dispatch priority:**

1. **Inline execution** — `/model <tier>` updates the orchestrate-local model override for subsequent prompts (no document mutation); `/compact [file]` delegates to `agent-doc compact` + `agent-doc commit` as subprocesses.
2. **Supervisor IPC** — if a supervisor socket is active for the document session (`.agent-doc/supervisor/<uuid>.sock`), the command text is sent via the `inject` IPC method, which writes it to the harness's pty stdin.
3. **tmux send-keys** — if the document's session has a registered pane in `sessions.json`, route first verifies that some live tmux process tree still proves document ownership for that session. When the registered pane is stale but another pane is the live owner, route re-registers there and sends via `tmux send-keys` with Enter-retry polling (30s timeout). When no live owner can be proven, dispatch fails closed instead of sending the command into an ambiguous pane.
4. **Failure** — commands that cannot be dispatched through any path fail immediately; orchestration halts.

**`/model` state:** the model override updated by `/model <tier>` applies to subsequent prompt items in the same orchestration batch. It does not persist to frontmatter.

**Ordering:** command and prompt items execute strictly in sequence. A command must complete before the next item starts.

### `--mode sequential`

Sequential orchestration runs one full fresh-agent lifecycle per task:

1. Inject `❯ <task>` into `agent:exchange`, immediately before the current boundary marker when one exists.
2. Run `agent-doc preflight <FILE>` after each prompt injection and use its `baseline_file` for response persistence.
3. If that preflight encounters an already-open `preflight_started` cycle from an outer skill/router pass, it first auto-attempts the safe snapshot-only `commit_already_current` closeout for that prior cycle, then continues into the new preflight. Sequential orchestration does not keep a bespoke task-1 reuse path anymore; `preflight` itself is the idempotent boundary.
4. If batch-level prompt presets were requested, prefix the concrete task prompt with one labeled block per preset, for example:

```text
(preset #1)
Today is 2026-04-25.
Keep the work tree clean.

do #prep
```

5. Build the normal edited-document prompt shape (`<diff>` + full `<document>`) from the current document state.
6. Resolve the agent backend from `--agent` → frontmatter `agent` → global config `default_agent` → `"claude"`.
7. For CRDT session docs with an `agent:exchange` component, if the chosen backend supports streaming (`claude` or `codex`), stream the in-progress step response into `exchange` before the normal write boundary. The streamed buffer is "existing exchange through injected prompt" plus the child response so far, with the boundary marker kept at the end.
8. Send exactly one fresh backend request with **no resume** (`session_id=None`, `fork=false`) so steps do not accumulate agent conversation state. When streaming is unavailable, sequential mode falls back to the blocking request path.
9. Persist the final response through `agent-doc finalize <FILE> --baseline-file ...` using the document's resolved write mode (`--stream` for CRDT docs, `--template` for merge/template docs, inline for append docs). Template-mode child responses must fail closed unless they include a real `<!-- patch:exchange -->` block and no raw unmatched transcript outside patch blocks; `orchestrate` must not rely on write-path synthesis to stuff malformed child output back into `agent:exchange`.
10. Run `agent-doc session-check <FILE>` immediately after finalize. Stop on the first failure.

Notes:

- Sequential orchestration requires the normal git-backed finalize path; `--no-git` is rejected.
- The imperative-response contract still applies because persistence flows through `finalize`.
- The injected prompt becomes part of the document/system-of-record before the agent runs.
- The same `preflight` call shape is used for every task. Any inherited open-cycle cleanup happens inside `preflight` before the new diff is emitted.
- Streamed step patchback is provisional until the final `finalize -> session-check` closeout succeeds. The stream path improves document visibility; the binary-owned commit boundary is still the authoritative persistence step.
- CRDT streaming only flushes a provisional orchestrate step into `exchange` after the child output has started an `exchange` patch block. Raw transcript-shaped chunks without `<!-- patch:exchange -->` must not be flushed into the document.

### `--mode parallel`

Parallel orchestration resolves tasks through the shared orchestrate surface, then runs the existing worktree fan-out backend:

- `--task` / `--from-file` / `--from-exchange` are resolved first.
- Any requested prompt presets are expanded into the concrete per-task prompt before the parallel worker prompt file is written, while the user-facing task label stays unchanged.
- The resolved tasks are then passed to the existing parallel engine with `--model`, `--no-git`, `--no-worktree`, and `--timeout`.
- The legacy `agent-doc parallel` command is a compatibility wrapper over this same dispatch path with explicit `--task` entries only, so both surfaces share task normalization and mode routing.

### `--mode dag`

Dependency-aware orchestration against the same shared document lifecycle.

Task entries may include an optional metadata prefix before the real prompt:

```md
- do #prep. Prepare context
- [after=#prep] do #bench. Run benchmarks
- [id=report after=#prep,#bench] Summarize both results
```

Rules:

1. If metadata provides `id=...`, that becomes the node id.
2. Otherwise the first `#token` found in the prompt becomes the node id (for example `do #prep...` → `#prep`).
3. If neither exists, the node gets an implicit `step-N` id.
4. `after=` (alias `deps=`) lists comma-separated prerequisite ids.
5. Unknown dependencies, duplicate ids, and dependency cycles fail fast before any task executes.

Execution semantics:

- The graph is topologically sorted in deterministic source order.
- Each ready node still runs through the normal single-document lifecycle: inject prompt → `preflight` → fresh agent request → `finalize` → `session-check`.
- Requested prompt presets are expanded into each DAG node's concrete prompt before dispatch, so `sequential`, `dag`, and `parallel` share the same labeled preset block behavior.
- Because every node writes back to the same session document, DAG mode does **not** run siblings concurrently. Fan-in is supported through multiple `after=` dependencies; fan-out across isolated worktrees remains `--mode parallel`.
- `--no-git` is rejected for the same reason as sequential mode: persistence still flows through git-backed `finalize`.

## agent:queue component

The `<!-- agent:queue -->...<!-- /agent:queue -->` component batches multiple prompts in one block. The binary parses the body into a `Vec<QueueEntry>` (I/O-free; callers handle file reads/writes).

### Syntax

Items inside the block use a hybrid syntax:

**Single-line prompts** — bare `- ` list items (column 0, dash-space):

```markdown
<!-- agent:queue -->
- do #fix1
- do #fix2
- run tests
<!-- /agent:queue -->
```

**Multi-line prompts** — opened by `~~~prompt` or bare `---`, closed by the matching `~~~` or `---`:

```markdown
<!-- agent:queue -->
~~~prompt
Review the changes in src/agent-doc/src/frontmatter.rs
and check for edge cases with empty body content.
Then run cargo test.
~~~
<!-- /agent:queue -->
```

Both fence styles are equivalent inside `<!-- agent:queue -->` boundaries. Outside those boundaries, `---` retains its standard markdown meaning (thematic break / frontmatter delimiter).

**Control fences:**

| Syntax | Meaning |
|--------|---------|
| `--- start` / `~~~start` | Activation signal; consumed on first cycle |
| `--- start at <time>` / `--- start <time>` | Scheduled activation; defer until `<time>` |
| `--- stop` / `~~~stop` | Breakpoint; halts the queue when reached |

Blank lines between items are silently skipped. Any other content is a parse error.

### Data model (`queue.rs`)

```rust
pub enum QueueEntry {
    Prompt(QueuePrompt),        // single-line or multi-line prompt
    StartFence(Option<String>), // None = immediate; Some(time) = scheduled
    StopFence,
}

pub struct QueuePrompt {
    pub text: String,
    pub multiline: bool,  // true for ~~~prompt/--- fences
}
```

**`parse(body: &str) -> Result<Vec<QueueEntry>>`** — pure parser for the component body. I/O-free; caller reads the component body from the document.

### Activation resolution (`queue.rs`)

```rust
pub enum QueueTrigger { Auto, StartFence, ExchangeRequest, Persisted }
pub struct QueueActivation {
    pub active: bool,
    pub trigger: Option<QueueTrigger>,
    pub deferred: bool,
    pub start_at: Option<String>,
    pub consumed_start_fence: bool,
    pub entries_after: Vec<QueueEntry>,
}
pub fn resolve_activation(entries, has_auto, exchange_triggered, persisted_active) -> QueueActivation
```

Priority order: `auto` attribute > inline start fence > exchange `do queue`/`run queue` > persisted `queue_active: true`.

### Preflight queue fields

When the document has an `agent:queue` component, preflight computes queue state and emits:

| Field | Type | Description |
|-------|------|-------------|
| `queue_prompts` | `Vec<String>` | Ordered prompt texts (only when active) |
| `queue_active` | `Option<bool>` | Whether the queue is currently active |
| `queue_deferred` | `bool` | True when a time-gated start fence defers activation |
| `queue_start_at` | `Option<String>` | Raw datetime string from `--- start at <time>` |
| `queue_trigger` | `Option<QueueTrigger>` | How the queue was activated |
| `queue_halted` | `Option<String>` | Halt reason: `"stop_fence"` or `"item_modified"` |

Preflight performs these mutations before emitting queue state:
- **Consume bare start fence:** If `--- start` (no `at`) is at the head, remove it from the queue body and write back to document + snapshot.
- **Strip auto on drain:** If `auto` is set but no prompts remain, strip `auto` from the opening tag.
- **Persist queue_active:** Set/clear `queue_active` in frontmatter when activation state changes.
- **Halt on stop fence (Phase 3):** If `--- stop` is at the head, consume the fence, strip `auto`, clear `queue_active`, and emit `queue_halted: "stop_fence"`. No prompt is dispatched.
- **Halt on time gate (Phase 3):** If `--- start at <time>` is at the head and the time hasn't arrived, emit `queue_deferred: true` and skip the cycle.
- **Halt on item modified (Phase 3):** If the head prompt's text differs between snapshot and file (user edited it between cycles), strip `auto`, clear `queue_active`, and emit `queue_halted: "item_modified"`.

### Post-commit consumption (Phase 3)

After a successful response write (`finalize` or `write --commit`), required closeouts first run the remaining strict post-write gates (pending maintenance and strict pending guards). Only after those gates pass is the consumed prompt removed from the `agent:queue` block so the same git commit can capture both the response and the queue advance.

- Acquire the document advisory lock (`acquire_doc_lock`) before reading the file. The lock is held for the entire read-parse-write cycle to prevent concurrent edits from invalidating parsed offsets.
- Read frontmatter: if `queue_active != true`, skip.
- Parse the queue component body. If `queue_active: true` but the document queue is missing, malformed, or has no prompt at the head, required closeouts fail before commit.
- Remove the first `Prompt` entry via `remove_first_prompt()`.
- If queue drained (no prompts remain): strip `auto` from opening tag, set `queue_active: false` in frontmatter.
- Load and parse the snapshot queue too before mutating either side. The same head prompt must exist in both file and snapshot, and removing it must yield the same remaining queue state; otherwise required closeouts fail before commit.
- Only after both transforms are proven do we write the updated file and snapshot content.
- Required closeouts (`finalize`, session-document `write --commit`) must fail closed unless queue consumption can prove the first prompt was removed from both the file and the snapshot. Best-effort non-session `write --commit` still downgrades queue-consume errors to warnings.
- The commit that follows stages the snapshot (which now includes the consumed queue).
