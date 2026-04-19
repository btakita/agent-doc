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
### Re: model-specific AGENTS.md guardrails — opus-4-6

The concern is valid: Opus 4.7 removed sections like "Self-Improvement Loop", "Task Management", and "Autonomous Bug Fixing" as anti-laziness scaffolding — but those sections may have been serving as useful guardrails for 4.6 (which occasionally needs more explicit process reminders).

**Options:**
1. **Model-tier-gate runbook** — add a section that injects model-specific AGENTS.md rules. When `effective_tier` is resolved, the agent reads the appropriate set of workflow rules. Low tier models get more scaffolding, high tier models get lean rules.
2. **Conditional sections in AGENTS.md** — `<!-- agent:model:opus-4-6 -->` gated blocks that only apply when the current model matches. Would need binary support.
3. **Restore the removed sections** — simpler: just put them back and accept the token cost for 4.7.

Option 1 is the cleanest for agent-doc sessions (where model tier is already resolved). Option 3 is the fastest.

Also: the response from Opus 4.7 got duplicated in the exchange — orphan recovery applied the same content twice. This is cosmetic but inflates the exchange.

Your "Why did" message appears incomplete — please finish the thought and I'll respond.

Added to pending: model-specific AGENTS.md guardrails.
<!-- agent:boundary:3c7fdf37 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#m1bz] install gate — cargo install + local JB plugin install; pause for user to verify live before publishing upstream
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
- [ ] [#x04k] Model-specific AGENTS.md guardrails — add conditional scaffolding sections for models that need explicit process reminders (4.6) vs those that don't (4.7). Either via model-tier-gate runbook or AGENTS.md sections gated by model tier.
<!-- /agent:pending -->
## Assistant
