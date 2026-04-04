---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Identified SKILL.md gap: no runbook reference section; compact should use `agent-doc compact`
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04)

**Bugs fixed this session:**

1. **Console output spam** — 24 task files contaminated. Cleaned + squashed 219 commits to 1.
2. **Boundary (HEAD) marker stripping** — concurrent preflight commits stripped markers. Fixed in `git.rs`.
3. **Stashed panes can't return** — registry window pointed to stash. Fixed in `resync.rs` with fallback.
4. **Claim not atomic** — modified files before validating pane. Fixed in `claim.rs`.
5. **Verbose git status in commit** — filtered to only print commit result line in `git.rs`.
6. **Window closure (last-pane guard)** — `sync.rs:687-701` stashed all panes including last. Fixed with guard.
7. **Navigation swap** — three fixes:
   - `SessionScope` allows cross-session joins/swaps (tmux-router `sync.rs:252-267`)
   - `--window` is authoritative for `target_session` (tmux-router `sync.rs:740-743`)
   - `Cargo.toml` path dep ensures local tmux-router is used

**Navigation swap spec (agreed design):**

```
Editor navigates to file B (was viewing file A)
├── B has agent_doc_session?
│   ├── YES → B has claimed pane in sessions.json?
│   │   ├── YES → join-pane B's pane from stash, stash A's pane
│   │   └── NO  → split + claim B with new session, stash A's pane
│   └── NO  → do nothing (keep A's pane visible)
```

**Verified:** 4-step cycle test passes (bugs-only → bugs+runbooks → runbooks-only → bugs+runbooks). All transitions work with cross-session stash/retrieve.

**Current tmux state (8 panes, 4 sessions):**
- %433 (bugs) + %431 (runbooks) in 5:agent-doc (correct)
- %434 (claim-error) + %436 (briantakita.me) displaced in session 3
- %13 (corky) in 1:stash, %84/%399/%400 shells

Why did you not use the compact-exchange runbook? Which would have told you to use `agent-doc compact` instead of the Edit tool?
What is missing from the agent-doc SKILL.md for runbook usage?
### Re: Compact runbook and SKILL.md gap (HEAD)

**Why I didn't use the runbook:** I forgot the compact-exchange runbook existed at `.claude/skills/agent-doc/runbooks/compact-exchange.md`. I went straight to the Edit/Write tool instead of checking runbooks first.

**What the runbook says I should have done:**
1. Read exchange content
2. Summarize (preserve decisions, discoveries, open items)
3. Run `agent-doc compact <FILE> --message "summary text"`
   - Archives original to `.agent-doc/archives/`
   - Replaces exchange with archive pointer + summary
   - Atomic snapshot update
4. Commit via `agent-doc commit`

Instead I used `Write` to overwrite the file, then manually updated the snapshot — fragile and loses the archive.

**What's missing from SKILL.md for runbook usage:**

The SKILL.md has no section about runbooks. It should instruct the agent to:
1. Check `.claude/skills/agent-doc/runbooks/` for relevant runbooks before executing operations like compaction
2. Specifically mention `compact-exchange.md` when "compact exchange" is requested
3. Use `agent-doc compact --message` instead of Edit/Write for exchange compaction

This is a SKILL.md gap — the skill should reference its own runbooks as standard operating procedures.
<!-- agent:boundary:33c1c472 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Clean spam from all 24 affected tasks/**/*.md files
- [x] Squash contaminated git commits (219 → 1)
- [x] Fix: boundary repositioning strips (HEAD) marker
- [x] Fix: stashed panes can't return
- [x] Fix: claim should be atomic
- [x] Remove agent:console from default scaffold (already done)
- [x] Suppress verbose git status in commit output
- [x] Fix: window closure — last-pane guard in unmanaged stash path
- [x] Navigation swap: cross-session joins + --window authority + path dep
- [ ] Publish tmux-router 0.3.9 to crates.io (then revert path dep)
- [ ] Add runbook reference section to SKILL.md
<!-- /agent:pending -->
