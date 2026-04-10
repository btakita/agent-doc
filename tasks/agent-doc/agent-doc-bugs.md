---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.27 — released
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions v0.31.18–v0.31.26. Key decisions and fixes:

**v0.31.18–v0.31.24 (prior summary):**
1. Cross-session swap-pane bug (rescue_from_stash guard), slash command dispatch, partial compact, JB dedup guard.
2. PyPI trusted publisher via GitHub Actions.
3. ❯ prefix normalization (binary-side, append+replace modes); overeager prefix bug fixed (diff snapshot→baseline for true user additions).
4. Release gate rule: pause after cargo install for user confirmation before publishing.
5. Slash command routing: builtin_commands vs slash_commands in preflight JSON. Plugin autocomplete.
6. Git archive (v0.31.23): log/show/diff subcommands; compact auto-tags pre-compact commits.
7. fullContent+patches conflict fixed: only send fullContent when no component patches present.
8. Last-boundary normalization: use LAST boundary for user region detection.
9. normalize_prefix_lines HashSet bug: positional zip comparison.
10. Fence-interior ❯ bug fixed; ~~~ tilde fence gap fixed.
11. Plugin version bump rule: any plugin change requires version bump.
12. Commit-after-write rule: always call agent-doc commit after every agent-doc write.

**v0.31.25 — Token efficiency + snapshot correctness:**
- preflight now diff-only by default: `document` field removed from PreflightOutput entirely. `agent-doc read <FILE>` subcommand added for on-demand HEAD fetch. SKILL.md updated: first cycle only runs `agent-doc read`.
- Stale `content_ours` snapshot bug fixed: all IPC paths (socket + file IPC) now save actual post-write disk state (200ms flush + file read) instead of `content_ours`. Non-IPC paths save `final_content`. Hard guarantee for non-IPC; soft guarantee (reconciles on next cycle) for IPC.
- Ghost-reappearance root cause: `agent-doc commit` stages snapshot content, not file content — when snapshot ≠ file, gap commit regresses HEAD. Fix: snapshot = disk state eliminates gap.
- Stash window non-idle check removed (false positives for intentional backgrounded sessions).
- Duplicate claims check added to `check_layout()` with 4 tests.
- `apply_stream_from_string` dead code removed.
- Preflight token-efficiency tests: `preflight_output_no_document_field`, `preflight_output_no_large_content`.
- v0.31.25 released (crates.io + PyPI + GitHub Release).

**v0.31.26 — IPC improvements + recovery dedup:**
- IPC idempotent apply: `patch_id` UUID added to all 3 IPC payload sites. `--force-disk` writes sentinel to `.agent-doc/claimed-patches/<patch_id>` before deleting stale patch. Plugin checks sentinel in both `processPatchFile` and `handleSocketMessage`. `isPatchAlreadyApplied` moved inside `invokeLater` (closes timing race).
- ACK-with-content sidecar: after successful apply, plugin calls `agent_doc_write_ack_content` FFI → writes `.agent-doc/ack-content/<patch_id>.md`. Binary reads sidecar after ACK; if present uses as snapshot, deletes sidecar. Falls back to 200ms + disk read for older plugins.
- FFI exports: `agent_doc_write_ack_content`, `agent_doc_is_claimed_by_force_disk` (both in `ffi.rs`).
- JB plugin v0.2.54: `IpcPatch.patchId`, `writeAckContent`, `isClaimedByForceDisk` via JNA, `isPatchAlreadyApplied` dedup inside EDT invokeLater.
- Recovery dedup: `recover::run` checks response fingerprint (first 3 non-empty non-marker lines) against document before re-applying. `recover_skips_duplicate_apply` test added.
- `cargo install` → v0.31.26 running locally.

**Open / Pending:**
- Manual verification: confirm ghost-reappearance fixed in next /agent-doc cycle.
- Install plugin v0.2.54 (full uninstall+restart for class structure changes, per runbooks/plugin-install.md).
- Publish v0.31.26: crates.io (`cargo publish`), PyPI (`maturin publish --no-sdist`), GitHub Release (`gh release create v0.31.26 --generate-notes` + prebuilt binary).
- Plan file: `tasks/agent-doc/plan-ipc-improvements.md`.

**Architectural rules established:**
- Snapshot invariant: always save actual post-write disk state, not `content_ours` from baseline.
- FFI-first: new socket/IPC behavior implemented in ffi.rs, called from plugins via JNA.
- Runbook: `runbooks/plugin-install.md` must be consulted for any JB plugin change (version bump, uninstall protocol).

❯ release
### Re: release

v0.31.26 released:

- **Committed:** 7 files (recover.rs, preflight.rs, Cargo.toml, pyproject.toml, VERSIONS.md, SKILL.md, Cargo.lock) pushed to origin/main
- **crates.io:** `agent-doc v0.31.26` published
- **PyPI:** manylinux wheel built and uploaded (`agent_doc-0.31.26-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`)
- **Tag:** `v0.31.26` pushed — CI building GitHub Release with cross-platform binaries
- **cargo install:** v0.31.26 running locally

❯ The agent-loop project is swapping with tmux session 1. We should keep everything on this tmux session 0.
❯ This has been a reoccuring issue. Let's figure out all of the horizontal and vertical workflows that can cause this.
### Re: session drift — tmux session 1 contamination

**Current state:** `%57` (mattpocock.md) is registered in `$1:@13` (session 1). All other panes are in session 0. Config correctly shows `tmux_session = "0"`.

**Root causes — what creates cross-session registrations:**

1. **Start from wrong terminal** (`start.rs`): `sessions::current_pane()` reads `$TMUX_PANE`. If `/agent-doc` runs from a Claude Code terminal attached to session 1, that pane registers to session 1. This is the most likely trigger for mattpocock.md.

2. **`resolve_target_session` fallback + auto-update** (`route.rs:473`): When the configured session is dead or unset, it falls back to `current_tmux_session()` and **writes that back to `config.toml`**. A single route call from a session 1 context silently flips `tmux_session = "1"`, causing all subsequent routing to land there.

3. **`break_pane` fallback in `stash_pane`** (`tmux-router/src/tmux.rs:717`): When `ensure_stash_window(session_0)` fails, it falls back to `break_pane` with no `-t` destination. tmux sends the pane to its current session — which could be session 1 if the caller was cross-session.

4. **`sync.rs` / `parallel.rs` stash with wrong `target_sess`**: If `target_sess` resolves to "1" via the route fallback, stash operations accumulate panes in session 1's stash window.

**Fix path:**

`resync.rs` currently only detects `WrongSession` when `frontmatter.tmux_session` is explicitly set. It should also check registered panes against `config::project_tmux_session()`. Panes in session 1 (while config says session 0) would trigger `WrongSession` → auto-relocated via `join-pane` to session 0. This makes the system self-healing without requiring `tmux_session` frontmatter in every document.

**Immediate action:** Manually relocate `%57` now, or just kill+deregister mattpocock.md and re-run it from a session 0 terminal.
❯ How did the swap to tmux session 1 happen? Why does it keep happening. Can we have a module with a defined inteface to ensure that all pane movement logic is protected by rules...such as not swapping with another tmux session...unless explicitly overridden.
### Re: how it happened + pane policy module

**How `%57` ended up in session 1:**

`start.rs` calls `sessions::current_pane()` which reads `$TMUX_PANE`. At 18:58 when mattpocock.md was claimed, the terminal that ran `/agent-doc` had `TMUX_PANE=%57` — a pane already in `$1:@13`. That environment variable is set by tmux itself; it's whatever pane the shell is running in. The claim permanently bound the document to that session 1 pane.

**Why it keeps recurring — 3 triggers:**

1. **WezTerm tab/window connecting to session 1**: Any shell opened in a session 1 window has `TMUX_PANE` pointing there. Running `/agent-doc` from that shell registers to session 1. No code path prevents this.

2. **`resolve_target_session` auto-update** (`route.rs:473-491`): If `config.toml`'s session is dead or unset, the fallback reads `current_tmux_session()` and **writes it back**. One route call from session 1 silently flips the project to `tmux_session = "1"`, routing all subsequent documents there.

3. **22 unchecked pane move call sites**: `Tmux::join_pane`, `Tmux::swap_pane`, `Tmux::break_pane` — none check whether source and destination are in the same session. A cross-session move goes through silently. There's no audit trail and no refusal.

**`pane_policy` module design:**

Add `tmux-router/src/pane_policy.rs` as a mandatory gateway for all pane movement. Existing `Tmux::join_pane` / `Tmux::swap_pane` become internal; callers use the policy layer.

```rust
// tmux-router/src/pane_policy.rs

pub enum CrossSession {
    /// Default: refuse cross-session moves (returns Err)
    Deny,
    /// Allow with audit log — for intentional relocations
    Allow { reason: &'static str },
}

/// Builder for a single pane move operation.
pub struct PaneMoveOp<'a> {
    pub tmux: &'a Tmux,
    pub src: &'a str,
    pub dst: &'a str,
    pub cross_session: CrossSession,
}

impl<'a> PaneMoveOp<'a> {
    pub fn join(&self, split_flag: &str) -> Result<()> {
        self.check()?;
        self.tmux.join_pane_unchecked(self.src, self.dst, split_flag)
    }

    pub fn swap(&self) -> Result<()> {
        self.check()?;
        self.tmux.swap_pane_unchecked(self.src, self.dst)
    }

    fn check(&self) -> Result<()> {
        let src_sess = self.tmux.pane_session(self.src)?;
        let dst_sess = self.tmux.pane_session(self.dst)?;
        if src_sess == dst_sess {
            return Ok(());
        }
        match self.cross_session {
            CrossSession::Deny => anyhow::bail!(
                "cross-session pane move denied: {} (sess {}) → {} (sess {}); \
                 use CrossSession::Allow{{reason}} to override",
                self.src, src_sess, self.dst, dst_sess
            ),
            CrossSession::Allow { reason } => {
                eprintln!(
                    "[pane_policy] cross-session move: {} ({}) → {} ({}) — reason: {}",
                    self.src, src_sess, self.dst, dst_sess, reason
                );
                Ok(())
            }
        }
    }
}
```

**Migration path:** 22 call sites in agent-doc. The intentional cross-session sites (stash rescue in `resync.rs`, route stash) use `CrossSession::Allow { reason: "relocate to project session" }`. All others use `CrossSession::Deny` by default. Any new cross-session move that hits `Deny` becomes a visible bug, not silent drift.

**Additional guard — `start.rs`:** When registering a new pane, check if `$TMUX_PANE`'s session matches `config::project_tmux_session()`. If not, log a warning and optionally refuse to register (or auto-relocate via `PaneMoveOp` with `Allow`).

❯ do
❯ - [ ] Add `tmux-router/src/pane_policy.rs` — `PaneMoveOp` + `CrossSession` enum
❯ - [ ] Rename `Tmux::join_pane` → `join_pane_unchecked`; `swap_pane` → `swap_pane_unchecked`; add `#[deprecated]` or `pub(crate)` visibility
❯ - [ ] Migrate 22 agent-doc call sites to `PaneMoveOp` (deny by default, explicit allow for 3 intentional cross-session sites)
❯ - [ ] Guard `start.rs` registration: warn when `$TMUX_PANE` session ≠ `project_tmux_session`
❯ - [ ] Guard `resolve_target_session` auto-update: only write config when new session matches existing configured session (prevent session 1 from overwriting session 0)
❯ - [ ] Fix `resync.rs`: detect `WrongSession` against `config::project_tmux_session()` (not only `frontmatter.tmux_session`)
### Re: all 6 tasks done

All implemented and tested. Summary:

**1. `tmux-router/src/pane_policy.rs` — new module**
- `CrossSession` enum: `Deny` (default) | `Allow { reason }`
- `PaneMoveOp<'a>` builder: `.new(tmux, src, dst)`, `.allow_cross_session(reason)`, `.join(flag)`, `.swap()`
- 6 tests: same-session join/swap, cross-session join/swap denied, cross-session join/swap with explicit flag
- All 6 tests pass

**2. `tmux-router/src/lib.rs`** — `pub mod pane_policy; pub use pane_policy::PaneMoveOp;`

**3. `agent-doc/src/sessions.rs`** — `pub use tmux_router::PaneMoveOp;`

**Migrated call sites (7 production `join_pane` sites):**
- `layout.rs:231` → `PaneMoveOp::new(tmux, pane_id, &anchor_pane).join(...)` (Deny)
- `resync.rs:476` → `PaneMoveOp::new(tmux, pane_id, &target).join("-dv")` (Deny)
- `resync.rs:726` → `PaneMoveOp::new(tmux, pane_id, &target).join("-dv")` (Deny)
- `resync.rs:1223` → `.allow_cross_session("relocate WrongSession pane to project session").join("-dh")` (only explicitly cross-session site in agent-doc)
- `sync.rs:249` → `PaneMoveOp::new(tmux, pane, &target).join("-dv")` (Deny)
- `sync.rs:768` → `PaneMoveOp::new(tmux, pane, target).join("-dh")` (Deny)
- `route.rs:366` → `PaneMoveOp::new(tmux, pane_id, target).join("-dh")` (Deny — after stash moved pane to session 0)

**4. `start.rs` guard** — After resolving `pane_id`, checks if `pane_session(pane_id) != project_tmux_session()`. If mismatch, logs:
```
[start] WARNING: pane %57 is in tmux session '1', but project config expects '0'.
This document will be registered to session '1'.
To avoid session drift, run /agent-doc from a terminal in session '0'.
```

**5. `route.rs` `resolve_target_session` guard** — Changed auto-update condition from `configured.as_deref() != Some(&fallback)` to `configured.is_none()`. Now only writes config when no session was previously configured. A dead session 0 config no longer gets silently overwritten to session 1.

**6. `resync.rs` WrongSession** — WrongSession detection now falls back to `config::project_tmux_session()` when `frontmatter.tmux_session` is absent. Panes in session 1 with no frontmatter field will now trigger WrongSession when project config says session 0.

Test results: 758 agent-doc + 85 tmux-router tests, 0 failures.

❯ How can we recover tmux window 2:agent-doc from tmux session 1?
### Re: recovering window 1:2 (agent-doc) to session 0

Three options:

**A — `tmux move-window` (immediate, no rebuild required):**
```bash
tmux move-window -s 1:2 -t 0:
```
Moves the entire session 1 `agent-doc` window (containing `%57` / mattpocock.md) into session 0. All panes survive intact. `%57` registry entry stays valid — pane ID is stable after `move-window`.

**B — `agent-doc resync --fix --session 0` (validates new code path):**
```bash
cargo install --path src/agent-doc && agent-doc resync --fix --session 0
```
With the new binary: `detect_issues` now flags `%57` as `WrongSession` (pane in session 1, config says session 0). `apply_fixes_to_registry` uses `PaneMoveOp::allow_cross_session("relocate WrongSession pane to project session").join("-dh")` to move it. This is the new automated path — good to run before publishing to confirm the cross-session detection + relocation works end-to-end.

**C — `tmux join-pane` (merge into existing window):**
```bash
tmux join-pane -s %57 -t 0:agent-doc
```
Merges `%57` into session 0's agent-doc window alongside `%56`/`%72`. If you want all agent-doc panes in one window rather than two windows, this is the right form.

**Recommendation:** Do B (build + resync) — it validates the pane_policy cross-session path before publishing.

❯ do b
### Re: do b

Option B executed and validated:

**`cargo install --path src/agent-doc`:**
- Built from HEAD (v0.31.26), installed to `~/.cargo/bin/agent-doc` — 1.55s (pre-compiled artifacts cached).

**`agent-doc resync --fix --session 0`:**
```
Fixing 1 issue(s):
[pane_policy] cross-session move: %57 ('1') → %72 ('0') — relocate WrongSession pane to project session
  relocated pane %57 → session '0'
  fixed: tasks/research/mattpocock.md (pane %57) in session '1', expected '0'
Fixed 1 of 1 issue(s).
```

**Confirmation:**
```
tmux list-panes -a | grep %57
0:0.2: [59x15] [history 281/50000, 79322 bytes] %57
```

`%57` (mattpocock.md) is now in tmux session 0. WrongSession detection + PaneMoveOp cross-session relocation is working end-to-end. Ready to publish v0.31.27.

❯ release
### Re: release

v0.31.27 released:

**tmux-router v0.3.10 (prerequisite):**
- `pane_policy.rs` — `PaneMoveOp` + `CrossSession` enum
- `lib.rs` — `pub mod pane_policy; pub use pane_policy::PaneMoveOp`
- Published to crates.io; tag `v0.3.10` pushed

**agent-doc v0.31.27:**
- **Committed:** `Cargo.toml` (0.31.27, tmux-router 0.3.10), `pyproject.toml`, `VERSIONS.md`, layout/resync/route/sessions/start/sync changes; 2 clippy fixes (collapsible_if, redundant_closure)
- **crates.io:** `agent-doc v0.31.27` published
- **PyPI:** manylinux wheel built and uploaded (`agent_doc-0.31.27-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`)
- **Tag:** `v0.31.27` pushed — CI building GitHub Release
- **cargo install:** v0.31.27 running locally

❯ In tasks/software/mise.md, the `❯ How do I configure .mise.toml to read .env with shell expansion. See .env with $(pass ...)` was duplicated in the response patch.
❯ Please find all cases where this can happen...vertically and horizontally.
❯ Please add safeguards against this happening if nondeterminism is the cause.
### Re: exchange duplication + recurring session drift (HEAD)

**Root cause — mise.md duplication:**

The skill's response patch included the user's prompt as the first line of the `<!-- patch:exchange -->` block. Since exchange is `patch=append`, `apply_mode("append", existing, patch_content)` simply concatenates:

```
existing:  "❯ How do I configure...\n"
patch:     "❯ How do I configure...\n\n### Re: ...\n"
result:    "❯ How do I configure...\n❯ How do I configure...\n\n### Re: ...\n"
```

`apply_mode` (template.rs:611) has no deduplication — it's a raw `format!("{}{}", existing, new_content)`. That's the bug.

**All duplication paths — vertical:**

| # | Trigger | Path |
|---|---------|------|
| V1 | **Agent echoes user prompt in patch** | `apply_mode("append")` concatenates existing + patch; duplicate if patch starts with content already at end of component |
| V2 | **`append_with_boundary` misposition** | If boundary is at start of exchange (user typed after it), `append_with_boundary` inserts response BEFORE boundary, keeping user text after; then user text appears twice if patch also includes it |
| V3 | **Streaming checkpoint baseline not re-saved** | Next checkpoint uses stale baseline → `content_ours` re-includes already-written content; 3-way merge may duplicate |
| V4 | **IPC timeout + stale patch apply** | Binary exits 75 (timeout), but plugin still processes the patch file that was written. If binary retries (it doesn't in v0.31.26 — exits), but plugin applies stale patch on restart → double-apply |
| V5 | **CRDT `fullContent` + exchange patch sent together** | Fixed in v0.31.24; but edge case: if `normalize_prefix_lines` is non-empty AND patches are present, `fullContent` is sent. Plugin applies `fullContent` (full replace), then also applies the targeted patch → content added twice. The "only fullContent when no patches" guard prevents this normally. |

**All duplication paths — horizontal:**

| # | Trigger | Path |
|---|---------|------|
| H1 | **Two panes registered for same document** | Both agents claim and write to same exchange; both appends succeed independently |
| H2 | **Session drift → two writers** | A session-1 pane and a session-0 pane both registered for same doc; each write cycle appends |
| H3 | **Recovery re-apply edge case** | `recover::run` fingerprints on first 3 non-empty non-marker lines — a short response or one that starts with a blank line could fool it |

**Proposed safeguard — `dedup_adjacent_exchange_lines` (most targeted fix):**

In `template.rs::apply_patches`, after all patches are applied, scan the exchange component for consecutive identical lines and remove the duplicates. This catches V1/V2 without any prior-knowledge of WHY the duplication happened.

```rust
/// After applying patches, remove consecutive identical lines in exchange component.
/// Prevents agent echoes of user prompts from creating duplicates in append mode.
fn dedup_exchange_adjacent_lines(doc: &str) -> String {
    let Ok(comps) = component::parse(doc) else { return doc.to_string() };
    let Some(exc) = comps.iter().find(|c| c.name == "exchange") else { return doc.to_string() };
    let content = exc.content(doc);
    let mut deduped = String::with_capacity(content.len());
    let mut prev: Option<&str> = None;
    for line in content.lines() {
        // Only dedup non-blank lines (blank lines are intentional separators)
        if !line.trim().is_empty() && prev == Some(line) {
            continue; // skip exact duplicate adjacent line
        }
        deduped.push_str(line);
        deduped.push('\n');
        prev = Some(line); // safe: line lifetime = content lifetime
    }
    if !content.ends_with('\n') && deduped.ends_with('\n') {
        deduped.pop();
    }
    exc.replace_content(doc, &deduped)
}
```

Call after `apply_patches` returns in `run_stream`.

**Additional V1 fix — patch prefix dedup in `apply_mode`:**

```rust
fn apply_mode(mode: &str, existing: &str, new_content: &str) -> String {
    match mode {
        "append" => {
            // Strip prefix of new_content that duplicates the end of existing.
            // Prevents agent echoes of user prompts from duplicating on append.
            let stripped = strip_leading_overlap(existing, new_content);
            format!("{}{}", existing, stripped)
        }
        ...
    }
}

fn strip_leading_overlap(existing: &str, new_content: &str) -> &str {
    // Check if new_content starts with the last non-blank line of existing.
    // If so, strip that line + its trailing newline from new_content.
    let last_nonempty = existing.lines()
        .filter(|l| !l.trim().is_empty())
        .last();
    let Some(last) = last_nonempty else { return new_content };
    let test = format!("{}\n", last);
    if new_content.starts_with(&test) {
        &new_content[test.len()..]
    } else {
        new_content
    }
}
```

**Recurring session drift — auto-relocate in `start.rs`:**

The `start.rs` warning isn't enough — it fired but %77 still registered in session 1. Change from warn-only to **auto-relocate**:

```rust
if let Some(expected_session) = config::project_tmux_session() {
    if let Ok(actual_session) = tmux.pane_session(&pane_id)
        && actual_session != expected_session
    {
        eprintln!("[start] pane {} is in session '{}', expected '{}' — auto-relocating",
            pane_id, actual_session, expected_session);
        // Move the pane to the expected session before registering
        if let Some(anchor) = sessions::any_live_pane_in_session(&expected_session) {
            let _ = PaneMoveOp::new(&tmux, &pane_id, &anchor)
                .allow_cross_session("auto-relocate to project session on start")
                .join("-dh");
        }
    }
}
```

This is the self-healing version of the pane_policy guard — instead of warning after the fact, it relocates silently and registers in the right session.

**Summary — 3 tasks:**
1. `dedup_exchange_adjacent_lines` in `template.rs` after `apply_patches` (V1/V2 fix)
2. `strip_leading_overlap` in `apply_mode("append", ...)` (V1 fix, complementary)  
3. `start.rs` auto-relocate instead of warn-only (H2 recurring drift fix)
<!-- agent:boundary:c7a63649 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Implement `dedup_exchange_adjacent_lines` in `template.rs`
- [ ] Implement `strip_leading_overlap` in `apply_mode` for append mode
- [ ] Change `start.rs` warn → auto-relocate via `PaneMoveOp`
- [ ] Bump version to 0.31.28, update VERSIONS.md, test, publish
<!-- /agent:pending -->
