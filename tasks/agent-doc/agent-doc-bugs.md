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
### Re: `agent_doc_model_tier` as a supervisor operation — yes, and it closes a real gap (HEAD)

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

### Re: Claude Code on-the-fly `! ENVVAR=...` — partial, and not a supported mutation path for session env (HEAD)

Breaking this into three layers because they behave differently:

**1. Bash tool (what I use for every tool call):** Shell state does NOT persist between calls. From my own system prompt verbatim: *"The working directory persists between commands, but shell state does not."* So `export FOO=bar` in one Bash call is gone by the next. Each call re-sources the user's profile. The preflight step 0c2 pattern (export in a single Bash call) only works because subsequent commands *in that same call* see the exports — not subsequent tool calls.

**2. `!` prefix in the Claude Code prompt (user-typed):** This runs the command in a shell the harness manages and pipes output back into the conversation. I'm not 100% certain whether that shell is persistent across `!` invocations — I'd need to test it, and I don't want to speculate. Even if it is persistent, it's a *separate* shell from the Bash tool's shell, so exports there don't reach my tool calls.

**3. The supervised claude pty (the actual target of session env):** This process has its env set **once**, at `fork/exec` time, from whatever `EnvSpec::resolve()` returned. Nothing the user types at the Claude Code prompt — `!` or otherwise — reaches back into that pty's environment. The kernel doesn't expose "mutate another running process's env" as an API (you can `ptrace` it but that's a sledgehammer, and anything the child already read into its own state is untouched).

**What this means for the design:** ad-hoc `export` is not a mutation path for document session env. The supported paths are:

1. Edit frontmatter → save → next restart picks it up. (Explicit via `agent-doc restart`, or implicit via crash-triggered restart.)
2. First spawn after claim: always reads current frontmatter.

Your note is exactly right — *even if* on-the-fly export worked, harness-specific vars (`AGENT_DOC_*`, `CLAUDE_*`, anything the claude binary caches at startup) would still need a restart. So making ad-hoc export a mutation surface would create a two-tier env model (hot-patchable vs. restart-required) with no clear user-visible line between them. That's a confusing API. Better to keep one rule: **frontmatter is the env, restart applies it.** If someone needs a quick throwaway var for one command, they can just `FOO=bar some_command` inline — that's not session env, that's a per-invocation override and doesn't need the supervisor's help.

I'll add a note in the spec doc making this explicit: "Session environment variables are set at spawn time from frontmatter `env:` only. Ad-hoc shell exports do not affect the supervised pty or future Bash tool calls."
<!-- agent:boundary:a1618b25 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#a001] 🔴 Investigate boundary/baseline divergence root cause — `out_of_band_write` always-on log line landed (`git.rs:286`); aggregate after 1 day.
- [ ] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [ ] [#a004] Drop Option B (skill-side submodule workaround) — A handles it deterministically; B redundant.
- [ ] [#a005] Investigate 1062 pre-existing `❯` lines across 46 snapshots — legitimate or prior corruption?
- [ ] [#a006] Investigate duplicated `/agent:exchange` closing marker — likely same class as mass-prefix bug.
- [ ] [#a010] PostToolUse hook for agent-doc write (layer 3, optional).
- [ ] [#a011] End-of-cycle write invariant — warn if `preflight_diff_start` without `ipc_write_attempt`.
- [ ] [#a012] Expose `agent-doc session-check <FILE>` — nonzero if last ops.log entry is `preflight_diff_start`.
- [ ] [#a013] SPEC.md breakup — flat `specs/*.md` mechanical extraction.
- [ ] [#a014] Session drift: `registered panes span 2 tmux sessions: 1, 4` — resync if recurring.
- [ ] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release after manual-testing gate
- [ ] [#14z4] Hash collision ceiling: implement 8-char fallback in pending::generate_hash if 4-char collides (spec §1)
- [ ] [#bak5] Optional: pending-done archive component for history trail (spec §3 step 3, deferred)
- [ ] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [ ] [#4sh0] Add planned-move logging to sync.rs reconciler so ClaimAction→SyncLayout pane swaps are debuggable
- [ ] [#xzz5] Preflight slash-command parser: absolute paths like /home/... get captured as commands. Require [a-z][a-z0-9:_-]* after the slash and reject tokens containing a second /.
- [ ] [#64mb] Investigate double commit_staging race: ops.log at 1776095605 shows two back-to-back commit_staging/out_of_band_write pairs for the same file within the same second.
- [ ] [#tw4a] claim: cd into document's .agent-doc root (nearest git repo) when claiming a pane. Fixes relative-path ambiguity and makes git ops naturally target the correct repo.
- [ ] [#8jzg] claim --isolate: spawn a fresh Claude Code process in the submodule root so CLAUDE.md, memory, and skills load scoped to that repo. Only real fix for cross-project context leakage (mid-session cd does not reload context).
- [ ] [#jg0d] supervisor/resize.rs: SIGWINCH unix path via signal-hook; Windows ConPTY stub for phase 1
- [ ] [#b486] supervisor/state.rs: crash classifier, ring buffer, Healthy->Transient->Flapping->Halted state machine
- [ ] [#40ct] supervisor/ipc.rs: Unix-domain socket accept loop, reuse length-prefixed JSON frame from ipc_socket.rs
- [ ] [#vnp0] supervisor: wire into start.rs (replace existing restart loop, remove #[allow(dead_code)])
- [ ] [#f7d5] supervisor: smoke test — real claude under supervisor in a tmux pane, verify restart + inject + CWD invariant end-to-end
- [ ] [#6ae3] supervisor/pty.rs: wire PtySession into start.rs (phase 1 wire-up stage) — replaces the current Command::status inherit-tty path
- [ ] [#zp02] supervisor/env.rs: wire into start.rs — replace the inline fm.env expansion loop in start.rs:189 with EnvSpec::from_frontmatter(&fm).resolve()?; feed the result into PtySpawnConfig.env once state.rs restart loop is in place
<!-- /agent:pending -->
## Assistant
