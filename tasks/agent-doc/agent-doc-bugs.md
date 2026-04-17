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
v0.32.5 session. Key fixes: supervisor IPC stack wired into start.rs (#vnp0/#6ae3/#zp02), DSR interleaving fix (prompt-poll instead of fixed sleep), supervisor smoke test (#f7d5), sun_path overflow for deep submodule paths (XDG_RUNTIME_DIR fallback when >107 bytes, 13 tests), tmux install detection (#t8tg), TerminalBackend trait spec (#1yk1), DirectTerminal plan (#zy7m), Library Context Policy stub in rust-package skill (#breh). Decisions: [/] vs [x] semantics (gate after impl, done on confirm); typed gates [/release]/[/deploy]; replace:* restricted to pending; --status flag mirrors --pending-add; always run cargo install --path . after builds without asking. State: 1055 tests, 7 gated pending items.
❯ Look at #vdyj. I also installed wozcode: https://app.wozcode.com/
❯ How can I synthesize skills janitor and wozcode to save on tokens?
### Re: #vdyj + WozCode synergy — opus-4-6

**#vdyj status:** Currently `[ ]` (unstarted). The plan is: fork skills-janitor → `src/skills-janitor` submodule → wire `/janitor-report` into `make precommit` → optionally add nightly cron via `/schedule`. This gives you drift detection as part of the normal commit flow.

**Synthesizing skills-janitor + wozcode for token savings:**

The two tools complement each other at different layers:

1. **Reduce skill context bloat (janitor):** Every installed skill adds its description to the system prompt. Run `/janitor-usage` to find skills you never invoke, `/janitor-duplicates` to find overlapping ones, then `/janitor-cleanup` to remove dead weight. Fewer skill descriptions = smaller system prompt = fewer tokens per turn.

2. **Use woz agents for cheaper execution (wozcode):** Once skills are clean, use `woz:explore` instead of general-purpose agents for read-only research (it's faster and cheaper). Use `woz:code` for editing tasks that benefit from batch edits and smart search. The `mcp__plugin_woz_code__Search` tool replaces multi-round Grep/Glob cycles with a single semantic call.

3. **Concrete workflow for #vdyj:**
   - Fork + submodule skills-janitor (the planned work)
   - Add `janitor-report` to `make precommit` — catches skill drift before it accumulates
   - Use `/janitor-usage` periodically to prune unused skills (immediate token savings every session)
   - Delegate janitor audit operations to `woz:explore` subagents instead of full agents (cheaper per-audit)

4. **Quick win right now:** Run `/janitor-usage` to see which of your ~40+ skills actually get used. Pruning the unused ones is the single highest-leverage token reduction — each removed skill saves its description tokens on every turn of every session.

Want me to run `/janitor-usage` now to identify pruning candidates? Or should I start on the #vdyj submodule setup?
<!-- agent:boundary:eff47234 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#p2xm] Orphaned patch:exchange marker leak: parse_patches() does not consume unclosed opening markers — they leak into unmatched text and get appended to exchange verbatim. Fix: advance last_end past unclosed openers in parse_patches()
- [/] [#wcf5] Advisory flock doesn't protect external watchers: IDE file-change listeners and git hooks race concurrent diff computations against stale snapshots
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [ ] [#vdyj] After fork: move forked skills-janitor under src/ as submodule, wire audit into make precommit, and optionally add nightly cron via schedule skill writing drift reports to tasks/agent-doc/skills-audit.md.
- [ ] [#9v7h] agent-doc: --pending-add should reject text starting with '[/]' / '[x]' / '[ ]' (ambiguous state marker). Add --pending-add-gated flag and emit assigned hash to stdout so skill can compose add+gate atomically without text-embedding.
- [ ] [#txex] notify: add --pending-add flag — auto-create agent:pending on target when absent, --no-create-pending to opt out. Pending-only (no exchange blockquote). Errors become no-ops under auto-create.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#hrmt] Rewrite src/agent-doc/SKILL.md (bundled skill) to invert Phase 2 pending guidance: remove 'every response MUST include a patch:pending block', replace with granular-ops contract from .claude/skills/agent-doc/SKILL.md §1b. RELEASE GATE for #gqep — v0.32.4 cannot ship with inverted bundled-skill guidance.
- [ ] [#e46w] Sweep-commit path: working-tree boundary lags until next write or editor re-open. Fix: have reposition_boundary_in_snapshot also rewrite the working-tree file when no active run is in progress (git.rs:593). Alternative: emit IPC reposition_boundary notification to open plugins.
- [ ] [#f8j3] Sweep-commit working-tree lag: reposition_boundary_in_snapshot (git.rs:593) only updates snapshot; working tree boundary only moves via IPC from agent-doc write. Fix: also rewrite working tree in snapshot reposition path, or emit IPC ping to open plugins.
- [ ] [#y4da] Deterministic boundary reposition in working tree (git.rs reposition_boundary_in_snapshot extends to disk write with active-run guard)
- [ ] [#zvmd] Submodule (HEAD) marker: fix show_head() to route via narrow_to_submodule — superproject rev-parse can't show submodule-tracked files, so add_head_marker falls through to the 'no HEAD → mark last heading only' fallback, which drops (HEAD) when the exchange component has no ### Re: heading yet or when the fallback runs during a boundary-only cycle
- [ ] [#np8p] Submodule pane cwd: agent-doc start/route spawns tmux panes with std::env::current_dir() instead of the nearest git toplevel for the target document — add a resolve_pane_cwd(file) helper that returns narrow_to_submodule(super_root, resolved).0 so submodule docs get a pwd inside their own submodule
- [ ] [#0xka] #hdap: reposition_boundary_to_end appends (HEAD) to last ### Re: heading in exchange, strips stale (HEAD) from earlier headings — symmetric counterpart to strip_head_markers on commit staging side
- [ ] [#77mf] Event-driven gate resolution: typed gate markers like [/release] and [/deploy] that auto-resolve on matching events (cargo publish, git push). Extends the binary [/] invariant with machine-actionable gate types — reaper checks event log before skipping.
- [/] [#6hgf] Add --status flag to agent-doc write: skill passes status text as CLI arg, binary handles component targeting with replace semantics
<!-- /agent:pending -->
## Assistant
