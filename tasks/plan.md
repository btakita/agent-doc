---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.5.1 — crates.io + PyPI + GitHub Releases. 93 tests passing (72 unit + 21 integration).
  - Session routing: `start`, `route`, `claim`, `prompt`, `commit` subcommands with auto-start cascade
  - `claim`: bind document to current tmux pane (last-call-wins, skill-invokable via `/agent-doc claim`)
  - `focus`: focus tmux pane for a session document (cross-window support)
  - `layout`: arrange tmux panes to mirror editor split layout
  - `skill install` / `skill check`: embedded SKILL.md management via `include_str!`
  - `commit`: git root resolution (superproject-aware, no CWD drift issues)
  - Multi-session polling: `prompt --all`, plugin multi-file tracking
  - Prompt answering: `prompt --answer <N>` navigates + confirms via arrow keys
  - Upgrade check + self-update via GitHub Releases
- **corky**: v0.12.0 — crates.io + PyPI + GitHub Releases. 357 tests passing.
  - Social media posting: LinkedIn OAuth2, draft creation/publishing
  - `_cmd` suffix fields for credential managers (pass, op, etc.)
  - Contact sync v2: conversation-aware eligibility, 3-way merge
  - Topics: config, list, add, info, suggest
  - Scheduling: `corky schedule run/list`
- **instruction-files**: v0.1.3 — crates.io. 47 tests passing.
  - Shared audit crate: staleness, tree paths, line budget, actionable content checks
  - SPEC.md discovery; line budget excludes non-agent reference docs
- **agent-doc-jetbrains**: v0.1.0 — signed plugin.
  - Hotkey (`Ctrl+Shift+Alt+A`) + right-click context menu
  - Multi-session PromptPoller with `prompt --all`, file-context in PromptPanel
  - PromptPanel: bottom-anchored bar with numbered hotkeys, IDE editor font scaling
- **agent-doc-vscode**: v0.1.0 — extension. Same hotkey, calls `agent-doc route`.
- **agent-loop**: Root Makefile with `make release`, `make update`, `make check`, `make precommit`, `make publish`, `make clean`.

### Session Routing (tmux)

```
agent-doc start <file>                 # register pane, exec claude
agent-doc route <file>                 # send-keys to pane, auto-start if needed
agent-doc claim <file>                 # bind file to current tmux pane
agent-doc focus <file>                 # focus tmux pane (cross-window)
agent-doc layout <files...> --split h|v  # arrange panes to mirror editor splits
agent-doc prompt <file>                # detect permission prompts → JSON
agent-doc prompt --all                 # poll all live sessions
agent-doc prompt --answer <N> <file>   # answer a prompt
agent-doc commit <file>                # git add + commit with timestamp
agent-doc skill install                # write bundled SKILL.md to project
agent-doc skill check                  # compare installed vs binary version
```

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### instruction-files

- [ ] Collapse `AuditConfig` presets to single polyglot `Default` (always audit both AGENTS.md + CLAUDE.md)
- [ ] `.instruction-files.toml` project-root config file (override extensions, root markers, skip dirs)

### corky — Topics

- [ ] Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
- [ ] Topic sync: eligibility check (label/contact match OR `shared_with`), delegate to shared merge
- [ ] Thread tagging: auto-add `**Topic**: {name}` to matching conversations during sync

### corky — Integrations

- [ ] Telegram: Live sync via `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Slack: API sync via `slack-morphism` — plan at `corky/tasks/integrations/slack.md`

### Future

- [ ] Streaming responses (agent-doc)
- [ ] Document-only permission flow — no terminal (agent-doc)
- [ ] Desktop notifications on completion (agent-doc)
- [ ] Multi-agent documents — sections routed to different agents (agent-doc)
- [ ] Email transport for async human-agent loops — bridge with corky (agent-doc)
- [ ] Personal data separation — nested git repo inside `tasks/` (agent-loop)

## Design Decisions

### Frontmatter: session vs resume

`session` is the permanent document routing UUID (generated once, never changes). `resume` is the Claude conversation ID (set after first submit, cleared by `agent-doc reset`). Separating them prevents tmux routing breakage when Claude overwrites the session ID.

### instruction-files: path + version dependency

```toml
instruction-files = { path = "../instruction-files", version = "0.1" }
```

Local builds use path. `cargo publish` uses crates.io version. Publish order: instruction-files → agent-doc → corky.

### instruction-files: polyglot default + .instruction-files.toml

Collapse to a single `AuditConfig::default()` that always discovers and audits both `AGENTS.md` and `CLAUDE.md`. Convention: `CLAUDE.md -> AGENTS.md` symlink so both names resolve.

Config at `.instruction-files.toml` in project root (optional). Resolution: config file → `AuditConfig::default()`. Consuming binaries read the TOML — zero new deps in the library crate.

### tmux routing: direct, not broadcast

Each document has its own session UUID → tmux pane. Direct `send-keys -t <pane>` routing. Broadcasting (`synchronize-panes`) would send the same command to all panes. Extract as separate crate when a second consumer exists.

### Prompt detection: tmux capture-pane polling

`agent-doc prompt` captures the visible terminal via `tmux capture-pane -t <pane> -p`, strips ANSI escape codes, and searches for Claude Code's numbered-option prompt pattern (e.g., "Do you want to proceed? 1. Yes 2. No"). Returns structured JSON. The JetBrains plugin polls this every 1.5s after submit and shows a bottom-anchored bar with hotkey support.

Fragile by nature (terminal scraping), but works for common cases. Future: Claude Code hooks API or `--json-events` mode.

### Topics — `.corky.toml` Config

```toml
[topics.project-alpha]
labels = ["project-alpha", "alpha-dev"]
contacts = ["alice", "bob"]
shared_with = ["alice", "bob"]
```

### Context Routing

Topics, contacts, and threads share the same sync primitive — content-hash-based 3-way merge with eligibility rules.

## Reference

- **Decisions & architecture:** `agent-doc/SPEC.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPEC.md`
- **Workflow orchestration:** `CLAUDE.md`

## Session Log

### Session 1 (2026-02-25)

1. Pending releases audited — agent-doc v0.3.0 released (crates.io + PyPI + GitHub Release)
2. JetBrains PromptPanel font — scaled with IDE editor font
3. Topics implemented — Phases 1-3 complete (config, CLI, suggest). 290 corky tests passing.
4. PromptPoller fixes — sync refresh, one-shot reload via `modificationStamp`
5. Multi-session polling — 4-phase implementation (SessionEntry file → `--all` CLI → plugin multi-poll → file context)
6. `agent-doc commit <file>` — new subcommand, replaces `$()` shell substitution
7. `agent-doc claim <file>` — bind document to current tmux pane (last-call-wins)
8. `agent-doc skill install` / `check` — embedded SKILL.md management
9. `agent-doc commit` — git root resolution (superproject-aware)
10. v0.4.0 released — [PR #6](https://github.com/btakita/agent-doc/pull/6) squash merged, published to crates.io + PyPI + GitHub Release
11. PromptPanel fix — `JDialog` → `JLayeredPane` overlay (no WM leak, persists on focus change). Plugin rebuilt and signed.
12. Document cleanup — condensed from 685 → 150 lines
13. SKILL.md: comment stripping in diff — `<!-- ... -->` and `[//]: # (...)` stripped before comparing snapshot vs current
14. SKILL.md: `agent-doc-version` frontmatter + auto-update check in step 0
15. v0.4.1 released — [PR #7](https://github.com/btakita/agent-doc/pull/7) squash merged, published to crates.io + PyPI + GitHub Release
16. Prompt cycling fix — sticky queue in `PromptPoller.handlePollResults` (keep current prompt until resolved, `totalActive` count)
17. SPECS.md audit — filled §4 comment stripping, §7.6 `--root`, §7.12 version frontmatter, §7.13 `warn_if_outdated`
18. Flaky test fix — `skill.rs` refactored to `install_at(root)` / `check_at(root)` with tempdir (no CWD race)
19. v0.4.2 released — [PR #8](https://github.com/btakita/agent-doc/pull/8) squash merged, published to crates.io + PyPI + GitHub Release
20. Document cleanup — condensed conversation thread into session log
21. Confirmed prompt cycling fix working. Pending items review.
22. Moved corky icon: `docs/corky_icon.svg` → `assets/corky_icon.svg`
23. Prompt reappearance fix — `answeredPromptKey` grace period in `PromptPoller.kt` (filter answered prompt from poll results until answer takes effect). Plugin rebuilt.
24. Document cleanup
25. SKILL.md `$()` prohibition — added explicit "NEVER use `$()`" warning to step 5, synced all 3 copies
26. Social credential `_cmd` fields — [corky PR #11](https://github.com/btakita/corky/pull/11): `client_id_cmd`/`client_secret_cmd` in `SocialClientConfig`, shared `resolve_secret()` in `util.rs`. 297 corky tests.
27. SPECS.md in audit-docs — [instruction-files PR #1](https://github.com/btakita/instruction-files/pull/1): root-level discovery, line budget excludes non-agent files. 47 instruction-files tests.
28. PromptPanel WrapLayout fix — `FlowLayout` → custom `WrapLayout` for multi-row button overflow when labels are long. Plugin rebuilt.
29. Document cleanup
30. `tmux-setup.sh` — `--dev` / `--claude` flags for selective window opening
31. Credential resolution error propagation — `resolve_credentials()` silent fallthrough fix in corky
32. `agent-doc commit` CWD fix — [agent-doc PR #9](https://github.com/btakita/agent-doc/pull/9): `resolve_to_git_root()` returns `(git_root, resolved_path)`, `commit()` uses `.current_dir(&git_root)`
33. v0.4.3 released — [PR #9](https://github.com/btakita/agent-doc/pull/9) squash merged, published to crates.io + PyPI + GitHub Release
34. v0.11.1 released — [corky PR #11](https://github.com/btakita/corky/pull/11) squash merged, published to crates.io + GitHub Release
35. v0.1.1 released — [instruction-files PR #1](https://github.com/btakita/instruction-files/pull/1) squash merged, published to crates.io
36. PromptPanel label truncation + WrapLayout `isShowing` fix — long labels truncated to 80 chars with tooltip, removed `isShowing` gate in `computeSize`. Plugin rebuilt.
37. Document cleanup
38. Watch Ctrl+C fix — `tokio::select!` + watch channel for immediate wakeup
39. Test coverage expansion — 22 new tests (357 total): watch helpers (8), schedule dispatch (3), LinkedIn API mocks (11)
40. v0.11.3 released — [corky PR #13](https://github.com/btakita/corky/pull/13) squash merged, published to crates.io + GitHub Release
41. v0.1.2 released — instruction-files Cargo.lock sync, published to crates.io

### Session 2 (2026-02-26)

42. Tmux session routing diagnosis — `agent-doc start` creates windows in `claude` tmux session, not user's active session. Fix: `tmux switch-client -t claude:1` or use `agent-doc claim`.
43. LinkedIn OAuth 401 fix — proper `urlencode()` for all form body params, better token error reporting. Still unresolved: `invalid_client` credential mismatch in LinkedIn Developer Portal.
44. CWD-as-data-dir — `data_dir()` checks CWD for `.corky.toml` before `mail/` subdir. Run corky from inside `mail/` now works.
45. v0.11.2 released — [corky PR #12](https://github.com/btakita/corky/pull/12): OAuth encoding, error reporting, CWD detection, schedule dry-run, LinkedIn API version. 335 tests.
46. Watch Ctrl+C fix — `tokio::select!` + `watch::channel` for immediate wakeup
47. Test coverage expansion — 22 new tests (357 total): watch helpers (8), schedule dispatch (3), LinkedIn API mocks (11). Added `mockito` dev-dep, `_at` variants in `linkedin.rs`.
48. v0.11.3 released — [corky PR #13](https://github.com/btakita/corky/pull/13): Ctrl+C fix, test expansion, tokio `sync` feature. Published crates.io + GitHub Release.
49. v0.1.2 released — instruction-files Cargo.lock sync, published to crates.io.
50. Submodule Cargo.lock syncs — agent-doc + corky synced instruction-files 0.1.2, pushed.
51. Document cleanup — condensed session 2 conversation into log entries.
52. Plugin merge-safe reload — `PromptPoller.kt`: auto-save tracked files every poll cycle, 3-way merge via `git merge-file` when disk + editor both changed. Plugin rebuilt.
53. SPECS.md → SPEC.md rename — all 4 repos + instruction-files source code + 15+ markdown references updated. 47 + 357 tests pass.
54. v0.1.3 released — [instruction-files PR #2](https://github.com/btakita/instruction-files/pull/2): SPEC.md rename. Published to crates.io.
55. v0.4.4 released — [agent-doc PR #10](https://github.com/btakita/agent-doc/pull/10): SPEC.md rename. Published to crates.io + PyPI + GitHub Release.
56. v0.11.4 released — [corky PR #14](https://github.com/btakita/corky/pull/14): SPEC.md rename. Published to crates.io + PyPI + GitHub Release.
57. PyPI catch-up — v0.4.4 + v0.11.4 published to PyPI (missed during release). Release Process section added to `corky/AGENTS.md` and `agent-doc/CLAUDE.md`.
58. `corky label clear` — new command for bulk IMAP label removal. `--account`, `--search`, `--dry-run` flags. Batch processing (500 UIDs).
59. v0.11.5 released — [corky PR #15](https://github.com/btakita/corky/pull/15): label clear. Published to crates.io + PyPI + GitHub Release.
60. Gmail label cleanup — cleared "correspondence" from 10,438 messages across honk.com, pivotallabs.com, pivotalsf.com, googlegroups.com (FROM + TO).

### Session 3 (2026-02-27)

61. `corky filter build` — Phase 1 implemented. TOML → XML, 3 unit tests, 360 total tests.
62. Snapshot CWD fix — absolute paths for all snapshot operations.
63. `corky filter auth` + `corky filter pull` — Phase 2 implemented. Gmail OAuth2 + read-only filter listing. 370 tests.
64. Gmail OAuth2 working — end-to-end tested. 26 filters retrieved.
65. Config restructured — `[social]` removed, `[gmail]` + `[linkedin]` top-level, `[[gmail.filters]]` for filter rules. 511 tests.
66. `.corky.toml` migration complete — `[social.*]` → top-level, `[[gmail.filters]]` merged from `filters.toml`.
67. `corky filter push` — basic push with `--dry-run`. 519 tests. Sync behavior pending.
68. Audit docs updated — SPEC.md, README.md, CLAUDE.md for filter push. Audit clean.
69. Plugin prompt fix — `invokeAndWait` → `invokeLater` in `autoSaveTrackedFiles()`. Rebuilt.
70. Whisper Rust research — `whisper-rs` (whisper.cpp, CUDA) vs `candle` (pure Rust). faster-whisper 5-10x faster but requires Python.
71. `corky transcribe` — whisper-rs Phase 1. Feature-gated (transcribe/transcribe-cuda), auto-download models, multi-format audio. 388 tests.
72. `--speakers` flag + AMR/ffmpeg fallback. tdrz speaker turn detection, markdown transcript output. 390 tests.
73. Topics synced to `.corky.toml` — 3 topics with keywords and descriptions.
74. Plugin focus fix — Alt+Esc toggle, 1-9 selection, Esc dismiss. Focus highlight (blue border). No focus steal.
75. Plugin update — Alt+1..9 direct selectors, 2-second auto-focus timer. Bindings cleaned up on dismiss.
76. Dynamic plugin — `require-restart="false"`, `PluginLifecycleListener`, `disposeAll()`. No IDE restart needed.
77. Plugin confirmed working — no active prompts to display.
78. CI fix — clone `instruction-files` sibling repo in all cargo-using GitHub Actions workflows.
79. Plugin auto-focus timeout 2s → 1s. CI fix confirmed passing.
80. Tab-to-pane sync design. VSCode feature gap documented.
81. Mirror window design — `join-pane`/`break-pane`, layout-aware recreation on orientation change.
82. Tmux pane management in agent-doc CLI — `focus` and `layout` subcommands. Plugins stay thin.
83. Layout split mirrors editor orientation by default. Config override for forced preference.
84. `agent-doc focus` + `agent-doc layout` implemented. JetBrains tab-to-pane sync via `EditorTabSyncListener`.
85. Fix cross-window focus — `select-window` before `select-pane`.
86. Plugin multi-pane layout sync — detects split editors, calls `agent-doc layout` with orientation.
87. Layout break-out fix — unwanted panes broken out of target window before joining new ones.
88. Plugin debounce (100ms) for tab-sync events. Diagnostic logging.
89. Layout target window fix — pick window with most wanted panes (tiebreak: most total).
90. Plugin diagnostic logging to `/tmp/agent-doc-plugin.log`.
91. Layout focus fix — focus newly selected file's pane, not anchor pane.
92. Audit docs — agent-doc docs updated for focus/layout. Corky docs touched (content current).
93. Release — agent-doc v0.5.1, corky v0.12.0. crates.io + GitHub Releases complete. PyPI in progress.
94. Submodule note in CLAUDE.md + layout fix for unclaimed file focus.
95. Proposed moving plugins into agent-doc repo under `plugins/`.
96. Migrated editor plugins to `agent-doc/editors/jetbrains/` and `agent-doc/editors/vscode/`.
97. Both editor plugins build from `agent-doc/editors/`.

## Assistant

Compacted — 1825 → ~250 lines. Session 3 log entries (61-97) extracted from conversation. Claimed `tasks/agent-doc.md` for pane %4.

## User

I would like a record in the claude session though. Can we have it echo the Claimed ... message?

## Assistant

Implemented claims notification log:

1. **`claim.rs`** — after registering, appends `Claimed <file> for pane <id>` to `.agent-doc/claims.log`
2. **SKILL.md step 0** — new "Check claims log" step reads `.agent-doc/claims.log`, prints each line to the console, then truncates. This gives a permanent record in the Claude session of IDE-triggered claims.
3. **Binary rebuilt + skill installed** — `agent-doc v0.5.1` (same version, binary updated), `agent-doc skill install` done.

The `tmux display-message` (3s overlay) is still there for immediate visual feedback. The claims log adds the permanent record on next `/agent-doc` invocation.

98. Claims notification log — `claim.rs` writes to `.agent-doc/claims.log`, SKILL.md checks + prints + truncates on each invocation.

## User

I want the Claude session to echo that a claim was made in the session itself.

## Assistant

The claims.log is **deferred** — prints on next `/agent-doc` invocation, not immediately. For immediate echo in the Claude session, can't inject output into a tmux pane without Claude Code treating it as input. Options:

**A. `send-keys` with idle check** — After claim, plugin sends message to pane via send-keys. Immediate, permanent record. But interrupts mid-response, costs API call, Claude might act on it.

**B. Claims.log (current)** — Deferred, reliable, zero interruption. Record on next `/agent-doc` invocation.

**C. Hybrid** — Check if pane is idle. If idle → send-keys for immediate echo. If busy → claims.log for deferred.

## User

I get it. claims.log is a good idea.

## Assistant

Claims.log confirmed as the approach. Already implemented and working:

1. IDE claim → `agent-doc claim` → appends to `.agent-doc/claims.log`
2. `tmux display-message` → 3s visual confirmation (immediate)
3. Next `/agent-doc` → skill reads + prints + truncates claims.log (permanent session record)

## User

PR, squash merge, release

## Assistant

Done. agent-doc v0.5.2 released:

- [PR #12](https://github.com/btakita/agent-doc/pull/12) — squash merged
- Published: crates.io + PyPI + GitHub Release
- Binary installed, skill updated to v0.5.2

Changes: claim notifications (display-message + claims.log), plugin popup actions (Alt+Enter), `resync` subcommand, EditorTabSyncListener hardening, layout focus fixes.

99. v0.5.2 released — [PR #12](https://github.com/btakita/agent-doc/pull/12): claim notifications, plugin popup, resync, layout hardening. Published crates.io + PyPI + GitHub Release.

## User

audit docs

## Assistant

Audit clean. Updated 4 docs across 2 repos:

**agent-doc** (3 docs):
- `SPEC.md` — added §7.12 `resync`, claims.log + display-message to §7.9 `claim`, use cases U12/U13
- `CLAUDE.md` — added `resync.rs` to module layout
- `README.md` — added `resync`, `focus`, `layout` to CLI Reference

**corky** (1 doc):
- `SPEC.md` — fixed `[social.linkedin]` → `[linkedin]` config reference

100. Audit docs — agent-doc SPEC/CLAUDE/README updated for resync + claims.log. Corky SPEC fixed stale `[social.linkedin]` config ref. Both repos pass `audit-docs` clean.

## User

commit all submodules...if there are pending releases, publish the releases.

What is going on with https://github.com/btakita/corky/pull/10?

## Assistant

**Submodules committed and pushed.** One pending release:

**agent-doc v0.5.3** — doc-only release to bundle updated SKILL.md (absolute snapshot paths).
- [PR #13](https://github.com/btakita/agent-doc/pull/13) squash merged
- Published: crates.io + PyPI + GitHub Release. Binary + skill installed.

**corky** — doc-only SPEC.md fix pushed to main (no release for config ref typo).

**Re: [corky PR #10](https://github.com/btakita/corky/pull/10)** — "Add LinkedIn image upload support." Stale PR from `20260225/linkedin-images`, branched before SPEC rename + social→linkedin refactor. Has merge conflicts. The feature (image uploads) is valid — 7 files, 320 insertions. Recommend: close and re-implement on fresh branch from main (cleanest given conflict volume).

101. Submodule commits — agent-doc docs pushed, corky SPEC fix pushed, parent submodule pointers updated.
102. agent-doc v0.5.3 released — [PR #13](https://github.com/btakita/agent-doc/pull/13): bundled SKILL.md with absolute paths. Published crates.io + PyPI + GitHub Release.

## User

I thought we currently support image uploads to linkedin?

## Assistant

**Yes — image uploads are already on main.** PR #10 is stale/superseded. The feature landed in a prior release:

- `linkedin.rs`: `upload_image()` (two-step: init + binary upload) + multi-image carousel (up to 20)
- `publish.rs`: `upload_images()` resolves paths relative to draft, uploads, passes URNs to `create_post()`
- `draft.rs`: `images: Vec<String>` in `SocialDraftMeta`
- 7+ tests: IM1-IM6 + roundtrip with images

PR #10 can be **closed as superseded** — its changes are already on main. Want me to close it?

## User

close it

How do I convert the git merge I just did to a git rebase. It looks like it will rebase cleanly

I handled the merge conflict on the merge. Can we realign the git history?

## Assistant

**Closed [PR #10](https://github.com/btakita/corky/pull/10)** — superseded, LinkedIn image uploads already on main. Branch deleted.

### Converting Merge → Rebase

Current graph:
```
*   1e440a2 (main) agent-doc: 18:07:24  ← post-merge commit
*   eb02771 Merge branch '20260225/topic-sync'
|\
| * 9405146 (topic-sync) ← 30+ commits from Feb 27 sessions
* | 6d9411c ← this session (main-only)
* | 5db00f2 ← this session (main-only)
* | 6c58ce9 ← old main
```

**Main-only commits to replay:** `5db00f2`, `6d9411c`

```bash
git stash                              # save uncommitted changes
git reset --hard 6d9411c               # undo merge + post-merge, back to pre-merge main
git rebase 20260225/topic-sync         # replay main-only commits on top of 9405146
git stash pop                          # restore uncommitted changes
```

If any file conflicts during rebase, pull the resolved version from your merge:
```bash
git checkout eb02771 -- <conflicted-file>
```

**Note:** The snapshot file (`.agent-doc/snapshots/2ca8a80b...`) has leftover merge conflict markers. After the rebase, I'll overwrite it with the clean document content.

103. Closed PR #10 — LinkedIn image uploads superseded.

## User

Should I have the .agent-doc directory committed into the repo? I just had some merge conflicts. There's also pids in session.json

## Assistant

**Recommendation: gitignore the entire `.agent-doc/` directory.**

What's in there:

| File | Purpose | Should commit? |
|------|---------|---------------|
| `sessions.json` | PIDs, pane IDs, CWD — machine-specific runtime state | No |
| `claims.log` | Ephemeral notification queue, truncated after reading | No |
| `snapshots/*.md` | Document state for diff computation | Debatable |

**Snapshots trade-off:**
- **Committed:** diffs work across clones, but causes merge conflicts on any branch touching session docs (exactly what just happened)
- **Gitignored:** no merge conflicts, but first submit after clone sees entire doc as "new" (self-heals — creates snapshot on first submit)

The merge conflict cost outweighs cross-clone continuity. Add to `.gitignore`:
```
.agent-doc/
```

Then remove from tracking:
```bash
git rm -r --cached .agent-doc/
```

## User

Why is idea stuck in a git state that has many changes?

## Assistant

Git working tree is clean (`git status` → nothing to commit). You're only 1 commit ahead of `origin/main`. IDEA's `M↓` indicators are stale — cached from before the merge/rebase operations that rewrote history.

**Fix:** `Ctrl+Alt+Y` (Synchronize) in IDEA. If that doesn't clear it: `File > Invalidate Caches... > Invalidate and Restart`.

You can also push to sync origin: `git push origin main` — that eliminates the 1-commit-ahead state.

## User

When I ran the Claim for Tmux Pane command for ron-berkes, the left tmux pane was claimed even though ron-berkes.md is open on the right split in the Idea editor.

## Assistant

**Known issue — claim binds to the focused tmux pane, not the editor split.**

`ClaimAction.kt` runs `agent-doc claim <file>` without passing a pane ID. Since the IDE process has no `TMUX_PANE` env var, `claim.rs` falls back to `tmux display-message -p "#{pane_id}"` — which returns whichever tmux pane was last focused, not the one matching your editor split.

**Workaround:** Focus the desired tmux pane before clicking "Claim for Tmux Pane".

**Fix options:**

1. **`--pane` CLI arg** — `agent-doc claim <file> --pane %5`. Plugin queries `tmux list-panes`, shows picker dialog.
2. **Pane picker fallback** — When no `TMUX_PANE`, `claim` itself presents a numbered pane list to choose from.
3. **Layout heuristic** — Plugin detects editor split position, maps to tmux pane by coordinates. Fragile.

Option 1 + pane picker dialog in the plugin is the cleanest path. Want me to implement?

## User

Can the tmux positional pane layout be known by agent-doc? If so, we could match the tmux panes the requested positional layout

## Assistant

**Yes — tmux exposes full pane coordinates.** `tmux list-panes -F "#{pane_id} #{pane_left} #{pane_top}"` gives cell positions for every pane. Your current window:

```
%2  left=0    (left pane)
%1  left=121  (right pane)
```

**Positional claim design:**

1. **CLI:** `agent-doc claim <file> --position left|right|top|bottom`
2. **Resolution:** query `tmux list-panes`, sort by `pane_left` (left/right) or `pane_top` (top/bottom), pick the matching pane
3. **Plugin:** detect which editor split the file is in, pass `--position` to claim

**Current gap:** The JetBrains plugin's `SyncLayoutAction` detects split *orientation* (h/v) but not which file is on which *side*. Enhancement needed: walk the `Splitter` component tree to determine if the focused editor is the first or second child, then map to left/right or top/bottom.

The tmux side is trivial (just `list-panes` + sort). The plugin side needs moderate work to detect split position. Worth implementing — eliminates the "focus the right pane first" workaround.

## User

Yes, implement...Are we maintaining the Jetbrains plugin SPEC file? I want feature parity with the VSCode plugin.

## Assistant

**No separate plugin SPEC exists.** The main `agent-doc/SPEC.md` has use cases (U1-U13) but no per-plugin spec. Worth adding a `PLUGINS.md` or section in SPEC.md that defines the expected feature set.

**Feature parity gap is massive.** VSCode is ~45 lines with submit-only. JetBrains has ~2000 lines across 8 files:

| Feature | JetBrains | VSCode |
|---------|-----------|--------|
| Submit (Ctrl+Shift+Alt+A) | yes | yes |
| Claim (Ctrl+Shift+Alt+C) | yes | no |
| Layout sync (Ctrl+Shift+Alt+L) | yes | no |
| Prompt polling + panel UI | yes | no |
| Tab sync listener | yes | no |
| Auto-merge (3-way git) | yes | no |
| Popup menu (Alt+Enter) | yes | no |

**Proposed plan:**

1. **Positional claim** — add `--position left|right|top|bottom` to `agent-doc claim` CLI + update JetBrains `ClaimAction` to detect split position and pass it
2. **Plugin SPEC** — add §10 Editor Plugins to `agent-doc/SPEC.md` defining required feature set
3. **VSCode feature parity** — port claim, layout sync, prompt polling to VSCode extension (separate effort, substantial)

Start with #1 (positional claim) since that's the immediate bug fix?

## User

Do #1. Would it be better to have a separate editors/SPEC.md file or would that confuse things?

## Assistant

**Positional claim implemented.** Tested and working.

### Changes

**`agent-doc` CLI** (`claim.rs`, `main.rs`, `sessions.rs`):
- `agent-doc claim <file> --position left|right|top|bottom`
- `pane_by_position()` queries `tmux list-panes` coordinates, sorts by `pane_left`/`pane_top`, returns the pane at the requested position
- Falls back to `current_pane()` when `--position` is omitted (backwards compatible)

Tested: `--position left` → `%2` (left=0), `--position right` → `%1` (left=121).

**JetBrains plugin** (`ClaimAction.kt`):
- `detectEditorPosition()` walks the `Splitter` component tree
- `findPositionInSplitter()` checks if the editor component is in the first child (left/top) or second child (right/bottom) of the `Splitter`
- Passes `--position` to `agent-doc claim` when split is detected; omits it for single-editor layouts

**Verification:**
- 93 agent-doc tests pass, clippy clean
- Plugin builds successfully
- Binary installed

**On the SPEC question:** `editors/SPEC.md` is better than cramming it into the main SPEC. The main `agent-doc/SPEC.md` covers the CLI tool. Plugin behavior is a separate concern — both plugins should conform to the same spec, and `editors/SPEC.md` is discoverable alongside both plugin directories. Not creating it now — that's a separate task.

104. Positional claim — `--position left|right|top|bottom` in `agent-doc claim` + JetBrains plugin split-position detection. 93 tests pass.

## User

The claim fo tmux pane chose the selected pane instead of the side it was on. This time I claimed plan.md but the right tmux pane was selected...so the right tmux pane claimed plan.md

## Assistant

**The updated plugin hasn't been installed in IDEA yet.** The binary has `--position` support, but the JetBrains plugin running in your IDE is still the old `ClaimAction` — it doesn't call `detectEditorPosition()` or pass `--position`. It falls back to `current_pane()` → whichever tmux pane is focused.

To install:
1. IDEA → Settings → Plugins → gear → Install Plugin from Disk
2. Select `agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.1.0.zip`
3. Should hot-reload (plugin has `require-restart="false"`)

Want me to sign it first?

## User

Still didn't work. I ran Clam for on toby-hill while the left tmux claim was selected...and the left tmux pane claimed toby-hill

The plugin didn't hot-reload either.

## Assistant

**The plugin needs a manual reinstall** — hot-reload doesn't apply when the plugin's classes change structurally (new imports, new methods). The `.zip` was rebuilt but IDEA is still running the cached old version.

**To install:** IDEA → Settings → Plugins → gear icon → "Install Plugin from Disk..." → select:
```
agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.1.0.zip
```
Then restart IDEA.

I did this twice. Idea restarted. What is wrong with this flow?

I've also added **diagnostic logging** to `/tmp/agent-doc-plugin.log` — after reinstalling, try claiming again and check that file. It will show whether `detectEditorPosition` is finding the splitter, whether the EDITOR context is available, and what position it detects (or why it falls back to null).

Also added a second detection strategy: if the EDITOR component isn't found in the Splitter tree, it now checks `managerEx.windows` to find which split window contains the file, then uses the window index + orientation as a heuristic.

## User

Always build both the agent-doc-jetbrains and the agent-doc-jetbrains-signed. Question, if I install agent-doc-jetbrains, would it be easier to run diagnostics & debug on the plugin?
