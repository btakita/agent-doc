---
name: reference_leaked_busyloop_load_generators_saturate_cpu
description: "Concurrency-repro scripts that spawn `while :; do :; done` load generators leak them to PID 1; `kill $(jobs -p)` does not work in a non-interactive `zsh -c` eval."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 2a899df3-bf74-4b20-b0aa-56160e0450dc
  modified: 2026-07-26T20:37:05.997Z
---

Observed 2026-07-26: 52 orphaned `zsh` busy loops burned ~3015% CPU (≈30 of 32
cores) for 58 minutes. Origin was a *different* session's lazily
`queue_family_conformance` deadlock repro that saturated the box on purpose:

```sh
for i in $(seq 1 24); do (while :; do :; done) & done
LOADPIDS=$(jobs -p)
... test runs ...
kill $LOADPIDS       # never landed
```

Two failure modes stacked: `jobs -p` inside an `eval` in a non-interactive
`zsh -c` does not reliably capture those subshells, and when the parent shell
exits the loops are reparented to PID 1 and spin forever. The test binary had
already finished — only the generators leaked.

**How to apply:** when a repro needs artificial load, bound it in the child
itself (`(timeout 120 sh -c 'while :; do :; done') &`) instead of relying on a
later `kill`, and record PIDs with `$!` per spawn rather than `jobs -p`.

**Diagnosing it later:** `ps -eo pid,ppid,pcpu,etime,args ww | awk '$2==1'` and
sum `%CPU`. Killing needs `xargs -n1 kill -9` — zsh does **not** word-split an
unquoted `$PIDS`, so a bare `kill -9 $PIDS` silently fails with
`illegal pid: 123\n456`.

Related: [[feedback_verify_proxy_before_kill]],
[[feedback_never_dispatch_concurrent_agents_into_one_repo]]
