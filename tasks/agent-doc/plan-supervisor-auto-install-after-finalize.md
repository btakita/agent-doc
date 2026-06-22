# Plan — supervisor auto-install after finalize (`#supautoinstall`)

Automate the dogfood install step with the PCP + supervisor so an agent-doc session
editing agent-doc's OWN source never has to stall on a mid-session `cargo install`
(which races the live supervisor → `live_prompt_drift` / exit-75 / dropped queue edits,
the `#no-mid-session-install` hazard). Operator goal (2026-06-14): "rather than stalling
on installing during a live supervisor … automate the install after finalize with the
PCP + supervisor."

## Root cause / gap

`#ctlrecycle` (LANDED) closed the *recycle-onto-fresh-binary* half: a stale supervisor
(installed binary newer than the running process) hot-reloads via `execve` at the next
idle/turn boundary. But nobody runs the **install** automatically, so in a dogfooding
session the chain is broken at the first step:

1. A finalize commits an edit to agent-doc's own source.
2. **Nothing builds+installs it** — the manual `runbooks/dogfood-supervisor-refresh.md`
   (steps 1–2: `cargo build --release && cargo install --path . && agent-doc lib-install`)
   is required, and the agent can't run it mid-session against the live supervisor.
3. So the installed binary never becomes newer than the running process → the
   `#ctlrecycle` recycle never fires → the session keeps serving stale code.

The install is the missing rung. It must run **in the supervisor at an idle boundary**,
not in the finalize client mid-cycle — that placement is exactly what root-fixes the
mid-session-install drift.

## Design — install rung precedes the existing recycle rung

Symmetric to `process_binary_is_stale` (installed-newer-than-process → recycle), add a
`source_newer_than_installed_binary` signal (source-newer-than-installed → install). The
two compose into one self-healing chain:

```
source edit committed
  └─(idle boundary, opt-in)→ supervisor_auto_install: cargo build --release
                                                      + cargo install --path .
                                                      + agent-doc lib-install
        └─ installed binary now newer than running process
              └─(same/next idle boundary)→ #ctlrecycle recycle: execve hot-reload
                    └─ fresh supervisor serves the next queue item on the new binary
```

The recycle half is entirely reused — this plan only adds the install trigger.

### Seams (all pure decisions + thin IO, mirroring `#ctlrecycle`)

- `decisions.rs::SupervisorInstallAction { None, Detect, Install }` +
  `supervisor_install_action(source_newer, auto_install, turn_boundary)` — pure, unit-tested.
- `project_controller::rpc::source_newer_than_installed_binary(newest_source_secs,
  installed_binary_secs) -> bool` — pure, unit-tested (strictly-newer; equal = not newer,
  fail-open).
- `resolve_supervisor_auto_install(env, frontmatter, project) -> bool` — pure precedence
  (env > frontmatter > project > built-in **default ON**), unit-tested, mirroring
  `resolve_supervisor_auto_recycle`. `supervisor_auto_install_enabled(doc)` wraps it,
  reading the `agent_doc_supervisor_auto_install` frontmatter + project knobs.
- `dogfood_agent_doc_crate_root(file) -> Option<PathBuf>` — IO: resolves the agent-doc
  crate root from the served doc's project root (project root itself, or `src/agent-doc`)
  only when a `Cargo.toml` with `name = "agent-doc"` is present, so it NEVER fires for an
  ordinary user's document. Fail-open None.
- `newest_crate_source_mtime_secs(crate_root) -> Option<u64>` — IO: bounded recursive walk
  of `.rs`/`.toml`/`.md` mtimes, skipping `target/`/`.git/`/`.agent-doc/`/`node_modules`/
  `.tsift`/`build`/`dist`. Fail-open None.
- `run_supervisor_auto_install(crate_root) -> Result<()>` — IO: runs the three install
  commands with cwd = crate root; returns Err naming the failed step.

### Wiring (`start/idle_watch.rs`)

Resolve `auto_install_enabled` + `install_crate_root` once before the loop (so the
default-OFF path does ZERO extra work — same shape as `recycle_auto_enabled`). Inside the
loop, after the self-kill check and before the staleness/recycle block, when opted in and
dogfooding:

- compute `source_newer` from `newest_crate_source_mtime_secs` vs the installed binary mtime,
- `supervisor_install_action(...)`, debounced via `recycle_debounce_decision` with its own
  `install_stale_since` (a build is heavy → always debounce, never the immediate path),
- `Detect` → log `supervisor_source_newer_detected` once (opted out),
- `Install` (debounced) → log `supervisor_auto_install_started`, run the install, log
  `supervisor_auto_install_succeeded` / `…_failed`. On failure set a one-shot
  `auto_install_disabled` latch (like `reexec_recycle_disabled`) so a broken build is not
  re-attempted every boundary.

After a successful install the binary is newer than the running process, so the existing
recycle block (same iteration) flips `supervisor_stale=true` and hot-reloads. After install
`source_newer` is false (binary mtime now ≥ source) so it is naturally idempotent / no
re-trigger.

## Safety / opt-in

- Default **ON** (operator directive 2026-06-14) — safe because the build only fires for a
  DOGFOODING session (`dogfood_agent_doc_crate_root` resolves); an ordinary user's document
  never triggers it. Opt out with a falsey `AGENT_DOC_SUPERVISOR_AUTO_INSTALL` env, the
  `agent_doc_supervisor_auto_install` frontmatter key, or the project-config key. When off,
  the dogfood source-ahead state only logs a detect line and the manual refresh applies.
- Idle-gated (turn boundary only) + debounced + fail-open on every stat/resolution error.
- Build BLOCKS the idle-watch thread for its duration — acceptable: it only runs at a
  proven turn boundary with no turn in flight, and the point is to refresh before the next
  queue item.
- Never runs in the finalize client → cannot cause the mid-session-install drift.

## Status — CODE-COMPLETE (this turn), live proof operator-drive

- Seams + wiring landed in the agent-doc submodule; `make check` green. Default OFF.
- Updated `runbooks/dogfood-supervisor-refresh.md` to document the opt-in auto path.
- **Live proof is operator-drive** (`[operator-verify]`, cannot be agent-done — it needs a
  live dogfood session + a real source-edit→install→recycle cycle): with
  `AGENT_DOC_SUPERVISOR_AUTO_INSTALL=1`, edit agent-doc source, finalize, and confirm
  ops.log shows `supervisor_auto_install_started` → `…_succeeded` →
  `supervisor_binary_stale_self_recycled`, with the session continuing on the new binary.

## Testing

Process install/recycle can't be SimWorld-modeled (same constraint as `#ctlrecycle`).
Coverage is the pure predicates: `supervisor_install_action` truth table,
`source_newer_than_installed_binary`, `resolve_supervisor_auto_install` precedence.

## Constraints

- `[clean-session]` for the live flip — touches `start/idle_watch.rs` (the contested live
  supervisor path). Code + `make check` are safe under a live session (build only, no
  install); do NOT `cargo install` / restart the supervisor mid-session.
- Follow-up: lift the env-only opt-in to a frontmatter/project knob (mirror
  `resolve_supervisor_auto_recycle`); optional preflight `source_newer` hint.
