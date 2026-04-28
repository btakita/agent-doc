# Versions

agent-doc is alpha software. Expect breaking changes between minor versions.

Use `BREAKING CHANGE:` prefix in version entries to flag incompatible changes.

## Unreleased

- **Codex Ctrl-D clean exits now restart fresh instead of dropping the pane back to the supervisor prompt.** `start.rs` now treats stdin EOF/Ctrl-D on a clean Codex exit as a fresh restart path rather than the old Enter/`q` prompt flow, and the same strategy helper also keeps single failed resume handoffs on the fresh-restart path before escalating to a prompt after repeated failures. This closes the tmux "session crashed" shape where quitting a Codex child with Ctrl-D left the pane looking dead until the user manually re-entered the supervisor loop. Added start-level regression coverage for the new exit-strategy split and updated the supervisor/Codex support docs to match.

- **Live-pane route ownership now falls back to supervisor PID before declaring ambiguity.** `route.rs` still prefers a tmux process-tree match on the document path, but when a registered pane is alive and the long-lived `agent-doc` supervisor no longer exposes that file path in argv, route now queries the per-session supervisor socket for the live child PID and maps that PID back to the owning tmux pane. This closes the JetBrains/IDE reroute shape where a live `agent-doc` pane was refused as "ambiguous" even though the supervisor still owned the document session. Added route regression coverage for recovering the live pane via supervisor PID when argv loses the file path.

- **Failed fresh-route cleanup no longer kills the new live pane.** When route creates and registers a new pane for a document but later fails closed because fresh-start acknowledgment was not observed, `route.rs` now preserves that pane if it is still the live registered owner instead of cleaning it up as an orphan. This keeps `fresh_route_start_missing` / `fresh_route_trigger_missing` from surfacing to the user as a tmux pane crash. Added route coverage for both preserving the registered owner and still cleaning up truly unregistered panes.

- **Resume auto-trigger cancellation now cuts through the shared child-pty writer path.** Supervisor shutdown now flips both the auto-trigger stop flag and the stdin->pty writer stop path before joining either thread, the auto-trigger waits for the shared writer mutex interruptibly, and Unix child-pty writes now poll in short intervals so cancellation can break backpressure instead of hanging behind `stdin->pty`. Added regression coverage for cancelling while the writer lock is busy and updated the supervisor spec to document the shutdown ordering.

- **Resume auto-trigger now proves the prompt from current child PTY output.** The restart watcher no longer decides readiness from `tmux capture-pane` history. It now watches the filtered output emitted by the current resumed child and only injects once the latest non-empty line is a harness prompt, so stale visible prompts left in tmux scrollback cannot trigger an early resume command. Added regression coverage for latest-line prompt detection and updated the supervisor spec/module contract to match.

- **Resume auto-trigger now injects through the child pty instead of pane stdin.** The restart watcher still waits for a visible harness prompt via `tmux capture-pane`, but once the prompt appears it now writes the trigger command directly through the supervisor-owned child pty writer instead of `tmux send-keys`. That closes the `#rvinjectrace` window where a stale watcher could inject into the supervisor restart prompt or a later replacement process after the resumed child died during the trigger handoff. Added regression coverage for carriage-return injection, late cancellation before write, and closed-writer failure during the trigger window.

- **Historical snapshot repair now adopts committed `HEAD` before later local drift.** When `session-check` or `commit` sees that `HEAD` already contains a previously bypassed assistant response, snapshot repair no longer requires the live worktree to be exactly `HEAD` or `HEAD` plus an exchange-only prompt follow-up. It now advances the snapshot to the committed `HEAD` state for any later local drift that does not introduce a newer `### Re:` / `## Assistant` block beyond `HEAD`, then reclassifies the remaining user edits normally. This closes the stale-snapshot/manual-commit `#pbc2` shape where a structurally valid committed response was still misreported as a direct patchback bypass. Added regressions for both `session-check` and `commit` on the committed-head-plus-local-status-edit case.

- **`agent-doc backlog` is now the canonical backlog CLI, with `agent-doc pending` retained as a deprecated alias.** The top-level backlog management subcommand now lives under `agent-doc backlog ...`; invoking the legacy `agent-doc pending ...` spelling still works for compatibility but emits a deprecation warning directing callers to the canonical name. Updated autocomplete command metadata and integration coverage for both the canonical and deprecated spellings.

- **Completed backlog reap now fails closed when persistence is incomplete.** Preflight no longer downgrades reap-persistence problems to a warning: if it removes `- [x]` backlog items from the working tree but cannot verify the same reap in the staged snapshot, the cycle stops before commit instead of silently letting completed items survive. `session-check` now also fails closed when a supposedly clean committed document still contains stale completed backlog items from an older cycle, while still allowing items that were newly marked `--pending-done` in the just-committed cycle to wait for the next preflight reap. Added regression coverage for the happy path, the missing-snapshot-backlog failure, and the post-commit closeout guard.

## 0.33.16

- **Pending add/backlog normalization now fail closed on malformed leading id prefixes.** Active `--pending-add` parsing still accepts canonical `id=<custom> ...` and compatibility `[#custom] ...`, but it now rejects bare `[#]` placeholders, empty `id=` prefixes, and stacked leading prefixes like `[#a] [#b] ...` or `id=a [#b] ...`. The accidental `replace:pending` / `patch:pending` normalization path still repairs a lone legacy `- [ ] [#] ...` line into a generated id, but it now blocks the stacked-prefix shape before any malformed prefix text can persist into backlog content. Added unit coverage for the add-time parser and write-path regression coverage for normalize-vs-reject behavior.

- **Submodule sessions now expose the parent working tree to workspace-write harnesses.** `append_workspace_access_args` no longer limits submodule-hosted Claude/Codex sessions to external git metadata dirs. Fresh launches now also add the superproject working tree as an extra writable root, so a session started in `src/session-share` can still patch parent-repo docs such as shared backlog files without misreporting them as outside the writable root. Existing Codex resume behavior is unchanged: `exec resume` still strips `--add-dir` because the resumed thread inherits those writable roots from the original exec. Added regression coverage for both the computed workspace-access dirs and the actual appended Codex args.

- **Already-committed closeout now blocks bypassed response patchbacks.** When the staged snapshot already matches `HEAD` but the working tree contains a likely direct assistant patchback (`### Re:` / `## Assistant`) with no newer `agent-doc` cycle, `git::commit` now fails closed instead of classifying that state as ordinary post-commit working-tree drift and returning `commit_already_current`. This closes the `#pbypass1` shape where a session doc could show a restored response but stop at "Nothing has been committed," leaving the patchback outside the binary-owned commit boundary. Added regression coverage for the committed-HEAD plus bypassed-response case.

- **Session closeout now fails before commit when completed backlog items omit `--pending-done`.** `write`/`finalize` gained a pre-commit pending-done gate that compares the active response capture against still-open backlog ids and blocks commit when a response clearly completes `#id` but the cycle recorded no matching `--pending-done <id>`. Session documents now default `pending_done_guard` to `strict` unless frontmatter or project config downgrades it, while non-session docs keep the old warn default. Added unit coverage for default/recorded/warn/suppressed paths plus integration coverage proving `finalize` leaves `HEAD` unchanged when the gate trips.

- **Blank `--window` sync scope now fails safe instead of reconciling the whole tmux server.** `sync.rs` now normalizes empty/whitespace-only window overrides to "unset" before repair, auto-start scoping, and `tmux_router::sync`, and `route.rs` ignores blank `context_session` overrides the same way. This closes the tmux-instability path where a JetBrains/plugin sync passed an empty window id, producing `target_window=` / `session=""` reconcile state that detached unrelated live panes into stash and triggered follow-on duplicate starts. Added regression coverage for blank sync/window scope normalization.

- **Stash rescue no longer swaps a live pane out of view.** `route.rs` and `sync.rs` now rescue stashed session panes back into the `agent-doc` window with guarded `join-pane`, placing them on the requested left/right edge instead of preferring `swap-pane`. This closes the remaining `claudescore-3.md` tmux swap/recovery bug where a recovered pane could displace another live pane into stash and only appear to "heal" on a later reroute. Added route/sync regressions that prove the existing visible pane stays in the `agent-doc` window during rescue.

- **Duplicate live `start` now fails closed before spawning a second pane.** `agent-doc start` now checks whether the document session UUID is already registered to another alive tmux pane and refuses to launch a duplicate live harness in the new pane when it is. This closes the `corky.md` restart failure class where the same session id was repeatedly started on `%194/%196/%197/%198/%199`, destabilizing other active panes instead of reusing the already-live session. Added start-level regression coverage for alive/same-pane/dead-pane cases.

- **Already-present recovery closeouts now advance the snapshot before commit.** When a reopened repair/Stop cycle finds that the live document already contains the assistant response but the snapshot still lags behind, `repair` now advances the snapshot and `write_applied` phase before the commit boundary runs. That closes the Codex direct-patch bypass shape where the response was visible in the document, but the later commit path downgraded the turn to post-commit local drift and left it unowned. Added regression coverage for the committed-cycle + direct-patch + already-applied recovery path.

- **Boundary-artifact-only preflight now stays cycle-free.** `preflight` no longer opens `preflight_started` on pure agent-owned `(HEAD)` / boundary churn in template docs. It classifies that shape first, collapses it back to `no_changes` / already-committed closeout, and prevents that transient drift from leaking a stale user-visible lock. Added regression coverage for the exact clean-snapshot plus transient-`(HEAD)` shape that previously surfaced as `cycle started but no write/commit followed`.

- **`compact exchange` write-back now replaces `agent:exchange` for that turn.** When the user-added diff explicitly starts with a direct `compact exchange` directive, template/CRDT write paths now override the normal append mode for `agent:exchange` and force replacement semantics instead. That closes the failure where repeated compaction requests kept appending new checkpoint summaries over older `### Re:` history instead of collapsing the component to one compacted checkpoint. Added directive-detection, template apply, and repair/write regression coverage for both patch-based and raw-response closeouts.

- **Route start-ack now rejects same-cycle committed churn.** `route.rs` no longer treats mutations to an already-committed baseline cycle as proof that a new document cycle started. When a routed or fresh trigger is dispatched against prompt-bearing drift on top of a closed cycle, acknowledgment now requires a genuinely newer cycle id; same-cycle `commit_already_current` updates fail closed instead of logging a false `route_cycle_start_acknowledged`. Added regression coverage for the exact same-cycle false-ack shape.

- **Route/sync now fail closed instead of inventing fallback tmux sessions or force-moving live stash panes.** `route.rs` no longer rewrites `config.toml` when a configured `tmux_session` is dead, refuses auto-start into an implicit dead fallback session like `"claude"`/`"codex"`, and re-registers an already-running pane for the same file before lazy-claim/auto-start. `sync.rs` now preserves stashed panes that belong to another live tmux session instead of moving them across sessions during rescue. Successful replacement paths also preserve prior stash panes unless there is explicit provenance for cleanup. Added regression coverage for dead implicit fallback refusal and non-destructive stash replacement.

- **Live-pane reroutes now require real cycle acknowledgment for pending prompt drift.** `route.rs` now applies the same fail-closed start-ack rule to dispatches into an already-running pane when the document already has unresolved `prompt_target` / `content_edit` drift on top of a closed cycle. A consumed routed trigger no longer counts as success by itself; route waits for a newer per-document cycle state and fails closed if none appears. Added route coverage for both the acknowledged and missing-ack live-pane shapes.

- **Post-commit stale-buffer guard for `codex (HEAD)` drift.** JetBrains post-commit boundary reposition now prefers the just-committed on-disk document when the open buffer differs only by agent-owned `### Re:` heading attribution and/or boundary churn. That prevents the stale-buffer failure where a successful patchback commit was immediately re-dirtied to `codex (HEAD)` with a newer boundary marker. Added JetBrains regression coverage for the prefer-disk decision and Rust closeout coverage that repairs historical heading-attribution drift back to clean `HEAD`.

- **`session-check` now catches startup-miss prompt drift.** When a session document already has unresolved prompt-bearing user edits (`prompt_target` / `content_edit`) relative to its snapshot, but no newer `agent-doc` cycle ever started, `session-check` now fails closed instead of reporting the stale committed state or `no cycle state or ops.log — ok`. The Codex Stop hook inherits that signal and can auto-close the missed-start case from `last_assistant_message` through the normal repair/write/commit path. Added `session_check` and Codex hook regression coverage.

- **Session-document `write --commit` now fails closed.** `write --commit` still behaves as a best-effort helper for non-session docs and `--pending-only`, but when it is writing a response into a real session document (`agent_doc_session` / legacy `session`) it now upgrades to the same strict closeout contract as `finalize`: reject non-git docs before mutation, fail the command on commit failure, and only return success once the cycle reaches `committed`. Added CLI integration coverage for gitless/session, git-backed/session, and non-session best-effort behavior.

- **Normalize accidental pending patches before capture/replay.** When a response still contains a single list-shaped `replace:pending` / `patch:pending` block, the write path now translates it into granular pending mutations before durable capture instead of capturing first and then failing on `replace:pending block forbidden`. That closes the `response_captured` orphan path behind `#pendops`. `repair` replays the same historical capture shape through the same normalization path, while unsupported pending/backlog patch shapes still fail closed before capture. Added live-write and repair regression coverage.

- **Fresh Codex start now requires real cycle acknowledgment.** `route.rs` no longer treats a consumed `agent-doc <file>` trigger as sufficient proof that a fresh pane started successfully. After trigger injection, route now waits for a new per-document cycle state (`preflight_started` or later) before declaring success, logs `fresh_route_start_acknowledged` / `fresh_route_start_missing`, and fails closed if the file never enters a real cycle. Added route unit coverage for fresh-cycle, fast-commit, and timeout shapes. Specs updated to document the stronger startup contract.

- **Fix Codex submodule handoff.** `codex exec resume` does not accept `--add-dir`, but `append_resume_args` was passing it through from `base_args`. The Codex backend now strips `--add-dir` (both `--add-dir <DIR>` and `--add-dir=<DIR>` forms) from resume args. Resumed sessions inherit writable roots from the original `exec`, so stripping is correct behavior. Specs updated to document backend-specific handling.

- **Pending-capture guard now catches single unresolved bug/follow-up prose.** The recommendation heuristic no longer requires a numbered batch when the response clearly identifies a current issue as still needing follow-up (for example, "still hitting the older ... bug that X was meant to close"). Strict `finalize` now blocks those uncaptured single-item responses before commit, and `session-check` warns on the same shape post-commit. Added regression coverage for unresolved-vs-resolved bug prose.

## 0.33.15

- **Supervisor model injection from frontmatter.** `start.rs` now injects `--model` from `claude_model` / `codex_model` / `model` frontmatter when the freeform args (`claude_args`, `agent_args`, etc.) don't already contain `--model`. Precedence: harness-specific field (`claude_model` for Claude, `codex_model` for Codex) > generic `model` field.

- **Pre-commit pending capture gate in `finalize`.** When `pending_capture_guard: strict`, `finalize` scans the response for uncaptured recommendations before committing. If recommendation-like items are detected without `--pending-add` flags, finalize exits non-zero before the commit step.

- **`plan` emits `ExpectAdd` pending mutations.** When prompt targets contain backlog/recommendation signals ("tasks", "todo", "backlog", "what's next", "recommendations", "next steps", "action items"), `plan` emits an `expect_add` entry in `pending_mutations`. Tells the skill that finalize should include `--pending-add` flags for actionable items in the response.

- **Post-preflight planning command.** `agent-doc plan <FILE>` emits a structured planning/dispatch record with `prompt_targets`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers`.

## 0.33.14

- **Inline guard marker stripping.** `strip_guard_markers` now removes `<!-- no-pending-capture -->` and `<!-- no-pending-done-guard -->` from within content lines (not just standalone lines where the entire trimmed line equals the marker). Trailing whitespace is trimmed after removal. Previously, inline markers like `**Bold text** <!-- no-pending-capture -->` survived into committed blobs.

- **Rename `agent:pending` → `agent:backlog`.** The component is now canonically `<!-- agent:backlog -->` with `agent:pending` accepted as a backward-compatible alias. `patch=replace` attribute on backlog/pending tags is deprecated and auto-stripped. Added `agent:icebox` component to template scaffold for parked items.

- **`agent-doc migrate` command.** New subcommand for deprecated component name/attribute migrations (e.g., `pending` → `backlog`).

- **Per-harness model override.** Frontmatter `claude_model` and `codex_model` fields allow different model selections per harness, resolved through the existing tier/config precedence chain.

- **Snapshot auto-migration on document rename.** State files (snapshots, baselines, captures, CRDT) now follow when a document path changes, preventing orphaned state after renames.

- **Pane eviction guard.** `route.rs` now skips tmux pane eviction when an agent process is still active, preventing mid-response pane recycling.

- **Route trigger path resolution.** Trigger paths are now resolved to absolute paths, preventing submodule CWD misrouting when the working directory differs from the document's repo root.

- **Pending-capture heuristic fix.** Detects unconditional follow-up patterns that were false-positive-triggering the recommendation batch guard.

- **Queue component (Phase 1–3).** Parser, data model, template scaffold, preflight integration, trigger resolution, consumption, dispatch, and halt detection for `<!-- agent:queue -->` orchestration.

- **Prompt preset expansion in orchestrate.** Frontmatter `prompt_presets` are now resolved during orchestrate task expansion, and `--plan` flag previews expanded prompts without execution.

- **Post-preflight planning command.** `agent-doc plan <FILE>` emits a structured planning record (prompt targets, repo actions, required commands, pending mutations, blockers, handoff) for the skill to execute against.

- **Compound task steering runbook.** Bundled guidance for normalizing multi-clause directives into explicit sequential steps.

- **Orchestrate synonym dispatch runbook.** Natural-language phrasing like "run these in order" maps to `orchestrate --mode sequential|parallel|dag`.

- **Orphaned supervisor socket GC.** Stale supervisor sockets are cleaned up automatically.

- **IPC snapshot integrity validation.** `start` now validates snapshot integrity before launching the IPC listener.

- **Code formatting cleanup.** Applied rustfmt across 8 source files.

## 0.33.13

- **Workspace-write submodule sessions now auto-add external gitdirs.** When a session document lives in a git submodule, the harness launch path and fresh-agent backends now append `--add-dir` entries for the submodule's external gitdir under the superproject `.git/modules/...` tree plus the superproject `.git` used by parent-pointer updates. That keeps normal workspace-write Claude/Codex sessions from tripping permission failures on submodule commits while preserving the existing arg-precedence chains. Added regression coverage for external-gitdir discovery and for Claude streaming preserving extra `--add-dir` args when switching to `stream-json`.

- **`agent-doc orchestrate` now executes real DAG batches.** The shared orchestration surface still resolves task batches from repeated `--task`, `--from-file`, and `--from-exchange`, but `--mode dag` now parses optional `[id=... after=...]` metadata, falls back to the first `#token` in each prompt as the node id, validates duplicate/missing/cyclic dependencies, and runs the resulting graph in deterministic topological order through the same per-step `inject -> preflight -> fresh agent -> finalize -> session-check` lifecycle. This gives same-document fan-in semantics without pretending concurrent patchback is safe. Added unit coverage for DAG metadata parsing, unknown-dependency and cycle failures, and topological execution order.

- **Legacy `parallel` now routes through the orchestrate dispatcher.** `agent-doc parallel` remains available, but it now forwards its explicit task list into the same `orchestrate --mode parallel` routing layer used by the newer command surface instead of bypassing orchestration entirely. This keeps task normalization and mode dispatch in one place while preserving the existing parallel backend and its empty-task compatibility behavior. Added coverage for shared parallel dispatch and the legacy compatibility path.

- **Compound single-line task steering is now bundled into the skill surface.** The installed skill/runbook now explicitly tells agents to normalize directives like `do #ntoc. Add to today's news. commit + push` into explicit sequential or dependency-ordered steps before execution instead of treating them as one opaque prose task. The command spec now documents that this remains skill-side steering, not binary-owned free-form parsing, and regression coverage locks the new bundled runbook into the installed harness content.

- **Pending ordering guidance now covers late additions from an existing ordered batch.** The bundled skill and `pending-ops.md` runbook still treat front insertion as the default, but now document the exception for follow-on steps: if Step 1 / Step 2 are already captured and you later promote Step 3, add it with a canonical custom id and reorder it into place adjacent to its predecessor in the same cycle instead of prepending it above earlier steps. Added regression checks for the new bundled guidance so the skill surface keeps the `#9pw9`-style placement rule.

- **Skill auto-update now targets the active harness explicitly.** Installed instruction content now renders `agent-doc-version` from `CARGO_PKG_VERSION` instead of inheriting a stale literal from the source template, Codex environment detection now recognizes live Codex shell vars like `CODEX_THREAD_ID` / `CODEX_CI`, and the rendered auto-update step now uses harness-specific install/reload commands (`--harness claude --reload compact` for Claude Code, `--harness codex --reload restart` for Codex). Added regression coverage for the new detection signals plus rendered Codex/Claude auto-update content.

- **Prompt-prefix enforcement now reuses the prompt-bearing classifier.** `write.rs` now treats prompt-prefix targets as a shared binary invariant derived from `diff.rs`'s canonical `prompt_target` classifier instead of relying only on a separate line-shape heuristic, and `session-check` now reports bare prompt-target lines when a bypassed `### Re:`/`## Assistant` patchback left the transcript uncanonicalized. Added unit coverage for prompt-prefix target extraction and the new `session_check` failure shape.

- **Pending-capture guard in `session-check`.** Committed response captures are now scanned for recommendation-like batches (priority labels, numbered action lists, recommendation headers, imperative follow-ups) when the cycle recorded no `--pending-add` / `--pending-add-gated`. Default mode warns on stderr; `pending_capture_guard: strict` or project `[guards] pending_capture = "strict"` upgrades the condition to a nonzero `session-check`, and `<!-- no-pending-capture -->` suppresses the guard for intentional skips. Added heuristic unit coverage plus `session_check` coverage for warn, strict, suppression, and frontmatter-overrides-project precedence.

- **Unified prompt-bearing change classifier.** The diff/prompt contract no longer splits explicit `required response targets` from `inline_annotations`. `diff.rs` now classifies ordered user-authored changes as `prompt_target`, `content_edit`, `recovery_artifact`, or `boundary_artifact`, prompt builders render that typed section directly, and preflight surfaces the canonical list as `prompt_bearing_changes` while keeping `inline_annotations` as a compatibility projection. Added regression coverage for inline prompt promotion, inline correction classification, and response-artifact detection.

- **Committed captures no longer trigger repeat recovery dedup on later preflights.** `repair` now ignores terminal durable-capture states (`committed`, `discarded`) unless there is still a pending response file to reconcile, so routine `preflight` runs stop emitting the "`Response already present in document`" self-heal message after a cycle has already closed cleanly. Added regression coverage for the committed-capture/no-pending shape.

- **Post-commit editor refresh now reuses the committed boundary ID.** Standalone IPC `reposition` messages can carry the exact exchange `boundary_id`, and both editor helpers now preserve that marker instead of minting a new one after `commit()`. This closes the boundary-only dirty-worktree shape where the response was already committed but the editor saved a fresh marker afterward. Added Rust, JetBrains, and VS Code regression coverage for explicit-ID repositioning.

- **Imperative detection now recognizes natural-language pending tasks.** The executable-directive guard no longer stops at hard-coded `do #id` / `run tests` phrases: pending-item prose that starts with an imperative verb (for example `[#n8q4] Fix the cross-repo ...`) is now classified as executable intent too. That means status-only replies like "I'm starting now" are rejected for those diffs instead of letting actionable pending text be misread as non-directive continuation prose. Added unit coverage for diff extraction and finalize integration coverage for the pending-item shape.

- **Delayed recovery patchbacks now keep provenance.** Durable capture records now retain lifecycle timestamps like `replayed_at` and `committed_at`, and `ops.log` emits `capture_committed_after_replay` when a response only reaches the commit boundary after recovery replay. This preserves the distinction between "same-turn patchback succeeded" and "the response was written back later during recovery/closeout" for forensic analysis and user-facing explanations.

- **`commit` now explains post-commit local drift explicitly.** When the stripped snapshot already matches `HEAD` but the working tree still has later local edits, `agent-doc commit` now classifies that state as post-commit local drift, logs whether it was a user follow-up or broader working-tree edits, and closes the cycle without mislabeling the state as a generic out-of-band patchback warning. Added regression coverage for both the safe follow-up and later-local-edit shapes.

- **Stale snapshots can no longer rewind already-committed responses on no-op closeout.** If the snapshot lags behind a response that is already in `HEAD`, and the working tree only adds a new user follow-up on top of that committed state, `agent-doc commit` now repairs the snapshot up to `HEAD` before the `HEAD`-current no-op path runs. This prevents a later closeout from staging the old snapshot blob and momentarily rewinding the document before recovery re-adds the response. Added regression coverage for the exact stale-snapshot + follow-up shape.

- **Relative submodule doc resolution no longer falls through to outer-repo shadows.** When `agent-doc` is invoked from inside a submodule with a relative document path like `tasks/monsterrodholders.md`, path resolution now prefers the caller's existing cwd-local file before consulting the superproject root. This fixes the case where `commit` / `show_head` / related git paths could silently target an outer-repo document with the same relative path, leaving the intended submodule doc uncommitted even though the closeout logged success. Added regression coverage for the shadowed-path shape.

- **Executable-directive backstop in `run` + `finalize`.** The binary now inspects the pending user diff for imperative document directives (`do #id`, `run tests`, `build + install`, `commit + push`, and approval words like `go`) and rejects status-only/meta-only replies unless they include either concrete execution evidence or a concrete blocker. Added unit coverage for directive extraction + response classification and finalize integration coverage for the reject path.

- **Codex closeout contract hardened.** `agent-doc finalize` is now the strict happy path for normal session responses, Codex/direct-exec instructions require an immediate `agent-doc session-check <FILE>` after `finalize` or `write --commit`, and the installed Codex `Stop` hook can auto-close a pending response cycle from `last_assistant_message` before failing closed. Added CLI/integration coverage for the `finalize + session-check` path and the real Codex hook flow.

- **Codex hook state now survives root / turn drift.** The repo-local `UserPromptSubmit` / `Stop` bridge now mirrors active-session state across nested `.agent-doc` roots and still inspects the tracked document on later `Stop` events in the same Codex session, so a closeout cannot be skipped just because the harness CWD moved between the superproject and a submodule or because the next `Stop` arrives with a newer turn id. Added regression coverage for the nested-root replay path.

- **Interrupted-cycle + historical-drift repair.** `preflight` now fails closed on unrecoverable `preflight_started` cycles instead of snapshot-committing over newer live content, while `commit` / `session-check` can narrowly repair already-committed historical `### Re:` drift when `HEAD` proves the response is no longer out-of-band.

- **Bare-path compatibility restored.** `agent-doc <FILE>` once again aliases to `agent-doc run <FILE>`, keeping older wrappers working while the explicit subcommand form remains canonical.

- **Boundary cleanup invariants locked.** Boundary/head-marker cleanup is now regression-covered across the Rust path plus both editor helpers so stale boundary IDs and duplicate visible `(HEAD)` churn do not survive reposition.

- **Repo-scoped commit closeout serialization.** `git::commit()` now keys its advisory closeout lock by the resolved git dir / submodule git dir, blocks for the short critical section instead of proceeding unlocked, and retries the full stage+commit transaction when `index.lock` contention hits `update-index`, `git add`, or `git commit`. Added regression coverage for a staged `index.lock` retry and two different docs contending on closeout in the same repo.

- **`repair` now closes git-backed recovery in one command.** `agent-doc repair` (legacy alias: `recover`) no longer stops after replaying or deduping a pending response; when recovery work happened inside git it now immediately runs the normal commit boundary so repaired assistant content does not remain uncommitted until a later `preflight`. Added regression coverage for both replayed and already-applied repair paths.

## 0.33.12

- **Codex agent backend (Phase 1).** New `agent/codex.rs` implements `Agent` + `StreamingAgent` for the OpenAI Codex CLI. Parses Codex JSONL event stream (`thread.started`, `item.completed`, `turn.completed`). Session resume via `codex exec resume <id>`, fork via `codex exec resume --last`. Registered in `agent::resolve("codex")`. 11 unit tests covering event parsing, session ID propagation, and stream iterator behavior.

## 0.33.11

- **Fix: lib-install uses atomic rename to prevent mmap corruption.** `install_versioned()` in `lib_install.rs` previously used `std::fs::copy(source, &dst)` which overwrites the versioned `.so` in place (same inode). On same-version reinstall during development, this corrupted IDEA's live mmap of the `.so`, triggering a crash. Now copies to a temp file then calls `rename()` — atomic on POSIX, creates a new inode so existing mmaps stay valid. 1 new test: `same_version_reinstall_creates_new_inode`.

## 0.33.10

- **Fix: Component parser peek guard for non-agent HTML comments.** `parse()` in `component.rs` previously consumed any `<!-- ... -->` sequence in document content, causing the close-comment search to eat the next `<!-- /agent:name -->` marker. Now peeks 20 bytes after `<!--` and skips non-agent sequences (advances 1 byte) rather than consuming them. Fixes "unclosed component" errors when pending items contain literal `<!-- ` in their text. 5 new tests.

- **Fix: CRDT stale-base detection uses prefix+suffix.** `merge()` in `crdt.rs` previously only checked `common_prefix_len` to decide if the base was stale. Template documents have structural content (frontmatter, component markers, pending sections) at both ends — a short exchange meant only the prefix went uncounted, causing valid bases to be classified as stale and triggering duplicate-user-prompt bugs. Now computes `ours_shared = (prefix + suffix).min(base_len)` and uses that ratio for the 50% threshold.

- **Cleanup: Remove IPC degraded mode.** `is_ipc_degraded`, `mark_ipc_degraded`, and `clear_ipc_degraded` removed from `write.rs`. The ack-content sidecar mechanism (v0.33.x) made the degraded marker obsolete — sidecar ACK is authoritative; disk fallback handles the timeout path. Replaced with `cleanup_legacy_ipc_degraded` that removes any stale `.agent-doc/ipc-degraded` marker left by older installs.

- **JB plugin 0.2.71: writeAckContent fires on all patch paths.** Previously `writeAckContent` was only called from the VFS patch path; the two exchange-level patch paths omitted it. Now all three paths (WriteCommandAction exchange, VFS exchange, boundary-reposition) call `writeAckContent`, ensuring the ack-content sidecar always fires regardless of which code path processes the patch.

- **Fix: Makefile `test` target unsets git hook env vars.** `make test` now runs `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test`. When the pre-commit hook calls `make precommit`, git sets `GIT_DIR` to the outer repo — all temp-repo tests in the suite inherited this and routed their git subcommands to the wrong repo, causing 24+ test failures during commit. The `env -u` strips the hook vars before cargo test, restoring correct isolation.

## 0.33.9

- **Fix: CommitLock uses try_lock_exclusive to prevent indefinite hang.** `CommitLock::acquire` (git.rs) previously called `fs2::lock_exclusive()` which blocks indefinitely when another process holds the lock. In the IPC-sidecar-timeout fallback path (exit 75), the write to disk succeeded but `git::commit` blocked at the flock — causing the skill process to hang. Changed to `try_lock_exclusive()`: returns `None` immediately when contended, proceeding unlocked. Git's own `index.lock` retry loop (3 attempts with exponential backoff) handles serialization at the git layer.

## 0.33.8

- **Rename debounce (#qam7).** `agent-doc sync --rename` writes a 5s debounce marker (`.agent-doc/rename-debounce/<hash>.marker`) for the focused file; subsequent auto-start checks skip files with active markers. Prevents spurious pane creation when `FileRenameListener` (JB) or `onDidRenameFiles` (VS Code) triggers sync for a file with no alive pane. Both editor plugins now pass `--rename` on file rename/move events. JB plugin 0.2.70, VS Code extension 0.2.7.
- **Auto-start pane ID logging.** `route::provision_pane` now returns `Result<String>` (the new pane ID). Sync logs `[sync] auto-started %XX for <file>` per pane; when >1 pane starts in a single call, a batch summary is printed. Both messages written to `/tmp/agent-doc-sync.log`.
- **Tests + spec.** 5 new tests: 3 rename debounce unit tests, 2 batch summary formatting tests. Spec, contracts, and evals added for both features in `sync.rs`.

## 0.33.7

- **Boundary reposition CAS guard (JB plugin 0.2.68 + VS Code extension).** `repositionBoundaryViaDocument()` in `PatchWatcher.kt` and `repositionBoundaryWithDebounce()` in `extension.ts` now verify the document content is unchanged between the `document.text` read and `document.setText()` / `WorkspaceEdit.apply()`. If the user typed between `await_idle` timeout expiry and the write dispatch, the reposition is silently skipped rather than overwriting the new keystrokes. Adds `repositionBoundaryToEndUtil` / `findCodeBlockRangesUtil` as internal top-level functions (JB) and `repositionBoundaryToEnd` as a vscode-free module (VS Code) for unit testability. New: `RepositionBoundaryTest.kt` (7 cases) and `reposition.test.ts` (5 cases).

- **Skip working-tree boundary reposition when IPC available.** `reposition_boundary_in_snapshot()` in `git.rs` now checks for `.agent-doc/patches/` before touching the working tree. When the IDE plugin is installed (IPC path), the CLI skips the disk-level read-modify-write entirely and relies on the IPC reposition signal — eliminating the TOCTOU race where concurrent user typing could produce duplicate boundary markers in the committed state. New regression tests: `reposition_skips_working_tree_when_ipc_available` and `reposition_updates_working_tree_when_no_ipc`.

## 0.33.6

- **Inline annotation surfacing.** Preflight JSON added `inline_annotations: Vec<String>` as the original surface for user additions (`[user+]`/`[user~]`) inside agent response blocks. In later versions this becomes the compatibility projection of the broader `prompt_bearing_changes` contract.

- **False positive fixes for `inline_annotations`.** Two exclusion rules eliminate boundary artifacts: (1) `[user~]` lines where the only change is appending ` (HEAD)` to a heading are skipped — these are binary reposition artifacts. (2) `[agent]` lines that are component tags (`<!-- ... -->`), section headers (`# ...`), or blank are excluded from the "substantive agent lines after" check — end-of-exchange user input followed only by structural markers is now correctly classified as regular input, not inline annotations.

## 0.33.5

- **FFI library hot-reload (JNA + koffi).** Fixes SIGSEGV crash (PC=0x0) when `cargo install` overwrote `libagent_doc.so` while IDEA held it mmap'd via JNA. Both plugins now stat the `.so` on every `get()` / `ensureLoaded()` call; if mtime changed, they force `Native.unregister` + reload (JNA) or `koffi.unload` + reload (VS Code). One `stat(2)`/`statSync()` per FFI dispatch — negligible overhead. Race window narrows to sub-microsecond.

- **Versioned cdylib install.** `cargo install` / `make install` now writes `libagent_doc-<version>.so` and atomically updates the `libagent_doc.so` symlink via `ln -sfn` + `rename(2)`. The old inode stays alive in any running editor's mmap — editor restarts pick up the new version. Backward-compatible: `agent-doc lib-path` still returns `libagent_doc.so` (now a symlink). Legacy installs (regular file) are upgraded to the symlink layout on first install.

- **Lockfile-tracked GC (`agent-doc gc-libs`).** On JNA/koffi load, plugins write `<so-path>.lock` containing their PID; on clean exit (JVM shutdown hook / VS Code `deactivate()`), they remove the lock. `agent-doc gc-libs` walks all `libagent_doc-*.so` siblings: keeps the current symlink target and any .so whose `.lock` has a live `/proc/<pid>`; unlinks stale .so files and orphaned locks. Triggered on load, on install, and manually. Crash-safe: stale locks from SIGKILL'd processes are cleaned on next sweep.

- **Post-reload version sanity check (JB + VS Code).** After each native library (re)load, both plugins now call `agent_doc_version()` and log `[native] loaded libagent_doc v{version} from {path}` on success. Warns on null return or exception (ABI mismatch). Helps diagnose cases where a reload brings in an incompatible .so.

## 0.33.4

- **SKILL.md § 1b: pending promotion heuristic.** Agents now have an explicit rule: if a response ends with a numbered list of distinct, actionable recommendations and pending is empty (or the user asked for backlog/tasks), each recommendation must be added via `--pending-add` in the same write. Prevents actionable items from being silently lost as prose-only responses.

## 0.33.3

- **IPC sidecar timeout: fall back to disk write instead of claiming success.** `try_ipc()` previously returned `success: true` when the socket acknowledged but the sidecar ack timed out, causing the caller to skip the disk write path. If the plugin didn't actually apply the content, the response was silently lost. Fixed: sidecar timeout now returns `success: false`, so the caller falls through to the CRDT disk write path — the reliable fallback that always works.

- **IPC fallback patch file pre-write.** The disk patch file is now pre-written before socket send (overwriting any stale content) and cleaned on confirmed sidecar success. On sidecar timeout, the file is left for file watcher recovery as an additional safety net. `patch_id` deduplication prevents double-apply.

- **IDE buffer stale fix (JB plugin 0.2.64).** `repositionBoundaryViaDocument()` in `PatchWatcher.kt` now calls `reloadFromDisk(document)` after VFS refresh so the buffer picks up the CRDT-merged content before the boundary is repositioned. Previously the handler read the pre-merge buffer, repositioned the stale content, and wrote it back — burying the agent's response.

- **Runbook: agent-proposed forward actions must be `--pending-add`ed.** `runbooks/pending-ops.md` now requires any response ending with a forward-looking question ("Ready to X?", "Should we A or B?", "Shall I capture Y?") to add each concrete next-step option to `agent:pending` in the same cycle, so the proposal survives user non-reply.

## 0.33.2

- **`agent_doc_resolve_project_path` FFI export.** Editor plugins can now resolve a file's nearest agent-doc project root (the ancestor containing `.agent-doc/`) and the path relative to that root. Fixes a JetBrains plugin bug where `Run Agent Doc` on a file inside a submodule (e.g. `src/session-share/tasks/foo.md`) passed the full monorepo-relative path to the submodule's Claude session, producing `file not found`. Plugins now pass the submodule-relative path (`tasks/foo.md`) and use the submodule root as CWD.

- **IPC timeout path: CRDT merge instead of atomic_write.** The exit(75) fallback now uses the same CRDT merge as the normal disk write path, preserving all concurrent changes (user edits, pending mutations, structural modifications) — not just the `agent:pending` component. Falls back to `splice_pending_component` only if CRDT merge itself fails.

- **Recovery dedup fix.** `is_already_applied()` now checks each fingerprint line individually instead of joining them into a single substring. Fixes false negatives caused by blank-line separation between paragraphs and `(HEAD)` boundary suffixes on headings, which prevented the joined fingerprint from matching.

- **5 new tests** covering nested-submodule resolution, no-ancestor fallback, file-in-root, and recovery dedup with blank lines/boundary markers.

## 0.33.1

- **Pending parse fix: bare `[#]` placeholder accumulation.** `parse_item_line` now strips `[#]` markers instead of prepend-on-backfill, preventing placeholder accumulation across cycles.

- **Pending dedup on `--pending-add`.** `op_add` checks for identical text before appending, preventing duplicate items when the same add is retried.

- **Content-shrink guard for `--stream` writes.** `check_exchange_shrink_guard()` in `write.rs` refuses writes when new exchange content is < 10% of existing length (and existing > 100 bytes). Prevents accidental truncation from malformed heredocs or trivial payloads. Fires in both IPC and disk fallback paths. Overridable with `--force`.

- **9 new tests** for pending parse fixes and shrink guard (5 shrink guard + 4 pending).

## 0.33.0

- **Typed gate markers (`[/release]`, `[/deploy]`, `[/code-review]`, etc.):** Parser recognizes typed gates alongside plain `[/]`. Gate types are alphanumeric with hyphens/underscores, case-insensitive, stored lowercase. State machine: `[/release]` is a refinement of `[/]`; gate type is metadata on `Gated` state, cleared when resolved to `[x]`. Untyped `[/]` items are never touched by `resolve-gate`.

- **Per-file gate commands** (`agent-doc pending <FILE>`): `resolve-gate <type>` finds all `[/<type>]` items and flips to `[x]`. `set-gate-type <id> <type>` transitions `[/]` → `[/release]` (errors if not gated).

- **Project-wide `resolve-gate` command** (`agent-doc resolve-gate <type>`): Scans all `.md` files under project root (or `--scope <dir>`) for items with matching typed gates. Designed for hook integration:
  ```jsonc
  { "match": "cargo publish", "run": "agent-doc resolve-gate release" }
  { "match": "git push",      "run": "agent-doc resolve-gate deploy" }
  ```

- **Write command gate flags:** `--pending-resolve-gate <type>` and `--pending-set-gate-type id=type` for atomic pending+response cycles.

- **`--pending-add-gated` flag:** Add items pre-gated as `[/]` instead of `[ ]`. Available on both `write` and `notify` commands.

- **`--pending-only` flag:** Skip stdin reading and exchange synthesis — only apply pending mutations. Requires at least one `--pending-*` flag; incompatible with `--template`/`--stream`/`--ipc`.

- **`--status` flag on `write`:** Replace the `agent:status` component content inline during a write operation, same pattern as pending ops.

- **`status` submodule (`status_cmd.rs`):** New module for status component manipulation.

- **Notify with pending:** `agent-doc notify` gains `--pending-add`, `--pending-add-gated`, and `--no-create-pending` flags. Message is now optional when `--pending-add` is used.

- **`session clear` subcommand:** Clear the configured tmux session, returning to auto-detect mode.

- **Supervisor PTY module (`supervisor/pty.rs`):** New 526-line module for PTY-based process spawning and management within the supervisor architecture.

- **Start.rs expansion:** Major rework (+627 lines) for improved tmux detection, session routing, and supervisor integration.

- **Debounce simplification:** Removed redundant debounce logic in favor of the consolidated approach.

- **Tests:** 20 new typed-gate tests (parse, render, roundtrip, resolve, set-gate-type, scan, case insensitivity, edge cases). All 1111 tests pass, clippy clean.

## 0.32.5

- **Fix submodule auto-start `file not found` (route.rs `rewrite_start_path`):** When the spawned tmux pane's `cwd` is narrowed to a submodule root (by `git::resolve_pane_cwd`), the `agent-doc start <path>` send-keys invocation now rewrites the caller-supplied super-root-relative `file_path` to be relative to that narrowed `cwd` before composition. Previously a path like `src/session-share/tasks/foo.md` was passed verbatim to a pane already `cd`'d into `src/session-share`, producing `Error: file not found: src/session-share/tasks/foo.md` and blocking auto-claim + auto-start on every submodule-hosted document. Fix lives at a single funnel (`auto_start_in_session`) and also feeds `send_command`'s `/agent-doc <path>` slash command for the same reason. Pure helper `rewrite_start_path(file, cwd, original) -> String` canonicalizes both sides, strips the cwd prefix, and falls back to `original` on any failure (preserves behavior for non-submodule docs, ghost paths, and files outside cwd). Tests: 4 new unit tests (`rewrite_start_path_narrows_to_submodule_relative`, `rewrite_start_path_no_op_when_file_under_cwd_with_same_prefix`, `rewrite_start_path_falls_back_when_canonicalize_fails`, `rewrite_start_path_falls_back_when_file_not_under_cwd`) plus full `route::` suite (43 passing). Forward-compatible with the supervisor track (#jg0d/#b486/#40ct/#vnp0/#6ae3/#zp02/#f7d5) — when `PtySpawnConfig.args` lands, the same helper feeds path rewriting at the new spawn funnel.
- **Binary strips trailing bare `❯` lines from exchange writes (`template::strip_trailing_caret_lines` in `apply_patches_with_overrides`):** The post-patch boundary marker `<!-- agent:boundary:... -->` lands directly after agent content, so a trailing `❯` on its own line becomes a phantom prompt-glyph row above the boundary on every cycle. Agent discipline is the wrong layer — this is now a code-enforced invariant. New pure helper `strip_trailing_caret_lines(content)` collapses all trailing lines whose trim is exactly `❯`; called on `patch.content` when `patch.name == "exchange"` and on unmatched content when it routes to `exchange`/`output` (including the auto-created-exchange path). Non-exchange components are untouched — `❯` in `notes`, `pending`, or user-authored content like `❯ follow-up` is preserved. Tests: 8 new (`strip_trailing_caret_removes_bare_prompt_line`, `_removes_multiple_trailing_lines`, `_preserves_mid_content_caret`, `_preserves_caret_with_text`, `_handles_no_trailing_newline`, `_noop_when_no_caret`, `apply_patches_strips_trailing_caret_from_exchange`, `apply_patches_preserves_caret_in_non_exchange`). Full `template::` suite: 64 passing. See [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md).
- **SKILL.md audit + prune (293 → 112 lines, ~62% cut):** Delegated rarely-consulted workflow detail to runbooks to keep the hot-path instructions tight. New runbooks bundled via `include_str!` in `src/skill.rs::BUNDLED_RUNBOOKS` and installed to `.claude/skills/agent-doc/runbooks/` on `agent-doc skill install`: `model-tier-gate.md` (precedence chain, `required_tier` gate, `model_switch` ack — was SKILL §0c), `streaming-checkpoints.md` (when/how to flush, baseline re-save pattern — was a §1 sub-section), `document-format.md` (frontmatter fields, inline vs template mode, `<!-- agent:name -->` component conventions + inline attributes + snapshot storage — was §Document Format + §Snapshot Storage), and `code-enforced-directives.md` (promoted from project-local into the bundled set). Removed from SKILL.md: the `❯`-rule paragraph (now binary-enforced, see above), the verbose preflight JSON schema code block (the agent parses the real output), the duplicated baseline/write-back instructions between §2a and §2b, the per-mode split between append and template (unified into a single write-back block), and `## Snapshot Storage`. Preserved verbatim (hot-path on every cycle): invocation + subcommand detection, preflight call + `no_changes`/`claims`/`baseline_file` handling, slash-command dispatch via `Skill` tool, `### Re:` header rule + model attribution, pending granular-ops 3-line summary, `--stream` write-back + immediate `agent-doc commit`, and the `IMPORTANT: Do NOT use Edit tool` guard. Memory cleanup: `feedback_no_trailing_prompt_glyph.md` deleted from `~/.claude/projects/-home-brian-work-btakita-agent-loop/memory/` and its `MEMORY.md` index line removed — the rule is now a binary invariant, not a per-agent memory.

## 0.32.4

- **Pending gated-state `[/]` (#pf01, #mgdw, #h1j2, #q90h, #sx35):** New `PendingState::Gated` variant for pending items that are code-complete but awaiting an external gate (release, telemetry, field validation). Rendered as `- [/] [#id] text` in the pending component. Never auto-reaped — only `- [x]` items are reaped by preflight. Spec: `src/agent-doc/specs/pending-system.md` — includes the full state-transition matrix (§4), lifecycle diagram, and reaper rules. State machine: `Open → Gated` via `gate`, `Gated → Open` via `ungate`, `Open|Gated → Done` via `mark-done`. Illegal transitions (`ungate` from `Open`/`Done`, `gate` from `Done`) return errors; idempotent transitions (`Gate` on `Gated`, `MarkDone` on `Done`) are no-ops. Parser: `pending::parse_item_line` accepts `[ ]` / `[/]` / `[x]` / `[X]`; `PendingItem::render` round-trips. CLI: `agent-doc write --pending-gate <id>` and `--pending-ungate <id>` flags on the `write` subcommand, combinable with `--pending-add` / `--pending-done` / `--pending-edit` / `--pending-reorder` in a single call (gate/ungate run before done so `--pending-gate X --pending-done X` promotes through `Open → Gated → Done` atomically). Preflight: emits `pending_gated_count: N` in the JSON output when at least one item is gated (omitted when zero to keep happy-path output compact), alongside the existing `pending_reordered` signal. Reaper: preflight's reap pass skips `Gated` items unchanged. Tests: `tests/pending_integration.rs` covers parser round-trip for `[/]`, all valid/invalid state transitions, reaper respects `Gated`, CLI flag integration (`write_pending_gate_open_to_gated`, `write_pending_gate_idempotent_on_gated`, `write_pending_gate_done_errors`, `write_pending_gate_then_done_in_one_call`, `preflight_emits_pending_gated_count`, `preflight_omits_pending_gated_count_when_zero`). Rationale: previously, long-lived release-gated tasks had no lexical distinction from active work — they either sat in `[ ]` and competed for attention, or got prematurely `[x]`-marked and reaped before the gate actually cleared. The `[/]` character was chosen for visual distinctness from `[ ]`/`[x]` and because it's already in GFM-task-list parser tolerance ranges across common editors.

- **Rename `patch:pending` → `replace:pending` (#25ag):** The full-replacement block syntax for the `pending` component is renamed from `<!-- patch:pending -->...<!-- /patch:pending -->` to `<!-- replace:pending -->...<!-- /replace:pending -->`. The `replace:` prefix signals full-replacement semantics explicitly (all other `patch:<name>` blocks are component-scoped patches; pending uniquely replaces the whole list). Corresponding renames: `--allow-patch-pending` → `--allow-replace-pending` (CLI flag), `AGENT_DOC_ALLOW_PATCH_PENDING` → `AGENT_DOC_ALLOW_REPLACE_PENDING` (env var). Dual-accept migration: the deprecated `patch:pending` form, `--allow-patch-pending` flag (via clap alias), and legacy env var all continue to work for one release. The parser emits a stderr deprecation warning on every `patch:pending` block so callers can find and update their usage. The default-reject gate applies to both forms — enforcement recognizes `name == "pending"` regardless of which prefix opened the block. Rationale: the `replace:` prefix is a higher-signal warning to human readers that this block clobbers a list the user is actively editing, reducing the silent-data-loss failure mode that `patch:` understates. Tests: `write_rejects_replace_pending_block`, `write_rejects_legacy_patch_pending_block` (covers deprecation warning), `write_allows_replace_pending_with_escape_hatch`, `write_allows_legacy_patch_pending_with_legacy_flag`, `write_allows_replace_pending_with_legacy_env_var`, `write_rejects_replace_pending_via_library_default`. **Next release removes dual-accept:** `patch:pending` will become a hard error; update any remaining call sites now.

## 0.32.3

- **Fix: Submodule-aware git commit routing** — Files inside git submodules (`src/boost-client/tasks/*.md`, `src/session-share/tasks/*.md`) previously caused `fatal: Pathspec '...' is in submodule '...'` errors during `agent-doc commit` (preflight sweep and session-final commits). Root cause: parent-level git operations tried to stage submodule-relative paths directly in the parent index. Fix: Added `narrow_to_submodule(super_root, file) -> (PathBuf, bool)` which detects submodule boundaries. When a file is in a submodule, all git staging/commit ops (`hash-object`, `update-index`, `commit`) run inside the submodule's repo with submodule-relative paths. After commit succeeds, `update_parent_submodule_pointer()` updates the parent's submodule pointer in a separate partial commit. Tests: `narrow_to_submodule_returns_super_root_for_non_submodule_file`, `commit_in_submodule_routes_through_submodule_repo` (integration test with actual `git submodule add` sandboxing). Live verification: Two separate submodule documents (`src/session-share/tasks/claudescore.md`, `src/boost-client/tasks/monsterrodholders.md`) now commit cleanly with zero `fatal:` lines.

- **Feature: `out_of_band_write` always-on forensic logging** — Added unconditional log emission when a file's on-disk size diverges from the last snapshot, regardless of threshold. Previously, only divergences >100 bytes emitted human warnings; now all out-of-band writes emit a structured ops.log entry: `out_of_band_write file=<path> drift=<bytes> snap_len=<N> file_len=<N>`. This enables downstream analysis (aggregation, correlation with concurrent operations, drift pattern classification) without requiring the safety rail to trip (which only fires at catastrophic thresholds). Helps root-cause the recurring 135-byte snapshot-vs-file gaps observed in monsterrodholders and other in-flight sessions.

- **Feature: Safety rail with forensic logging in `normalize_user_prompts_in_exchange`** — When a user's added content (between snapshots) contains escaped newlines or other encodings that decompose during normalization, the normalization logic could diverge from the user's source. Added: (1) `normalize_threshold_exceeded` detection when decomposition deltas exceed a configurable threshold (default 500 bytes), (2) forensic logging of applied normalization counts and byte deltas, (3) automatic git commit with diagnostic context if threshold trips. Log schema: `normalize_user_prompts snap_len=<N> base_len=<N> applied=<count>` (fires on every write, no threshold), plus `normalize_threshold_exceeded file=... delta=... snap_len=... base_len=...` (fires if `delta > threshold`). Enables early detection of corruption patterns in heterogeneous editor environments (mixed CRLF, smart quotes, etc.). See ops.log for real-world drift data.

## 0.32.2

- **Feature: `env` frontmatter for per-document environment configuration:** Documents can now declare environment variables in YAML frontmatter that apply to all Bash tool calls and Claude spawns within that session. Syntax:
  ```yaml
  env:
    OPENROUTER_API_KEY: "$(passage btak/OPENROUTER_API_KEY)"
    ANTHROPIC_BASE_URL: "https://openrouter.ai/api"
    ANTHROPIC_AUTH_TOKEN: "$OPENROUTER_API_KEY"
    ANTHROPIC_MODEL: "qwen/qwen3.6-plus"
  ```
- **Shell expansion support:** Environment variable values support shell expansion (`$(command)`, `$VAR`, `${VAR}`). Cross-references work (later vars can reference earlier ones). Values are expanded at runtime; expanded secrets never appear in JSON output or logs.
- **Coverage across all paths:** Env vars apply to:
  - Interactive Claude sessions started via `agent-doc start <FILE>` (via `cmd.env()` on spawned process)
  - Non-streaming submits via `agent-doc run` (via `Claude::with_env()`)
  - Streaming submits via `agent-doc stream` (via `StreamingAgent::send_streaming()`)
  - Parallel fan-out (via unexpanded shell exports in tmux send-keys, so target shell handles expansion safely)
  - `/agent-doc` skill in existing sessions (preflight JSON returns unexpanded values; skill runs `export` in Bash)
- **Preflight JSON field:** `"env": {"KEY": "unexpanded_shell_expr"}` — skill exports these unexpanded so secret expansion happens inside the Bash call, never in JSON output.
- **New module `src/env.rs`:** 
  - `expand_values(env)` — expands all vars through the shell (used by start/run/stream paths)
  - `shell_export_prefix(env)` — builds `export K="V" && ...` string with unexpanded values (used by parallel path)
- **Tests added:** 42 existing tests + 8 new env tests covering plain values, shell expansion, cross-references, empty env, and safe quoting in send-keys commands. All 72 tests passing.
- **SKILL.md step 0c2:** Skill now exports env vars from preflight JSON into the shell before tool calls.

## 0.32.1

- **Fix: CRDT state not refreshed after `agent-doc compact`:** When a template-mode document with CRDT write strategy ran `compact`, the binary correctly rewrote the file and snapshot on disk, but the CRDT state in `.agent-doc/crdt/<hash>.yrs` was stale. On the next `agent-doc write` or `stream`, the 3-way merge loaded the stale CRDT (containing pre-compact exchange AND pre-compact pending), causing non-target components (like `agent:pending`) to be clobbered by old CRDT view of pending items. Fix: After `run_component_compact` or `run_component_compact_partial`, when `is_crdt`, refresh CRDT state by creating a new `CrdtDoc` from the post-compact content and saving it to `.agent-doc/crdt/<hash>.yrs`. This resets the CRDT to a fresh state, discarding pre-compact history (appropriate since compact is a "new epoch" operation).
- **Runbook hardened:** `.claude/skills/agent-doc/runbooks/compact-exchange.md` now explicitly forbids mutations to non-target components. Added Safety Invariants section and pre/post verification steps using git snapshots.
- **Tests added:** `crdt_compact_preserves_pending_with_state_refresh` (verifies fix), `compact_preserves_boundary_marker` (tests ❯ preservation in non-target component), `compact_working_tree_consistency` (disk/snapshot consistency).

## 0.32.0

- **Fix: Submodule-aware patch routing:** `try_ipc()` and `try_ipc_full_content()` in `write.rs` now use `git::resolve_to_git_root()` to detect submodule context. When a session document lives inside a git submodule, IPC patches are routed to the **superproject's** `.agent-doc/patches/` directory instead of the submodule's local `.agent-doc/patches/`. Previously, patches written to submodule documents (e.g. `src/session-share/tasks/claudescore.md`) would land in `<submodule>/.agent-doc/patches/` where the JetBrains plugin (which only watches the parent repo) never saw them. The fix falls back to `find_project_root()` if git resolution fails, preserving backward compatibility for non-git and non-submodule cases.
- **Tests added:** `try_ipc_routes_to_superproject_when_available` (creates a real git submodule structure and verifies patches route to parent), `try_ipc_falls_back_to_find_project_root_when_not_in_git` (fallback behavior), and `test_submodule_write_patches_dir_structure` (integration-level directory layout validation).

- **Feature: Harness-agnostic model tier selection:** New `model_tier` module defines a `Tier` enum (`auto | low | med | high`) and composes an `effective_tier` from four sources, highest precedence first:
  1. Inline `/model <x>` command in the diff (stripped from downstream diff/classifier)
  2. `<!-- agent:model -->` component content
  3. `agent_doc_model_tier` frontmatter field
  4. Diff heuristic (`suggested_tier`) based on `diff_type` + document path
- **Config: `[model.tiers.<harness>]` maps** let users customize tier→model mappings per harness (`claude-code`, `codex`, `default`). Built-in defaults: claude-code → haiku/sonnet/opus, codex → gpt-4o-mini/gpt-4o/o3.
- **Harness detection:** `detect_harness()` checks `CLAUDE_CODE_SESSION` / `CLAUDECODE` / `CODEX_SESSION` env vars and returns `claude-code | codex | default`.
- **Preflight JSON additions:** `effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier` fields.
- **Diff scanner strips `/model` lines:** `scan_model_switch` runs before classification, so downstream classifier/slash-command parser never see `/model`.
- **SKILL.md step 0c (Model tier gate):** Documents how skills should read `effective_tier` / `required_tier` and either proceed, acknowledge a `/model` switch, or ask the user to `/model` before re-invoking.
- **Frontmatter field:** `agent_doc_model_tier: low | med | high | auto` on session documents.
- **Tests added:** 48 tests in `model_tier.rs` covering tier parse/resolve, harness detection, component read, scanner guards (code fence, blockquote), heuristic path boosts, composition precedence, and JSON serialization.

## 0.31.31

- **Fix: Commit-reliability — snapshot committed even on IPC timeout exit(75):** `write.rs` now saves snapshot + calls `git::commit` before `process::exit(75)`, so agent responses are preserved even when the IDE plugin doesn't ACK the patch in time.
- **Fix: Commit-reliability — commit before `result?` propagation:** `main.rs` reordered to run commit before `result?`, ensuring partial writes that saved a snapshot are always tracked in git.
- **Fix: Commit-reliability — retry on git index.lock contention:** `git.rs` retries `git commit` up to 3× with exponential backoff (100/200/400ms) when concurrent sessions cause lock contention.
- **Fix: Commit-reliability — `agent_doc_commit` FFI export:** `ffi.rs` exports `agent_doc_commit(file_path)` for IDE plugins to call after applying a patch. `NativeLib.kt` + `PatchWatcher.kt` updated to call it on the Document API path.
- **Fix: Commit-reliability — preflight cross-document sweep:** `preflight.rs` scans all tracked docs in the same project at the start of each cycle and commits any doc where the snapshot is newer than the file (missed commit backstop).
- **Fix: `project_config_path()` CWD-sensitivity:** Walks up from CWD to find `.agent-doc/` instead of always using a bare relative path. Prevents wrong-config reads when subcommands run from a subdirectory (e.g., submodule CWD drift). Falls back to CWD for uninitialized projects.
- **Tests added:** `commit_retry_logic_handles_index_lock_error`, `commit_succeeds_when_no_lock_contention` (Fix 3); `agent_doc_commit_returns_false_for_null`, `ffi_git_commit_commits_staged_file` (Fix 4); `preflight_sweep_commits_other_tracked_docs` (Fix 5).
- **Fix: `(HEAD)` marker incorrectly applied to bash comments inside fenced code blocks:** The old ad-hoc fence tracker (`is_fence_marker`) toggled `in_fence` on every line starting with 3+ backticks — including `` ```bash `` which per CommonMark can only OPEN a fence, not close one. When a `` ``` `` plain fence contained inner `` ```bash `` lines (e.g., terminal output referencing a bash command), the state inverted, causing `# On the server — run once` inside a subsequent `` ```bash `` block to appear "outside" the fence and receive a `(HEAD)` marker it must not have. Fix: replace the ad-hoc `is_fence_marker` / `in_fence` toggling in `strip_head_markers` and all four code paths in `add_head_marker` (step 1 cleanup, step 2 heading collection, step 3 HEAD heading counting, re-application loop) with CommonMark-compliant code block detection via `pulldown-cmark`. A closing fence cannot have an info string — `pulldown-cmark` correctly handles this. The re-application path also now filters out any `# comment (HEAD)` lines in git HEAD that are themselves inside a code block, preventing propagation of the baked-in bad marker across commits.
- **Test added:** `add_head_marker_bash_comment_inside_plain_fence` — exercises the specific failure path: a plain `` ``` `` fence containing a `` ```bash `` line, followed by a real heading, followed by a `` ```bash `` fence with a `# comment` line.

## 0.31.30

- **Fix: `❯ ` prefix applied to `agent:pending` patches (regression in v0.31.29):** `normalize_patch_content` was called on all IPC patches, not just exchange patches. When `normalize_prefix_lines` contained a line that also appeared verbatim in the `agent:pending` patch content, that line incorrectly received the `❯ ` prefix. Fix: gate `normalize_patch_content` on `is_append_mode_component(&p.name)` at both the primary IPC write path and the IPC timeout fallback in `write.rs`. Replace-mode components (`pending`, `status`, etc.) now always pass patch content through unchanged.
- **Test added:** `normalize_prefix_lines_skipped_for_replace_mode_components` — verifies that `agent:pending` content is not normalized.

## 0.31.29

- **`agent-doc write --commit` flag:** Runs `git::commit` immediately after a successful write. Eliminates the separate `agent-doc commit` step — the final write in the SKILL.md skill now uses `--commit`. Silently skips commit if the document is not inside a git repo (`git rev-parse --is-inside-work-tree` guard). Streaming checkpoint writes do not use `--commit`; only the final write does.
- **`git::is_in_git_repo` helper:** New `pub(crate)` function that checks whether a file path is inside a git repository.
- **SKILL.md updated:** Step 2a/2b final writes now use `--commit`; step 3 updated to reflect merged write+commit.

## 0.31.28

- **`start.rs` auto-relocate:** When claiming a pane from a terminal in a different tmux session than the project expects, automatically relocates the pane to the correct session before registration (was warn-only). Falls back to warn-only if no anchor pane exists in the expected session.
- **`relocate_if_wrong_session` helper + 3 tests:** Extracted guard into a testable `pub(crate)` function; 3 `IsolatedTmux`-based tests cover noop, cross-session success, and no-anchor fallback.

## 0.31.27

- **`pane_policy` module (tmux-router 0.3.10):** New `PaneMoveOp` + `CrossSession` enum as a mandatory gateway for all pane movement. `CrossSession::Deny` by default; `CrossSession::Allow { reason }` for intentional cross-session relocations. All 7 `join_pane` call sites in agent-doc migrated to use `PaneMoveOp`.
- **Guard `start.rs` registration:** When claiming a pane, warns if `$TMUX_PANE`'s session ≠ `project_tmux_session()` — prevents silent session drift on claim.
- **Guard `resolve_target_session` auto-update (route.rs):** No longer overwrites `tmux_session` config when a previously-configured session is dead. Only writes config when no session was previously set. Prevents session 1 from silently overwriting session 0.
- **Fix `resync.rs` WrongSession detection:** `detect_issues` now falls back to `config::project_tmux_session()` when `frontmatter.tmux_session` is absent. Panes in a wrong session are flagged even without per-document session frontmatter. `apply_fixes_to_registry` uses `PaneMoveOp::allow_cross_session("relocate WrongSession pane to project session")` to move them.

## 0.31.26

- **Fix: orphan repair dedup guard (repair.rs):** `repair::run` now reads the document before applying a pending response and checks if the content is already present using a 3-line fingerprint. If already applied (e.g., IPC path wrote the content but `clear_pending` was never called due to exit 75), the pending file is removed without re-applying. Prevents ghost-reappearance of previous responses. New test: `recover_skips_duplicate_apply`.

## 0.31.25

- **`preflight` diff-only always (preflight.rs):** `document` field is always `null` — the full document is never sent automatically. Use `agent-doc read <FILE>` to fetch on demand.
- **BREAKING CHANGE: `--diff-only` and `--with-document` flags removed from `preflight`:** Both flags removed. Diff-only is now unconditional. Any callers using either flag must remove it.
- **`agent-doc read <FILE> [--component <name>]` (read.rs):** New subcommand to fetch the full document or a single named component's body on demand. Use on the first cycle when the document is not yet in context.
- **Stash window pane check removed (preflight.rs):** `check_layout` no longer flags panes in `stash*` windows as layout issues. Stash windows hold intentional backgrounded sessions.
- **Fix: `collapsible_if` in `git.rs` (CI):** Nested `if` at line 410 collapsed to satisfy Rust 1.94.1 clippy.

## 0.31.24

- **Fix: `~~~` tilde fences protected from `❯ ` prefix normalization (write.rs):** `normalize_user_prompts_in_exchange` previously only tracked `` ``` `` (backtick) fences. Lines inside `~~~` fenced regions could incorrectly enter `user_added` and receive a `❯ ` prefix. Fixed by extracting `fence_open`/`fence_close` helpers that handle both `` ` `` and `~` fence chars with proper length tracking (matching `diff.rs`'s `fence_char`/`fence_len` approach). New test: `normalize_user_prompts_tilde_fence_interior_skipped`.

## 0.31.23

- **Fix: `❯ ` prefix normalization via IPC `fullContent` (write.rs):** When `normalize_prefix_lines` is non-empty, `try_ipc` now also sends `fullContent = content_ours` in the IPC payload (both socket and file paths). The plugin's `fullContent` path replaces the entire document, guaranteeing `❯ ` prefixes reach the editor file even when targeted string replacement fails.
- **Fix: boundary regex in `findBoundaryInComponent` + `repositionBoundaryToEnd` (PatchWatcher.kt v0.2.51):** Pattern updated from `[a-f0-9-]+` to `[a-z0-9][a-z0-9:-]*` so summary-style boundary IDs (e.g. `a0cfeb34:agent-doc-bugs`) are correctly matched.
- **Fix: boundary stripping regex in VSCode extension (extension.ts v0.2.4):** `[a-f0-9]+` → `[a-z0-9][a-z0-9:-]*` in boundary marker strip-before-replace path.
- **Regression test:** `normalize_user_prompts_restores_prefix_lost_in_file` — verifies snapshot `❯ do` is restored when editor file has bare `do`.
- **`agent-doc compact --tag <name>` (compact.rs):** Creates a lightweight git tag at HEAD before compaction as a pre-compact checkpoint. Without `--tag`, auto-generates `agent-doc/<doc-name>/pre-compact-N`. Use `--tag skip` to disable. Tagging failure is a warning, not an error.
- **`agent-doc log <FILE>` (history.rs):** Annotated git log for a session document. Walks `git log`, loads all `agent-doc/<name>/pre-compact-*` tags, and annotates matching commits in the output table (COMMIT, DATE, SUBJECT, TAG columns).
- **`agent-doc show <FILE> [--back N | --at N | --tag <name>]` (history.rs):** Shows document content at a specific point in git history. `--back N` maps to `HEAD~N`; `--at N` selects the Nth commit in log order (0 = newest); `--tag <name>` resolves the tag to its commit.
- **`agent-doc diff <FILE> --from <ref> [--to <ref>]` (history.rs):** Shows a unified diff of the document between two git refs. `--to` defaults to `HEAD`. Without `--from`, falls back to the existing live diff behavior.

## 0.31.22

- **Fix: quoted strings skip `❯ ` prefix normalization (write.rs):** `normalize_user_prompts_in_exchange` now excludes lines starting with `"` from `❯ ` prefix tagging. Previously, user-written quoted strings (e.g., `"Merge conflict with external write"`) were incorrectly tagged as terminal prompts. New test: `normalize_user_prompts_quoted_string_skipped`.

## 0.31.21

- **Fix overeager `❯ ` prefix on agent response lines (write.rs):** `normalize_user_prompts_in_exchange` now takes a `baseline` parameter. User-added lines are identified by diffing `snapshot → baseline` (not `snapshot → content_ours user_region`). After `apply_patches_with_overrides`, the boundary moves to the end of exchange — so content_ours' "user region" incorrectly included agent response lines. The fix diffs against baseline (pre-agent state), ensuring only genuine user additions get `❯ `. New regression test: `normalize_user_prompts_agent_response_not_prefixed`.

## 0.31.20

- **`❯ ` prefix normalization for exchange user prompts (write.rs):** After each agent cycle, new user-typed lines in `patch=append` exchange components are prefixed with `❯ ` to visually distinguish user input from agent responses. Implemented via `similar` diff of snapshot vs `content_ours`; only Insert lines before the boundary marker are prefixed. `normalize_user_prompts_in_exchange()` and `extract_normalization_targets()` added. 6 tests.
- **IPC-side prefix normalization (write.rs + PatchWatcher.kt v0.2.49):** `try_ipc` passes `normalize_prefix_lines: Option<&[String]>` in the IPC payload. JetBrains plugin applies `normalizeExchangePrefixes()` targeting only the user region (before `<!-- agent:boundary:UUID -->`) via targeted text replacement. Both Document API and VFS paths updated.
- **SKILL.md rule: never echo user input in patch:exchange (SKILL.md):** For `patch=append` exchange components, the patch must contain only new agent response content — echoing user input creates duplicates.

## 0.31.19

- **AGENT_PROCESSES guard on wrong-session recovery (route.rs):** `is_agent_process()` helper added. Wrong-session recovery path now skips `stash_pane`+`rescue_from_stash` for panes running non-agent processes (corky, shells, etc.) — falls through to auto-start instead. Prevents corky/foreign panes from being dragged across tmux sessions.
- **AGENT_PROCESSES guard on lazy claim Strategy 2 (route.rs):** `find_target_pane()` result is now gated by `is_agent_process()` — panes running non-agent processes are not claimed. Prevents corky from being registered as the owner of a document pane.
- **`resync --fix --session <target>` (resync.rs + main.rs):** `WrongSession` fix now supports `--session <name>` to relocate panes via `join-pane` instead of killing them. `apply_fixes_to_registry` takes `relocate_session: Option<&str>`. Falls back to deregister if no active pane found in target session.

## 0.31.18

- **Partial compact `--keep N` (compact.rs):** `agent-doc compact <FILE> --keep N` archives only exchanges older than the last N `### Re:` sections, preserving recent context. `parse_topic_sections()` helper added; 4 new tests.
- **Slash command dispatch from diff (diff.rs + preflight.rs):** `parse_slash_commands(diff)` extracts slash commands from user-added lines; preflight returns them in `slash_commands[]`; the SKILL executes each before responding. Guards: code fences, blockquotes, non-added/removed lines excluded.
- **Dedupe stale patch cleanup (dedupe.rs):** After removing duplicate blocks, deletes `.agent-doc/patches/<hash>.json` to prevent `processPendingPatches()` from re-applying removed content on next plugin startup.
- **JB plugin startup dedup guard (PatchWatcher.kt v0.2.48):** Before applying a pending patch file, compares snapshot mtime against patch file mtime. If snapshot is newer, the patch was already applied — deletes stale file and skips. Replaces the incorrect boundary-ID check from v0.2.47.
- **Cross-session pane swap fix (route.rs + sync.rs):** `rescue_from_stash()` now checks pane session before swap; uses `join-pane` for cross-session panes. Session-drift detection added to `check_layout()` in preflight.
- **PromptPoller FFI CRDT merge (editors/jetbrains):** FFI-based CRDT merge, fix unnecessary reload, preserve edits on conflict.
- **SPEC.md §7.26 + §7.28 updated:** preflight JSON now documents `slash_commands[]`; dedupe documents stale patch file cleanup.

## 0.31.17

- **CRDT duplicate bug fix (write.rs):** When boundary-synthesis consumed unmatched content into a patch, the IPC payload also sent the same content as `"unmatched"` — the plugin applied both, producing duplicates. Fixed by clearing `effective_unmatched` to `""` when synthesis occurred, on both socket and file IPC paths.
- **Write-time dedup (write.rs):** `build_ipc_patches_json` now checks if the unmatched content already exists in the target component before synthesizing a patch. Skips synthesis if a match is found, making writes idempotent.
- **SKILL.md demoted (SKILL.md):** `<!-- patch:exchange -->` wrapper is now "preferred, not required" — the binary correctly handles both wrapped and raw content paths.
- **3 new tests (write.rs):** `synthesis_dedup_skips_when_content_already_present`, `synthesis_proceeds_when_content_is_new`, `effective_unmatched_cleared_when_synthesis_consumes_content`.

## 0.31.16

- **Extreme drift snapshot re-sync (git.rs):** When `commit()` detects file is >5x larger than snapshot (typical of file move/rename), automatically re-syncs snapshot from file content. Prevents the drift loop that caused "externally saved" dialogs and lost keystrokes after renaming files.
- **Claim auto-scaffold (claim.rs):** Empty `.md` files get the full template (UUID + format + crdt + components) when claimed. Previously only wrote `agent_doc_session`, causing scaffolding to skip (no format detected).

## 0.31.15

- **Transfer auto-init (extract.rs):** `agent-doc transfer` auto-creates the target file in template mode if it doesn't exist. Creates parent dirs, generates UUID session, copies agent name from source. Always defaults to template format.
- **Write silent-drop warnings (write.rs):** `run_stream` warns when file has no template components but receives unmatched content. `try_ipc` logs `ipc_unmatched_content_dropped` to ops.log. Improved ops.log to include `ipc_patches` count alongside original `patches` count.
- **Investigation runbook:** New `runbooks/investigate-behavior.md` for debugging agent-doc behavior (ops.log, git history, affected files, common failure patterns).

## 0.31.14

- **Binding invariant enforcement (claim.rs):** When target pane is already claimed by another document, `claim` now provisions a new pane instead of erroring. Enforces SPEC §8.5: "never commandeer another document's pane."
- **Sync auto-scaffold (sync.rs):** Empty `.md` files in editor layout are automatically scaffolded with template frontmatter + status/exchange/pending components. Scaffold is saved as snapshot and committed to git immediately.
- **Transfer pending merge (extract.rs):** `agent-doc transfer` now automatically transfers the `pending` component alongside the named component. Source pending is cleared after merge.
- **SPEC.md updates:** §7.10 (claim provisions on occupied pane), §8.5 (empty file auto-scaffold in initialization step).
- **Tests:** 6 sync scaffold tests (positive + negative), 2 pending merge tests. 458 total.
- **Runbook:** `code-enforced-directives.md` — behavioral invariants enforced by binary, not agent instructions.

## 0.31.13

- **Diff-type classification (P1)**: `classify_diff()` classifies user diffs into 7 types (Approval, SimpleQuestion, BoundaryArtifact, Annotation, StructuralChange, MultiTopic, ContentAddition). Wired into preflight JSON as `diff_type` + `diff_type_reason`. 13 tests.
- **Annotated diff format (P3)**: `annotate_diff()` transforms unified diffs into `[agent]`/`[user+]`/`[user-]`/`[user~]` format. Wired into preflight JSON as `annotated_diff`. 5 tests.
- **Content-source annotation sidecar (P4)**: New `agent-doc annotate` command generates `.agent-doc/annotations/<hash>.json` mapping each line to agent/user source. SHA256 cache invalidation. GC integration. 6 tests.
- **Reproducible operation logs (P5)**: New `.agent-doc/logs/cycles.jsonl` with structured JSONL entries (op, file, timestamp, commit_hash, snapshot_hash, file_hash). Wired into all write paths + git commit. 2 tests.
- **Post-preflight eval diffs (P2)**: Moved `strip_comments` to `component.rs` (shared between binary and eval-runner). eval-runner preprocesses diffs with comment stripping.
- **Transfer-source metadata**: `PatchBlock` now supports `attrs` field. `<!-- patch:name key=value -->` attributes parsed and preserved. 3 tests.
- **JB plugin Gson migration**: Replaced hand-rolled JSON parser with `com.google.gson.JsonParser`. Fixes `\\n` unescape ordering bug. Plugin v0.2.44.
- **SKILL.md enhancements**: Diff-type routing (0b), multi-topic `---` separators (0c), process discipline clarification.
- **Domain ontology**: Interaction Model section in README.md (Directive, Cycle, Diff, Annotation). `directive.md` kernel node.
- **Module-harness**: New `ontology-references` runbook for cross-referencing domain ontology in module specs.

## 0.31.12

- **Refactor `ensure_initialized()`**: Split into 3 focused functions: `ensure_session_uuid()`, `ensure_snapshot()`, `ensure_git_tracked()`. Composite `ensure_initialized()` calls all three.
- **Rename `auto_start_no_wait()` → `provision_pane()`**: Aligns with domain ontology (Provisioning = creating a new pane + starting Claude).
- **Tests**: 8 new tests for ensure_session_uuid (3), ensure_snapshot (2), ensure_initialized (1), plus 2 helpers.

## 0.31.11

- **Sync auto-initialization**: `ensure_initialized()` now called in sync's `resolve_file`. Files with `agent_doc_format` but no session UUID get one assigned automatically on editor navigation. Fixes: files created by skills (granola import) are no longer invisible to sync.
- **Binding invariant spec**: SPEC.md section 8.5 documents the pane lifecycle invariant — document drives pane resolution, never commandeers another document's pane.
- **Domain ontology**: README.md now has Document Lifecycle, Pane Lifecycle, and Integration Layer ontology tables (Binding, Reconciliation, Provisioning, Initialization).
- **Module docs**: sync.rs, claim.rs, snapshot.rs, route.rs updated with ontology terminology.

## 0.31.10

- **Auto-init for new documents**: `ensure_initialized()` in `snapshot.rs` — claim and preflight now auto-create snapshot + git baseline for files entering agent-doc. No more untracked files after import.
- **Cross-process typing detection**: FFI exports `agent_doc_is_typing_via_file` and `agent_doc_await_idle_via_file` for CLI tools running in separate processes. `is_idle` and `await_idle` now bridge to file-based indicator when untracked in-process.
- **Diff stability fix**: `wait_for_stable_content` counter now tracks consecutive stable reads across outer iterations (was resetting within each pass).
- **IPC error propagation**: `ipc_socket::send_message` now returns proper errors instead of swallowing connection/timeout failures as `Ok(None)`.
- **Template patch boundary fix**: Improved boundary marker handling in `apply_patches_with_overrides`.
- **CI/build**: `make release` target, idempotent release workflows, version-sync check in `make check`.

## 0.31.9

- **Transfer-extract runbook**: New bundled runbook for cross-file content moves (`agent-doc transfer`/`extract`). Installed via `skill install`.
- **Compact-exchange runbook update**: Added note about preserving unanswered user input during compaction.
- **SKILL.md Runbooks section**: Added runbook links to SKILL.md so the skill knows about transfer/extract/compact procedures.
- **Housekeeping**: Gitignore `.cargo/config.toml`, resolve clippy warnings, remove accidentally committed files.

## 0.31.8

- **CI fix**: Removed `path = "../tmux-router"` override from Cargo.toml. CI runners don't have the local submodule; uses crates.io dependency exclusively.

## 0.31.7

- **Stash-bounce fix**: Removed `return_stashed_panes_bulk()` from automatic `prune()` path. Active panes now stay in stash until the reconciler explicitly needs them, eliminating the stash→return→stash loop that caused visible pane bouncing.
- **Sync file lock**: Added `flock` on `.agent-doc/sync.lock` to serialize concurrent sync calls. Prevents race conditions when rapid tab switches fire overlapping syncs.
- **Route sync removal**: Removed redundant `sync::run_layout_only` from Route command dispatch and `sync_after_claim` from route.rs. The JB plugin's `EditorTabSyncListener` is now the sole authority for layout sync.
- **Diagnostic checkpoints**: Added checkpoint logging in sync (`post-repair`, `post-prune`, `pre-tmux_router`) to pinpoint pane state at key transitions.

## 0.31.6

- **Debounce fix**: Default mtime debounce increased from 500ms to 2000ms. Configurable per-document via `agent_doc_debounce` frontmatter field.
- **Structured logging**: Added `tracing` + `tracing-subscriber` + `tracing-appender`. Set `AGENT_DOC_LOG=debug` to log to `.agent-doc/logs/debug.log.<date>`. Zero overhead when unset.
- **Pre-response cleanup bug**: `clear_pending()` now deletes pre-response snapshots after successful writes. Previously accumulated indefinitely.
- **Lock file cleanup bug**: `SnapshotLock::Drop` now deletes the lock file (not just unlocks). CRDT lock acquisition cleans stale locks (>1 hour old).
- **`agent-doc gc` subcommand**: Garbage-collects orphaned files in `.agent-doc/` directories. Supports `--dry-run` and `--root` flags.
- **Auto-GC on preflight**: Runs GC once per day via `.agent-doc/gc.stamp` timestamp check.
- **Cleanup runbook**: New `runbooks/cleanup.md` documenting `.agent-doc/` directory structure and cleanup rules.
- **Tracing instrumentation**: `tracing::debug!` at key decision points in sync, route, layout, and resync modules.
- **Source annotations for extract/transfer**: `agent-doc extract` and `agent-doc transfer` now wrap content with `[EXTRACT from ...]` or `[TRANSFER from ...]` blockquote annotations including timestamp.
- **Post-sync session health check**: After every sync, verifies the tmux session still exists. Logs `CRITICAL` if session was destroyed.
- **Route cleanup on failure**: When route fails, orphaned panes created during the attempt are killed before the error propagates.

## 0.31.5

- **Commit on claim**: `agent-doc claim` now commits the file after saving the initial snapshot. Ensures the first prompt appears as a diff against a committed baseline.
- **Auto-setup untracked files**: Preflight auto-adds untracked files to git (snapshot + `git add`), so `/agent-doc` works on new files without claiming first.
- **VCS refresh after commit**: `agent-doc commit` writes a VCS refresh signal file, prompting IDEs to update their git status display.
- **Preflight `--diff-only` flag**: Omits the full document from preflight JSON output, reducing token usage by ~80% on subsequent cycles.
- **Skill-bundled runbooks**: `agent-doc skill install` now installs runbooks alongside SKILL.md at `.claude/skills/agent-doc/runbooks/`. First runbook: `compact-exchange.md`.
- **JetBrains prompt button truncation**: maxLabelLen reduced from 45 to 25 characters.
- **Debounce module**: New `src/debounce.rs` for reusable debounce logic.

## 0.31.4

- **IPC reposition simplified**: Removed file-based IPC fallback from `try_ipc_reposition_boundary`. Boundary reposition now uses socket IPC exclusively (through FFI listener callback). Non-fatal on failure.
- **Inline `max_lines=N` attribute**: Component tags support `max_lines=N` to trim content to the last N lines after patching. Precedence: inline attr > `components.toml` > unlimited. Example: `<!-- agent:exchange patch=append max_lines=50 -->`.
- **Boundary-stripping in watch hash**: `hash_content()` strips boundary markers before hashing, preventing reactive-mode feedback loops where boundary repositions trigger infinite re-runs.
- **Console component scaffolding**: `agent-doc claim` now scaffolds a `<!-- agent:console -->` component for template-mode documents.
- **HEAD marker cleanup**: `git.rs` strips stray `(HEAD)` markers from working tree after commit (defensive cleanup).
- **StreamConfig max_lines**: `agent_doc_stream.max_lines` frontmatter field limits console capture lines (default: 50).
- **Tests**: 612 total. New: 4 `max_lines_*` tests in template.rs.
- **Docs**: SPEC.md, README.md, CLAUDE.md updated for max_lines and socket-only IPC.

## 0.31.3

- **Claim snapshot fix**: `agent-doc claim` now saves the initial snapshot with empty exchange content. Existing user text in the exchange becomes a diff on the next run, preventing unresponded prompts from being absorbed into the baseline.
- **Tests**: 608 total. New: `strip_exchange_content_removes_user_text`, `strip_exchange_content_preserves_no_exchange`.

## 0.31.2

- **`agent-doc dedupe`**: New command removes consecutive duplicate response blocks. Ignores boundary markers in comparison. Used to fix duplicate responses caused by watch daemon race conditions.
- **Write-origin tracing**: `--origin` flag on `agent-doc write` logs the write source (skill/watch/stream) to ops.log. Aids diagnosis when snapshot drift occurs.
- **Commit drift warning**: Warns when `file_len - snap_len > 100` bytes, indicating a possible out-of-band write that bypassed the snapshot pipeline.
- **Watch daemon busy guard**: Skips files with active agent-doc operations (`is_busy()` check), preventing the watch daemon from generating duplicate responses when competing with the skill.
- **PatchWatcher EDT fix**: Patch computation moved outside `WriteCommandAction`. No-op patches skip the write action entirely, eliminating EDT blocking and typing lag.
- **ClaimAction claim+sync**: `Ctrl+Shift+Alt+C` now calls `agent-doc claim` on the focused file before syncing, handling unclaimed/empty files.
- **Single-char truncation fix**: Single characters are treated as potentially truncated in `looks_truncated()`, requiring 1.5s stability check. Prevents partial typing (e.g., "S" from "Save as a draft.") from triggering premature runs.
- **SKILL.md**: All write examples include `--origin skill`. Version 0.31.2.
- **JetBrains plugin**: Version 0.2.40.
- **Tests**: 606 total. New: `truncated_single_chars`, `dedupe_*` (4 tests).
- **Docs**: SPEC.md §7.22 (--origin), §7.23 (busy guard), §7.28 (dedupe). CLAUDE.md module layout.

## 0.31.1

- **Declarative layout sync**: Navigating to a file in a split editor now creates a tmux pane automatically. Files with session UUIDs are always treated as Registered by sync, even without a registry entry (reverses 0.31.0 Unmanaged guard). Auto-start phase also no longer requires registry entries.
- **ClaimAction simplified**: JetBrains ClaimAction (Ctrl+Shift+Alt+C) now delegates entirely to SyncLayoutAction — removed 200+ lines of position detection, pane ID extraction, and independent auto-start logic.
- **Claim registry protection**: `agent-doc claim` refuses to overwrite an existing live claim without `--force`, preventing silent pane corruption from fallback position detection.
- **HEAD marker duplicate fix**: `add_head_marker` uses occurrence counting instead of substring matching, correctly marking new headings even when the same heading text exists earlier in the document.
- **Busy guard removed**: EditorTabSyncListener no longer blocks sync when any visible file has an active session. The binary's own concurrency guards (startup locks, registry locks) are sufficient.
- **Build stamp**: New `build.rs` embeds a build timestamp. On sync, the binary compares against `.agent-doc/build.stamp` and clears stale startup locks on new build detection.
- **Plugin binary resolution fix**: EditorTabSyncListener and SyncLayoutAction now pass `basePath` to `resolveAgentDoc()`, correctly resolving `.bin/agent-doc` instead of falling through to `~/.cargo/bin/agent-doc`.
- **JetBrains plugin**: Version 0.2.38. Requires uninstall→restart→install→restart (structural class changes).
- **Tests**: 602 total. New: `add_head_marker_duplicate_heading_text`.
- **Docs**: SPEC.md §7.10 (claim protection), §7.15 (occurrence counting), §7.20 (UUID-always-registered, build stamp). Ontology claim.md updated.

## 0.31.0

- **`agent-doc session` CLI**: Show/set configured tmux session with pane migration (`session_cmd.rs`).
- **Stash pane safety**: `purge_unregistered_stash_panes` no longer kills agent processes (agent-doc, claude, node) in stash — only idle shells. Prevents loss of active Claude sessions when registry goes stale.
- **Session resolution consolidation**: `resolve_target_session()` extracts duplicated session-targeting logic from route.rs into a single function. Config.toml is the source of truth; claim/route no longer auto-overwrite it.
- **Stale UUID handling**: Files with frontmatter session UUID but no registry entry are treated as Unmanaged by sync — prevents auto-starting sessions for unclaimed files.
- **Unused variable cleanup**: Fixed 8 warnings across route.rs and template.rs.
- **Docs**: SPEC.md §7.27 (session command), CLAUDE.md module layout updated.
- **Tests**: 601 total, 1 new (`purge_preserves_unregistered_agent_process_in_stash`).

## 0.30.1

- **FFI `agent_doc_is_idle`**: Non-blocking typing check for editor plugins to query idle state before boundary reposition.
- **JetBrains plugin typing debounce**: Boundary reposition deferred until typing stops, using FFI idle check.
- **VS Code koffi FFI bindings**: `native.ts` with koffi-based native bindings for the shared FFI library.
- **VS Code reposition boundary handling**: Boundary reposition with typing debounce via FFI idle check.
- **tmux_session config drift fix**: `route.rs` follows pane session, `claim.rs` updates config to match.
- **2 new FFI tests**: Coverage for `agent_doc_is_idle` and related FFI surface.
- **Dependencies**: `tmux-router` v0.3.8.

## 0.30.0

- **Stale baseline guard (component-aware)**: `is_stale_baseline()` now parses components and only checks append-mode (exchange, findings). Replace-mode components (status, pending) are skipped. Falls back to prefix check for inline docs. 11 new tests.
- **Busy pane guard**: `SyncOptions.protect_pane` callback in tmux-router DETACH phase + `layout.rs`. Prevents stashing panes with active agent-doc/claude sessions during layout changes.
- **Auto-start startup lock**: `.agent-doc/starting/<hash>.lock` with 5s TTL prevents double-spawn when sync fires twice in quick succession.
- **Bug 2A fix**: IPC snapshot save failure after successful write is now non-fatal with warning. Commit auto-recovers via divergence detection.
- **Bug 2B fix**: Removed commit-time divergence detection that was eating user edits into the snapshot.
- **Hook system**: `agent-doc hook fire/poll/listen/gc` CLI. Cross-session event coordination via `agent-kit` hooks (v0.3). `post_write` and `post_commit` events fired from write + commit paths.
- **HookTransport trait**: Abstract delivery mechanism with `FileTransport`, `SocketTransport`, `ChainTransport` implementations.
- **Ops logging tests**: 2 new tests for `.agent-doc/logs/ops.log`.
- **Dependencies**: `agent-kit` v0.3 (hooks feature), `tmux-router` v0.3.7 (SyncOptions).
- **Docs**: SPEC.md §6.6/§7.9/§7.20/§9.5, README.md key features, CLAUDE.md module layout.
- **Tests**: 595 total (16 new), 0 failures.

## 0.29.0

- **Links frontmatter**: Renamed `related_docs` → `links` (backward-compat alias). URL links (`http://`/`https://`) are fetched via `ureq`, converted HTML→markdown via `htmd` (stripping script/style/nav/footer), cached in `.agent-doc/links_cache/`, and diffed on each preflight. Non-HTML content passes through unchanged.
- **Session logging**: Persistent logs at `.agent-doc/logs/<session-uuid>.log` with timestamped events for session start, claude start/restart/exit, user quit, and session end.
- **Auto-trigger on restart**: After `--continue` restart, background thread sends `/agent-doc <file>` via `tmux send-keys` after 5s delay to re-trigger the skill workflow.
- **Security documentation**: README.md top-level security notice + detailed Security section. SPEC.md Section 10 with threat model, known risks, and recommendations.
- **New dependency**: `htmd` v0.5.3 (HTML-to-markdown, ~13 new crates from html5ever ecosystem, no HTTP server).
- **Tests**: 7 new tests for URL detection, HTML conversion, boilerplate stripping, cache paths. 361 total, 0 failures.

## 0.28.3

- **Write dedup boundary fix**: Strip `<!-- agent:boundary:XXXXXXXX -->` markers before dedup comparison. Boundary marker IDs change on each write, causing false negatives in the dedup check (content appeared different when only the boundary ID changed).

## 0.28.2

- **Write dedup**: All 4 write paths (`run`, `run_template`, `run_stream` disk, `run_stream` IPC) skip the write when merged content is identical to the current file. Dedup events logged to `/tmp/agent-doc-write-dedup.log` with backtrace.
- **Pane ownership verification**: `verify_pane_ownership()` called at entry of `run`, `run_template`, `run_stream`. Rejects writes when a different tmux pane owns the session (lenient — passes silently when not in tmux or pane is indeterminate).
- **Column memory**: `.agent-doc/last_layout.json` saves column→agent-doc mapping (carried from v0.28.1, now documented).

## 0.28.1

- **Column memory**: `.agent-doc/last_layout.json` saves column→agent-doc mapping. When a column has no agent doc, sync substitutes the last known agent doc from the state file. Preserves 2 tmux panes when one column switches to a non-agent file.

## 0.28.0

- **Empty col_args filtering**: `sync` now filters out empty strings from `col_args` before processing. Fixes phantom empty columns sent by the JetBrains plugin during rapid editor split changes.
- **Sync debug logging**: Added `/tmp/agent-doc-sync.log` trace logging at key sync decision points (col_args, repair_layout, auto-start, pre/post tmux_router::sync pane counts).
- **Post-auto_start stash removed**: The explicit stash after auto-start is no longer needed — `tmux_router::sync` always runs the full reconcile path (no early exits), so excess panes are stashed during the DETACH phase.
- **tmux-router v0.3.6**: Early exits removed from `sync` — the full reconcile path now runs for 0, 1, or 2+ resolved panes uniformly. Previous early exits for `resolved < 2` bypassed the DETACH phase, leaving orphaned panes from previous layouts visible.
- **JetBrains plugin v0.2.36**: Filter empty columns in SyncLayoutAction.kt

## 0.27.9

- **tmux-router v0.3.5**: Updated dependency — trace logging at key sync decision points + early-exit stash removal (preserves previous-column panes)

## 0.27.8

- **tmux-router v0.3.4**: Updated dependency — early-exit stash now derives session from pane via `pane_session()` instead of dead `doc_tmux_session` path
- **VERSIONS.md backfill**: Added entries for v0.23.2 through v0.26.6

## 0.27.7

- **Sync path column-aware split**: `auto_start_no_wait` now accepts `col_args` and computes `split_before` via `is_first_column()`. Previously hardcoded `split_before = false`, causing new panes to always split alongside the rightmost pane regardless of column position. The sync path (editor tab switches) now matches the route path behavior.

## 0.27.6

- **Bold-text pseudo-header fallback for `(HEAD)` marker**: `add_head_marker()` in `git.rs` now falls back to bold-text lines (`**...**`) when no markdown headings are found in new content. `strip_head_markers()` also handles stripping `(HEAD)` from bold-text lines.
- **SKILL.md header format guidance**: Added "Response header format (template mode)" section instructing agents to use `### Re:` headers. Bold-text pseudo-headers are supported as a fallback but real headings are preferred for outline visibility and sub-section nesting.

## 0.27.5

- **Column-aware split target**: `auto_start_in_session` picks the split target based on column position — first pane (leftmost) for left-column files, last pane (rightmost) for right-column files. Fixes 3-pane layout bug where new panes split the wrong existing pane.
- **Early-exit stash**: Before the `resolved < 2` early return in `tmux-router::sync`, excess panes in the agent-doc window are now stashed. Previously, old panes from previous layouts stayed visible when only one file resolved.
- **tmux-router v0.3.3**: Published with the early-exit stash fix.

## 0.27.4

- **Rescue stashed panes in sync**: `sync.rs` now rescues stashed panes back to the agent-doc window via swap-pane/join-pane before falling back to auto-start. Preserves Claude session context across editor tab switches.

## 0.27.3

- **Revert auto-kill**: Reverts v0.27.2 auto-kill of idle stashed Claude sessions. The `❯` prompt is the normal state of a stashed session waiting to be rescued — not an orphan indicator.

## 0.27.2

- **Auto-kill idle stashed Claude sessions**: Added auto-cleanup in `return_stashed_panes_bulk()` for stashed panes running agent-doc/claude at the `❯` prompt with no return target. (Reverted in v0.27.3 — too aggressive, killed active sessions.)

## 0.27.1

- **Fix "externally modified" popup**: Removed stale boundary disk write that caused spurious file modification notifications in editors.

## 0.27.0

- **Fix stash rescue deregistration**: Fixed pane deregistration during stash rescue operations.
- **Socket IPC**: Added `ipc_socket` module using Unix domain sockets via the `interprocess` crate for direct binary-to-plugin communication.
- **Bulk resync**: `return_stashed_panes_bulk()` for batch stash rescue operations.

## 0.26.6

- **FFI sync lock/debounce**: Added `agent_doc_sync_try_lock`/`unlock` FFI exports for cross-editor concurrency control. Added `agent_doc_sync_bump`/`check_generation` for cross-editor event coalescing.
- **Layout debounce fix**: `LayoutChangeDetector` uses generation counter instead of spawning concurrent threads per event.
- **JetBrains plugin v0.2.35**: Uses FFI sync primitives with local fallback.

## 0.26.5

- **Skip no-op IPC reposition**: IPC reposition signal skipped when boundary position is unchanged, eliminating ~64% of no-op PatchWatcher operations.
- **Handle inotify overflow**: PatchWatcher scans for missed files on inotify OVERFLOW events.
- **CI: crates.io-only dependencies**: All path dependencies (instruction-files, tmux-router, agent-kit, module-harness, existence) replaced with crates.io versions in CI workflows.

## 0.26.4

- **Prompt detection for Claude Code v2.1+**: Support numbered list format (`N. label`) in prompt option parsing alongside bracket format (`[N] label`).
- **Auto-start PromptPoller**: Plugin auto-starts PromptPoller on project open.
- **JetBrains plugin v0.2.32**: PromptPoller auto-start, `.bin/` path resolution, diagnostic logging.

## 0.26.3

- **Sync no longer auto-inits frontmatter**: Sync returns `Unmanaged` for files without session UUIDs; only `claim` adds frontmatter now.
- **Plugin mixed-layout sync**: Uses focus-only when non-`.md` files are in editor splits, preventing stashing.
- **JetBrains plugin v0.2.25**: Alt+Space popup, removed ActionPromoter (frees Alt+Enter for native JetBrains intentions).

## 0.26.2

- **Route single exit point**: Refactored route to `resolve_or_create_pane()` eliminating propagation bugs. `sync_after_claim` now runs on ALL route paths.
- **Response status signals**: File-based status signals (`.agent-doc/status/<hash>`) for cross-process visibility. FFI: `set_status`/`get_status`/`is_busy` for in-process plugin checks.
- **Auto-init unclaimed files in sync**: Sync writes session UUID for unclaimed files.
- **`agent_doc_version()` FFI export**: Runtime version tracking for plugins.
- **JetBrains plugin v0.2.24**: `is_busy()` guard in `EditorTabSyncListener` + `TerminalUtil`.

## 0.26.1

- **Sync layout authority**: `sync_after_claim` uses editor-provided `col_args`, preventing 3-pane layout regression on file switch.
- **Clippy fixes**: `doc_lazy_continuation` fixes in sync.rs, upgrade.rs. Unused variable fix in tmux-router `break_pane_to_stash`.
- **SPEC.md updates**: Added sections on project config, IPC write verification, and sync layout authority.

## 0.26.0

- **Kill pane safety**: `kill_pane` refuses to destroy a session's last window (tmux-router v0.3.0).
- **IPC verification**: Content verification catches partial plugin application failures. `--force-disk` cleans stale patches to prevent double-writes.
- **Module harness context**: All 53+ modules annotated with Spec/Contracts/Evals doc comments (468 named evals, 68% coverage).
- **Existence-lang ontology**: 9 domain terms defined (Document, Session, Component, Boundary, Snapshot, Patch, Exchange, Route, Claim). Dev dependencies: existence v0.4.0, module-harness v0.2.0.
- **README rewrite**: Concise GitHub-facing guide.

## 0.25.15

- **Sync layout repair**: Added `repair_layout()` to fix window index mismatches (agent-doc window not at index 0). Sync tests added for repair skip and move scenarios.
- **Blank line collapse on tmux_session strip**: Collapsing 3+ consecutive newlines to 2 when stripping deprecated `tmux_session` frontmatter field.

## 0.25.14

- **Sync pane repair**: Window index repair, pane state reconciliation, effective window tracking.
- **Resync enhancements**: Enhanced dead pane detection and session validation.
- **Route improvements**: Improved command routing logic.

## 0.25.13

- **Install script**: Rewritten `install.sh` with platform detection and improved install paths.
- **Homebrew formula**: Added `Formula/agent-doc.rb` for macOS/Linux Homebrew installation.
- **Deprecate `tmux_session` frontmatter**: Sync strips the field on encounter instead of repairing it. Route `auto_start` no longer attempts repair.

## 0.25.12

- **Sync swap-pane atomic reconcile**: `context_session` overrides frontmatter `tmux_session`, auto-repairs on mismatch.
- **Visible-window split**: New panes split in the visible agent-doc window instead of stash.
- **Resync report-only in sync**: `resync --fix` disabled in sync path to preserve cross-session panes.
- **tmux-router v0.2.9**: Swap-pane atomic transitions.

## 0.25.11

- **Tmux-router swap-pane atomic transitions**: Pane moves use `swap-pane` for flicker-free layout changes. CI fix for path dependencies (agent-kit, tmux-router).

## 0.25.10

- **Preflight mtime debounce**: 500ms idle gate before computing diff.
- **Unified diff context**: Diff output uses unified format with 5-line context radius.
- **Route `--debounce` flag**: Opt-in mtime polling for coalescing rapid editor triggers.
- **`is_tracked` FFI export**: For editor plugins to check file tracking status.
- **Sync no-wait auto-start**: `auto_start_no_wait` for non-blocking session creation during sync.
- **JetBrains plugin v0.2.21**: Sync logging improvements.

## 0.25.9

- **`is_tracked()` FFI export**: Conservative debounce on untracked files (fallback to local tracking).
- **Untracked file debounce fix**: Untracked files no longer bypass debounce.
- **JetBrains plugin v0.2.20**: `is_tracked` binding + FFI logging tags.

## 0.25.8

- **Preflight debounce**: Mtime-based 500ms idle gate before computing diff.
- **Unified diff context**: Switch diff output to unified format with 5-line context radius.
- **Route `--debounce`**: New flag for opt-in mtime polling to coalesce rapid editor triggers.
- **Truncation detection fix**: Smarter dot handling for domain fragments in `looks_truncated`.

## 0.25.7

- **Rename `submit` to `run`**: `submit.rs` renamed to `run.rs`; all internal "submit" terminology updated to "run".
- **FFI debounce module**: `document_changed()` + `await_idle()` FFI exports for editor-side debounce.
- **Route sync fix**: Route calls `sync::run_layout_only()` to prevent auto-start race conditions.
- **JetBrains plugin v0.2.19**: FFI debounce, conditional typing wait, layout-only sync.

## 0.25.6

- **Route `--col`/`--focus` args**: Declarative layout sync from the route command. Plugin `sendToTerminal` passes editor layout in a single CLI call.
- **Layout change detection**: `LayoutChangeDetector` using `ContainerListener` with 5s fallback poll in the JetBrains plugin.
- **EDT-safe threading**: Plugin uses `invokeLater` for Swing reads, background thread for CLI calls.
- **JetBrains plugin v0.2.17**.

## 0.25.5

- **FFI boundary reposition**: Export `agent_doc_reposition_boundary_to_end()` for plugin use.
- **Boundary ID summaries**: 8-char hex IDs with optional `:summary` suffix (filename stem). `new_boundary_id_with_summary()` wired into all write paths.
- **Snapshot boundary cleanup**: Commit path uses `remove_all_boundaries()`. Working tree cleaned via `clean_stale_boundaries_in_working_tree()` on commit.
- **JetBrains plugin v0.2.14**: FFI-first reposition with Kotlin fallback.

## 0.25.4

- **Boundary accumulation fix**: Plugin `repositionBoundaryToEnd` removes ALL boundaries, not just the last one.
- **Short boundary IDs**: 8 hex chars instead of full UUID (centralized in `lib.rs`).
- **Autoclaim pruning**: Validate file existence, prune stale entries on rename/delete.
- **Sync stale pane detection**: Detect alive panes with non-existent registered files (rename), kill stale pane and auto-start new session.

## 0.25.3

- **Fix IPC boundary reposition for prompt ordering**: All IPC write paths call `reposition_boundary_to_end()` before extracting boundary IDs. Previously the stale boundary position caused responses to appear before the prompt.

## 0.25.2

- **Fix skill install superproject root resolution**: Added `resolve_root()` to detect git superproject when CWD is in a submodule. `skill install`/`check` now writes to the project root, not the submodule's `.claude/skills/`.

## 0.25.1

- **IPC boundary reposition from commit**: After committing, send an IPC reposition signal to the plugin so it moves the boundary marker to end-of-exchange in its Document buffer. Avoids writing to the working tree (which would lose user keystrokes).

## 0.25.0

- **`agent-doc preflight` command**: Consolidated pre-agent command (recover + commit + claims + diff + document read) returning JSON for skill consumption.
- **Boundary reposition fix**: Snapshot-only reposition prevents losing user input; no working tree writes during reposition.
- **CRDT merge simplification**: Removed `reorder_agent_before_human()`, deterministic client IDs.
- **Pulldown-cmark outline**: CommonMark-compliant heading parser for outline.
- **Plugin boundary reposition via IPC**: `reposition_boundary: true` flag in IPC payloads.
- **Stash window routing**: Target largest pane, overflow to stash windows.
- **JetBrains plugin v0.2.12**: Plugin-side boundary reposition.

## 0.24.4

- **Deterministic boundary re-insertion in `apply_patches`**: Binary handles boundary re-insertion after checkpoint writes, removing the need for SKILL.md to manually re-insert boundaries.

## 0.24.3

- **Context session for auto_start**: Pass context session to `auto_start` to prevent routing to the wrong tmux session. Post-sync resync for consistency.

## 0.24.2

- **SKILL.md step 3b**: Added mandatory pending updates check each cycle.
- **`plugin install --local`**: Install JetBrains/VS Code plugins from local build directory.
- **JetBrains plugin v0.2.10**: `resync --fix` on startup.
- **JetBrains plugin v0.2.9**: VCS refresh signal fix (ENTRY_MODIFY event).

## 0.24.1

- **SKILL.md heredoc examples**: Updated bundled SKILL.md with heredoc examples for the write command.

## 0.24.0

- **`agent-doc install` command**: System-level setup that checks prerequisites (tmux, claude) and detects/installs editor plugins.
- **`agent-doc init` project mode**: No-arg `init` now initializes a project (creates `.agent-doc/` directory structure, installs SKILL.md) instead of requiring a file argument.
- **SKILL.md content tests**: CLI integration tests for skill install/check content verification.
- **Sync pane guard**: Pre-sync alive pane check prevents duplicate session creation.

## 0.23.3

- **Cross-platform sync pane guard**: `find_alive_pane_for_file()` uses `ps(1)` instead of `/proc` for Linux+macOS compatibility. Pre-sync auto-start checks alive panes before creating duplicates.
- **Clippy fixes**: Fix `collapsible_if` warnings in template.rs, git.rs, terminal.rs. Suppress `dead_code` warnings for library-only boundary functions.

## 0.23.2

- **Explicit patch boundary-aware insertion**: `apply_patches_with_overrides()` checks for boundary markers when applying explicit patch blocks in append mode, not just unmatched content. Prevents boundary markers from accumulating as orphans.
- **Version bump**: Includes all v0.23.1 fixes (IPC snapshot, HEAD marker cleanup, boundary insertion).

## 0.23.1

- **Boundary-aware insertion for unmatched content**: `apply_patches_with_overrides()` now uses boundary-aware insertion for both explicit append-mode patches and unmatched content routed to `exchange`/`output`. Previously only explicit patches used boundary markers; unmatched content used plain append.
- **IPC snapshot correctness**: `try_ipc()` now accepts a `content_ours` parameter (baseline + response, without user concurrent edits). On IPC success the snapshot is saved from `content_ours` instead of re-reading the current file, preventing user edits typed after the boundary from being absorbed into the snapshot.
- **IPC synthesized exchange patch**: When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware component patch so the plugin inserts at the correct position.
- **`boundary.insert()` cleans stale markers**: Before inserting a new boundary marker, `insert()` strips all existing boundary markers from the document. Prevents orphaned markers accumulating across interrupted sessions.
- **`boundary::find_boundary_id_in_component()`**: New public function. Scans a pre-parsed `Component` for any boundary marker UUID, skipping matches inside code blocks. Used by `template.rs` and external callers without re-parsing components.
- **Post-commit working tree cleanup**: After `git.commit()` succeeds, `strip_head_markers()` is applied to both the snapshot and the working tree file. Ensures `(HEAD)` markers never appear in the editor — they exist only in the committed version (creating the blue gutter diff).

## 0.23.0

- **Boundary marker for response ordering**: New `agent-doc boundary <FILE>` command inserts `<!-- agent:boundary:UUID -->` at the end of append-mode component content. The marker acts as a physical anchor — responses are inserted at the marker position, ensuring correct ordering when the user types while a response is being generated. Replaces the fragile caret-offset approach.
- **Boundary-aware FFI**: New `agent_doc_apply_patch_with_boundary()` C ABI export. JetBrains plugin (`NativeLib.kt`, `PatchWatcher.kt`) uses boundary markers with priority over caret-aware insertion.
- **Component parser: boundary marker exclusion**: `<!-- agent:boundary:* -->` comments are now skipped by the component parser (no longer cause "invalid component name" errors).
- **IPC boundary_id**: All IPC patch JSON payloads include `boundary_id` when a boundary marker is present in the target component.
- **SKILL.md: boundary marker step**: Updated bundled SKILL.md to call `agent-doc boundary <FILE>` after reading the document (step 1b).
- **Claim auto-start**: JetBrains plugin "Claim for Tmux Pane" action now auto-starts the agent session after successful claim.
- **JetBrains plugin v0.2.8**: Boundary-aware patching + claim auto-start.

## 0.22.2

- **SKILL.md: immediate commit after write**: Updated bundled SKILL.md to call `agent-doc commit` right after `agent-doc write`, replacing the old "Do NOT commit after writing" instruction. All sessions get the new behavior after `agent-doc skill install`.
- **Plugin default modes**: `exchange` and `findings` components now default to `append` mode in the JetBrains plugin (matching the Rust binary's `default_mode()`), so `<!-- agent:exchange -->` works without explicit `patch=append`.

## 0.22.1

- **Any-level HEAD markers**: `(HEAD)` marker now matches any heading level (`#`–`######`), not just `###`. Only root-level (shallowest) headings in the agent's appended content are marked.
- **Multi-heading markers**: When the agent response has multiple sections, ALL new root headings get `(HEAD)` markers (comparing snapshot vs git HEAD).
- **VCS refresh signal**: After `agent-doc commit`, writes `vcs-refresh.signal` to `.agent-doc/patches/`. Plugin watches for this and triggers `VcsDirtyScopeManager.markEverythingDirty()` + VFS refresh so git gutter updates immediately.
- **JetBrains plugin v0.2.7**: VCS refresh signal handling, cursor-aware FFI, VFS refresh before dirty scope.

## 0.22.0

- **`agent-doc terminal` subcommand**: Cross-platform terminal launch from editor plugins. Config-first (no hard-coded terminal list): `[terminal] command` in `config.toml` with `{tmux_command}` placeholder. Fallback to `$TERMINAL` env var. Detects stale frontmatter sessions and scans registry for live panes.
- **Selective commit**: `agent-doc commit` stages only the snapshot content via `git hash-object` + `git update-index`, leaving user edits in the working tree as uncommitted. Agent response → committed (no gutter). User input → uncommitted (green gutter).
- **HEAD marker**: Committed version of the last `### ` heading gets ` (HEAD)` suffix, creating a single modified-line gutter as a visual boundary and navigation point.
- **First-submit snapshot fix**: When no snapshot exists and git HEAD content matches the current file, treat as first submit (entire file is the diff) instead of "no changes detected".
- **Cursor-aware FFI**: `agent_doc_apply_patch_with_caret()` in shared library — inserts append-mode patches before the cursor position. `Component::append_with_caret()` in `component.rs`. JNA binding in `NativeLib.kt`.
- **JetBrains plugin v0.2.7**: Cursor-aware append ordering via native FFI with Kotlin fallback. Captures caret offset from `TextEditor` before `WriteCommandAction`.

## 0.21.0

- **`agent-doc parallel` subcommand**: Fan-out parallel Claude sessions across isolated git worktrees. Each subtask gets its own worktree and tmux pane. Results collected as markdown with diffs. `--no-worktree` for read-only tasks.
- **CRDT post-merge reorder**: Agent content ordered before human content at append boundary using Yrs per-character attribution (`Text::diff` with `YChange::identity`).
- **README**: Added parallel fan-out documentation section.

## 0.20.3

- **`agent-doc claims` subcommand**: Read, print, and truncate `.agent-doc/claims.log` in a single binary call. Replaces the shell one-liner (`cat + truncate`) that was prone to zombie process accumulation when the Bash tool auto-backgrounded it.

## 0.20.2

- **Fix: numeric session name ambiguity** (tmux-router v0.2.8): `new_window()` now appends `:` to session name (`-t "0:"` instead of `-t "0"`). Without the colon, tmux interprets numeric names as window indices, creating windows in the wrong session. Root cause of persistent session 1 bleedover bug.

## 0.20.1

- **Session affinity enforcement**: Route and auto_start bail with error instead of falling back to `current_tmux_session()` when `tmux_session` is set in frontmatter. Prevents pane creation in wrong tmux session.

## 0.20.0

- **CRDT conservative dedup** (#15): Post-merge pass removes identical adjacent text blocks.
- **CRDT frontmatter patches** (#16): `patch:frontmatter` now applied on disk write path (was IPC-only).
- **Binary-vs-agent responsibility** documented in CLAUDE.md.

## 0.19.0

- **ExecutionMode in config.toml**: `execution_mode = "hybrid|parallel|sequential"` in global config.
- **TmuxBatch**: Command batching in tmux-router v0.2.7 — reduces flicker via `\;` separator. `select_pane()` uses batch (2 → 1 invocation).

## 0.18.1

- **Revert Gson**: Hand-written JSON parser restored in JetBrains plugin (Gson causes ClassNotFoundException).
- **H2 scaffolding**: `claim` scaffolds h2 headers before components for IDE code folding.
- **SKILL.md**: Canonical pattern documented — h2 header before every component.

## 0.18.0

- **`agent-doc undo`**: Restore document to pre-response state (one-deep).
- **`agent-doc extract`**: Move last exchange entry between documents.
- **`agent-doc transfer`**: Move entire component content between documents.
- **Pre-response snapshots**: Saved before every write for undo support.

## 0.17.30

- **Immutable session binding**: `claim` refuses to overwrite `tmux_session` unless `--force`. Prevents cross-session pane swapping.

## 0.17.29

- **JNA FFI integration**: `NativeLib.kt` JNA bindings for JetBrains plugin with Kotlin fallback.
- **`agent_doc_merge_frontmatter()`**: New FFI export for frontmatter patching.
- **`agent-doc lib-path`**: Print path to shared library for plugin discovery.
- **VS Code prepend mode**: Fixed missing `prepend` case in `applyComponentPatch()`.

## 0.17.28

- **Validate tmux_session before routing**: Guard against routing to a non-existent tmux session.

## 0.17.27

- **Plugin code-block fix**: JetBrains and VS Code plugins skip component tags inside fenced code blocks. JB plugin 0.2.4, VSCode 0.2.2.

## 0.17.26

- **PLUGIN-SPEC docs update**: Document recent plugin features in PLUGIN-SPEC.

## 0.17.25

- **Stash else-branch fix**: Fix else-branch stash logic. Use `diff --wait` for truncation detection.

## 0.17.24

- **Pulldown-cmark for code range detection**: Replace hand-rolled code span/fence parser with `pulldown-cmark` in component parser. Stash overflow panes instead of creating new windows.

## 0.17.23

- **Stash overflow fix**: Overflow panes stashed instead of creating new tmux windows.

## 0.17.22

- **UTF-8 corruption fix**: Sanitize component tags in response content before writing to prevent UTF-8 corruption in `sanitize_component_tags`.

## 0.17.21

- **Indented fenced code blocks**: Component parser skips markers inside indented fenced code blocks. Scaffold `agent:pending` in claim for template documents.

## 0.17.20

- **BREAKING CHANGE: Rename `mode` to `patch`** for inline component attributes (`patch=append|replace`). `mode=` accepted as backward-compatible alias.

## 0.17.19

- **Split-window in auto_start**: Use `split-window` instead of `new-window` for auto-started Claude sessions. Resync tests added.

## 0.17.18

- **Resync `--fix` enhancements**: Detect wrong-session panes and wrong-process registrations. Renamed `--dangerously-set-permissions` to `--dangerously-skip-permissions`.

## 0.17.17

- **Parse fix**: `parse_option_line` matches `[N]` bracket format only. Fix `find_registered_pane_in_session` lookup.

## 0.17.16

- **Cursor editor support**: Add Cursor as a supported editor. `claude_args` frontmatter field for custom CLI arguments. Tmux session routing fix. VS Code extension bumped to v0.2.1.

## 0.17.15

- **Route/sync improvements**: Routing and sync refinements for multi-session workflows.

## 0.17.14

- **Plugin IPC fix**: VS Code IPC parity with JetBrains. History command improvements. Documentation updates.

## 0.17.13

- **Fix exchange append mode**: Remove hardcoded replace override in `run_stream`, allowing exchange component to use its configured patch mode.

## 0.17.12

- **Inline component attributes**: `<!-- agent:name mode=append -->` — patch mode configurable directly on the component tag.

## 0.17.11

- **History command**: `agent-doc history` shows exchange version history from git with restore support. IPC-priority writes with `--force-disk` flag to bypass.

## 0.17.10

- **Default component scaffolding**: Auto-scaffold missing components on claim. Append-mode exchange default. Route flash notification via `tmux display-message`.

## 0.17.9

- **Fix CRDT character interleaving**: Switch to line-level diffs to prevent character-level interleaving artifacts.

## 0.17.8

- **Template parser code block awareness**: Component markers inside fenced code blocks are now skipped by the template parser.

## 0.17.7

- **Fix CWD drift**: Recover and claim commands no longer drift from the project root working directory.

## 0.17.6

- **Documentation update**: Align docs with IPC-first write architecture from v0.17.5.

## 0.17.5

- **IPC-first writes**: All write paths (`run`, `stream`, `write`) try IPC to the IDE plugin via `.agent-doc/patches/` before falling back to disk. Exit code 75 on IPC timeout.

## 0.17.4

- **Tmux pane orientation fix**: Arrange files side-by-side (horizontal split) instead of stacking vertically.

## 0.17.3

- **Fix CRDT character-level interleaving bug**: Resolve text corruption caused by character-level merge conflicts in CRDT state.

## 0.17.2

- **Fix CRDT shared prefix duplication bug**: Prevent duplicate content when CRDT documents share a common prefix.

## 0.17.1

- **Fix stream snapshot**: Use replace mode for exchange component in stream snapshot writes.

## 0.17.0

- **BREAKING CHANGE: `agent_doc_format`/`agent_doc_write` split**: Replace `agent_doc_mode` with separate format (`inline`|`template`) and write strategy (`disk`|`crdt`) fields. IPC write path for IDE plugins. Layout fix.

## 0.16.1

- **Native compact for template/stream mode**: `agent-doc compact` now works natively with template and stream mode documents.

## 0.16.0

- **Reactive stream mode**: CRDT-mode documents get zero-debounce reactive file-watching from the watch daemon. Truncation detection and CRDT stale base fix.

## 0.15.1

- **Patch release**: Version bump and minor fixes.

## 0.15.0

- **CRDT-based stream mode**: Real-time streaming output with CRDT conflict-free merge (`agent-doc stream`). Chain-of-thought support with optional `thinking_target` routing. Deferred commit workflow. Snapshot resolution prefers snapshot file over git.

## 0.14.9

- **Multi-backtick code span support**: `find_code_ranges` handles multi-backtick code spans (e.g., ` `` ` and ` ``` `).

## 0.14.8

- **Code-range awareness for strip_comments**: Fix `<!-- -->` stripping inside code spans and fenced blocks. Stash window purge for orphaned idle shells.

## 0.14.7

- **Bidirectional convert**: `agent-doc convert` works in both directions (inline <-> template). Autoclaim sync improvements.

## 0.14.6

- **Auto-sync on lazy claim**: Automatically sync tmux layout after lazy claim in route. Plugin autocomplete fixes for JetBrains.

## 0.14.5

- **`agent-doc commands` subcommand**: List available commands. Plugin autocomplete for JetBrains/VS Code. Remove auto-prune (moved to resync). Purge orphaned claude/stash tmux windows in resync.

## 0.14.4

- **Claim pane focus**: Focus the claimed pane after `agent-doc claim`. `convert` handles documents with pre-set template mode.

## 0.14.3

- **Autoclaim pane refresh**: Refresh pane info during autoclaim. Template missing-component recovery on write.

## 0.14.2

- **Skill reload via `--reload` flag**: Compact and restart skill installation in a single command.

## 0.14.1

- **SKILL.md workflow fix**: Move git commit to after write step in the skill workflow to prevent committing stale content.

## 0.14.0

- **Route focus fix + claim defaults to template mode**: New documents claimed via `agent-doc claim` default to template format. `agent-doc mode` CLI command for inspecting/changing document mode.

## 0.13.3

- **Bump tmux-router to v0.2.4**: Fix spare pane handling in tmux-router dependency.

## 0.13.2

- **Sync registers claims**: `agent-doc sync` registers claims for previously unregistered files in the layout.

## 0.13.1

- **Sync updates registry file paths**: Fix autoclaim file path tracking when sync moves files between panes.

## 0.13.0

- **Autoclaim + git-based snapshot fallback**: Automatic claim on route when no claim exists. Fall back to git for snapshot when snapshot file is missing.

## 0.12.2

- **Exchange component defaults to append mode**: The `exchange` component uses append patch mode by default instead of replace.

## 0.12.1

- **Lazy claim fallback**: `agent-doc claim` without `--pane` falls back to the active tmux pane.

## 0.12.0

- **`agent-doc convert` command**: Convert between inline and template document formats. Lazy claim support. `agent-doc compact` for git history squashing. Exchange component as default template target.

## 0.11.2

- **Strip trailing `## User` heading**: Also strip trailing `## User` heading from agent responses (complement to v0.11.1).

## 0.11.1

- **Strip duplicate `## Assistant` heading**: Remove duplicate `## Assistant` heading from agent responses when already present in the document.

## 0.11.0

- **Append-friendly merge strategy**: Improved 3-way merge strategy optimized for append-style document workflows.

## 0.10.1

- **Bundle template-mode instructions in SKILL.md**: SKILL.md now includes template-mode workflow instructions for the Claude Code skill.

## 0.10.0

- **BREAKING CHANGE: Rename `response_mode` to `agent_doc_mode`**: Frontmatter field renamed with backward-compatible aliases.

## 0.9.10

- **Code-span parser fix**: Component parser skips markers inside fenced code blocks and inline backticks. Template input/output component support.

## 0.9.9

- **Template mode + compaction recovery**: New template mode for in-place response documents using `<!-- agent:name -->` components. Durable pending response store for crash recovery during compaction.

## 0.9.8

- **Relocate advisory locks**: Move document advisory locks from project root to `.agent-doc/locks/`.

## 0.9.7

- **`agent-doc write` command**: Atomic response write-back command for use by the Claude Code skill.

## 0.9.6

- **Race condition mitigations**: Stale snapshot recovery, atomic file writes, and various race condition fixes.

## 0.9.5

- **Advisory file locking**: Lock the session registry during writes. Stale claim auto-pruning.

## 0.9.4

- **Bump tmux-router to v0.2**: Update tmux-router dependency.

## 0.9.3

- **Bump tmux-router to v0.1.3**: Fix stash window handling in tmux-router.

## 0.9.2

- **`agent-doc plugin install` CLI**: Install editor plugins from GitHub Releases. VS Code extension reaches feature parity with JetBrains.

## 0.9.1

- **Stash window resize fix**: Bump tmux-router to v0.1.2 to fix stash window resize issues.

## 0.9.0

- **Dashboard-as-document**: Component-based documents with `<!-- agent:name -->` markers, `agent-doc patch` for programmatic updates, `agent-doc watch` daemon for auto-submit on file change.

## 0.8.1

- **Auto-prune registry**: Prune dead session entries before route/sync/claim operations.

## 0.8.0

- **Tmux-router integration**: Wire `tmux-router` as a dependency for pane management. Fix `route` auto_start bug.

## 0.7.2

- **Attach-first reconciliation**: Sync uses attach-first strategy with auto-register for untracked panes. Column-positional focus. Tmux session affinity.

## 0.7.1

- **Additive reconciliation**: Convergent reconciliation loop (max 3 attempts) with deferred eviction and reorder phase. Nuclear rebuild fallback.

## 0.7.0

- **Snapshot-diff sync architecture**: Rewrite sync to use snapshot-based diffing for tmux layout reconciliation. Dead window handling and column inversion fix.

## 0.6.6

- **`--focus` on sync**: `agent-doc sync` accepts `--focus` flag. Inline hint notification at cursor position in JetBrains plugin.

## 0.6.5

- **Always use `sync --col`**: Single-file sync uses column mode. Break out unwanted panes. Plugin notification balloon for detected layout.

## 0.6.4

- **Sync window filtering + layout equalization**: Filter sync to target window only. Equalize pane sizes after layout.

## 0.6.3

- **LayoutDetector fix**: Skip non-splitter Container children in JetBrains plugin 3-column layout detection.

## 0.6.2

- **Fire-and-forget Junie bridge**: Junie bridge script resolved automatically. Plugin clipboard handoff for non-tmux editors.

## 0.6.1

- **Junie agent backend**: Add Junie as an agent backend with JetBrains plugin action support.

## 0.6.0

- **`agent-doc sync` command**: 2D columnar tmux layout synced to editor split arrangement. Dynamic pane groups.

## 0.5.6

- **Commit message includes doc name**: `agent-doc commit` message format now includes the document filename. `agent-doc outline` command for markdown section structure with token counts.

## 0.5.5

- **Window-scoped routing**: Route commands scoped to tmux window (not just session). `--pane`/`--window` flags. Layout safeguards. JetBrains plugin self-disabling Alt+Enter popup (removes ActionPromoter).

## 0.5.4

- **Positional claim**: `agent-doc claim <file>` accepts file as positional argument. Editor plugin improvements and SPEC updates.

## 0.5.3

- **Bundled SKILL.md with absolute snapshot paths**: Snapshot paths use absolute paths for reliability. Resync subcommand and claims log documentation.

## 0.5.2

- **Claim notifications + resync + plugin popup**: Notification on claim. `agent-doc resync` validates sessions.json and removes dead panes. JetBrains and VS Code editor plugins added.

## 0.5.1

- **Windows build fix**: Cfg-gate unix-only exec in `start.rs` for cross-platform compilation.

## 0.5.0

- **`agent-doc focus` and `agent-doc layout`**: Focus a tmux pane for a session document. Layout arranges tmux panes to mirror editor split arrangement.

## 0.4.4

- **Rename SPECS.md to SPEC.md**: Standardize specification filename.

## 0.4.3

- **Commit CWD fix**: Fix working directory for `agent-doc commit`. SKILL.md prohibition rules.

## 0.4.2

- **SPEC.md gaps filled**: Document comment stripping as skill-level behavior (§4), `--root DIR` flag for audit-docs (§7.6), `agent-doc-version` frontmatter field for auto-update detection (§7.12), and startup version check (`warn_if_outdated`).
- **Flaky test fix**: Skill tests no longer use `std::env::set_current_dir`. Refactored `install`/`check` to accept an explicit root path (`install_at`/`check_at`), eliminating CWD races in parallel test execution.
- **CLAUDE.md module layout updated**: Added `claim.rs`, `prompt.rs`, `skill.rs`, `upgrade.rs` to the documented module layout.

## 0.4.1

- **SKILL.md: comment stripping for diff**: Strip HTML comments (`<!-- ... -->`) and link reference comments (`[//]: # (...)`) before comparing snapshot vs current content. Comments are a user scratchpad and no longer trigger agent responses.
- **SKILL.md: auto-update check**: New `agent-doc-version` frontmatter field enables pre-flight version comparison. If the installed binary is newer, `agent-doc skill install` runs automatically before proceeding.
- **PromptPanel: JDialog to JLayeredPane overlay**: Replace `JDialog` popup with a `JLayeredPane` overlay in the JetBrains plugin, eliminating window-manager popup leaks.

## 0.4.0

- **`agent-doc claim <file>`**: New subcommand — claim a document for the current tmux pane. Reads session UUID from frontmatter + `$TMUX_PANE`, updates `sessions.json`. Last-call-wins semantics. Also invokable as `/agent-doc claim <file>` via the Claude Code skill.
- **`agent-doc skill install`**: Install the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. The skill content is embedded in the binary via `include_str!`, ensuring version sync.
- **`agent-doc skill check`**: Compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.
- **SKILL.md updated**: Fixed stale `$()` pattern → `agent-doc commit <FILE>`. Added `/agent-doc claim` support.
- **SPEC.md expanded**: Added §7.7–7.13 (all commands), §8 Session Routing with use case table (U1–U11), §8.3 Claim Semantics.

## 0.3.0

- **Multi-session prompt polling**: `agent-doc prompt --all` polls all live sessions in one call, returns JSON array. `SessionEntry` now includes a `file` field for document path (backward-compatible).
- **`agent-doc commit <file>`**: New subcommand — `git add -f` + commit with internally-generated timestamp. Replaces shell `$()` substitution in IDE/skill workflows.
- **Prompt detection**: `agent-doc prompt` subcommand added in v0.2.0 (unreleased).
- **send-keys fix**: Literal text (`-l`) + separate Enter, `new-window -a` append flag (unreleased since v0.2.0).

## 0.1.4

- **`agent-doc upgrade` self-update**: Downloads prebuilt binary from GitHub Releases as the primary upgrade strategy. Falls back to `cargo install`, then `pip install --upgrade`, then manual instructions including `curl | sh`.

## 0.1.3

- **Upgrade check**: Queries crates.io for latest version with a 24h cache. Prints a one-line stderr warning on startup if outdated.
- **`agent-doc upgrade`**: New subcommand tries `cargo install` then `pip install --upgrade`, or prints manual instructions.

## 0.1.2

- **Language-agnostic audit-docs**: Replace Cargo.toml-only root detection with 3-pass strategy (project markers → .git → CWD fallback). Scan 28 file extensions across 6 source dirs instead of .rs only.
- **--root CLI flag**: Override auto-detection of project root for audit-docs.
- **Test coverage**: Add unit tests for frontmatter, snapshot, and diff modules.

## 0.1.0

Initial release.

- **Interactive document sessions**: Edit a markdown document, run an AI agent, response appended back into the document.
- **Session continuity**: YAML frontmatter tracks session ID, agent backend, and model. Fork from current session on first run, resume on subsequent.
- **Diff-based runs**: Only changed content is sent as a diff, with the full document for context. Double-run guard via snapshots.
- **Merge-safe writes**: 3-way merge via `git merge-file` if the file is edited during agent response. Conflict markers written on merge failure.
- **Git integration**: Pre-commit user changes before agent call, leave agent response uncommitted for editor diff gutters. `-b` flag for auto-branch, `--no-git` to skip.
- **Agent backends**: Agent-agnostic core. Claude backend included. Custom backends configurable via `~/.config/agent-doc/config.toml`.
- **Commands**: `run`, `init`, `diff`, `reset`, `clean`, `audit-docs`.
- **Editor integration**: JetBrains External Tool, VS Code task, Vim/Neovim mapping.
