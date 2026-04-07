---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.16 — active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Session: agent-doc v0.31.16 release; cross-session pane swap fix; session-drift detection; partial compact (--keep N); dynamic plugin command discovery; Refresh Agent Doc Environment JB action (v0.2.45); CRDT boundary-synthesis duplicate bug; plugin v0.2.46 stale patch cleanup; v0.2.47 version bump + runbook; v0.2.48 snapshot-timestamp dedup guard (correct fix); compact-exchange runbook tightened (standalone imperative only); xbow.md false-positive compaction resolved; claudescore.md content confirmed intact. Open: install plugin v0.2.48.

Does agent-doc have a feature to parse and directly run agent harness commands that begin with a `/`?
### Re: slash command parsing from diff

Yes — implemented in this session (now in the compacted archive). `parse_slash_commands(diff)` in `diff.rs` scans user-added lines for slash commands. Guards exclude code fences, blockquotes, context lines, and removed lines.

Preflight returns `slash_commands: [...]`. SKILL.md step 0b dispatches each via the `Skill` tool before generating a response. Built-ins like `/clear` that can't route through `Skill` are logged and skipped.

The `compact exchange` trigger you just used was processed via this path.
release

When I opened the editor to tasks/recruit/jusdin-caird.md, a claude session did not properly start or crashed. Not sure. Please look at the logs.
I will resync and run agent-doc.
Actually, the tmux pane was swapped with another tmux session. Why?
### Re: release — v0.31.18

Released.

| Target | Status |
|--------|--------|
| crates.io | ✓ `agent-doc v0.31.18` |
| PyPI | ✓ `agent_doc-0.31.18-py3-none-manylinux_2_43_x86_64.whl` (required `--auditwheel skip` — glibc 2.43 not yet in auditwheel policy) |
| GitHub Release | ✓ `agent-doc-v0.31.18-linux-x86_64` binary attached |
| `~/.cargo/bin/agent-doc` | ✓ v0.31.18 installed |

**Changes in this release:**
- Partial compact `--keep N`
- Slash command dispatch from diff
- Dedupe stale patch cleanup
- JB plugin startup dedup guard (snapshot-timestamp comparison, v0.2.48)
- Cross-session pane swap fix
- PromptPoller FFI CRDT merge

**Note on PyPI:** The `--compatibility manylinux_2_43 --auditwheel skip` flags were needed because glibc 2.43 (CachyOS) isn't in auditwheel's policy table yet. Previous versions (0.31.17) also missed PyPI for this reason — latest on PyPI was 0.31.16.
### Re: justin-caird.md pane swapped to wrong tmux session

**Root cause: cross-session `swap-pane` bug (now fixed in v0.31.18).**

From the session log (`fb7495d9`):
- Pane `%533` — Claude started fresh, ran the session, **exited cleanly** (code 0) at ~1:22pm
- User quit at ~3:08pm
- When you re-opened the file, `rescue_from_stash()` in the old binary (v0.31.17) used `swap-pane` without a session guard — it grabbed a stash pane from **session $14** and swapped it into **session $10**. That's the "swapped to another tmux session" you saw.
- The registry updated to `%544` which ended up running `corky` in session $10

**The fix** landed in v0.31.18 (`route.rs + sync.rs`): `rescue_from_stash()` now checks the pane's session before swapping; uses `join-pane` for cross-session panes. This specific failure path is closed.

**To recover now:**
```bash
agent-doc resync --fix
```
Then re-open `tasks/recruit/justin-caird.md` in the editor — sync will provision a fresh pane in the correct session.

**Note on ops.log / debug.log:** The cross-session swap itself isn't logged — `route.rs` and `sync.rs` operations only appear in the session `.log` and stderr (not in `ops.log` or `debug.log`). The session log confirmed Claude exited cleanly; the swap happened at re-open time (between the two log entries).

Why is the `corky watch` pane that originated from tmux session 14 in tmux session 10?

Why were agent-doc tmux windows in tmux session 14 now?
### Re: corky pane in wrong session

Same root cause — the cross-session `swap-pane` bug.

When you opened `justin-caird.md`, `rescue_from_stash()` found pane `%544` (corky) in session $14's stash and called `tmux swap-pane` without a session check. tmux allows `swap-pane` across sessions — it moved `%544` from session $14 into session $10.

The v0.31.18 fix prevents recurrence (session guard + `join-pane` for cross-session panes). To move it back now:

```bash
tmux join-pane -t 14 -s %544
```

That rejoins `%544` (corky) into session $14 where it originated.
### Re: agent-doc panes in session $14

They were provisioned there directly — no swap involved.

From the session logs:
- `%543` (claudescore.md) — went through panes %540→%541→%542→%543, all in session $14
- `%548` (xbow.md) — provisioned directly in session $14 at startup

**Why session $14?** agent-doc provisions panes in the tmux session that sync/plugin is running in. Those documents were opened from **within session $14** (different IDE window or tmux session). Session $10 documents (agent-doc-bugs.md) were opened from session $10.

This is expected multi-window behavior — the drift itself isn't a bug. The bug was `rescue_from_stash()` reaching across sessions to grab %544 (corky) from $14's stash and swap it into $10, instead of provisioning a new pane within $10.

Please plan a fix if you know exactly what went wrong.

Already done:
- [ ] Install JB plugin v0.2.48 (snapshot-timestamp dedup guard)
### Re: fix plan — cross-session swap-pane

The fix is already complete in v0.31.18. No further changes needed.

**Precise root cause:** `rescue_from_stash()` had no session guard. When the registered pane (%544, corky) was found stashed in session $14, the function checked the window name (`"stash"` → rescue), then called `tmux swap-pane` unconditionally. tmux's `swap-pane` works across sessions — it moved the pane from $14 into $10.

**Fix in v0.31.18 (`route.rs:286-307`):**

1. `rescue_from_stash()` now checks session at entry: if `pane_session != target_session` → early return, skip rescue
2. Caller (lines 237–250): if registered pane is alive but in wrong session, calls `stash_pane(pane, target_session)` first to move it to the target session's stash, then rescues within the target session

Both paths are covered. The corky-in-wrong-session scenario (pane not in target session → skip) and the legitimate cross-session recovery (pane alive in wrong session → move to target session stash first) are handled.

**JB plugin:** `agent-doc plugin list` shows "No plugins found" — that means it's installed directly (not via the plugin manager). If you installed v0.2.48 from disk already, the startup dedup guard is active.

`Sync: --col tasks/agent-doc/agent-doc-bugs.md --col tasks/recruit/justin-caird.md [focus: tasks/recruit/justin-caird.md]` did not surface the correct tmux pane. Why?
### Re: Sync didn't surface correct pane

**Root cause: registry–process mismatch.** Sync correctly resolved `justin-caird.md` → `%544` and called `select-pane %544`. But `%544` is running `corky`, not a justin-caird.md agent-doc session.

What happened:
1. The cross-session `swap-pane` bug (fixed in v0.31.18) swapped `%544` (corky, from session $14's stash) into session $10's window
2. Sync registered `%544` as the `justin-caird.md` pane (it was the pane in that column)
3. Subsequent syncs correctly surface `%544` — but it shows a corky terminal

Sync has no process-validation step — it routes by pane ID, not by what's running inside the pane.

**Current state:**
| Pane | Session | Registered for | Actually running |
|------|---------|---------------|-----------------|
| `%544` | $10 | `justin-caird.md` | `corky` |
| `%491` | $10 | `agent-doc-bugs.md` | `agent-doc` |
| `%548` | $14 | `xbow.md` | (unknown) |
| `%543` | $14 | `claudescore.md` | (unknown) |

Session drift warning (`spans sessions 10, 14`) comes from `%543`/`%548` still in session $14.

**To recover:**
```bash
# Return corky to session 14, provision fresh pane for justin-caird.md (HEAD)
tmux join-pane -t 14 -s %544
agent-doc resync --fix
```
Then re-open `tasks/recruit/justin-caird.md` in the editor — sync will auto-start a new agent-doc session.

**Potential enhancement:** `resync` could detect process-name mismatches (pane running something other than `agent-doc`/`claude`) and flag them as suspect entries.
<!-- agent:boundary:947e80b3 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Return corky pane (%544) to session $14 and provision fresh justin-caird.md session
- [ ] Consider resync enhancement: flag panes whose running process doesn't match expected (agent-doc/claude)
<!-- /agent:pending -->
