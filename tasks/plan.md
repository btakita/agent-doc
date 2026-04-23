---
agent: codex
agent_doc_session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
tmux_session: '1'
---

# Plan: agent-doc & agent-loop

## Current Plan

### 2026-04-22 — agent-doc `#vedj`

- [x] Reproduce and isolate the `normalize_user_prompts_in_exchange` bug where synthetic heading-only churn (for example `(HEAD)` suffix updates) suppresses `❯ ` prefixing for the following user prompt.
- [x] Patch the write-path normalization logic in `src/agent-doc/src/write.rs` and add focused regression coverage.
- [x] Update the relevant `agent-doc` spec/docs to make the normalization rule explicit.
- [x] Verify with targeted tests plus build/install for local testing, then record the result in the session document.

### 2026-04-22 — agent-doc release + publish

- [x] Audit `src/agent-doc` release readiness: version files, changelog entry, branch/state, and any prerelease blockers.
- [x] Run prerelease verification (`make check`, local install, relevant audits) and resolve local release blockers (`clippy` helper arity + flaky flock test + stale docs audit).
- [x] Complete release actions in `src/agent-doc`: create release commit, push branch, tag `v0.33.12`, push tag, publish to crates.io, and commit the parent submodule pointer update.
- [ ] Update the session document with exact outcomes and note the remaining asynchronous CI state (`Release` workflow in progress on GitHub).

## Status

- **agent-doc**: v0.5.4 — crates.io + PyPI + GitHub Releases. 93 tests (72 unit + 21 integration).
  - Session routing: `start`, `route`, `claim`, `focus`, `layout`, `prompt`, `commit`
  - `claim`: `--position`, `--window`, `--pane` flags. Window-scoped routing via sessions.json.
  - `layout`: mirror editor splits, session-aware break-out, `--window` scoping
  - `prompt`: capture-pane polling, `--all`, `--answer <N>`. Plugin shows bottom-anchored panel.
  - `skill install/check`, `commit` (superproject-aware), `upgrade` self-update, `plugin install/update/list`
- **corky**: v0.12.0 — crates.io + PyPI + GitHub Releases. 519 tests.
  - LinkedIn OAuth2, `_cmd` credential fields, contact sync v2, topics, scheduling
  - Gmail filters: `filter auth/pull/push/build`, `label clear`
  - `corky transcribe`: whisper-rs, feature-gated CUDA, speaker turn detection
- **instruction-files**: v0.1.3 — crates.io. 47 tests. SPEC.md discovery, line budget.
- **agent-doc-jetbrains**: v0.1.0 — signed plugin.
  - Run/Claim/Layout Sync hotkeys, Alt+Enter popup (ActionPromoter), PromptPoller/PromptPanel
  - Positional claim (Splitter tree walk), tab-to-pane sync (EditorTabSyncListener)
  - Window-scoped `--window` passthrough via `TerminalUtil.projectWindowId()`
- **agent-doc-vscode**: v0.1.0 — extension.
- **agent-loop**: Root Makefile (`release`, `update`, `check`, `precommit`, `publish`, `clean`).
  - Local symlinks (gitignored): `briantakita.me-dev`, `ctx-core-dev`, `python-intentions`

### Session Routing (tmux)

```
agent-doc start <file>                 # register pane, exec claude
agent-doc route <file>                 # send-keys to pane, auto-start if needed
agent-doc claim <file> [--position] [--window] [--pane]
agent-doc focus <file> [--pane]        # focus tmux pane (cross-window)
agent-doc layout <files...> --split h|v [--window]
agent-doc prompt <file>                # detect permission prompts → JSON
agent-doc prompt --all                 # poll all live sessions
agent-doc prompt --answer <N> <file>   # answer a prompt
agent-doc commit <file>                # git add + commit with timestamp
agent-doc skill install|check          # SKILL.md management
agent-doc plugin install|update|list   # Editor plugin management
```

## Outstanding

### agent-doc
- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`
- [ ] `TmuxBatch` command batching — reduce tab-switch flicker

### instruction-files
- [ ] Collapse `AuditConfig` presets to single polyglot `Default`
- [ ] `.instruction-files.toml` project-root config file

### corky — Topics
- [ ] Extract 3-way merge to shared `context_sync` module
- [ ] Topic sync with eligibility check
- [ ] Thread tagging during sync

### corky — Integrations
- [ ] Telegram: `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Slack: `slack-morphism` — plan at `corky/tasks/integrations/slack.md`

### Future
- [ ] Streaming responses, document-only permission flow, desktop notifications
- [ ] Multi-agent documents, email transport bridge with corky
- [ ] Personal data separation — nested git repo inside `tasks/`

## Design Decisions

- **session vs resume**: `session` = permanent routing UUID, `resume` = Claude conversation ID (cleared by reset)
- **instruction-files dependency**: `path + version` — local builds use path, `cargo publish` uses crates.io
- **tmux routing**: direct `send-keys -t <pane>`, not broadcast. Each doc has own UUID → pane mapping.
- **Prompt detection**: `tmux capture-pane` polling, ANSI strip, regex match. Fragile but works. Future: Claude Code hooks API.
- **Window-scoped routing**: `SessionEntry` stores `window` field. Plugin reads `projectWindowId()` from registry. Layout filters panes to claimed window; only breaks out registered session panes.
- **Topics config**: `[[topics]]` in `.corky.toml` with labels, contacts, `shared_with`.
- **Context routing**: Topics, contacts, threads share content-hash 3-way merge with eligibility rules.

## Reference

- `agent-doc/SPEC.md` — Decisions & architecture
- `agent-doc/CLAUDE.md` — Module layout & conventions
- `SPEC.md` — Workspace structure
- `CLAUDE.md` — Workflow orchestration

## Session Log

### Sessions 1-4 (2026-02-25 to 2026-02-28)

Releases: agent-doc v0.3.0 → v0.5.4, corky v0.11.1 → v0.12.0, instruction-files v0.1.1 → v0.1.3.

Key work: Topics (Phases 1-3), Gmail filters, `corky transcribe`, session routing (`start/route/claim/focus/layout/prompt`), JetBrains plugin (positional claim, ActionPromoter, PromptPanel), VSCode extension, window-scoped routing, `agent-doc commit/skill/upgrade`. LinkedIn OAuth fix, SPECS→SPEC rename, PyPI catch-up, `corky label clear`.

### Session 5 (2026-03-03)

Key work: Yolo mode, token efficiency (claude-mem plugin), audit-docs fixes, submodule management (briantakita.me-dev → gitignored symlinks).

### Session 6 (2026-03-05)

Key work: briantakita.me migration to Cloudflare Pages (prerender + static deploy), R2 asset migration, Contabo decommission, email worker fix (SLACK_WEBHOOK_URL), `python-intentions` symlink added. Audit-docs: updated agent-doc SPEC.md/README.md/CLAUDE.md for new `plugin.rs` module (`plugin install/update/list`).

## Assistant

### Audit results

Ran `audit-docs` across submodules:
- **corky**: Clean
- **instruction-files**: Skipped (library crate)
- **agent-doc**: 4 staleness issues from `src/plugin.rs` (new module: `plugin install/update/list`). Updated:
  - **README.md**: Added plugin commands to Quick Start and CLI Reference
  - **SPEC.md**: Added section 7.18 (plugin management)
  - **CLAUDE.md**: Added `plugin.rs` to module layout
  - **SKILL.md**: Touched (no content changes needed)
- Only remaining warning: CLAUDE.md ontology table (14 rows) — advisory, intentional

### Compacted

Reduced plan.md from 185 to ~100 lines. Merged Sessions 1-4 into single summary, preserved all outstanding items and design decisions. Added `plugin install/update/list` to status and CLI reference.

### Committed agent-loop

## User

### 2026-04-22 — Install agent-doc as Codex plugin

- [x] Add Codex plugin metadata to the local `src/agent-doc` checkout.
- [x] Register the plugin in the repo-local marketplace so Codex can discover it from this workspace.
- [x] Verify manifest and marketplace JSON parse cleanly and point at the local source checkout.

Review:
- Added `src/agent-doc/.codex-plugin/plugin.json` with version `0.33.12`, `skills: "./"`, and local asset references.
- Added `.agents/plugins/marketplace.json` with `agent-doc` sourced from `./src/agent-doc` and `INSTALLED_BY_DEFAULT`.
- Updated `src/agent-doc/SKILL.md` frontmatter to `agent-doc-version: "0.33.12"` so it matches the local binary used by the plugin.
- Ran `codex marketplace add /home/brian/work/btakita/agent-loop`; Codex registered `[marketplaces.agent-loop-local]` in `/home/brian/.codex/config.toml`.
- Replaced the tracked empty `.codex` placeholder file with Codex install output: `.codex/AGENTS.md` plus 8 `.codex/runbooks/*.md` files.
- Found stale compiled `agent-kit` behavior in the installed `agent-doc` binary, cleaned the dependency artifact, reinstalled `agent-doc` from `src/agent-doc`, and verified `agent-doc skill install --harness codex` is now idempotent.
- Verified JSON parsing, source path, skill file, icon, logo paths, Codex marketplace registration, and Codex skill/runbook install.

### 2026-04-22 — Fix Codex Invocation

- [x] Make Codex-installed agent-doc instructions use plain-text `agent-doc <file>` invocation instead of `/agent-doc`.
- [x] Add regression tests covering Codex-vs-Claude installed skill content.
- [x] Build and install the local `agent-doc` binary, reinstall Codex skill files, and verify the installed `.codex/AGENTS.md` text.

Review:
- Updated `src/agent-doc/src/skill.rs` so `agent-doc skill install --harness codex` writes Codex-specific `.codex/AGENTS.md` content instead of reusing the Claude-first slash-command text verbatim.
- Updated `src/agent-doc/runbooks/harness-invocation.md` so the Codex section explicitly says not to use `/agent-doc` and gives `agent-doc <FILE>` as the supported form.
- Added regression tests for Codex content generation and installed Codex output.
- Verified with `cargo test --manifest-path src/agent-doc/Cargo.toml skill::tests -- --nocapture`, `cargo check --manifest-path src/agent-doc/Cargo.toml`, `cargo build --manifest-path src/agent-doc/Cargo.toml --release`, `cargo install --path .`, and `agent-doc skill install --harness codex`.

### 2026-04-22 — Fix JetBrains Route For Codex

- [x] Patch harness-specific tmux trigger generation so Codex routes `agent-doc <file>` instead of `/agent-doc <file>`.
- [x] Remove slash-specific route acceptance polling and add Codex regression coverage.
- [x] Build/install the local binary and verify the updated route behavior.
- [x] Patch back the root-cause explanation into `tasks/agent-doc/agent-doc-bugs.md` using `agent-doc write` + `agent-doc commit`.

Review:
- Updated `src/agent-doc/src/harness.rs` so `HarnessConfig::codex()` now emits `agent-doc {file}` while Claude keeps `/agent-doc {file}`.
- Updated `src/agent-doc/src/route.rs` to poll for the exact resolved harness trigger instead of hardcoding `/agent-doc`, and tightened matching so plain `agent-doc ...` does not false-match inside `/agent-doc ...`.
- Added regression coverage for Claude and Codex trigger generation, recent-line trigger detection, the cross-trigger substring bug, and tmux command delivery for both harnesses.
- Aligned stale JetBrains and supervisor comments with the actual route flow so the plugin docs no longer claim JetBrains sends `/agent-doc ...` directly.
- Verified with targeted `cargo test` runs covering the new harness and route cases, plus `cargo check`, `cargo build --release`, and `cargo install --path .`.

### 2026-04-22 — Pending Capture Rules (`#cs6n`)

- [x] Update the bundled skill and pending runbook to require explicit pending capture for pending-worthy follow-up work, tag unaccepted suggestions as `recommended`, and document front-of-list default ordering.
- [x] Change pending add behavior so new items land at the beginning of the list while preserving batch sequence order for local agent-doc workflows.
- [x] Add regression tests covering the new pending ordering behavior and bundled skill/runbook guidance.
- [x] Build and install the local `agent-doc` binary for local testing.

Review:
- Updated `src/agent-doc/SKILL.md` and `src/agent-doc/runbooks/pending-ops.md` so pending-worthy follow-up work must be explicitly captured, unaccepted suggestions are tagged `recommended`, and new items default to the beginning of `agent:pending`.
- Changed `src/agent-doc/src/pending.rs` so `op_add` prepends new items, added `pending_cmd::add_many()` to preserve batch sequence order at the front of the list, and routed the `write` and `notify` CLI paths through that behavior.
- Added regression tests for prepend semantics, batch-order preservation, notify multi-add ordering, and bundled skill/runbook content.
- Verified with targeted `cargo test` filters, `cargo check --manifest-path src/agent-doc/Cargo.toml`, `cargo build --manifest-path src/agent-doc/Cargo.toml --release`, `cargo install --path .`, and `agent-doc skill install --harness codex`.

### 2026-04-22 — Custom Pending IDs (`#9b2k`)

- [x] Add a custom-ID path for pending adds using a leading `id=<value>` prefix, validate it, and preserve it instead of generating a hash.
- [x] Update specs/runbooks/CLI docs so the custom-ID syntax and constraints are explicit.
- [x] Add regression tests for the parser, pending CLI paths, and user-facing docs.
- [x] Build and install the local `agent-doc` binary for local testing.

### 2026-04-22 — JetBrains Prompt Fix (`agent-doc-bugs`)

- [x] Reproduce the incorrect JetBrains prompt rendering from the current prompt-panel implementation/spec/tests and isolate the mismatch.
- [x] Patch the JetBrains plugin prompt UI so the rendered row matches the intended prompt wording/layout, with focused regression coverage.
- [x] Verify with targeted JetBrains plugin tests and a project build, then record the outcome.

### 2026-04-22 — Explain Uncommitted Prompt In `agent-doc-bugs`

- [x] Inspect the current task-doc diff and root git status to confirm what remained dirty after the prior patchback.
- [x] Verify the `agent-doc commit` behavior in `src/agent-doc/src/git.rs` and the command spec to determine whether this was expected selective-commit behavior or a regression.
- [x] Re-run the focused `git.rs` commit tests and record the root-cause explanation in the task document.

### 2026-04-22 — Repair Missed `agent-doc-bugs` Patchback

- [x] Confirm whether the missing patchback was a real `agent-doc` write regression or a missed edit in the previous turn.
- [x] Append the missing previous two responses into `tasks/agent-doc/agent-doc-bugs.md`.
- [x] Record the immediate failure mode so the session document reflects what actually happened.

### 2026-04-22 — Fix Self-Heal For `status+exchange` Patchbacks

- [x] Inspect the uncommitted `src/agent-doc` patchback and confirm it is the missed self-heal fix for `status+exchange` drift.
- [x] Re-run focused verification for the new absorb classifier / commit behavior and confirm the root cause of the missed commit.
- [x] Commit the `src/agent-doc` patchback and write the explanation plus commit hash into `tasks/agent-doc/agent-doc-bugs.md`.

- [x] Reproduce the current self-heal miss where a patchback changes both `agent:status` and `agent:exchange`, causing `commit()` to leave the patchback uncommitted.
- [x] Patch the out-of-band absorb classifier so safe agent-owned `status` mutations can be absorbed alongside `exchange` / `pending` without broadening into user-prompt commits.
- [x] Add focused regression tests and update the snapshot/commit specs, then verify with targeted `git::tests`.

### 2026-04-22 — Commit Parent Patchbacks After Self-Heal Fix

- [x] Confirm the remaining dirty state is in the parent `agent-loop` repo, not the `src/agent-doc` submodule worktree.
- [ ] Commit the parent repo changes that correspond to the repaired patchbacks (`src/agent-doc` submodule pointer plus `tasks/agent-doc/agent-doc-bugs.md`).
- [ ] If the parent commit path exposes another selective-commit gap, patch `agent-doc` again and re-verify.

Review:
- Updated `src/agent-doc/src/pending.rs` so `pending::op_add` accepts a leading `id=<custom> ` prefix, validates 1-8 ASCII alphanumeric ids, strips an optional leading `#`, rejects duplicates, and preserves the provided id instead of generating one.
- Updated `src/agent-doc/src/main.rs`, `src/agent-doc/src/pending_cmd.rs`, `src/agent-doc/runbooks/pending-ops.md`, and `src/agent-doc/specs/pending-system.md` so the custom-ID syntax is documented in the CLI/help/runbook/spec surfaces.
- Added regression coverage in `pending.rs`, `tests/pending_integration.rs`, and `skill.rs` for valid custom ids, invalid ids, duplicate custom ids, missing post-prefix text, and bundled runbook content.
- Verified with `cargo test --manifest-path src/agent-doc/Cargo.toml custom_id -- --nocapture`, `cargo check --manifest-path src/agent-doc/Cargo.toml`, `cargo build --manifest-path src/agent-doc/Cargo.toml --release`, `cargo install --path .`, and `agent-doc skill install --harness codex`.

### 2026-04-22 — Codex Start Ignores `claude_args` (`#mmgb`)

- [x] Make start-time arg resolution harness-specific so Codex ignores Claude-only arg aliases while Claude keeps backward-compatible fallback behavior.
- [x] Update the format/config/Codex support specs to document `agent_args` and the harness-specific precedence chains.
- [x] Add regression tests for Claude-vs-Codex arg resolution and rebuild/install the local `agent-doc` binary for testing.

Review:
- Updated `src/agent-doc/src/start.rs` so start-time arg resolution is driven by the active harness: Claude keeps the compatibility chain `agent_args > claude_args > config.agent_args > config.claude_args > AGENT_DOC_CLAUDE_ARGS`, while Codex now uses only `agent_args` from frontmatter/config and ignores `claude_args` plus `AGENT_DOC_CLAUDE_ARGS`.
- Updated `src/agent-doc/specs/codex-support.md`, `src/agent-doc/specs/02-document-format.md`, and `src/agent-doc/specs/06-config.md` so the harness-specific arg behavior is explicit in the docs/spec.
- Added regression coverage in `start.rs` for Claude alias precedence, Codex ignoring Claude-only aliases, and Codex using `agent_args` only.
- Verified with `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_agent_args -- --nocapture`, `cargo check --manifest-path src/agent-doc/Cargo.toml`, `cargo build --manifest-path src/agent-doc/Cargo.toml --release`, and `cargo install --path .`.

### 2026-04-22 — JetBrains PromptPanel Single-Row Mode (`#n6dj #p9h7 #7g66 #9kqp`)

- [x] Replace the wrapping prompt-row layout with a non-wrapping single-row layout and fixed row height in the JetBrains plugin.
- [x] Move prompt overflow detail out of the main row, add truncation/tooltip handling, and update the JetBrains spec to match the new invariant.
- [x] Add focused JetBrains unit tests for the single-row helpers/layout behavior, then build/install the local plugin and local `agent-doc` binary for testing.

Review:
- Updated `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPanel.kt` so the prompt panel is a fixed-height single row: the old wrapping layout was removed, the question text now truncates with tooltip fallback, buttons remain on a non-wrapping horizontal row, and the hotkey/pending metadata moved into a compact `?` tooltip affordance instead of inline text.
- Added `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/PromptPanelTest.kt` covering truncation, pending-count tooltip preservation, the non-wrapping controls row, and fixed-height locking.
- Updated `src/agent-doc/editors/jetbrains/SPEC.md` to document the single-row invariant and bumped `src/agent-doc/editors/jetbrains/gradle.properties` plugin version to `0.2.72`.
- Verified with `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin`, `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew signPlugin`, `cargo build --release`, `cargo install --path .`, and `agent-doc plugin install --local jetbrains` targeting `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`.

### 2026-04-22 — session-share patchback commit investigation

- [x] Inspect `src/session-share/tasks/claudescore.md`, its git history, and the current diff against `HEAD`.
- [x] Check `src/session-share/.agent-doc/logs/ops.log` and the saved snapshot state to determine whether the missing commit was a git-routing failure or an out-of-band write.
- [x] Patch the root-cause explanation back into `tasks/agent-doc/agent-doc-bugs.md`.

Review:
- `git -C src/session-share log -- tasks/claudescore.md` stops at commit `1987d68` dated `2026-04-22 19:31:33 -0400`, while the working tree still contains later response blocks (`#sz4a`, `#jf7f`, `#avce`, `#ptsm`).
- The last real `agent-doc` patchback cycle for that file in `src/session-share/.agent-doc/logs/ops.log` is the `write_origin` + `ipc_socket_delivered` + `commit_success` sequence at `2026-04-22 11:37:23` through `11:37:27 -0400`.
- After that, the log shows only repeated `out_of_band_write` warnings with `snap_len` pinned at the older snapshot (`18223`/`18230`) while `file_len` keeps growing. That means `git::commit()` kept staging the saved snapshot by design and left the newer working-tree edits uncommitted.

### 2026-04-22 — Fix snapshot / out-of-band-write handling

- [x] Reproduce the failure mode in `src/agent-doc` tests: newer working-tree content and pending mutations exist, but `git::commit()` still stages an older snapshot.
- [x] Patch the commit/snapshot logic so agent-doc safely absorbs genuine out-of-band agent-doc document mutations instead of repeatedly committing stale snapshot content.
- [x] Update the relevant spec/docs to document the new snapshot/out-of-band-write behavior and its safety boundary.
- [x] Run targeted tests plus build/install for local testing, then patch the result back into `tasks/agent-doc/agent-doc-bugs.md`.

Review:
- Updated `src/agent-doc/src/git.rs` with a narrow out-of-band absorb path. `commit()` now refreshes the snapshot from the live file when the redacted component structure is unchanged and the drift looks like a missed agent-doc mutation: an appended `exchange` `### Re:` block and/or a `pending` stable-ID superset. Plain user-prompt drift still stays uncommitted.
- Added four regressions in `src/agent-doc/src/git.rs`: classifier accepts exchange+pending drift, classifier rejects user-prompt drift, commit absorbs missed response+pending changes, and commit still leaves a raw user prompt in the working tree/snapshot-old state.
- Updated `src/agent-doc/specs/03-snapshot-system.md` and `src/agent-doc/specs/07-commands.md` so the new absorb behavior and its safety boundary are explicit.
- Verified with `cargo test --manifest-path src/agent-doc/Cargo.toml git::tests -- --nocapture`, `cargo check`, `cargo build --release`, `cargo install --path .`, and `agent-doc --version` (`0.33.12`).
