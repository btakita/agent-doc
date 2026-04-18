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
v0.33.0 session. Landed: tsift project extracted (btakita/tsift, src/tsift submodule, tasks/tsift.md) absorbing skills-janitor; #9v7h (--pending-add-gated + hash echo + state marker rejection); #txex (notify --pending-add/--pending-add-gated, auto-create pending, pending-only mode); #hrmt (synced repo-local SKILL.md to v0.32.0); #e46w/#f8j3/#y4da/#zvmd/#np8p/#0xka (already fixed: boundary lag, narrow_to_submodule, annotate_re_headings); #77mf (typed gate markers [/release]/[/deploy], resolve-gate per-file+project-wide, set-gate-type, skill-layer inference on do); v0.33.0 released crates.io+GitHub (typed gates, resolve-gate, --pending-only, --pending-add-gated, --status, notify pending, session clear, supervisor PTY); #p2xm already fixed (orphaned patch:exchange leak template.rs:257-265); #wcf5 (copy-on-read guard: mtime capture, skip recovery on mtime change, 4 integration tests). State: 1116 tests, 2 gated pending (#a002 awaiting production telemetry, #ha3h blocked on multi-user support).
❯ When I transferred pending items to tsift.md, some of the pending items have `[#] [#] [#] [#] [#] [#] [#] [#] [#] [#] [#] [#] [#] [#] [#]`. Why?
❯ Why no patchback?
❯ ### Re: [#] accumulation + no patchback — sonnet-4-6 (HEAD)

**Why `[#] [#] [#]...` (15 copies):**

The first 8 items in `tasks/tsift.md` were written with bare `[#]` placeholders — the agent wrote `- [ ] [#] Wire Sift...` directly into the pending component without going through `--pending-add`. Each preflight/commit cycle runs the ID-assignment pass, which finds the unresolved `[#]` and should replace it with a real `[#abc1]`. Instead, the parser is **prepending** a new `[#]` each cycle rather than replacing the existing one — so after 15 cycles you get 15 copies. That's the bug.

The later audit items (`#nkec`, `#f5ty`, `#jh84`, etc.) were added via `--pending-add` and got proper IDs — but they're duplicated 3x, meaning `--pending-add` was called 3 separate times with the same items (likely 3 `agent-doc notify` calls or 3 write cycles without dedup).

**Root cause (two separate bugs):**
1. **ID-assign prepend instead of replace** — when the pending parser finds `[#]` (empty ID slot), `assign_missing_ids()` appends a new placeholder before it instead of substituting in place. Fix: replace the `[#]` token, don't insert.
2. **Duplicate `--pending-add` calls** — no dedup guard on item text; same item added N times gets N entries with different IDs.

**Why no patchback last cycle:**

The previous `/agent-doc` cycle was interrupted by the WozCode hook. The hook redirected all file reads to `mcp__plugin_woz_code__Search`, which requires authentication. WozCode wasn't authenticated, so the tool call failed and the cycle never reached the write-back step. The preflight this cycle logged `previous cycle ended at preflight_diff_start without a write`.

**Bugs to file:**
- `[#]` accumulation on bare-placeholder pending items (assign-prepend bug)
- Duplicate items from repeated `--pending-add` with identical text (no dedup)
❯ Fix bugs
### Re: Fix bugs — opus-4-6

Both bugs fixed in `src/agent-doc/src/pending.rs`:

**Bug 1 — bare `[#]` placeholder accumulation:** `parse_item_line` now recognizes `[#]` (empty hash) as a placeholder and strips it from the text field. Previously it fell through to the invalid-hash fallback which kept `[#]` in the text, causing `- [ ] [#xxxx] [#] Wire Sift...` after backfill. Now produces clean `- [ ] [#xxxx] Wire Sift...`.

**Bug 2 — `--pending-add` dedup:** `op_add` now checks for existing items with identical text before appending. Bails with `"duplicate item text already exists"` — callers handle appropriately (`notify.rs` logs no-op, `main.rs` propagates).

3 tests added, 1119 total passing. `make check` clean.
<!-- agent:boundary:e002f987 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->
## Assistant
