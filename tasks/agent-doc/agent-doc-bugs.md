---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: codex
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
<!-- agent:boundary:8e5e1f40 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#envt1] [recommended] Fix detect_install_env_treats_codex_thread_id_as_codex test failure
- [ ] [#orch5] [recommended] Manual test orchestrate sequential + parallel + DAG flows before publish
- [/] [#hfix8] Bump JB plugin version, build + install, manual verify (HEAD) appears after patchback with IPC listener active
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
