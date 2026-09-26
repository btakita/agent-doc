---
name: reference_pgrep_f_wait_loop_matches_its_own_command_line
description: "A `while pgrep -f \"<pattern>\"` wait loop matches its own wrapper shell's command line and waits forever; match the binary name with `pgrep -x` instead."
metadata:
  node_type: memory
  type: reference
---

A background wait loop like

```bash
while pgrep -f "cargo nextest run --workspace" >/dev/null; do sleep 15; done
make check
```

never proceeds. The harness runs the whole script through `zsh -c '<script text>'`,
so the wrapper's own `/proc/<pid>/cmdline` CONTAINS the pattern — `pgrep -f` matches
the waiter itself. Observed 2026-09-26: a 10-minute wait on an already-idle tree,
and `pgrep -af` output showed the wrapper shell as the only "match".

**How to apply:** wait on the process's binary name, not a command-line substring:
`pgrep -x cargo-nextest`, `pgrep -x cargo`, `pgrep -x rustc`. If a `-f` pattern is
unavoidable, exclude self with `pgrep -f "pat" | grep -v "^$$\$"` or bracket a
character (`"cargo nextes[t]"`) so the literal pattern text cannot match.
Relevant whenever waiting out a concurrent build in this shared tree —
see [[reference_concurrent_make_test_deletes_nextest_binary_midrun]].
