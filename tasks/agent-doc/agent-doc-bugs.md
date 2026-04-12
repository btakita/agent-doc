---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.32.0+fix — Phase 1+2 guard gates deployed, binary fresh, ready for manual testing
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Summary

Diagnosed and fixed agent-doc submodule patch routing regression:
- **Root cause:** patches written to submodule's .agent-doc/patches/ (watched only by plugin in parent)
- **Fix:** 5 bug fixes implemented (git.rs, write.rs, tests)
  1. resolve_to_git_root() absolute-path branch now resolves to superproject
  2. resolve_ipc_project_root() helper ensures all IPC sites target parent patches dir
  3. Timeout fallback ipc_payload now includes normalize_prefix_lines, reposition_boundary, patch_id
  4. Timeout fallback writes content_ours to disk before snapshot/exit
  5. Regression test added for submodule patch routing
- **Stale binary issue:** fixes in source but running binary was 2h old (workflow change: new manual testing gate window with no auto-rebuild trigger)
- **Stale binary fix plan (Phase 1+2):** 
  - Phase 1: Added pre-preflight mtime guard in SKILL.md that auto-rebuilds if source newer
  - Phase 2: Hardened prerelease.md step 8 to require both make build-release AND cargo install before testing gate
- **Status:** All 5 fixes live in rebuilt binary, Phase 1+2 guards active
- **Next:** Manual testing gate on src/session-share/tasks/claudescore.md to verify ❯ preservation and working-tree consistency
❯ When I ran `compact exchange` in src/session-share/tasks/claudescore.md, items in agent:pending were removed. This should never happen. Add to SKILL.md->runbook. This is very important.
### Re: compact exchange must never touch pending

Agreed — this is a critical invariant. Runbook + memory updated.

**Runbook changes** (`.claude/skills/agent-doc/runbooks/compact-exchange.md`):
- Added **⚠️ Safety Invariants** section at top: compact affects ONLY `agent:exchange`; pending/status/others must be preserved byte-for-byte. Any mutation of non-exchange components is a binary bug — stop, report, do not re-run.
- Added **step 3** (baseline snapshot): before running compact, capture current `agent:pending` contents.
- Added **step 5** (verify): after compact, diff against baseline. If pending changed, do NOT commit — restore via `git checkout --` or `pre-compact-N` tag.

**Memory:** saved `feedback_compact_exchange_pending_invariant.md` + indexed in `MEMORY.md`.

**Binary-level concern:** this points at a likely `agent-doc compact` bug — `replace_content()` should be scoped to a single named component, so pending should be untouchable. Worth investigating `src/agent-doc/src/compact.rs` to confirm the component scope is correct, and adding a test that asserts non-target components are byte-identical before/after compact. Adding to pending.

❯ do
❯ - 🔴 **Investigate compact.rs scope bug**: compact exchange removed items from `agent:pending` in claudescore.md. Binary should scope `replace_content()` to the target component only. Add regression test asserting non-target components are byte-identical before/after.
### Re: compact.rs scope bug — root cause + fix (HEAD)

**Root cause:** CRDT state not refreshed after compact. `replace_content()` itself is correctly scoped (only replaces bytes between `open_end` and `close_start` of the target component). Non-target components are byte-identical in the compacted output.

The real bug: in CRDT-mode documents, `compact.rs` was:
1. GC'ing the CRDT tombstones (line 103-113) — but keeping pre-compact document content
2. Running `replace_content()` + `atomic_write_pub()` + `snapshot::save()` — all correct on disk
3. **Never refreshing the CRDT state** with the post-compact content

So after compact: disk and snapshot have `exchange=compacted, pending=intact`. But the CRDT state still contains the pre-compact exchange AND the pre-compact pending. On the next `agent-doc write` or `stream`, `merge.rs` loads the stale CRDT, performs a 3-way merge, and the stale CRDT's view of pending can override user edits made after compact.

**Fix (compact.rs):**

Both `run_component_compact` and `run_component_compact_partial` now call:
```rust
if is_crdt {
    let new_crdt = CrdtDoc::from_text(&compacted).encode_state();
    snapshot::save_crdt(file, &new_crdt)?;
}
```

This creates a fresh CRDT document from the post-compact text and saves it. The old CRDT history (tombstones, edit operations) is discarded — appropriate since compact is a "new epoch" operation. Console output: `[compact] CRDT state refreshed from post-compact content`.

**Regression test:** `component_compact_preserves_non_target_components`
- Creates a template doc with `status`, `exchange`, and `pending` components
- Runs compact on exchange only
- Asserts `pending` and `status` are byte-identical before/after
- Asserts exchange was actually compacted (contains summary, no old `### Re:` topics)
- Passes (513 tests pass, 3 pre-existing tmux flakes, 7 ignored)

**Binary rebuilt:** `.bin/agent-doc` is now fresh with the fix.

**Ready for manual testing gate:** Run `agent-doc compact` on a CRDT doc with pending items and verify pending survives.
<!-- agent:boundary:e51c1d29 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- ✅ compact.rs CRDT state refresh — bug fixed, regression test added, binary rebuilt
- ✅ Runbook hardened with Safety Invariants + pre/post verification steps
- ✅ Memory note saved for compact-must-not-touch-pending invariant
- 🆕 Manual testing gate: test compact on CRDT doc with pending items
- 🆕 Manual testing gate: verify ❯ preservation + working-tree consistency on live src/session-share/tasks/claudescore.md
- → Commit source edits once live-tested
- → Bump version, update VERSIONS.md, publish (crates/PyPI/GitHub Release)
<!-- /agent:pending -->
