# Plan — cross-project stash-pane condition (`registered in its own project root`)

## Symptom

JB `Run Agent Doc` on a superproject doc (`tasks/software/tsift.md`) logs:

```
resync: stash pane %32 is registered in its own project root — skipping kill
```

The user's position: the `skipping kill` message is benign, but **the condition that makes
resync encounter a foreign-root pane at all is the bug** — "Why did the condition surface in
the first place?"

## Live root-cause evidence (2026-05-30)

`tmux list-panes -a` for session `0`:

```
0 @4 agent-doc           (active window)
0 @3 stash               (single stash window)
   %34 %31 %20 %33 %4 %36 %3 %35 %21  agent-doc  /home/brian/work/btakita/agent-loop          (superproject root)
   %32                               agent-doc  /home/brian/work/btakita/agent-loop/src/boost-client  (submodule root)
```

`%32` is in the **same tmux session (`0`) and the same stash window (`@3`)** as the
superproject panes — it is *not* in a separate session. So the server-wide `list-windows -a` /
`list-panes -a` scan in `resync.rs` (`fetch_all_window_metadata` ~1117, `fetch_all_pane_metadata`
~1149) is **not** the cause here; the foreign pane genuinely lives in the current session's stash.

### Why the condition exists

1. agent-doc resolves the target tmux session by **auto-detecting the current session**
   (`resolve_target_session` → current session `0`) when a project root has not pinned a session
   via `session set`. Both the **superproject** (`agent-loop`) and the **nested submodule**
   (`src/boost-client`) resolve to the *same* session `0`.
2. They therefore share **one** tmux session and **one** `stash` window. When the superproject
   syncs/arranges its layout (`sync.rs`), every pane that is not part of the superproject's
   current layout — including the live `boost-client` session pane `%32` — is moved into
   `0:stash`.
3. The superproject's resync stash sweep
   (`purge_unregistered_stash_panes_bulk_with_supervisors_in_mode`, `resync.rs:1303`) then
   inspects `%32`: it is absent from the superproject registry but present in the
   `boost-client` registry (`current_root != pane_root`), so the cross-project guard preserves
   it and logs `registered in its own project root — skipping kill`.

So the message is the **correct safety net** for a deeper layout condition: **nested project
roots collapse onto one shared tmux session + one stash window**, so the superproject's sync
keeps stashing (and re-discovering) the submodule's live session pane.

### Contributing factor — stash accumulation

Nine superproject `agent-doc` panes are parked in `0:stash`. The stash sweep "never kills
agent processes (`agent-doc`, `claude`, `node`) even if unregistered" (`resync.rs:752`, `1277`),
so stale/orphaned agent-doc panes accumulate in the shared stash, amplifying how often the
cross-project condition is hit.

## Fix options

- **A. Nested-root session isolation (root cause).** Give a nested submodule project root its
  own tmux session (or its own stash window) rather than collapsing it onto the parent's
  auto-detected session. The superproject's sync/resync would then never stash or inspect a
  submodule's live session pane. Largest change; touches `start.rs`, `route.rs`
  (`resolve_target_session`), `sync.rs`, and session config; needs a migration story for
  already-shared sessions.
- **B. Per-root stash partitioning.** Keep one tmux session but route stashed panes into a
  root-scoped stash window (e.g. `stash` for the current root, foreign-root panes left in their
  own root's stash and never pulled into the current root's sweep). Medium change in
  `sync.rs` + `resync.rs`; preserves single-session UX.
- **C. Out-of-scope skip, not "skip kill" (scoped reduction of the symptom).** When resync's
  current root differs from a stash pane's proven root, treat the pane as **out of scope** and
  `continue` *before* the kill-evaluation/log, instead of evaluating it and emitting a
  `skipping kill` line. This removes the misleading log and the unnecessary cross-project
  evaluation without changing which panes get reaped. Smallest change, but it is closer to the
  UX suppression the user already pushed back on — only worth doing alongside A or B.
- **D. Reap stale unregistered agent-doc stash panes after a proven-dead/TTL check.** Address
  the accumulation contributing factor so the shared stash does not grow unbounded. Independent
  of A–C; pairs with `gc.rs`.

## Recommendation (superseded — see Implemented)

Original recommendation was **A + D**. After the user weighed the UX cost of separate sessions
(`prefix+s` session switching, IDE focus following sessions), the choice moved to **B + C + D**,
then to **C** after reading the sweep code.

## Implemented (2026-05-30)

Reading the full stash sweep changed the picture — the only correct, non-redundant change is **C**:

- **A — not implemented.** Per-root sessions fragment the single-session workflow (you'd switch
  sessions to reach a submodule's pane, and IDE focus would have to follow). The user moved off it.
- **B — not implemented (unnecessary / mildly harmful).** Hiding a non-focused foreign-root pane
  in the shared stash is *correct* layout behavior. Not stashing it (or a separate stash window)
  would either clutter the focused view or add cross-root rescue complexity for no visible benefit
  — the user never sees stash windows.
- **D — already implemented.** `resync.rs` step 8 (`purge_unregistered_stash_panes_bulk_*`,
  ~line 1352) already reaps truly-orphaned agent panes (unregistered, no owned file, no live
  supervisor, not cross-project) in non-passive mode. The nine `agent-loop` `agent-doc` panes
  observed in `0:stash` survive because they are *legitimate* live/registered sessions, not
  orphans — there is nothing safe to reap. An optional **TTL-prune of long-idle stash sessions**
  remains a possible future opt-in, but it is destructive and needs live verification, so it is
  not auto-enabled.
- **C — implemented.** The benign cross-project preservation event
  (`registered in its own project root — skipping kill`) was routed from `eprintln!` (stderr,
  which the JB plugin renders as an error) to `ops_log` telemetry at all three sweep sites
  (windowed / bulk / non-stash, `resync.rs`). The skip behavior is unchanged; only genuine
  failures (e.g. failed `kill-pane`) remain on stderr. Spec: `specs/08-session-routing.md`
  multi-root truth-table note. This aligns with the existing principle that diagnostics belong
  in logs, not the stderr stream that pollutes prompt detection / the IDE error surface.

The residual IDE-side concern — the plugin rendering *any* route/resync stderr as an error — is
tracked under `#jb-busy-queue-ux-feedback` (classify benign info vs. real failures).

## Verification (when implemented)

- Deterministic SimWorld/resync regression: a superproject sync with a nested-submodule live
  session pane present must not stash/inspect/log the submodule pane under the chosen model.
- `make tmux-ci` for the live-layout leg.
- Update `specs/07-session-tmux-commands.md` and the session/tmux command spec for the chosen
  nested-root session/stash model.
