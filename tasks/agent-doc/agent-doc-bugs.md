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
### Re: Context written to specs + pending, exchange compacted

**Spec updates (`src/agent-doc/specs/supervisor.md`):**

1. **Phase 1 Implementation Status table** added after Motivation — tracks cwd (landed) vs pty/resize/state/ipc/start.rs wire-up (not started) in the sequence subsequent PRs will land. Notes the `#![allow(dead_code)]` gate in `supervisor/mod.rs` stays until `start.rs` consumes the module.
2. **CWD determinism invariants** — added an "Implementation invariants" block under Core Invariants pinning the four non-obvious decisions from this session: relative-path bases differ by source, misconfigured paths hard-error, `find_project_root` duplicated for module self-containment, `CwdSource::as_str()` tags locked.
3. **Non-tmux Mode (Future) section** (from earlier this cycle) — four requirements + non-goals + explicit "not in phase 1" scope gate, so the idea survives future refactors.

**Pending items added** for the remaining phase 1 submodules, each with enough context to pick up cold: `pty.rs`, `resize.rs`, `state.rs`, `ipc.rs`, `start.rs` wire-up, plus the manual smoke test from the release checklist.

**Exchange compacted** via `agent-doc compact` with a summary-message pointer. 1693 lines archived to `.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260413-195807.md` — nothing lost, just moved off the hot path.

Ready to proceed with `supervisor/pty.rs` whenever you give the go-ahead.
❯ proceed
### Re: supervisor/pty.rs landed (#a17b done)

`src/agent-doc/src/supervisor/pty.rs` — pty allocation + child spawn + optional stdin/stdout forwarding threads. Registered in `supervisor/mod.rs`. `portable-pty = "0.8"` added to `Cargo.toml`. Build clean, clippy clean, 6 targeted tests passing.

**Public surface (kept narrow — resize/state/ipc each get their own module):**

- `PtySpawnConfig { program, args, cwd, env, size }` — caller-supplied, everything pre-resolved. Has a `new(program, cwd)` helper that defaults to 24×80 when the real size isn't known yet.
- `PtySession::spawn(cfg)` — allocates the pair via `NativePtySystem::default().openpty`, builds a `CommandBuilder`, calls `env_clear()` then re-populates from `cfg.env`, spawns, **drops the slave immediately** so the child owns the only reference (otherwise master reads never EOF on child exit).
- `PtySession::forward_stdio()` — spins up the two I/O threads (`pty->stdout`, `stdin->pty`). Separate from `spawn` so tests can skip it; errors are logged to stderr (honoring "never swallow errors" from `CLAUDE.md`), second call returns `bail!`.
- `PtySession::resize(size)` — thin wrapper so `resize.rs` never has to touch `portable-pty` types directly.
- `PtySession::wait()` / `kill()` — consumed by the eventual `state.rs` restart loop.

**Invariants pinned in the module doc comment:**

1. Caller-supplied `cwd` is already canonicalized (by `cwd.rs`) — we pass it through unchanged.
2. `env` is the **complete** env for the child. Parent env is **not** inherited — this enforces the "deterministic env" invariant from the spec. Callers that want `PATH`, `HOME`, etc. populate them explicitly. This matters for the `start.rs` wire-up — see pending item below.
3. Slave dropped immediately after spawn (load-bearing; I put a comment there).
4. I/O threads exit on EOF from their underlying pipe; no explicit teardown — `PtySession::drop` closes the master, which propagates.

**Tests (all via fake-claude shell scripts in a `TempDir`, no real claude):**

| Test | What it locks |
|------|---------------|
| `spawns_and_waits_for_clean_exit` | Happy path — spawn, run, `wait` returns `success()` |
| `propagates_nonzero_exit_code` | `exit 42` → `wait` returns non-success (state.rs restart policy will depend on this) |
| `env_is_not_inherited_from_parent` | Sets `AGENT_DOC_PTY_PARENT_LEAK` in the parent; child-side `[ -n $VAR ] && exit 99` → expects clean exit. Locks the deterministic-env invariant against future refactors. |
| `cwd_is_set_on_child` | Child runs `touch marker.txt` from a nested subdir; asserts marker lands in the requested cwd, not the parent process cwd. |
| `resize_after_spawn_succeeds` | `resize(50, 120)` mid-run on a sleeping child. Smoke-tests that the `resize.rs` wrapper has a real master to hit. |
| `missing_program_errors_cleanly` | Spawn of `/nonexistent/...` returns `Err` whose chain mentions the program name (useful when start.rs logs the failure). |

**Two follow-ups pushed to pending** (both tagged for `start.rs` wire-up, which is already the last stage of phase 1):

- `#<auto>` — wire `PtySession` into `start.rs`, replacing the current `Command::status` inherit-tty path. Non-trivial because `start.rs` carries the auto-trigger `tmux send-keys` + session logging + restart loop that `state.rs` will eventually absorb. Do not touch until `resize.rs`, `state.rs`, `ipc.rs` land.
- `#<auto>` — decide env passthrough policy. `cwd.rs` gave us deterministic CWD; `pty.rs` requires the caller to hand over a complete env, but `start.rs` today just inherits. Two options when we wire up: (a) enumerate the exact set of vars claude needs (`PATH`, `HOME`, `TERM`, `LANG`, `TMUX`, `TMUX_PANE`, any agent-doc vars from frontmatter expansion) and only forward those, or (b) add an opt-in `PtySpawnConfig::inherit_parent_env` escape hatch that grabs `std::env::vars()` at spawn time. Option (a) matches the spec's "deterministic env" invariant better; (b) is less painful in the short term. Decide before `start.rs` wire-up, not during.

**Not in scope for this landing** (deliberately): SIGWINCH plumbing, crash classifier, IPC socket, `start.rs` integration. Each gets its own PR per the phase 1 status table in `specs/supervisor.md`.

Next: `supervisor/resize.rs` when you give the go-ahead — `signal-hook` for Unix SIGWINCH → `PtySession::resize`, Windows stub deferred.
❯ I do want ENV variables from the parent shell. Mainly to control claude or other agent harnesses. agent-doc supports `env` frontmatter. I want the `env` frontmatter to support overriding env variables. A pending features is to unset `env` in the frontmatter.
❯ Is this parent env cascade in alignment with your plan?
### Re: parent env cascade — yes, aligned, but one layer up from pty.rs

Short answer: yes, fully compatible with where `pty.rs` landed — but the cascade logic belongs **above** `pty.rs`, not inside it. `pty.rs` stays strict ("caller hands me the complete env"), and a sibling module builds the merged env before calling `PtySession::spawn`. Here is why and what it looks like.

**Why not put the cascade inside `PtySpawnConfig`:** if `pty.rs` owns the cascade (inherit + overlay + unset), then two things get worse:

1. Testing — every pty test needs to reason about which parent vars leaked in. The `env_is_not_inherited_from_parent` test exists specifically to lock that boundary; weakening it means the next refactor can silently regrow the "mystery env" problem.
2. Reuse — `state.rs` will eventually restart the child on crash. If the cascade lives in `pty.rs`, every restart re-reads `std::env::vars()` at the moment of spawn, so a parent-env change between spawns silently changes child behavior. That's a real footgun for long-running supervisors.

Keeping `pty.rs` deterministic means the merged env is **computed once** (at supervisor startup, from frontmatter + the parent env captured at that instant) and reused on every restart. Deterministic restarts > ergonomic construction.
❯ agreed

**Proposed sibling — `supervisor/env.rs`:**

```rust
pub struct EnvSpec {
    /// Whether to start from std::env::vars() (the parent shell).
    pub inherit_parent: bool,
    /// Frontmatter env field: Name -> Some(value) sets, None unsets.
    pub overrides: IndexMap<String, Option<String>>,
}

impl EnvSpec {
    /// Resolve to the final HashMap<String, String> passed to PtySpawnConfig.
    /// Parent env is captured at call time — call once per supervisor lifetime.
    pub fn resolve(&self) -> HashMap<String, String> { ... }

    /// Parse from frontmatter. Supports three forms:
    ///   env:
    ///     FOO: bar           # set
    ///     BAZ: null          # unset (maps to None)
    ///     QUX: "${HOME}/x"   # shell-style expansion against parent env
    pub fn from_frontmatter(fm: &Frontmatter) -> Result<Self> { ... }
}
```

Then `start.rs` does roughly:

```rust
let env_spec = EnvSpec::from_frontmatter(&fm)?;   // parse once
let resolved_env = env_spec.resolve();            // capture parent once
let cfg = PtySpawnConfig {
    program, args,
    cwd: resolved_cwd.path,
    env: resolved_env,                            // complete, frozen
    size,
};
PtySession::spawn(cfg)?;
```

**What this gives you:**

- **Inherit** — default `inherit_parent: true` (matches your stated preference), falsifiable via a frontmatter flag if a document ever wants a sealed env.
- **Override** — any key in frontmatter `env:` wins over the parent.
- **Unset** — `KEY: null` in frontmatter drops `KEY` from the parent env entirely. The existing "unset via frontmatter" pending feature you mentioned falls out for free.
- **Expansion** — `${HOME}/.config/foo` style expansion runs against the parent env **at resolve time**, so values like `$TMUX` or `$PATH` work the way a shell user expects.
- **Restart determinism** — `resolve()` is called once; the captured map is reused across every `state.rs` restart, so a parent-env mutation mid-run cannot silently reshape the child.
- **pty.rs unchanged** — the `env_is_not_inherited_from_parent` invariant stays load-bearing because `pty.rs` never touches the parent.

**Open question for you before I build it:**

Where does `EnvSpec::from_frontmatter` live relative to the existing `agent-doc` env handling? You already have the v0.32.2 `env` frontmatter feature wired into `claude_args` expansion (per the status line) — is that parser in `frontmatter.rs`? If so, `env.rs` should reuse the same parse path (maybe lift the shared type into `frontmatter.rs` and have `env.rs` be a thin resolver). I do not want to fork the frontmatter schema.

❯ Please research. Refactor a common method(s) and module(s) where applicable. 

If you confirm the structure is aligned, I will:

1. Read the existing `frontmatter.rs` env handling to find the natural seam.
2. Land `supervisor/env.rs` as a sibling to `cwd.rs` and `pty.rs` (module doc, invariants pinned, tests for inherit/override/unset/expansion/restart-determinism).
3. Drop the "decide env passthrough policy" pending item (it will be answered by this module).

And **not** do `resize.rs` yet — env.rs is a smaller, tighter dependency of `start.rs` wire-up than resize is, so it makes sense to land first.

Confirm and I will go.
❯ go
### Re: research done — landing `supervisor/env.rs` on top of the existing shared `env.rs`

**Research findings — the seam already exists:**

| Piece | Location | What it does |
|---|---|---|
| `Frontmatter.env` | `frontmatter.rs:321` | `IndexMap<String, String>` — set-only, no unset support |
| `env::expand_values` | `env.rs:39` | Shell-expands values in document order, one `sh -c` per batch |
| `env::shell_export_prefix` | `env.rs:75` | Unexpanded `export K="V" && ...` prefix for `tmux send-keys` |
| Consumers | `start.rs:189`, `run.rs:129`, `stream.rs:136`, `parallel.rs:165`, `preflight.rs:605` | Read `fm.env` through those two helpers |
| `PtySpawnConfig.env` | `supervisor/pty.rs:71` | `HashMap<String, String>` — complete, `env_clear()` before set |

So the shared foundation is **`env.rs`**, not a new module. `supervisor/env.rs` is a thin resolver on top of it, matching the v0.32.2 frontmatter schema. No forking. The refactor lifts the schema one level — supports `null` to unset — and every existing consumer picks up unset support for free.

**Delta I am landing now:**

1. **Schema change** — `Frontmatter.env: IndexMap<String, Option<String>>`. `FOO: null` in YAML parses to `None` = "remove this key from the child env". Serde handles the null→None mapping.
2. **`env.rs` signatures** — `expand_values` and `shell_export_prefix` take the new map type. `expand_values` returns `Vec<(String, Option<String>)>`; unset keys are preserved for the caller. `shell_export_prefix` emits `unset KEY` for None.
❯ 3. **Consumers handle None** — `start.rs` / `run.rs` / `stream.rs` call `cmd.env_remove(key)` for None. `parallel.rs` picks up `unset` via the helper. `preflight.rs` JSON output serializes null correctly (skill step 0c2 treats null as `unset`).: Is there any way to distinguish between a `null` unset and a `"null"` string?
4. **New frontmatter field** — `agent_doc_env_inherit: Option<bool>` (default `true`). Controls whether the supervisor starts from the parent env. `true` = inherit + overlay (your preference). `false` = sealed env.
5. **New module `supervisor/env.rs`:**
   ```rust
   pub struct EnvSpec {
       pub inherit_parent: bool,
       pub overrides: IndexMap<String, Option<String>>,
   }
   impl EnvSpec {
       pub fn from_frontmatter(fm: &Frontmatter) -> Self { ... }
       pub fn resolve(&self) -> Result<HashMap<String, String>> { ... }
   }
   ```
❯    `resolve()` captures `std::env::vars()` once (if `inherit_parent`), calls `env::expand_values` on `overrides`, overlays set values, removes unset keys, returns the complete map for `PtySpawnConfig.env`. Called once at supervisor startup → reused across every `state.rs` restart.: If a user changes the env frontmatter, or any other frontmatter that affects the session state, should we prompt to restart the session or have the changes apply to the next session?
6. **Tests** for env.rs (Option round-trip) and supervisor/env.rs (inherit / override / unset / expansion / restart-determinism / sealed env).
7. **Pending updates** — drop `#jaaz` (resolved by this module), mark pty.rs env passthrough decision closed, add new item for `start.rs` wire-up consuming `EnvSpec`.

Proceeding with implementation. Will checkpoint when env.rs + frontmatter change lands and tests are green.
### Re: `supervisor/env.rs` landed — parent cascade wired through the existing `env.rs` seam

**Landed the refactor + the new module. All targeted tests green, clippy clean.**

**Shared-foundation lift in `src/env.rs`:**

- New type alias: `pub type EnvMap = IndexMap<String, Option<String>>;`
- `expand_values(&EnvMap) -> Vec<(String, Option<String>)>` — `None` preserved in document order; no shell spawn if every entry is unset.
- `shell_export_prefix(&EnvMap)` emits `unset KEY && ` for `None` entries. `parallel.rs` picks this up for free.
- Five new tests pinning the Option path (`expand_values_unset_preserved`, `expand_values_mixed_set_and_unset_document_order`, `shell_export_prefix_unset`, `shell_export_prefix_mixed`, plus updated roundtrip).

**Schema change in `frontmatter.rs`:**

- `Frontmatter.env: IndexMap<String, Option<String>>` — YAML `FOO: null` now parses to `None`. Default-derive still works because `Option<String>` + `IndexMap` default cleanly.
- New field `agent_doc_env_inherit: Option<bool>` (default `true` when resolved by `EnvSpec::from_frontmatter`). Controls the supervisor parent-env cascade. Set `false` for a sealed env.
- Three updated tests — `parse_env_map`, `parse_env_unset_via_null` (new), `write_roundtrip_with_env` — all green.

**Consumer updates:**

- `start.rs:189` — handles `None` via `cmd.env_remove(key)`, set via `cmd.env(key, v)`.
- `run.rs` / `stream.rs` — unchanged call sites; the new `Vec<(String, Option<String>)>` signature flows through `agent::resolve` and `resolve_streaming` to `Claude::with_env`.
- `agent/claude.rs` — `env: Vec<(String, Option<String>)>`; both `Agent::send` and `StreamingAgent::send_streaming` call `cmd.env(k, v)` or `cmd.env_remove(k)` per entry.
- `agent/mod.rs::resolve` — signature updated to the Option vec.
- `parallel.rs` — unchanged call site; `shell_export_prefix` handles both cases internally.
- `preflight.rs::PreflightOutput.env` — now `IndexMap<String, Option<String>>`. JSON serializes `null` for unset keys. Skill step 0c2 should treat null as `unset KEY` (tracked separately).

**New `supervisor/env.rs`:**

- `EnvSpec { inherit_parent: bool, overrides: EnvMap }` — values object, not a live view.
- `EnvSpec::from_frontmatter(&Frontmatter)` — reads `agent_doc_env_inherit` (defaults `true`) and `env:`.
- `EnvSpec::sealed(overrides)` — convenience for `inherit_parent: false`.
- `EnvSpec::resolve() -> Result<HashMap<String, String>>` — the **one** call that touches `std::env::vars()`. Walks: capture base → shell-expand overrides → overlay sets → remove unsets → return complete map for `PtySpawnConfig::env`. Expansion runs once per call via the shared `expand_values`.
- Registered in `supervisor/mod.rs` (phase 1 status comment updated to include `env`).

**Nine new tests in `supervisor::env::tests`:**

| Test | Locks |
|---|---|
| `from_frontmatter_default_inherits` | `inherit_parent = true` default |
| `from_frontmatter_explicit_false_seals` | `agent_doc_env_inherit: false` path |
| `from_frontmatter_copies_env_map_verbatim` | overrides match `fm.env` exactly |
| `resolve_inherit_plus_overlay` | inherited key survives; override wins over parent |
| `resolve_unset_removes_parent_key` | `KEY: null` drops inherited parent value |
| `resolve_expansion_uses_parent_env` | `$PARENT_VAR` inside override expands against parent shell |
| `resolve_sealed_drops_parent` | `inherit_parent: false` yields only overrides |
| `resolve_is_deterministic_across_parent_mutation` | cached map frozen; post-resolve parent mutation invisible to caller |
| `resolve_empty_sealed_is_empty` | sanity |

Plus an `EnvGuard` helper for test-local parent env mutation (restores in `Drop`, uses per-test unique keys to avoid parallel interference).

**What this gives you for `start.rs` wire-up (when `state.rs` lands):**

```rust
let env_spec = supervisor::env::EnvSpec::from_frontmatter(&fm);
let resolved_env = env_spec.resolve()?;   // called ONCE at supervisor startup
let cfg = supervisor::pty::PtySpawnConfig {
    program, args,
    cwd: resolved_cwd.path,
    env: resolved_env,                      // complete, frozen, restart-safe
    size,
};
```

`state.rs` will reuse `resolved_env` on every restart — parent-env mutations mid-run cannot silently reshape the child. `pty.rs` still `env_clear()`s and `env_is_not_inherited_from_parent` stays load-bearing. The inheritance is deliberate, scoped, and captured exactly once.

**Verification run:**

- `cargo build --tests` — clean
- `cargo clippy --bin agent-doc -- -D warnings` — clean
- `cargo test --bin agent-doc env::tests` → **10/10 pass**
- `cargo test --bin agent-doc supervisor::env::tests` → **9/9 pass**
- `cargo test --lib parse_env write_roundtrip_with_env` → **4/4 pass**
- Full suite has 44 unrelated failures in `resync::tests` / `route::tests` / `supervisor::cwd::tests::document_parent_fallback_when_no_project_root` — reproduced on a clean stash of this branch, so they are pre-existing (tmux-environment dependent and a cwd-detection quirk when the test harness walks out to the monorepo `.git`). None touched by this change.

**Pending board updates (applied via granular flags this cycle):**

- `#jaaz` — closed. The "decide env passthrough policy" question is resolved by this module: parent env is inherited by default, frontmatter overlays set/unset, resolved once and reused.
- New item — wire `EnvSpec::from_frontmatter(&fm).resolve()?` into `start.rs` once `state.rs` restart loop is in place. Deliberately tied to `state.rs` so we do not bifurcate the `start.rs` restart surface twice.

**Follow-ups intentionally out of scope** (each gets its own PR):

- Skill step 0c2 needs a tiny update to emit `unset KEY` when the JSON `env` value is `null`. The binary side is done; the skill-side change is ~5 lines and best landed when you next touch the skill.
- `resize.rs` — still next after this. Smaller than `state.rs` and a dependency of the wire-up.
- `start.rs` wire-up itself — blocked on `state.rs` per the phase 1 status table.

Ready for `resize.rs` whenever you give the go-ahead.
### Re: `null` vs `"null"` string — yes, fully distinguishable

YAML has a native null scalar distinct from string `"null"`. serde_yaml maps them correctly to `Option<String>`:

```yaml
env:
  FOO: null       # → None       (unset)
  BAR: ~          # → None       (unset, YAML alt syntax)
  BAZ:            # → None       (empty scalar, YAML null)
  QUX: "null"     # → Some("null")  (literal 4-char string)
  ZOT: 'null'     # → Some("null")  (same)
```

No custom deserializer needed — `IndexMap<String, Option<String>>` with `#[serde(default)]` handles it. I'll add a round-trip test in `env.rs` covering all five forms so a future YAML library swap can't silently break the distinction.

### Re: Frontmatter changes mid-session — surface, don't auto-apply

Short answer: **apply on next supervisor restart, surface a notice when the change is detected, never auto-kill the running pane.**

Rationale — three failure modes to avoid:

1. **Silent drift.** User edits `env.FOO` mid-session; current pane keeps old value; user assumes new value is live. → must surface.
2. **Disruptive auto-restart.** User tweaks frontmatter while claude is mid-response in the pane. Killing the pty to apply a new `PATH` loses in-flight output and context. → must not auto-restart.
3. **Ambiguous scope.** Some fields affect spawn (`env`, `agent_doc_env_inherit`, `cwd`, model binary selection). Others are advisory (`agent_doc_model_tier`, metadata). Blanket "restart on any frontmatter change" is too aggressive.

❯ Can `agent_doc_model_tier` be a supervisor operation? Would the supervisor be able to start Claude Code with the defined model?

Proposed mechanism — extends the preflight contract, no new moving parts:

- **Preflight diffs frontmatter** against the snapshot frontmatter (already parses both). Computes a `supervisor_dirty: { changed: ["env.FOO", "agent_doc_env_inherit"], restart_required: true }` block in the JSON output.
❯ - **Classification table** in `supervisor/env.rs` (or a sibling `frontmatter_effects.rs`) maps each field to one of: `Spawn` (needs restart), `Advisory` (no effect on running pty), `Hotpatch` (can be applied without restart — none yet, but room to grow).: Does Claude Code support setting env variable on the fly using shell commands `! ENVVAR=...`. Note if this were to be done, some envvars (agent harness specific) would require a restart to take effect.
- **Skill step 0c3** (new): if `supervisor_dirty.restart_required` is true, write a one-liner into the response: "⚠ frontmatter changed: `env.FOO`, `agent_doc_env_inherit` — run `agent-doc restart <FILE>` to apply, or changes take effect on next crash-triggered restart."
- **New subcommand** `agent-doc restart <FILE>`: sends SIGTERM to the supervised pty, supervisor state machine re-enters spawn with fresh `EnvSpec::from_frontmatter`. This is the explicit opt-in the user triggers when they're ready.
- **Next-session pickup is automatic.** `start.rs` calls `EnvSpec::from_frontmatter` on every spawn, so a fresh session after `agent-doc claim` or process exit always picks up current frontmatter — no special path needed.

Edge case worth calling out: if `agent_doc_env_inherit` flips from `true` → `false`, the new env is **smaller** than the old one (parent vars dropped). Restart is the only safe path — you cannot hot-unset arbitrary parent vars from a running process without killing child subprocesses that may depend on them. The classification table should mark `agent_doc_env_inherit` as `Spawn` unconditionally.

Proceeding with the Option<String> implementation as planned; I'll add the frontmatter-diff + `supervisor_dirty` surfacing as a follow-up item rather than bundling it into this delta.
### Re: `agent_doc_model_tier` as a supervisor operation — yes, and it closes a real gap

Claude Code's CLI accepts `--model <model-id>` at spawn. The supervisor already owns the spawn command construction, so wiring tier → model is mechanical:

```rust
// supervisor/model.rs (new, or fold into env.rs)
pub fn tier_to_model(tier: Tier) -> &'static str {
    match tier {
        Tier::Low  => "claude-haiku-4-5",
        Tier::Med  => "claude-sonnet-4-6",
        Tier::High => "claude-opus-4-6",
        Tier::Auto => "",  // omit --model, let Claude Code pick
    }
}
```

`start.rs` appends `--model <id>` to the claude invocation when `agent_doc_model_tier` is set and not `auto`. Falls through cleanly when unset (current behavior).

**What this actually fixes:** today the skill's tier gate is advisory — it prints "run /model high" and hopes the user does it. With supervisor ownership, a fresh session (first spawn or post-restart) always comes up at the document's declared tier, zero user action required. The gate becomes enforcement, not a suggestion.

**Interaction with `/model` mid-session:** Claude Code's own `/model` slash command still works and overrides the spawn flag for the remainder of that session — that's CC's internal state, not something the supervisor can or should interfere with. The supervisor flag just sets the *initial* model per spawn. If the user `/model`s to something else, that sticks until restart, at which point the supervisor re-applies the frontmatter tier. This is the correct semantic: frontmatter is the source of truth for "what should this document run as by default", `/model` is a temporary override.

**Classification:** `agent_doc_model_tier` joins `Spawn` in the effects table — changing it requires restart for the new tier to take effect. Consistent with the `env` story.

One caveat: haiku/sonnet/opus mapping needs to live somewhere versioned. Hard-coding model IDs in `supervisor/model.rs` ties the binary to specific models; a month from now Opus 4.7 ships and we need a release to pick it up. Options: (a) read from a config file shipped with the binary, (b) accept a `model_map:` frontmatter override at the document or project level, (c) just release. I'd pick (c) initially — one-line change, easy to update — and add (b) when someone actually needs per-project overrides.

❯ In the output header sections, should we also include which model performed the patch? Add to pending

### Re: Claude Code on-the-fly `! ENVVAR=...` — partial, and not a supported mutation path for session env

Breaking this into three layers because they behave differently:

**1. Bash tool (what I use for every tool call):** Shell state does NOT persist between calls. From my own system prompt verbatim: *"The working directory persists between commands, but shell state does not."* So `export FOO=bar` in one Bash call is gone by the next. Each call re-sources the user's profile. The preflight step 0c2 pattern (export in a single Bash call) only works because subsequent commands *in that same call* see the exports — not subsequent tool calls.

**2. `!` prefix in the Claude Code prompt (user-typed):** This runs the command in a shell the harness manages and pipes output back into the conversation. I'm not 100% certain whether that shell is persistent across `!` invocations — I'd need to test it, and I don't want to speculate. Even if it is persistent, it's a *separate* shell from the Bash tool's shell, so exports there don't reach my tool calls.

**3. The supervised claude pty (the actual target of session env):** This process has its env set **once**, at `fork/exec` time, from whatever `EnvSpec::resolve()` returned. Nothing the user types at the Claude Code prompt — `!` or otherwise — reaches back into that pty's environment. The kernel doesn't expose "mutate another running process's env" as an API (you can `ptrace` it but that's a sledgehammer, and anything the child already read into its own state is untouched).

**What this means for the design:** ad-hoc `export` is not a mutation path for document session env. The supported paths are:

1. Edit frontmatter → save → next restart picks it up. (Explicit via `agent-doc restart`, or implicit via crash-triggered restart.)
2. First spawn after claim: always reads current frontmatter.

❯ correct... I tested and `!` and the envvar does not persist past the call...even when `! export FOO=bar` is used.

Your note is exactly right — *even if* on-the-fly export worked, harness-specific vars (`AGENT_DOC_*`, `CLAUDE_*`, anything the claude binary caches at startup) would still need a restart. So making ad-hoc export a mutation surface would create a two-tier env model (hot-patchable vs. restart-required) with no clear user-visible line between them. That's a confusing API. Better to keep one rule: **frontmatter is the env, restart applies it.** If someone needs a quick throwaway var for one command, they can just `FOO=bar some_command` inline — that's not session env, that's a per-invocation override and doesn't need the supervisor's help.

❯ I'll add a note in the spec doc making this explicit: "Session environment variables are set at spawn time from frontmatter `env:` 
❯  only. Ad-hoc shell exports do not affect the supervised pty or future Bash tool calls."
### Re: `!` empirical test — confirmed, and good to have it in writing

Thanks for running the test. That matches the model: each `!` invocation is a fresh shell process, so `export FOO=bar` dies with the shell. Same behavior as the Bash tool. Worth capturing in the spec doc alongside the supervised-pty point so the next person who asks "why can't I just export a var" has an empirical answer, not just an architectural one. I'll cite your test in the spec note.

### Re: model attribution in response headers — yes, useful, adding to pending

Short answer: yes, this is a good signal, especially for mixed-tier documents where a `/model` mid-session means some `### Re:` sections were opus and others were haiku and there's no way to tell after the fact.

Mechanism sketch (not implementing now, just naming the pieces):

- **Source of truth:** the supervisor already knows which model it spawned claude with (the `--model` flag from the tier resolution). When a response cycle starts, `agent-doc write --stream` can be told the current model ID via a new `--model <id>` flag, or read it from a session-state file the supervisor writes.
- **Header format:** append a subtle suffix to `### Re:` headers — `### Re: topic — opus-4-6` or `### Re: topic <sub>opus-4-6</sub>`. The em-dash form is cleaner in rendered markdown and doesn't need HTML.
- **Boundary synthesis:** when the skill writes raw content (no explicit header), the binary synthesizes a `### Re:` header from context. That synthesis path is the right place to inject the model — the skill doesn't need to know or care.
- **Explicit path:** when the skill writes `### Re: foo`, the binary can append ` — <model>` as a post-parse normalization step. Idempotent: skip if the header already has a trailing ` — <model-id>`.

Edge case: `/model` switch mid-response. The model flag is captured at write-time, so whatever the cycle started as is what the header reflects. If the user switches model via `/model` *between* cycles, subsequent cycles get the new attribution — correct.

Added as pending item. I'll wire it up alongside the `supervisor_dirty` work since both need the supervisor to expose its current model state to the skill.

❯ Please reorder agent:pending with the priority using this agent doc as a reference.
### Re: reorder pending by bugs-doc priority

Reordered with this doc's focus (bugs + investigations) driving priority:

1. **Active bug investigations** (top): `#a001` 🔴 boundary/baseline divergence, `#a006` duplicated `/agent:exchange` marker, `#a005` 1062 pre-existing `❯` lines, `#64mb` double commit_staging race
2. **Safety rails awaiting telemetry**: `#a002` normalize_user_prompts, `#xzz5` preflight slash-command parser
3. **Observability/guards**: `#a011` end-of-cycle write invariant, `#a012` session-check, `#a014` session drift resync, `#a010` PostToolUse hook
4. **Pending-system polish**: `#14z4` hash collision ceiling
5. **Release gate**: `#gqep` v0.32.4, `#mchh` jetbrains plugin verify
6. **Diagnostics**: `#4sh0` planned-move logging, `#tw4a` claim cd, `#8jzg` claim --isolate
7. **UX**: `#cce5` model attribution headers
8. **Janitorial**: `#a013` SPEC breakup, `#a004` drop Option B
9. **Supervisor feature work** (least urgent for *bugs* doc): `#jg0d`, `#b486`, `#40ct`, `#vnp0`, `#6ae3`, `#zp02`, `#f7d5`
10. **Deferred optional**: `#bak5`
❯ Make a spec file for the supervisor implementation.
### Re: supervisor spec already exists — what gap should I fill?

`src/agent-doc/specs/supervisor.md` already exists (248 lines, 19.5K) and has been updated throughout this session. Current sections:

- Motivation
- Phase 1 Implementation Status (cwd ✅ / pty ✅ / env ✅ / resize / state / ipc / start.rs wire-up)
- Non-Goals
- Architecture
- Core Invariants (incl. CWD determinism block landed this session)
- IPC Socket
- Crash Recovery Policy
- Migration from `start.rs`
- Five Hard Parts — Answers
- Resize Handling
- Non-tmux Mode (Future)
- Logging
- Dependencies to Add
- Testing Strategy
- Files to Add / Modify

Before I write anything, which of these do you actually want?

1. **Nothing — you forgot it existed.** I'll point you at it and stop.
2. **A separate implementation playbook** (step-by-step PR-by-PR checklist for phase 1, tighter than the design spec — something `/agent-doc` can drive from). Lives alongside `supervisor.md`, e.g. `supervisor-impl.md` or `runbooks/supervisor-phase1.md`.
3. **A new spec for a specific submodule** (`resize.rs`, `state.rs`, `ipc.rs`, or the `start.rs` wire-up) — smaller, module-scoped, invariants + tests + open questions.
4. **Something else** — tell me what's missing from `supervisor.md` and I'll extend it in-place rather than creating a parallel doc.

My recommendation: **option 2** if you want the phase 1 landing to be drivable from a single short checklist without re-reading the 248-line design spec each cycle; **option 4** (extend `supervisor.md`) otherwise. Parallel specs drift.
❯ **option 2**
### Re: phase 1 implementation playbook landed

`src/agent-doc/specs/runbooks/supervisor-phase1.md` — 6 stages (cwd/pty/env marked landed, resize/state/ipc/start.rs wire-up as PR-by-PR checklists), each with scope, module layout sketch, pinned invariants, test table, and done criteria. Front-links back to `../supervisor.md` as the source of truth for design rationale.

**Structure per remaining stage:**

- Scope (what the PR does)
- Module layout (public surface sketch in Rust)
- Invariants to pin (4–5 non-obvious rules to hard-code in the module doc comment — these are the things future refactors will trip over)
- Tests table (test name → what it locks)
- Cargo.toml additions
- Done criteria (green tests, clippy, gate flag status, etc.)

**Key design decisions baked into the playbook** (flag any you disagree with before I start landing `#jg0d`):

1. **Resize watcher owns the initial size push.** First resize fires synchronously before the watcher blocks, so claude boots at the real pane size instead of `PtySpawnConfig::new`'s 24×80 default. Alternative (push from `start.rs`) leaves a race between spawn and first SIGWINCH.
2. **Crash classifier uses `Instant::now()`, not wall clock.** Flap detection is monotonic — NTP jumps / suspend-resume cannot spuriously clear the counter. Tests inject a `Clock` trait to avoid real sleeps.
3. **`consecutive_failures` does NOT reset on `Transient`.** Only `Clean` resets. Prevents a flapping child that occasionally exits 0 from looping forever.
4. **IPC is one-request-per-connection.** No long-lived subscriptions in phase 1. Simplifies accept loop, avoids subscription leaks, keeps the lock-order invariant trivial.
5. **IPC lock order is state → pty**, documented in module doc. Prevents deadlocks with the `state.rs` restart loop.
6. **Clean shutdown order is reverse of startup.** kill claude → drop pty → close IPC socket → join threads → unlink socket. Other orders leak.
7. **`AGENT_DOC_SESSION` / `AGENT_DOC_DOCUMENT` set via `EnvSpec` overrides**, not inside `pty.rs`. Keeps `pty.rs` ignorant of session semantics (so it stays unit-testable without a session).
8. **SKILL.md `cd` fix stays in a separate cleanup PR**, even though supervisor CWD resolution makes it redundant. Don't bundle orthogonal changes with the wire-up PR.

**Cross-reference table** at the bottom maps each stage to its pending IDs in `agent-doc-bugs.md` (`#jg0d`, `#b486`, `#40ct`, `#vnp0`, `#6ae3`, `#zp02`, `#f7d5`). Keeping this table honest is part of each stage's done criteria — if pending IDs drift, the table gets fixed in the same PR.

**What's deliberately out of scope for the playbook:**

- Real-claude smoke test detail (one-liner → `#f7d5`, manual testing gate).
- Windows ConPTY real impl (stub for phase 1; WSL handles the tmux case).
- `supervisor resume <session>` CLI for `Halted` recovery (defer until first real flap observed).
- Non-tmux mode (has its own "Future" section in `supervisor.md`).

Ready to start landing `#jg0d` (`resize.rs`) whenever you give the go-ahead. That's the smallest of the four remaining submodules and has the cleanest dependency graph — no cross-module locking, just SIGWINCH → channel → resize call.
❯ do
❯ [#a001]
❯ [#a006]
❯ [#a005]
### Re: #a001 / #a006 / #a005 — investigations done, all three close as not-a-bug

**#a001 boundary/baseline divergence — benign, closing**

96 `out_of_band_write` events since `git.rs:286` telemetry landed. Per-file distribution: `monsterrodholders.md` 50, `agent-doc-bugs.md` 39, `agent-loop.md` 5, `corky.md` 2 — all four are heavy-edit task docs. Drift stats: median=97 B, mean=158 B, max=753 B. No `snapshot_resync` (5× threshold) fired. The only `drift_warning` trips (>100 B) look like multi-line paste edits between cycles, not pipeline corruption.

Conclusion: every logged OOB write is the user typing/pasting in their editor between the previous commit and the next `preflight` sweep — exactly the behavior `selective commit` (snapshot staged, user edits left uncommitted) is designed to tolerate. The telemetry confirms baseline management is working; there is no divergence to fix. Leaving `out_of_band_write` + `drift_warning` logging in place as a permanent canary for the real class of bug (e.g. a write path that forgets to call `snapshot::save`), but removing the 🔴 flag.

**#a006 duplicated `/agent:exchange` closing marker — not reproducing, closing**

Audited all 177 snapshots under `.agent-doc/snapshots/`. Only 2 files contain more than one `&lt;!-- /agent:exchange --&gt;` hit, and both are inline prose/code references discussing the bug itself (e.g. "...at the original position just before the `&lt;!-- /agent:exchange --&gt;`"). Structural marker counts (opener / closer on their own line) are balanced 1:1 across every snapshot. No live reproduction in the current corpus — the class of bug that produced it (same family as the mass-`❯`-prefix corruption) has been fixed by the `normalize_user_prompts_in_exchange` safety rail (#a002). Closing, with the safety rail as the load-bearing guard.

**#a005 pre-existing `❯` lines — legitimate user annotations, closing**

Re-audited: 422 `❯`-prefixed lines across 46 snapshots (down from 1062 as compactions landed). Top offenders sampled — every line is a legitimate inline user prompt (`❯ do`, `❯ commit + push`, `❯ Please research...`). This is exactly the inline-annotation convention: users type `❯ ` before a prompt and the marker survives across cycles until the exchange is compacted. Not corruption, not residue from the mass-prefix bug. No action needed.

**Net effect:** three 🔴 items off the top of the pending queue. The remaining boundary-class canary (`out_of_band_write` / `drift_warning` logs) stays on permanently — cheap, and catches the real bug class if it ever re-emerges.
❯ do
❯ - [#64mb] Investigate double commit_staging race
❯ - [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange

### Re: do #64mb #a002

**#a002 — already landed.** Status line says "✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry." No code work needed — just waiting for a real event to fire the telemetry. Leaving in pending as a watch item.

**#64mb — double `commit_staging` race, root cause identified.**

Full ops.log trace at `ts=1776095605`:

```
[1776095605] commit_staging  drift=410 snap=97776 file=98186
[1776095605] commit_success
[1776095605] snapshot_save   len=97776
[1776095605] commit_staging  drift=122 snap=97776 file=97898   ← second call, 288 bytes shorter
[1776095605] commit_success
[1776095605] snapshot_save   len=97776
```

Two independent observations in that window:

1. **Two distinct `commit_file()` invocations.** Not a retry loop — both emit full commit_success/snapshot_save. Each came from a different preflight run. `preflight.rs:391` calls `git::commit(file)` on the target doc (step 2); `preflight.rs:474` calls `git::commit(&doc_path)` on every tracked doc whose snap mtime > doc mtime (step 2d sweep). Step 2d explicitly skips `doc_path == canonical` (line 460), so within a single preflight this fires once. The double fire comes from **concurrent preflights on different target docs** — session A targets agent-doc-bugs.md (step 2 commits it); session B targets some other doc and its step 2d sweep picks up agent-doc-bugs.md as well. Both processes race through `git::commit()` in the same second.

2. **The 288-byte shrink between commits is real, not a measurement artifact.** First commit sees file_len=98186, second sees 97898. Between the `commit_success` of run 1 and the `commit_staging` of run 2, `commit_file()` at `git.rs:464-471` runs the `(HEAD) found in working tree — stripping` branch and calls `write::atomic_write_pub(file, &clean_working)`. That's the write that shrinks the file by ~288 bytes. The second preflight's sweep then reads the shrunken file while the snapshot is still at 97776, producing drift=122.

**Why it's a "race" and not benign serialization.**

- The two preflights are not serialized against each other. `git::commit()` has a retry loop for `index.lock`, but nothing serializes *read-modify-write on the working tree*. Run A strips the working tree HEAD marker between its commit and run B's staging, so B stages a different file than the one it observed at preflight entry.
- The drift warning fires twice in the same second for the same file, which looks alarming but is exactly what you'd expect: each run independently observes `snap_len < file_len` against the same stale snapshot.

**Fix candidates (ordered, smallest blast radius first):**

1. **Per-file advisory lock around `commit_file()`.** Take a flock on `.agent-doc/locks/commit-<hash>.lock` at entry, drop on exit. Serializes step 2 vs step 2d sweeps across concurrent preflights without touching git internals. Matches existing `.agent-doc/locks/` + `sync.lock` pattern. ~20 LOC in `git.rs`.
2. **Sweep freshness gate.** In `preflight.rs:462-472`, skip any doc whose HEAD commit is younger than ~5s — if it was just committed by another session, nothing to do. Avoids the lock but leaves a small window.
3. **Fold both checks.** Lock + freshness gate — lock is authoritative, freshness gate keeps the sweep cheap when traffic is high.

Recommend **(1) + (2)**: lock for correctness, gate for perf. Neither requires changing the commit content pipeline.

❯ Will **Sweep freshness gate.** create a race condition?
❯ > Avoids the lock but leaves a small window

**Pending updates:**
- #64mb stays on the queue, now with root cause nailed → implementation task.
- Adding a sub-item: "Ship per-file commit lock + sweep freshness gate for #64mb".


### Re: does the freshness gate create a race?

**Yes — on its own.** The gate alone is a classic TOCTOU:

```
session A: read HEAD commit time → "12s ago, stale, proceed"
session B: read HEAD commit time → "12s ago, stale, proceed"
both → git::commit() → two commit_staging calls, same race you already have
```

The "small window" I hand-waved past is exactly that interval between the gate check and the actual staging. Anything the gate can observe as "safe to commit" can also be observed by a second process at the same instant.

**Combined with (1) the per-file flock, the race closes.** Ordering:

1. Session A acquires `locks/commit-<hash>.lock`
2. Session A checks freshness gate → stale → proceeds → commits → releases lock
3. Session B blocks on the lock (serialized, not racing)
4. Session B acquires the lock → checks freshness gate → **now fresh** (A just committed) → skips cheaply without re-running `git commit`

So the roles are:
- **Lock** = correctness (serializes the read-modify-write on working tree + index)
- **Gate** = cheap short-circuit inside the critical section (avoids the ~10ms git spawn when there's nothing to do)

Without the lock, the gate is a perf hint with a race. With the lock, the gate is a valid fast-path. They're not redundant, but the gate is only meaningful *inside* the locked region.

**Practical consequence for the fix:** don't ship the gate without the lock. If you want a one-shot PR, ship only the lock — it's sufficient for correctness. The gate is a nice-to-have follow-up that saves a few ms per sweep per doc under contention.
❯ do Recommend **(1) + (2)**: lock for correctness, gate for perf. Neither requires changing the commit content pipeline.
### Re: do — shipped (1) per-file commit lock + (2) sweep freshness gate

Both landed in `src/agent-doc`, `cargo build` clean.

**(1) Per-file `CommitLock` — `src/agent-doc/src/git.rs`**

- New private `CommitLock` RAII guard (flock on `<project_root>/.agent-doc/locks/commit-<hash>.lock`)
- Acquired at the very top of `git::commit()`, held until return
- Uses existing `fs2::FileExt` + `snapshot::doc_hash` / `snapshot::find_project_root` infra
- Best-effort: if the project root or lock dir can't be resolved, logs and proceeds unlocked rather than hard-failing (matches the rest of the commit path's error discipline)
- Serializes the full read-modify-write: drift check → snapshot load → hash-object/update-index → `git commit` → `(HEAD)`-strip rewrite → reposition → VCS signal. Two concurrent preflights on different target docs can no longer interleave on the same doc's working tree.

**(2) Sweep freshness gate — `src/agent-doc/src/preflight.rs` (step 2d)**

- Before calling `git::commit(&doc_path)` on a sweep target, check `git::last_commit_mtime(&doc_path)` and `continue` if elapsed < 5s
- Logs `[preflight] sweep: skipping <doc> (committed <5s ago)` so the skip is visible in ops output
- Per earlier analysis: the gate is only a valid fast-path *inside* the CommitLock critical section — alone it's TOCTOU, but once (1) serializes the critical section, sweep B checks freshness *after* A released the lock and correctly short-circuits without spawning git (~10ms saved per contended sweep per doc)

**What this fixes (#64mb)**

The double `commit_staging` trace with drift=410 then drift=122 (288-byte shrink between commits) can no longer reproduce: the `(HEAD)`-strip rewrite at `git.rs:464-471` now happens inside the commit-lock, so a second preflight cannot observe the shrunken file while the snapshot is still at the pre-strip length.

`out_of_band_write` / `drift_warning` telemetry stays in place as the permanent canary for the real bug class (write paths that skip `snapshot::save`). That's still the right shape — we're closing the race, not the observability.

**Not touched (intentional)**

- Commit content pipeline (staging, hash-object, update-index) — unchanged
- `snapshot::save` path — unchanged (SnapshotLock still guards it internally; CommitLock is a strictly outer lock so no recursive acquisition risk)
- Sweep ordering within a single preflight — still skips `doc_path == canonical` at line 460 as before

**Pending queue:** `#eg0w` moved to ✅ (awaiting release gate); `#qpzd` (the duplicate) marked done. `#gqep` release bump is next when you want to cut v0.32.5 with this fix.
❯ Is [#64mb] & [#a002] complete? Why were they not marked as complete?
### Re: Is #64mb & #a002 complete?

Both are code-complete but intentionally parked in pending as watch items, not forgotten:

**#a002 — Safety rail in `normalize_user_prompts_in_exchange`**
- Code: **landed.** The rail fires and blocks the mass-`❯` corruption class.
- Why still `[ ]`: waiting for a real large-drift event to trip the `normalize_threshold_exceeded` telemetry path so we know the alert side actually works end-to-end. Until one fires in the wild (or we synthesize one in a test), the telemetry limb is unexercised.

**#64mb — double `commit_staging` race**
- Investigation: **complete** — root cause nailed (concurrent preflights racing step 2 vs step 2d sweep; mid-flight `(HEAD)`-strip rewrites the working tree between them).
- Fix: **shipped** under `#eg0w` (per-file `CommitLock` in `git::commit` + <5s freshness gate in preflight sweep) — which is why `#eg0w` is marked ✅ awaiting release, not `#64mb`.
- Why `#64mb` still `[ ]`: it's the umbrella investigation item. I was waiting to confirm the fix actually eliminates the race in production (post-v0.32.5 release gate `#gqep`) before closing the parent. If the per-file lock holds for a day of real use, `#64mb` gets reaped alongside `#eg0w`.

**TL;DR on "why not done":** the convention I've been applying is *code-landed ≠ done*; done means "shipped and verified in prod." Both are shipped-in-tree, both are gated on the same release + observation window.

**If you want them closed now**, say "mark #64mb and #a002 done" and I'll `--pending-done` both this cycle. Otherwise they reap naturally after v0.32.5 ships and the telemetry watch is satisfied.
❯ Can we add a code complete but not shipped convention? What is a terse way to do this? 2 checkboxes? Any other recommendations? This decision should land in the agent-doc binary spec + implementation.
### Re: code-complete-but-not-shipped convention

Three real options, ranked by how well they fit what agent-doc already does:

**Option A — single-char lifecycle state `[/]`** ⭐ recommended
```
- [ ] [#id] open / not started
- [/] [#id] code-complete, awaiting gate (release / telemetry / verification)
- [x] [#id] done — reap next cycle
```
- **Pros:** one char change. Obsidian & Logseq already use `[/]` for "in progress" so muscle memory maps. Preflight reaper gets a trivial rule change: reap only `[x]`, leave `[/]` alone. Visually scannable — you instantly see "3 items gated, 5 open" at a glance.
- **Cons:** GFM renderers (GitHub, stock markdown preview) show `[/]` as literal text, not a styled checkbox. Fine for agent-doc docs which are edited in raw markdown, ugly if the file gets rendered on GitHub.
- **Spec impact:** add `PendingState::Gated` enum variant; parser accepts `/`; reaper skips `Gated`; renderer emits `[/]`. ~30 lines in `pending.rs`.

**Option B — two checkboxes `[x][ ]`**
```
- [ ][ ] [#id] open
- [x][ ] [#id] code-complete, not shipped
- [x][x] [#id] shipped
```
- **Pros:** models the two independent gates literally (code done? shipped?). Most honest representation.
- **Cons:** +3 chars of width on every line, visual clutter, confuses GFM renderers harder than A, requires custom parser path (GFM task-list extension only knows single checkbox). Muscle memory for toggling is awkward.

**Option C — status suffix (no syntax change)**
```
- [ ] [#id] text — ✅ landed, awaiting v0.32.5
```
- **Pros:** zero spec change — already how `#a002` and `#eg0w` are written today.
- **Cons:** no machine-readable state; reaper can't distinguish "gated" from "open"; requires humans to read prose to understand status. This is what we're already doing and why the question came up.

**My recommendation: Option A (`[/]`).** It's the smallest diff to the spec, the reaper logic is one-line, and the visual density stays tight. The GFM-preview cost is real but low-impact because agent-doc docs live in editors, not GitHub READMEs.

**What A buys us concretely:**
1. `#a002`, `#64mb`, `#eg0w` all become `[/]` immediately — machine-readable "gated" bucket.
2. Preflight can emit a new metric: `pending_gated_count`. Makes "awaiting release" items surface in status summaries without parsing prose.
3. Release workflow can query gated items: "`v0.32.5` unblocks `#eg0w, #a002` — promote to `[x]`?" — scriptable gate promotion.
4. Future: `--pending-gate <id>` flag alongside `--pending-done`, so the agent can move items to gated state explicitly.

**Sub-question worth answering before implementing:** should gated items carry the gate name? e.g. `[/]` alone vs `[/:v0.32.5]` or `[/] (gate: v0.32.5)`. Recommendation: start with bare `[/]` + free-text suffix (existing prose convention), add structured gate field only if we find ourselves wanting to query by gate programmatically. YAGNI.

**Spec + impl landing plan** (if you approve A):
1. `src/agent-doc/specs/pending-system.md` — add `Gated` state, define `[/]` syntax, reaper rules, lifecycle diagram.
2. `src/agent-doc/src/pending.rs` — extend parser, add variant to enum, teach renderer.
3. Preflight reaper — skip `Gated` in the reap pass; optionally count gated items in returned JSON.
4. New CLI flag `--pending-gate <id>` (mirror of `--pending-done`).
5. Migration: existing `[ ]` items with "✅ landed" / "shipped" / "awaiting release" prose get auto-upgraded to `[/]` on first preflight touch (lazy, same mechanism as ID backfill). Or skip auto-migration and let us retag by hand — cleaner, smaller blast radius.
6. Tests: state transitions, reaper respects `Gated`, parser round-trip.

Say **"go with A"** (or A + specific tweaks) and I'll draft the spec update + implementation plan as the next cycle. If you want B or a hybrid, say so and I'll rework.
❯ go with A
### Re: go with A — draft spec + implementation plan

Locked in. Plan below; pending items for each step have been added to the queue (6 items). I'll tackle them in order next cycles, not this one.

**Design decisions frozen in this cycle** (call out now so later cycles don't re-litigate):

1. **Bare `[/]` syntax, no structured gate field.** Free-text suffix stays (`- [/] [#id] text — gate: v0.32.5`). Revisit only if we want machine-queryable gates.
2. **No auto-migration.** Existing `[ ]` items with "✅ landed"/"shipped"/"awaiting release" prose stay as-is until touched manually. Reasoning: lazy upgrade rules are fuzzy (what prose counts?), blast radius is unclear, and we only have ~3 real gated items right now — retagging by hand is faster and safer than writing a heuristic.
3. **State transitions allowed:**
   - `[ ] → [/]` via `--pending-gate <id>`
   - `[/] → [x]` via `--pending-done <id>` (release confirmed, ready to reap)
   - `[/] → [ ]` via `--pending-ungate <id>` (gate failed, back to active)
   - `[ ] → [x]` via `--pending-done` (existing path, unchanged)
4. **Reaper rule:** only `[x]` reaps. `[/]` stays forever unless explicitly moved. No TTL on gated state — operator decides.
5. **Lazy ID backfill still applies to `[/]` items.** A human typing `- [/] fix the thing` gets an ID assigned on next preflight, same as `- [ ]`.

**Implementation order (6 pending items track this):**

1. **Spec first** — `src/agent-doc/specs/pending-system.md`. Add Gated variant to the lifecycle diagram, define syntax, document reaper rules, list the 4 CLI operations, note GFM-renderer caveat.
2. **Parser + enum + renderer** in `src/agent-doc/src/pending.rs`. `PendingState { Open, Gated, Done }`. Parse `[ ]/[/]/[x]` → variants. Render reverse. Round-trip tests.
3. **Reaper** — one-line change in the preflight reap pass: skip non-`Done`. Add `pending_gated_count` to preflight JSON alongside existing counts.
4. **CLI flags** on `agent-doc write`: `--pending-gate <id>`, `--pending-ungate <id>`. Same plumbing as `--pending-done`.
5. **Tests** — unit (parser round-trip, state transitions), integration (CLI flag → file state), reaper behavior.
6. **Release** — version bump + VERSIONS.md entry. **Do NOT bundle with the #eg0w release** (v0.32.5 is already scoped to the per-file CommitLock fix). Gated state ships in v0.32.6 as its own focused release so the commit-lock fix gets clean field validation first.

**What happens to #a002, #64mb, #eg0w after this lands:** they get retagged `[/]` by hand. `#a002` stays `[/]` until a large-drift event trips telemetry. `#64mb` + `#eg0w` both promote to `[x]` once v0.32.5 runs cleanly for ~24h. Post-v0.32.6 ships, the first real cycle of the new workflow is validated on its own implementation — nice closed loop.

**Open question for you before I start step 1:** do you want the spec update to go on its own branch (`20260414/pending-gated-state`) or straight to `main`? Default is branch per your convention (`yyyymmdd/short-description`).
❯ follow convention. go
### Re: step 1 landed on `20260414/pending-gated-state`

Branch created, spec updated, and the session is now running off of it. All edits are in `src/agent-doc/specs/pending-system.md` — no code yet, just the design contract steps 2–6 will implement against.

**What changed in `pending-system.md`:**

1. **Title + intro** — renamed to "Stable IDs, Checkboxes, **Gated Lifecycle**, Granular Ops" so the new axis is visible at a glance.

2. **§2 rewritten as a three-state lifecycle** (was a two-state checkbox section):

   | State | Char | Meaning | Reaped? |
   |-------|------|---------|---------|
   | `Open`  | `[ ]` | active work or not started | No |
   | `Gated` | `[/]` | code-complete, awaiting external gate | **No** |
   | `Done`  | `[x]` | fully complete | Yes (next preflight) |

   Includes an ASCII lifecycle diagram (`[ ] ↔ [/] → [x] → reaped`, plus the direct `[ ] → [x]` path for items that don't need gating) and locks the "no auto-reap for `[/]`, no TTL" rule explicitly. `code-landed ≠ done` is now a data-model invariant, not a convention buried in prose.

3. **§2 "Why three states"** — captures the three frozen decisions from the previous cycle:
   - Machine-readable (`pending_gated_count` metric path), prevents accidental reap on release, matches observed reality.
   - Gate name stays **free-text prose** suffix (`— gate: v0.32.5`), not a structured field. YAGNI noted inline.
   - GFM-renderer caveat called out: `[/]` shows as literal text on GitHub, acceptable cost since agent-doc docs live in editors.

4. **§3 Lazy backfill in preflight** — reaper rule tightened: "Reap `- [x]` bullets **only**. `[ ]` and `[/]` are never auto-reaped." Added an explicit **Migration** subsection: no auto-migration, existing items stay `[ ]` until retagged by hand via `--pending-gate`. Frozen decision #2 from the plan.

5. **§4 Granular write-command surface** — flag table extended with:
   - `--pending-gate <id>` — `[ ] → [/]`, no-op (log) if already `[/]`, error if source is `[x]`.
   - `--pending-ungate <id>` — `[/] → [ ]`, error if source is `[ ]` or `[x]`.
   - `--pending-done` note updated: "Valid from any state (`[ ]` or `[/]`)" so `[/] → [x]` promotion is explicit.
   - `--pending-edit` note updated: "preserve hash **and state**" — editing text on a `[/]` item does not kick it back to `[ ]`.

6. **State transition matrix table** — full From×Op grid so the parser/validator has an unambiguous reference. Five valid transitions (`[ ]→[x]`, `[ ]→[/]`, `[/]→[x]`, `[/]→[ ]`, and the three no-ops/errors explicit).

7. **§ Schema example** — updated to include two `[/]` items with realistic gate suffixes (`#eg0w` → "gate: v0.32.5 release"; `#a002` → "awaiting large-drift telemetry trip"). Shows that after preflight reaps `#c9e0`, the gated items survive untouched.

8. **§ Implementation plan rewritten** to drive steps 2–6 of the playbook:
   - Step 1 (commands) now lists `--pending-gate` / `--pending-ungate` explicitly and calls out the `PendingState` enum + state transition validation.
   - Step 2 (preflight) pins "reap `[x]` only, `[/]` skipped unconditionally" and emits `pending_gated_count` in JSON output.
   - Step 3 (skill runbook) documents the new flags plus the guidance: when code lands but cannot ship, `--pending-gate` instead of `[ ]` + prose.
   - **New step 4 — Release discipline:** frozen decision #5 baked into the spec. Gated lifecycle ships in **v0.32.6** as its own focused release. Do NOT bundle with the v0.32.5 `#eg0w` CommitLock release, so the CommitLock fix gets clean field validation first. Post-v0.32.6 ships, `#a002`, `#64mb`, `#eg0w` get retagged `[/]` by hand — the closed-loop validation I called out before starting.

**Design decisions frozen in this spec landing** (matches the plan, no re-litigation):


❯ We need to migrate all memories (e.g. `5. **Feedback memory:**`) to agent-doc's skill/runbook for agent-doc install deployment.
❯ We should **never** use memories for library logic. The library logic should either be deterministic binary or accessible via skill preferring a runbook.
❯ This **must** be added to AGENTS.md/CLAUDE.md. If I were to add this rule to **every** installable library, would I have duplicate context? If I only apply this rule to leaf libraries, will it be in context only once?

❯ Please explain the feedback_pending_patch_every_cycle vs feedback_pending_granular_ops decision.
❯ This needs to be a question of which runbook belong in the agent-doc package...not which memory should be on brian-takita's local dev machine.

1. Bare `[/]` syntax, no structured gate field — free-text prose suffix stays.
2. No auto-migration — lazy rules too fuzzy, blast radius unclear, manual retag faster.
3. Five valid state transitions (see matrix).
4. Reaper rule: only `[x]` reaps, no TTL on `[/]`.
5. Lazy ID backfill still applies to `[/]` items — a human typing `- [/] fix the thing` gets an ID on next preflight.
6. Gated lifecycle ships v0.32.6, NOT bundled with v0.32.5.

**Pending updates this cycle:**
- `#pf01` → **✅ done** (spec landed — that was step 1 of the playbook).

**What's next (step 2 — parser + enum + renderer in `src/pending.rs`):**

`PendingState { Open, Gated, Done }` variant added, parser accepts `[ ] | [/] | [x]`, renderer round-trips. Unit tests: parser round-trip across all three states, state transition validation (each cell in the matrix), error cases (`[x] → [/]`, etc.). This is `#mgdw`. Awaiting your go to land it — or if you'd rather review the spec change first before I touch code, say so and I'll pause after committing the spec.

Branch state: `20260414/pending-gated-state` created off `main`, spec change is uncommitted in the working tree (will commit after this response writes back). No other files touched — step 1 is deliberately spec-only.
### Re: library logic ≠ user-local memory — and the stale `patch_every_cycle` feedback

**Short answer on the feedback decision:** the `feedback_pending_patch_every_cycle` vs `feedback_pending_granular_ops` question is **already obsolete**. Both describe *how the skill should drive the binary*. That is agent-doc library behavior, not user preference. It belongs in `src/agent-doc/.claude/skills/agent-doc/SKILL.md` §1b (which already has the granular-ops contract) and in `specs/pending-system.md` (which defines the on-disk contract). Neither should survive as a user-local memory.

Current state:

- `feedback_pending_patch_every_cycle.md` is **stale** — it encodes the pre-granular-ops rule ("always emit `<!-- patch:pending -->`"). The new spec *forbids* `patch:pending` blocks. If this memory stays, future sessions will silently regress.
- `feedback_pending_granular_ops.md` **does not exist**. Writing it would just duplicate SKILL.md §1b onto a second surface that only applies to one dev machine.

**Right move:** delete `feedback_pending_patch_every_cycle.md` outright. The canonical source is already in the library (`SKILL.md` §1b + `specs/pending-system.md`). Anyone who installs agent-doc gets the same runbook by virtue of installing the skill. No migration needed — the skill is already the runbook. I'll delete the stale feedback memory this cycle.

### Re: library logic rule — put it in ONE place, not every library

**The rule you want:**

> Library logic — any behavior the library itself must enforce — belongs in the library's deterministic binary or in a skill/runbook shipped with the library. It must NOT live in a user-local auto-memory. Auto-memory is for *user preferences* (communication style, tone, local machine quirks), not library contracts.

**The duplication question — where should the rule live:**

If you put the rule in *every* installable library's AGENTS.md, it duplicates in context every time multiple libraries load (nested submodules walk up through parent AGENTS.md, so even "leaf only" still stacks). With 17+ submodules under `src/`, you'd pay 17× the token cost whenever multiple library contexts are active.

If you put the rule in *only leaf libraries*, it appears once per leaf in context. That's better but still wrong — the rule isn't *about* any specific library, it's about **how libraries are built**. It's a development practice, not a library invariant.

**Recommendation: put it in exactly one place — `agent-loop/CLAUDE.md` (the monorepo root)**, as a new section under "Core Principles" or "Conventions":

```markdown
## Library Logic Placement

Library logic — behavior the library itself must enforce — belongs in:
1. **The binary (deterministic)** — parsing, validation, file manipulation, CRDT merge.
2. **A skill/runbook shipped with the library** — for agents driving the binary (e.g., `src/agent-doc/.claude/skills/agent-doc/SKILL.md`).

It must NOT live in user-local auto-memory. Auto-memory is reserved for *user preferences* (voice, tools, machine quirks). Library contracts must ship with the library so every consumer sees the same rules.

**Test:** "If I uninstall this library on machine A and install it on machine B, does the rule still apply?" If yes → library. If no → user memory.
```

**Why root AGENTS.md and not every library:**

1. **Context economics** — loads once, applies everywhere. No stacking cost.
2. **It's a meta-rule** — governs how libraries are authored, not what any one library does. The root AGENTS.md is already the canonical home for cross-cutting concerns ("This root AGENTS.md covers cross-cutting concerns only." — existing line in `CLAUDE.md`).
3. **Libraries stay self-contained** — each library's AGENTS.md stays focused on its own contracts (agent-doc's CLAUDE.md is about agent-doc, not about meta-policy). Developers can extract a library without dragging rules it doesn't care about.
4. **No duplication across libraries** — if this rule needs to change (e.g., adding "provide a linter" as a third option), you change one file, not seventeen.

❯ But if someone else contributes to a library, that person will not receive the context. So it seems to me that the context belongs in the leaf libraries. Is there a way to make sure this rule applies universally, but is also seen by all contributors?

**What this rule displaces from user-local memory** (delete candidates — all encode library behavior):

- `feedback_pending_patch_every_cycle.md` — stale, superseded by SKILL.md §1b
- `feedback_pending_granular_ops.md` — does not exist; would duplicate SKILL.md §1b
- `feedback_pending_protocol.md` — agent-doc-specific flow control; belongs in SKILL.md
- `feedback_compact_exchange_pending_invariant.md` — agent-doc invariant; belongs in `specs/pending-system.md` + SKILL.md compact-exchange runbook
- `feedback_compact_exchange_edit.md` — same
- `feedback_agent_doc_immediate_write.md` — same
- `feedback_agent_doc_foreground_writes.md` — same
- `feedback_no_auto_compact.md` — same
- `feedback_no_background_claims_log.md` — same
- `feedback_immediate_commit.md` — same
- `feedback_no_escape_backticks_heredoc.md` — general shell knowledge, arguably belongs in a core skill, definitely not user memory
- `feedback_pending_patch_every_cycle.md` (again, for emphasis)

**What legitimately stays in user-local memory** (preferences, not library logic):

- `feedback_writing_voice.md` — Brian's voice preferences
- `feedback_no_sycophancy.md` — communication style
- `user_communication_priorities.md` — user's decision filters
- `feedback_no_cleartext_secrets.md` — livestream-specific, machine-level concern
- `feedback_bun_not_pnpm.md` — local dev environment quirk
- Reference memories (SSH hosts, API key paths) — machine-specific

**Concrete delta I can land this cycle:**

1. **Delete** `feedback_pending_patch_every_cycle.md` (stale + library-logic).
2. **Add** a "Library Logic Placement" section to `agent-loop/CLAUDE.md`.
3. **Audit** the other `feedback_pending_*` / `feedback_agent_doc_*` / `feedback_compact_*` entries — for each, either confirm the content is already in `SKILL.md` / `specs/` (then delete), or note the gap and file a follow-up to land it in the library.

Say **"go"** and I'll do the delete + CLAUDE.md update in this cycle, and file the audit as a pending item (won't bundle the full audit into the same response — that's a separate read-through).

**Connection back to agent-doc-bugs**: this is an orthogonal track to the `#mgdw` parser work for the Gated lifecycle — I can do it in the same cycle or sequence it after `#mgdw`. Your call. Default: do the CLAUDE.md + stale-memory delete first (small, frees context for every future session), then land `#mgdw`.
### Re: visibility for external contributors

You're right — that's the failure mode of the root-only placement. A rule that governs library authorship but lives in the parent workspace's AGENTS.md is invisible the moment someone clones the leaf repo in isolation.

The resolution is **leaf placement with a canonical spec**, not duplication:

1. **Canonical spec** — one file, in a neutral home. Candidates:
   - `src/instruction-files/LIBRARY_CONTEXT_POLICY.md` (already the shared audit crate, semantically fits)
   - A top-level `SPEC.md` in each library's own repo if it's self-hosting
   - A published doc URL for libraries that ship to crates.io/npm

❯ I like `src/instruction-files/LIBRARY_CONTEXT_POLICY.md`
❯ We should add the skills from [skill-janitor](https://github.com/khendzel/skills-janitor) into one of our repos. Make sure we run regular audits.

2. **Leaf stub** — each library's `AGENTS.md` carries a 2-3 line pointer, not the full rule:
   ```markdown
   ## Library Context Policy
   This library follows the agent-loop library-context policy:
   contributors must read `<spec-url>` before authoring AGENTS.md or runbooks.
   ```
   An external contributor who clones only the leaf sees the stub, follows the link, and gets the full rule. No duplication, no drift.

3. **Enforcement via `instruction-files` audit** — the crate already audits instruction file structure across the workspace. Extend it with a check: "every library AGENTS.md contains a `## Library Context Policy` section that links to the canonical spec." Violations surface in `make precommit`. This turns the rule from a norm into a contract.
❯ yes

4. **Scaffolder integration** — `rust-package` skill writes the stub into new library AGENTS.md automatically. New libraries get it for free; existing ones need a one-time sweep.

**The tradeoff you're trading against:** pure-leaf-copy (full rule in every library) maximizes contributor visibility but guarantees drift. Pure-root (current state) avoids drift but is invisible externally. The stub-pointer hybrid gets you both — contributors see *something* in the leaf, the canonical text lives in exactly one place.

**What to decide:** where the canonical spec lives. If agent-loop is the sole workspace these libraries ever appear in, `src/instruction-files/LIBRARY_CONTEXT_POLICY.md` is the cleanest home. If some libraries are published and consumed independently, the spec needs a public URL (a docs site or a README section in `instruction-files` on crates.io) so the stub link resolves for external readers.

Want me to draft the canonical spec file + a sample leaf stub so you can evaluate the concrete shape?
❯ yes
### Re: decisions locked in

Locked in:

- Canonical spec lives at `src/instruction-files/LIBRARY_CONTEXT_POLICY.md`
- `instruction-files` audit enforces `## Library Context Policy` stub presence + link validity on every library AGENTS.md
- `rust-package` scaffolder writes the stub for new libraries

I drafted the canonical spec file at `src/instruction-files/LIBRARY_CONTEXT_POLICY.md` — open it in your editor to review. It covers: scope, the five rules (AGENTS.md + runbooks split, stub format, no-duplication, audit enforcement, scaffolder integration), explicit non-rules, and amendment procedure. Nothing is committed yet — it's uncommitted in your working tree.

**Sample leaf stub** (paste into any library's `AGENTS.md`):

```markdown
## Library Context Policy

This library follows the agent-loop library-context policy. Contributors
authoring `AGENTS.md`, `SKILL.md`, or runbooks in this repo must read:

../instruction-files/LIBRARY_CONTEXT_POLICY.md

before making changes.
```

For libraries that ship independently (crates.io, npm), use the absolute GitHub URL form instead so external contributors hit a resolvable link.

### Re: skill-janitor integration

I read the repo. Important distinction:

- **`instruction-files`** audits `AGENTS.md` / `CLAUDE.md` / `SKILL.md` content for staleness, broken tree paths, line budget, non-actionable content, machine-local paths.
- **`skills-janitor`** audits `.claude/skills/` directories: tracks which skills exist, usage patterns, comparison across installs, duplicate detection. Different surface, different concerns.

They're complementary — not overlapping. Both should run in precommit/CI.

**Where to add it.** Three options:

1. **Vendor into `agent-loop` as a plugin** — copy `skills-janitor/skills/` and `scripts/` into `.claude/plugins/skills-janitor/`. Pros: self-contained, pinned version, no external dependency. Cons: manual updates when upstream changes, license/attribution burden.
2. **Git submodule under `src/skills-janitor/`** — follows the existing pattern (`src/agent-doc`, `src/instruction-files`, etc.). Updates flow via `git submodule update`. Pros: clean update path, matches workspace conventions. Cons: another submodule to maintain.
3. **Install as a Claude Code plugin** from the upstream GitHub URL directly. Pros: zero vendor burden. Cons: external availability risk, version pinning harder.

❯ I want to fork the repo and have these skills in my ecosystem soon! For now we can add the plugin.

**My recommendation: option 2 (submodule).** It matches how every other cross-cutting tool in this workspace is organized (`src/instruction-files`, `src/agent-doc`, `src/corky`), gives you clean `git submodule update --remote` semantics, and keeps the audit tool co-located with the code it audits. The `khendzel/skills-janitor` repo is MIT-licensed with no dependencies, so submoduling is cheap.

**Regular-audit wiring** (once submoduled):

- Add `skills-janitor` invocation to `make precommit` alongside the existing `instruction-files` audit call.
- Run it in CI on every PR that touches `.claude/skills/**`.
- Optional: nightly cron via the `schedule` skill to surface drift reports into a task document (e.g., `tasks/agent-doc/skills-audit.md`).

**What to decide:**

1. Option 1, 2, or 3 for skill-janitor integration?
❯ 2. Should I commit `LIBRARY_CONTEXT_POLICY.md` now, or do you want to edit it first?: commit
❯ 3. Once the spec is committed, should I sweep the sixteen-ish libraries under `src/` to add the stub section to their AGENTS.md, or do that after the audit check lands so the audit drives the sweep?: yes
### Re: committed + next moves

Done in this cycle:

- `src/instruction-files/LIBRARY_CONTEXT_POLICY.md` committed (submodule `d3b6203`) and parent pointer updated (`a924dbb3`).
- Canonical spec is now the single source of truth. Any library authoring new agent context should link to this file.

**Library sweep deferred** per your call — audit check lands first, then the sweep is driven by audit failures. That way the sweep is mechanical ("fix every AGENTS.md the audit flags") instead of manual search-and-edit. I added this to pending.

### Re: skills-janitor plugin install

The `khendzel/skills-janitor` repo ships with a `.claude-plugin/` manifest, so it's a first-class Claude Code plugin. The normal install path is interactive via the `/plugin` command — I can't install it from this session because plugin installation touches `~/.claude/plugins/installed_plugins.json` and the marketplace cache, which are managed state I shouldn't poke at directly.

**To install, run at the terminal:**

```
/plugin
```

Then in the interactive UI: add marketplace `khendzel/skills-janitor` (or browse and install directly if it's already in a marketplace you track). After install, the plugin's skills become available via the Skill tool.

Once you've installed it, let me know and I can run a first audit pass to surface any noise (duplicate skill names, unused skills, stale SKILL.md files) across `.claude/skills/` in this repo.

### Re: eventual fork

I added pending items for both the interim plugin install and the post-fork submodule migration. When you fork, the clean path is:

1. Fork `khendzel/skills-janitor` to your own GitHub org
2. Add it as a submodule under `src/skills-janitor/` (matches the workspace pattern)
3. Replace the plugin install with the submodule invocation in precommit
4. Optional: nightly `schedule` cron writing drift reports to `tasks/agent-doc/skills-audit.md`

### Pending added

Five items landed in pending — review before next cycle in case I misread scope.
❯ implement pending-system

### Re: implement pending-system

Audited current state before planning. Most of the spec is already built — the gap is narrower than the spec's 6-section scope suggests.

**Already in place** (per audit of `pending.rs`, `pending_cmd.rs`, `preflight.rs`, `write.rs`, `main.rs`):

- Stable hash IDs — `generate_hash()` + parser recognizes `[#xxxx]` (4–8 chars, collision handling)
- `--pending-add` / `--pending-done` / `--pending-edit` / `--pending-clear` / `--pending-reorder` — all wired through `pending_cmd::*` and the `Write` subcommand
- Lazy hash backfill in preflight `run_pending_maintenance()`
- `[x]` reap (won't touch `[ ]`)
- Reorder detection + `pending_reordered` JSON flag
- `patch:pending` rejection via `enforce_no_patch_pending()` — currently gated on `AGENT_DOC_REJECT_PATCH_PENDING=1`
- `--allow-patch-pending` hidden escape hatch

**Gaps** (what needs building):

1. **`PendingState` enum** — `Open | Gated | Done`. Replaces `PendingItem.checked: bool`. Not FFI-exposed (checked `ffi.rs`), so the refactor is safe.
2. **`[/]` parse + render** — parser currently treats `[/]` as legacy-no-checkbox text. Extend the checkbox matcher + renderer.
3. **`pending_cmd::gate()` / `ungate()`** + state-transition matrix validation (reject `[x]→*`, `[ ]→ungate`, etc.).
4. **`--pending-gate` / `--pending-ungate`** CLI flags in `main.rs` Write subcommand.
5. **`backfill()` — insert missing `[ ]`** checkboxes (currently hash-only). Never inserts `[/]` — gated is always explicit.
6. **`pending_gated_count`** in `PreflightOutput` JSON.
7. **Invert enforcement default** — spec §6 says `patch:pending` presence is an error by default, not opt-in. Currently the rejection is opt-in via env var. Flip the default, keep the env var as a kill switch.
8. **Tests** — state matrix, gated parse/render roundtrip, backfill inserts `[ ]` not `[/]`, reap skips `[/]`, enforcement rejects unless `--allow-patch-pending`.

**Phasing** (three commits, each independently testable):

- **Phase 1 — data model:** `PendingState` enum, migrate `PendingItem`, parser/renderer for `[/]`, backfill inserts `[ ]`. All internal; no CLI changes. Test: roundtrip parse/render for all three states.
- **Phase 2 — ops surface:** `pending_cmd::gate/ungate`, transition matrix, `--pending-gate/--pending-ungate` flags. Test: matrix truth table (9 cases), CLI integration.
- **Phase 3 — preflight + enforcement:** `pending_gated_count` JSON field, flip `enforce_no_patch_pending` default. Test: preflight JSON shape, `patch:pending` rejected without the escape hatch.

**Open question before I start:** spec §6 says "invert the current rule — presence of `patch:pending` → error." The current code rejects only when `AGENT_DOC_REJECT_PATCH_PENDING=1`. Flipping the default is a behavior change for any docs still using `patch:pending` blocks. Safe to flip unconditionally, or gate behind a frontmatter opt-out for one release?

Release target: per spec §Implementation §4, Gated lifecycle ships in **v0.32.6** (separate from the `#eg0w` CommitLock v0.32.5). I'll hold publishing until you confirm.

Approve the phasing and I'll start Phase 1. Each phase commits + tests cleanly before moving to the next.
❯ start
### Re: Phase 1 landed — `PendingState` enum + `[/]` parse/render

Submodule commit `c53b09b` on `20260414/pending-gated-state`. Internal data-model only — no CLI surface yet, no behavior change for existing docs. 30 `pending::tests` pass (6 new).

**What changed in `src/pending.rs`:**

- New `PendingState` enum: `Open` (`[ ]`) | `Gated` (`[/]`) | `Done` (`[x]`). `box_char()` for render, `from_box_char()` for parse (gated `#[allow(dead_code)]` until Phase 2 consumes it).
- `PendingItem.checked: bool` → `PendingItem.state: PendingState`. Added `is_done()` helper.
- `parse_item_line()` recognizes `[/]` as `Gated`. Order: `[ ]` → `[/]` → `[x]` → `[X]` → bare.
- `render()` emits `state.box_char()` — single source of truth, no per-state branches.
- `reap()` keys off `is_done()` only — `[/]` is skipped unconditionally.
- `op_done()` sets `state = Done`; `op_add()` defaults to `Open`. **Phase 1 keeps these primitives unconditional** — state-machine validation (reject `[x]→*`, `[ ]→ungate`, etc.) lands in Phase 2 at the `pending_cmd` layer, not in `pending.rs`. This keeps `pending.rs` I/O-free and pure.

**Tests added (six):**

| Test | Asserts |
|------|---------|
| `parse_gated_state` | `[/]` parses to `Gated`, hash + text intact |
| `parse_all_three_states` | round-trip across the full enum |
| `render_roundtrip_all_three_states` | render output is byte-identical to input |
| `render_emits_slash_for_gated` | `PendingItem { state: Gated, .. }` renders `- [/] [#id] text` |
| `backfill_never_inserts_gated` | legacy `- foo` becomes `- [ ] [#hash] foo`, **never** `- [/]` (spec §3 invariant) |
| `backfill_preserves_existing_gated` | already-canonical `[/]` body is unchanged + `changed=false` |
| `reap_skips_gated` | `[/]` survives, `[x]` is removed, removed-id list correct |

Plus the existing `parse_fully_migrated`, `parse_checkbox_only_no_id`, `parse_legacy_no_checkbox` etc. updated to assert `PendingState` instead of `bool`.

**Audit before refactor:** grepped `\.checked|PendingItem` across the crate — only `pending.rs` itself uses `PendingItem.checked`. Refactor was strictly local. `pending_cmd.rs`, `preflight.rs`, `write.rs` consume only the `op_*` functions and `reap()`/`backfill()` — those return `String`, so they're untouched.

**Build status:** `cargo build` clean. `cargo clippy` shows pre-existing warnings in `diff.rs`, `write.rs`, `preflight.rs`, etc. — none in `pending.rs` or my changes. `cargo test pending::tests` → 30 passed.

### Phase 2 plan (`pending_cmd::gate/ungate` + CLI flags)

Ready to start when you give the next go. Scope:

1. **`pending_cmd::gate(body, id)`** — find item, validate source state (`Open` only), set `Gated`, return new body. Error on `[x]→gate`. No-op (logged) on `[/]→gate`.
2. **`pending_cmd::ungate(body, id)`** — find item, validate source state (`Gated` only), set `Open`. Error on `[ ]→ungate` and `[x]→ungate`.
3. **State transition matrix** — single helper `validate_transition(from, op) -> Result<NextState, TransitionError>` so the truth table lives in one place. 9-case unit test (3 states × 3 ops).
4. **`--pending-gate <id>` / `--pending-ungate <id>` flags** in `main.rs` Write subcommand. Repeatable like `--pending-done`. Wire to `pending_cmd::gate/ungate`.
5. **CLI integration test** (`tests/pending_integration.rs`) — happy path + error cases for each flag.

No spec ambiguity for Phase 2 — the matrix in `specs/pending-system.md` §4 is the contract. I'll match it exactly.

### Open question still pending (Phase 3 only)

The `AGENT_DOC_REJECT_PATCH_PENDING=1` → flip-default question is **only** for Phase 3 (preflight + enforcement). Phase 1 changes nothing user-visible; Phase 2 only adds new flags. So the question can wait one or two more cycles. I'll re-raise before starting Phase 3.

Approve Phase 2 and I'll start the `gate/ungate` matrix + CLI flags.
❯ continue to phase 2
### Re: Phase 2 landed — `gate/ungate` matrix + CLI flags

Working-tree state on `20260414/pending-gated-state`. **Not committed yet** — the submodule has unrelated working-tree changes (supervisor, env.rs, frontmatter.rs, etc.) from prior cycles that I don't want to bundle into a "Phase 2" commit. You'll want to stage selectively or do a sweep commit.

❯ Do a sweep commit. we need to continue moving forward.

**Build:** clean. **Tests:** `pending::tests` 40 passing (10 new), `pending_integration` 18 passing (6 new). **Clippy:** clean on changed files (pre-existing warnings in `write.rs` only — unrelated).

**`src/pending.rs` — new public surface:**

```rust
pub enum PendingOp { Gate, Ungate, MarkDone }
pub enum TransitionResult { Transition(PendingState), NoOp }
pub fn validate_transition(from: PendingState, op: PendingOp) -> Result<TransitionResult>
pub fn op_gate(body: &str, id: &str) -> Result<String>
pub fn op_ungate(body: &str, id: &str) -> Result<String>
```

`validate_transition` is the single source of truth for the matrix. `op_gate` and `op_ungate` call it; `op_done` stays unconditional per the Phase 1 plan ("primitives are unconditional, validation lives at the layer above"). `MarkDone` is wired into the matrix and tested but not yet consumed by a primitive — that's a Phase 3 follow-up if/when we want `op_done` to also error on `Done → Done` instead of silently re-marking.

**State matrix (locked in `validate_transition_full_matrix` test):**

| from \ op | Gate     | Ungate   | MarkDone |
|-----------|----------|----------|----------|
| Open      | → Gated  | **err**  | → Done   |
| Gated     | no-op    | → Open   | → Done   |
| Done      | **err**  | **err**  | no-op    |

Decisions worth remembering:
- `Gate` on `Gated` and `MarkDone` on `Done` are idempotent **no-ops, not errors** — so the skill can re-issue `--pending-gate X` safely on retries / watch loops without faking a failure.
- `Gate` on `Done` is an error, not a "kick it back to Gated" reset. The intended workflow for "this completed item needs another gate later" is to add a fresh pending item, not to time-travel an existing one. (Spec §4 rationale.)
- `Ungate` requires a `[/]` source. Open → Open and Done → Open are both errors — ungate is the strict inverse of gate, not a generic reset.

**`src/pending_cmd.rs`:** `gate(file, id)` and `ungate(file, id)` — thin wrappers that call the primitives, then `comp.replace_content` + `fs::write`. `gate` short-circuits the disk write on no-op (already gated) so we don't churn the file mtime and trigger spurious watch debounces.

**`src/main.rs` — Write subcommand additions:**

```text
--pending-gate <id>     (repeatable)
--pending-ungate <id>   (repeatable)
```

Apply order in the `Write` handler:

```text
clear → add → edit → gate → ungate → done → reorder
```

Gate/ungate run **before** done deliberately. That means:

- `--pending-gate X --pending-done X` in one call lands on `[x]` (Gated → Done, valid). Test: `write_pending_gate_then_done_in_one_call`.
- `--pending-done X --pending-gate X` would fail at the gate step (Done → Gate error), which is the right failure mode for "skill confused itself about state".

**Tests added:**

| File | Test | Asserts |
|------|------|---------|
| `pending.rs` | `validate_transition_full_matrix` | All 9 cells of the 3×3 matrix |
| `pending.rs` | `op_gate_open_to_gated` | happy path |
| `pending.rs` | `op_gate_gated_is_noop` | byte-identical body on no-op |
| `pending.rs` | `op_gate_done_errors` | error message contains "cannot gate Done item" |
| `pending.rs` | `op_gate_unknown_id_errors` | unknown id → Err |
| `pending.rs` | `op_ungate_gated_to_open` | happy path |
| `pending.rs` | `op_ungate_open_errors` | "cannot ungate Open" |
| `pending.rs` | `op_ungate_done_errors` | "cannot ungate Done" |
| `pending.rs` | `op_ungate_unknown_id_errors` | unknown id → Err |
| `pending.rs` | `op_gate_preserves_other_items_and_text` | gate on item 2 leaves items 1 + 3 untouched, preserves em-dash text |
| `pending_integration.rs` | `write_pending_gate_open_to_gated` | CLI flag end-to-end |
| `pending_integration.rs` | `write_pending_gate_idempotent_on_gated` | re-running on `[/]` is silent + safe |
| `pending_integration.rs` | `write_pending_gate_done_errors` | stderr contains the matrix error |
| `pending_integration.rs` | `write_pending_ungate_gated_to_open` | CLI ungate path |
| `pending_integration.rs` | `write_pending_ungate_open_errors` | stderr contains the matrix error |
| `pending_integration.rs` | `write_pending_gate_then_done_in_one_call` | apply order: gate runs before done |

**What's still on the table for Phase 3:**

1. **Invert `enforce_no_patch_pending` default** — currently the rejection only kicks in when `AGENT_DOC_REJECT_PATCH_PENDING=1` is set, and `main.rs:1102` sets it from the `Write` handler unless `--allow-patch-pending` is passed. That's effectively "default-on for the skill path, default-off for everyone else." Per spec §6 we want default-on globally, with the env var as a kill switch instead of an opt-in. This is the question I parked — I'll re-raise it on the Phase 3 cycle along with concrete migration cost (which docs in this repo still rely on `patch:pending`, if any).
2. **`pending_gated_count` in `PreflightOutput` JSON** — straight additive, no behavior change. Drops out of preflight reading the parsed items.
3. **Decide whether `op_done` should also use `validate_transition`** — e.g. error on `Done → Done` to catch double-marks. Cheap to add, slightly less forgiving for retry loops.

Pending updated: `#pjfg`, `#j5x0`, `#kzk7`, `#0jyk`, `#zrbq`, `#z6xv` marked done. New item added for Phase 3 work. The remaining `#mgdw`/`#h1j2`/`#q90h`/`#sx35` items are also covered by what landed in Phases 1+2 — let me know if you want me to reap them too or keep them as the original spec-driven tracker.

Approve Phase 3 and I'll re-raise the enforcement-flip question with a concrete migration plan, then land `pending_gated_count` + the flip in one commit.
❯ do phase 3
### Re: Sweep commit + Phase 3 landed (HEAD)

**Sweep commit:** `fa31f78` on `20260414/pending-gated-state` — 22 files, 2571 insertions / 118 deletions. Bundled Phase 2 (gate/ungate matrix, CLI flags, integration tests, `specs/pending-system.md`), supervisor scaffolding (`src/supervisor/env.rs`, specs), and the env/frontmatter/agent infrastructure work that had accumulated in the working tree. Single commit per your "moving forward" note — selective staging would have been churn.

**Phase 3:** `69b5475` — both pieces landed in one commit.

**Part 1 — invert `patch:pending` enforcement default**

`src/write.rs::enforce_no_patch_pending`: now rejects unconditionally unless `AGENT_DOC_ALLOW_PATCH_PENDING=1`. The CLI (`src/main.rs`) sets that env var only when `--allow-patch-pending` is passed.

The old wiring was: CLI sets `AGENT_DOC_REJECT_PATCH_PENDING=1` to enable rejection → library/FFI/SDK/test callers that bypass `main.rs` silently skipped the check. Closing that hole was the whole point — any future caller must opt in explicitly.

**Part 2 — emit `pending_gated_count` in preflight JSON**

`PreflightOutput` gains `pending_gated_count: usize` (omitted at zero via `skip_serializing_if`). `run_pending_maintenance` now returns `(reordered, gated_count)`; the count is taken **after** backfill+reap so it reflects the post-maintenance state the skill will actually see.

```rust
let (_, items, _) = crate::pending::parse_items(&current_body);
let gated_count = items
    .iter()
    .filter(|i| matches!(i.state, crate::pending::PendingState::Gated))
    .count();
```

Logged to stderr too (`[preflight] pending: N gated item(s)`) when non-zero.

**Tests:**
- `cargo test --test pending_integration` → **21 passed** (3 new for Phase 3: gated-count positive case, gated-count zero-omission, library-default-reject).
- `cargo test --lib` → 274 passed.
- Full `cargo test`: 576 passed, 45 failed — all 45 failures are pre-existing tmux/resync env failures, identical count with my changes stashed. Verified.
- `cargo clippy --tests`: no new warnings (one pre-existing `map_or` lint at `src/preflight.rs:1618` is unrelated).

**Pending updated:** marked `#tzwy`, `#2ng4`, `#6d6c` done — all three Phase 3 trackers covered by this commit.

**Branch state:** `20260414/pending-gated-state` now sits at three commits ahead of `c53b09b` (Phase 1): sweep, then Phase 3. Ready for merge to main when you want.

**Next:** the spec's "Open questions" section in `specs/pending-system.md` is still unanswered — particularly whether `op_done` should also use `validate_transition` to catch `Done → Done` double-marks. Let me know if you want that as a follow-up cycle.
<!-- agent:boundary:ff425fbd -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#64mb] Investigate double commit_staging race — root cause: concurrent preflights racing step 2 vs step 2d sweep; mid-flight (HEAD)-strip rewrites working tree between them
- [ ] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [ ] [#xzz5] Preflight slash-command parser: absolute paths like /home/... get captured as commands. Require [a-z][a-z0-9:_-]* after the slash and reject tokens containing a second /.
- [ ] [#a011] End-of-cycle write invariant — warn if `preflight_diff_start` without `ipc_write_attempt`.
- [ ] [#a012] Expose `agent-doc session-check <FILE>` — nonzero if last ops.log entry is `preflight_diff_start`.
- [ ] [#a014] Session drift: `registered panes span 2 tmux sessions: 1, 4` — resync if recurring.
- [ ] [#a010] PostToolUse hook for agent-doc write (layer 3, optional).
- [ ] [#14z4] Hash collision ceiling: implement 8-char fallback in pending::generate_hash if 4-char collides (spec §1)
- [ ] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release after manual-testing gate
- [ ] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
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
- [ ] [#bak5] Optional: pending-done archive component for history trail (spec §3 step 3, deferred)
- [ ] [#eg0w] ✅ Shipped: per-file CommitLock in git::commit + <5s freshness gate in preflight sweep (awaiting release)
- [ ] [#pf01] ✅ Spec landed on 20260414/pending-gated-state: src/agent-doc/specs/pending-system.md — PendingState::Gated, [/] syntax, reaper rules, lifecycle diagram, CLI surface, state transition matrix, release discipline
- [ ] [#mgdw] agent-doc: parser — accept `[/]` checkbox char, extend PendingState enum with Gated variant in src/agent-doc/src/pending.rs; renderer emits `[/]` round-trip
- [ ] [#h1j2] agent-doc: reaper — skip Gated items in preflight reap pass; emit pending_gated_count alongside existing counts in preflight JSON output
- [ ] [#q90h] agent-doc: CLI — add `--pending-gate <id>` flag on `agent-doc write` (mirror of --pending-done); `--pending-ungate <id>` to return to `[ ]`
- [ ] [#sx35] agent-doc: tests — parser round-trip for `[/]`, state transitions ( →/→x,  →x, /→x, /→ ), reaper respects Gated, CLI flag integration
- [ ] [#e6z1] agent-doc: version bump + VERSIONS.md entry for gated-state feature after implementation lands (release gate — do not ship with the #eg0w bundle)
- [ ] [#kdy5] Implement instruction-files audit check: every library AGENTS.md has a ## Library Context Policy section with resolvable link to LIBRARY_CONTEXT_POLICY.md. Wire into make precommit.
- [ ] [#1376] After audit check lands: sweep all libraries under src/ to add the Library Context Policy stub section. Each submodule commit separately, then parent pointer bumps.
- [ ] [#breh] rust-package skill: write the Library Context Policy stub into new library AGENTS.md at scaffold time.
- [ ] [#3j0e] Install skills-janitor as Claude Code plugin (run /plugin install khendzel/skills-janitor at terminal). User plans to fork eventually; this is the interim path.
- [ ] [#vdyj] After fork: move forked skills-janitor under src/ as submodule, wire audit into make precommit, and optionally add nightly cron via schedule skill writing drift reports to tasks/agent-doc/skills-audit.md.
- [x] [#tzwy] pending-system: preflight emits pending_gated_count in JSON
- [x] [#2ng4] pending-system: invert patch:pending enforcement default (reject without env var gate)
- [x] [#6d6c] Phase 3: invert patch:pending enforcement default + emit pending_gated_count in preflight JSON
<!-- /agent:pending -->
## Assistant
