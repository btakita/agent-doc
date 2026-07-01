//! # Module: queue_continuation
//!
//! Binary-owned "queue continuation required" final gate
//! (`#codex-auto-queue-stalled-final-gate`).
//!
//! Codex auto-queue continuation historically depended on the `codex-stop` hook
//! finding tracked in-memory session state and then calling
//! `active_auto_queue_prompt`. That is too fragile for the live failure mode:
//! the Stop hook can miss the document when `UserPromptSubmit` did not persist
//! state for the exact API/session/root shape, or when the turn closed through a
//! manual / recovery path after a recursive direct-invocation rejection. A clean
//! `session-check` is not enough either — a committed document can still owe an
//! auto-queue continuation.
//!
//! The only durable proof after closeout is the document itself: explicit `go`
//! mode plus `queue_active: true` plus a ready head. `auto`/`start` are start
//! triggers; they do not silently opt a document into unattended continuation
//! after the current closeout. The detector here is the single shared source of
//! truth; `session-check`, the `codex-stop` hook, and the closeout paths all
//! consult it instead of duplicating the activation reasoning.

use agent_doc_queue::queue_continuation as queue_policy;
use agent_doc_queue_io::continuation_marker::{
    ContinuationMarker, clear_continuation_marker, write_continuation_marker,
};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Detect whether `file` currently requires queue continuation.
///
/// True only when: frontmatter `queue_active: true`, explicit `go` mode,
/// [`agent_doc_queue::document_queue::resolve_activation`] is active, the head is a real prompt
/// (not a stop fence or future time gate), and the head was not edited between
/// the committed snapshot and the file.
///
/// `auto` and `start` are start triggers only. A persisted-active plain
/// `agent:queue` without `queue: go` or marker-side `go` is not a self-driving
/// loop. This mirrors the codex-hook `active_auto_queue_prompt` logic in one
/// shared, testable place.
pub fn detect(file: &Path) -> Result<Option<queue_policy::QueueContinuation>> {
    // `#qpausego` note: a controller `admin queue pause` does NOT short-circuit
    // continuation here. The pause suppresses the *unattended* supervisor
    // idle-watch auto-injection (see `start/idle_watch.rs`), but the attended
    // in-session `/loop` — and `session-check` / the codex-stop continuation
    // gate that consult this detector — must keep draining real queue work. A
    // pause stalling the in-session loop strands genuine drainable backlog (the
    // operator-rejected over-reach); `queue: stop` / `--- stop` is the in-session
    // stop control.
    // `#wd40` / `#staleloop-recycle-restart`: when the route-owned supervisor is
    // stale and has asked the in-session loop to yield one boundary so it can
    // hot-reload onto a freshly-installed binary, report no continuation so the
    // loop ends its turn. The idle boundary lets the `execve` recycle fire; the
    // fresh supervisor clears the request and the drain resumes. This is a
    // *temporary* yield (the request is short-TTL and cleared post-recycle), not a
    // drained queue — surfaces are expected to print
    // [`agent_doc_queue::queue_continuation::RECYCLE_YIELD_GUIDANCE`].
    // The supervisor's OWN idle-watch drain uses `live_drainable_continuation_head`
    // (not this), so it is unaffected and resumes the drain after recycling.
    if agent_doc_supervisor_io::recycle_yield::recycle_yield_pending(file) {
        return Ok(None);
    }
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let snapshot_content = crate::snapshot::load(file)?;
    queue_policy::required_continuation(&content, snapshot_content.as_deref())
}

/// Whether the document's effective controller queue-control state is `paused`
/// (`#qpausego`).
///
/// An accepted `agent-doc admin queue pause <FILE>` records a durable
/// `queue_controls` row that the controller *dispatch* RPC already honors
/// (`load_effective_queue_control_from_db` → `failed_stage=queue_paused`). But
/// the supervisor idle-watch injects `agent-doc <FILE>` triggers straight into
/// the pane — bypassing that RPC — and this continuation signal was computed from
/// the document alone, so a `go`-mode auto-queue kept re-dispatching after an
/// accepted pause. Resolving the controller pause here lets both the idle-watch
/// drain decision and `preflight` defer to an accepted pause even for `go`-mode
/// queues. `resume`/`drain` are not `paused`, so they do not block here (the
/// controller owns draining).
///
/// Best-effort and read-only: returns `false` (not paused) when the project root
/// or controller state DB cannot be resolved/opened, so a missing control plane
/// never wedges an otherwise-active queue. A non-absent open/query error is
/// logged to stderr (never silently swallowed) and treated as not-paused so a
/// transient DB hiccup cannot strand the drain.
pub fn document_queue_controller_paused(file: &Path) -> bool {
    document_queue_controller_pause_reason(file).is_some()
}

/// The effective controller pause reason for `file` when (and only when) the
/// queue-control state is `paused` (`#qpausego` / `#qpausemix`).
///
/// Returns `Some(reason)` when an accepted `admin queue pause` is the effective
/// control state — `reason` is the operator/controller-recorded pause reason, or
/// an empty string when the pause carried none. Returns `None` when the queue is
/// not controller-paused (or the control plane / state DB cannot be resolved).
///
/// Surfacing the reason is what resolves the operator-perceived "mixed signal"
/// (`queue_paused: true` alongside `queue_continuation_required: true`): the
/// reason and the pause-aware
/// [`agent_doc_queue::queue_continuation::continuation_guidance`] preamble let
/// the agent see *why* the queue was paused and that the pause only suppresses the
/// unattended supervisor idle-watch, instead of guessing whether the pause is
/// operator intent or transient drain-coordination state. Same best-effort,
/// read-only error handling as [`document_queue_controller_paused`].
pub fn document_queue_controller_pause_reason(file: &Path) -> Option<String> {
    let root = agent_doc_fs::find_project_root(file)?;
    let db_path = agent_doc_sqlite::state_store::state_db_path(&root);
    if !db_path.exists() {
        // No control plane has ever run for this project: nothing can be paused.
        return None;
    }
    let canonical = file.canonicalize().ok()?;
    let document_id = canonical.to_string_lossy().to_string();
    let conn = match agent_doc_sqlite::state_store::open_state_db(&root) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_continuation: failed to open controller state DB at {} ({err:#}) — treating queue as not controller-paused",
                root.display()
            );
            return None;
        }
    };
    match agent_doc_sqlite::state_store::load_effective_queue_control_from_db(
        &conn,
        &document_id,
        &root.to_string_lossy(),
    ) {
        Ok(control) => control.and_then(|control| {
            (control.state == "paused").then(|| control.reason.unwrap_or_default())
        }),
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_continuation: failed to load controller queue control for {} ({err:#}) — treating queue as not controller-paused",
                file.display()
            );
            None
        }
    }
}

/// Reconcile the durable continuation marker for `file` after a successful
/// closeout: write it when a continuation is required, clear it otherwise
/// (queue drained, `auto` removed, `queue_active` false, or head advanced).
/// Best-effort and never fatal to closeout — a marker write/clear failure is
/// logged, not propagated.
pub fn reconcile_marker(
    file: &Path,
    source_command: &str,
) -> Option<queue_policy::QueueContinuation> {
    match detect(file) {
        Ok(Some(continuation)) => {
            if let Err(err) = write_continuation_marker(file, &continuation, source_command) {
                eprintln!(
                    "[queue-continuation] WARNING: failed to write continuation marker for {}: {}",
                    file.display(),
                    err
                );
            }
            Some(continuation)
        }
        Ok(None) => {
            if let Err(err) = clear_continuation_marker(file) {
                eprintln!(
                    "[queue-continuation] WARNING: failed to clear continuation marker for {}: {}",
                    file.display(),
                    err
                );
            }
            None
        }
        Err(err) => {
            eprintln!(
                "[queue-continuation] WARNING: continuation detect failed for {}: {}",
                file.display(),
                err
            );
            None
        }
    }
}

/// Scan every project root for a durable continuation marker whose document
/// still requires continuation. Used by the Codex Stop hook when no tracked
/// in-memory session state is available. Returns the first still-valid
/// `(file, continuation, marker)`.
/// `#codex-stop-cross-doc-queue-continuation`: a durable Stop-hook marker is
/// owned by *some* document; the fallback must not instruct the current Codex
/// pane to run a document owned by ANOTHER live actor. A marker doc is foreign
/// when it has a live (non-Closed) authoritative actor bound to a pane other
/// than `current_pane`. Unknown / closed / unowned ownership is NOT foreign
/// (allowed — covers the safe-claim and same-session cases). `current_pane` of
/// `None` (no tmux context) disables the gate and preserves prior behavior.
fn is_foreign_owned_marker(root: &Path, doc: &Path, current_pane: &str) -> bool {
    match crate::project_controller::authoritative_actor_binding(root, doc) {
        Ok(Some(record))
            if record.state != agent_doc_sqlite::state_store::ActorState::Closed
                && !record.pane_id.trim().is_empty() =>
        {
            record.pane_id != current_pane
        }
        _ => false,
    }
}

/// Find the first still-valid durable `agent:queue auto` continuation marker
/// across `roots`. `current_pane` is the tmux pane of the Codex session whose
/// Stop hook is asking; markers owned by a different live pane are skipped (the
/// scan continues) so the hook never tells pane A to run document B while B has
/// its own live owner (`#codex-stop-cross-doc-queue-continuation`).
pub fn pending_marker_continuation_for_roots(
    roots: &[PathBuf],
    current_pane: Option<&str>,
) -> Result<Option<(PathBuf, queue_policy::QueueContinuation, ContinuationMarker)>> {
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let dir = root.join(".agent-doc/queue-continuations");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).with_context(|| format!("read {}", dir.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(marker) = serde_json::from_str::<ContinuationMarker>(&content) else {
                continue;
            };
            let doc = PathBuf::from(&marker.file);
            if !seen.insert(doc.clone()) {
                continue;
            }
            // `#codex-stop-cross-doc-queue-continuation`: skip a marker owned by
            // another live actor (different pane) and keep scanning, so this
            // Codex pane is never told to run a foreign-owned document. Does NOT
            // remove the marker — it stays for that document's own owner.
            if let Some(current) = current_pane
                && is_foreign_owned_marker(root, &doc, current)
            {
                crate::ops_log::log_op(
                    &doc,
                    &format!(
                        "codex_stop_foreign_queue_marker_skip file={} current_pane={}",
                        doc.display(),
                        current
                    ),
                );
                continue;
            }
            // The marker is durable but advisory — re-confirm against the live
            // document so a stale marker (queue since drained / edited) never
            // forces a spurious continuation.
            match detect(&doc)? {
                Some(continuation) => return Ok(Some((doc, continuation, marker))),
                None => {
                    // Stale marker — clean it up so it cannot mislead later.
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_continuation::{
        DrainScope, deferred_backlog_ids, deferred_head_count, drainable_head_count,
        extract_head_id, head_requires_clean_session_in, head_requires_context_reset_in,
        head_requires_focused_cycle_in, is_drainable_queue_head,
        is_drainable_queue_head_with_context, is_recurring_imperative_head, live_continuation_head,
        live_drainable_continuation_head, open_review_item_count, queue_stale_noise_lines,
        review_phase_routed, supervisor_deferred_backlog_ids,
    };
    use agent_doc_queue_io::continuation_marker::{
        continuation_marker_path, load_continuation_marker, record_continuation_requested_head,
    };

    fn write_doc(dir: &Path, prompts: &[&str], queue_active: bool, has_auto: bool) -> PathBuf {
        let queue_attrs = if has_auto { " auto go" } else { "" };
        write_doc_with_queue_attrs(dir, prompts, queue_active, queue_attrs)
    }

    /// `#qpausego`: set the document-scope controller queue-control state for a doc.
    fn set_document_queue_control(root: &Path, doc: &Path, state: &str) {
        let conn = agent_doc_sqlite::state_store::open_state_db(root).unwrap();
        let scope_id = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_sqlite::state_store::upsert_queue_control_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &scope_id,
                state,
                reason: Some("test pause"),
                operation_receipt_id: None,
            },
        )
        .unwrap();
    }

    /// `#qpausego`: with no controller state DB at all, a doc is never reported
    /// as controller-paused (a missing control plane must not wedge a queue).
    #[test]
    fn document_queue_controller_paused_false_without_state_db() {
        let dir = tempfile::tempdir().unwrap();
        // `.agent-doc` must exist for project-root resolution, but no state DB.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = write_doc(dir.path(), &["do [#a]"], true, true);
        assert!(
            !document_queue_controller_paused(&doc),
            "no state DB means nothing can be controller-paused"
        );
    }

    /// `#qpausego`: an accepted `admin queue pause` (document-scope `paused`
    /// control row) is reported as paused; `resume` clears it.
    #[test]
    fn document_queue_controller_paused_reflects_paused_then_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#a]"], true, true);

        set_document_queue_control(dir.path(), &doc, "paused");
        assert!(
            document_queue_controller_paused(&doc),
            "an accepted pause must report the queue as controller-paused"
        );

        set_document_queue_control(dir.path(), &doc, "resumed");
        assert!(
            !document_queue_controller_paused(&doc),
            "resume must clear the controller pause"
        );
    }

    /// `#qpausego`: a `paused` controller state must NOT short-circuit
    /// continuation — the attended in-session `/loop` (and `session-check` /
    /// codex-stop, which consult `detect`) keeps draining real queue work. The
    /// pause only suppresses the unattended supervisor idle-watch injection.
    #[test]
    fn detect_still_continues_when_controller_paused() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do something"], true, true);
        assert!(
            detect(&doc).unwrap().is_some(),
            "active auto queue with a live head should require continuation"
        );

        set_document_queue_control(dir.path(), &doc, "paused");
        assert!(
            detect(&doc).unwrap().is_some(),
            "a controller pause must NOT stall the in-session loop continuation"
        );
    }

    fn write_doc_with_queue_attrs(
        dir: &Path,
        prompts: &[&str],
        queue_active: bool,
        queue_attrs: &str,
    ) -> PathBuf {
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        write_doc_with_queue_body(dir, &queue, queue_active, queue_attrs)
    }

    fn write_doc_with_queue_body(
        dir: &Path,
        queue_body: &str,
        queue_active: bool,
        queue_attrs: &str,
    ) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/snapshots")).unwrap();
        let doc = dir.join("task.md");
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: {queue_active}\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue{queue_attrs} -->\n{queue_body}<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        doc
    }

    fn doc_with_backlog(queue_prompts: &[&str], backlog_items: &[&str]) -> String {
        let queue: String = queue_prompts.iter().map(|p| format!("- {p}\n")).collect();
        let backlog: String = backlog_items.iter().map(|b| format!("{b}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Backlog\n\n<!-- agent:backlog queue=sync -->\n{backlog}<!-- /agent:backlog -->\n\n\
## Queue\n\n<!-- agent:queue auto go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn deferred_backlog_ids_defers_only_operator_verify() {
        // #qcontdrain: ONLY [operator-verify] is deferred. [clean-session] is
        // always drainable now — the in-session /loop drains it in place rather
        // than deferring to a possibly-stalled supervisor — so live IPC state no
        // longer changes the deferred set (the function is now content-only).
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]", "do [#c]"],
            &[
                "- [ ] [#a] [clean-session] needs quiet",
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain",
            ],
        );
        let deferred = deferred_backlog_ids(&content);
        assert!(
            !deferred.contains("a"),
            "clean-session drains in-loop (#qcontdrain)"
        );
        assert!(deferred.contains("b"), "operator-verify always deferred");
        assert!(!deferred.contains("c"));
    }

    fn doc_with_review(review_items: &[&str]) -> String {
        let review: String = review_items.iter().map(|r| format!("{r}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Review\n\n<!-- agent:review -->\n{review}<!-- /agent:review -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#next]\n<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn review_phase_routed_detects_added_open_review_item() {
        // #mphaseloop: moving a phase to agent:review (a gated `[/]` item) is the
        // signal that this cycle routed-to-review rather than completed/blocked.
        let none = doc_with_review(&[]);
        let one = doc_with_review(&["- [/] [#p1] phase 1 needs live verify"]);
        assert!(
            review_phase_routed(&none, &one),
            "a newly-added open review item is a routed phase"
        );
        // No delta → not routed (idempotent re-commit must not re-fire the proof).
        assert!(!review_phase_routed(&one, &one));
        // Fewer open items (a review item reaped/completed) is not a routed phase.
        assert!(!review_phase_routed(&one, &none));
        // A `[x]` done item does not count as an open routed phase.
        let done = doc_with_review(&["- [x] [#p1] phase 1 reviewed"]);
        assert!(!review_phase_routed(&none, &done));
    }

    #[test]
    fn review_phase_routed_counts_only_review_component() {
        // A growing backlog/queue must NOT be misread as a routed review phase —
        // only the agent:review component's open-item delta counts.
        let prior = doc_with_review(&["- [/] [#p1] one"]);
        let more_reviews = doc_with_review(&["- [/] [#p1] one", "- [/] [#p2] two"]);
        assert!(review_phase_routed(&prior, &more_reviews));
        assert_eq!(open_review_item_count(&prior), 1);
        assert_eq!(open_review_item_count(&more_reviews), 2);
        assert_eq!(open_review_item_count(&doc_with_review(&[])), 0);
    }

    #[test]
    fn every_clean_session_head_drains() {
        // #qcontdrain: clean-session heads drain in-loop unconditionally (the
        // `#freshgrant` grant machinery is gone). Only operator-verify defers.
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]", "do [#e]"],
            &[
                "- [ ] [#a] [clean-session] one",
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#e] [clean-session] two",
            ],
        );
        let deferred = deferred_backlog_ids(&content);
        assert!(!deferred.contains("a"), "clean-session drains in-loop");
        assert!(
            !deferred.contains("e"),
            "every clean-session head drains in-loop"
        );
        assert!(
            deferred.contains("b"),
            "operator-verify is never drainable by any agent scope"
        );
    }

    #[test]
    fn supervisor_drains_focused_cycle_but_loop_defers_it() {
        // #qfocsup: a [focused-cycle] head is deferred by the in-session loop (it
        // cannot give the fresh context the tag demands) but DRAINED by the
        // supervisor (force-/clear + re-dispatch), so it never strands the queue
        // idle. [operator-verify] stays deferred in BOTH scopes.
        let content = doc_with_backlog(
            &["do [#f]", "do [#o]"],
            &[
                "- [ ] [#f] [focused-cycle] merge-core work",
                "- [ ] [#o] [operator-verify] live drive",
            ],
        );
        let loop_deferred = deferred_backlog_ids(&content);
        assert!(
            loop_deferred.contains("f"),
            "focused-cycle deferred by in-session loop"
        );
        assert!(
            loop_deferred.contains("o"),
            "operator-verify deferred by in-session loop"
        );

        let sup_deferred = supervisor_deferred_backlog_ids(&content);
        assert!(
            !sup_deferred.contains("f"),
            "focused-cycle is supervisor-drainable (#qfocsup)"
        );
        assert!(
            sup_deferred.contains("o"),
            "operator-verify never drainable by any scope"
        );
    }

    #[test]
    fn focused_cycle_head_drains_for_supervisor_not_in_session_loop() {
        // The queue is NOT idle when only a [focused-cycle] head remains: the
        // supervisor picks it up (and force-/clears first), while the in-session
        // loop yields it (drainable_head_count == 0).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("d.md");
        let content = doc_with_backlog(
            &["do [#f]"],
            &["- [ ] [#f] [focused-cycle] merge-core work"],
        );
        std::fs::write(&doc, &content).unwrap();

        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some("f"),
            "supervisor drains the focused-cycle head (clear-and-continue)"
        );
        assert_eq!(
            drainable_head_count(&content),
            0,
            "in-session loop yields the focused-cycle head to the supervisor"
        );
        assert!(
            head_requires_context_reset_in(&content, "do [#f]"),
            "supervisor force-/clears before a focused-cycle head"
        );
    }

    #[test]
    fn head_requires_context_reset_covers_clean_session_and_focused_cycle() {
        let content = doc_with_backlog(
            &["do [#c]", "do [#f]", "do [#o]", "do [#p]"],
            &[
                "- [ ] [#c] [clean-session] quiet",
                "- [ ] [#f] [focused-cycle] merge-core",
                "- [ ] [#o] [operator-verify] live",
                "- [ ] [#p] plain",
            ],
        );
        assert!(head_requires_context_reset_in(&content, "c"));
        assert!(head_requires_context_reset_in(&content, "f"));
        assert!(
            !head_requires_context_reset_in(&content, "o"),
            "operator-verify is not a context-reset (it is never auto-dispatched)"
        );
        assert!(!head_requires_context_reset_in(&content, "p"));
        // clean-session-only check stays narrow (focused-cycle excluded).
        assert!(head_requires_clean_session_in(&content, "c"));
        assert!(!head_requires_clean_session_in(&content, "f"));
        assert!(!head_requires_focused_cycle_in(&content, "c"));
        assert!(head_requires_focused_cycle_in(&content, "f"));
    }

    #[test]
    fn head_requires_clean_session_maps_head_id_to_tag() {
        // #cleandrainsup: the idle-watch maps the active head (an `#id`) back to its
        // backlog item's `[clean-session]` tag to decide whether to force a `/clear`.
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]"],
            &[
                "- [ ] [#a] [clean-session] needs quiet",
                "- [ ] [#b] plain drainable",
            ],
        );
        assert!(head_requires_clean_session_in(&content, "a"));
        assert!(head_requires_clean_session_in(&content, "do [#a]"));
        assert!(!head_requires_clean_session_in(&content, "b"));
        assert!(!head_requires_clean_session_in(&content, "nonexistent"));
    }

    #[test]
    fn detect_skips_deferred_head_and_continues_on_drainable() {
        // #goqueuestall: with no live IPC listener, the clean-session head drains
        // normally, so continuation lands on it (first drainable head).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#b]", "do [#c]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain drainable",
            ],
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        // No socket → not live IPC. operator-verify (#b) is deferred regardless,
        // so continuation must land on the drainable #c head.
        let continuation = detect(&doc).unwrap().expect("drainable head remains");
        assert_eq!(continuation.head_id.as_deref(), Some("c"));
    }

    #[test]
    fn detect_none_when_only_deferred_heads_remain() {
        // #goqueuestall: when every remaining head is undrainable
        // (operator-verify), continuation is NOT required — this is the stall fix.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#b]", "do [#d]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#d] [operator-verify] also live",
            ],
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        assert!(
            detect(&doc).unwrap().is_none(),
            "all-deferred heads must not require continuation"
        );
        assert_eq!(deferred_head_count(&content), 2);
    }

    #[test]
    fn drainable_head_count_zero_when_only_deferred_heads_remain() {
        // #cleardrainsignal: the preflight no-stall signal must read 0 drainable
        // heads when every remaining head is undrainable (operator-verify always),
        // so the agent / Claude Code auto-loop never loops a #qchurn no-op cycle.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#b]", "do [#d]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#d] [operator-verify] also live",
            ],
        );
        std::fs::write(&doc, &content).unwrap();
        assert_eq!(drainable_head_count(&content), 0);
    }

    #[test]
    fn drainable_head_count_counts_only_real_drainable_heads() {
        // #cleardrainsignal: a deferred head is excluded; prose reports and
        // directive heads both count as drainable work.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &[
                "do [#b]",
                "the kanban is missing the accepted application section",
                "do [#c]",
            ],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain drainable",
            ],
        );
        std::fs::write(&doc, &content).unwrap();
        // #b deferred (operator-verify), the prose report and #c remain real work.
        assert_eq!(drainable_head_count(&content), 2);
    }

    #[test]
    fn drainable_head_count_excludes_orphan_id_head_when_backlog_present() {
        // #cleardrainsignal: a `do [#id]` head whose id is absent from the open
        // backlog (completed/archived/orphan) is stale — it must not keep the
        // go-mode drain alive when the doc is backlog-driven.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#orphan]", "do [#c]"],
            &["- [ ] [#c] plain drainable"],
        );
        std::fs::write(&doc, &content).unwrap();
        // #orphan has no backlog item → excluded; only #c counts.
        assert_eq!(drainable_head_count(&content), 1);
    }

    #[test]
    fn drainable_head_count_free_form_id_queue_without_backlog_still_drains() {
        // #cleardrainsignal regression guard: when the doc has NO backlog component,
        // id-heads are the work themselves and must stay drainable.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]", "do [#y]"], true, true);
        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(drainable_head_count(&content), 2);
    }

    #[test]
    fn is_drainable_queue_head_treats_fenced_paste_as_noise() {
        // #cleardrainsignal: a pure pasted console block (fenced ```) is noise
        // even when it incidentally contains directive words.
        let fenced = "```\n[route] target tmux session: 0\nError: run blocked\n```";
        assert!(!is_drainable_queue_head(fenced));
        // A prose bug report with diagnostic evidence is still operator-authored
        // queue work and must not be demoted to noise.
        let report = ":pushpin: JB `Run Agent Doc` should self-heal.\n```\n[route] target tmux session: 0\nError: dispatch blocked\n```";
        assert!(is_drainable_queue_head(report));
        // A genuine inline directive (no fence) stays drainable.
        assert!(is_drainable_queue_head("run the full test suite"));
    }

    #[test]
    fn preset_queue_treats_prompt_lines_as_drainable_directives() {
        // #goqnoise: with a queue-level preset, the preset supplies the verb for
        // each non-empty prompt line; the line itself is the object to act on.
        let dir = tempfile::tempdir().unwrap();
        let cpa_line = "\"CPA / attorney letter or supporting document\" in https://example.test/accreditation and the dialog should allow multiple files. Use our standard multi-file upload component.";
        let doc = write_doc_with_queue_attrs(
            dir.path(),
            &[cpa_line, "deploy"],
            true,
            " auto preset=\"#spec-test-commit-push\" go",
        );
        let content = std::fs::read_to_string(&doc).unwrap();

        let continuation = detect(&doc).unwrap().expect("preset queue should drain");
        assert_eq!(continuation.head_prompt, cpa_line);
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some(cpa_line)
        );
        assert_eq!(drainable_head_count(&content), 2);
        assert_eq!(
            queue_stale_noise_lines(&std::fs::read_to_string(&doc).unwrap()),
            0
        );
    }

    #[test]
    fn is_drainable_queue_head_treats_cross_doc_response_fragment_as_noise() {
        // #qcontam: a cross-doc agent response-fragment bullet that a CRDT merge
        // split into this queue is noise even under a preset, even though it has no
        // fence and incidentally contains directive verbs / stray ids.
        let bold_lead = ":pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).";
        assert!(!is_drainable_queue_head(bold_lead));
        assert!(!is_drainable_queue_head_with_context(bold_lead, true));
        let resolved_lead = ":pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done).";
        assert!(!is_drainable_queue_head_with_context(resolved_lead, true));
        // An agent component/boundary comment spliced into a head is also noise.
        let boundary = ":pushpin: stray response tail\n<!-- agent:boundary:7c96f9ce -->";
        assert!(!is_drainable_queue_head_with_context(boundary, true));
        // A genuine operator directive (no bold lead, no agent markers) stays
        // drainable under a preset, even when long.
        let legit = "All upload preview dialogs should be full screen. deploy";
        assert!(is_drainable_queue_head_with_context(legit, true));
        // A plain `**bold**`-free imperative is still drainable without a preset.
        assert!(is_drainable_queue_head("run the full test suite"));
    }

    #[test]
    fn preset_queue_treats_prose_plus_fenced_diagnostics_as_drainable() {
        // A queue-level preset supplies the directive; a prose bug report followed
        // by fenced diagnostics is real work, not stale noise.
        let dir = tempfile::tempdir().unwrap();
        let fenced = ":pushpin: JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. The actor record did not switch.\n```\n[route] target tmux session: 0\nError: authoritative actor record is bound to harness claude-code, not codex\n```";
        let queue_body = format!("~~~prompt\n{fenced}\n~~~\n");
        let doc = write_doc_with_queue_body(
            dir.path(),
            &queue_body,
            true,
            " auto preset=\"#spec-test-commit-push\" go",
        );
        let content = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(
            detect(&doc)
                .unwrap()
                .map(|continuation| continuation.head_prompt),
            Some(fenced.to_string())
        );
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some(fenced)
        );
        assert_eq!(drainable_head_count(&content), 1);
        assert_eq!(
            queue_stale_noise_lines(&std::fs::read_to_string(&doc).unwrap()),
            0
        );
    }

    #[test]
    fn preset_separator_freetext_drains_ahead_of_operator_verify_pins() {
        // Live regression: a free-text bug report typed before a `---` separator
        // must not be skipped as Freeform/noise, or preflight sees only the
        // operator-verify mirror heads and stalls with 0 drainable work.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("task.md");
        let prompt = concat!(
            "JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. ",
            "The session switched from claude to codex, but the actor record did not switch.\n",
            "```\n",
            "[route] target tmux session: 0\n",
            "Error: authoritative actor record is bound to harness claude-code, not codex\n",
            "```",
        );
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" priority go -->\n{prompt}\n---\n- :pushpin: do [#ov]\n<!-- /agent:queue -->\n\n\
## Backlog\n\n<!-- agent:backlog priority queue -->\n- [ ] [#ov] [operator-verify] live drive\n<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();

        assert_eq!(
            detect(&doc)
                .unwrap()
                .map(|continuation| continuation.head_prompt),
            Some(prompt.to_string())
        );
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some(prompt)
        );
        assert_eq!(drainable_head_count(&content), 1);
        assert_eq!(
            queue_stale_noise_lines(&std::fs::read_to_string(&doc).unwrap()),
            0
        );
    }

    #[test]
    fn preset_queue_treats_all_log_fenced_paste_as_noise() {
        // #goqnoise: preset context must not re-admit pure console evidence.
        let dir = tempfile::tempdir().unwrap();
        let fenced = "```\n[route] target tmux session: 0\nError: dispatch blocked\n```";
        let queue_body = format!("~~~prompt\n{fenced}\n~~~\n");
        let doc = write_doc_with_queue_body(
            dir.path(),
            &queue_body,
            true,
            " auto preset=\"#spec-test-commit-push\" go",
        );
        let content = std::fs::read_to_string(&doc).unwrap();

        assert!(detect(&doc).unwrap().is_none());
        assert!(live_drainable_continuation_head(&content, DrainScope::Supervisor).is_none());
        assert_eq!(drainable_head_count(&content), 0);
        assert_eq!(
            queue_stale_noise_lines(&std::fs::read_to_string(&doc).unwrap()),
            1
        );
    }

    #[test]
    fn detect_returns_head_for_active_auto_go_queue() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#seopdp] next", "do [#third]"],
            true,
            true,
        );
        let continuation = detect(&doc).unwrap().expect("ready auto-queue head");
        assert_eq!(continuation.head_prompt, "do [#seopdp] next");
        assert_eq!(continuation.head_id.as_deref(), Some("seopdp"));
    }

    #[test]
    fn detect_yields_when_supervisor_requests_recycle_yield() {
        // `#wd40` / `#staleloop-recycle-restart`: a stale supervisor that can never
        // reach its own recycle boundary during a continuously self-draining session
        // writes a recycle-yield request; while it is live the in-session loop must
        // see NO continuation (so it ends its turn and the execve recycle fires),
        // even though the active queue still has drainable heads. Clearing the
        // request restores normal continuation so the drain resumes post-recycle.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#seopdp] next", "do [#third]"],
            true,
            true,
        );
        let doc_str = doc.to_string_lossy().to_string();

        // Baseline: continuation is owed.
        assert!(detect(&doc).unwrap().is_some());

        // A live recycle-yield request suppresses continuation entirely.
        agent_doc_supervisor_io::recycle_yield::request_recycle_yield(
            &doc_str,
            agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STALE_BINARY,
        )
        .unwrap();
        assert!(
            detect(&doc).unwrap().is_none(),
            "a pending recycle-yield must make the in-session loop yield"
        );

        // Clearing the request hands the drain back so the loop resumes.
        agent_doc_supervisor_io::recycle_yield::clear_recycle_yield(&doc_str);
        assert!(
            detect(&doc).unwrap().is_some(),
            "clearing the recycle-yield must restore normal continuation"
        );
    }

    #[test]
    fn detect_none_for_persisted_active_queue_without_go() {
        // A persisted-active queue without explicit `go` is not an unattended
        // continuation loop. `auto`/`start` are start triggers only.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#persisted] next", "do [#third]"],
            true,
            false,
        );
        assert!(
            detect(&doc).unwrap().is_none(),
            "plain persisted-active queues must not continue without go"
        );
    }

    #[test]
    fn detect_returns_head_for_active_go_queue_without_auto() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc_with_queue_attrs(
            dir.path(),
            &["do [#persisted] next", "do [#third]"],
            true,
            " go",
        );
        let continuation = detect(&doc).unwrap().expect("ready go-mode head");
        assert_eq!(continuation.head_prompt, "do [#persisted] next");
        assert_eq!(continuation.head_id.as_deref(), Some("persisted"));
        assert!(
            continuation.reason.contains("go"),
            "go-mode reason should name the go trigger, got: {}",
            continuation.reason
        );
    }

    #[test]
    fn detect_none_when_inactive_plain_queue() {
        // A queue without `auto` AND without `queue_active: true` must never
        // self-start — the `queue_active` guard fails first.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]"], false, false);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_queue_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]"], false, true);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_queue_drained() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &[], true, true);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_stop_fence_at_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["-- stop placeholder"], true, true);
        // Replace the queue body with a real stop fence at the head.
        let content = std::fs::read_to_string(&doc)
            .unwrap()
            .replace("- -- stop placeholder\n", "--- stop\n- do [#x]\n");
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        // A stop fence at the head must not force continuation.
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn extract_head_id_handles_bracket_and_bare() {
        assert_eq!(extract_head_id("do [#abc] thing").as_deref(), Some("abc"));
        assert_eq!(
            extract_head_id("#bare-id do it").as_deref(),
            Some("bare-id")
        );
        assert_eq!(extract_head_id("no id here"), None);
    }

    #[test]
    fn is_drainable_queue_head_classifies_directive_vs_noise() {
        // #goqstall2: `#id` heads, directives, questions, and prose reports are
        // drainable.
        assert!(is_drainable_queue_head(":round_pushpin: do [#fcc0]"));
        assert!(is_drainable_queue_head("- :pushpin: Fix the submit bug"));
        assert!(is_drainable_queue_head(
            "JB Run Agent Doc ... does not submit. Fix and add Simworld regression tests."
        ));
        assert!(is_drainable_queue_head(
            "JB Clear Exchange ... the content came back...from a stale HEAD?"
        ));
        assert!(is_drainable_queue_head("#bare-id continue the drain"));
        // Harness slash commands are drainable command heads, not prose noise.
        assert!(is_drainable_queue_head("- /model sonnet"));
        assert!(is_drainable_queue_head(":pushpin: /clear"));
        assert!(is_drainable_queue_head("deploy"));
        assert!(is_drainable_queue_head(
            "- I'm still seeing JB `File Cache Conflict` dialogs. There should be 0."
        ));
        assert!(is_drainable_queue_head(
            ":pushpin: JB `Compact Exchange` has a partially uncommitted response."
        ));
        assert!(is_drainable_queue_head(
            "Queue items are being struck without being worked on."
        ));
        assert!(is_drainable_queue_head(
            "- This document has `agent:backlog priority queue`...but the backlog items are not being added to the `agent:queue`."
        ));
        assert!(is_drainable_queue_head(
            ":pushpin: Ensure the markdown AST handles strings and code blocks. Tags within the blocks should be treated as content, not as part of the document tags."
        ));

        // Empty or artifact-only heads remain noise.
        assert!(!is_drainable_queue_head("- "));
        assert!(!is_drainable_queue_head(":pushpin:"));
        assert!(!is_drainable_queue_head("[route] target tmux session: 0"));
        assert!(!is_drainable_queue_head(
            "[error] dispatch blocked: only the gated #5eq8 remains."
        ));
        assert!(is_drainable_queue_head(
            "Error: queue items are being struck without being worked on."
        ));
    }

    #[test]
    fn recurring_imperative_head_classification() {
        // #qimpstrike: recurring imperative command heads are executable
        // directives, exempt from both strike sites.
        for head in [
            "deploy",
            "- deploy",
            ":pushpin: deploy",
            "commit",
            "push",
            "build",
            "install",
            "release",
            "test",
            "sync",
            "recycle",
            "publish",
            "commit + push",
            "push origin main",
            "#commit-push",
            "#spec-test-commit-push",
        ] {
            assert!(
                is_recurring_imperative_head(head),
                "{head:?} should classify as a recurring-imperative head"
            );
            // And it stays drainable (dispatchable through convergence), never
            // demoted to noise.
            assert!(
                is_drainable_queue_head(head),
                "{head:?} must remain drainable"
            );
        }

        // NOT recurring-imperative: multi-word prose tasks that merely mention a
        // command verb, prose bug reports, questions, unrelated verbs. These stay
        // eligible for the legitimate `#qftbklgstrike` restatement strike.
        for head in [
            "fix the deploy script so it retries on a transient 500",
            "the deploy failed last night",
            "investigate why the build is slow",
            "add a dark mode toggle to the settings panel",
            "why is the commit history squashed?",
            "Queue items are being struck without being worked on.",
            "#lender-card-msg-email",
        ] {
            assert!(
                !is_recurring_imperative_head(head),
                "{head:?} must NOT classify as a recurring-imperative head"
            );
        }
    }

    #[test]
    fn detect_serves_prose_report_before_later_directive() {
        // #freshprosequeue: a natural-language bug report is work even without an
        // imperative verb, so it stays the continuation head instead of being
        // skipped/struck as noise.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "I'm still seeing JB File Cache Conflict dialogs. There should be 0.",
                "Fix the submit bug and add coverage",
            ],
            true,
            true,
        );
        let continuation = detect(&doc).unwrap().expect("a drainable head remains");
        assert_eq!(
            continuation.head_prompt,
            "I'm still seeing JB File Cache Conflict dialogs. There should be 0."
        );
        assert_eq!(
            queue_stale_noise_lines(&std::fs::read_to_string(&doc).unwrap()),
            0
        );
    }

    #[test]
    fn detect_quiesces_when_only_artifact_noise_heads_remain() {
        // #goqstall2: a queue of only console/status artifacts is NOT a stall —
        // continuation is not required and the lines are counted as predicate-
        // proven noise.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["[route] target tmux session: 0", "[error] dispatch blocked"],
            true,
            true,
        );
        assert!(
            detect(&doc).unwrap().is_none(),
            "only-noise queue must not require continuation"
        );
        assert_eq!(
            queue_stale_noise_lines(&std::fs::read_to_string(&doc).unwrap()),
            2
        );
    }

    #[test]
    fn live_drainable_head_skips_artifact_noise_and_lands_on_directive() {
        // #qchurn: the supervisor idle-watch dispatch head must skip inert
        // artifact lines and land on real work — matching detect(), so the watch
        // does not churn no-op /agent-doc cycles when the only ready head is noise.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "[route] target tmux session: 0",
                "Fix the submit bug and add coverage",
            ],
            true,
            true,
        );
        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some("Fix the submit bug and add coverage"),
        );
    }

    #[test]
    fn live_drainable_head_none_when_only_artifact_noise() {
        // #qchurn: a queue of only artifact lines yields NO drainable idle-watch
        // head, so the supervisor stops re-dispatching. The unfiltered
        // live_continuation_head still returns the first head, proving the filter
        // is what quiesces the churn.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["[route] target tmux session: 0", "[error] dispatch blocked"],
            true,
            true,
        );
        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).is_none(),
            "only-noise queue must not yield a drainable idle-watch head"
        );
        assert!(
            live_continuation_head(&content).is_some(),
            "unfiltered head still returns the first noise line (the old churn source)"
        );
    }

    #[test]
    fn reconcile_marker_writes_then_clears() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#seopdp]"], true, true);

        // Active continuation → marker written.
        let continuation = reconcile_marker(&doc, "commit").expect("continuation required");
        assert_eq!(continuation.head_prompt, "do [#seopdp]");
        let marker = load_continuation_marker(&doc)
            .unwrap()
            .expect("marker persisted");
        assert_eq!(marker.head_prompt, "do [#seopdp]");
        assert_eq!(marker.source_command, "commit");

        // Drain the queue (queue_active flips false) → marker cleared.
        let _ = write_doc(dir.path(), &["do [#seopdp]"], false, true);
        assert!(reconcile_marker(&doc, "commit").is_none());
        assert!(load_continuation_marker(&doc).unwrap().is_none());
    }

    #[test]
    fn pending_marker_for_roots_finds_then_prunes_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let doc = write_doc(&root, &["do [#seopdp]"], true, true);
        reconcile_marker(&doc, "commit").expect("marker written");

        // The marker is found and re-confirmed against the live document.
        let found = pending_marker_continuation_for_roots(&[root.clone()], None)
            .unwrap()
            .expect("durable continuation found");
        assert_eq!(found.0, doc);
        assert_eq!(found.1.head_prompt, "do [#seopdp]");

        // Drain the queue but leave the marker file on disk (stale).
        let _ = write_doc(&root, &["do [#seopdp]"], false, true);
        let path = continuation_marker_path(&doc).unwrap().unwrap();
        assert!(path.exists(), "stale marker still on disk before scan");
        // Scan re-confirms against the document, finds it no longer owes
        // continuation, returns None, and prunes the stale marker.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], None)
                .unwrap()
                .is_none()
        );
        assert!(!path.exists(), "stale marker pruned during scan");
    }

    // `#codex-stop-cross-doc-queue-continuation`: a marker for a document owned
    // by another live actor's pane must be skipped (not driven) when the current
    // Codex pane differs; a same-pane / unowned marker is still returned.
    #[test]
    fn pending_marker_skips_foreign_owned_then_finds_same_pane() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // Foreign doc: owned by a live actor on pane %70.
        let foreign = write_doc(&root, &["do [#foreign]"], true, true);
        reconcile_marker(&foreign, "commit").expect("foreign marker written");
        crate::session_actor::project_binding_in(
            &root,
            &foreign.to_string_lossy(),
            "foreign-session",
            "%70",
            "@1",
            "test",
            "foreign_owner",
        )
        .unwrap();

        // From pane %74, the foreign-owned marker must be skipped → None.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], Some("%74"))
                .unwrap()
                .is_none(),
            "foreign-owned marker (pane %70) must be skipped from pane %74"
        );
        // The foreign marker must NOT be pruned — it belongs to its own owner.
        assert!(
            continuation_marker_path(&foreign)
                .unwrap()
                .unwrap()
                .exists(),
            "foreign marker must survive the skip (not stale)"
        );

        // The foreign doc's OWN pane (%70) still drives its marker.
        let owned = pending_marker_continuation_for_roots(&[root.clone()], Some("%70"))
            .unwrap()
            .expect("same-pane owner drives its own marker");
        assert_eq!(owned.0, foreign);

        // Unknown pane context (None) preserves prior behavior — returns it.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], None)
                .unwrap()
                .is_some(),
            "None current_pane disables the gate (prior behavior)"
        );
    }

    // `#codex-stop-cross-doc-queue-continuation` (scan ordering): a foreign-owned
    // marker scanned BEFORE a valid same-pane marker must be skipped while the
    // scan continues to return the later valid marker — never the foreign one.
    #[test]
    fn pending_marker_scan_continues_past_foreign_to_valid() {
        let foreign_dir = tempfile::tempdir().unwrap();
        let valid_dir = tempfile::tempdir().unwrap();
        let foreign_root = foreign_dir.path().to_path_buf();
        let valid_root = valid_dir.path().to_path_buf();

        // Foreign doc (scanned first): owned by a live actor on pane %70.
        let foreign = write_doc(&foreign_root, &["do [#foreign]"], true, true);
        reconcile_marker(&foreign, "commit").expect("foreign marker written");
        crate::session_actor::project_binding_in(
            &foreign_root,
            &foreign.to_string_lossy(),
            "foreign-session",
            "%70",
            "@1",
            "test",
            "foreign_owner",
        )
        .unwrap();

        // Valid doc (scanned second): owned by the current pane %74.
        let valid = write_doc(&valid_root, &["do [#valid]"], true, true);
        reconcile_marker(&valid, "commit").expect("valid marker written");
        crate::session_actor::project_binding_in(
            &valid_root,
            &valid.to_string_lossy(),
            "current-session",
            "%74",
            "@1",
            "test",
            "current_owner",
        )
        .unwrap();

        // From pane %74, the foreign root is scanned first; its %70-owned marker
        // is skipped and the scan continues to the %74-owned valid marker.
        let found = pending_marker_continuation_for_roots(
            &[foreign_root.clone(), valid_root.clone()],
            Some("%74"),
        )
        .unwrap()
        .expect("scan must continue past foreign marker to the valid one");
        assert_eq!(
            found.0, valid,
            "must return the same-pane valid doc, not foreign"
        );
        assert_eq!(found.1.head_prompt, "do [#valid]");

        // The skipped foreign marker must survive for its own owner.
        assert!(
            continuation_marker_path(&foreign)
                .unwrap()
                .unwrap()
                .exists(),
            "foreign marker must survive the skip (belongs to its own owner)"
        );
    }

    #[test]
    fn record_continuation_requested_head_persists_for_nonadvancing_guard() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#seopdp]"], true, true);
        reconcile_marker(&doc, "commit").expect("marker written");
        record_continuation_requested_head(&doc, "do [#seopdp]").unwrap();
        let marker = load_continuation_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.last_requested_head.as_deref(), Some("do [#seopdp]"));
        // A re-detect/reconcile preserves the requested head.
        reconcile_marker(&doc, "commit");
        let marker = load_continuation_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.last_requested_head.as_deref(), Some("do [#seopdp]"));
    }
}
