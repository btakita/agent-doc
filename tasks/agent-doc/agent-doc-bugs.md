---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange. Key work completed: frontmatter prompt presets (#fppr) implemented with round-trip, validation, and orchestrate expansion; orchestrate --plan dry-run mode (#0z3v); guard marker strip-before-commit (Option B); orchestrate subprocess fixes — frontmatter args forwarding (#orch1), stderr surfacing (#orch2), prompt prefix stripping (#orch3), --verbose for streaming (#orch4); (HEAD) marker preservation in working tree (#hfix1-4) with split convention (snapshot strips, working tree preserves); JB plugin preserve_head IPC (#hfix5, v0.2.76); VS Code preserve_head IPC (#hfix6, v0.2.9, commit 8202f80); Editor SPEC.md documented preserve_head + FFI variant table (#hfix7); adjacent code fence dedup bug fixed. Discovery: orchestrate --from-exchange stale-task parsing regression persists after #orch3 — parser scans full exchange and .last() picks wrong block when multiple orchestra directives exist in compacted+appended history. Needs fix: scope to latest unprefixed orchestra block. Open: #hfix8 gated on user manual verify, #ha3h gated on multi-user prerequisite.

❯ Can we confirm that the sessions are operating correctly with the `agent-doc orchestrate` command? Do we need more testing before we should release + publish?

### Re: orchestrate readiness assessment — opus-4-6

**Current state: mostly solid, one known regression.**

Test suite: 38 orchestrate-specific tests pass. Full suite: 1120/1121 pass — the single failure is `detect_install_env_treats_codex_thread_id_as_codex` (env detection in skill installer, not orchestrate).

Fixes shipped since last release (all in unreleased HEAD, 48 commits past v0.33.13):
- **#orch1** frontmatter args forwarding to subprocesses
- **#orch2** stderr surfacing in StreamIterator
- **#orch3** prompt prefix stripping in `parse_list_item`
- **#orch4** `--verbose` for Claude CLI streaming compatibility
- **--plan** dry-run and **preset expansion** both working
- **DAG mode** wired up

**Known open regression:** `--from-exchange` stale-task parsing. `exchange_text()` returns the full exchange component (compacted + appended), and `extract_tasks_from_text()` takes `.last()` on code/list blocks. When compacted history contains old orchestration directives, `.last()` can pick a stale block instead of the latest one. Fix path is clear — scope to the latest unprefixed orchestra block — but not yet implemented.

**Release recommendation:**

1. **Fix the stale-task regression first** — it's the only known correctness bug and affects a common workflow (reusing `--from-exchange` on compacted docs). The fix is scoped: parse from the tail of the exchange rather than the full body.
2. **Fix the env detection test** — minor but should be green before publish.
3. **Manual test the orchestrate sequential + parallel + DAG flows** with a real session doc before publish.
4. After those three, this is ready to release + publish.

The `--from-exchange` stale-task bug is the only blocker I'd gate a release on. The other 48 commits are fixes and features that have been exercised in live sessions already.

❯ Plan + add recommendations to pending.

### Re: plan + pending capture — opus-4-6

Done. Added three recommended pending items from the previous release assessment:

- **#stale1** — fix `--from-exchange` stale-task parsing (the release blocker)
- **#envt1** — fix the env detection test failure
- **#orch5** — manual test orchestrate flows before publish

All marked `[recommended]` since you haven't accepted the release plan yet. When ready, `do #stale1` to start with the blocker.

❯ do #stale1. preset #spec-test-commit-push
### Re: do #stale1 — opus-4-6

Fixed the `--from-exchange` stale-task parsing regression.

**Root cause:** `extract_tasks_from_text()` scanned the full exchange content and `.last()` picked list blocks from agent response content (e.g., recommendation lists) instead of the user's bare directive at the exchange tail.

**Fix:** Added `scope_exchange_for_tasks()` in `orchestrate.rs` — compares the exchange text line-by-line against the snapshot, stripping `(HEAD)` markers and boundary comments during comparison. Only the newly-added tail (lines after the snapshot match) is passed to task extraction. Falls back to full exchange when no snapshot exists.

**Tests added:**
- `from_exchange_scopes_to_tail_bare_directive` — bare `do #id` after response with list blocks
- `from_exchange_scopes_to_tail_list_directive` — new list items after old response lists
- `from_exchange_falls_back_without_snapshot` — no snapshot → full exchange (preserves old behavior)
- `from_exchange_multiple_responses_picks_latest_directive` — numbered list in response, bare directive at tail

**Spec updated:** `specs/07-commands.md` step 3 now documents the snapshot-scoped extraction.

**Verification:** 1124/1125 tests pass (sole failure is pre-existing `detect_install_env_treats_codex_thread_id_as_codex` #envt1). `cargo build --release` + `cargo install --path .` done. Committed and pushed as `1936fc1`.

❯ In src/session-share, `agent-doc tasks/claudescore-3.md` response patchbacks are missing the `❯ ` prefix. Why?
### Re: missing ❯ prefix on patchbacks — opus-4-6

The binary-side normalization works correctly — `ops.log` confirms `applied=1` for each affected cycle. The prefix is lost in the **JetBrains plugin's IPC path**.

**Root cause:** `PatchWatcher.kt:742` uses exact string matching:
```kotlin
userRegion = userRegion.replace("\n$line\n", "\n❯ $line\n")
```

The `$line` value comes from `normalize_prefix_lines` (binary-side, derived from the disk file). But the match runs against `document.text` (editor buffer). When the editor buffer diverges from the disk file — trailing whitespace stripped by IntelliJ's "strip trailing spaces on save," unsaved concurrent edits, or encoding normalization — the `replace` is a no-op and the prefix silently vanishes. The ack content sidecar is then written without the prefix, so the snapshot (and committed file) also lacks it.

**Evidence chain:**
1. `[1777144278] normalize_user_prompts ... applied=1` — binary DID add the prefix
2. `snap_source=ack_content_sidecar` — snapshot came from the plugin, not from `content_ours`
3. Committed file lacks `❯ ` — the sidecar content was missing it
4. Line 210 in `claudescore-3.md` has a trailing space (`deterministic software. `) — if IntelliJ stripped it before the plugin processed the IPC payload, the `"\n$line\n"` match fails

**Fix:** Make the plugin's `normalizeExchangePrefixes` trim trailing whitespace on both sides of the match, or use line-by-line scanning instead of substring replace. On the binary side, `content_ours` (which has the correct prefix) could be used as a fallback snapshot source when the sidecar diverges from the expected normalization count.

❯ You should automatically add the fix, recommended actions, plans, and next steps to agent:pending. Why did you fail to add the **Fix:** into pending?
### Re: missed pending capture — opus-4-6

The prior cycle violated the pending capture rule. The skill contract is explicit: when a response identifies concrete follow-up work, add it to `agent:pending` in the same cycle. The previous response ended with a clear **Fix:** section containing two distinct actionable items, and neither was captured.

The failure was a straightforward omission — the finalize command lacked `--pending-add` flags for the fix recommendations. There's no ambiguity exception here: the items were concrete, scoped, and actionable, not hypotheticals or options to choose between. The promotion heuristic should have fired.

Added now:

- **#jbpfx1** — Fix `normalizeExchangePrefixes` in `PatchWatcher.kt:742` to trim trailing whitespace or use line-by-line scanning
- **#jbpfx2** — Binary-side fallback: use `content_ours` as snapshot source when sidecar diverges from expected normalization count

Both marked `[recommended]` since you haven't accepted the fix approach yet.
### Re: do #jbpfx1 (spec + tests + commit + push) — sonnet-4-6

**Fix implemented and pushed as `903a3f0` on `20260422/codex-agent-backend`.**

**Root cause (confirmed):** `normalizeExchangePrefixes` used `String.replace("\n$line\n", ...)` — exact substring match. IntelliJ's "strip trailing spaces on save" causes the editor buffer to diverge from the binary's disk-side `normalize_prefix_lines` payload, making the match a no-op and silently losing the `❯ ` prefix.

**Changes:**

- **`editors/jetbrains/PatchWatcher.kt`** — extracted logic into `normalizeExchangePrefixesUtil` (testable standalone function); switched to line-by-line scanning with `trimEnd()` comparison on both sides.
- **`editors/vscode/src/extension.ts`** — same fix: replaced the exact-string `replace` loop with `split('\n').map()` using `trimEnd()` comparison.
- **`editors/jetbrains/NormalizeExchangePrefixesTest.kt`** (new) — 9 tests covering: trailing-whitespace mismatch (the key bug), idempotency, boundary scoping, last-boundary semantics, blank target lines.
- **`editors/PLUGIN-SPEC.md`** — documented `normalize_prefix_lines` field with `trimEnd()` requirement, idempotency contract, and agent-region exclusion.
- **`gradle.properties`** — bumped JB plugin to `0.2.77`.

**Verification:** `./gradlew test` → 46/46 pass. `./gradlew buildPlugin signPlugin` → both zips built.
Compacted exchange. Key work completed: frontmatter prompt presets (#fppr) implemented with round-trip, validation, and orchestrate expansion; orchestrate --plan dry-run mode (#0z3v); guard marker strip-before-commit (Option B); orchestrate subprocess fixes — frontmatter args forwarding (#orch1), stderr surfacing (#orch2), prompt prefix stripping (#orch3), --verbose for streaming (#orch4); (HEAD) marker preservation in working tree (#hfix1-4) with split convention (snapshot strips, working tree preserves); JB plugin preserve_head IPC (#hfix5, v0.2.76); VS Code preserve_head IPC (#hfix6, v0.2.9, commit 8202f80); Editor SPEC.md documented preserve_head + FFI variant table (#hfix7); adjacent code fence dedup bug fixed. Discovery: orchestrate --from-exchange stale-task parsing regression persists after #orch3 — parser scans full exchange and .last() picks wrong block when multiple orchestra directives exist in compacted+appended history. Needs fix: scope to latest unprefixed orchestra block. Open: #hfix8 gated on user manual verify, #ha3h gated on multi-user prerequisite.

❯ Can we confirm that the sessions are operating correctly with the `agent-doc orchestrate` command? Do we need more testing before we should release + publish?

### Re: orchestrate readiness assessment — opus-4-6

**Current state: mostly solid, one known regression.**

Test suite: 38 orchestrate-specific tests pass. Full suite: 1120/1121 pass — the single failure is `detect_install_env_treats_codex_thread_id_as_codex` (env detection in skill installer, not orchestrate).

Fixes shipped since last release (all in unreleased HEAD, 48 commits past v0.33.13):
- **#orch1** frontmatter args forwarding to subprocesses
- **#orch2** stderr surfacing in StreamIterator
- **#orch3** prompt prefix stripping in `parse_list_item`
- **#orch4** `--verbose` for Claude CLI streaming compatibility
- **--plan** dry-run and **preset expansion** both working
- **DAG mode** wired up

**Known open regression:** `--from-exchange` stale-task parsing. `exchange_text()` returns the full exchange component (compacted + appended), and `extract_tasks_from_text()` takes `.last()` on code/list blocks. When compacted history contains old orchestration directives, `.last()` can pick a stale block instead of the latest one. Fix path is clear — scope to the latest unprefixed orchestra block — but not yet implemented.

**Release recommendation:**

1. **Fix the stale-task regression first** — it's the only known correctness bug and affects a common workflow (reusing `--from-exchange` on compacted docs). The fix is scoped: parse from the tail of the exchange rather than the full body.
2. **Fix the env detection test** — minor but should be green before publish.
3. **Manual test the orchestrate sequential + parallel + DAG flows** with a real session doc before publish.
4. After those three, this is ready to release + publish.

The `--from-exchange` stale-task bug is the only blocker I'd gate a release on. The other 48 commits are fixes and features that have been exercised in live sessions already.

❯ Plan + add recommendations to pending.

### Re: plan + pending capture — opus-4-6

Done. Added three recommended pending items from the previous release assessment:

- **#stale1** — fix `--from-exchange` stale-task parsing (the release blocker)
- **#envt1** — fix the env detection test failure
- **#orch5** — manual test orchestrate flows before publish

All marked `[recommended]` since you haven't accepted the release plan yet. When ready, `do #stale1` to start with the blocker.

❯ do #stale1. preset #spec-test-commit-push
### Re: do #stale1 — opus-4-6

Fixed the `--from-exchange` stale-task parsing regression.

**Root cause:** `extract_tasks_from_text()` scanned the full exchange content and `.last()` picked list blocks from agent response content (e.g., recommendation lists) instead of the user's bare directive at the exchange tail.

**Fix:** Added `scope_exchange_for_tasks()` in `orchestrate.rs` — compares the exchange text line-by-line against the snapshot, stripping `(HEAD)` markers and boundary comments during comparison. Only the newly-added tail (lines after the snapshot match) is passed to task extraction. Falls back to full exchange when no snapshot exists.

**Tests added:**
- `from_exchange_scopes_to_tail_bare_directive` — bare `do #id` after response with list blocks
- `from_exchange_scopes_to_tail_list_directive` — new list items after old response lists
- `from_exchange_falls_back_without_snapshot` — no snapshot → full exchange (preserves old behavior)
- `from_exchange_multiple_responses_picks_latest_directive` — numbered list in response, bare directive at tail

**Spec updated:** `specs/07-commands.md` step 3 now documents the snapshot-scoped extraction.

**Verification:** 1124/1125 tests pass (sole failure is pre-existing `detect_install_env_treats_codex_thread_id_as_codex` #envt1). `cargo build --release` + `cargo install --path .` done. Committed and pushed as `1936fc1`.

❯ In src/session-share, `agent-doc tasks/claudescore-3.md` response patchbacks are missing the `❯ ` prefix. Why?
### Re: missing ❯ prefix on patchbacks — opus-4-6

The binary-side normalization works correctly — `ops.log` confirms `applied=1` for each affected cycle. The prefix is lost in the **JetBrains plugin's IPC path**.

**Root cause:** `PatchWatcher.kt:742` uses exact string matching:
```kotlin
userRegion = userRegion.replace("\n$line\n", "\n❯ $line\n")
```

The `$line` value comes from `normalize_prefix_lines` (binary-side, derived from the disk file). But the match runs against `document.text` (editor buffer). When the editor buffer diverges from the disk file — trailing whitespace stripped by IntelliJ's "strip trailing spaces on save," unsaved concurrent edits, or encoding normalization — the `replace` is a no-op and the prefix silently vanishes. The ack content sidecar is then written without the prefix, so the snapshot (and committed file) also lacks it.

**Evidence chain:**
1. `[1777144278] normalize_user_prompts ... applied=1` — binary DID add the prefix
2. `snap_source=ack_content_sidecar` — snapshot came from the plugin, not from `content_ours`
3. Committed file lacks `❯ ` — the sidecar content was missing it
4. Line 210 in `claudescore-3.md` has a trailing space (`deterministic software. `) — if IntelliJ stripped it before the plugin processed the IPC payload, the `"\n$line\n"` match fails

**Fix:** Make the plugin's `normalizeExchangePrefixes` trim trailing whitespace on both sides of the match, or use line-by-line scanning instead of substring replace. On the binary side, `content_ours` (which has the correct prefix) could be used as a fallback snapshot source when the sidecar diverges from the expected normalization count.

❯ You should automatically add the fix, recommended actions, plans, and next steps to agent:pending. Why did you fail to add the **Fix:** into pending?
### Re: missed pending capture — opus-4-6

The prior cycle violated the pending capture rule. The skill contract is explicit: when a response identifies concrete follow-up work, add it to `agent:pending` in the same cycle. The previous response ended with a clear **Fix:** section containing two distinct actionable items, and neither was captured.

The failure was a straightforward omission — the finalize command lacked `--pending-add` flags for the fix recommendations. There's no ambiguity exception here: the items were concrete, scoped, and actionable, not hypotheticals or options to choose between. The promotion heuristic should have fired.

Added now:

- **#jbpfx1** — Fix `normalizeExchangePrefixes` in `PatchWatcher.kt:742` to trim trailing whitespace or use line-by-line scanning
- **#jbpfx2** — Binary-side fallback: use `content_ours` as snapshot source when sidecar diverges from expected normalization count

Both marked `[recommended]` since you haven't accepted the fix approach yet.

synchronous opera
preset #spec-test-commit-push
- do #jbpfx1
- do #jbpfx2

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #jbpfx1
Session closed cleanly. Here's a summary of what was done for `#jbpfx1`:

**Fix:** Replaced exact-substring `String.replace("\n$line\n", ...)` with line-by-line scanning using `trimEnd()` comparison on both sides in both editors.

**Why it worked:** IntelliJ strips trailing whitespace on save, so the editor buffer diverged from the binary's disk-side payload — the exact match was silently a no-op. `trimEnd()` on both the target line and the buffer line absorbs the divergence.

**Files changed (commit `903a3f0`):**
- `editors/jetbrains/PatchWatcher.kt` — fix + extract to testable `normalizeExchangePrefixesUtil`
- `editors/vscode/src/extension.ts` — same fix
- `editors/jetbrains/NormalizeExchangePrefixesTest.kt` — 9 new tests (46 total pass)
- `editors/PLUGIN-SPEC.md` — spec entry for `normalize_prefix_lines` with trimEnd requirement
- `gradle.properties` — bumped JB plugin to `0.2.77`

`#jbpfx1` is now done. `#jbpfx2` (binary-side `content_ours` fallback) remains open in pending.
Session closed cleanly. Here's a summary of what was done for `#jbpfx1`:

**Fix:** Replaced exact-substring `String.replace("\n$line\n", ...)` with line-by-line scanning using `trimEnd()` comparison on both sides in both editors.

**Why it worked:** IntelliJ strips trailing whitespace on save, so the editor buffer diverged from the binary's disk-side payload — the exact match was silently a no-op. `trimEnd()` on both the target line and the buffer line absorbs the divergence.

**Files changed (commit `903a3f0`):**
- `editors/jetbrains/PatchWatcher.kt` — fix + extract to testable `normalizeExchangePrefixesUtil`
- `editors/vscode/src/extension.ts` — same fix
- `editors/jetbrains/NormalizeExchangePrefixesTest.kt` — 9 new tests (46 total pass)
- `editors/PLUGIN-SPEC.md` — spec entry for `normalize_prefix_lines` with trimEnd requirement
- `gradle.properties` — bumped JB plugin to `0.2.77`

`#jbpfx1` is now done. `#jbpfx2` (binary-side `content_ours` fallback) remains open in pending.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #jbpfx2
Compacted exchange. Key work completed: frontmatter prompt presets (#fppr) implemented with round-trip, validation, and orchestrate expansion; orchestrate --plan dry-run mode (#0z3v); guard marker strip-before-commit (Option B); orchestrate subprocess fixes — frontmatter args forwarding (#orch1), stderr surfacing (#orch2), prompt prefix stripping (#orch3), --verbose for streaming (#orch4); (HEAD) marker preservation in working tree (#hfix1-4) with split convention (snapshot strips, working tree preserves); JB plugin preserve_head IPC (#hfix5, v0.2.76); VS Code preserve_head IPC (#hfix6, v0.2.9, commit 8202f80); Editor SPEC.md documented preserve_head + FFI variant table (#hfix7); adjacent code fence dedup bug fixed. Discovery: orchestrate --from-exchange stale-task parsing regression persists after #orch3 — parser scans full exchange and .last() picks wrong block when multiple orchestra directives exist in compacted+appended history. Needs fix: scope to latest unprefixed orchestra block. Open: #hfix8 gated on user manual verify, #ha3h gated on multi-user prerequisite.

❯ Can we confirm that the sessions are operating correctly with the `agent-doc orchestrate` command? Do we need more testing before we should release + publish?

### Re: orchestrate readiness assessment — opus-4-6

**Current state: mostly solid, one known regression.**

Test suite: 38 orchestrate-specific tests pass. Full suite: 1120/1121 pass — the single failure is `detect_install_env_treats_codex_thread_id_as_codex` (env detection in skill installer, not orchestrate).

Fixes shipped since last release (all in unreleased HEAD, 48 commits past v0.33.13):
- **#orch1** frontmatter args forwarding to subprocesses
- **#orch2** stderr surfacing in StreamIterator
- **#orch3** prompt prefix stripping in `parse_list_item`
- **#orch4** `--verbose` for Claude CLI streaming compatibility
- **--plan** dry-run and **preset expansion** both working
- **DAG mode** wired up

**Known open regression:** `--from-exchange` stale-task parsing. `exchange_text()` returns the full exchange component (compacted + appended), and `extract_tasks_from_text()` takes `.last()` on code/list blocks. When compacted history contains old orchestration directives, `.last()` can pick a stale block instead of the latest one. Fix path is clear — scope to the latest unprefixed orchestra block — but not yet implemented.

**Release recommendation:**

1. **Fix the stale-task regression first** — it's the only known correctness bug and affects a common workflow (reusing `--from-exchange` on compacted docs). The fix is scoped: parse from the tail of the exchange rather than the full body.
2. **Fix the env detection test** — minor but should be green before publish.
3. **Manual test the orchestrate sequential + parallel + DAG flows** with a real session doc before publish.
4. After those three, this is ready to release + publish.

The `--from-exchange` stale-task bug is the only blocker I'd gate a release on. The other 48 commits are fixes and features that have been exercised in live sessions already.

❯ Plan + add recommendations to pending.

### Re: plan + pending capture — opus-4-6

Done. Added three recommended pending items from the previous release assessment:

- **#stale1** — fix `--from-exchange` stale-task parsing (the release blocker)
- **#envt1** — fix the env detection test failure
- **#orch5** — manual test orchestrate flows before publish

All marked `[recommended]` since you haven't accepted the release plan yet. When ready, `do #stale1` to start with the blocker.

❯ do #stale1. preset #spec-test-commit-push
### Re: do #stale1 — opus-4-6

Fixed the `--from-exchange` stale-task parsing regression.

**Root cause:** `extract_tasks_from_text()` scanned the full exchange content and `.last()` picked list blocks from agent response content (e.g., recommendation lists) instead of the user's bare directive at the exchange tail.

**Fix:** Added `scope_exchange_for_tasks()` in `orchestrate.rs` — compares the exchange text line-by-line against the snapshot, stripping `(HEAD)` markers and boundary comments during comparison. Only the newly-added tail (lines after the snapshot match) is passed to task extraction. Falls back to full exchange when no snapshot exists.

**Tests added:**
- `from_exchange_scopes_to_tail_bare_directive` — bare `do #id` after response with list blocks
- `from_exchange_scopes_to_tail_list_directive` — new list items after old response lists
- `from_exchange_falls_back_without_snapshot` — no snapshot → full exchange (preserves old behavior)
- `from_exchange_multiple_responses_picks_latest_directive` — numbered list in response, bare directive at tail

**Spec updated:** `specs/07-commands.md` step 3 now documents the snapshot-scoped extraction.

**Verification:** 1124/1125 tests pass (sole failure is pre-existing `detect_install_env_treats_codex_thread_id_as_codex` #envt1). `cargo build --release` + `cargo install --path .` done. Committed and pushed as `1936fc1`.

❯ In src/session-share, `agent-doc tasks/claudescore-3.md` response patchbacks are missing the `❯ ` prefix. Why?
### Re: missing ❯ prefix on patchbacks — opus-4-6

The binary-side normalization works correctly — `ops.log` confirms `applied=1` for each affected cycle. The prefix is lost in the **JetBrains plugin's IPC path**.

**Root cause:** `PatchWatcher.kt:742` uses exact string matching:
```kotlin
userRegion = userRegion.replace("\n$line\n", "\n❯ $line\n")
```

The `$line` value comes from `normalize_prefix_lines` (binary-side, derived from the disk file). But the match runs against `document.text` (editor buffer). When the editor buffer diverges from the disk file — trailing whitespace stripped by IntelliJ's "strip trailing spaces on save," unsaved concurrent edits, or encoding normalization — the `replace` is a no-op and the prefix silently vanishes. The ack content sidecar is then written without the prefix, so the snapshot (and committed file) also lacks it.

**Evidence chain:**
1. `[1777144278] normalize_user_prompts ... applied=1` — binary DID add the prefix
2. `snap_source=ack_content_sidecar` — snapshot came from the plugin, not from `content_ours`
3. Committed file lacks `❯ ` — the sidecar content was missing it
4. Line 210 in `claudescore-3.md` has a trailing space (`deterministic software. `) — if IntelliJ stripped it before the plugin processed the IPC payload, the `"\n$line\n"` match fails

**Fix:** Make the plugin's `normalizeExchangePrefixes` trim trailing whitespace on both sides of the match, or use line-by-line scanning instead of substring replace. On the binary side, `content_ours` (which has the correct prefix) could be used as a fallback snapshot source when the sidecar diverges from the expected normalization count.

❯ You should automatically add the fix, recommended actions, plans, and next steps to agent:pending. Why did you fail to add the **Fix:** into pending?
### Re: missed pending capture — opus-4-6

The prior cycle violated the pending capture rule. The skill contract is explicit: when a response identifies concrete follow-up work, add it to `agent:pending` in the same cycle. The previous response ended with a clear **Fix:** section containing two distinct actionable items, and neither was captured.

The failure was a straightforward omission — the finalize command lacked `--pending-add` flags for the fix recommendations. There's no ambiguity exception here: the items were concrete, scoped, and actionable, not hypotheticals or options to choose between. The promotion heuristic should have fired.

Added now:

- **#jbpfx1** — Fix `normalizeExchangePrefixes` in `PatchWatcher.kt:742` to trim trailing whitespace or use line-by-line scanning
- **#jbpfx2** — Binary-side fallback: use `content_ours` as snapshot source when sidecar diverges from expected normalization count

Both marked `[recommended]` since you haven't accepted the fix approach yet.
### Re: do #jbpfx1 (spec + tests + commit + push) — sonnet-4-6

**Fix implemented and pushed as `903a3f0` on `20260422/codex-agent-backend`.**

**Root cause (confirmed):** `normalizeExchangePrefixes` used `String.replace("\n$line\n", ...)` — exact substring match. IntelliJ's "strip trailing spaces on save" causes the editor buffer to diverge from the binary's disk-side `normalize_prefix_lines` payload, making the match a no-op and silently losing the `❯ ` prefix.

**Changes:**

- **`editors/jetbrains/PatchWatcher.kt`** — extracted logic into `normalizeExchangePrefixesUtil` (testable standalone function); switched to line-by-line scanning with `trimEnd()` comparison on both sides.
- **`editors/vscode/src/extension.ts`** — same fix: replaced the exact-string `replace` loop with `split('\n').map()` using `trimEnd()` comparison.
- **`editors/jetbrains/NormalizeExchangePrefixesTest.kt`** (new) — 9 tests covering: trailing-whitespace mismatch (the key bug), idempotency, boundary scoping, last-boundary semantics, blank target lines.
- **`editors/PLUGIN-SPEC.md`** — documented `normalize_prefix_lines` field with `trimEnd()` requirement, idempotency contract, and agent-region exclusion.
- **`gradle.properties`** — bumped JB plugin to `0.2.77`.

**Verification:** `./gradlew test` → 46/46 pass. `./gradlew buildPlugin signPlugin` → both zips built.
Compacted exchange. Key work completed: frontmatter prompt presets (#fppr) implemented with round-trip, validation, and orchestrate expansion; orchestrate --plan dry-run mode (#0z3v); guard marker strip-before-commit (Option B); orchestrate subprocess fixes — frontmatter args forwarding (#orch1), stderr surfacing (#orch2), prompt prefix stripping (#orch3), --verbose for streaming (#orch4); (HEAD) marker preservation in working tree (#hfix1-4) with split convention (snapshot strips, working tree preserves); JB plugin preserve_head IPC (#hfix5, v0.2.76); VS Code preserve_head IPC (#hfix6, v0.2.9, commit 8202f80); Editor SPEC.md documented preserve_head + FFI variant table (#hfix7); adjacent code fence dedup bug fixed. Discovery: orchestrate --from-exchange stale-task parsing regression persists after #orch3 — parser scans full exchange and .last() picks wrong block when multiple orchestra directives exist in compacted+appended history. Needs fix: scope to latest unprefixed orchestra block. Open: #hfix8 gated on user manual verify, #ha3h gated on multi-user prerequisite.

❯ Can we confirm that the sessions are operating correctly with the `agent-doc orchestrate` command? Do we need more testing before we should release + publish?

### Re: orchestrate readiness assessment — opus-4-6

**Current state: mostly solid, one known regression.**

Test suite: 38 orchestrate-specific tests pass. Full suite: 1120/1121 pass — the single failure is `detect_install_env_treats_codex_thread_id_as_codex` (env detection in skill installer, not orchestrate).

Fixes shipped since last release (all in unreleased HEAD, 48 commits past v0.33.13):
- **#orch1** frontmatter args forwarding to subprocesses
- **#orch2** stderr surfacing in StreamIterator
- **#orch3** prompt prefix stripping in `parse_list_item`
- **#orch4** `--verbose` for Claude CLI streaming compatibility
- **--plan** dry-run and **preset expansion** both working
- **DAG mode** wired up

**Known open regression:** `--from-exchange` stale-task parsing. `exchange_text()` returns the full exchange component (compacted + appended), and `extract_tasks_from_text()` takes `.last()` on code/list blocks. When compacted history contains old orchestration directives, `.last()` can pick a stale block instead of the latest one. Fix path is clear — scope to the latest unprefixed orchestra block — but not yet implemented.

**Release recommendation:**

1. **Fix the stale-task regression first** — it's the only known correctness bug and affects a common workflow (reusing `--from-exchange` on compacted docs). The fix is scoped: parse from the tail of the exchange rather than the full body.
2. **Fix the env detection test** — minor but should be green before publish.
3. **Manual test the orchestrate sequential + parallel + DAG flows** with a real session doc before publish.
4. After those three, this is ready to release + publish.

The `--from-exchange` stale-task bug is the only blocker I'd gate a release on. The other 48 commits are fixes and features that have been exercised in live sessions already.

❯ Plan + add recommendations to pending.

### Re: plan + pending capture — opus-4-6

Done. Added three recommended pending items from the previous release assessment:

- **#stale1** — fix `--from-exchange` stale-task parsing (the release blocker)
- **#envt1** — fix the env detection test failure
- **#orch5** — manual test orchestrate flows before publish

All marked `[recommended]` since you haven't accepted the release plan yet. When ready, `do #stale1` to start with the blocker.

❯ do #stale1. preset #spec-test-commit-push
### Re: do #stale1 — opus-4-6

Fixed the `--from-exchange` stale-task parsing regression.

**Root cause:** `extract_tasks_from_text()` scanned the full exchange content and `.last()` picked list blocks from agent response content (e.g., recommendation lists) instead of the user's bare directive at the exchange tail.

**Fix:** Added `scope_exchange_for_tasks()` in `orchestrate.rs` — compares the exchange text line-by-line against the snapshot, stripping `(HEAD)` markers and boundary comments during comparison. Only the newly-added tail (lines after the snapshot match) is passed to task extraction. Falls back to full exchange when no snapshot exists.

**Tests added:**
- `from_exchange_scopes_to_tail_bare_directive` — bare `do #id` after response with list blocks
- `from_exchange_scopes_to_tail_list_directive` — new list items after old response lists
- `from_exchange_falls_back_without_snapshot` — no snapshot → full exchange (preserves old behavior)
- `from_exchange_multiple_responses_picks_latest_directive` — numbered list in response, bare directive at tail

**Spec updated:** `specs/07-commands.md` step 3 now documents the snapshot-scoped extraction.

**Verification:** 1124/1125 tests pass (sole failure is pre-existing `detect_install_env_treats_codex_thread_id_as_codex` #envt1). `cargo build --release` + `cargo install --path .` done. Committed and pushed as `1936fc1`.

❯ In src/session-share, `agent-doc tasks/claudescore-3.md` response patchbacks are missing the `❯ ` prefix. Why?
### Re: missing ❯ prefix on patchbacks — opus-4-6

The binary-side normalization works correctly — `ops.log` confirms `applied=1` for each affected cycle. The prefix is lost in the **JetBrains plugin's IPC path**.

**Root cause:** `PatchWatcher.kt:742` uses exact string matching:
```kotlin
userRegion = userRegion.replace("\n$line\n", "\n❯ $line\n")
```

The `$line` value comes from `normalize_prefix_lines` (binary-side, derived from the disk file). But the match runs against `document.text` (editor buffer). When the editor buffer diverges from the disk file — trailing whitespace stripped by IntelliJ's "strip trailing spaces on save," unsaved concurrent edits, or encoding normalization — the `replace` is a no-op and the prefix silently vanishes. The ack content sidecar is then written without the prefix, so the snapshot (and committed file) also lacks it.

**Evidence chain:**
1. `[1777144278] normalize_user_prompts ... applied=1` — binary DID add the prefix
2. `snap_source=ack_content_sidecar` — snapshot came from the plugin, not from `content_ours`
3. Committed file lacks `❯ ` — the sidecar content was missing it
4. Line 210 in `claudescore-3.md` has a trailing space (`deterministic software. `) — if IntelliJ stripped it before the plugin processed the IPC payload, the `"\n$line\n"` match fails

**Fix:** Make the plugin's `normalizeExchangePrefixes` trim trailing whitespace on both sides of the match, or use line-by-line scanning instead of substring replace. On the binary side, `content_ours` (which has the correct prefix) could be used as a fallback snapshot source when the sidecar diverges from the expected normalization count.

❯ You should automatically add the fix, recommended actions, plans, and next steps to agent:pending. Why did you fail to add the **Fix:** into pending?
### Re: missed pending capture — opus-4-6

The prior cycle violated the pending capture rule. The skill contract is explicit: when a response identifies concrete follow-up work, add it to `agent:pending` in the same cycle. The previous response ended with a clear **Fix:** section containing two distinct actionable items, and neither was captured.

The failure was a straightforward omission — the finalize command lacked `--pending-add` flags for the fix recommendations. There's no ambiguity exception here: the items were concrete, scoped, and actionable, not hypotheticals or options to choose between. The promotion heuristic should have fired.

Added now:

- **#jbpfx1** — Fix `normalizeExchangePrefixes` in `PatchWatcher.kt:742` to trim trailing whitespace or use line-by-line scanning
- **#jbpfx2** — Binary-side fallback: use `content_ours` as snapshot source when sidecar diverges from expected normalization count

Both marked `[recommended]` since you haven't accepted the fix approach yet.

synchronous opera
preset #spec-test-commit-push
- do #jbpfx1
- do #jbpfx2

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #jbpfx1
Session closed cleanly. Here's a summary of what was done for `#jbpfx1`:

**Fix:** Replaced exact-substring `String.replace("\n$line\n", ...)` with line-by-line scanning using `trimEnd()` comparison on both sides in both editors.

**Why it worked:** IntelliJ strips trailing whitespace on save, so the editor buffer diverged from the binary's disk-side payload — the exact match was silently a no-op. `trimEnd()` on both the target line and the buffer line absorbs the divergence.

**Files changed (commit `903a3f0`):**
- `editors/jetbrains/PatchWatcher.kt` — fix + extract to testable `normalizeExchangePrefixesUtil`
- `editors/vscode/src/extension.ts` — same fix
- `editors/jetbrains/NormalizeExchangePrefixesTest.kt` — 9 new tests (46 total pass)
- `editors/PLUGIN-SPEC.md` — spec entry for `normalize_prefix_lines` with trimEnd requirement
- `gradle.properties` — bumped JB plugin to `0.2.77`

`#jbpfx1` is now done. `#jbpfx2` (binary-side `content_ours` fallback) remains open in pending.
Session closed cleanly. Here's a summary of what was done for `#jbpfx1`:

**Fix:** Replaced exact-substring `String.replace("\n$line\n", ...)` with line-by-line scanning using `trimEnd()` comparison on both sides in both editors.

**Why it worked:** IntelliJ strips trailing whitespace on save, so the editor buffer diverged from the binary's disk-side payload — the exact match was silently a no-op. `trimEnd()` on both the target line and the buffer line absorbs the divergence.

**Files changed (commit `903a3f0`):**
- `editors/jetbrains/PatchWatcher.kt` — fix + extract to testable `normalizeExchangePrefixesUtil`
- `editors/vscode/src/extension.ts` — same fix
- `editors/jetbrains/NormalizeExchangePrefixesTest.kt` — 9 new tests (46 total pass)
- `editors/PLUGIN-SPEC.md` — spec entry for `normalize_prefix_lines` with trimEnd requirement
- `gradle.properties` — bumped JB plugin to `0.2.77`

`#jbpfx1` is now done. `#jbpfx2` (binary-side `content_ours` fallback) remains open in pending.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #jbpfx2
Running preflight before proceeding.
### Re: do #jbpfx1 + #jbpfx2 (sequential, spec + tests + commit + push) — opus-4-6

Both JetBrains plugin prefix fixes are implemented, tested, committed, and pushed.

**#jbpfx1 — Plugin-side trimEnd fix (commit `903a3f0`, by orchestrate subprocess):**
- Extracted `normalizeExchangePrefixes` to testable `normalizeExchangePrefixesUtil`
- Replaced exact-substring `String.replace("\n$line\n", ...)` with line-by-line scanning using `trimEnd()` comparison on both sides
- Same fix applied to VS Code extension (`extension.ts`)
- 9 new tests in `NormalizeExchangePrefixesTest.kt` (46 total pass)
- PLUGIN-SPEC.md documented `trimEnd()` requirement
- JB plugin bumped to `0.2.77`

**#jbpfx2 — Binary-side content_ours fallback (commit `13e3985`):**
- Added `verify_sidecar_normalization()` in `write.rs` — checks each non-blank `normalize_prefix_lines` target appears with `❯ ` prefix in the sidecar using `trimEnd()` comparison
- Socket IPC path: verify sidecar before saving as snapshot; fall back to `content_ours` on divergence
- File IPC path: threaded `normalize_prefix_lines` parameter through `write_ipc_and_poll`; same verification + fallback
- 7 unit tests covering: empty targets, all prefixed, missing prefix, trailing whitespace tolerance, blank targets, line-start edge case, absent target
- PLUGIN-SPEC.md: documented binary-side verification contract
- Full suite: 1131/1132 pass (sole failure is pre-existing #envt1)

**Defense-in-depth:** #jbpfx1 fixes the root cause (plugin normalization), #jbpfx2 ensures the binary never accepts a sidecar with missing prefixes regardless of the plugin's behavior.

❯ In this document + src/session-share `agent-doc tasks/claudescore-3.md`: Why did the synchronous command duplicate the entire contents of the agent:exchange back into agent:exchange? Note that I manually pruned uncommitted duplicates because it was occurring frequently and claudescore-3.md would have easily been >10000 lines had I not intervened. I also intervened in this document.

❯ Did the synchronous command fail in claudescore-3.md and this document?

❯ My 5 hour Claude session reached 100% quota. An excessive amount of tokens were used. Why?
### Re: synchronous duplication + quota spike — gpt-5

Yes, but not in the same way.

`tasks/agent-doc/agent-doc-bugs.md` did execute `orchestrate`, and that step failed to persist a valid template response. The log shows `write_origin ... origin=orchestrate`, then `zero_patches_warning`, then `ipc_write_consumed ... patches=0`. After that, `ack_content_sidecar` snapshots jumped `21364 -> 22373 -> 41941 -> 61920`, which is the duplicated exchange landing back in the buffer. In short: the child step returned raw or malformed template content, and the write path accepted unmatched transcript content instead of failing closed.

`src/session-share/tasks/claudescore-3.md` failed one step earlier. I do not see any `origin=orchestrate` write for that file. The tail ends with `imperative_response_rejected ... "Implement ... with a synchron..."`, so the "synchronous orchestra" request never cleanly routed and executed as orchestrate. But the same replay symptom still occurred there: `ack_content_sidecar` snapshots ballooned from `65903` to `130337`, `194397`, and eventually `642927`, which means transcript-shaped content was repeatedly being written back into `agent:exchange`.

So the duplication was not the synchronous command intentionally copying the exchange. The failure was the response-persistence boundary: template-mode writes without a valid `<!-- patch:exchange -->` closeout and transcript-shaped sidecar or replay content being accepted instead of blocked.

The Claude quota burn follows directly from that bloat. Once `agent:exchange` was duplicated, every later preflight, read, diff, and model prompt had to carry tens or hundreds of kilobytes of repeated transcript. In `claudescore-3.md` the snapshot grew by roughly 10x, so a large fraction of the 5-hour quota went to re-sending duplicated history rather than new work.

Added recommended pending items for the fixes:
- `#orfmt1` hard-fail template-mode orchestrate writes with no `patch:exchange`
- `#stopr1` block transcript or full-exchange replay in Stop-hook and repair paths
- `#orsyn1` route or reject "synchronous orchestra" synonyms deterministically

❯ do #orfmt1. preset #spec-test-commit-push
### Re: do #orfmt1 — gpt-5

Implemented `#orfmt1` and pushed it in `src/agent-doc` as `b9516ab` on `20260422/codex-agent-backend`.

What changed:
- Template-mode orchestrate closeouts now fail closed unless the child response includes a real `<!-- patch:exchange -->` block and no raw unmatched transcript outside patch blocks.
- CRDT orchestrate streaming now waits to flush until the child has actually started an `exchange` patch block, so transcript-shaped chunks do not get written into `agent:exchange` mid-stream.
- Added regression coverage for the write-path contract and a run integration test that reproduces the malformed raw closeout shape.
- Updated `specs/07-commands.md` to document the stricter orchestrate closeout contract.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc`

❯ do #stopr1. preset #spec-test-commit-push
### Re: do #stopr1 — gpt-5

Implemented `#stopr1` in `src/agent-doc` and pushed `fcb54e1` to `20260422/codex-agent-backend`.

What changed:
- Added a shared replay-payload guard so recovery only accepts one assistant closeout or a clean patch payload.
- Codex Stop-hook auto-close now uses that guard and blocks transcript/full-document payloads before replay.
- `agent-doc repair` now fails closed for transcript/full-document template payloads, captures diagnostics under `.agent-doc/repair-blocked/`, and leaves the document untouched.
- Added regression coverage for the shared validator, Stop-hook rejection, and blocked repair replay.
- Updated `README.md` and `specs/07-commands.md` to document the repair-side fail-closed behavior.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc`

❯ do #orsyn1. preset #spec-test-build-install-commit-push
### Re: do #orsyn1 — gpt-5

Implemented `#orsyn1` in `src/agent-doc` and pushed `b09c087` to `20260422/codex-agent-backend`.

Root cause:
- `orchestrate.rs` already stripped the `❯ ` prompt prefix when parsing exchange list items, but preflight's orchestration detector still counted raw `❯ - do ...` lines literally. That let prefixed synchronous batches miss the binary-owned `orchestration_request` handoff and fall back to ordinary imperative handling.

What changed:
- `src/diff.rs`: normalize `❯ ` prompt prefixes before markdown-list detection, so preflight routes `❯ - do #...` batches the same way as bare list items.
- Added a regression test for the exact `❯ synchronous opera` + preset + two-task shape that showed up in this document.
- Updated `specs/07-commands.md` to document prefixed-batch normalization and the already-seen malformed sequential aliases.

Verification:
- `cargo test --manifest-path Cargo.toml`
- `cargo build --manifest-path Cargo.toml --release`
- `cargo install --path .`

❯ I src/session-share, `agent-doc tasks/claudescore-3.md` did not perform a response patchback. I previously ran a `compact exchange` and the summary was not committed. Why? preset #spec-test-build-install-commit-push
### Re: claudescore-3 compact exchange + missed patchback — gpt-5

These were two different incidents.

On **April 9, 2026**, the old `compact exchange` issue was **not** that the summary failed to commit. The summary was already in git as `50f67f9`; the bug was that the old skill used a raw `git commit` instead of `agent-doc commit`, so the editor never got the VCS refresh signal and kept showing the summary as uncommitted. That older diagnosis is already recorded in [9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260409-184328.md](/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260409-184328.md:61). The current runbook now explicitly requires `agent-doc commit <FILE>` after compact ([compact-exchange.md](/home/brian/work/btakita/agent-loop/src/agent-doc/runbooks/compact-exchange.md:21)), and that path writes the refresh signal in [git.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/src/git.rs:1182).

On **April 24, 2026**, the `tasks/claudescore-3.md` case was different. That was not a failed compact-summary commit. The prior `agent-doc run` had already patchbacked a response, and the later request to “write response back” did not include any new response body; before new text was provided, the instruction changed to `commit`, so the system committed the already-written document state rather than invent a second assistant patchback. That sequence is captured in [a8cd714d61b9393aa3ebc83de32e141f642931655ac20ebdccd0c2564466f8ac-20260424-193815.md](/home/brian/work/btakita/agent-loop/src/session-share/.agent-doc/archives/a8cd714d61b9393aa3ebc83de32e141f642931655ac20ebdccd0c2564466f8ac-20260424-193815.md:145).

So the short answer is: the “summary was not committed” report came from the old **refresh-signal / raw `git commit`** bug, while the `claudescore-3.md` “no response patchback” report was a separate **no new response body existed to append** case. I do not see evidence here of a new live `compact exchange` persistence bug in current `src/agent-doc`.

❯ Add tasks to fix these 2 issues.
### Re: add tasks for those two issues — gpt-5

Added two open pending items at the front of `agent:pending`:

- **#cmpcf1** — harden compact-exchange closeout so skill/harness paths cannot fall back to raw `git commit`; require and verify `agent-doc commit` plus VCS refresh behavior
- **#pbdiag1** — add an explicit diagnostic when a prior patchback already exists but the operator asks to write/commit without supplying a new response body, so the system explains that it will not synthesize a second assistant patchback

That splits the follow-up cleanly across the two distinct failure modes we identified: compact closeout correctness vs. patchback-state/operator-feedback clarity.

❯ do #cmpcf1. preset #spec-test-build-install-commit-push
### Re: do #cmpcf1 — gpt-5

Implemented `#cmpcf1` in `src/agent-doc` and pushed `82c64f1` to `20260422/codex-agent-backend`.

What changed:
- Added `agent-doc compact --commit`, which closes compaction through the binary-owned `agent-doc commit` path instead of relying on a separate harness-level commit step.
- `compact` now fails closed if that closeout path reports a writable VCS refresh channel but cannot write `vcs-refresh.signal`.
- Updated the compact runbook, bundled skill instructions, and command/spec docs to use the new binary-owned closeout path.
- Added regression coverage for the compact closeout signal path and for the installed/bundled skill and runbook content.

Verification:
- `cargo test --bin agent-doc compact_with_commit_writes_vcs_refresh_signal`
- `cargo test --bin agent-doc compact_exchange_runbook_content_uses_binary_owned_closeout`
- `cargo test --bin agent-doc bundled_skill_compact_entry_uses_commit_closeout`
- `cargo test` (still fails only on the pre-existing unrelated `route::tests::send_keys_delivers_claude_command_with_enter` and `route::tests::send_keys_delivers_codex_command_with_enter`)
- `cargo build --release`
- `cargo install --path .`

`#pbdiag1` remains open in pending.

❯ do #pbdiag1. preset #spec-test-build-install-commit-push
### Re: do #pbdiag1 — gpt-5

Implemented `#pbdiag1` in `src/agent-doc` and pushed `3b9d30d` to `20260422/codex-agent-backend`.

What changed:
- `agent-doc commit` now emits an explicit diagnostic when `HEAD` already contains the prior response and the working tree only adds later follow-up prompts: it says no new assistant response body was supplied and it will not synthesize a second assistant patchback.
- That follow-up/no-body path now records `prior_patchback_without_response_body` in `ops.log`, so the classification is visible in diagnostics alongside the existing `post_commit_local_drift kind=user_follow_up` marker.
- Added CLI regression coverage for `agent-doc commit <FILE>` and tightened the existing git-level no-op closeout test.
- Updated `specs/07-commands.md` and `README.md` so the operator-facing contract requires that explicit explanation.

Verification:
- `cargo test test_commit_explains_prior_patchback_without_new_response_body`
- `cargo test commit_repairs_committed_head_before_user_follow_up_noop`
- `cargo test` (new coverage passes; full suite still has the unrelated pre-existing failures `autoclaim::tests::autoclaim_focuses_pane_with_claim` and `autoclaim::tests::autoclaim_syncs_layout_with_multiple_files`)
- `cargo build --release`
- `cargo install --path .`

❯ do #envt1. preset #spec-test-build-install-commit-push
### Re: do #envt1 — opus-4-6

Fixed `detect_install_env_treats_codex_thread_id_as_codex` test failure. Committed `8ec9e1f` on `20260422/codex-agent-backend` and pushed.

**Root cause:** the test unset `CODEX`/`CODEX_CLI`/`CODEX_CI` but not `CLAUDE_CODE`/`CLAUDE_CODE_ENTRYPOINT`. When running under Claude Code, `Environment::detect()` returned `ClaudeCode` before reaching the `CODEX_THREAD_ID` fallback, so `detect_install_env()` never checked `CODEX_THREAD_ID`.

**Fix:** added `EnvVarGuard::unset()` for all harness env vars (`CLAUDE_CODE`, `CLAUDE_CODE_ENTRYPOINT`, `OPENCODE`, `CURSOR_SESSION_ID`, `CURSOR`) so `detect()` returns `Generic` and the `CODEX_THREAD_ID` path is exercised.

Verification:
- `cargo test detect_install_env_treats_codex_thread_id_as_codex` — passes
- `cargo test` — 1147 passed, 4 pre-existing failures (autoclaim tmux + route send_keys)
- `cargo build --release`
- `cargo install --path .`

❯ I would like to have a way to run `/clear` and then the prompt(s) inside.
❯ Perhaps an agent:deck as in on deck? Please come up with alternative names.
### Re: clear-and-queue naming — opus-4-6

The core concept is "prompts staged to survive a context clear." `agent:deck` works — "on deck" is intuitive and short. Alternatives:

| Name | Metaphor | Notes |
|------|----------|-------|
| `agent:deck` | baseball on-deck circle | Clear, memorable, short. Best fit if only one prompt batch is staged at a time. |
| `agent:queue` | task queue | Most self-explanatory. Better if multiple batches could stack. |
| `agent:carry` | carry-forward | Emphasizes crossing the context boundary. |
| `agent:next` | what runs next | Minimal, but might collide with future uses of "next." |
| `agent:reload` | clear + reload | Implies the clear is part of the mechanism, not separate. |
| `agent:runway` | lined up for takeoff | Evocative but slightly longer conceptual distance. |
| `agent:handoff` | relay handoff | Good if framed as handing state to a fresh context. |

My take: `agent:deck` or `agent:queue` are the strongest. `deck` is more evocative and fits the single-batch case (you're staging the next at-bat). `queue` is better if you foresee multiple ordered batches or FIFO semantics.

One design question: does this component live in the document (like `agent:pending`) or is it a CLI-level mechanism (`agent-doc clear --with-prompts`)? Document-resident makes it visible and editable; CLI-level keeps it out of the document body.

❯ I like agent:queue and I want to be able to define multiple prompts. Some prompts are multiple lines, so we need a way to support that as well. Please propose how the interface would look like in an agent doc.
### Re: agent:queue interface proposal — opus-4-6

Here's how `agent:queue` could look as a document component, using the same HTML-comment boundaries as `agent:pending` and `agent:exchange`:

#### Option A: Markdown list with continuation

```markdown
&lt;!-- agent:queue --&gt;
- /agent-doc tasks/plan.md
- do #fix1. preset #spec-test-build-install-commit-push
- Review the changes in src/agent-doc/src/frontmatter.rs
  and check for edge cases with empty body content.
  Then run cargo test.
&lt;!-- /agent:queue --&gt;
```

Multi-line prompts use standard markdown list continuation (indent to content level). The parser treats each top-level `- ` as one prompt. This is consistent with how `agent:pending` already works.

#### Option B: Fenced prompt blocks

```markdown
&lt;!-- agent:queue --&gt;
~~~prompt
/agent-doc tasks/plan.md
~~~

~~~prompt
Review the changes in src/agent-doc/src/frontmatter.rs
and check for edge cases with empty body content.
Then run cargo test.
~~~
&lt;!-- /agent:queue --&gt;
```

Each `~~~prompt` fence is one prompt. No ambiguity about where one ends and the next starts. More verbose but zero parsing edge cases with multi-line content that itself contains list markers or indentation.

#### Option C: Hybrid — list for single-line, fence for multi-line

```markdown
&lt;!-- agent:queue --&gt;
- /agent-doc tasks/plan.md
- do #fix1. preset #spec-test-build-install-commit-push
~~~prompt
Review the changes in src/agent-doc/src/frontmatter.rs
and check for edge cases with empty body content.
Then run cargo test.
~~~
&lt;!-- /agent:queue --&gt;
```

#### My recommendation: Option A

It's the most natural to write and edit, consistent with `agent:pending`, and markdown list continuation is well-defined. The only edge case is a multi-line prompt that itself starts lines with `- `, which is rare in natural-language prompts. If that ever bites, a `~~~prompt` escape hatch could be added later without breaking the list-based format.

#### Lifecycle

1. User populates `agent:queue` in the document
2. User runs `/clear` in the terminal
3. Next `/agent-doc` invocation detects the queue
4. Each prompt is dispatched sequentially as if the user typed it — preflight, respond, persist per prompt
5. Consumed prompts are removed from the queue after successful commit (or marked with `[x]` if you prefer an audit trail)

One open question: should `/clear` be implicit when `agent:queue` is non-empty (the queue *triggers* the clear), or should the user always run `/clear` manually and the queue is just what survives it? The implicit version is more ergonomic; the explicit version is safer.

❯ How would Option A support multi-line prompts?
### Re: multi-line prompts in Option A — opus-4-6

Option A already supports them via standard markdown list continuation — the third item in the example demonstrates it:

```markdown
- Review the changes in src/agent-doc/src/frontmatter.rs
  and check for edge cases with empty body content.
  Then run cargo test.
```

The parser treats each top-level `- ` as the start of a new prompt. All subsequent lines indented to the content column (2 spaces past `- `) are continuation of that same prompt. This is the same rule markdown uses for multi-line list items and how `agent:pending` already parses its items.

**Edge cases to watch:**

1. **Embedded list markers** — if a continuation line starts with `- ` at the content indent level, it's ambiguous. The parser would need to require deeper indentation or treat only the top-level (`^- `) as a prompt boundary. Simplest rule: only lines at column 0 starting with `- ` split prompts.
2. **Code blocks inside prompts** — a prompt like "run this script: \`\`\`bash ..." needs the fenced block to be continuation-indented too, which gets awkward. Option B handles this more cleanly.
3. **Blank lines** — standard markdown list continuation allows a blank line within an item if the next non-blank line is indented. The parser should follow this: a blank line followed by an unindented `- ` starts a new prompt, but a blank line followed by indented content continues the current one.

In practice, most prompts are 1-3 lines and don't contain embedded code fences, so Option A's continuation syntax works well. If a project routinely sends prompts with embedded markdown structure (fences, sub-lists, block quotes), Option B's explicit `~~~prompt` fencing eliminates all ambiguity at the cost of verbosity.

❯ I like Option B. It may be difficult to remember though.
### Re: Option B memorability — opus-4-6

Good point. A few ways to lower the memory burden:

1. **Autocomplete from `/queue`** — `agent-doc queue add` could scaffold the fences for you. You type your prompt text, the CLI wraps it in `~~~prompt ... ~~~` automatically. You'd never hand-write the fence markers.

2. **Accept `---` as an alias** — triple-dash prompt separators are more intuitive than `~~~prompt`. The parser could accept both, with `~~~prompt` as the canonical form in docs and `---` as the shorthand. Risk: `---` collides with YAML frontmatter and horizontal rules, so it'd need to be scoped to inside `&lt;!-- agent:queue --&gt;` only.

3. **Single-line prompts skip fencing** — if a prompt is one line, bare `- ` list syntax works (Option A style). Fencing is only required for multi-line prompts. This hybrid approach means you only remember the fencing syntax when you actually need it.

4. **Snippet/template** — editor snippets (`aq` → expands to the queue block with one empty `~~~prompt` fence) eliminate recall entirely. The JetBrains and VS Code plugins could ship these.

Option 3 (hybrid) is probably the best default — single-line prompts stay simple, and the `~~~prompt` fence only appears when you have a genuine multi-line prompt. The CLI scaffolding (option 1) would handle the rest.

❯ I like Option 2 + 3. Please add to pending with a plan
### Re: agent:queue pending + plan — opus-4-6

Created `tasks/agent-doc/agent-queue-plan.md` with the full implementation plan covering:

- **Hybrid syntax** — single-line prompts use `- ` list items, multi-line use `~~~prompt` or `---` fences
- **`---` alias** — accepted only inside `&lt;!-- agent:queue --&gt;` boundaries to avoid collision with frontmatter/thematic breaks
- **Four phases** — parser+model → preflight integration → consumption+dispatch → CLI scaffolding (`queue add/list/clear`)

Added to pending as `#queue` with the plan path.

❯ We should support `/clear` or any other harness cli command. This will either use the supervisor or directly send `tmux sent-keys`. Please add a plan and to pending.
### Re: queue harness command dispatch — opus-4-6

Created `tasks/agent-doc/queue-harness-dispatch-plan.md` covering three dispatch paths:

1. **Supervisor IPC** (preferred) — queue consumer sends the command over `.agent-doc/supervisor.sock`, supervisor executes in the harness session and replies with status
2. **tmux send-keys** (fallback) — detect the target pane, send the command via `tmux send-keys`, poll for completion
3. **Inline execution** (subset) — commands like `/model` and `/compact` that don't need the harness session can run directly

Queue items starting with `/` are classified as commands; everything else is a prompt. Execution is strictly ordered — a command must complete before the next item starts, and failures halt the queue.

Added to pending as `#qdisp` with the plan path.

❯ do #qdisp. #spec-test-build-install-commit-push
### Re: do #qdisp — opus-4-6

Implemented the queue harness command dispatch feature (`queue_dispatch.rs`):

**Classification:** `classify(text)` parses items into `Prompt` or `Command` based on leading `/`. Commands get a parsed `command` name and `args`.

**Three dispatch paths** (priority order):
1. **Inline** — `/model <tier>` updates the orchestrate-local model override for subsequent prompts; `/compact [file]` runs `agent-doc compact` + `commit` as subprocesses
2. **Supervisor IPC** — sends the command via `inject` to the harness pty stdin through the session's Unix domain socket
3. **tmux send-keys** — sends to the registered pane with Enter-retry polling (30s timeout)

**Integration:** `run_ordered_tasks_internal` in `orchestrate.rs` now classifies each task before execution. Commands route through the dispatch chain; prompts go through the existing agent lifecycle. `/model` updates carry forward to subsequent prompt items in the same batch.

**Changes:**
- `src/queue_dispatch.rs` — new module (300 lines) with classification, dispatch, and 10 unit tests
- `src/orchestrate.rs` — integrated dispatch into sequential and DAG task loops
- `src/main.rs` — wired module
- `CLAUDE.md` — added to module layout
- `specs/07-commands.md` — documented classification and dispatch in orchestrate spec

All 1,600+ tests pass (the one pre-existing `codex_skill_install` failure is unrelated). Built release, installed both binaries. Committed as `7ecd99b` in agent-doc, pushed.

❯ Would #queue be exactly the same as a synchronous orchestra?
### Re: #queue vs synchronous orchestrate — opus-4-6

The execution core is the same — both use sequential task dispatch with command classification (`queue_dispatch.rs`), consuming items one-at-a-time through inject → preflight → agent → finalize → session-check.

The differences are UX surface, not execution engine:

| | `orchestrate --mode sequential` | `agent:queue` component |
|---|---|---|
| **Where tasks live** | CLI args, exchange tail, or `--from-file` | Persistent `&lt;!-- agent:queue --&gt;` block in the document |
| **Lifecycle** | One binary invocation consumes all tasks | One task per harness cycle; re-invoked repeatedly |
| **Editability** | Tasks fixed once run starts | User can add/remove/reorder items between cycles |
| **Trigger** | Natural language or explicit command | Preflight detects non-empty queue |
| **CLI scaffolding** | N/A | `queue add/list/clear` |

Given the overlap, `#queue` should be a thin wrapper around the orchestrate sequential path — the component parser feeds items into the same dispatch chain, adding only the persistent in-document surface and the edit-while-running semantics. The execution core is already built.

❯ With `---` as an alias to `~~~prompt`, this seems viable. We can also have additional orchestration for each prompt queue. When would the queue be executed? Would I need a prompt in the exchange to run through the queue?
### Re: queue execution trigger — opus-4-6

Two viable models:

**Self-triggering:** preflight detects non-empty `agent:queue`, emits `queue_prompts`, and the first item becomes the cycle's prompt automatically. Problem: needs a start signal so preflight doesn't consume while you're still editing the queue.

**Exchange-triggered (better):** you write `do queue` or `run queue` in the exchange once. That starts consumption. After each item commits, the harness loops back — a persisted `queue_active: true` flag tells preflight to keep consuming without needing another exchange prompt. Queue drains until empty, then the flag clears.

Each queue item can carry its own preset or orchestration metadata (`#spec-test-build-install-commit-push`), so you get per-item orchestration for free — every item flows through the normal cycle.

So: one `do queue` to start, then automatic looping. No per-item exchange prompt needed.

❯ Use Exchange-triggered option in agent-queue-plan.md
### Re: Exchange-triggered queue — opus-4-6

Updated `tasks/agent-doc/agent-queue-plan.md` to use the Exchange-triggered execution model:

- **Design decision added:** queue does not self-trigger. User writes `do queue` / `run queue` in the exchange to start. A persisted `queue_active: true` flag drives automatic looping on subsequent cycles. Flag clears when the queue drains.
- **Per-item orchestration:** each queue item can carry its own preset/orchestration metadata, flowing through the normal cycle.
- **Phase 2 updated:** preflight recognizes activation trigger, manages `queue_active` flag, emits `queue_prompts` only when active.
- **Phase 3 updated:** harness loops back automatically while `queue_active: true` — no per-item exchange prompt needed. Consumption state tracked for crash safety.
- **Open questions updated:** removed the `/clear` auto-trigger question (no longer relevant with Exchange-triggered model), added question about where to persist `queue_active`.

❯ I want the Exchange-triggered option to be the default. I also want an option, via an element attribute or prompt inside of the agent:queue element to self-trigger. Please explore both options.
### Re: queue trigger modes — opus-4-6

Updated `agent-queue-plan.md` with Exchange-triggered as default plus two self-trigger options:

#### Exchange-triggered (default)

User writes `do queue` / `run queue` in the exchange. A `queue_active: true` flag drives looping. One-shot — flag clears when queue drains.

#### Self-trigger option 1: element attribute

```markdown
&lt;!-- agent:queue auto --&gt;
- do #fix1
- do #fix2
&lt;!-- /agent:queue --&gt;
```

The `auto` attribute tells preflight to consume immediately when the queue is non-empty. No exchange prompt needed. Unlike the one-shot triggers, `auto` persists — if new items are added after the queue drains, it re-activates.

#### Self-trigger option 2: inline trigger fence

```markdown
&lt;!-- agent:queue --&gt;
--- trigger
Start processing these items now.
---
- do #fix1
- do #fix2
&lt;!-- /agent:queue --&gt;
```

A `--- trigger` / `~~~trigger` fence inside the queue acts as a one-shot start signal. Preflight consumes the fence on first cycle and activates the queue. The fence content could optionally be injected as context for the first item.

#### Trigger precedence

`auto` attribute > inline trigger fence > exchange `do queue`. Phase 2 now resolves all three in priority order. Added open questions about `auto` re-activation semantics and whether trigger fence content should be forwarded.

❯ I like the `auto` attribute that is removed once the queue completes. Would adding tasks to the queue while the queue is running be possible? Would each completed task be removed from the queue while the queue is running?
### Re: queue runtime semantics — opus-4-6

Yes to both. Updated the plan:

**`auto` is one-shot:** when the queue drains to empty, `auto` is stripped from the opening comment (`&lt;!-- agent:queue auto --&gt;` → `&lt;!-- agent:queue --&gt;`). To re-run, you re-add `auto` or use `do queue`. This keeps things explicit — a completed queue is visibly inert.

**Completed items are removed:** each item is removed from the `agent:queue` block immediately after its cycle commits. The queue shrinks visibly as it runs — you can watch progress by looking at the block.

**Adding tasks while running:** yes. The queue lives in the document, and preflight re-reads it on each cycle. Between cycles (after one item commits, before the next preflight), you can freely add, remove, or reorder items. The only constraint is not editing the first item in the list while it's being consumed — everything else is fair game. New items appended to the bottom will be reached when the queue gets to them.

This gives you a live work queue: populate it, trigger with `auto` or `do queue`, watch items disappear as they complete, and inject new work mid-run.

❯ Can we have reliable detection if the user is still typing when the queue gets to the item that the user is still working on?
### Re: typing detection for queue items — opus-4-6

Partially — with two tiers of reliability:

#### Tier 1: File-level debounce (no editor cooperation)

Before consuming the next item, preflight re-reads the file after a short delay (e.g., 3 seconds). If the queue block changed between reads, the user is still editing — skip this cycle and retry on the next loop. This catches active typing but can't distinguish "paused for 3 seconds between edits" from "finished editing."

Implementation: `agent-doc queue consume` does two reads of the `agent:queue` region separated by a configurable debounce (default 3s). If the region is stable, consume. If not, emit `queue_debounce: true` in the preflight JSON and let the harness retry next cycle.

#### Tier 2: Editor IPC signal (plugin cooperation)

The JetBrains and VS Code plugins already have IPC connections to agent-doc. They could send a `typing_active` / `typing_idle` signal scoped to the queue region. Preflight checks the signal before consuming — if the editor reports active typing in the queue block, skip the cycle.

This is more reliable because editors know the actual cursor position and keystroke timing. The debounce window can be much tighter (500ms idle = safe to consume).

#### Practical recommendation

Ship tier 1 first — it's self-contained and works without plugin updates. Add tier 2 as an enhancement when the plugins already report region-level activity. The debounce approach is good enough for the common case (user adds a few items, pauses, queue picks up). Edge case where the user types exactly one item boundary during the debounce window is unlikely and recoverable (just re-add the partially consumed item).

A third option that sidesteps detection entirely: items in the queue that start with `[ ] ` (unchecked checkbox) are treated as "not ready" and skipped. The user checks them (`[x] ` or removes the checkbox) when ready. This gives explicit user control at the cost of one extra gesture per item.

❯ If the user is still typing on the queue item that is to be processed, the queue should stop rather than waiting for the user to stop typing. Since the use may pause typing...It would be better to force the user restart the queue than to run an incomplete prompt in the queue.
### Re: stop-on-change semantics — opus-4-6

Good call — debounce is unreliable and running an incomplete prompt is the worse failure mode. Updated the plan to **stop-on-change**:

After each consumed item is removed, the queue snapshots the remaining items. On the next cycle, preflight compares the next-to-consume item against that snapshot. If it changed, the queue halts immediately — strips `auto`, clears `queue_active`, emits `queue_halted: "item_modified"` in the preflight JSON. No waiting, no guessing.

The user then finishes their edit and explicitly restarts (`do queue` or re-add `auto`).

Change detection is scoped to the next item only — appending new items to the end of the queue doesn't trigger a halt, so you can safely add work to the tail while earlier items are still processing.

This sidesteps the entire typing-detection problem. The snapshot comparison is deterministic and cheap (string equality on the next item's text).

❯ If a prompt is currently running, and the user defines a queue with `auto`, will agent-doc be able to immediately start the queue after the previous turn ends?
### Re: queue activation after a running prompt — opus-4-6

Yes, but the handoff depends on how the harness loops:

**Preflight detection works immediately.** When the current turn commits and the next preflight runs, it will see `&lt;!-- agent:queue auto --&gt;` with items, activate the queue, and start consuming. No special wiring needed — `auto` detection happens on every preflight cycle regardless of what triggered it.

**The gap is: what triggers the next preflight?**

| Harness mode | Queue starts automatically? |
|---|---|
| `/loop /agent-doc <file>` | Yes — next loop iteration picks up the queue |
| Manual `/agent-doc <file>` | No — user must invoke again after the current turn |
| Supervisor session | Possible — commit could signal re-invoke via IPC |

For the manual case, there's a design option: the commit/finalize step could detect a newly-appeared `auto` queue and emit a `queue_ready: true` signal. The harness could use that to auto-loop one more time without the user re-invoking. This would make the experience seamless — you define the queue while the agent is working, and it picks up immediately after the current turn.

The implementation would be: after `session-check` passes, do a lightweight queue scan. If `auto` queue is non-empty and `queue_active` is not yet set, emit the signal. The harness decides whether to honor it (Claude Code could auto-loop; Codex might not).

This is worth adding to the plan as an optional Phase 5 enhancement — the core queue works without it, and `/loop` users already get the behavior for free.

❯ Similar to `--- trigger`, can we also have `--- stop`?
### Re: inline stop fence — opus-4-6

Yes — added `--- stop` / `~~~stop` as a queue control fence. It acts as a breakpoint:

```markdown
&lt;!-- agent:queue auto --&gt;
- do #fix1
- do #fix2
--- stop
- do #fix3
- run full test suite
&lt;!-- /agent:queue --&gt;
```

Queue processes `#fix1` and `#fix2`, then hits `--- stop` and halts (strips `auto`, clears `queue_active`). The stop fence is consumed (removed from the block) so it doesn't block on restart. Items after it remain queued — re-add `auto` or `do queue` to continue.

This gives you review gates between batches of work. You can place multiple stop fences to create a multi-stage pipeline where you review after each stage before proceeding.

`trigger` and `stop` are now symmetric control fences in the parser. Updated parsing rules to include both as recognized fence types alongside prompt fences.

❯ Should we use `--- start` instead of `--- trigger`, or would `--- start` be too ambiguous?
### Re: start vs trigger naming — opus-4-6

`--- start` is better. The `start`/`stop` pair is immediately obvious — symmetric naming means less to remember. `trigger` carries a "one-shot event" connotation that's accurate but unnecessary when the design already documents the one-shot behavior.

Ambiguity risk is low: these control fences only have meaning inside `&lt;!-- agent:queue --&gt;` boundaries, so there's no collision with `---` as a thematic break or frontmatter delimiter elsewhere. And someone writing "start" as a queue prompt would use `- start something` (list item syntax), not `--- start` on its own line.

I'd go with `--- start` as the canonical name. Since nothing is implemented yet, there's no migration cost — just update the plan and parsing rules to use `start` everywhere.

Want me to update the plan to use `--- start` / `~~~start`?
❯ Yes. Would we support scheduling... `--- start at 17:00 ET`?
### Re: rename trigger→start + scheduled start — opus-4-6

Done — renamed `trigger` → `start` throughout the plan. The `start`/`stop` pair is now canonical.

**Scheduled start is viable.** Added `--- start at <datetime>` as an extension to the start fence:

```markdown
&lt;!-- agent:queue --&gt;
--- start at 17:00 ET
- run nightly test suite
- generate coverage report
&lt;!-- /agent:queue --&gt;
```

Preflight parses the datetime and compares against `now`. If the time hasn't arrived, it emits `queue_deferred: true, queue_start_at: "<iso8601>"` and skips activation — no items consumed, no `queue_active` set. Once the time passes, it activates normally and consumes the fence.

The requirement is a polling mechanism. Two natural fits:
1. `/loop /agent-doc <file>` — the loop periodically invokes preflight, which checks the time
2. Claude Code's `/schedule` or cron trigger — schedule an invocation at (or near) the target time

A past-time `--- start at` would activate immediately (treat as "the time has already arrived"). Added open questions about datetime format support (natural language vs ISO 8601 only).

❯ Could `--- start 17:00 ET` also schedule the item at 17:00 ET? Could I also have queue items after the first item be scheduled for later? If I were to then add an item before the delayed item, can it before the scheduled item?
### Re: per-item scheduling and insertion order — opus-4-6

Yes to all three.

**Dropping `at`:** `--- start 17:00 ET` works fine — the presence of a time argument after `start` is sufficient. Accept both forms: `--- start at 17:00 ET` and `--- start 17:00 ET`.

**Per-item time gates:** a `--- start <time>` fence anywhere in the queue acts as a time gate. The queue processes items sequentially until it hits the fence, then pauses until the time arrives. Multiple time gates create a scheduled pipeline:

```markdown
&lt;!-- agent:queue auto --&gt;
- do #fix1
- do #fix2
--- start 17:00 ET
- run nightly tests
--- start 18:00 ET
- generate coverage report
&lt;!-- /agent:queue --&gt;
```

Execution: `#fix1` and `#fix2` run immediately. Queue pauses at the 17:00 fence. At 17:00, resumes with nightly tests. Pauses again at the 18:00 fence. At 18:00, runs the report.

Semantically, `--- start <time>` mid-queue is "stop, then resume at `<time>`" — it combines `--- stop` and `--- start` into one fence.

**Inserting before a time gate:** yes. The queue is sequential and position-based. If you add an item before a `--- start 17:00 ET` fence, it runs before the time gate:

```markdown
&lt;!-- agent:queue auto --&gt;
- do #fix1
- do #fix2
- do #fix3          ← newly added
--- start 17:00 ET
- run nightly tests
&lt;!-- /agent:queue --&gt;
```

`#fix3` runs after `#fix2`, before the 17:00 pause. Items after the time fence are unaffected. This follows naturally from sequential consumption — no special ordering logic needed.

Want me to update the plan with per-item time gates?
❯ yes
### Re: plan updated with per-item time gates — opus-4-6

Updated `agent-queue-plan.md` with per-item time gates:

- **Design decision added:** `--- start <time>` fences placed between items act as time gates. Items before run immediately; queue pauses until the time arrives. Multiple gates create a scheduled pipeline.
- **New syntax section** with examples showing multi-gate pipelines and insertion before a time gate.
- **Phase 1 updated:** unit tests now cover `--- start <time>`, `--- stop`, and multiple time gates.
- **Phase 3 updated:** consumption loop now checks for control fences at the head before consuming. Time gates emit `queue_deferred` when the time hasn't arrived (queue stays active but paused). Stop fences and item modifications halt the queue.
- **Parsing rules updated:** `--- start` accepts both `--- start <time>` and `--- start at <time>`, and can appear at head (initial activation) or mid-queue (time gate).

❯ Can we add a noclear attribute to prevent `/clear` from being called for an item in the queue?
### Re: noclear attribute for queue items — opus-4-6

Yes — `noclear` fits naturally as both an item-level and queue-level annotation.

**Default behavior:** the queue runner calls `/clear` between items so each runs with a clean context window. `noclear` opts out of that reset.

**Item-level:**

```markdown
<!-- agent:queue auto -->
- do #fix1
- [noclear] do #fix2
- do #fix3
<!-- /agent:queue -->
```

`#fix1` runs → `/clear` → `#fix2` runs with `#fix1`'s context still present → `/clear` → `#fix3` runs clean.

Semantics: `noclear` on an item means "don't clear context before processing this item." The prior item's full conversation — tool results, code reads, diffs — stays available. Useful when items are coupled: one produces analysis the next consumes.

**Queue-level:**

```markdown
<!-- agent:queue auto noclear -->
- do #fix1
- do #fix2
- do #fix3
<!-- /agent:queue -->
```

Disables clearing for the entire queue — one continuous session across all items. Good for tightly coupled batches where every item builds on prior context.

**Interaction with `--- start <time>` fences:** a time gate inherently introduces a long pause. Context from before the gate may be stale or compacted by the time the gate opens. I'd recommend that time gates implicitly clear context (even under queue-level `noclear`), unless the item after the gate also has `[noclear]`. This keeps the default safe — stale context across a 3-hour pause is more likely to confuse than help.

**Plan update:** added `noclear` to the queue syntax section, Phase 1 test coverage, and Phase 3 consumption loop logic.

❯ Can we transfer items onto a queue from another document?
❯ Can we have a prompt in the exchange or in the queue to write a prompt into the exchange or queue?
### Re: cross-document queue transfer and self-writing prompts — opus-4-6

#### Cross-document transfer

Yes. `agent-doc transfer` already moves content between documents — extending it to target a queue is natural:

```bash
agent-doc transfer source.md --to target.md --into queue
```

This would extract items from `source.md` (exchange prompts, pending items, or explicit selections) and append them as queue entries in `target.md`'s `<!-- agent:queue -->` block. The reverse — pulling from a queue into another document's exchange or pending — works the same way with `--from queue`.

Use cases:
- **Triage document → work queue:** a brainstorm session produces a list of tasks; transfer them into a queue for sequential execution.
- **Multi-document pipeline:** document A's queue includes `transfer results.md --into queue` as an item, chaining documents together.
- **Splitting a large queue:** move items from one queue to another to parallelize across sessions.

The transfer would respect queue ordering — items are appended at the tail by default, or inserted at a specific position with `--before <item>` / `--after <item>`.

#### Self-writing prompts (exchange/queue → exchange/queue)

Yes — this is meta-prompting. A prompt in the exchange or queue that writes content back into the same or another component:

```markdown
<!-- agent:queue auto -->
- Add "run regression suite" to pending
- Write a prompt into exchange: "Summarize the changes from today's session"
<!-- /agent:queue -->
```

The queue item `Add "run regression suite" to pending` is an imperative that the agent executes via `--pending-add`. The second item instructs the agent to write a new prompt into the exchange — effectively scheduling a future conversation turn.

Implementation: the agent recognizes "write a prompt into exchange/queue" as a directive and uses `agent-doc write` with appropriate flags to inject the content. For queue self-insertion, items appended during a running queue would be picked up on subsequent cycles (the queue re-reads on each preflight).

**Safety consideration:** self-writing prompts create feedback loops. A queue item that adds more queue items could run indefinitely. Safeguards:
1. **Depth limit** — track how many times a queue item was "generated" vs "user-authored." Auto-generated items can't spawn more auto-generated items (depth = 1).
2. **Explicit syntax** — require a specific annotation like `[emit]` so accidental phrasing doesn't trigger meta-prompting: `- [emit:queue] run regression suite after deploy`
3. **Audit trail** — generated prompts carry a `<!-- generated-by: #item-id -->` annotation so the user can trace provenance.

The `[emit]` prefix keeps it opt-in and visible. Without it, the agent treats queue items as normal work directives — no self-modification.

Ther term `[emit]` seem ambiguous. Is there a more precise term to add an active prompt into the document? Something like `[prompt:queue]`, `[reprompt:queue]`, `[schedule:queue]`, `[write:queue]`, `[emit:queue]`, or other good alternatives? 
### Re: naming the self-writing prompt annotation — opus-4-6

Good instinct — `[emit]` reads like a programming term (event emitters) and doesn't communicate the document-level intent. Here's my assessment of the options:

| Annotation | Pros | Cons |
|---|---|---|
| `[enqueue]` | Most precise for queue target; verb matches data structure exactly | Awkward for exchange target (`[enqueue:exchange]`?); CS jargon |
| `[post:queue]` | Conversational metaphor ("post to queue"); works for both targets (`[post:exchange]`) | Slightly ambiguous — "post" could mean publish |
| `[prompt:queue]` | Says exactly what it produces — a prompt | "prompt" is heavily overloaded in this context |
| `[defer:queue]` | Communicates intent — "do this later" | Implies postponement, not just scheduling; doesn't convey that a new prompt is being created |
| `[schedule:queue]` | Clear intent | Collides with `--- start <time>` scheduling; different mechanism |
| `[write:queue]` | Describes the mechanism | Too generic — every queue operation "writes" |
| `[reprompt:queue]` | Clear that a new prompt cycle is created | Implies re-doing something, not creating new work |

**My recommendation: `[enqueue]`** for queue targets and **`[post:exchange]`** for exchange targets, with `[post:queue]` accepted as an alias for `[enqueue]`.

Reasoning: `[enqueue]` is unambiguous — it means exactly one thing in any programming or workflow context. For the exchange target, `[post]` reads naturally: "post this prompt to the exchange." Having `[post]` as the unified verb with target-specific shorthand (`[enqueue]` = `[post:queue]`) gives you both precision and consistency.

Examples:
```markdown
<!-- agent:queue auto -->
- do #fix1
- [enqueue] run regression suite after all fixes land
- [post:exchange] Summarize today's queue results
<!-- /agent:queue -->
```

`#fix1` executes → "run regression suite..." is added to the tail of the queue → when the queue drains, "Summarize today's queue results" appears as a new prompt in the exchange for the next conversation cycle.

I like `[defer:queue]`/`[defer:exchange]` as it mirrors Zig & Go `defer` command. Would `[defer:*]` immediately execute the prompt once the turn is done in the exchange or the item is reached in the queue?  
### Re: defer semantics — opus-4-6

`defer` is the right name. The Zig/Go analogy maps cleanly: "execute this when the current scope exits."

**Execution semantics by context:**

| Where `[defer:*]` appears | What happens |
|---|---|
| `[defer:queue]` in a queue item | Appended to queue tail. Runs when the queue reaches it — immediately after current item if nothing else is queued after it. |
| `[defer:exchange]` in a queue item | Prompt injected into exchange. Runs on the next cycle after the queue drains or halts. |
| `[defer:queue]` in an exchange prompt | Item added to queue. Runs when the queue is next activated (`auto` or `do queue`). |
| `[defer:exchange]` in an exchange prompt | Prompt written to exchange. Runs on the very next cycle — effectively "continue this conversation with this follow-up." |

So yes — `[defer:*]` executes the prompt as soon as its scope completes. The "scope" is the containing unit: for a queue item, scope exit = item completion; for an exchange prompt, scope exit = turn commit.

**Stacking:** multiple `[defer]` annotations in the same item execute in LIFO order (matching Zig/Go semantics), or FIFO if we prefer document-order predictability. I'd lean FIFO here — document order is more intuitive for a markdown workflow than stack unwinding. But worth flagging as a design choice.

**Updated naming:** `[defer:queue]` / `[defer:exchange]` replaces `[emit]`, `[enqueue]`, and `[post]` from the previous discussion. One keyword, target-qualified, with well-understood scope semantics.

❯ If we are using FIFO, perhaps defer is an incorrect term. `[enqueue:queue]`, `[enqueue:exchange]`, `[post:queue]`, `[post:exchange]` would be a better set of instructions. If we type in `enqueue:queue`, `enqueue:exchange`, `post:queue`, `post:exchange` without the square brackets `[]`, would that also run the operation?
### Re: FIFO naming and bare syntax — opus-4-6

You're right — `defer` implies LIFO by convention. If the ordering is FIFO (document order), the verb should reflect that. Between the two:

- **`enqueue`** — precise, data-structure-correct for FIFO. Slightly technical but unambiguous.
- **`post`** — conversational, works across contexts ("post to queue", "post to exchange"). Less precise but more readable.

I'd go with **`post`** as the primary verb. It's shorter, reads naturally in markdown (`[post:queue] run tests`), and doesn't carry the CS-jargon weight of `enqueue`. Accept `enqueue` as an alias for users who prefer it.

#### Bare syntax (without brackets)

Yes — bare `post:queue` and `post:exchange` could work as recognized prefix commands. The parser would match the pattern `^post:(queue|exchange)\s` at the start of a queue item or exchange line.

```markdown
<!-- agent:queue auto -->
- [post:queue] run regression suite
- post:queue run regression suite
<!-- /agent:queue -->
```

Both forms would be equivalent. The brackets version is canonical (explicit, no ambiguity), but bare prefix is accepted as shorthand. Same for exchange context:

```
post:exchange Summarize today's session
```

**Disambiguation rule:** bare `post:*` is only recognized as a command when it appears as the first token of a queue item (`- post:queue ...`) or as a standalone line in the exchange. Mid-sentence occurrences like "we should post:queue items later" are treated as regular text. This mirrors how `do #id` works — position-dependent parsing.

**Full syntax table:**

| Form | Context | Recognized? |
|---|---|---|
| `[post:queue] text` | queue item or exchange | Yes (canonical) |
| `[post:exchange] text` | queue item or exchange | Yes (canonical) |
| `post:queue text` | first token of queue item | Yes (bare shorthand) |
| `post:exchange text` | standalone exchange line | Yes (bare shorthand) |
| `...post:queue...` | mid-sentence | No (plain text) |
| `[enqueue:queue] text` | anywhere | Yes (alias) |
| `enqueue:queue text` | first token | Yes (alias, bare) |

❯ With `queue`, `pending` is ambiguous. Rename `pending` to `backlog` with `pending` being an alias to `backlog`.
### Re: rename pending to backlog — opus-4-6

Good call — with `queue` in the picture, `pending` is ambiguous. "Pending in queue" vs "pending in backlog" are different states, and the current naming conflates them.

**`backlog`** is the right name. The distinction becomes:

| Component | Purpose | Processing |
|---|---|---|
| `<!-- agent:backlog -->` | Items planned but not yet started; tracked work | Manual — user selects via `do #id` |
| `<!-- agent:queue -->` | Items actively staged for sequential execution | Automatic — consumed in order by the queue runner |

`backlog` is universally understood (agile, project management, gamedev) and communicates "we intend to do these, but not right now in this order." `queue` communicates "these run next, in this order, automatically."

**Migration:**

- `<!-- agent:pending -->` → `<!-- agent:backlog -->` (canonical)
- `<!-- agent:pending -->` remains accepted as an alias — existing documents continue to work
- CLI flags: `--backlog-add`, `--backlog-done`, `--backlog-edit`, `--backlog-gate`, `--backlog-ungate`, `--backlog-clear`, `--backlog-reorder`
- Old flags (`--pending-add`, etc.) remain as aliases
- `post:backlog` / `[post:backlog]` works alongside `post:queue` and `post:exchange`

**Workflow with both:**

```markdown
<!-- agent:backlog -->
- [ ] [#fix1] Fix auth timeout
- [ ] [#fix2] Update error messages  
- [ ] [#fix3] Refactor token validation
<!-- /agent:backlog -->

<!-- agent:queue auto -->
- do #fix1
- do #fix2
--- start 17:00 ET
- do #fix3
<!-- /agent:queue -->
```

Backlog tracks what exists. Queue determines execution order and timing. Completing `do #fix1` in the queue also marks `#fix1` done in the backlog — the `do #id` closeout rule applies across both components.
<!-- agent:boundary:f1fd9c1e -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#queue] Implement agent:queue component with hybrid syntax (- list + ~~~prompt/--- fences), sequential dispatch, and CLI scaffolding — tasks/agent-doc/agent-queue-plan.md
- [/] [#hfix8] Bump JB plugin version, build + install, manual verify (HEAD) appears after patchback with IPC listener active
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
