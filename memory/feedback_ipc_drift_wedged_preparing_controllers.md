---
name: feedback_ipc_drift_wedged_preparing_controllers
description: Wedged --handoff-state preparing controller drift is root-fixed (M1b self-reap); trust the self-heal, don't manually pkill/reset
metadata:
  node_type: memory
  type: feedback
  originSessionId: 2c3efeec-2218-4806-8f50-b9b0a6604404
---

The recurring `live_prompt_drift_after_preflight` + `postcommit_worktree_check match=false` drift in a managed `/agent-doc` session traces to a `controller serve … --handoff-state preparing` proc **wedged mid-handoff** racing the IDE over `.agent-doc/ipc.sock`. The root cause is **fixed in the binary** (`#stuckhandoff2` M1b, agent-doc `e54bb2b7`, 2026-06-15): `promote_handoff` flips a replacement straight to `Stable` (clearing `handoff_started_at`) before the client renames its temp socket onto the public path, so a client that dies in that window left a `Stable`-in-memory orphan stranded on its `controller-handoff-*` temp socket — invisible to M1's `Preparing`/`Promoted`-only watchdog, reaped only by the slow gc/M5 cmdline sweep at 7–21 min. M1b's `controller_handoff_replacement_is_stranded` keys off the temp socket structurally (a temp socket still present past the 45s threshold = the promote rename never completed) so the orphan **self-reaps within 45s regardless of in-memory state**, and `lib-install` now auto-recycles running controllers onto a fresh binary (`#autorecycle-on-install`).

**Why this memory was rewritten:** the prior version had accreted contradictory CODE-COMPLETE/CORRECTION layers and a long manual recovery ladder (`pkill`, `kill <pid>`, `git checkout HEAD`, quarantine sidecars) justified by "M1 self-watchdog is dead in production." That justification is now false and obscured the true self-healing behavior — it trained the manual orphan-kill dance instead of trusting (or fixing) the binary. agent-doc's own dev rule: behavioral fixes are packagable, not per-user memory.

**How to apply:** trust the self-heal. After installing a fresh binary, let controllers recycle at their next idle boundary (lib-install marks them; a long-running controller keeps the deleted-inode old binary only until that boundary). Do NOT script manual `pkill`/`reset --from-current` recovery as the default — if a wedge survives well past 45s on a CURRENT binary, that is a NEW regression to file + fix in the product, not to paper over with a memory.

**Durable safety constraint (still true):** in a managed `/agent-doc` session the ancestry is `tmux → zsh → agent-doc(<sup-pid>) start --route-owned → claude(<my-pid>) → my shell`, so the `start --route-owned` supervisor is the PARENT of the in-session agent — a hard `kill <sup-pid>` from inside the session kills the session mid-turn. If you ever must act on a controller process, verify with `pstree -ps $$ | grep <pid>` that it is NOT your host first; an orphan `controller serve … --handoff-state preparing` with a dead `--previous-controller-pid` is never your host. The graceful in-session verb is `agent-doc admin recycle --all-projects`.
