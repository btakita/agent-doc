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
## Compacted exchange summary (2026-04-14)

### Shipped to main

- **Pending system Phase 1-3** — branch `20260414/pending-gated-state` ff-merged to main (`69b5475`). Three commits: Phase 1 data model (`PendingState::{Open, Gated, Done}`, `[/]` parse/render, `backfill_never_inserts_gated` invariant), sweep commit (Phase 2 gate/ungate matrix + `--pending-gate`/`--pending-ungate` CLI + supervisor scaffolding), Phase 3 (inverted `enforce_no_patch_pending` default — now rejects unconditionally unless `AGENT_DOC_ALLOW_PATCH_PENDING=1`, emits `pending_gated_count` in preflight JSON).
- **Supervisor Phase 1 progress** — `supervisor/cwd.rs` (priority chain, 11 tests) and `supervisor/pty.rs` (portable-pty alloc/spawn/IO, 6 tests, deterministic env invariant locked). `#a17b` done. Still TODO: `resize.rs`, `state.rs`, `ipc.rs`, `start.rs` wire-up.
- **`src/instruction-files/LIBRARY_CONTEXT_POLICY.md`** committed as canonical spec for library agent context. Library sweep deferred until the audit check drives it mechanically.

### Build + install state

- `cargo build --release` clean, `cargo install --path .` replaced `~/.cargo/bin/agent-doc`. Binary reports `agent-doc 0.32.3` — no version bump yet, this is a pre-release testing install on top of the shipped tag. **Not published.**
- Test baseline: 576 pass / 45 fail. 43 failures are pre-existing tmux env dependencies (`route::*`, `resync::*`). 2 non-tmux failures (`supervisor::cwd::document_parent_fallback_when_no_project_root`, `preflight::preflight_sweep_commits_other_tracked_docs`) are test-isolation issues (nested `agent-loop/.git` contamination); both present on pre-merge HEAD — **not regressions**.

### Document calibrated for live test

Three pending items transitioned `[ ] → [/]` (preflight confirms `pending_gated_count: 3`):

- **`#a002`** — safety rail landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- **`#gqep`** — release v0.32.4, awaiting manual-testing gate.
- **`#mchh`** — JetBrains plugin reinstall + session-picker verify, awaiting rebuilt zip.

### Key design decisions locked

- **`validate_transition` matrix** (`src/pending.rs`) is single source of truth for `Open/Gated/Done × Gate/Ungate/MarkDone`. Gate-on-Gated and MarkDone-on-Done are idempotent **no-ops**, not errors, so the skill can retry safely. Gate-on-Done is an error (no time-travel resurrection). Ungate requires a `[/]` source strictly.
- **Apply order in Write handler**: `clear → add → edit → gate → ungate → done → reorder`. Gate before done means `--pending-gate X --pending-done X` in one call lands `[x]` cleanly (`Gated → Done`, valid).
- **Why reject `patch:pending` by default** (answered): full-replace on a list the user concurrently edits enables silent-data-loss via (1) concurrent-edit clobber, (2) hash instability breaking reap invariant, (3) intent laundering (greppability loss). Granular flags preserve all three.
- **Rename `patch:pending` → `replace:pending`** approved and queued as `#25ag`. Migration: dual-accept one release with deprecation warning; `--allow-patch-pending` gains `--allow-replace-pending` alias; `AGENT_DOC_ALLOW_REPLACE_PENDING=1` becomes canonical. Lands **after** v0.32.4 ships.
- **pty.rs invariants**: caller supplies canonicalized cwd, env is complete (no parent inherit — enforces deterministic-env spec invariant), slave dropped immediately after spawn, I/O threads exit on EOF. Env passthrough policy for `start.rs` wire-up is a queued decision (option a: enumerate exact vars needed, or b: opt-in `inherit_parent_env` escape hatch).

### Open questions parked

1. **`op_done` + `validate_transition`** — should `op_done` use the matrix too (to error on `Done → Done` double-mark), or stay unconditional? Cheap to add, slightly less forgiving for retry loops.
2. **`--pending-done` on `[/]` edge case** — expected to transition `[/] → [x]` cleanly (completion overrides gating); behavior not yet verified. If it errors, queue behind `#a002`.
3. **v0.32.4 release sequencing** — hold for `#25ag` (rename) and ship together, or release pending enforcement standalone? Recommendation on record: hold for rename.
4. **skills-janitor install** — not yet run; requires interactive `/plugin` command at the terminal. Fork + `src/skills-janitor/` submodule plan captured for post-install.

### Branch / workspace state

- `main` at `69b5475` (three commits ahead of origin).
- `20260414/pending-gated-state` redundant, still points at HEAD (safe to delete after origin push).
- Submodule `src/agent-doc` has working-tree clean; `.bin/agent-doc` symlink points at the fresh release binary.

❯ reorder agent:pending to have the tasks related to pending moved to the top
### Re: Pending reordered — pending-system items at top (HEAD)

Ten items moved to the head of the list via `--pending-reorder`, ordered from "active next" to "shipped tracker":

1. `#25ag` — Rename `patch:pending` → `replace:pending` (next queued task)
2. `#gqep` — Release v0.32.4 (gated, awaiting manual-testing gate)
3. `#e6z1` — Version bump + VERSIONS.md entry for gated-state (release gate)
4. `#14z4` — Hash collision ceiling: 8-char fallback in `generate_hash`
5. `#bak5` — Optional: pending-done archive component (spec §3 step 3, deferred)
6. `#pf01` — ✅ Spec landed: `specs/pending-system.md`
7. `#mgdw` — ✅ Parser `[/]` (Phase 1 shipped)
8. `#h1j2` — ✅ Reaper skip + `pending_gated_count` (Phase 3 shipped)
9. `#q90h` — ✅ CLI `--pending-gate`/`--pending-ungate` (Phase 2 shipped)
10. `#sx35` — ✅ Tests: parser round-trip + state transitions + CLI integration

All other items retain their original relative order beneath this group — `--pending-reorder` only moves the listed ids; unlisted ids fall through in place.

**Reap question:** `#pf01`, `#mgdw`, `#h1j2`, `#q90h`, `#sx35` are all shipped work now tracked only as history. If you want a cleaner list, I can mark them `--pending-done` next cycle and preflight will reap them. Keeping them for now since you asked for reorder, not reap — but say the word and they're gone. `#14z4` (hash collision ceiling) and `#bak5` (archive component) are genuinely future work and should stay.

**`pending_reordered` guard:** the spec says not to reorder again for at least one cycle after the user does. I just reordered per your explicit instruction — next cycle will respect `pending_reordered: true` if preflight reports it and leave the order alone unless you re-ask.
<!-- agent:boundary:81026d77 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#25ag] Rename patch:pending → replace:pending. Parser accepts both for one release with deprecation warning; --allow-patch-pending gains --allow-replace-pending alias; AGENT_DOC_ALLOW_REPLACE_PENDING=1 becomes canonical env var; specs/pending-system.md + SKILL.md updated to new name; drop old name after one release. Queue behind current phase-3 work.
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release after manual-testing gate
- [ ] [#e6z1] agent-doc: version bump + VERSIONS.md entry for gated-state feature after implementation lands (release gate — do not ship with the #eg0w bundle)
- [ ] [#14z4] Hash collision ceiling: implement 8-char fallback in pending::generate_hash if 4-char collides (spec §1)
- [ ] [#bak5] Optional: pending-done archive component for history trail (spec §3 step 3, deferred)
- [ ] [#pf01] ✅ Spec landed on 20260414/pending-gated-state: src/agent-doc/specs/pending-system.md — PendingState::Gated, [/] syntax, reaper rules, lifecycle diagram, CLI surface, state transition matrix, release discipline
- [ ] [#mgdw] agent-doc: parser — accept `[/]` checkbox char, extend PendingState enum with Gated variant in src/agent-doc/src/pending.rs; renderer emits `[/]` round-trip
- [ ] [#h1j2] agent-doc: reaper — skip Gated items in preflight reap pass; emit pending_gated_count alongside existing counts in preflight JSON output
- [ ] [#q90h] agent-doc: CLI — add `--pending-gate <id>` flag on `agent-doc write` (mirror of --pending-done); `--pending-ungate <id>` to return to `[ ]`
- [ ] [#sx35] agent-doc: tests — parser round-trip for `[/]`, state transitions ( →/→x,  →x, /→x, /→ ), reaper respects Gated, CLI flag integration
- [ ] [#64mb] Investigate double commit_staging race — root cause: concurrent preflights racing step 2 vs step 2d sweep; mid-flight (HEAD)-strip rewrites working tree between them
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [ ] [#xzz5] Preflight slash-command parser: absolute paths like /home/... get captured as commands. Require [a-z][a-z0-9:_-]* after the slash and reject tokens containing a second /.
- [ ] [#a011] End-of-cycle write invariant — warn if `preflight_diff_start` without `ipc_write_attempt`.
- [ ] [#a012] Expose `agent-doc session-check <FILE>` — nonzero if last ops.log entry is `preflight_diff_start`.
- [ ] [#a014] Session drift: `registered panes span 2 tmux sessions: 1, 4` — resync if recurring.
- [ ] [#a010] PostToolUse hook for agent-doc write (layer 3, optional).
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
<!-- /agent:pending -->
## Assistant
