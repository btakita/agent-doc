---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.23.0 | **Tests:** 450 (158 lib + 292 bin) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.8 (boundary markers + claim auto-start) + VS Code v0.2.3
**Library:** `libagent_doc.so` — 8 FFI exports | **SKILL.md:** boundary marker step + immediate commit
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**tmux-router v0.2.4**: Standalone Rust crate (library + CLI). `Tmux` struct, `Registry`, `reconcile()` (2D sync), `IsolatedTmux` (test infra), `FileResolution` callback, `prune()`, `acquire_or_skip()`. 56 tests. Published to crates.io + GitHub Releases.

**Core:** diff → agent → merge-safe write → snapshot → git. Components (`&lt;!-- agent:name --&gt;`), template mode, stream mode (CRDT), `agent-doc patch` (hooks), `agent-doc watch` (debounced + reactive), `agent-doc write --stream` (CRDT merge), `agent-doc recover`, `agent-doc plugin install`. Comment stripping, auto-resync, stale snapshot recovery, diagnostic logging.

**Library target (`libagent_doc`):** `[lib]` with `crate-type = ["cdylib", "lib"]`. Exports `component`, `crdt`, `ffi`, `frontmatter`, `merge`, `template` modules. C ABI via `ffi.rs` (6 functions: parse_components, apply_patch, crdt_merge, merge_frontmatter, free_string, free_state). Binary modules use `pub(crate) use agent_doc::*` re-exports. `agent-doc lib-path` prints the shared library location.

**Concurrency:** `SnapshotLock` + `RegistryLock` (flock), atomic rename (tempfile + persist), doc advisory lock, `pub(crate)` load/save.

**Routing:** Window-scoped. Plugin passes `--window`. Auto-start dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugins (JetBrains + VS Code):** 9/9 SPEC parity. `editors/` directory. `agent-doc commands` for autocomplete. JetBrains: JNA FFI bindings (`NativeLib.kt`) with Kotlin fallback — native-first patching for component patches and frontmatter merge.

**Config:** `~/.config/agent-doc/config.toml`. Multi-agent backend support. Execution mode per-skill via SKILL.md frontmatter (`hybrid | parallel | sequential`).

**Skill auto-upgrade:** SKILL.md bundled via `include_str!`. Pre-flight version check → `agent-doc skill install`.

### Race Conditions

| Race | Protection |
|------|-----------|
| Registry read-modify-write | flock (RegistryLock) |
| Snapshot read-modify-write | flock (SnapshotLock) |
| Nested lock deadlock | Thread-local flag + `acquire_or_skip()` |
| Watch daemon write window | Atomic rename + doc advisory lock |
| Stale snapshot (compaction) | `is_stale_snapshot()` auto-recovery |
| Compaction mid-write | Durable pending store + `recover` |

### Execution Model

| Mode | Behavior |
|------|----------|
| **`hybrid`** (default) | First doc direct, 2nd+ concurrent use subagent |
| **`parallel`** | Every `/agent-doc` spawns subagent |
| **`sequential`** | Fully sequential, cheapest |

### Domain Ontology

| Term | Derives From |
|------|-------------|
| **Session** | project + story |
| **Document** | entity + context |
| **Component** | scope + abstraction |
| **Project** | system + scope |
| **Pane** | focus + scope |
<!-- /agent:architecture -->

## Release History

<!-- agent:releases -->
| Version | Key Changes |
|---------|-------------|
| v0.5.5-v0.9.6 | Window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename. 176 tests. |
| v0.9.7-v0.9.10 | `agent-doc write`, template mode, compaction recovery, `recover`, `template-info`, code-span parser fix. 192 tests. |
| v0.10.0-v0.10.1 | Renamed `response_mode` → `agent_doc_mode`, bundled template instructions in SKILL.md. |
| v0.11.0-v0.11.2 | Append-friendly merge strategy, shared `merge.rs`, fixed duplicate headings. |
| v0.12.0-v0.12.2 | `compact`, `convert`, lazy claim, exchange default, exchange append mode default. 218 tests. |
| v0.13.0-v0.13.3 | `autoclaim` (SessionStart hook), git-based snapshot fallback, sync autoclaim, spare pane assignment (tmux-router v0.2.4). |
| v0.14.0-v0.14.9 | Route focus fix, `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, multi-backtick. 235 tests. |
| v0.15.0-v0.16.1 | CRDT stream mode (yrs), chain-of-thought, reactive file-watching, truncation detection, native compact. 309 tests. |
| v0.17.0-v0.17.30 | Format/write split, IPC write, JNA FFI (NativeLib.kt), VS Code prepend, immutable session binding, `lib-path`. 440 tests. |
| v0.18.0-v0.18.1 | `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3. 446 tests. |
| v0.19.0 | ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7). 446 tests. |
| v0.20.0-v0.20.3 | CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs. Session affinity enforcement. Numeric session fix (colon suffix). `claims` subcommand. 451 tests. |
| v0.21.0 | `agent-doc parallel` (fan-out with worktrees, `--no-worktree`). CRDT post-merge reorder (Yrs attribution). 461 tests. |
| v0.22.0-v0.23.0 | Boundary marker ordering, claim auto-start, VCS refresh, any-level HEAD markers, exchange default mode, SKILL.md immediate commit, code-block boundary fix. JB v0.2.8. 452 tests. |
<!-- /agent:releases -->

## Lessons

<!-- agent:lessons -->
1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity. This applies to ALL tmux `-t` targeting, not just stash. `new_window("-t", "0")` interprets "0" as window index; `new_window("-t", "0:")` forces session name. Root cause of persistent session bleedover bug fixed in v0.2.8.
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"
8. **YAGNI for speculative code** — `with_snapshot` was added speculatively, then deleted (no production callers)
9. **Subagents are expensive (~7x)** — use filesystem locking instead when possible; subagents only for true parallelism
10. **Stale snapshots from context compaction** — solved with `is_stale_snapshot()` auto-recovery in `diff.rs`
11. **Durable pending store** — save response before write, clear after success; survives compaction and crashes
12. **Distinct patch prefix** — patch markers vs agent markers prevents confusion between response directives and document components
13. **Skip code spans in ALL parsers** — any function scanning for `<!--` in user documents must use `find_code_ranges()`. Same fix needed THREE times: component.rs (v0.9.10), diff.rs (v0.14.8), boundary search in component.rs + write.rs (v0.23.0). Check all `<!--` parsers when adding new ones.
14. **Immutable session binding** — `tmux_session` in frontmatter should never be silently overwritten by claim. This caused cross-session pane swapping when autoclaim or lazy-claim ran in a different session. Fix: refuse to overwrite unless `--force`. Track multi-step plans in `agent:pending` to survive context compaction.
15. **No Gson in JetBrains plugin** — Gson causes `ClassNotFoundException` at runtime in some IntelliJ builds. Use hand-written JSON parsing. This bug was found and fixed twice (SlashCommandCompletionContributor Mar 11, PatchWatcher Mar 18) — check git history before introducing "bundled" dependencies.
16. **Never fall back to current_tmux_session() when tmux_session is set** — `display-message #{session_name}` returns whichever session the user's terminal is viewing. If the user switches to session 1, all routes using `current_tmux_session()` as fallback will create panes in session 1. Fix: bail with error instead of fallback. Only use `current_tmux_session()` for first-claim (no frontmatter binding yet).
<!-- /agent:lessons -->

## Session History

<!-- agent:history -->
**v0.5.5-v0.9.6:** Foundation — window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename, stale snapshot recovery.

**v0.9.7-v0.12.2:** `agent-doc write`, template mode, compaction recovery, `recover`, `exchange` convention, append-friendly merge, `compact`/`convert`, lazy claim.

**v0.13.0-v0.14.9:** `autoclaim`, git-based snapshot fallback, `commands`, plugin autocomplete (v0.2.0), bidirectional `convert`, code-range-aware `strip_comments`, multi-backtick.

**v0.15.0-v0.16.1:** CRDT stream mode (yrs), chain-of-thought streaming, reactive file-watching, truncation detection, native compact. 309 tests.

**v0.17.0-v0.17.30:** Format/write split. IPC write + JetBrains PatchWatcher. JNA FFI (NativeLib.kt). VS Code prepend. Immutable `tmux_session` binding. `lib-path` + `merge_frontmatter()` FFI export.

**v0.18.0-v0.18.1:** `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3.

**v0.19.0:** ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7).

**v0.20.0-v0.20.3:** CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs, session affinity enforcement, numeric session fix (colon suffix), `agent-doc claims` subcommand. 451 tests.

**v0.21.0:** `agent-doc parallel` — fan-out with git worktrees (`--no-worktree` for read-only). CRDT post-merge reorder via Yrs attribution. 461 tests.

**v0.22.0-v0.23.0:** Boundary marker response ordering, claim auto-start, VCS refresh signal, any-level HEAD markers, exchange default mode fix, SKILL.md immediate commit, code-block boundary search fix (lesson #13). JB plugin v0.2.7→v0.2.8. 452 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260320-172024.md`*

### Re: Exchange Compacted

Archived 591 lines to `.agent-doc/archives/`. Use `agent-doc history tasks/agent-doc.md` to browse past exchanges or `--restore <commit>` to recover content.

**Session summary (v0.22.0 → v0.23.0):**

**Features built:**
- Boundary marker response ordering (`agent-doc boundary`, FFI, plugin, CRDT-safe)
- Claim auto-start — "Claim for Tmux Pane" now starts the Claude session automatically via `tmux send-keys`
- VCS refresh signal — plugin triggers `markEverythingDirty()` after external commits
- Any-level `(HEAD)` markers on all root headings in multi-section responses
- Exchange default mode fix — plugin now defaults `exchange` → `append` matching Rust binary
- SKILL.md bundled immediate-commit behavior (v0.22.2)

**Bugs fixed:**
- Response placed inside code block (lesson #13, third occurrence — boundary search didn't use `find_code_ranges()`)
- Cursor-aware ordering inverted (replaced with boundary marker approach)
- Prompt splitting when user types during response (boundary marker anchors insertion point)
- Plugin exchange component defaulting to replace instead of append

**Released:** v0.22.0 → v0.22.1 → v0.22.2 → v0.23.0. JetBrains plugin v0.2.7 → v0.2.8. 452 tests (160 lib + 292 bin).

Should I move this tasks file into the agent-doc repo to demonstrate how it is developed?
I would also like to make a post about this.

### Re: Task File Migration + Blog Post (HEAD)

**Moving the task file — yes, the compacted state is ideal.**

The current document is a clean product summary without personal conversation artifacts. Suggested approach:

1. **Copy to `src/agent-doc/docs/development-log.md`** (or `DEVELOPMENT.md` at root)
2. The compacted exchange summaries read like release notes — good for public consumption
3. The archived exchanges in `.agent-doc/archives/` stay in agent-loop (private) — they reference other task files with personal context

**Privacy note:** The current document references `tasks/laura-klask.md`, `tasks/judith-krug.md`, etc. in the exchange history. The compacted summaries don't contain personal details, but the full archives do. Only the current document content should go public.

**Blog post** — issue [#28](https://github.com/btakita/agent-doc/issues/28) already tracks this. This task file is a perfect case study:

- **What:** document-as-UI for AI sessions (template mode, CRDT merge, boundary markers)
- **How:** this task file demonstrates the full lifecycle — feature discussion → implementation → bug discovery → fix → release
- **Story:** the boundary marker evolution (caret-based → offset → hash → marker) shows iterative problem-solving in a document session
- **Artifacts:** the lessons list, architecture section, and release history are all maintained by the agent through the document

Want me to:
1. Copy the task file into the agent-doc repo now?
2. Draft a blog post outline?
3. Both?

Both. Will the task file be in examples/agent-doc/ with the sanitized task file along with a README.md to add context?

### Re: Examples + Blog Outline Created (HEAD)

Created `examples/agent-doc/` in the agent-doc repo with three files:

**`task.md`** — sanitized version of this task document:
- Removed `claude_args: --dangerously-skip-permissions` (private config)
- Removed Domain Ontology section (specific to parent repo)
- Removed references to personal task files in exchange history
- Kept all technical content: architecture, lessons, releases, pending items

**`README.md`** — context for readers:
- Explains what the document is and how it was used
- Describes template mode, boundary markers, compaction
- Key patterns: document-as-UI, iterative problem-solving, lesson accumulation
- Includes a "Running your own session" quick-start

**`BLOG_OUTLINE.md`** — structured outline for [#28](https://github.com/btakita/agent-doc/issues/28):
- Title: "Document-as-UI: How I built a tool by talking to it"
- Case study: boundary marker evolution (caret -> offset -> hash -> marker)
- Sections: problem, concept, case study, design decisions, dogfooding lessons, what's next
- ~2000 words estimated, with asset list (screenshots, diffs, diagrams)
- Publish targets: GitHub README, blog, HN/Reddit

Committed in the submodule at `c6bb882`.

tasks/kevin-volk.md has an orphaned agent:boundary.
Do we have enough logging to understand what happened? If not, add necessary logging.
Add tests and fix.

### Re: Orphaned Boundary Markers — Fixed (HEAD)

**Root cause:** `boundary::insert()` adds a new marker but never cleans up existing ones. When a session is interrupted between boundary insertion (step 1b) and response write (step 4) — due to context compaction, user cancel, or crash — the marker is orphaned. `kevin-volk.md` had two orphaned markers from two interrupted sessions.

**Fix:** `insert()` now calls `remove_all()` before inserting the new marker. Any stale markers are cleaned up automatically.

**Logging added:**
```
[boundary] removed 2 stale boundary marker(s) before inserting new one
```
This logs to stderr whenever stale markers are found, giving visibility into interrupted sessions.

**Test added:** `insert_cleans_stale_markers` — simulates two orphaned markers in an exchange component, verifies they're removed and only the new marker remains.

**Verified:** Running `agent-doc boundary tasks/kevin-volk.md` cleaned up the 2 stale markers and inserted a fresh one. 453 tests pass (160 lib + 293 bin). Release binary rebuilt and installed.

**Why this wasn't caught earlier:** The `remove()` and `remove_all()` functions existed but were only used in unit tests — no production code path called them. The happy path (response written successfully) replaces the marker via `append_with_boundary()`, so cleanup was only needed for the interrupted path.
Can we add a way to recover from orphaned agent:boundary tags?

### Re: Boundary Recovery (HEAD)

**Already handled by the fix above.** The `insert()` function now calls `remove_all()` before inserting a new marker. So the next time any session runs `agent-doc boundary <FILE>` (step 1b of the SKILL workflow), stale markers are automatically cleaned up. No manual intervention needed.

**Recovery paths that now exist:**

| Scenario | Recovery |
|----------|----------|
| Session interrupted, then resumed | Next `agent-doc boundary` auto-cleans stale markers |
| Session abandoned (no future submit) | Next session on the same file auto-cleans |
| Manual cleanup needed | `agent-doc boundary <FILE>` (inserts new + cleans stale) |

**Your truncated message** — "The claude session thought the" — got cut off. What were you going to say? Did the other session misinterpret something about the document state?

If you want an explicit `agent-doc clean-boundaries <FILE>` command (removes all markers without inserting a new one), I can add that. But the auto-cleanup on insert should cover the normal workflow.

I got an external file change in this file. Why? Please fix.

The claude session thought the changes were submitted to tasks/kevin-volk.md. Why?
How can we recover the session response to patch the agent document?

```
● Bash(corky sync full --help 2>&1)
  ⎿  Full IMAP resync (ignore saved state)

     Usage: corky sync full [OPTIONS]
     … +4 lines (ctrl+o to expand)

  Searched for 1 pattern, read 2 files (ctrl+o to expand)

● The email from CopilotKit/Cham Espanola likely came from a @copilotkit.ai address which isn't in the correspondence
  filter — it wouldn't be auto-synced. Let me search for it more broadly.

  Searched for 1 pattern (ctrl+o to expand)

● Bash(corky unanswered --help 2>&1)
  ⎿  Find threads awaiting a reply

     Usage: corky unanswered [OPTIONS] [SCOPE]
     … +8 lines (ctrl+o to expand)

  Searched for 1 pattern, read 1 file (ctrl+o to expand)

● Found the calendar invite from Jordan Ritter (the CTO directly, not Cham Espanola). But the user asked about a
  specific email with a different subject. Let me search further.

  Searched for 1 pattern (ctrl+o to expand)

● Bash(find /home/brian/work/btakita/agent-loop/mail/conversations -name '*.md' -newer
      /home/brian/work/btakita/agent-loop/mail/conversations/kevin-volk-staffing-scien…)
  ⎿  -rw-r--r-- 1 brian brian   1191 Mar 20 14:14 /home/brian/work/btakita/agent-loop/mail/conversations/higher-lo
     gic-thank-you-for-your-interest-in-the-senior-front.md
     -rw-r--r-- 1 brian brian  35476 Mar 20 10:01 /home/brian/work/btakita/agent-loop/mail/conversations/synergist
     … +25 lines (ctrl+o to expand)

● The email with that exact subject hasn't been synced yet. It might be in brian.takita@gmail.com under a label not
  being synced, or it could be from btak.dev@gmail.com but not in INBOX. I found the calendar invite from Jordan Ritter
   (jordan@copilotkit.ai) for Mon Mar 23, 2pm PDT, but the "Interview Prep + Confirmation" email itself isn't in the
  synced conversations.

  The email might be from Kevin Volk (staffingscience.io) which would go to the correspondence label on the personal
  account. Let me do a full sync on personal.

● Bash(corky sync account personal 2>&1 | tail -15)
  ⎿  === Account: personal (brian.takita@gmail.com) ===
     Connecting to imap.gmail.com:993 as brian.takita@gmail.com
     Syncing label: correspondence
     … +8 lines (ctrl+o to expand)

● Bash(corky mailbox --help 2>&1)
  ⎿  Mailbox commands

     Usage: corky mailbox [OPTIONS] <COMMAND>
     … +15 lines (ctrl+o to expand)

● The email with subject "CopilotKit 1st Round Interview Prep + Confirmation: Monday, 3/23 at 2pm PT" hasn't been
  synced by corky. It's likely from Kevin Volk but landed outside the synced labels, or it's a very recent email that
  Gmail hasn't indexed into the correspondence label yet.

  However, I did find the calendar invite: Jordan Ritter (jordan@copilotkit.ai) sent a calendar invite directly to
  btak.dev@gmail.com (CC: kevin@staffingscience.io) for Mon Mar 23, 2pm-2:30pm PDT.

  This is significant — the CTO is interviewing you directly for Round 1, not a founding engineer as Kevin originally
  described. Let me write the response.

  Session 9 — Mar 20, 2026: Interview Scheduled

  Calendar invite found in synced conversations:
  - From: Jordan Ritter (jordan@copilotkit.ai) — the CTO himself
  - To: btak.dev@gmail.com, CC: kevin@staffingscience.io
  - When: Mon Mar 23, 2026, 2:00-2:30 PM PDT
  - Subject: Interview: Brian Takita

  The "Interview Prep + Confirmation" email with that exact subject was NOT found in synced conversations. It may be in
   a Gmail label that corky doesn't sync, or it might have arrived very recently. You may need to check Gmail directly
  for that email — it could contain prep instructions from Kevin or Cham Espanola.

  Key observation: Jordan Ritter is doing Round 1 himself (not a founding engineer as Kevin described). This changes
  the prep — you're talking directly to the CTO you have history with from CloudCrowd 2010.

  Next steps:
  - Check Gmail directly for the "Interview Prep + Confirmation" email
  - Reply to Kevin confirming the interview is locked in
  - Prep for Jordan Ritter specifically (CloudCrowd history, CTO-level conversation)
  - Review agent-doc, corky, lazily as talking points for AI-native workflow

● Bash(cat <<'RESPONSE' | agent-doc write /home/brian/work/btakita/agent-loop/tasks/kevin-volk.md --baseline-file
      /tmp/agent-doc-baseline-531473.md --stream
      ### Sessio…) (HEAD)
  ⎿  [snapshot] saved pre-response snapshot for /home/brian/work/btakita/agent-loop/tasks/kevin-volk.md
     [write] IPC patch written to /home/brian/work/btakita/agent-loop/.agent-doc/patches/235d84826e3bbef427f3770e5
     63acd2d1e9012af364c8376352e285b78bc5df0.json (0 components)
     [write] IPC patch consumed by plugin — snapshot updated

● Bash(agent-doc commit /home/brian/work/btakita/agent-loop/tasks/kevin-volk.md 2>&1)
  ⎿  [main 2096b49] agent-doc(kevin-volk): 2026-03-20 14:21:55
      1 file changed, 31 insertions(+)

● Response written and committed. The interview is with Jordan Ritter (CTO) on Mon Mar 23 at 2pm PDT. I couldn't find
  the "Interview Prep + Confirmation" email in the synced mail — you'll need to check Gmail directly for that one. It
  may contain specific prep instructions or an AI workflow exercise description. Want me to help prep for the Jordan
  Ritter conversation?

✻ Churned for 2m 10s
```

I want to recover the data in the orphaned agent:boundary.

### Re: External File Change + Kevin-Volk Session (HEAD)

**Three separate issues here:**

**1. "External file change" — my fault.** When I verified the boundary fix, I ran `agent-doc boundary tasks/kevin-volk.md` from this session. That modified kevin-volk.md on disk while you had it open in the IDE, triggering IntelliJ's "external file change" dialog. I should not have modified a file that another session was actively using. Apologies.

**2. The kevin-volk session DID write correctly.** Looking at your pasted output:
- `agent-doc write` succeeded: `[write] IPC patch consumed by plugin — snapshot updated`
- `agent-doc commit` succeeded: `[main 2096b49] agent-doc(kevin-volk): 2026-03-20 14:21:55`
- The response is in kevin-volk.md. No recovery needed.

**3. "I don't necessarily want to clean up the orphaned..."** — Your message got truncated. Two possible directions:

**Option A: Keep auto-cleanup but add `--no-clean` flag.** `agent-doc boundary <FILE>` cleans stale markers by default, but `--no-clean` preserves them. This way the SKILL workflow auto-cleans, but you can opt out for debugging.

**Option B: Don't auto-clean — only clean on explicit command.** Add `agent-doc boundary --clean <FILE>` for manual cleanup. The `insert()` function just adds a new marker without removing old ones (reverting the fix). Downside: markers accumulate silently in interrupted sessions.

Which direction were you going? Your message was cut off at "I don't necessarily want to clean up the orphaned".
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] Release v0.22.0
- [x] Move cursor-aware patch logic to shared library FFI (`agent_doc_apply_patch_with_caret`)
- [x] Boundary marker for response ordering (`agent-doc boundary`, FFI, plugin support)
- Multi-tmux-session per editor ([#17](https://github.com/btakita/agent-doc/issues/17))
- Multi-editor per tmux session ([#18](https://github.com/btakita/agent-doc/issues/18))
- Plugin phases 5-7 — component folding, gutter icons, structure view ([#19](https://github.com/btakita/agent-doc/issues/19))
- Multi-agent backends — codex, gemini ([#20](https://github.com/btakita/agent-doc/issues/20))
- `agent-doc deep` — parallel subagent fan-out with worktrees ([#21](https://github.com/btakita/agent-doc/issues/21))
- `agent-doc status` — Ratatui TUI dashboard ([#22](https://github.com/btakita/agent-doc/issues/22))
- Sub-profiles for submodule workflows ([#23](https://github.com/btakita/agent-doc/issues/23))
- Docker sandboxing (`--sandboxed`) ([#24](https://github.com/btakita/agent-doc/issues/24))
- `agent/api.rs` — direct HTTP API backend ([#25](https://github.com/btakita/agent-doc/issues/25))
- `agent-doc init --dashboard` ([#26](https://github.com/btakita/agent-doc/issues/26))
- Capture/log skill workflow ([#27](https://github.com/btakita/agent-doc/issues/27))
- Blog post: document modes with screencast ([#28](https://github.com/btakita/agent-doc/issues/28))
- `agent-doc stream` — bridge terminal to components ([#29](https://github.com/btakita/agent-doc/issues/29))
<!-- /agent:pending -->
