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
release
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
### Re: skill fix applied — opus-4-7

**Done — edited § 1b in both canonical and project-local SKILL.md.**

| File | Status |
|------|--------|
| `src/agent-doc/SKILL.md` (canonical, bundled in crate) | ✅ heuristic added after § 1b paragraph |
| `.claude/skills/agent-doc/SKILL.md` (project-local copy) | ✅ identical — `cmp` confirms SYNCED |

**New rule added to § 1b:**

> **Promotion heuristic (when to add pending in the same cycle):** if your response ends with a numbered list of distinct, actionable recommendations (e.g., "What I'd recommend: 1. ..., 2. ..., 3. ..."), and either pending is currently empty OR the user asked for a backlog / "tasks" / "todo", add each recommendation as a pending item via `--pending-add` in the same `agent-doc write` invocation. Don't leave actionable ranked items as prose-only — the pending component is where backlog lives. Skip promotion when items are hypotheticals, options you're asking the user to choose between, or already captured elsewhere.

**What's still needed to ship this:** the edit only affects already-installed skills. For the bundled copy to reach *new* installs via `agent-doc skill install`, the agent-doc crate needs a release (0.33.4). Adding a pending item for that so it's tracked rather than lost — per the heuristic I just wrote. Also eating my own dog food: this cycle *does* end with an actionable next step, so I'm `--pending-add`ing it in the same write.

Heuristic applies recursively. Nice.
<!-- agent:boundary:b058fd85 -->
<!-- agent:boundary:2273296c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#m1bz] install gate — cargo install + local JB plugin install; pause for user to verify live before publishing upstream
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
- [/] [#y2gr] Fix agent-doc skill install: does not sync runbooks/*.md to ~/.claude/skills/agent-doc/runbooks/ or <project>/.claude/skills/agent-doc/runbooks/ — only SKILL.md propagates. Had to cp manually today.
- [ ] [#919p] Release agent-doc 0.33.4: bundles SKILL.md § 1b promotion heuristic so new installs via 'agent-doc skill install' pick up the rule. Both local SKILL.md copies (src/agent-doc/, .claude/skills/agent-doc/) already updated in working tree. Publish path: crates.io + GitHub Release (PyPI auto via CI per reference_pypi_publish_via_ci.md).
<!-- /agent:pending -->
## Assistant
