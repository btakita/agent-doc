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
# Return corky to session 14, provision fresh pane for justin-caird.md
tmux join-pane -t 14 -s %544
agent-doc resync --fix
```
Then re-open `tasks/recruit/justin-caird.md` in the editor — sync will auto-start a new agent-doc session.

**Potential enhancement:** `resync` could detect process-name mismatches (pane running something other than `agent-doc`/`claude`) and flag them as suspect entries.

Describe
- [ ] Consider resync enhancement: flag panes whose running process doesn't match expected (agent-doc/claude)
### Re: resync WrongProcess enhancement

Already implemented. `agent-doc resync` just output:

```
Found 1 issue(s) (run with --fix to resolve):
  tasks/recruit/justin-caird.md (pane %544) running 'corky', expected agent-doc/claude
```

`resync.rs` has `WrongProcess` as a first-class issue type (alongside `WrongSession`, `WrongWindow`, `InStash`). It checks `#{pane_current_command}` against `AGENT_PROCESSES = ["agent-doc", "claude", "node"]`.

**Fix behavior:** `--fix` deregisters `%544` without killing corky. Then re-opening `justin-caird.md` auto-provisions a fresh session.

Is it installed? I don't see the justin-caird tmux session.
### Re: plugin installed? + justin-caird session

**Plugin v0.2.48:** Installed in IntelliJIdea2026.1 (`agent-doc-jetbrains-0.2.48.jar`). `plugin list` shows nothing because it's a direct install (not via plugin manager) — that's expected.

Older installs still have stale versions:
- IntelliJIdea2025.3 → v0.2.24
- PyCharm2025.3 → v0.2.12

I see 0.2.48

![img_2.png](img_2.png)

**Justin-caird session:** Not present because `%544` is registered but running `corky`. The agent-doc session for justin-caird.md was never provisioned after the swap. Fix:

```bash
agent-doc resync --fix
```

Then re-open `tasks/recruit/justin-caird.md` in the editor — sync will auto-start a fresh session.

Still did not happen. The `corky watch` tmux pane is still in there.

How did the `corky watch` pane end up in tmux session 10 to begin with?
### Re: corky in session $10 + resync still pending

**`resync --fix` hasn't run** — the WrongProcess issue is still in the registry. Important: `resync --fix` deregisters the sessions.json entry but does NOT kill or move corky. The pane will still be physically visible in the window after the fix. Corky leaves when sync stashes the excess pane after justin-caird.md provisions a new session.

Full sequence to recover:
```bash
agent-doc resync --fix
# then re-open tasks/recruit/justin-caird.md in the editor
```

**How corky ended up in session $10 — exact chain:**

Two-step failure in the pre-v0.31.18 code:

1. **Cross-session rescue_from_stash():** `%544` (corky) was in session $14's stash window. When something triggered `rescue_from_stash()` for session $10, the old code called `tmux swap-pane` without checking sessions — this moved `%544` from $14's stash into $10's agent-doc window.

We should **always** check the tmux session before running **any** `tmux swap-pane`. Please extract a function for `tmux swap-pane` that validates the tmux session.

I don want the system to "self-correct" in case an agent-doc tmux window moved to another tmux session. I don't want this bug where the tmux pane was moved from the correct tmux session to another tmux session. 

2. **Lazy claim:**  When `%533` (the real justin-caird.md Claude session) exited, the next open of justin-caird.md hit Strategy 2 in `resolve_or_create_pane()` (`route.rs:261-268`): registered pane was dead → `find_target_pane()` found `%544` (corky, now in session $10's window) → registered it as the justin-caird.md pane → sent the `agent-doc route` command to corky's pane.

Both bugs contributed: rescue brought corky into the window, lazy claim adopted it as the wrong document's pane. v0.31.18 fixes rescue (session guard). Lazy claim is still a risk if a foreign process is already in the window — that's the WrongProcess detection `resync` now covers.
### Re: swap_pane_guarded()

Added `swap_pane_guarded(tmux, src, dst, expected_session)` to `sessions.rs` and updated both call sites.

**`sessions.rs`** — new helper after `capture_pane`:
```rust
pub fn swap_pane_guarded(tmux: &Tmux, src: &str, dst: &str, expected_session: &str) -> Result<()> {
    // check src session
    // check dst session
    // only then: tmux.swap_pane(src, dst)
}
```
Returns `Err` with a clear message if either pane is in the wrong session — callers fall through to `join-pane` as before.

**Call sites updated:**
- `route.rs:328` — `rescue_from_stash()`: `tmux.swap_pane(pane_id, target)` → `sessions::swap_pane_guarded(tmux, pane_id, target, target_session)`
- `sync.rs:760` — stash rescue path: same pattern, uses `target_sess` as the expected session

Why did the original `rescue_from_stash()` call identify the incorrect tmux pane (running `corky watch`)?

`cargo check` + `cargo clippy` — clean.

This closes the last direct `tmux.swap_pane()` callsite in agent-doc code. Any future accidental `tmux.swap_pane()` call skips the guard — so this should be noted as a convention: prefer `swap_pane_guarded()` over `tmux.swap_pane()` in agent-doc.
### Re: why did rescue_from_stash() identify the wrong pane?

`rescue_from_stash()` doesn't identify the pane — it receives it from `sessions::lookup()`. The wrong pane was in the registry because of a two-step sequence:

**Step 1 — corky enters session $10's window (first incident):**
An earlier `rescue_from_stash()` (pre-v0.31.18, no session guard) found `%544` (corky) stashed in session $14 and called `swap_pane` unconditionally → physically moved corky into session $10's agent-doc window.

**Step 2 — lazy claim registers the wrong pane:**
When `%533` (the real justin-caird.md Claude pane) exited, `resolve_or_create_pane()` fell to Strategy 2. `find_target_pane()` found `%544` (corky, now in the window) and called:
```rust
sessions::register("fb7495d9", "%544", "justin-caird.md")
```
Now corky IS the registry entry for justin-caird.md — legitimately registered, wrong process.

**Step 3 — all subsequent calls operate on the now-registered wrong pane:**
Future `rescue_from_stash()` calls for justin-caird.md receive `%544` from `sessions::lookup()`. They're acting correctly on the registered pane; it's just that the registered pane is running the wrong process.

`swap_pane_guarded()` prevents Step 1 from recurring. The lazy claim path (Step 2) is the remaining risk — `find_target_pane()` picks any alive pane without checking the running process. A secondary hardening would be adding a process check in Strategy 2, skipping panes not in `AGENT_PROCESSES`.

The issue I'm running into now is I have an 0:agent-doc window in tmux session 14. How sdo I get those tmux panes back into tmux session 10 using agent-doc?
### Re: moving session $14 panes to session $10

**agent-doc has no built-in "relocate pane to session" command.** Two options:

**Option 1 — Physical move with tmux (immediate):**
```bash
tmux join-pane -t 10 -s %543
tmux join-pane -t 10 -s %548
```
Moves each pane from session $14 into session $10's current window. The running agent-doc sessions (claudescore.md, xbow.md) continue in $10.

**Option 2 — Deregister + re-provision:**
```bash
agent-doc resync --fix
# then re-open claudescore.md and xbow.md from within session $10
```
`resync --fix` currently only handles `WrongProcess` (`%544`/corky). The `WrongSession` panes (`%543`, `%548`) aren't flagged — they're alive and correct, just provisioned in $14 (because the docs were opened from $14).

**Enhancement to add:** `resync` should support a `WrongSession` fix mode — `join-pane` to relocate panes to a target session. That would give `agent-doc resync --fix --session 10` to pull all registered panes into $10.

For now: `tmux join-pane -t 10 -s %543 && tmux join-pane -t 10 -s %548`.

Why is the `corky watch` pane claiming tasks/recruit/justin-caird.md?
### Re: why is corky claiming justin-caird.md? (HEAD)

`resync --fix` hasn't been run. The registry still has `%544` (corky) → `justin-caird.md`. That mapping persists until explicitly cleared — every sync, open, and route for that document targets corky.

Fix:
```bash
agent-doc resync --fix
```
Then re-open `tasks/recruit/justin-caird.md` from the editor. Deregisters `%544`, provisions a fresh session.

**Note:** preflight also flagged `%543` and `%548` (the session $14 agent-doc panes) as non-idle in the stash window. They've been moved to the stash rather than the active window — they'll be rescued or killed on next open of those documents.
<!-- agent:boundary:26e909c5 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Run `agent-doc resync --fix` then re-open justin-caird.md
- [ ] Build + release v0.31.19 with swap_pane_guarded()
- [ ] Consider: add AGENT_PROCESSES check to Strategy 2 (lazy claim) in resolve_or_create_pane() to skip foreign-process panes
- [ ] Consider: resync WrongSession --fix mode using join-pane to relocate panes to target session
<!-- /agent:pending -->
