---
name: reference_agent_doc_test_fixture_controllers_leak_to_pid1
description: agent-doc test fixtures leak one controller per run to PID 1 plus its /tmp/.tmpXXXX root — 214 procs / 4.2GB / 1183 dirs accumulated in days
metadata:
  node_type: memory
  type: reference
---

agent-doc's test suite spawns a `controller serve --project-root /tmp/.tmpXXXXXX` per fixture and **never reaps it**. Snapshot 2026-08-09: **220** live controllers, **214** on `/tmp` fixture roots, all **reparented to PID 1**, holding **4.2 GB RSS**; **1183** `/tmp/.tmp*` dirs on disk (399 MB). 155 were stuck in `--handoff-state preparing`. Accumulation spanned ~6 days (oldest dirs 08-03).

Nothing respawns them — every one is orphaned, so this is pure accumulation, not a live storm. That distinction matters: killing them is permanent, unlike the respawn storm in [[reference_agent_doc_controller_respawn_storm_locks_state_db]].

Individually they are ~0.1% CPU (memory + fd pressure, not CPU); the CPU burn in the same snapshot came from the three *real*-root controllers in [[reference_agent_doc_autorecycle_install_handoff_wedge_survives_m1b]].

**Fixture dir shape** — every leaked dir is one of: contains `.agent-doc/`, contains a `project/` subdir (matching `--project-root /tmp/.tmpXXX/project`), or is empty. That triple test matched 1183/1183, so it cleanly separates agent-doc fixtures from other tools' `mkdtemp` dirs.

**How to apply:** sweep periodically — match controllers by `--project-root /tmp/`, confirm no live process has a `cwd` inside those dirs, `kill -TERM` (they all exit on TERM, no KILL needed), then remove the dirs. Note the sandbox hook blocks `rm -rf` when paths arrive via `xargs` because it cannot see the `/tmp` prefix — use a literal `rm -rf /tmp/.tmp*` glob instead. The real fix belongs in the test harness (reap the controller in fixture teardown), not in a recurring manual sweep.

**`agent-doc-native-<pid>` is a second, much larger leak (2026-09-22).** 148 such dirs held **51 GB**, filling the 94G `/tmp` tmpfs to 99% and blocking every cargo build and test in the workspace. The name carries the owning PID, so the safe reclaim is exact rather than mtime-guessed:

```sh
cd /tmp && for d in agent-doc-native-*; do
  pid=${d#agent-doc-native-}
  kill -0 "$pid" 2>/dev/null || rm -rf -- "$d"
done
```

147 of 148 PIDs were dead; skipping the live one avoided any PID-reuse risk. That took `/tmp` from 99% to 45%. Do **not** sweep the large `fpe-*` / `haiven-*` / `lazily-*` trees in the same pass — those are other sessions' live worktrees.

**Pointing `TMPDIR` at a disk-backed dir is NOT a valid workaround for a full `/tmp`.** It lets cargo compile, but any `TMPDIR` under `$HOME` reddens tsift's `lint::tests::ambient_tsift_state_dir_is_not_a_workspace_root`: the test builds a fake home in a tempdir and declares only that tempdir ambient, so the ancestor walk continues past it, finds the real `~/.tsift`, and resolves the root to `$HOME`. That is correct behavior, not a bug — but it means a `TMPDIR` override manufactures a false red. Reclaim the space instead. See [[reference_tsift_home_is_not_a_workspace_root]].
