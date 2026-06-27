# Plan: `Sync Tmux Layout` crashes the tmux server (`#tmuxsynccrash`)

**Status:** root-caused from logs, not yet fixed.
**Severity:** high — a single operator action destroys the entire tmux server (all sessions/panes), then silently respawns a fresh one.
**Repro (operator):** run JB `Sync Tmux Layout` (Ctrl+Shift+Alt+L) twice in quick succession with ≥2 `.md` editor splits open → tmux server dies both times.

## Evidence

- **tmux version:** 3.7.
- **Server respawn proof:** current `tmux` server `pid=1517998` has `STARTED Fri Jun 26 23:54:47 2026`, launched by `tmux new-session -d -s 0 -c .../agent-loop -P -F #{pane_id}` — i.e. the prior server died and `route::auto_start` recreated it. The operator perceives this respawn as "tmux crashed."
- **Repair storm (idea.log, `SyncLayoutAction`), 23:54:42 → 23:55:29:** a tight burst of full-mode `agent-doc sync` runs, several tagged `[sync] re-running after supersede`, one `[sync] exit=1`, each emitting `doctor repair: Repaired agent-doc/stash layout` or `[repair] layout needs repair: target=… stash_count=…`.
- **Non-convergence:** the repair messages alternate `target=true stash_count=0` ↔ `target=false stash_count=1`. The layout never reaches `repair_layout`'s fast-path skip condition, so destructive ops run on *every* sync.
- **ops.log:** the full-mode `tmux_router` phase logged `status=ok` for the surviving run — agent-doc does not observe the crash; it issues a tmux command, tmux dies, the command returns, and autostart respawns a fresh server.

## Mechanism

`SyncLayoutAction.syncLayout` (editors/jetbrains/.../SyncLayoutAction.kt) runs `agent-doc sync --col … --col … --focus …` (full mode, 2 columns). Full sync runs the doctor-backed `repair_layout` (agent-doc-orchestration/src/sync.rs:849).

`repair_layout` fast-path skip (sync.rs:902-906):
```
let skip_phase_1_2 = has_target && stash_count == 1 && has_exact_stash;
```
When `stash_count == 0` (the observed steady state for a clean 1-pane agent-doc window), the skip is NOT taken, so Phase 1 *creates* a stash window (`ensure_stash_window`), then index-normalization runs `move-window` / `swap-window` (sync.rs:1705/1720) and stash consolidation runs `resize-window -t … -y 1000` (sync.rs:971) + `join-pane -dv` + `kill-window`. The next sync then sees `stash_count == 1` but a different shape and repairs again — the two states ping-pong and never settle into `stash_count == 1 && has_exact_stash && has_target`.

Two amplifiers turn that into a crash:
1. **JB supersede re-run loop** — `shouldRerunAfterSupersede(heldGuard, !generationStillCurrent)` re-invokes `syncLayout` (full mode) when any newer sync bumped the generation mid-run. Automatic editor-tab sync bumps the generation, so a manual sync keeps re-running the *destructive* repair instead of a cheap focus-only pass.
2. **Automatic tab sync** also issues full `agent-doc sync` on selection/layout changes, interleaving with the manual ones.

Result: rapid `swap-window` / `move-window` / `join-pane` / `resize-window -y 1000` churn against tmux 3.7 → server crash. tmux pane/layout mutation under rapid succession is a known segfault surface.

## Fix design (defensive; we cannot patch tmux, only stop hammering it)

Land independently, smallest-risk first. All are packagable product fixes (binary + skill surfaces), not per-user memory.

1. **Make `repair_layout` converge (binary, primary).** The skip condition must hold immediately after a successful repair. Treat `stash_count == 0` with a correct target window as already-correct (do not force-create a stash window when there are no excess panes to stash). Add a regression: two consecutive `repair_layout` calls on a clean `0:agent-doc` + zero-stash layout — the second must be a pure no-op (no `move-window`/`swap-window`/`join-pane`/`resize-window`). This alone removes the per-sync destructive churn.

2. **Supersede re-run must be non-destructive (JB + binary).** The `shouldRerunAfterSupersede` re-run should dispatch a focus/passive pass (`--no-autostart`, no doctor repair), not a second full destructive repair. The destructive repair should run at most once per user gesture.

3. **Rate-limit the destructive doctor repair (binary).** Debounce `repair_file_state_with_tmux`'s destructive phase per session (e.g. skip if a successful repair ran < N ms ago and the window inventory is unchanged), so an auto+manual+supersede burst collapses to one repair.

4. **(Optional) batch tmux mutations / add a tiny settle between `swap-window`+`join-pane` bursts** to avoid the 3.7 crash window even when a repair is genuinely needed.

## Verification

- `make check` in `src/agent-doc` (+ `src/tmux-router` if `tmux.rs` touched; keep the workspace cargo patch aligned).
- New deterministic SimWorld / tmux-router regression for repair convergence (item 1) and the supersede-re-run-is-passive rule (item 2).
- `make tmux-ci` for the live-tmux repair leg.
- Live operator proof (`[operator-verify]`): run `Sync Tmux Layout` repeatedly with 2 splits and confirm the tmux server pid is stable (no respawn).
