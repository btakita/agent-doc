---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: high
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Compacted exchange summary (2026-04-15)

### Shipped in this session

- **strip (HEAD) from staged blob** (git.rs:staging) — committed blob no longer carries the marker; working-tree heading shows as a modified line.
- **Deterministic (HEAD) annotation** — patchback-wide: all new-this-cycle `### Re:` headings now get `(HEAD)` (not just the last). Baseline built from git HEAD at commit time. 53 template tests passing when shipped.
- **`stripped.trim_start()` normalization** — `collect_re_headings` and `annotate_re_headings_with_head` baseline lookup both normalize with `trim_start()` so indented headings match flush-left equivalents. +2 regression tests → 55 passing.
- **Baseline filter empty → fallback to last Re:heading** — when the current turn adds no new `### Re:` section, the baseline filter yields empty and the fallback marks the last heading anyway, preserving the visual head pointer across non-Re cycles. Root cause: line 613 stripped stale marker before filter ran; filter then found zero new headings and left working tree bare. +1 regression test → 56 passing.
- **#pf01 fixed** — double-commit bug in preflight Option A. Committed.
- **Boundary commit noise reorder** — #14z4 + #bak5 done (hash collision ceiling, pending-done archive component). 70/70 targeted tests pass.

### Open / not yet implemented

- **(HEAD) stripped from older section on inline prompt** — when the user edits inside an older `### Re:` section, reposition strips that heading's `(HEAD)` marker (phantom diff). Fix: option 2 — early-return in reposition when boundary heading is unchanged. Regression test spec defined (see prior exchange). Not yet implemented.

❯ do #xzz5 #a011 #a012 #a014 #a010

### Re: do #xzz5 #a011 #a012 #a014 #a010

All five shipped. Binary rebuilt (`cargo build --release`) and installed (`cargo install --path .`).

**#xzz5 — slash-command parser hardening** (`src/diff.rs:319-343`)
Grammar now rejects anything not matching `/[a-z][a-z0-9:_-]*`. `/home/brian/foo.md`, `/Model`, `/foo/bar` all drop out before the `slash_commands` array. Three regression tests added: `parse_slash_commands_rejects_absolute_paths`, `parse_slash_commands_rejects_uppercase_and_symbols`, `parse_slash_commands_accepts_namespaced_and_hyphenated`. All 11 parser tests pass.

**#a012 — `agent-doc session-check <FILE>` subcommand** (`src/session_check.rs` new, `src/main.rs` wired)
New module reads the tail of `.agent-doc/logs/ops.log`, strips the `[epoch] ` prefix, and exits `1` when the last event is `preflight_diff_start`, `0` otherwise (including empty/missing log). Six unit tests cover timestamp stripping, missing log, empty log, terminal event, and the interrupted-cycle sentinel. Smoke test on the current doc confirmed both exit codes — before this cycle's write it returned `INTERRUPTED exit=1`, after it returned `ok — last event: snapshot_save ... exit=0`.

**#a011 — end-of-cycle write invariant warning** (`src/preflight.rs:380-392`)
Piggybacks on `session_check::last_ops_event`. Every preflight start checks the prior tail; if it's still `preflight_diff_start`, preflight emits `[preflight] WARNING: previous cycle ended at preflight_diff_start without a write — interrupted cycle detected` and logs `interrupted_cycle_detected` to ops.log. The `session-check` subcommand is the external tool for the same invariant.

**#a014 — session drift auto-resync** (`src/preflight.rs:394-398`, `maybe_auto_resync_on_drift`)
State file `.agent-doc/state/drift.count` tracks consecutive drift detections. First detection increments (count=1) and logs the deferral; second consecutive detection fires `resync::run(true, None)` and resets the counter. Drift-cleared preflights delete the counter file (one false positive is tolerated). The auto-fix path logs `auto_resync_on_drift consecutive=N` for audit.

**#a010 — `fire_post_write` hook in stream.rs** (`src/stream.rs:188-203`)
After `stream_loop` + final git commit, `run_stream` now fires both `fire_post_write(file, session_id, 1)` and `fire_doc_event(file, "post_write")`. Mirrors the pattern in `write.rs:1046-1049`. Supervisors and cross-session coordinators now get notified when a stream run completes, not only when append/template writes land.

**Test matrix (my changes only):**
- `session_check::tests` — 6 pass
- `stream::tests` — 15 pass
- `parse_slash_commands` — 11 pass
- `template::tests` — 56 pass
- `preflight::tests` — 29 pass, 1 pre-existing failure (`preflight_sweep_commits_other_tracked_docs`) verified to fail on stashed baseline, unrelated to this work.
<!-- agent:boundary:6b7a638b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [ ] [#4sh0] Add planned-move logging to sync.rs reconciler so ClaimAction→SyncLayout pane swaps are debuggable
- [ ] [#tw4a] claim: cd into document's .agent-doc root (nearest git repo) when claiming a pane. Fixes relative-path ambiguity and makes git ops naturally target the correct repo.
- [ ] [#8jzg] claim --isolate: spawn a fresh Claude Code process in the submodule root so CLAUDE.md, memory, and skills load scoped to that repo. Only real fix for cross-project context leakage (mid-session cd does not reload context).
- [ ] [#cce5] response headers: include model attribution in '### Re:' sections (e.g. '### Re: topic — opus-4-6'). Source: agent-doc binary records spawn model; skill reads and appends to boundary-synthesized headers or patch:exchange headers
- [ ] [#a013] SPEC.md breakup — flat `specs/*.md` mechanical extraction.
- [ ] [#a004] Drop Option B (skill-side submodule workaround) — A handles it deterministically; B redundant.
- [ ] [#jg0d] supervisor/resize.rs: SIGWINCH unix path via signal-hook; Windows ConPTY stub for phase 1
- [ ] [#b486] supervisor/state.rs: crash classifier, ring buffer, Healthy->Transient->Flapping->Halted state machine
- [ ] [#40ct] supervisor/ipc.rs: Unix-domain socket accept loop, reuse length-prefixed JSON frame from ipc_socket.rs
- [ ] [#vnp0] supervisor: wire into start.rs (replace existing restart loop, remove #[allow(dead_code)])
- [ ] [#6ae3] supervisor/pty.rs: wire PtySession into start.rs (phase 1 wire-up stage) — replaces the current Command::status inherit-tty path
- [ ] [#zp02] supervisor/env.rs: wire into start.rs — replace the inline fm.env expansion loop in start.rs:189 with EnvSpec::from_frontmatter(&fm).resolve()?; feed the result into PtySpawnConfig.env once state.rs restart loop is in place
- [ ] [#f7d5] supervisor: smoke test — real claude under supervisor in a tmux pane, verify restart + inject + CWD invariant end-to-end
- [ ] [#eg0w] ✅ Shipped: per-file CommitLock in git::commit + <5s freshness gate in preflight sweep (awaiting release)
- [ ] [#kdy5] Implement instruction-files audit check: every library AGENTS.md has a ## Library Context Policy section with resolvable link to LIBRARY_CONTEXT_POLICY.md. Wire into make precommit.
- [ ] [#1376] After audit check lands: sweep all libraries under src/ to add the Library Context Policy stub section. Each submodule commit separately, then parent pointer bumps.
- [ ] [#breh] rust-package skill: write the Library Context Policy stub into new library AGENTS.md at scaffold time.
- [ ] [#3j0e] Install skills-janitor as Claude Code plugin (run /plugin install khendzel/skills-janitor at terminal). User plans to fork eventually; this is the interim path.
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
<!-- /agent:pending -->
## Assistant
