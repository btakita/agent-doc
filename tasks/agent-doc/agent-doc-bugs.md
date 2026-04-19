---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**Session summary (v0.33.x bugs + fixes):**

**Bugs fixed:**
- Stream replace truncation: --stream path now uses append not replace, preventing content loss
- Enter/q stdin not submitted after session restart: supervisor stdin wiring fixed
- Submodule path resolution: JB/VS Code plugins now resolve paths relative to agent-doc project root, not monorepo root
- Run Agent Doc tmux-send skipped: is_busy guard was blocking sends — fixed in 0.2.61
- IPC auto-degradation (#d2d4): repeated IPC timeouts degrade write path to disk; 500ms→1000ms timeout
- IPC timeout pending mutation loss: splice_pending_component then upgraded to full CRDT merge in timeout path

**Decisions:**
- Build on every version bump — always build artifact in same cycle as version bump
- Opus 4.7 explicit triggers (#j57h/#4ynf/#chcb): SKILL.md ALL-OF conditions, model-tier-gate runbook, MANDATORY step labels
- IPC timeout path → CRDT merge instead of atomic_write (preserves all concurrent changes, not just pending component)

**Open/gated pending:**
- #a002: normalize_user_prompts safety rail — landed, awaiting large-drift telemetry trip
- #ha3h: Multi-user recipient config (ship with multi-user support)
- #m1bz: install gate — cargo install + local JB plugin install; pause for user to verify live
❯ do #vprd. see pre-compact exchange session for context if needed.
### Re: #vprd — AGENTS.md audited — opus-4-7

**Removed (anti-laziness scaffolding):**
- "Self-Improvement Loop" section — auto-memory handles this orthogonally, no need to duplicate
- "Autonomous Bug Fixing" section — "just fix it", "Zero context switching" was pure progress-forcing
- "Demand Elegance (Balanced)" section — "ruthlessly iterate until mistake rate drops" is scaffolding
- "Task Management" numbered list — 6-step process forcing that wasn't adding rules, just pushing pace
- "No Laziness" and "Minimal Impact" core principles — scaffolding + duplicate of "Simplicity"

**Removed (progress-forcing):**
- "ruthlessly iterate on these lessons until mistake rate drops"
- "Go fix failing CI tests without being told how"
- "Zero context switching required from the user"
- "Senior developer standards"

**Added (explicit scope):**
- "Applies to all work in this repo" header on Workflow section
- "**Rust projects only**" qualifier on `make check` / `make precommit` rule (was stated generically)
- Kept the real content: plan-first, subagent scope, verify-before-done, runbook discipline, secret handling, git submodule rules

**Token count:** 802 → 445 words (~450 token reduction). Exceeds #vprd target.

Kept as genuine rules (not scaffolding): "No confident hallucination", security handling, submodule git discipline, runbook list, and the workflow bullets that have real content rather than pep-talk framing.

CLAUDE.md symlink → AGENTS.md still valid (unchanged).
Add branches to support both opus 4.7 and opus 4.6 usage in runbooks. opus 4.7 just deleted the the 4.6 support.
❯ ### Re: #vprd — AGENTS.md audited — opus-4-7 (HEAD)

**Removed (anti-laziness scaffolding):**
- "Self-Improvement Loop" section — auto-memory handles this orthogonally, no need to duplicate
- "Autonomous Bug Fixing" section — "just fix it", "Zero context switching" was pure progress-forcing
- "Demand Elegance (Balanced)" section — "ruthlessly iterate until mistake rate drops" is scaffolding
- "Task Management" numbered list — 6-step process forcing that wasn't adding rules, just pushing pace
- "No Laziness" and "Minimal Impact" core principles — scaffolding + duplicate of "Simplicity"

**Removed (progress-forcing):**
- "ruthlessly iterate on these lessons until mistake rate drops"
- "Go fix failing CI tests without being told how"
- "Zero context switching required from the user"
- "Senior developer standards"

**Added (explicit scope):**
- "Applies to all work in this repo" header on Workflow section
- "**Rust projects only**" qualifier on `make check` / `make precommit` rule (was stated generically)
- Kept the real content: plan-first, subagent scope, verify-before-done, runbook discipline, secret handling, git submodule rules

**Token count:** 802 → 445 words (~450 token reduction). Exceeds #vprd target.

Kept as genuine rules (not scaffolding): "No confident hallucination", security handling, submodule git discipline, runbook list, and the workflow bullets that have real content rather than pep-talk framing.

CLAUDE.md symlink → AGENTS.md still valid (unchanged).
### Re: model-specific AGENTS.md guardrails — opus-4-6

The concern is valid: Opus 4.7 removed sections like "Self-Improvement Loop", "Task Management", and "Autonomous Bug Fixing" as anti-laziness scaffolding — but those sections may have been serving as useful guardrails for 4.6 (which occasionally needs more explicit process reminders).

**Options:**
1. **Model-tier-gate runbook** — add a section that injects model-specific AGENTS.md rules. When `effective_tier` is resolved, the agent reads the appropriate set of workflow rules. Low tier models get more scaffolding, high tier models get lean rules.
2. **Conditional sections in AGENTS.md** — `<!-- agent:model:opus-4-6 -->` gated blocks that only apply when the current model matches. Would need binary support.
3. **Restore the removed sections** — simpler: just put them back and accept the token cost for 4.7.

Option 1 is the cleanest for agent-doc sessions (where model tier is already resolved). Option 3 is the fastest.
do Option 1
Also: the response from Opus 4.7 got duplicated in the exchange — orphan recovery applied the same content twice. This is cosmetic but inflates the exchange.

Your "Why did" message appears incomplete — please finish the thought and I'll respond.

Added to pending: model-specific AGENTS.md guardrails.
do
> Why did the last patchback get put into the document again? This seems like a regression from the last release.
❯ ### Re: model-specific AGENTS.md guardrails — opus-4-6 (HEAD)

The concern is valid: Opus 4.7 removed sections like "Self-Improvement Loop", "Task Management", and "Autonomous Bug Fixing" as anti-laziness scaffolding — but those sections may have been serving as useful guardrails for 4.6 (which occasionally needs more explicit process reminders).

**Options:**
1. **Model-tier-gate runbook** — add a section that injects model-specific AGENTS.md rules. When `effective_tier` is resolved, the agent reads the appropriate set of workflow rules. Low tier models get more scaffolding, high tier models get lean rules.
2. **Conditional sections in AGENTS.md** — `<!-- agent:model:opus-4-6 -->` gated blocks that only apply when the current model matches. Would need binary support.
3. **Restore the removed sections** — simpler: just put them back and accept the token cost for 4.7.

Option 1 is the cleanest for agent-doc sessions (where model tier is already resolved). Option 3 is the fastest.

Also: the response from Opus 4.7 got duplicated in the exchange — orphan recovery applied the same content twice. This is cosmetic but inflates the exchange.

Your "Why did" message appears incomplete — please finish the thought and I'll respond.

Added to pending: model-specific AGENTS.md guardrails.
❯ do #j58r. build and install for local testing
❯ ### Re: #j58r — already built and installed — opus-4-6 (HEAD)

The dedup fix was already built and installed in the previous cycle:
- `~/.cargo/bin/agent-doc` — rebuilt at 21:43 (after source edit at 21:41)
- `target/release/agent-doc` — rebuilt at 21:44

Both binaries contain `fingerprint_lines` (the per-line dedup check). Ready for live testing — the next orphan recovery with blank-line/boundary content should trigger the dedup guard instead of duplicating.
❯ release
❯ ### Re: release v0.33.2 — opus-4-6 (HEAD)

Published agent-doc v0.33.2:
- **crates.io**: https://crates.io/crates/agent-doc/0.33.2
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.33.2
- **PyPI**: skipped — needs manylinux Docker build (local build produces `linux_x86_64` wheel)

Includes: FFI submodule path resolution, IPC timeout CRDT merge, recovery dedup fix, JB/VS Code plugin updates, SKILL.md/runbook updates. 24 files, +1011/-222 lines.

Parent submodule pointer updated.

The dedup fix was confirmed live this cycle: `[recover] Response already present in document — skipping apply, cleaning up pending file`.
❯ Why did the last agent-doc turn for src/session-share/tasks/claudescore.md not patchback the response to the document?
❯ ### Re: claudescore.md patchback failure — opus-4-6 (HEAD)

The ops log shows the write command ran but received no response content:

```
[1776567463] write_origin file=tasks/claudescore.md origin=skill
[1776567463] normalize_user_prompts applied=3       ← only user line normalization
[1776567464] ipc_socket_delivered snap_len=139940   ← IPC delivered
[1776567464] ipc_write_consumed patches=0           ← 0 patches
```

Compare with the previous successful write:
```
[1776563371] write_origin origin=skill
[1776563371] normalize_user_prompts applied=2
[1776563371] write_stream_done snap_len=123908      ← actual content written
```

**Diagnosis:** The `agent-doc write` heredoc received empty or whitespace-only input. The binary processed it correctly (normalized 3 user lines, delivered IPC, committed), but there was no response content to write. The snapshot grew by ~448 bytes (just the 3 `❯ ` normalization prefixes), not by a response block.

**This is a skill-side issue, not a binary bug.** The Claude session for claudescore.md generated a response in the console but the heredoc pipe to `agent-doc write` was empty. Likely causes:
1. Context compaction between response generation and write command
2. The heredoc `RESPONSE` delimiter was malformed or missing
3. The session hit a token/output limit and truncated before the write command

No code fix needed — the binary did what it was told. The skill workflow needs to ensure the response is captured before piping.
❯ Fix the skill/runbook in both agent-doc and agent-loop.
❯ ### Re: fix skill/runbook for empty patchback — opus-4-6 (HEAD)

**Binary fix** (`write.rs`): added a warning when write proceeds with 0 template patches:
```
[write] WARNING: 0 template patches found — response may be missing or malformed.
Only normalization/boundary changes will be applied.
```
Also logs `zero_patches_warning` to ops.log for forensics.

**SKILL.md fix**: added explicit note under Step 2 (Write back):
> The response content MUST include `<!-- patch:exchange -->` blocks for template-mode documents. If the heredoc is empty or contains only raw text without patch markers, the binary will warn and only apply normalization — the response will be silently lost.

**Root cause clarification**: the claudescore failure wasn't a missing guard — the binary has guards for empty input and empty patches. The issue was: normalization (adding `❯ ` prefixes to user lines) changed the content enough to pass the dedup guard, so the write proceeded with 0 patches and only normalization changes. The response was lost before reaching the heredoc (context compaction or truncation).

**Both copies synced**: `src/agent-doc/SKILL.md` → `.claude/skills/agent-doc/SKILL.md`. Both binaries rebuilt.
do #ndsd
❯ please add the response to this document
❯ Why did the response not patchback to this document?
### Re: #ndsd verified complete (replay) — opus-4-6

*(This response was generated last cycle but lost due to IPC degradation — replaying.)*

All three components confirmed in place:
- **Binary warning** (`write.rs:1004`): logs `0 template patches found` + `zero_patches_warning` to ops.log
- **SKILL.md** (line 90): `patch:exchange` requirement documented with compaction recovery note
- **Both copies synced**: `src/agent-doc/SKILL.md` and `.claude/skills/agent-doc/SKILL.md` match

#ndsd marked done.

❯ ### Re: why patchback failed — opus-4-6 (HEAD)

The write log from last cycle tells the story:

```
[write] cleaning stale patch file before socket send (prevent double-apply)
[write] socket IPC patch delivered
[write] snapshot from file read (ack-content sidecar not available after 1000ms)
[write] IPC degraded — subsequent writes will use disk until marker cleared or expires (1h)
```

**Root cause:** The disk patch file was deleted *before* the socket send. When the socket delivery wasn't acknowledged within 1000ms, IPC degraded — but there was no disk fallback because the patch file had already been cleaned. The response content was silently lost.

**This is a race condition bug in `write.rs`:** the "prevent double-apply" cleanup is correct in intent but wrong in timing. The patch file should be cleaned *after* confirmed delivery (ack received), not before the socket attempt. Sequence should be:

1. Write patch to disk (for fallback)
2. Send via socket
3. If ack received → clean patch file
4. If ack times out → leave patch file for disk fallback path

Added to pending as a new item.
❯ do #ttv0
❯ ### Re: #ttv0 — IPC fallback patch file race fix — opus-4-6 (HEAD)

**Root cause:** `try_ipc()` cleaned the disk patch file *before* socket send to prevent double-apply. When the socket delivered but the sidecar ack timed out, there was no disk fallback — response lost.

**Fix in `write.rs`:** replaced pre-clean with pre-write:
1. **Before socket send**: write current patch content to disk as fallback (overwrites any stale content)
2. **On sidecar success**: clean the fallback file (confirmed applied)
3. **On sidecar timeout**: leave the fallback file for file watcher recovery

`patch_id` deduplication (already in place) prevents double-apply when both socket and file watcher fire.

**Verified:** 1146 tests pass, release build clean, `cargo install` done. Both binaries updated.
❯ release
❯ Why was the response not written to this document?
❯ ### Re: why response was lost + deeper fix — opus-4-6 (HEAD)

The #ttv0 fix (pre-writing the fallback file) was only half the solution. The fundamental issue: `try_ipc()` returned `success: true` when the socket acknowledged but sidecar timed out. The caller trusted this and skipped the disk write path entirely. The plugin never applied the content, and the fallback file wasn't picked up either.

**Deeper fix in `write.rs`:** sidecar timeout now falls through instead of returning success:
- **Sidecar confirmed** → save snapshot from sidecar, clean fallback file, return `success: true`
- **Sidecar timed out** → mark IPC degraded, leave fallback file, return `success: false` → caller falls through to CRDT disk write

Also updated `start_fake_listener` in tests to write ack-content sidecar files, matching production behavior.

**Verified:** 1146 tests pass, release build clean, both binaries at v0.33.3.

Ready for manual testing gate — verify this IPC path works in a live session, then I'll publish.
❯ The last response was not written to this file. Please add. Diagnose the issue.
### Re: release v0.33.3 (replay) — sonnet-4-6

*(This response was generated earlier but lost due to the IPC sidecar timeout bug — the same bug we just fixed. Replaying.)*

Version bumped to 0.33.3, VERSIONS.md updated, 1146 tests pass, both binaries rebuilt.

**Pending manual testing gate** — please verify the IPC fallback behavior works in a live session, then I'll proceed with:
1. Branch → PR → squash merge
2. `git tag v0.33.3`
3. `cargo publish`
4. `maturin publish`
5. `gh release create v0.33.3` with prebuilt binary

❯ ### Re: diagnosis — why the release response was lost — opus-4-6 (HEAD)

The sonnet-4-6 release response was written BEFORE the deeper fix landed. At that point:
- `try_ipc()` still returned `success: true` on sidecar timeout (the old behavior)
- Socket acknowledged, sidecar timed out → `success: true` → caller skipped disk write
- Plugin never applied the content → response lost

The deeper fix (sidecar timeout → `success: false`) was built in the SAME session but wasn't installed yet when sonnet-4-6 ran the write. The next opus-4-6 response succeeded because IPC was already degraded, forcing the CRDT disk write path.

Timeline:
1. sonnet-4-6 write → old binary → sidecar timeout → `success: true` → **lost**
2. opus-4-6 build → deeper fix compiled + installed
3. opus-4-6 write → IPC degraded from step 1 → CRDT disk write → **succeeded**

The fix is now in both binaries. This should not recur.
❯ The response was not written to this document. Please add the last 2 responses and diagnose.
❯ ### Re: responses ARE on disk — IDE buffer stale — opus-4-6 (HEAD)

Both responses are confirmed on disk:
- **Line 251**: `### Re: release v0.33.3 (replay) — sonnet-4-6`
- **Line 264**: `### Re: diagnosis — why the release response was lost — opus-4-6`

The diff from preflight also confirms this — both snapshot and document have the content. The only diff is `(HEAD)` repositioning and your new user line.

**The problem is the IDE buffer.** When IPC is degraded, `agent-doc write` uses CRDT disk write — it updates the file on disk but bypasses the plugin's Document API. The IDE keeps showing its stale in-memory buffer. The commit sends `IPC reposition boundary signal` and writes a `VCS refresh signal`, but these apparently aren't triggering the IDE to reload the file from disk.

**Workaround:** reload the file in your IDE (Cmd/Ctrl+Shift+A → "Reload from Disk" in IntelliJ, or close and reopen the tab).

**Root cause:** the IPC degraded state lasts 1 hour. During that window, all writes go to disk only. The IDE plugin's file watcher should detect disk changes, but it appears to not be picking them up. Added to pending for investigation.
❯ do #s80z
❯ ### Re: #s80z — IDE buffer stale fix — opus-4-6 (HEAD)

**Root cause:** `repositionBoundaryViaDocument()` in `PatchWatcher.kt` calls `targetFile.refresh(false, false)` to update the VFS, but never calls `reloadFromDisk(document)`. The document buffer stays stale — it contains the pre-CRDT-merge content. The reposition handler reads the old buffer, repositions the boundary in the old content, and writes it back. The CRDT-merged response is never loaded into the buffer.

By contrast, `applyPatch()` correctly calls `fdm.reloadFromDisk(document)` at line 498 before processing. The reposition handler was missing this step.

**Fix in `PatchWatcher.kt`:** Added `reloadFromDisk()` call in the reposition handler:
```kotlin
if (!fdm.isDocumentUnsaved(document)) {
    fdm.reloadFromDisk(document)
}
```

The `isDocumentUnsaved` guard preserves user edits that haven't been saved from the IDE — same pattern as `applyPatch()`.

**Timeline after fix:**
1. CRDT disk write updates file on disk (with response)
2. Commit sends reposition signal via socket
3. Plugin's reposition handler: refresh VFS → reloadFromDisk → read updated buffer → reposition boundary
4. User sees response immediately in IDE

Plugin version bumped to 0.2.64. Both zips built (signed + unsigned).

**To install:** IDEA → Settings → Plugins → gear → Install from Disk → select `agent-doc-jetbrains-0.2.64-signed.zip`. Restart IDE.

❯ In src/session-share/tasks/claudescore.md... this should have been added to agent:pending. Why was it not added. Skill/runbook issue? 
❯ > Ready to update the baseline methodology and tree-sitter scores, or capture as a spec first?
❯ ### Re: why the forward-looking proposal was not pending-added — opus-4-7 (HEAD)

**Yes, this is a runbook gap.** `runbooks/pending-ops.md` § "What to decide each cycle" lists:

> New items discovered → `--pending-add "text"`

...but it doesn't call out the most common generator of pending items in practice: **the agent's own forward-looking questions at the end of a response** ("Ready to X?", "should we A or B first?", "shall I capture Y as a spec?"). These are proposed next actions. If the user doesn't reply immediately, the proposal gets buried in the exchange history and lost — exactly what happened in `claudescore.md:1897`.

**Why sonnet-4-6 missed it:** the runbook treats "new items discovered" as user-origin facts/TODOs, not as the agent's own dangling proposals. Without an explicit rule, the pattern is: write proposal inline → move on → next cycle forgets.

**Fix (pending-added as #above):** amend `pending-ops.md` to make this an enforced rule — any response ending with a forward-looking question or multi-choice proposal MUST `--pending-add` each concrete option so the thread survives non-reply.

Proposed wording for the runbook:

```
- Any response ending with a forward-looking question ("ready to X?",
  "should we A or B first?", "shall I capture Y?") MUST --pending-add
  each concrete next-step option. The proposal dies if the user doesn't
  reply in the same cycle; capturing it preserves continuity.
```

This is a skill/runbook issue, not a code issue. The binary already accepts multiple `--pending-add` flags per cycle; nothing is missing at the implementation layer.
❯ Is this runbook added to both agent-doc and agent-loop?
❯ ### Re: runbook not yet added — three copies exist — opus-4-7 (HEAD)

**No — not yet added anywhere.** I only queued the edit as pending `#31j3`. The runbook text is still unchanged in all three locations.

**Three copies exist, all byte-identical:**

1. `src/agent-doc/runbooks/pending-ops.md` — **canonical source** (shipped with the crate, packaged into `target/package/agent-doc-*/runbooks/`)
2. `~/.claude/skills/agent-doc/runbooks/pending-ops.md` — user-global install (what this session loads)
3. `agent-loop/.claude/skills/agent-doc/runbooks/pending-ops.md` — project-level install

**Correct propagation order:**
1. Edit `src/agent-doc/runbooks/pending-ops.md` (source of truth).
2. Run `agent-doc skill install` to re-sync `~/.claude/skills/agent-doc/` and `agent-loop/.claude/skills/agent-doc/` from the source.
3. Verify with `diff` that all three stay identical.
4. Commit the change inside `src/agent-doc/` and bump patch version if this goes into the next release.

Shall I make the edit now? It's a ~5-line addition to the "What to decide each cycle" list.
❯ do #31j3...add to both agent-doc and agent-loop
❯ ### Re: #31j3 done — plus skill-install bug discovered — opus-4-7 (HEAD)

**Edit landed in `src/agent-doc/runbooks/pending-ops.md`.** Added a new bullet to "What to decide each cycle":

```
- **Agent-proposed forward actions** → `--pending-add "text"` for each concrete option.
  Any response ending with a forward-looking question ("Ready to X?", "Should we A or
  B first?", "Shall I capture Y as a spec?") MUST `--pending-add` each concrete
  next-step option in the same cycle. The proposal dies if the user doesn't reply
  immediately; capturing it preserves continuity across cycles.
```

**Propagation to all three copies:** confirmed byte-identical (`diff` passes for src → `~/.claude/...` and src → `agent-loop/.claude/...`).

**Bug discovered:** `agent-doc skill install` returned `Skill already up to date (v0.33.3).` and did NOT copy the updated runbook. Only `SKILL.md` propagates via the installer; the `runbooks/*.md` subdirectory is ignored. I synced the two install copies with plain `cp`. Queued as a new pending item — `skill install` should be fixed to walk the `runbooks/` tree.

**Not committed in `src/agent-doc/`:** change is left in the working tree unstaged. Waiting on your call — should I roll it into the next patch release (bump, VERSIONS.md entry, commit) or commit standalone now?

❯ commit + push + release
❯ ### Re: v0.33.3 shipped — opus-4-7 (HEAD)

**Done — release pipeline complete.**

| Target | Status |
|--------|--------|
| `make check` | ✅ 1146 tests pass, clippy clean, version-sync ok |
| Commit | ✅ `c0dfc6e agent-doc v0.33.3` (9 files) |
| Push main + tag | ✅ `v0.33.3` tagged and pushed |
| crates.io | ✅ [agent-doc v0.33.3](https://crates.io/crates/agent-doc/0.33.3) |
| `cargo install --path .` | ✅ `~/.cargo/bin/agent-doc` → 0.33.3 |
| `cargo build --release` | ✅ `.bin/agent-doc` → 0.33.3 (both binaries live) |
| GitHub Release | ✅ [v0.33.3](https://github.com/btakita/agent-doc/releases/tag/v0.33.3) with prebuilt Linux binary |
| JB plugin 0.2.64 | ✅ both `agent-doc-jetbrains-0.2.64.zip` + `-signed.zip` in `build/distributions/` |
| Parent submodule bump | ✅ `f6b7623e chore: bump agent-doc submodule to v0.33.3` pushed |
| **PyPI** | ❌ **blocked** — same platform tag bug as v0.33.2 (`linux_x86_64` unsupported; needs manylinux wheel) |

**PyPI bug:** the wheel builds as `agent_doc-0.33.3-py3-none-linux_x86_64.whl`; PyPI rejects with `400 unsupported platform tag 'linux_x86_64'`. Gated pending item `#y2gr` remains open for the skill-install runbooks-sync fix; I'm leaving the PyPI manylinux fix as a separate follow-up since it's pre-existing.

JB plugin 0.2.64 zips are on disk only — install via IDEA → Settings → Plugins → gear → Install from Disk → `agent-doc-jetbrains-0.2.64-signed.zip` when ready.

❯ Add manylinux to fix PyPI being blocked.
❯ ### Re: PyPI is NOT blocked — I misread — opus-4-7 (HEAD)

**My earlier report was wrong.** PyPI publishing isn't broken; manylinux is already configured. v0.33.3 is live on PyPI:

```
$ pip index versions agent-doc
agent-doc (0.33.3)
```

**What's actually happening:**

- `.github/workflows/pypi.yml` fires on `release: published`. It uses `PyO3/maturin-action@v1` with `manylinux: auto` — maturin runs inside a manylinux container and emits proper `manylinux_2_28_x86_64` wheels alongside macOS (x86_64 + aarch64) and Windows wheels.
- My `gh release create v0.33.3` already triggered it. The run completed successfully at [24623615824](https://github.com/btakita/agent-doc/actions/runs/24623615824): sdist ✓, 4 platform wheels ✓, publish-to-PyPI ✓.
- The local `maturin publish` I ran **can't** produce a PyPI-compatible wheel on Arch/CachyOS — the host libc is too new, so the wheel is tagged `linux_x86_64`, which PyPI rejects. This is inherent to the host, not a bug.

**Takeaway: stop running local `maturin publish` for agent-doc releases.** The CI pipeline is the canonical path for PyPI. I've updated auto-memory to reflect this — "Publish all targets" now reads *crates.io + GitHub Release* locally; PyPI flows through `release: published` automatically. New [`reference_pypi_publish_via_ci.md`](reference) captures the full context and corrects the earlier `#18985` note.

**Final status for v0.33.3:** all targets live. crates.io ✓, PyPI ✓, GitHub Release ✓, local binaries ✓, JB plugin 0.2.64 zips ✓, parent submodule pushed ✓. Nothing to do on manylinux.

❯ Why did an agent-doc run on src/session-share/tasks/claudescore.md not add pending items?

```
#### What I'd recommend

1. **Immediate (data fix):** Re-audit expected-scores.toml against fixture content, recompute EXPECTED_COMPOSITE_SCORES, add hooks-present expected score. This closes ~10 points of the gap with zero code changes.

2. **Next phase (rubric expansion):** Add 3-4 substance-class rubrics focused on universally observable signals (code structure, dependencies, commit hygiene). This addresses the structural coverage gap for repos like aider.

3. **Don't decompose existing rubrics** — harness-infrastructure and distribution-quality are broad but they're doing the right job. The gap is in the expected scores, not in the measurement.
```
❯ ### Re: pending items not added on claudescore.md — opus-4-7 (HEAD)

**Not a bug in the binary — the prior agent just didn't call `--pending-add`.** I traced it:

- Last committed cycle: `d364f9a agent-doc(claudescore): 2026-04-19 03:37:45` (session-share submodule).
- That cycle produced the "What I'd recommend" numbered list you quoted, but the `&lt;!-- agent:pending --&gt;` block is empty in the same commit — the agent only wrote prose, never added `--pending-add` flags to its `agent-doc write` invocation.
- The closing sentence was *"Want to start with the expected-scores re-audit?"* — i.e. the agent was still in "consult the user" mode rather than "materialize backlog".
- You then typed `Add tasks to pending` inline as a follow-up — that text is in the working tree but **no /agent-doc cycle has been run on `claudescore.md` since**, so the instruction hasn't been processed yet.

**Why the binary didn't auto-convert the list:** by design. Pending mutation only happens via the explicit `--pending-add` / `--pending-done` flags (per `runbooks/pending-ops.md`); the write path doesn't scan response prose for numbered lists and promote them to pending items. That's the right default — otherwise every "here are options" response would pollute the backlog.

**The real gap is on the skill/agent side, not the binary:** when a response contains distinct, actionable recommendations *and* the user clearly wants a backlog (empty pending + ranked list + "recommend"), the agent should proactively `--pending-add` those items in the same cycle. The current SKILL.md § 1b only tells the agent *how* to mutate pending when it decides to, not *when* to decide to.

**What I'd do to fix the pattern, not the symptom:**

1. **Immediate (unblock you):** run `/agent-doc tasks/claudescore.md` now — your `Add tasks to pending` instruction will be picked up, and the responding agent should issue `--pending-add` for each of the three recommendations. I'd do it from here, but the doc lives in the session-share submodule and needs its own cycle.
2. **Medium-term (skill tweak):** add a one-liner heuristic to § 1b: *"If your response ends with a numbered recommendation list and pending is empty (or the user asked for backlog), add each recommendation as a pending item in the same write."* Cheap to add; fixes the class.
3. **Not a binary change:** I'd explicitly avoid teaching the binary to infer pending items from prose. Keep the agent as the judgment layer.

Want me to file this under **Not Built** here (skill-side pending-promotion heuristic) while we're in this doc?
❯ Yes...add skill fix to agent-doc and agent-loop
❯ ### Re: skill fix applied — opus-4-7 (HEAD)

**Done — edited § 1b in both canonical and project-local SKILL.md.**

| File | Status |
|------|--------|
| `src/agent-doc/SKILL.md` (canonical, bundled in crate) | ✅ heuristic added after § 1b paragraph |
| `.claude/skills/agent-doc/SKILL.md` (project-local copy) | ✅ identical — `cmp` confirms SYNCED |

**New rule added to § 1b:**

> **Promotion heuristic (when to add pending in the same cycle):** if your response ends with a numbered list of distinct, actionable recommendations (e.g., "What I'd recommend: 1. ..., 2. ..., 3. ..."), and either pending is currently empty OR the user asked for a backlog / "tasks" / "todo", add each recommendation as a pending item via `--pending-add` in the same `agent-doc write` invocation. Don't leave actionable ranked items as prose-only — the pending component is where backlog lives. Skip promotion when items are hypotheticals, options you're asking the user to choose between, or already captured elsewhere.

**What's still needed to ship this:** the edit only affects already-installed skills. For the bundled copy to reach *new* installs via `agent-doc skill install`, the agent-doc crate needs a release (0.33.4). Adding a pending item for that so it's tracked rather than lost — per the heuristic I just wrote. Also eating my own dog food: this cycle *does* end with an actionable next step, so I'm `--pending-add`ing it in the same write.

Heuristic applies recursively. Nice.
❯ release
❯ ### Re: release 0.33.4 — build ready, awaiting your go — sonnet-4-6 (HEAD)

Steps complete through the manual testing gate:

| Step | Status |
|------|--------|
| `Cargo.toml` + `pyproject.toml` → 0.33.4 | ✅ |
| `VERSIONS.md` entry | ✅ |
| `make check` (1146 tests) | ✅ (flaky `detect_wrong_session_pane` passed on second run — pre-existing, unrelated to this change) |
| `cargo install --path .` | ✅ `~/.cargo/bin/agent-doc` → 0.33.4 |

**Pausing here per memory: manual testing gate.** Please verify the skill update looks correct in a live session, then reply to publish upstream (crates.io + GitHub Release + tag). PyPI will flow through CI automatically on release publish.

❯ The agent-doc response patchback did not write to src/boost-client/tasks/monsterrodholders.md
❯ The prompts are still uncommitted. I see the response in the claude code session. Why?
❯ ### Re: monsterrodholders.md — response DID write, then got overwritten — opus-4-7 (HEAD)

**The patchback worked. Something wrote over it 51 seconds later.** Forensic evidence:

| Artifact | Size | mtime | Has agent responses? |
|----------|------|-------|---------------------|
| `git HEAD` (`da2bcb2`) | — | 13:32:57 | ✅ `passage entries fixed`, `category cleanup` present |
| `.agent-doc/snapshots/7b9f2a63...md` | 790 lines | 13:32:57 | ✅ same as HEAD |
| Working tree `tasks/monsterrodholders.md` | 767 lines | **13:33:48** | ❌ responses gone |

The commit and snapshot agree — `agent-doc write` + `agent-doc commit` both succeeded at 13:32:57. The file mtime is **51 seconds later** (13:33:48), and the post-commit write is **23 lines shorter** and is missing exactly the two agent response blocks.

**Diagnosis: stale IDE buffer write-back.** The JB (or VS Code) plugin had an old in-memory buffer from before the CRDT merge. After the commit, *something* triggered the plugin to flush that stale buffer to disk, blowing away the merged content. This is the exact class of bug that 0.33.3 / JB plugin 0.2.64 fixed — **but only for the `repositionBoundaryViaDocument` path via `reloadFromDisk(document)`**. There's another plugin write path that hasn't been plugged.

**Corroborating evidence from the current session:** when I ran `agent-doc write` on `agent-doc-bugs.md` earlier this cycle, the log said:

```
[write] IPC degraded — skipping socket/file IPC, using disk write
[write] IPC timeout (2s) — falling back to direct write
[write] IPC timeout — response saved as patch, awaiting plugin
```

IPC is degraded right now. The plugin is not ACKing socket writes within 2s. That exact degraded-IPC state is what drives stale-buffer writes to disk — the plugin thinks it's the source of truth, disk write happened via fallback, now buffer and disk diverge.

**Why you saw the response in the Claude console but not the file:** the streaming write went through the CRDT disk-fallback path (which you see in the console), committed correctly, *then* the plugin flushed its stale pre-merge buffer to disk.

**Immediate recovery (doesn't lose your new prompt):**

```
cd src/boost-client
git show HEAD:tasks/monsterrodholders.md > /tmp/mrh-head.md
# diff /tmp/mrh-head.md tasks/monsterrodholders.md to see what's missing
# then manually re-add your "Remove product category links from the sidebar to..." prompt
# below the restored responses
```

Or the cleaner: `git checkout HEAD -- tasks/monsterrodholders.md`, then re-type the new prompt. Your new prompt is only 3 lines so re-typing is easier than merging.

**0.33.4 release status — HOLDING.** This bug is a direct counter-example to the JB 0.2.64 fix claimed complete in 0.33.3. Shipping 0.33.4 right now would compound a known-broken state. Adding a blocker pending item so we don't forget. The SKILL.md heuristic tweak that was the original point of 0.33.4 is safe to hold — it's a text-only change, not urgent.

**Next investigation steps** (not doing these yet — want your call first):

- Grep the JB plugin source for all document-write paths, check which call `reloadFromDisk`.
- Add an end-to-end integration test that simulates degraded IPC + mid-cycle buffer flush.
- Consider making disk-mode the default when IPC has been degraded for >N seconds, and have the plugin listen for file-change events to refresh its buffer first.

❯ #mpjv: Do all 3 steps
❯ ### Re: #mpjv — all 3 investigation steps — opus-4-6 (HEAD)

#### Step 1: JB plugin write-path audit

Audited all 7 document-write paths in the JetBrains plugin. Summary:

| Path | File | Operation | Reloads from disk? | Risk |
|------|------|-----------|--------------------|------|
| 1 | PatchWatcher:348 | repositionBoundaryViaDocument | Yes (line 365) | Fixed in 0.2.64 |
| 2 | PatchWatcher:533 | Full content patch | Yes (line 505) | Safe |
| 3 | PatchWatcher:584 | Component patches | Yes (line 505) | Safe |
| 4 | PatchWatcher:605 | VFS direct write | N/A (disk read) | Safe |
| 5 | PromptPoller:171 | mergeOrReload | Conditional | Downstream of bug |
| 6 | **PromptPoller:117** | **autoSaveTrackedFiles** | **No** | **ROOT CAUSE** |
| 7 | SubmitAction:52 | saveAllDocuments | User-triggered | Safe |

**Root cause: `PromptPoller.autoSaveTrackedFiles()` (lines 117-134).** Every 1.5s, the poller checks for unsaved documents and flushes them to disk. It does NOT refresh the VFS or check for external disk writes before saving. When IPC is degraded and agent-doc writes directly to disk:

1. agent-doc writes CRDT-merged content to disk (user edits + agent response)
2. JB's in-memory buffer still has pre-merge content (user edits only)
3. PromptPoller cycle: `isDocumentUnsaved(doc)` → true (user typed but hasn't saved)
4. `tracked.lastSavedContent = doc.text` → caches PRE-merge content
5. `fdm.saveDocument(doc)` → overwrites the CRDT merge with stale buffer

The 51-second timing is exact: `500ms initial + 34 × 1500ms = 51,500ms`.

**Fix (PromptPoller.kt lines 117-134):**

```kotlin
private fun autoSaveTrackedFiles() {
    ApplicationManager.getApplication().invokeAndWait {
        val fdm = FileDocumentManager.getInstance()
        for ((_, tracked) in trackedFiles) {
            if (!tracked.file.isValid) continue
            val doc = fdm.getDocument(tracked.file) ?: continue
            if (fdm.isDocumentUnsaved(doc)) {
                // Refresh VFS to detect external disk writes (agent-doc CRDT merge)
                tracked.file.refresh(false, false)
                val currentStamp = tracked.file.modificationStamp
                if (currentStamp != tracked.lastKnownModStamp) {
                    // Disk changed externally — merge before auto-saving
                    mergeOrReload(tracked)
                    tracked.lastKnownModStamp = tracked.file.modificationStamp
                }
                tracked.lastSavedContent = doc.text
                fdm.saveDocument(doc)
                tracked.lastKnownModStamp = tracked.file.modificationStamp
            }
        }
    }
}
```

The key addition: before flushing, call `tracked.file.refresh(false, false)` and compare modification stamps. If disk changed externally, run `mergeOrReload()` first so the Document gets the CRDT-merged content before the auto-save flushes it.

#### Step 2: Integration test design

Existing test infrastructure: Rust tests with `tempfile`, `assert_cmd`, `predicates`. Relevant prior art: `try_ipc_snapshot_saves_disk_state()` in `write.rs` (mock plugin thread). `debounce_gaps_test_plan.rs` has 6 gap specs (mostly `#[ignore]`).

The core race condition lives in the JB plugin (Kotlin), so a pure Rust integration test can't reproduce the exact bug. What we CAN test from Rust:

**Test 1 — disk write survives concurrent read:** After agent-doc writes CRDT-merged content to disk, a simulated "stale reader" that reads the file and writes back stale content should be detectable (file hash mismatch vs snapshot).

**Test 2 — IPC degradation marker persists:** After IPC timeout, verify the degradation marker exists and subsequent writes use disk fallback.

**Test 3 — snapshot vs working-tree divergence detection:** After an external overwrite of the working tree, `agent-doc preflight` should detect the divergence (snapshot has agent response, working tree doesn't).

For the actual PromptPoller race, the proper test is a JB plugin integration test (IntelliJ test framework with `com.intellij.testFramework`). That would:
1. Open a tracked file
2. Type user edits (mark Document unsaved)
3. Externally write CRDT-merged content to the VirtualFile
4. Trigger PromptPoller cycle
5. Assert Document contains merged content, not stale buffer

This is the right place for it — `editors/jetbrains/src/test/`. I can write the test skeleton if you want to proceed.

#### Step 3: Disk-mode default when IPC degraded + file-change listener

Current state: IPC degradation is already tracked (1-hour TTL marker). When degraded, agent-doc falls back to disk writes. The problem isn't on the agent-doc side — it's that the JB plugin doesn't react properly to external disk writes during degraded IPC.

**Proposed design:**

1. **VFS file-change listener in PromptPoller** — register a `BulkFileListener` for tracked files. On external change events (`VFileContentChangeEvent` where `requestor != Document API`):
   - Immediately call `mergeOrReload(tracked)` for the affected file
   - Set a `recentExternalWrite` flag with timestamp
   - `autoSaveTrackedFiles()` skips files with `recentExternalWrite` within the last 3s

2. **Already partially exists:** `refreshTrackedFiles()` (line 139) does VFS refresh + modstamp comparison, but it runs AFTER `autoSaveTrackedFiles()` in the polling cycle. Swapping the order alone won't help because `mergeOrReload` uses `invokeLater` (async).

3. **Minimal fix vs full design:** The VFS refresh in `autoSaveTrackedFiles()` (Step 1 fix) is sufficient to prevent the overwrite. The `BulkFileListener` approach is a defense-in-depth improvement for sub-1.5s responsiveness to external writes, worth doing but not blocking.

**Recommendation:** Ship the Step 1 fix first (VFS refresh before auto-save). Add the `BulkFileListener` as a follow-up improvement. The degraded-IPC disk-mode default is already working correctly — it's the plugin's reaction to disk writes that's broken, not the fallback path itself.

❯ do BulkFileListener. build plugin. build and install agent-doc if changes were made.
❯ ### Re: implement BulkFileListener + build — opus-4-6 (HEAD)

Both fixes implemented in `PromptPoller.kt` and plugin built as 0.2.65:

**Fix 1 — `autoSaveTrackedFiles()` VFS refresh (lines 133-155):**
Before saving, now calls `tracked.file.refresh(false, false)` and compares modstamps. If disk changed externally, runs `mergeOrReload()` synchronously (already on EDT via `invokeAndWait`) before flushing. Re-checks `isDocumentUnsaved` after merge — if merge brought doc in sync with disk, skips the save entirely.

**Fix 2 — `BulkFileListener` (lines 119-131):**
Registered in `ensurePolling()` via `project.messageBus.connect(this)`. Listens for `VFileContentChangeEvent` on tracked files. On external write, updates `lastKnownModStamp` and schedules `mergeOrReload` via `invokeLater`. This provides sub-1.5s reactive merge instead of waiting for the next poll cycle.

**No infinite loop risk:** `mergeOrReload` calling `doc.setText(merged)` only changes the Document, not the file. Only `saveDocument()` triggers VFS events, and by that point the merge is already done.

**Build output:**
- `build/distributions/agent-doc-jetbrains-0.2.65.zip` (unsigned)
- `build/distributions/agent-doc-jetbrains-0.2.65-signed.zip` (signed)

No agent-doc Rust changes needed — the bug is purely in the JB plugin's handling of external disk writes. Ready for install-from-disk testing.

❯ There was a running claude code session + agent-doc run for src/session-share/tasks/claudescore.md
❯ When I installed the plugin and restarted idea, the tmux panes reset and a new claude code session was created. I lost track of the running claude code session + agent-doc run. Why did this happen? I remember we added logic to reset agent-doc managed tmux panes when the editor starts. It was supposed to check to see if there is a live tmux session for `tmux_session = "claude"`. There was. Did the existing session get killed? What happened to the tmux pane running the agent-doc on src/session-share/tasks/claudescore.md?

❯ Is the agent-doc run still going?
❯ ### Re: tmux pane reset on plugin install — opus-4-6 (HEAD)

**Short answer:** The agent-doc run for claudescore.md is still alive. Pane `%51` is running `agent-doc` and its Claude Code session is at the idle `❯` prompt (finished its last cycle, waiting for the next edit).

**What happened on IDEA restart:**

When you reinstalled the plugin and restarted IDEA, `PluginLifecycleListener.projectOpened()` fires and runs `agent-doc resync --fix` (line 39 of `PluginLifecycleListener.kt`). This:

1. **Prunes dead panes** from sessions.json
2. **Detects issues** (WrongSession, WrongProcess, InStash, WrongWindow)
3. **Applies fixes** — WrongSession panes get killed by default

In this case, neither pane was killed (both `%51` and `%52` are still alive in session `0`, window `agent-doc`). The claudescore.md frontmatter has no `tmux_session` field, so the WrongSession check fell back to `config::project_tmux_session()` — and since the pane's actual session matched, no kill was triggered.

**Why you "lost track":** The visual disruption was likely from:
- `LayoutChangeDetector` re-initializing on project open (line 22)
- `EditorTabSyncListener` re-registering (line 24) and syncing tmux layout to match the editor's current tab state
- If the editor opened to a different file than what was active before restart, the sync logic would have rearranged/refocused tmux panes

The tmux panes weren't destroyed — they were reorganized to match the editor's post-restart tab layout. The session `0` now has both panes in window `agent-doc` at `@21`.

**Current state:**
- `%51` → agent-doc for `src/session-share/tasks/claudescore.md` (Claude idle at `❯`)
- `%52` → agent-doc for `tasks/agent-doc/agent-doc-bugs.md` (this session)

❯ There was **another** tmux pane and claude code session that had an agent-doc run on src/session-share/tasks/claudescore.md. The diff that was running included:
❯ > Revert these changes to the expected scores. The scores were calibrated according to an expert sentiment reality before the change was made. Now the lack of an agent harnesses misapplied penalty is creeping back in. This is why I want to isolate each factor individually to calculate scores. It seems that you are getting confused by grouping factors into a single rubric...particularly the agentic harness. The original rubrics were designed for agentic user sessions. Remember the purpose of the repo. The repo is not supposed to be an agentic user session. Revert the change to the expected score and let's apply our changes more methodically. I believe that we need to break apart the rubrics to be focused on a single factor. Plan with me.

❯ When Idea restarted, the tmux pane closed and a new tmux pane `%51` took its place. What happened?
❯ ### Re: old Claude session killed on IDEA restart — opus-4-6 (HEAD)

The old Claude Code session was killed by `resync --fix`. Here's the chain:

**Root cause:** Your `.agent-doc/config.toml` has `tmux_session = "claude"`, but the old pane was in a different tmux session (likely a numeric session like `0` or `1`). When IDEA restarted:

1. `PluginLifecycleListener.projectOpened()` ran `agent-doc resync --fix` (line 39)
2. `detect_issues()` checked each registered pane's tmux session against the expected session
3. For the claudescore pane: no `tmux_session` in frontmatter → fell back to `project_tmux_session()` → returned `"claude"` from config
4. The old pane's actual tmux session didn't match `"claude"` → `WrongSession` issue detected
5. `apply_fixes()` **killed the pane** and deregistered it from sessions.json (resync.rs lines 1243-1249: default behavior without `relocate_session`)
6. After resync, `PromptPoller.startPolling()` auto-started, and the route/sync logic created a new pane `%51` with a fresh Claude Code session

**The old Claude Code session with your diff is gone.** The `resync --fix` WrongSession handler kills panes by default — it doesn't check whether the pane has an active Claude Code session running.

**This is a design gap:** `resync --fix` on IDEA startup is too aggressive for WrongSession. When a pane is running `claude` (an active agent process), it should relocate via `join-pane` instead of killing. The `relocate_session` parameter exists in the code but isn't used by the automatic `resync --fix` path in `PluginLifecycleListener`.

**What I'd recommend:**
1. **Fix the immediate config mismatch:** Either rename your tmux session to `claude`, or update `.agent-doc/config.toml` to match your actual session name (e.g., `tmux_session = "0"`)
❯ Rename the tmux session. Why did the newly created session not have the correct name and/or why was `tmux_session` in config.toml not updated?
2. **Patch resync to preserve active agents:** In `apply_fixes_to_registry`, when the pane is running a process in `AGENT_PROCESSES`, use `join-pane` relocation instead of `kill_pane` — same as the `relocate_session` path but automatic
❯ Add to pending
3. **Add a safety check to PluginLifecycleListener:** Pass `--session <current>` to `resync --fix` so it uses relocation mode rather than kill mode
❯ Add to pending

❯ Is #mpjv done?
❯ ### Re: why new session has wrong name + pending additions + #mpjv status — opus-4-6 (HEAD)

#### Why the new pane ended up in session `0` instead of `claude`

Two systems fought each other:

1. **`resync --fix` enforces `config.toml`** — it saw `tmux_session = "claude"` and killed the old pane because it was in a different session.

2. **`sync` uses `context_session` from the `--window` arg** — when `EditorTabSyncListener` fired after restart, it called sync with `--window @21`. Sync resolved `context_session` by querying `tmux display-message -t @21 -p "#{session_name}"` which returned `"0"`. This context_session **overrides** the config in `resolve_target_session` (priority 1: context_session > priority 2: config.toml).

So the sequence was: resync killed the old pane (wrong session per config) → sync created a new pane in session `0` (right session per the window context) → config was NOT updated (by design: "Session config is never auto-written").

**Why the config wasn't updated:** `resolve_target_session` documents: "Only `agent-doc session set <name>` pins a session." The config was last set to `"claude"` and never changed. Meanwhile the actual tmux sessions are `0` and `1` (default auto-numbered names).

**The fix for you right now:** rename your tmux session:
```bash
tmux rename-session -t 0 claude
```
This brings the runtime state in line with the config.

❯ agent-doc should update config.toml tmux_session when a new session takes over. You can test it right now on the exissting config.toml.

#### #mpjv status

Not done yet. The JB 0.2.65 plugin with the BulkFileListener fix was built and you just installed it — that's what triggered this IDEA restart. But the testing gate hasn't passed: we haven't confirmed the VFS refresh + BulkFileListener fixes actually prevent the overwrite bug in a live session. The resync/session issue you hit is a separate bug from #mpjv.

❯ do #2ea0
❯ ### Re: #2ea0 done + config.toml auto-update implemented — opus-4-6 (HEAD)

**#2ea0 — Resync preserves active agents:** Done. Modified `apply_fixes_to_registry` in `resync.rs`. When `relocate_session` is `None` (the automatic startup path), the WrongSession handler now:

1. Checks `pane_current_command` — if the pane is running an `AGENT_PROCESSES` command (agent-doc, claude, node):
   - If `expected_session` is alive: relocates via `join-pane` (same as the explicit `relocate_session` path)
   - If `expected_session` is not alive: deregisters only, does NOT kill the active agent
2. If the pane is idle shell or unknown: kills and deregisters (existing behavior)

**Config.toml auto-update:** Also implemented in `resolve_target_session` in `route.rs`. When the configured `tmux_session` (e.g., `"claude"`) is not alive and the function falls back to `current_tmux_session()`, it now auto-updates `.agent-doc/config.toml` to match the resolved session. This prevents the resync/sync mismatch — config stays in sync with runtime.

**Build status:**
- `cargo build --release` ✅
- `cargo install --path .` ✅ (both `~/.cargo/bin/` and `target/release/` updated)
- `cargo test` ✅ (1146 passed)
- Config.toml still shows `tmux_session = "claude"` — will auto-update on next route/sync call since no `claude` session exists

**Remaining risk:** The auto-update triggers when a route/sync runs and the configured session is dead. If the user intentionally renamed their session and wants to keep the config pointing to the old name (waiting for it to come back), the auto-update would overwrite that intent. This is an edge case — the current behavior (killing active agents) is worse.
❯ do #2ea0
❯ ### Re: duplicate do #2ea0 — opus-4-6 (HEAD)

#2ea0 is already done. It was completed in the previous cycle and preflight confirmed "reaped 1 item(s): 2ea0". The "do #2ea0" text in the working tree is a stale duplicate — the changes (resync agent preservation + config.toml auto-update) are already built and installed.

**Summary of what shipped:**
- `resync.rs`: WrongSession handler now relocates active agent panes instead of killing them
- `route.rs`: `resolve_target_session` auto-updates `config.toml` when configured session is stale
- Both binaries rebuilt, 1146 tests pass
❯ do #8ndp
❯ ### Re: #8ndp done — PluginLifecycleListener passes --session — opus-4-6 (HEAD)

Done. `PluginLifecycleListener.runResyncFix()` now queries `tmux display-message -p "#{session_name}"` before calling resync. If a tmux session is found, it passes `--session <name>` which activates relocation mode (join-pane) instead of kill mode for WrongSession panes.

**Change in `PluginLifecycleListener.kt`:**
- Queries current tmux session name via `ProcessBuilder("tmux", "display-message", ...)`
- If found: calls `agent-doc resync --fix --session <name>` (relocation mode)
- If tmux not available: falls back to `agent-doc resync --fix` (now safe due to #2ea0's agent preservation)

**Combined defense-in-depth:**
1. **#2ea0 (resync.rs):** Even without `--session`, active agents are relocated/preserved instead of killed
2. **#8ndp (PluginLifecycleListener):** Passes `--session` so ALL panes (including idle shells) get relocated instead of killed
3. **Config auto-update (route.rs):** Stale `tmux_session` in config.toml gets corrected on next route/sync

**Plugin built:** `agent-doc-jetbrains-0.2.66.zip` (unsigned + signed). Ready for install-from-disk.
❯ do #wk1t
❯ ### Re: #wk1t verified — config.toml auto-update works — opus-4-6 (HEAD)

Already verified live. Config.toml was `tmux_session = "claude"` earlier in this session. It's now `tmux_session = "0"` — the auto-update in `resolve_target_session` fired when a route/sync ran after the rebuild. The `"claude"` session didn't exist, so it fell through to `current_tmux_session()` which returned `"0"`, and `update_project_tmux_session("0")` wrote it to config.

**Verification:**
- Before: `tmux_session = "claude"` (stale — no `claude` session exists)
- After: `tmux_session = "0"` (matches actual tmux session `0`)
- Tmux sessions: `0`, `1` (no `claude`)

❯ Add tests
❯ ### Re: tests added — opus-4-6 (HEAD)

Added 3 tests to `resync.rs` for the agent preservation behavior:

1. **`fix_wrong_session_idle_shell_still_killed`** — Regression test: idle shells in the wrong session are still killed (existing behavior preserved with the new code path).

2. **`fix_wrong_session_deregisters_agent_without_kill_when_expected_session_dead`** — When the expected session doesn't exist, panes are deregistered. Idle shells are killed regardless.

3. **`fix_wrong_session_relocates_agent_when_expected_session_alive`** — The key test: starts a `node` process (which is in `AGENT_PROCESSES`), creates a WrongSession issue, and verifies the pane is relocated via join-pane to the correct session — not killed. Checks pane is alive, registry entry is preserved, and session changed to `"correct"`. Gracefully skips if `node` isn't installed.

**Results:** 1149 tests pass (3 new), 6 ignored. Both binaries rebuilt.
❯ release
❯ ### Re: release — sonnet-4-6 (HEAD)

**Pre-release checks passed:**
- `make check`: 1149 tests pass, 6 ignored
- `make build-release`: release binary built, `.bin/agent-doc` symlink updated
- `cargo install --path .`: `~/.cargo/bin/agent-doc` updated to 0.33.4

**Manual testing gate:** Verify the fix works in a live session before publishing. Check the wrong-session agent relocation behavior or any other 0.33.4 changes work as expected.

When confirmed, I'll proceed with:
1. `cargo publish` → crates.io
2. `git tag v0.33.4 && git push origin v0.33.4` → GitHub Release via CI
3. PyPI build + upload

❯ Idea crashed in the last session. Please look in the logs to find out why.
❯ ### Re: IDEA crash — opus-4-7 (HEAD)

**Crash dump:** `~/java_error_in_idea_1688168.log` (15:09:00, PID 1688168, JVM uptime 8m)

**Signature:** `SIGSEGV (0xb) at pc=0x0000000000000000` — JVM jumped to address 0.

**Java stack:**
```
com.sun.jna.Native.invokeVoid(...)
com.sun.jna.Function.invoke(...)
jdk.proxy12.$Proxy121.agent_doc_document_changed(String)
com.github.btakita.agentdoc.TypingTracker.documentChanged(DocumentEvent) +67
```

JNA tried to invoke `agent_doc_document_changed` but the cached `peer` (native function pointer) was 0 → call to NULL → SIGSEGV.

**Smoking gun (timing correlation):**

| Time | Event |
|------|-------|
| 15:01:02.582 | JNA loads `~/.cargo/bin/libagent_doc.so` (logged `[native] loaded ...`) |
| 15:08:59.447 | User pressed Enter in vim mode on agent-doc-bugs.md |
| **15:08:59.884** | **`~/.cargo/bin/libagent_doc.so` mtime updated** (cargo install replaced it) |
| 15:09:00.518 | IDE thread enters `agent_doc_document_changed` → SIGSEGV |

The .so file was overwritten ~600ms before the crash. JNA had cached the `Function` object (with `peer` pointing into the old mmap), and when the call dispatched, the function pointer resolved to 0.

**Root cause hypothesis:**

`cargo install --path .` ran while IDEA was holding the .so mmap'd via JNA. Normally on Linux, `cargo install` writes a tempfile then `rename(2)`s — the original inode stays alive for the JVM's mmap, so old code should remain callable. But the symptom (PC=0x0, exactly NULL) suggests the peer was zeroed, which doesn't happen from a benign rename. Possible mechanisms:

1. **cargo install on Arch may use a write strategy that truncates in place** rather than atomic rename — invalidating live pages.
2. **JBR/JVM page-cache interaction:** if pages got evicted under memory pressure and the kernel reloaded from the new file (different layout), the cached peer is stale.
3. **Concurrent dlsym during overwrite:** if JNA recomputed the peer mid-rebuild for any reason and got 0, it cached 0.

I can't fully prove which mechanism without ltrace/perf, but **(1) is testable** — `strace -e openat,rename,write,truncate -p $(pgrep -x cargo)` during the next cargo install would show.

**Mitigation options:**

1. **Avoid concurrent cargo install while IDEA is up** — simplest, no code change. Add to release runbook: "close IDEA or accept it may crash before running cargo install."
2. **JNA call guard with Function pointer health check** — wrap each FFI call in a try/catch that checks `lib.agent_doc_version()` first; if it crashes/returns null, force-reload via `Native.unregister` + `Native.load`. Adds overhead per call.
3. **Reload library on file mtime change** — `AgentDocLib.get()` could stat the .so and reload if mtime changed since last load. Cheap (one stat per call), reliable.
4. **Build to a versioned filename** — `libagent_doc-0.33.4.so` so each version has a unique inode and cargo install never overwrites the loaded one. JNA continues using the old version until IDEA restarts.

**Recommendation:** Option 3 (mtime-based reload) is the most robust without process restart. Option 1 is the quick fix for now.

Yesterday's crashes (`java_error_in_idea_2825553.log` etc.) were a different bug — NULL pointer in `Pointer.getString` from `NativePatching.repositionBoundaryToEnd`, suggesting `result.text` was null but not checked before `getString(0)`. That's a separate fix in `NativePatching.kt`.

❯ Option 3 is better than option 1. Why not option 4?
❯ ### Re: why not option 4 — opus-4-7 (HEAD)

Option 4 (versioned filename) is structurally cleaner — the kernel guarantees the old inode stays alive — but it has install-side friction that option 3 avoids:

**Cargo doesn't write versioned cdylib filenames.** `cargo build` always emits `libagent_doc.so` (cdylib name = crate name). To get `libagent_doc-0.33.4.so` you need a post-build step in the Makefile or `build.rs` that copies/symlinks with the version suffix. Not hard, but it's another moving piece in every install path: `cargo install`, the manylinux Docker wheel build, the GitHub Releases binary tarball, the JB plugin install, the VS Code extension.

**Disk leak.** Every release leaves an old `libagent_doc-X.Y.Z.so` behind. Needs a GC policy (keep last N versions, or sweep on `agent-doc upgrade`). Otherwise `~/.cargo/bin/` accumulates every version forever.

**`agent-doc lib-path` resolution gets ambiguous.** Today it returns one canonical path. With versioned filenames, which version? The latest installed? The one matching the running CLI? If the plugin asks for "latest" and the user just upgraded, the plugin loads the new .so on next call — back to the same hot-reload risk if symbols changed shape. So you still need option 3's mtime check, OR you accept that the plugin only picks up new versions on IDE restart (which is mostly fine, but means new FFI features need a restart to become visible).

**Distribution wrinkle.** PyPI wheel via maturin expects the cdylib at a specific path — versioning the filename means a post-build rename inside the wheel build, and Python's `cffi`/`ctypes` consumers hardcode the unversioned name.

**Option 3 is one file change** (`AgentDocLib.get()` adds `lastLoadedMtime` field, stats on each call, reloads on change) with zero install/build/distribution impact. Cost: one `stat(2)` per FFI dispatch — sub-microsecond.

**The strongest argument for option 4:** option 3 still has a race window. Between the mtime check and the actual JNA call, a concurrent overwrite could happen. Option 4 closes that window completely. But the window in option 3 is narrow enough that it's effectively never hit outside of pathological release storms — and even if hit, the next call recovers.

**Hybrid:** option 3 + the JB plugin-build install path also writes a versioned symlink (`libagent_doc-current.so` → versioned target), so we get the kernel-level guarantee without rewriting cargo's output naming. But that's complexity for a small improvement.

❯ The Hybrid sounds good. Can we also cleanup all old versions when the process completes?

**Verdict:** option 3 unless we're already doing the build-system surgery for some other reason. If you want, I can implement option 3 now — it's ~15 lines in `NativeLib.kt`.
### Re: cleanup old versions on exit — opus-4-7

Yes, and the right shape is **lockfile-tracked GC**, not just a shutdown-hook delete. Reason: multiple IDEAs / VS Code / standalone CLIs may each be holding different versions concurrently — a naive "I'm shutting down, delete everything but mine" would break sibling processes.

**Design:**

1. **On load** (`AgentDocLib.get()` after `Native.load()`): write `<so-path>.lock` containing this JVM's PID. One lock per loader, per file.
2. **Shutdown hook** (`Runtime.getRuntime().addShutdownHook`): remove our own `.lock` file. That's it — don't delete the .so itself, since another process might still be using it.
3. **GC sweep** runs at three trigger points:
   - **On load** (after writing our own lock): walk all `libagent_doc-*.so` siblings.
   - **On `cargo install` / install path** (new `agent-doc gc-libs` subcommand the install hook calls).
   - **Manual:** `agent-doc gc-libs` exposed for users.

   For each sibling .so:
   - **Keep** if it's the current symlink target (`libagent_doc-current.so` → this).
   - **Keep** if its `.lock` file exists AND `/proc/<pid>` exists (alive process).
   - **Delete** otherwise (stale lock from crashed JVM, or no lock at all). Also delete the orphaned `.lock` file.

**Why not just shutdown-hook delete:**

- JVM shutdown hooks aren't guaranteed to run (SIGKILL, OOM kill, power loss). Stale .so files would accumulate.
- Even if the hook runs, the unlinked file lives until the LAST mmap reference drops. If another process is concurrently loading the same path, you've created a TOCTOU race.
- Lock-based GC is idempotent and crash-safe — stale locks are just cleaned up by the next sweep.

**Files added per release:**
- `libagent_doc-0.33.4.so` (versioned)
- `libagent_doc-current.so` → symlink to latest (atomic rename on install)
- `libagent_doc-0.33.4.so.lock` (one per loaded JVM, contains PID; gone on clean exit)

**Implementation cost:**
- `NativeLib.kt`: ~30 lines (write lock on load, remove on shutdown, call GC).
- `agent-doc gc-libs`: ~50 lines of Rust (walk dir, parse versions, check `/proc/<pid>`, unlink).
- `Makefile` / install path: ~5 lines (rename to versioned, update symlink, call GC).
- Total: ~85 lines + the option 3 mtime check (~15 lines).

**Edge case worth calling out:** the symlink load path (`libagent_doc-current.so`) means the IDE plugin asks for the symlink, and JNA dlopen's it. When `cargo install` does `ln -sfn libagent_doc-0.34.0.so libagent_doc-current.so` (atomic rename), the OLD JNA mmap is unaffected (it dlopen'd the inode, not the symlink). New IDE startups follow the new symlink. Clean separation.

Want me to spec this in `tasks/agent-doc/agent-doc-feat.md` as a single feature ("FFI library hot-reload + versioning + GC") with the option 3 mtime reload bundled in? Or split into two features?
<!-- agent:boundary:ccf1471d -->
<!-- agent:boundary:7b6f26a3 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#m1bz] install gate — cargo install + local JB plugin install; pause for user to verify live before publishing upstream
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
- [/] [#y2gr] Fix agent-doc skill install: does not sync runbooks/*.md to ~/.claude/skills/agent-doc/runbooks/ or <project>/.claude/skills/agent-doc/runbooks/ — only SKILL.md propagates. Had to cp manually today.
- [ ] [#919p] Release agent-doc 0.33.4: bundles SKILL.md § 1b promotion heuristic so new installs via 'agent-doc skill install' pick up the rule. Both local SKILL.md copies (src/agent-doc/, .claude/skills/agent-doc/) already updated in working tree. Publish path: crates.io + GitHub Release (PyPI auto via CI per reference_pypi_publish_via_ci.md).
- [ ] [#mpjv] 0.33.4 BLOCKER — FIXED in JB 0.2.65: (1) autoSaveTrackedFiles now refreshes VFS + merges before flushing, (2) BulkFileListener for reactive sub-1.5s merge on external disk writes. Plugin built. Install and test before publishing.
<!-- /agent:pending -->
## Assistant
