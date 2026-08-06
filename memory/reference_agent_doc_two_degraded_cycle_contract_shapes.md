---
name: reference_agent_doc_two_degraded_cycle_contract_shapes
description: "A missing cycle contract has TWO distinct causes with different fixes: NO marker = cwd is not the doc's repo root (relative path fails resolve_document); marker + EMPTY body = preflight errored to stderr."
metadata: 
  node_type: memory
  type: reference
  originSessionId: e02d13b6-2abb-4ec9-baeb-f98355152c7a
  modified: 2026-08-03T03:46:31.756Z
---

Diagnosed 2026-08-02 after four wasted `/agent-doc` cycles on
`tasks/professional/equityfundingsource.md`.

`src/preflight_hook.rs` prints the marker **unconditionally at line 60**, then
runs preflight and sends any error to **stderr**, which the agent never sees.
That single detail splits the failure into two unrelated bugs:

| symptom | meaning | fix |
|---|---|---|
| **no marker at all** | returned *before* line 60 — `invoked_document()` or `resolve_document()` gave `None` | make cwd the doc's repo root, or invoke with an ABSOLUTE path |
| **marker + empty body** | preflight ran and returned `Err`; diagnostic went to stderr | run `plan` / `session-check` to surface the real error |

**No marker — the cwd trap.** `resolve_document` is:

```rust
let candidate = if Path::new(target).is_absolute() { PathBuf::from(target) }
                else { cwd.join(target) };
candidate.is_file().then_some(candidate)
```

A relative `tasks/foo.md` is joined to the hook payload's `cwd`. The Bash tool's
working directory **persists across calls**, so `cd`-ing into a submodule
(`src/agent-doc`, `src/censible-core`, …) for repo work silently breaks the next
agent-doc invocation: the joined path is not a file, the hook returns `Ok(())`,
and nothing is emitted. It looks identical to "not a trigger."

Two habits avoid it entirely: **invoke with an absolute path**, and wrap
submodule work in a subshell — `(cd src/foo && cmd)` — so the session cwd never
leaves the repo root.

**Marker + empty body — read the real error.** `plan` and `session-check` still
work and will name it. On this occasion:

```
editor is still attached for <doc>; missing_replica recovery exhausted
and disk read authority is refused
```

IntelliJ was live (check `ps` for `idea`/`jetbrainsd`), so neither `--force-disk`
nor hand-aligning disk was permissible. **`agent-doc admin reload-lib` cleared
it** — `typed reload delivered to 1/3 editor endpoints (2 unavailable)`. Those
unavailable endpoints are stale plugin peers, and preflight's `stale_install`
warning had been flagging exactly that on every cycle of the session. Treat a
persistent `stale_install` warning as a *prediction* of this wedge class, not
noise.

A third, separate cause of an empty body is a stale snapshot ahead of HEAD — see
[[reference_agent_doc_hand_commit_leaves_stale_snapshot_empty_preflight_contract]].
Distinguish by `session-check`: stale snapshot reports
`snapshot does not match HEAD (snapshot_len=…, head_len=…)`, the replica wedge
reports `missing_replica … disk read authority is refused`.

**Worth fixing upstream:** by the time `resolve_document` runs,
`invoked_document` has already proven the prompt IS an agent-doc trigger, so an
unresolvable path is a genuine error rather than an uninteresting prompt. A
stderr diagnostic there — or resolving against the document's owning repo root —
would turn four silent cycles into one message.
