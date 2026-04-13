---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Supervisor Phase 1 session (2026-04-13)

Dogfooded Phase 1 kickoff. Outcomes captured in `src/agent-doc/specs/supervisor.md`:

- **`supervisor/cwd.rs` landed** — priority chain (CLI flag > frontmatter > .agent-doc walk > doc parent), 11 unit tests passing, clippy clean. Added `agent_doc_cwd` field to `Frontmatter`. Module registered in `main.rs` with `#[allow(dead_code)]` until `start.rs` consumes it.
- **Design invariants pinned in spec** (§ Core Invariants / CWD determinism): relative-base differs by source (CLI → process CWD, frontmatter → doc parent); misconfigured paths hard-error instead of silently falling through; `find_project_root` duplicated to keep the supervisor module self-contained; document existence is a precondition; `CwdSource::as_str()` tags locked by test.
- **Non-tmux Mode (Future) section added** to `supervisor.md` — four requirements (entry gate, resize source, IPC discoverability, claim/binding model) plus non-goals (visual multiplexing stays tmux's job). Explicitly out of Phase 1.
- **Phase 1 Implementation Status table added** to `supervisor.md` — tracks cwd (landed) + pty/resize/state/ipc/start.rs wire-up (not started), in the order subsequent PRs will land.
- **Question answered:** supervisor works outside tmux architecturally (owns its own pty), but every current consumer — livestream setup, `/agent-doc` cross-pane routing, claim system, focus — assumes tmux. Dropping tmux is a separate spec cycle.

**Next submodule:** `supervisor/pty.rs` — awaiting user go-ahead to proceed.
### Re: Context written to specs + pending, exchange compacted (HEAD)

**Spec updates (`src/agent-doc/specs/supervisor.md`):**

1. **Phase 1 Implementation Status table** added after Motivation — tracks cwd (landed) vs pty/resize/state/ipc/start.rs wire-up (not started) in the sequence subsequent PRs will land. Notes the `#![allow(dead_code)]` gate in `supervisor/mod.rs` stays until `start.rs` consumes the module.
2. **CWD determinism invariants** — added an "Implementation invariants" block under Core Invariants pinning the four non-obvious decisions from this session: relative-path bases differ by source, misconfigured paths hard-error, `find_project_root` duplicated for module self-containment, `CwdSource::as_str()` tags locked.
3. **Non-tmux Mode (Future) section** (from earlier this cycle) — four requirements + non-goals + explicit "not in phase 1" scope gate, so the idea survives future refactors.

**Pending items added** for the remaining phase 1 submodules, each with enough context to pick up cold: `pty.rs`, `resize.rs`, `state.rs`, `ipc.rs`, `start.rs` wire-up, plus the manual smoke test from the release checklist.

**Exchange compacted** via `agent-doc compact` with a summary-message pointer. 1693 lines archived to `.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260413-195807.md` — nothing lost, just moved off the hot path.

Ready to proceed with `supervisor/pty.rs` whenever you give the go-ahead.
<!-- agent:boundary:6cbe5f80 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#a001] 🔴 Investigate boundary/baseline divergence root cause — `out_of_band_write` always-on log line landed (`git.rs:286`); aggregate after 1 day.
- [ ] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [ ] [#a004] Drop Option B (skill-side submodule workaround) — A handles it deterministically; B redundant.
- [ ] [#a005] Investigate 1062 pre-existing `❯` lines across 46 snapshots — legitimate or prior corruption?
- [ ] [#a006] Investigate duplicated `/agent:exchange` closing marker — likely same class as mass-prefix bug.
- [ ] [#a010] PostToolUse hook for agent-doc write (layer 3, optional).
- [ ] [#a011] End-of-cycle write invariant — warn if `preflight_diff_start` without `ipc_write_attempt`.
- [ ] [#a012] Expose `agent-doc session-check <FILE>` — nonzero if last ops.log entry is `preflight_diff_start`.
- [ ] [#a013] SPEC.md breakup — flat `specs/*.md` mechanical extraction.
- [ ] [#a014] Session drift: `registered panes span 2 tmux sessions: 1, 4` — resync if recurring.
- [ ] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release after manual-testing gate
- [ ] [#14z4] Hash collision ceiling: implement 8-char fallback in pending::generate_hash if 4-char collides (spec §1)
- [ ] [#bak5] Optional: pending-done archive component for history trail (spec §3 step 3, deferred)
- [ ] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [ ] [#4sh0] Add planned-move logging to sync.rs reconciler so ClaimAction→SyncLayout pane swaps are debuggable
- [ ] [#xzz5] Preflight slash-command parser: absolute paths like /home/... get captured as commands. Require [a-z][a-z0-9:_-]* after the slash and reject tokens containing a second /.
- [ ] [#64mb] Investigate double commit_staging race: ops.log at 1776095605 shows two back-to-back commit_staging/out_of_band_write pairs for the same file within the same second.
- [ ] [#tw4a] claim: cd into document's .agent-doc root (nearest git repo) when claiming a pane. Fixes relative-path ambiguity and makes git ops naturally target the correct repo.
- [ ] [#8jzg] claim --isolate: spawn a fresh Claude Code process in the submodule root so CLAUDE.md, memory, and skills load scoped to that repo. Only real fix for cross-project context leakage (mid-session cd does not reload context).
- [ ] [#a17b] supervisor/pty.rs: portable-pty allocation, stdin/pty forwarding threads, fake-claude shell-script integration test
- [ ] [#jg0d] supervisor/resize.rs: SIGWINCH unix path via signal-hook; Windows ConPTY stub for phase 1
- [ ] [#b486] supervisor/state.rs: crash classifier, ring buffer, Healthy->Transient->Flapping->Halted state machine
- [ ] [#40ct] supervisor/ipc.rs: Unix-domain socket accept loop, reuse length-prefixed JSON frame from ipc_socket.rs
- [ ] [#vnp0] supervisor: wire into start.rs (replace existing restart loop, remove #[allow(dead_code)])
- [ ] [#f7d5] supervisor: smoke test — real claude under supervisor in a tmux pane, verify restart + inject + CWD invariant end-to-end
<!-- /agent:pending -->
## Assistant
