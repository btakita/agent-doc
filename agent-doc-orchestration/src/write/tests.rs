    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    /// #pcp2: a document disk write records write-provenance, but `.agent-doc/`
    /// sidecar/snapshot writes do not (provenance is only meaningful for the
    /// editor-visible document).
    #[test]
    fn atomic_write_records_provenance_for_document_not_sidecar() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();

        let doc = tmp.path().join("prov-doc.md");
        atomic_write(&doc, "hello document").unwrap();
        let doc_key = doc
            .canonicalize()
            .unwrap_or(doc.clone())
            .to_string_lossy()
            .to_string();
        let prov = crate::debounce::write_provenance(&doc_key)
            .expect("document write should record provenance");
        assert_eq!(prov.len, "hello document".len());
        assert_eq!(prov.hash, crate::debounce::content_hash("hello document"));
        assert_eq!(prov.actor, "agent");
        assert!(!prov.write_id.is_empty());

        // A write under .agent-doc/ (sidecar/snapshot) must NOT record provenance.
        let sidecar = tmp.path().join(".agent-doc").join("snapshots").join("s.md");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        atomic_write(&sidecar, "snapshot bytes").unwrap();
        let sidecar_key = sidecar
            .canonicalize()
            .unwrap_or(sidecar.clone())
            .to_string_lossy()
            .to_string();
        assert!(
            crate::debounce::write_provenance(&sidecar_key).is_none(),
            "an .agent-doc/ sidecar write must not record document provenance"
        );
    }

    /// 08b end state: a routed visible-document write still records write
    /// provenance, because the queue job runs `atomic_write` on the owner thread
    /// where the owner-scope guard takes the raw path (`atomic_write_raw`), and
    /// that raw path is what records provenance.
    #[test]
    fn write_authority_routed_write_records_provenance() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("routed-doc.md");
        atomic_write(&doc, "routed content").unwrap();
        assert_eq!(fs::read_to_string(&doc).unwrap(), "routed content");
        let key = doc
            .canonicalize()
            .unwrap_or(doc.clone())
            .to_string_lossy()
            .to_string();
        assert!(
            crate::debounce::write_provenance(&key).is_some(),
            "the routed write's inner raw path records provenance"
        );
    }

    /// 08b end state: every editor-visible document write is routed through the
    /// session actor's ordered write queue (no flag). The routed write executes
    /// `atomic_write` again on the owner thread; the owner-scope re-entrancy
    /// guard keeps that inner write on the raw path, so this must not deadlock
    /// and the content must land.
    #[test]
    fn write_authority_visible_write_routes_through_queue_without_deadlock() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("routed-doc2.md");
        atomic_write(&doc, "routed content").unwrap();
        assert_eq!(fs::read_to_string(&doc).unwrap(), "routed content");
        let ops = fs::read_to_string(tmp.path().join(".agent-doc").join("logs").join("ops.log"))
            .unwrap_or_default();
        assert!(
            ops.contains("write_authority action=routed"),
            "a visible-document write must report the routed decision to ops.log: {ops:?}"
        );
    }

    /// `.agent-doc/` sidecar writes are never routed — they always take the raw
    /// path directly.
    #[test]
    fn write_authority_never_routes_agent_doc_sidecars() {
        let tmp = TempDir::new().unwrap();
        let sidecar = tmp.path().join(".agent-doc").join("snapshots").join("s.md");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        atomic_write(&sidecar, "sidecar bytes").unwrap();
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "sidecar bytes");
    }

    #[test]
    fn free_text_head_struck_despite_prompt_prefix_flip_on_answered_prompt() {
        // #free-text-head-consume-genuine-not-struck: the consume decision diffs
        // the normalized snapshot baseline against the LIVE editor buffer. The
        // buffer preserves `❯` prefixes on already-answered prompts that the
        // snapshot normalized to the bare form. A pure `do x` → `❯ do x`
        // prefix flip then surfaces as an added `+❯ …` diff line. It must
        // NOT be read as a new foreign prompt — that wrongly blocked the
        // free-text head strike and stalled the auto-loop.
        let head = "Evaluate axocoatl thing";
        let baseline = concat!(
            "<!-- agent:exchange -->\n",
            "do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        // Live buffer: the prior prompt regained its `❯` prefix; this cycle
        // only added the `### Re: axocoatl` answer.
        let prefix_flip = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "### Re: axocoatl\n",
            "plan written.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), prefix_flip, head),
            "a `❯` prefix flip on an already-answered baseline prompt is not new foreign work"
        );

        // A genuinely new `❯` prompt whose text never appeared at baseline still
        // counts as foreign work, keeping the free-text head queued.
        let genuine_foreign = concat!(
            "<!-- agent:exchange -->\n",
            "do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "❯ a brand new unrelated prompt\n",
            "### Re: axocoatl\n",
            "plan.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            cycle_answered_foreign_exchange_prompt(Some(baseline), genuine_foreign, head),
            "a genuinely new unrelated `❯` prompt absent from baseline is foreign work"
        );
    }

    #[test]
    fn queued_file_reposition_patch_carries_generation_token() {
        // #late-ipc-patch-duplicate-stall: the durable file reposition patch must
        // carry the cycle id + a baseline content hash so a LATE applier can
        // fence a superseded patch (drop instead of re-materialize a duplicate
        // response block). Reposition-only body invariant must hold too.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        let doc = root.join("plan.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — opus\nDone.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        let cs = crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let result = queue_file_ipc_reposition_boundary(&doc, Some("abc123"), &[]).unwrap();
        assert!(matches!(result, FileIpcRepositionResult::Queued));

        let hash = snapshot::doc_hash(&doc).unwrap();
        let patch_file = root.join(".agent-doc/patches").join(format!("{hash}.json"));
        let payload: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&patch_file).unwrap()).unwrap();

        assert_eq!(
            payload["cycle_id"].as_str(),
            Some(cs.cycle_id.as_str()),
            "queued reposition patch must tag the originating cycle id"
        );
        assert_eq!(
            payload["baseline_hash"].as_str(),
            Some(crate::debounce::content_hash(content).as_str()),
            "queued reposition patch must tag the baseline content hash it targets"
        );
        // Reposition-only invariant: no response body re-materialized.
        assert_eq!(payload["patches"], serde_json::json!([]));
        assert_eq!(payload["unmatched"], serde_json::json!(""));
        assert_eq!(payload["reposition_boundary"], serde_json::json!(true));
        assert!(existing_patch_is_reposition_only(&payload));
    }

    // #queue-strike-on-halt: queue head consumption requires an explicit
    // closeout flag, not a `### Re:` heading that merely names the head.
    const HALT_QUEUE_DOC: &str = concat!(
        "---\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#foo]\n",
        "- do [#bar]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn explicit_signal_halt_without_flag_does_not_consume() {
        // (a) Halt response, no --done/--pending-gate/--pending-edit → no consume.
        assert!(!queue_head_has_explicit_completion_signal(HALT_QUEUE_DOC, &[], &[], &[]).unwrap());
    }

    #[test]
    fn explicit_signal_done_flag_consumes() {
        // (b) --done naming the head → consume. (c) also covers no-heading + --done.
        assert!(
            queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &["foo".to_string()],
                &[],
                &[],
            )
            .unwrap()
        );
    }

    #[test]
    fn explicit_signal_gate_and_edit_flags_consume() {
        assert!(
            queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &[],
                &["foo".to_string()],
                &[],
            )
            .unwrap(),
            "--pending-gate naming the head is a completion signal"
        );
        assert!(
            queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &[],
                &[],
                &["foo=rewritten text".to_string()],
            )
            .unwrap(),
            "--pending-edit naming the head is a completion signal"
        );
    }

    #[test]
    fn explicit_signal_flag_for_other_id_does_not_consume() {
        assert!(
            !queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &["bar".to_string()],
                &["baz".to_string()],
                &["qux=text".to_string()],
            )
            .unwrap(),
            "flags for non-head ids must not consume the head"
        );
    }

    #[test]
    fn explicit_signal_none_when_queue_inactive() {
        let inactive = HALT_QUEUE_DOC.replace("queue_active: true", "queue_active: false");
        assert!(
            !queue_head_has_explicit_completion_signal(&inactive, &["foo".to_string()], &[], &[],)
                .unwrap()
        );
    }

    #[test]
    fn done_head_consumes_despite_bundled_pending_add() {
        // #pending-add-suppresses-queue-consume: a finalize that completes the
        // queue head with --done must still consume it even when --pending-add
        // added a new backlog item in the same diff. The bundled add makes the
        // diff-based "active prompt" check return false, but the explicit --done
        // short-circuit authorizes consumption regardless.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#foo] head work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n- do [#bar]\n",
            "<!-- /agent:queue -->\n",
        );
        // Current = baseline + a bundled --pending-add backlog item (the diff
        // shape that used to suppress consumption).
        let current = baseline.replace(
            "- [ ] [#foo] head work\n",
            "- [ ] [#newitem] bundled follow-up\n- [ ] [#foo] head work\n",
        );
        std::fs::write(&doc, &current).unwrap();
        assert!(
            should_consume_queue_prompt_for_write(
                &doc,
                Some(baseline),
                &current,
                &["foo".to_string()],
            )
            .unwrap(),
            "--done naming the head must consume despite a bundled --pending-add"
        );
        // Without an explicit completion flag, the bare do[#id] head is NOT
        // consumed by the diff alone (#queue-strike-on-halt).
        assert!(
            !should_consume_queue_prompt_for_write(&doc, Some(baseline), &current, &[]).unwrap(),
            "bare do[#id] head needs an explicit completion flag"
        );
    }

    #[test]
    fn done_id_marks_later_queue_prompt_completed_without_consuming_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#opportunistic]\n",
            "- do [#tail]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let marked =
            mark_completed_queue_prompts_for_done_ids(&doc, &["opportunistic".to_string()], true)
                .unwrap();
        assert_eq!(marked, 1);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("- do [#head]\n"), "{updated}");
        assert!(updated.contains("- ~~do [#opportunistic]~~\n"), "{updated}");
        assert!(updated.contains("- do [#tail]\n"), "{updated}");
        let snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snapshot.contains("- ~~do [#opportunistic]~~\n"),
            "{snapshot}"
        );
    }

    #[test]
    fn done_id_marking_ignores_already_completed_queue_prompt() {
        let entries = crate::queue::parse(concat!(
            "- do [#head]\n",
            "- ~~do [#opportunistic]~~\n",
            "- do [#tail]\n",
        ))
        .unwrap();

        let (updated, marked) =
            mark_entries_completed_by_done_ids(&entries, &["opportunistic".to_string()]);
        assert!(marked.is_empty());
        assert_eq!(updated, entries);
    }

    #[test]
    fn free_text_queue_head_detection() {
        // #free-text-queue-head-consume: a plain question typed into the queue
        // has no #id and is not a do-directive/preset/trigger → free text.
        let doc = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Is tsift properly integrated into multi-crate architecture?\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(doc).unwrap(),
            "a no-#id queue head is free text and consumable by being answered"
        );
        // A bare do[#id] head is NOT free text (needs an explicit completion flag).
        assert!(!queue_head_is_free_text_prompt(HALT_QUEUE_DOC).unwrap());
        let pinned_do = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- :pushpin: do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            !queue_head_is_free_text_prompt(pinned_do).unwrap(),
            "a pinned do[#id] head is still id-backed, not free text"
        );
        // A #preset head carries an #id, so it is not free text either.
        let preset = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(!queue_head_is_free_text_prompt(preset).unwrap());
        // Inactive queue → no head → not free text.
        let inactive = doc.replace("queue_active: true", "queue_active: false");
        assert!(!queue_head_is_free_text_prompt(&inactive).unwrap());

        // #free-text-queue-owner-consume: a free-text head that MENTIONS ids in
        // prose (but is not a pure id directive) is still free text — it has no
        // single id to `--done`, so it must complete on being answered. This is
        // the live repro head from src/boost-client/tasks/monsterrodholders.md.
        let id_mentioning = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Approve [#shoptiers]. What are #next-steps?\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(id_mentioning).unwrap(),
            "a free-text head that merely mentions #ids must stay free text (consumable by being answered)"
        );

        // A leading action verb + bracketed id alone (`re [#id]`) is NOT a pure
        // `#id`/`[#id]`/`do [#id]` directive, so it is treated as free text and
        // completes on answer (it still has a single mentioned id, but the verb
        // makes it prose, not a bare directive).
        let verb_prefixed = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Summarize the findings for #report and ship it\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(verb_prefixed).unwrap(),
            "a prose head mentioning a single #id is still free text"
        );
    }

    // #queue-consume-on-stream-ipc-timeout: the shared decision used by both the
    // strict closeout and the stream IPC-timeout `exit(75)` closeout. Mirrors the
    // exact scenario that treadmilled the auto-loop: a free-text head answered by
    // a finalize response whose write fell back to direct disk on IPC timeout.
    #[test]
    fn queue_consume_reconciles_diverged_snapshot_instead_of_bailing() {
        // #finalize-divergence-orphans-committed-head / IPC-CRDT resilience: when
        // the post-merge document queue diverges from the snapshot queue (a
        // concurrent user/editor edit the CRDT merge already reconciled), consume
        // must RECONCILE (the merged document wins) and strike the head — not
        // hard-bail and orphan the unstruck head. Regression for the divergence
        // error hit repeatedly under live editor races.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: do the thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the thing\n",
            "- user added later\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // Snapshot diverges: same head, but missing the concurrently-added item.
        let snap = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: do the thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the thing\n",
            "<!-- /agent:queue -->\n",
        );
        snapshot::save(&doc, snap).unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("consume must not bail on a reconcilable divergence");
        assert!(outcome.is_some(), "the answered head should be consumed");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- ~~do the thing~~"),
            "head must be struck after reconcile:\n{result}"
        );
        assert!(
            result.contains("- user added later"),
            "the concurrently-added item must be preserved (document wins):\n{result}"
        );
        let snap_result = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap_result.contains("- user added later"),
            "snapshot must adopt the reconciled document queue:\n{snap_result}"
        );
    }

    #[test]
    fn queue_consume_uses_node_keys_to_preserve_duplicate_prompt_identity() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: duplicate prose\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- duplicate prose\n",
            "- duplicate prose\n",
            "- keep\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("node-keyed queue consume should handle duplicates")
            .expect("the answered duplicate head should be consumed");

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- ~~duplicate prose~~\n- duplicate prose\n- keep\n"),
            "only the first duplicate prompt should be struck:\n{result}"
        );
        assert_eq!(outcome.consumed_count, 1);
        assert_eq!(outcome.node_ops.len(), 1);
        assert_eq!(outcome.node_ops[0].component, "queue");
        assert_eq!(outcome.node_ops[0].op, "consume");
        assert!(
            outcome.node_ops[0].node_id.starts_with("queue:")
                && outcome.node_ops[0].node_id.contains(":ft-"),
            "node op should carry the queue node key, got {:?}",
            outcome.node_ops[0]
        );
        assert_eq!(
            outcome.node_ops[0].to_json()["op"].as_str(),
            Some("consume")
        );
    }

    #[test]
    fn consume_decision_strikes_answered_free_text_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: JB Run Agent Doc should start the queue\n\nFixed in route.rs.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- JB `Run Agent Doc` on a `queue: stop` + `agent:queue go` doc should start the queue.\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // baseline == current (no new exchange prompt this cycle), non-empty
        // response → the free-text head is answered and must be consumed.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: JB Run Agent Doc should start the queue\n\nFixed.",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "an answered free-text head must be consumed even on the IPC-timeout closeout"
        );
    }

    #[test]
    fn consume_decision_strikes_synthetic_preset_head_on_heading_match() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: #spec-test-build-install-commit-push\n\nDone.",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "a preset head answered by a matching heading id must be consumed"
        );
    }

    #[test]
    fn consume_decision_keeps_bare_do_id_head_without_explicit_flag() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // A bare do[#id] head is halt-safe: a response that does not record an
        // explicit --done/--gate/--edit outcome must NOT strike it.
        assert!(
            !queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: not doing this, here is why",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "a bare do[#id] head must stay queued without an explicit completion flag"
        );
        // The same head WITH --done foo is consumed.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: do [#foo]\n\nDone.",
                &["foo".to_string()],
                &[],
                &[],
            )
            .unwrap(),
            "--done naming the head id must consume it"
        );
    }

    #[test]
    fn consume_decision_keeps_operator_pinned_tracked_backlog_head_without_explicit_flag() {
        // #zwn5: an operator-pinned bare id head (`:round_pushpin: [#ktw8]`) whose
        // id names a tracked agent:backlog item is an id-backed directive — e.g. an
        // operator-drive live-verify item the agent answers with a log-check but can
        // never close itself. A `### Re: #ktw8` log-check heading must NOT strike it
        // (the old synthetic/preset heading-id path wrongly consumed it, then
        // session-check dropped the struck head and locked the snapshot).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ktw8] operator live-verify: destructive /clear path, operator drives.\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- :round_pushpin: [#ktw8]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        assert!(
            !queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: #ktw8 — destructive /clear live-verify (operator-drive log check)\n\nops.log shows 0 markers; stays open.",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "an operator-pinned head naming a tracked backlog item must stay queued without an explicit completion flag"
        );
        // The same head WITH --pending-gate naming its id is a real completion signal.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: #ktw8\n\nGated pending live verification.",
                &[],
                &["ktw8".to_string()],
                &[],
            )
            .unwrap(),
            "--pending-gate naming the head id must consume it"
        );
    }

    #[test]
    fn free_text_head_kept_only_when_cycle_answered_foreign_prompt() {
        // #queue-head-struck-on-foreign-exchange-answer: the predicate that gates
        // free-text head consumption. A drain cycle (only this turn's `### Re:`
        // response added, no new user prompt) is NOT foreign → head drains. A
        // cycle that added a NEW unrelated `❯` exchange prompt IS foreign → the
        // free-text head stays queued so its work is not silently struck.
        let head = "lazily-rs plan-update";
        let baseline = "\
---
agent_doc_format: template
queue_active: true
---

<!-- agent:exchange -->
### Re: older
Old.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- lazily-rs plan-update
<!-- /agent:queue -->
";
        let drain = baseline.replace(
            "<!-- agent:boundary:x -->",
            "### Re: updated the plan\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), &drain, head),
            "a drain cycle (only a new response, no new prompt) is not foreign work"
        );

        let foreign = baseline.replace(
            "<!-- agent:boundary:x -->",
            "❯ Fix the JB cache conflict instead\n### Re: fix jb\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            cycle_answered_foreign_exchange_prompt(Some(baseline), &foreign, head),
            "a cycle that added a new unrelated exchange prompt answered foreign work"
        );
    }

    #[test]
    fn queue_skip_diagnostic_names_head_shape_and_repair_path() {
        let id_backed = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        let id_message = queue_skip_diagnostic_for_content(id_backed).unwrap();
        assert!(id_message.contains("[queue] kept head `do #foo`"));
        assert!(id_message.contains("`--done foo`"));
        assert!(id_message.contains("`--pending-gate foo`"));
        assert!(id_message.contains("`--pending-edit \"foo=...\"`"));
        assert!(id_message.contains("missing proof"));

        let free_text = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Review the queue diagnostics\n",
            "<!-- /agent:queue -->\n",
        );
        let free_text_message = queue_skip_diagnostic_for_content(free_text).unwrap();
        assert!(
            free_text_message
                .contains("[queue] kept free-text head `Review the queue diagnostics`")
        );
        assert!(free_text_message.contains("answered-response path"));
    }

    #[test]
    fn heading_topic_matches_head_exactly_or_by_exact_id() {
        // Codex Stop-hook path: exact-topic match, or a topic that resolves to
        // EXACTLY the head id (#queue-head-consume-on-topic-id-regression).
        assert!(response_topic_matches_queue_head("do [#foo]", "do [#foo]"));
        assert!(response_topic_matches_queue_head(
            "do [#foo]",
            ":pushpin: do [#foo]"
        ));
        assert!(response_topic_matches_queue_head("#fix1", "do #fix1"));
        assert!(response_topic_matches_queue_head("#foo", "do [#foo]"));
        // Halt/modifier headings must NOT count as completion (#queue-strike-on-halt).
        assert!(!response_topic_matches_queue_head("#foo halt", "do [#foo]"));
        assert!(!response_topic_matches_queue_head(
            "#foo deferred",
            "do [#foo]"
        ));
    }

    #[test]
    fn bare_do_directive_detection() {
        // Queue parser strips the `- ` bullet, so heads arrive as `do [#id]`.
        assert!(queue_head_is_bare_do_directive("do [#foo]"));
        assert!(queue_head_is_bare_do_directive("do #foo"));
        assert!(queue_head_is_bare_do_directive(":pushpin: do [#foo]"));
        assert!(queue_head_is_bare_do_directive(":round_pushpin: do #foo"));
        // A synthetic/preset prompt carrying a trailing `#preset` id is NOT a
        // bare directive.
        assert!(!queue_head_is_bare_do_directive(
            "JB Run Agent Doc on tsift.md add the prompt into agent:queue.\n#spec-test-build-install-commit-push"
        ));
        // A bare preset id on its own line is also not a `do` directive.
        assert!(!queue_head_is_bare_do_directive(
            "#spec-test-build-install-commit-push"
        ));
    }

    #[test]
    fn topic_resolves_to_exact_id_rejects_modifiers() {
        assert!(topic_resolves_to_exact_id(
            "#spec-test-build-install-commit-push",
            "spec-test-build-install-commit-push"
        ));
        assert!(topic_resolves_to_exact_id("do [#foo]", "foo"));
        assert!(topic_resolves_to_exact_id("#Foo", "foo")); // case-insensitive
        // Trailing modifiers (#queue-strike-on-halt) must never resolve to the id.
        assert!(!topic_resolves_to_exact_id("#foo halt", "foo"));
        assert!(!topic_resolves_to_exact_id("#foo deferred", "foo"));
        assert!(!topic_resolves_to_exact_id("#other", "foo"));
    }

    fn patch_with_heading(heading: &str) -> crate::template::PatchBlock {
        crate::template::PatchBlock::new("exchange", format!("{heading}\n\nbody line one\n"))
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_captures_unowned_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        // candidate (disk / ack sidecar) carries the user's freshly-typed "go".
        let candidate = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );
        // content_ours (baseline + response, no user edits) does NOT have "go".
        let content_ours = baseline;

        let dropped = dropped_prompt_lines_after_content_ours(baseline, &candidate, content_ours);
        assert_eq!(dropped, vec!["go".to_string()]);
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_empty_when_content_ours_owns_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let with_go = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );
        // Both candidate and content_ours contain "go" → nothing is dropped.
        let dropped = dropped_prompt_lines_after_content_ours(baseline, &with_go, &with_go);
        assert!(dropped.is_empty());
    }

    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_captures_adjacent_free_text_items() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- item being edited\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- previous user queue item\n",
            "- item being edited\n",
            "- next user queue item\n",
            "<!-- /agent:queue -->\n",
        );

        let dropped = dropped_queue_prompt_lines_after_content_ours(baseline, candidate, baseline);
        assert_eq!(
            dropped,
            vec![
                "previous user queue item".to_string(),
                "next user queue item".to_string()
            ]
        );
    }

    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_empty_when_items_are_owned() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- item being edited\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- previous user queue item\n",
            "- item being edited\n",
            "- next user queue item\n",
            "<!-- /agent:queue -->\n",
        );

        let dropped = dropped_queue_prompt_lines_after_content_ours(baseline, candidate, candidate);
        assert!(dropped.is_empty());
    }

    // #dropqueue-consumed-falsecount: a queue item the user added this cycle that
    // content_ours CONSUMED (struck `~~...~~`) is answered, not dropped. It must
    // not be recorded as a dropped user edit (which would trip the
    // #queue-user-edit-overwrite guard on a correct closeout).
    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_excludes_consumed_struck_item() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- keep me\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- do [#consumed]\n",
            "- keep me\n",
            "<!-- /agent:queue -->\n",
        );
        let content_ours = concat!(
            "<!-- agent:queue auto -->\n",
            "- ~~do [#consumed]~~\n",
            "- keep me\n",
            "<!-- /agent:queue -->\n",
        );
        let dropped =
            dropped_queue_prompt_lines_after_content_ours(baseline, candidate, content_ours);
        assert!(
            dropped.is_empty(),
            "a struck/consumed item is answered, not dropped: {dropped:?}"
        );
    }

    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_counts_duplicate_user_items() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- repeat me\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- repeat me\n",
            "- repeat me\n",
            "- repeat me\n",
            "<!-- /agent:queue -->\n",
        );

        let dropped = dropped_queue_prompt_lines_after_content_ours(baseline, candidate, baseline);
        assert_eq!(
            dropped,
            vec!["repeat me".to_string(), "repeat me".to_string()]
        );
    }

    #[test]
    fn apply_live_queue_deletions_removes_deleted_items_without_absorbing_additions() {
        let content_ours = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: response — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let live_candidate = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ live prompt after preflight\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto priority -->\n",
            "- do [#manual]\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n",
        );

        let reconciled =
            apply_live_queue_deletions_to_content_ours(content_ours, live_candidate, content_ours);

        assert!(reconciled.contains("### Re: response — gpt-5"));
        assert!(!reconciled.contains("❯ live prompt after preflight"));
        assert!(reconciled.contains("<!-- agent:queue auto -->"));
        assert!(
            !reconciled.contains("do [#manual]"),
            "live queue additions stay next-cycle visible, not in content_ours:\n{reconciled}"
        );
        assert!(reconciled.contains("do [#head]"));
        assert!(
            !reconciled.contains("do [#deleted]"),
            "live queue deletion must not be resurrected:\n{reconciled}"
        );
    }

    #[test]
    fn ipc_live_prompt_drift_content_ours_preserves_live_queue_deletions() {
        let dir = tempfile::tempdir().unwrap();
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let content_ours = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "### Re: original prompt — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "❯ live prompt after preflight\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#manual]\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = init_repo_with_doc(dir.path(), "session.md", baseline);
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut decision = IpcRepairDecision::ack_content(candidate.to_string());

        let blocked = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &doc,
            "test",
            Some("patch-q"),
            Some(baseline),
            Some(content_ours),
            &mut decision,
        );

        assert!(blocked);
        assert_eq!(decision.snap_source, IpcSnapshotSource::ContentOurs);
        assert!(
            decision
                .snapshot_content
                .contains("### Re: original prompt — gpt-5")
        );
        assert!(
            !decision
                .snapshot_content
                .contains("❯ live prompt after preflight")
        );
        assert!(
            !decision.snapshot_content.contains("do [#manual]"),
            "live queue addition must not be absorbed into the response commit:\n{}",
            decision.snapshot_content
        );
        assert!(
            !decision.snapshot_content.contains("do [#deleted]"),
            "live queue deletion must not be resurrected:\n{}",
            decision.snapshot_content
        );
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains("queue_content_ours_reconciled")
                && log.contains("reason=live_queue_deletion_authoritative")
                && log.contains("dropped_queue_prompt_recorded"),
            "queue reconciliation must leave ops.log proof:\n{log}"
        );
    }

    #[test]
    fn extract_response_headings_returns_re_lines_in_order() {
        let patches = vec![
            patch_with_heading("### Re: first topic — opus-4-7"),
            patch_with_heading("### Re: second topic — opus-4-7"),
            // Patch with no Re: heading should be skipped.
            crate::template::PatchBlock::new("status", "Just a status update.\n"),
        ];
        let headings = extract_response_headings_from_patches(&patches);
        assert_eq!(
            headings,
            vec![
                "### Re: first topic — opus-4-7".to_string(),
                "### Re: second topic — opus-4-7".to_string(),
            ]
        );
    }

    #[test]
    fn extract_response_headings_picks_first_re_per_patch() {
        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: outer — opus-4-7\n\nbody mentioning ### Re: inner — opus-4-7 elsewhere\n",
        );
        let headings = extract_response_headings_from_patches(&[patch]);
        assert_eq!(headings, vec!["### Re: outer — opus-4-7".to_string()]);
    }

    #[test]
    fn materialization_probe_uses_patch_body_not_patch_markers() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: materialized — gpt-5\n\n",
            "Committed through boundary insertion.\n",
            "<!-- /patch:exchange -->\n",
        );

        let probe = response_materialization_probe_from_response(response);

        assert!(probe.contains("### Re: materialized — gpt-5"));
        assert!(!probe.contains("<!-- patch:exchange -->"));
        assert!(!probe.contains("<!-- /patch:exchange -->"));
    }

    #[test]
    fn patch_wrapped_response_is_materialized_by_visible_patch_body() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- /patch:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_materialized_in_content(response, content));
    }

    #[test]
    fn marker_bearing_zero_patch_parse_is_rejected_before_capture() {
        let err = reject_marker_response_with_zero_patches(1, 0).unwrap_err();

        assert!(
            err.to_string()
                .contains("parsed zero patches despite 1 patch marker")
        );
        assert!(reject_marker_response_with_zero_patches(0, 0).is_ok());
        assert!(reject_marker_response_with_zero_patches(2, 1).is_ok());
    }

    #[test]
    fn patch_response_headings_already_in_head_true_when_no_patches() {
        // Empty patch list — conservatively preserve the existing late-fallback
        // gate behavior (reject when no response evidence is present).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "doc body\n").unwrap();
        assert!(patch_response_headings_already_in_head(&doc, &[]));
    }

    fn init_repo_with_doc(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["init", "-q", "--initial-branch=main"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["config", "user.name", "Test"])
            .status()
            .unwrap();
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["add", name])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["commit", "-q", "-m", "seed"])
            .status()
            .unwrap();
        path
    }

    #[test]
    fn patch_response_headings_already_in_head_true_when_heading_in_head() {
        let dir = TempDir::new().unwrap();
        let doc = init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: shipped — opus-4-7\n\nbody\n",
        );
        let patch = patch_with_heading("### Re: shipped — opus-4-7");
        assert!(patch_response_headings_already_in_head(&doc, &[patch]));
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_heading_missing_from_head() {
        // Mid-turn rotation signature: HEAD has been advanced by a different
        // operation (compact, sibling commit) and does not yet contain the
        // response we're about to apply. The late-fallback gate must allow
        // the patch through.
        let dir = TempDir::new().unwrap();
        let doc = init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: prior cycle — opus-4-7\n\nold\n",
        );
        let patch = patch_with_heading("### Re: new response — opus-4-7");
        assert!(
            !patch_response_headings_already_in_head(&doc, &[patch]),
            "mid-turn rotation must allow the patch (response not in HEAD)"
        );
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_any_heading_missing() {
        let dir = TempDir::new().unwrap();
        let doc = init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: first — opus-4-7\n\nbody\n",
        );
        let patches = vec![
            patch_with_heading("### Re: first — opus-4-7"),
            patch_with_heading("### Re: second — opus-4-7"),
        ];
        assert!(
            !patch_response_headings_already_in_head(&doc, &patches),
            "all headings must be in HEAD for the gate to skip"
        );
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_file_not_in_git() {
        // No git repo → show_head returns Ok(None). Fail-safe: treat as not
        // in HEAD so the late-fallback gate rotates the cycle rather than
        // rejecting the patch.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "no git\n").unwrap();
        let patch = patch_with_heading("### Re: something — opus-4-7");
        assert!(!patch_response_headings_already_in_head(&doc, &[patch]));
    }

    #[test]
    fn write_appends_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello\n").unwrap();

        // Simulate stdin by calling run logic directly
        let base = fs::read_to_string(&doc).unwrap();
        let response = "This is the assistant response.";

        let mut content_ours = base.clone();
        if !content_ours.ends_with('\n') {
            content_ours.push('\n');
        }
        content_ours.push_str("## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push('\n');
        content_ours.push_str("\n## User\n\n");

        atomic_write(&doc, &content_ours).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("## Assistant\n\nThis is the assistant response."));
        assert!(result.contains("\n\n## User\n\n"));
        assert!(result.contains("## User\n\nHello"));
    }

    #[test]
    fn write_updates_snapshot() {
        // Use a direct snapshot write/read to avoid CWD dependency.
        // The snapshot module uses relative paths (.agent-doc/snapshots/),
        // so we verify the pattern works via snapshot::path_for + direct I/O.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();

        // Verify snapshot path computation works
        let snap_path = snapshot::path_for(&doc).unwrap();
        assert!(
            snap_path
                .to_string_lossy()
                .contains(".agent-doc/snapshots/")
        );

        // Verify atomic_write + read roundtrip (the core of snapshot save)
        let snap_abs = dir.path().join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
        let loaded = fs::read_to_string(&snap_abs).unwrap();
        assert_eq!(loaded, content);
    }

    #[test]
    fn visible_write_guard_blocks_when_editor_typing_active() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/typing")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "body\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::document_changed(&doc_str);

        let err = guard_visible_write_idle_with_budget(&doc, "test_visible_write", 60_000, 0)
            .unwrap_err();

        assert!(err.to_string().contains("editor typing did not settle"));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_typing_defer_active_typing:test_visible_write"));
        assert!(log.contains("visible_write_deferred_active_typing"));
    }

    #[test]
    fn visible_write_guard_blocks_when_current_changed_after_merge() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

<!--
scratch
-->
";
        fs::write(&doc, expected).unwrap();
        fs::write(
            &doc,
            expected.replace("scratch", "scratch\nstill typing this line"),
        )
        .unwrap();

        let err = guard_visible_write_idle_and_current(&doc, "test_current_changed", expected)
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("document changed after the response merge was computed")
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_current_changed:test_current_changed"));
        assert!(log.contains("visible_write_deferred_current_changed"));
    }

    #[test]
    fn visible_write_guard_blocks_when_idle_editor_buffer_differs_from_disk() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        let live_buffer = expected.replace(
            "<!-- /agent:exchange -->",
            "prompt typed but not saved\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest(
            &doc_str,
            live_buffer.len(),
            &crate::debounce::content_hash(&live_buffer),
        )
        .unwrap();

        let err = guard_visible_write_idle_and_current(&doc, "test_live_buffer_changed", expected)
            .unwrap_err();

        assert!(
            err.to_string().contains("visible editor buffer"),
            "expected live-buffer guard error: {err}"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), expected);
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_current_changed:test_live_buffer_changed"));
        assert!(log.contains("visible_write_deferred_live_buffer_changed"));
    }

    #[test]
    fn visible_write_reconcile_treats_editor_matching_disk_as_reconcilable_drift() {
        // #nm1x: the editor reported a buffer that diverges from `expected` but
        // *matches the current on-disk content* (an independent document edit the
        // editor already saved). That is not a pending unsaved user edit, so the
        // guard must not fail closed — it reports the reconcilable DiskDrifted case
        // instead, letting the response re-merge against the fresh disk content.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        // Disk + editor both carry an independent queue edit not present in
        // `expected`; the editor digest equals disk (saved, no pending edit).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do [#sibling]\n<!-- /agent:queue -->",
        );
        fs::write(&doc, &drifted).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest(
            &doc_str,
            drifted.len(),
            &crate::debounce::content_hash(&drifted),
        )
        .unwrap();

        let outcome =
            guard_visible_write_reconcile(&doc, "test_editor_matches_disk", expected).unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_live_buffer_matches_disk"),
            "expected provenance-suppression log: {log}"
        );
        assert!(
            log.contains("source=test_editor_matches_disk"),
            "marker must identify the write source: {log}"
        );
        assert!(
            log.contains(&format!("expected_len={}", expected.len())),
            "marker must carry expected length: {log}"
        );
        assert!(
            log.contains(&format!(
                "expected_hash={}",
                crate::ops_log::content_hash(expected)
            )),
            "marker must carry expected hash: {log}"
        );
        assert!(
            log.contains(&format!("disk_len={}", drifted.len())),
            "marker must carry disk length: {log}"
        );
        assert!(
            log.contains(&format!(
                "disk_hash={}",
                crate::ops_log::content_hash(&drifted)
            )),
            "marker must carry disk hash: {log}"
        );
        assert!(
            log.contains(&format!("live_len={}", drifted.len())),
            "marker must carry live-buffer length: {log}"
        );
        assert!(
            log.contains(&format!(
                "live_hash={}",
                crate::ops_log::content_hash(&drifted)
            )),
            "marker must carry live-buffer hash: {log}"
        );
        assert!(
            log.contains("live_ts="),
            "marker must carry live timestamp: {log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "must not record a fail-closed live-buffer block: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_reports_clean_when_disk_matches() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        fs::write(&doc, expected).unwrap();

        let outcome = guard_visible_write_reconcile(&doc, "test_clean", expected).unwrap();
        assert!(matches!(outcome, VisibleWriteReconcile::Clean));
    }

    #[test]
    fn visible_write_reconcile_reports_disk_drift_without_live_buffer_edit() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        // Disk grew under us with a foreign agent-doc append (no live editor buffer
        // sidecar = no pending user edit), so the guard must report it as a
        // reconcilable drift rather than failing closed (#ipc-drift-visbuf-reconcile).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: foreign\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &drifted).unwrap();

        let outcome = guard_visible_write_reconcile(&doc, "test_drift", expected).unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("visible_write_disk_drift_reconcilable"));
    }

    #[test]
    fn reconcile_visible_write_remerges_foreign_append_then_lands_clean() {
        // The first guard call sees a foreign disk append; the loop must re-merge
        // the captured response against the fresh disk content and then succeed
        // without failing closed and stranding the response (#ipc-drift-visbuf-reconcile).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed\n").unwrap();

        let base = "BASE";
        let foreign = "BASE+FOREIGN";
        let guard_calls = std::cell::RefCell::new(0usize);
        let recompute_calls = std::cell::RefCell::new(0usize);

        let guard = |_f: &Path, expected: &str| -> Result<VisibleWriteReconcile> {
            let mut n = guard_calls.borrow_mut();
            *n += 1;
            if *n == 1 {
                assert_eq!(expected, base);
                Ok(VisibleWriteReconcile::DiskDrifted {
                    fresh_current: foreign.to_string(),
                })
            } else {
                assert_eq!(expected, foreign);
                Ok(VisibleWriteReconcile::Clean)
            }
        };
        let recompute = |current: &str| -> Result<String> {
            *recompute_calls.borrow_mut() += 1;
            // The re-merge incorporates the foreign disk content + the response.
            Ok(format!("{current}+RESPONSE"))
        };
        let fail_closed = |_f: &Path, _c: &str| -> Result<()> {
            panic!("must not fail closed on a reconcilable foreign append");
        };

        let (current, payload) = reconcile_visible_write(
            &doc,
            base.to_string(),
            format!("{base}+RESPONSE"),
            3,
            guard,
            recompute,
            fail_closed,
        )
        .unwrap();

        assert_eq!(current, foreign);
        assert_eq!(payload, "BASE+FOREIGN+RESPONSE");
        assert_eq!(*guard_calls.borrow(), 2);
        assert_eq!(*recompute_calls.borrow(), 1);
    }

    #[test]
    fn reconcile_visible_write_falls_back_to_fail_closed_when_drift_never_settles() {
        // A document that keeps drifting past the attempt bound must fall back to
        // the fail-closed guard so the operator retries instead of looping forever.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed\n").unwrap();

        let counter = std::cell::RefCell::new(0usize);
        let guard = |_f: &Path, _e: &str| -> Result<VisibleWriteReconcile> {
            let mut n = counter.borrow_mut();
            *n += 1;
            Ok(VisibleWriteReconcile::DiskDrifted {
                fresh_current: format!("drift-{n}"),
            })
        };
        let recompute = |current: &str| -> Result<String> { Ok(current.to_string()) };
        let fail_closed = |_f: &Path, _c: &str| -> Result<()> {
            anyhow::bail!("document still changing");
        };

        let err = reconcile_visible_write(
            &doc,
            "start".to_string(),
            "start".to_string(),
            3,
            guard,
            recompute,
            fail_closed,
        )
        .unwrap_err();
        assert!(err.to_string().contains("document still changing"));
        assert_eq!(*counter.borrow(), 3);
    }

    #[test]
    fn capture_locked_pre_response_reads_live_content_after_lock_wait() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "original\n").unwrap();

        let lock_path = snapshot::lock_path_for(&doc).unwrap();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let held_lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        held_lock.lock_exclusive().unwrap();

        let doc_for_thread = doc.clone();
        let capture = std::thread::spawn(move || capture_locked_pre_response(&doc_for_thread));

        std::thread::sleep(Duration::from_millis(100));
        fs::write(&doc, "updated while waiting\n").unwrap();
        drop(held_lock);

        let (captured_lock, captured_content) = capture.join().unwrap().unwrap();
        drop(captured_lock);

        assert_eq!(captured_content, "updated while waiting\n");
        assert_eq!(
            snapshot::load_pre_response(&doc).unwrap().unwrap(),
            "updated while waiting\n"
        );
    }

    #[test]
    fn missing_explicit_baseline_reads_migrated_baseline_after_document_move() {
        let dir = TempDir::new().unwrap();
        for subdir in [
            "snapshots",
            "baselines",
            "locks",
            "pending",
            "crdt",
            "pre-response",
        ] {
            fs::create_dir_all(dir.path().join(".agent-doc").join(subdir)).unwrap();
        }

        let session_uuid = "moved-baseline-session";
        let old_doc = dir.path().join("old.md");
        let doc_content = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\nBody\n",
            session_uuid
        );
        fs::write(&old_doc, &doc_content).unwrap();

        let old_hash = snapshot::doc_hash(&old_doc).unwrap();
        let old_snapshot = dir
            .path()
            .join(".agent-doc/snapshots")
            .join(format!("{}.md", old_hash));
        fs::write(&old_snapshot, &doc_content).unwrap();
        let old_baseline = dir
            .path()
            .join(".agent-doc/baselines")
            .join(format!("{}.md", old_hash));
        fs::write(&old_baseline, "preflight baseline\n").unwrap();

        let new_doc = dir.path().join("new.md");
        fs::rename(&old_doc, &new_doc).unwrap();

        assert!(snapshot::try_migrate_renamed(&new_doc).unwrap());
        assert!(!old_baseline.exists());
        let migrated_baseline = snapshot::baseline_path_for(&new_doc).unwrap();
        assert!(migrated_baseline.exists());

        let baseline = read_explicit_baseline(&new_doc, Some(&old_baseline))
            .unwrap()
            .expect("baseline should be recovered from migrated hash");
        assert_eq!(baseline, "preflight baseline\n");
    }

    #[test]
    fn apply_template_from_string_compact_exchange_replaces_exchange_body() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n",
        );
        fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let response = "<!-- patch:exchange -->\nCompacted summary.\n<!-- /patch:exchange -->\n";
        apply_template_from_string(&doc, response).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Compacted summary.\n"));
        assert!(!result.contains("Old progress."));
        assert!(!result.contains("compact exchange"));
    }

    #[test]
    fn apply_template_from_string_same_base_retry_adopts_existing_response() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: closeout follow-up — gpt-5\n\n",
            "The `spec-test-build-install-commit-push` request is complete in the response above.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: closeout follow-up — gpt-5\n\n",
            "The `spec-test-build-install-commit-push` request is complete in the response above.\n",
            "do #duppb. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: closeout follow-up — gpt-5\n\n",
            "The `spec-test-build-install-commit-push` request is complete in the response above.\n",
            "<!-- /patch:exchange -->\n",
        );
        apply_template_from_string(&doc, response).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            result.matches("### Re: closeout follow-up — gpt-5").count(),
            1,
            "same-base retry must not append a duplicate response block"
        );
        assert!(result.contains("❯ do #duppb. spec-test-build-install-commit-push"));
        assert!(!result.contains("\ndo #duppb. spec-test-build-install-commit-push\n"));
    }

    #[test]
    fn apply_template_from_string_strips_safe_progress_before_exchange_patch() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #rspdigest. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let response = concat!(
            "I am checking the write path and existing replay guard before editing.\n",
            "The fix is small; next I will run the targeted regression.\n\n",
            "<!-- patch:exchange -->\n",
            "### Re: rspdigest — gpt-5\n\n",
            "Implemented and verified.\n",
            "<!-- /patch:exchange -->\n",
        );

        apply_template_from_string(&doc, response).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("### Re: rspdigest — gpt-5"));
        assert!(result.contains("Implemented and verified."));
        assert!(!result.contains("I am checking the write path"));
        assert!(!result.contains("The fix is small"));
    }

    #[test]
    fn apply_template_from_string_rejects_trailing_unmatched_patchback_text() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #rspdigest. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: rspdigest — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /patch:exchange -->\n",
            "extra transcript text\n",
        );

        let err = apply_template_from_string(&doc, response).unwrap_err();

        assert!(
            err.to_string().contains("unsafe unmatched content"),
            "trailing unmatched patchback text must fail closed, got: {err:#}"
        );
    }

    #[test]
    fn apply_template_from_string_rejects_raw_component_form_without_mutating() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #churn. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        // Operator pipes the raw template form (component markers) instead of
        // `<!-- patch:exchange -->` blocks — this is the shape that previously
        // committed escaped directives into the live exchange.
        let raw_template_form = concat!(
            "<!-- agent:status -->\nWork complete.\n<!-- /agent:status -->\n\n",
            "<!-- agent:exchange -->\n### Re: churn — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n",
        );
        let err = apply_template_from_string(&doc, raw_template_form).unwrap_err();
        assert!(
            err.to_string().contains("escaped template patchback"),
            "raw component-form stdin must fail closed, got: {err:#}"
        );

        // The document must be untouched — no escaped markers committed.
        let after = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            after, content,
            "rejected patchback must not mutate the document"
        );
        assert!(!after.contains("### Re: churn"));
    }

    #[test]
    fn guard_rejects_normal_write_when_diff_requests_compact_exchange() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n\n",
            "compact exchange\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let err = guard_no_exchange_compaction_request_between(&doc, Some(baseline), current)
            .expect_err("ordinary response write should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("compact exchange"));
        assert!(msg.contains("agent-doc compact"));
    }

    #[test]
    fn write_preserves_user_edits_via_merge() {
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        let response = "My response";

        // "ours" = base + response
        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // "theirs" = user added a follow-up to the User block
        let theirs = "---\nsession: test\n---\n\n## User\n\nOriginal question\nAnd a follow-up!\n";

        let merged = merge::merge_contents(base, &ours, theirs).unwrap();

        // Both the response and the user's follow-up should be in the merge
        assert!(
            merged.contains("My response"),
            "response missing from merge"
        );
        assert!(
            merged.contains("And a follow-up!"),
            "user edit missing from merge"
        );
    }

    #[test]
    fn write_no_merge_when_unchanged() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "Response here";

        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // theirs == base (no edit)
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, base).unwrap();

        let doc_lock = acquire_doc_lock(&doc).unwrap();
        let content_current = fs::read_to_string(&doc).unwrap();

        let final_content = if content_current == base {
            ours.clone()
        } else {
            merge::merge_contents(base, &ours, &content_current).unwrap()
        };

        drop(doc_lock);
        assert_eq!(final_content, ours);
    }

    #[test]
    fn atomic_write_correct_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.md");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn concurrent_writes_no_corruption() {
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.md");
        fs::write(&path, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let p = path.clone();
            let parent = dir.path().to_path_buf();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut tmp = tempfile::NamedTempFile::new_in(&parent).unwrap();
                std::io::Write::write_all(&mut tmp, content.as_bytes()).unwrap();
                tmp.persist(&p).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_content = fs::read_to_string(&path).unwrap();
        assert!(
            final_content.starts_with("writer-") && final_content.ends_with("-content"),
            "unexpected content: {}",
            final_content
        );
    }

    #[test]
    fn snapshot_matches_disk_state() {
        // Snapshot saved after write must equal the actual post-merge file on disk.
        // Using content_ours (pre-merge) as the snapshot risks phantom diffs when
        // the baseline is stale (e.g. streaming checkpoint with an outdated baseline).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc").join("snapshots");
        fs::create_dir_all(&agent_doc_dir).unwrap();

        let doc = dir.path().join("test.md");
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        fs::write(&doc, base).unwrap();

        // Build content_ours = baseline + response
        let response = "Agent response here";
        let mut content_ours = base.to_string();
        content_ours.push_str("\n## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push_str("\n\n## User\n\n");

        // Simulate user editing the file concurrently (adding a follow-up)
        let user_edited = format!("{}Follow-up question\n", base);
        fs::write(&doc, &user_edited).unwrap();

        // Merge: content_ours + user edits
        let merged = merge::merge_contents(base, &content_ours, &user_edited).unwrap();

        // Write merged content (includes both response and user edit)
        atomic_write(&doc, &merged).unwrap();
        assert!(merged.contains(response), "response missing from merged");
        assert!(
            merged.contains("Follow-up question"),
            "user edit missing from merged"
        );

        // KEY: Save snapshot as final_content (the actual disk state after merge)
        snapshot::save(&doc, &merged).unwrap();

        // Verify: snapshot matches what's on disk exactly
        let snap = snapshot::load(&doc).unwrap().unwrap();
        let current = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            snap, current,
            "snapshot must match actual disk state after write"
        );
        assert!(
            snap.contains(response),
            "snapshot should contain agent response"
        );
        assert!(
            snap.contains("Follow-up question"),
            "snapshot should contain merged user edit"
        );
    }

    #[test]
    fn explicit_baseline_preserves_concurrent_user_edits_for_next_cycle() {
        let baseline = Some("baseline");
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::ContentOurs
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            content_ours
        );
    }

    #[test]
    fn explicit_baseline_forward_merges_concurrent_comment_tail_into_this_cycle() {
        // `#fintol2`: a PLAIN concurrent edit to a parked comment-note outside
        // `exchange` (`old note` -> `edited note`) carries NO next-cycle directive,
        // so `non_exchange_drift_carries_directive` is false and
        // `snapshot_persist_mode_with_current` falls through to `FinalContent`. The
        // edit is forward-merged into THIS cycle's commit (the response lands AND
        // the user's edit is preserved together) instead of being carried forward
        // uncommitted. A directive-bearing outside edit (a new prompt, `do #id`,
        // `#tag`, question) still routes to `ContentOurs` via the sibling
        // `explicit_baseline_preserves_concurrent_user_edits_for_next_cycle`.
        let baseline = Some("baseline");
        let base = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let content_current = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";
        let content_ours = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let final_content = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";

        assert_eq!(
            snapshot_persist_mode_with_current(
                baseline,
                base,
                content_current,
                content_ours,
                final_content
            ),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode_with_current(
                    baseline,
                    base,
                    content_current,
                    content_ours,
                    final_content
                ),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn implicit_baseline_still_persists_final_merged_disk_state() {
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(None, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(None, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn explicit_baseline_keeps_final_content_when_delta_is_prior_streamed_agent_prefix() {
        let baseline = Some("baseline");
        let content_ours = "<!-- agent:exchange -->\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: orchestrate streaming — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn try_ipc_file_fallback_skips_when_patches_already_applied_to_live_buffer() {
        // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
        // `[#ipcfilehashskip]` defense-in-depth dedupe gate.
        //
        // When the live file already contains the response body (e.g. via a
        // prior socket-IPC retry whose sidecar ack arrived late), the file-IPC
        // fallback must hash-compare patch outcome vs current and skip the
        // write so it does not stack a duplicate `### Re:` heading.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let already_applied_content = concat!(
            "---\nsession: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, already_applied_content).unwrap();

        // Build a patch whose application against the current file is a no-op
        // (replace exchange with the same content it already has).
        let exchange_body = "### Re: topic — gpt-5\n\nImplemented.\n";
        let patch = crate::template::PatchBlock::new("exchange", exchange_body);

        let started = std::time::Instant::now();
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        let elapsed = started.elapsed();

        assert!(
            result.success,
            "already-applied file-IPC fallback must short-circuit as success"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "skip path must not block on the 2s IPC timeout: elapsed={:?}",
            elapsed
        );

        let patches_dir = agent_doc_dir.join("patches");
        let leftover: Vec<_> = fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            leftover.is_empty(),
            "skip path must clean up any fallback patch files left around"
        );

        let live_after = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            live_after, already_applied_content,
            "skip path must not mutate the live file"
        );

        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("file_ipc_fallback_skip_already_applied"),
            "skip event must be logged for audit:\n{ops_log}"
        );
    }

    #[test]
    fn try_ipc_returns_false_when_no_patches_dir() {
        // Without .agent-doc/patches/, IPC should return false immediately
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let result = try_ipc(&doc, &patches, "", None, None, None, None, None).unwrap();
        assert!(
            !result.success,
            "should return false when patches dir doesn't exist"
        );
    }

    #[test]
    fn ipc_ack_timeouts_degrade_current_session_to_direct_disk() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test-session\n---\n\ncontent").unwrap();

        assert!(
            !record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap(),
            "first timeout should only record health state"
        );
        assert!(
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap(),
            "second consecutive timeout should mark the listener degraded"
        );
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "current session should now bypass IPC"
        );

        fs::write(&doc, "---\nsession: next-session\n---\n\ncontent").unwrap();
        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a new session id must not inherit the old session's degraded marker"
        );
    }

    #[test]
    fn is_socket_ack_timeout_error_is_duration_agnostic() {
        // `#ipc-ack-timeout-align`: the sender's ack budget is configurable, so
        // the degrade-vote classifier must match the stable prefix, not a
        // hard-coded "(2s)".
        assert!(is_socket_ack_timeout_error(&anyhow::anyhow!(
            "IPC ack timeout (2s)"
        )));
        assert!(is_socket_ack_timeout_error(&anyhow::anyhow!(
            "IPC ack timeout (6s)"
        )));
        assert!(!is_socket_ack_timeout_error(&anyhow::anyhow!(
            "IPC ack status error: something else"
        )));
    }

    #[test]
    fn degraded_latch_self_heals_when_listener_recovers() {
        // `#ipc-degrade-self-heal`: the degrade latch is a circuit breaker, not
        // a permanent session verdict. Once a recovered plugin socket is
        // accepting connections again, `ipc_direct_disk_degraded` must clear the
        // marker and resume the reliable IPC path instead of staying disk-only
        // until session restart.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: heal-session\n---\n\ncontent").unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should stay degraded"
        );

        // Bring a live socket listener up (the recovered plugin).
        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&root_clone, |_msg| {
                Some(r#"{"type":"ack","id":"x"}"#.to_string())
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a recovered live listener must self-heal the degrade latch"
        );
        let marker = dir
            .path()
            .join(".agent-doc/ipc-degraded")
            .join(format!("{}.json", snapshot::doc_hash(&doc).unwrap()));
        assert!(
            !marker.exists(),
            "self-heal must remove the degraded marker"
        );

        let _ = std::fs::remove_file(crate::ipc_socket::socket_path(dir.path()));
        drop(server);
    }

    #[test]
    fn try_ipc_prefers_file_ipc_when_socket_degraded() {
        // `#ipc-degraded-prefers-file-ipc`: a latched-degraded socket must NOT
        // jump straight to a raw disk write. The write routes through the
        // file-IPC patch queue (plugin file watcher applies via Document API).
        // With no plugin consuming the patch, file IPC times out and returns
        // `false` so the caller can fall back to disk as the LAST resort — but
        // the degraded write still attempted file IPC first.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "new content");
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();

        assert!(
            !result.success,
            "degraded file-IPC with no plugin should report not consumed (disk is last resort)"
        );
        // The file-IPC poll cleans up the unconsumed patch on timeout.
        let leftover: Vec<_> = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            leftover.is_empty(),
            "file-IPC timeout must clean up the unconsumed patch"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_socket_degraded_prefer_file_ipc")
                && ops_log.contains("transport=try_ipc"),
            "degraded socket should log the prefer-file-IPC routing decision:\n{ops_log}"
        );
        assert!(
            ops_log.contains("ipc_write_attempt"),
            "degraded write must still attempt the file-IPC patch queue, not bypass it:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("ipc_listener_degraded_direct_disk"),
            "degraded write must NOT take the old direct-disk bypass:\n{ops_log}"
        );
    }

    #[test]
    fn try_ipc_degraded_succeeds_via_file_ipc_when_plugin_consumes() {
        // `#ipc-degraded-prefers-file-ipc`: even with the socket latched
        // degraded, a live plugin file watcher consuming the file-IPC patch
        // makes the degraded write succeed through the plugin (Document API) —
        // no raw disk write, so no manufactured File Cache Conflict.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should latch degraded"
        );

        // Simulate the plugin file watcher applying then deleting the patch.
        let watcher_dir = agent_doc_dir.join("patches");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            let _ = fs::write(
                                &doc_for_watcher,
                                "---\nsession: test\n---\n\n<!-- agent:exchange -->\nnew content\n<!-- /agent:exchange -->\n",
                            );
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let patch = crate::template::PatchBlock::new("exchange", "new content");
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "degraded write must succeed through the file-IPC patch queue when the plugin consumes it"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_socket_degraded_prefer_file_ipc"),
            "degraded socket should log the prefer-file-IPC routing decision:\n{ops_log}"
        );
    }

    #[test]
    fn try_ipc_times_out_when_no_plugin() {
        // With .agent-doc/patches/ existing but no plugin consuming, should timeout
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "new content");

        // This will timeout after 2s — patch file is written but never consumed
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            !result.success,
            "should return false on timeout (no plugin)"
        );

        // Patch file should be cleaned up after timeout
        let patches_dir = agent_doc_dir.join("patches");
        let entries: Vec<_> = fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            entries.is_empty(),
            "patch file should be cleaned up after timeout"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=no_ack")
                && log.contains("recovery=direct_write_fallback"),
            "IPC timeout should log the failed invariant and recovery path:\n{log}"
        );
    }

    #[test]
    fn try_ipc_succeeds_when_plugin_consumes() {
        // Simulate plugin by spawning a thread that deletes the patch file
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "new content");

        // Spawn "plugin" thread that watches for patch files, writes content, then deletes
        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            // Simulate plugin applying the patch by modifying the doc
                            let _ = fs::write(
                                &doc_for_watcher,
                                "---\nsession: test\n---\n\n<!-- agent:exchange -->\nnew content\n<!-- /agent:exchange -->\n",
                            );
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "should return true when plugin consumes patch"
        );
    }

    #[test]
    fn try_ipc_rejects_consumed_partial_response_materialization() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let original = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: missed patchback - gpt-5\n\nRecovered answer.",
        );
        let partial = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "content\n",
            "### Re: missed patchback - gpt-5\n",
            "<!-- /agent:exchange -->\n"
        );

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let partial_for_watcher = partial.to_string();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let _ = fs::write(&doc_for_watcher, &partial_for_watcher);
                            let _ =
                                fs::write(ack_dir.join(format!("{pid}.md")), &partial_for_watcher);
                        }
                        let _ = fs::remove_file(path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(&doc, &[patch], "", None, Some(original), None, None, None).unwrap();
        assert!(
            !result.success,
            "IPC consume without the full response body must fall back instead of saving a successful snapshot"
        );

        let snap = snapshot::load(&doc).unwrap();
        assert!(
            snap.as_deref()
                .is_none_or(|content| !content.contains("Recovered answer.")),
            "partial IPC materialization must not become the committed snapshot: {snap:?}"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_materialization_missing_response") && log.contains("source=file_ipc"),
            "missing response materialization should be logged for operator repair:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=missing_response_probe")
                && log.contains("recovery=direct_write_fallback"),
            "missing response materialization should name its invariant and recovery:\n{log}"
        );
    }

    #[test]
    fn file_ipc_consumed_with_live_exchange_edit_requires_ack_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[],
            "",
            None,
            Some(baseline),
            None,
            None,
            Some("patch-live-edit"),
        )
        .unwrap();
        watcher.join().unwrap();

        assert!(
            !result.success,
            "file IPC consumption with live exchange edits and unchanged disk content must fall back"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "unacknowledged live-edit IPC must not become the saved snapshot"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("file_ipc_live_exchange_unacknowledged")
                && log.contains("patch_id=patch-live-edit"),
            "unacknowledged live-edit IPC should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=live_exchange_without_ack_content")
                && log.contains("recovery=direct_write_fallback"),
            "unacknowledged live-edit IPC should name its invariant and recovery:\n{log}"
        );
    }

    #[test]
    fn file_ipc_accepts_ack_content_sidecar_when_disk_lags_live_exchange() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let ack_content = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "### Re: live prompt — gpt-5\n\n",
            "Handled.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let ack_for_watcher = ack_content.to_string();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    if let Ok(text) = fs::read_to_string(entry.path())
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                    {
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch =
            crate::template::PatchBlock::new("exchange", "### Re: live prompt — gpt-5\n\nHandled.");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            None,
            None,
            Some("patch-live-edit-ack"),
        )
        .unwrap();
        watcher.join().unwrap();

        assert!(
            result.success,
            "ack-content proof should let file IPC accept an applied response even when disk still shows the pre-ack live exchange edit"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(ack_content),
            "snapshot must use the authoritative ack-content sidecar"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            before,
            "this regression models the sidecar-only path where the editor proved the apply before disk caught up"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("file_ipc_live_exchange_unacknowledged"),
            "ack-content proof must bypass the unacknowledged live-edit fallback:\n{log}"
        );
    }

    #[test]
    fn file_ipc_ack_content_live_prompt_drift_uses_content_ours_snapshot() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ New prompt typed during closeout\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let ack_content = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ New prompt typed during closeout\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let ack_for_watcher = ack_content.to_string();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    if let Ok(text) = fs::read_to_string(entry.path())
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                    {
                        let _ = fs::write(&doc_for_watcher, &ack_for_watcher);
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("patch-live-prompt-drift"),
        )
        .unwrap();
        watcher.join().unwrap();

        assert!(
            result.success,
            "IPC delivery itself should remain successful"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "snapshot must not absorb prompt-bearing drift typed after preflight"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            ack_content,
            "visible live prompt should remain in the working tree for the next cycle"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("flow=document_mutation")
                && log.contains("stage=ipc_snapshot_adoption")
                && log.contains("reason=live_prompt_drift_after_preflight")
                && log.contains("ipc_snapshot_adoption_blocked"),
            "unsafe snapshot adoption should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=live_prompt_drift_after_preflight")
                && log.contains("recovery=content_ours_snapshot_next_cycle"),
            "live prompt drift should name its failed invariant and recovery:\n{log}"
        );
    }

    // #exch-intermix fixtures: a wedge requires the adopted `content_ours`
    // snapshot (baseline + response) to be meaningfully larger than the
    // fragmented visible file that lost the response.
    fn drift_baseline() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn drift_content_ours() -> String {
        // baseline + a substantial `### Re:` response (well over the 100-byte
        // stale-drift threshold) so adopting it is a real wedge.
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "### Re: do #fix — opus-4-8\n\n",
            "Implemented the fix and verified it end to end. The response body is long\n",
            "enough to clear the stale-snapshot-reset-drift threshold so the wedge shape\n",
            "is genuinely detected by the recovery discriminator under test here.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn start_live_prompt_drift_ack_listener(
        project_root: &Path,
        ack_content: String,
    ) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let root_clone = root.clone();
            let _ = crate::ipc_socket::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = fs::create_dir_all(&ack_dir);
                if let Some(file_path) = v.get("file").and_then(|value| value.as_str()) {
                    let _ = fs::write(file_path, &ack_content);
                }
                let _ = fs::write(ack_dir.join(format!("{patch_id}.md")), &ack_content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    fn wait_for_live_prompt_drift_listener(project_root: &Path) {
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_accepts_benign_wedge() {
        // Snapshot owns the response the fragmented disk file lost; no disk-only
        // user prompt → safe to auto-recover.
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();
        assert!(
            live_prompt_drift_auto_recovery_safe(&snapshot, &fragmented),
            "benign live-prompt-drift wedge should be recoverable"
        );
    }

    #[test]
    fn live_prompt_drift_convergence_patches_builds_replace_patch_for_exchange() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();

        let patches = live_prompt_drift_convergence_patches(&fragmented, &snapshot).unwrap();

        assert_eq!(patches.len(), 1, "only exchange should need convergence");
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["op"], "replace");
        assert!(
            patches[0]["content"]
                .as_str()
                .unwrap()
                .contains("### Re: do #fix"),
            "replace payload should carry the recovered response body: {patches:?}"
        );
    }

    #[test]
    fn try_compact_editor_converge_falls_back_to_disk_without_listener() {
        // `#w42v`: with no live JB IPC listener, compact convergence must report
        // disk fallback (Ok(false)) so the caller does the guarded disk write —
        // never silently skip the compaction.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let current = drift_baseline();
        let compacted = drift_content_ours();
        std::fs::write(&doc, &current).unwrap();

        let converged = try_editor_converge(&doc, &compacted, &current, "compact").unwrap();
        assert!(
            !converged,
            "without a live JB IPC listener, compact must fall back to the disk write"
        );
    }

    /// Pre-compact document with a multi-item `queue` an operator could be
    /// concurrently editing while compaction archives the exchange tail.
    fn compact_convergence_source() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #a\n",
            "### Re: do #a — opus-4-8\n\n",
            "A long historical response body that compaction will archive and replace\n",
            "with a summary marker so the exchange shrinks substantially past the\n",
            "stale-drift threshold and a genuine convergence patch is produced.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    /// Post-compact document: the `exchange` collapses to a summary marker while
    /// the `queue` is byte-identical to the source (compaction never touches it).
    fn compact_convergence_compacted() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "*Compacted. Content archived to `.agent-doc/archives/test.md`*\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    #[test]
    fn compact_convergence_is_exchange_scoped_preserving_concurrent_queue_edits() {
        // `#jbcompactcrdt`/`#w42v`: compaction only rewrites `exchange`, so the
        // editor-IPC convergence patch must be component-scoped to `exchange` and
        // never carry a `queue` replace. That scoping is exactly what lets an
        // operator concurrently typing queue items survive compaction without a
        // JB `File Cache Conflict` — the editor applies the exchange `op:replace`
        // via the Document API and leaves the live queue buffer untouched.
        let source = compact_convergence_source();
        let compacted = compact_convergence_compacted();

        let patches = live_prompt_drift_convergence_patches(&source, &compacted).unwrap();

        assert_eq!(
            patches.len(),
            1,
            "only exchange changed during compaction; queue must not be patched: {patches:?}"
        );
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["op"], "replace");
        assert!(
            patches[0]["content"]
                .as_str()
                .unwrap()
                .contains("*Compacted. Content archived"),
            "the exchange replace must carry the compacted summary body: {patches:?}"
        );
        assert!(
            !patches
                .iter()
                .any(|patch| patch["component"] == "queue"),
            "a queue replace would clobber the operator's concurrent edits: {patches:?}"
        );
    }

    #[test]
    fn try_compact_editor_converge_converges_via_editor_ipc_with_listener() {
        // `#jbcompactcrdt`/`#w42v`: with a live JB IPC listener, compaction must
        // converge the compacted document through the editor (`transport=editor_ipc`)
        // instead of a direct disk write that diverges from the open buffer and
        // raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = compact_convergence_source();
        let compacted = compact_convergence_compacted();
        fs::write(&doc, &source).unwrap();

        // The fake editor acks with the compacted content, mirroring a JB plugin
        // that applied the exchange `op:replace` and converged its buffer.
        let _listener = start_live_prompt_drift_ack_listener(dir.path(), compacted.clone());
        wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &compacted, &source, "compact").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_editor_convergence_attempt"),
            "compact convergence attempt should be observable in ops.log:\n{log}"
        );
        assert!(
            log.contains("compact_writeback") && log.contains("transport=editor_ipc"),
            "successful compaction must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged compaction must not also take the disk fallback:\n{log}"
        );
    }

    /// Pre-consume document with a `go` queue head an operator could be concurrently
    /// editing while the queue is struck.
    fn queue_consume_convergence_source() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "### Re: do #fix — opus-4-8\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    /// Post-consume document: only the `queue` head is struck; every other
    /// component is byte-identical (queue consume never touches the exchange).
    fn queue_consume_convergence_target() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "### Re: do #fix — opus-4-8\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- ~~do [#fix]~~\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    #[test]
    fn queue_consume_writeback_converges_via_editor_ipc_with_listener() {
        // `#fcc0`: the queue-consume write must route through the shared
        // converger so an active JB listener converges the struck queue through
        // the editor (`transport=editor_ipc`, `queue_consume`-labelled) instead of
        // a direct disk write that raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = queue_consume_convergence_source();
        let target = queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = start_live_prompt_drift_ack_listener(dir.path(), target.clone());
        wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_convergence_attempt"),
            "queue-consume convergence attempt should be source-labelled in ops.log:\n{log}"
        );
        assert!(
            log.contains("queue_consume_writeback") && log.contains("transport=editor_ipc"),
            "a converged queue consume must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged queue consume must not also take the disk fallback:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_falls_back_to_guarded_disk_without_listener() {
        // `#fcc0`: with no live JB listener the shared converger must land the
        // target on disk via the guarded write and record the source-labelled
        // disk fallback — never silently skip the write.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = queue_consume_convergence_source();
        let target = queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_document_or_disk(&doc, &target, &source, "queue_consume").unwrap();

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger must write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=disk_fallback")
                && log.contains("reason=no_listener"),
            "a no-listener queue consume must record the source-labelled disk fallback:\n{log}"
        );
    }

    #[test]
    fn try_editor_converge_skips_wedged_socket_when_latched_degraded() {
        // `#fcc0e`: once the de-wedge latch trips degraded (repeated socket ack
        // timeouts) and no live listener can be re-probed, the converger must
        // short-circuit to the disk fallback (`reason=listener_degraded`) instead
        // of hammering the wedged socket on every queue/template converge — the
        // same skip the reposition/finalize socket paths already take.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = queue_consume_convergence_source();
        let target = queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        // Trip the degraded latch (threshold = 2 distinct ack timeouts), mirroring
        // the existing dewedge tests. No live listener exists, so the self-heal
        // re-probe in `ipc_direct_disk_degraded` cannot clear it.
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "queue_consume").unwrap();
        let degraded =
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "queue_consume").unwrap();
        assert!(degraded, "two distinct ack timeouts must trip the degraded latch");

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            !converged,
            "a latched-degraded session must skip the socket and disk-fall-back"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback") && log.contains("reason=listener_degraded"),
            "the degraded skip must be source-labelled in ops.log:\n{log}"
        );
        assert!(
            !log.contains("reason=no_listener"),
            "the degraded check must short-circuit before the no_listener check:\n{log}"
        );
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_no_wedge() {
        // Snapshot == file: no wedge, nothing to recover, must not fire.
        let snapshot = drift_content_ours();
        assert!(
            !live_prompt_drift_auto_recovery_safe(&snapshot, &snapshot),
            "no drift means no auto-recovery"
        );
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_exchange_prompt() {
        // The visible file carries a NEW user prompt the snapshot never saw —
        // adopting content_ours would silently drop it. Fail closed.
        let snapshot = drift_content_ours();
        let mut fragmented = drift_baseline();
        fragmented = fragmented.replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\n❯ do #brand-new-user-prompt-typed-after-preflight\n<!-- /agent:exchange -->",
        );
        assert!(
            !live_prompt_drift_auto_recovery_safe(&snapshot, &fragmented),
            "a disk-only user prompt must block auto-recovery"
        );
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_queue_item() {
        // A user-added `do [#id]` queue line on disk the snapshot lacks must
        // block auto-recovery (the silent-queue-deletion class).
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline().replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-queue-item]\n<!-- /agent:queue -->",
        );
        assert!(
            !live_prompt_drift_auto_recovery_safe(&snapshot, &fragmented),
            "a disk-only queue item must block auto-recovery"
        );
    }

    // -------- #fintol1: response_target_disjoint_from_user_edit primitive --------

    #[test]
    fn fintol_queue_directive_carries_forward() {
        // A user adds a `do [#id]` queue directive — a next-cycle instruction, not
        // a plain content edit — so it is carried forward, never forward-merged.
        let baseline = drift_baseline();
        let ours = drift_content_ours();
        let candidate = ours.replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-directive]\n<!-- /agent:queue -->",
        );
        assert!(
            !response_target_disjoint_from_user_edit(&baseline, &ours, &candidate),
            "a queue do-directive is a next-cycle instruction and must carry forward"
        );
    }

    #[test]
    fn fintol_plain_outside_edit_is_forward_mergeable() {
        // A plain prose edit OUTSIDE `exchange` (no prompt/directive) is the case
        // that forward-merges: the response and the edit occupy independent regions.
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\nold parked note body\n-->\n",
        )
        .to_string();
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented and verified with a long-enough response body to matter.\n<!-- /agent:exchange -->",
        );
        let candidate = ours.replace("old parked note body", "edited parked note body");
        assert!(
            response_target_disjoint_from_user_edit(&baseline, &ours, &candidate),
            "a plain comment-note edit outside the response must be forward-mergeable"
        );
    }

    #[test]
    fn fintol_response_body_rewrite_collides_fails_closed() {
        // The user rewrites the response body the agent just wrote — a genuine
        // collision in the response target span. `git merge-file` would
        // append-resolve it into a duplicated response, so the exchange-confinement
        // guard must reject it instead.
        let baseline = drift_baseline();
        let ours = drift_content_ours();
        let candidate = ours.replace(
            "Implemented the fix and verified it end to end. The response body is long",
            "User rewrote the committed response body inside the live buffer.",
        );
        assert!(
            !response_target_disjoint_from_user_edit(&baseline, &ours, &candidate),
            "a user rewrite of the response body must collide and fail closed"
        );
    }

    #[test]
    fn fintol_new_exchange_prompt_below_is_not_forward_merged() {
        // A new prompt typed below the response is disjoint but lives in
        // `exchange`; it is carried forward uncommitted, never folded into the
        // commit, so the primitive excludes it.
        let baseline = drift_baseline();
        let ours = drift_content_ours();
        let candidate = ours.replace(
            "<!-- /agent:exchange -->",
            "❯ a brand new prompt typed during closeout\n<!-- /agent:exchange -->",
        );
        assert!(
            !response_target_disjoint_from_user_edit(&baseline, &ours, &candidate),
            "a new exchange prompt must stay on the carry-forward path"
        );
    }

    #[test]
    fn fintol_no_user_edit_is_not_forward_merged() {
        // No concurrent user edit (candidate == content_ours): there is nothing to
        // forward-merge, so the primitive returns false (the caller commits
        // content_ours directly).
        let baseline = drift_baseline();
        let ours = drift_content_ours();
        assert!(
            !response_target_disjoint_from_user_edit(&baseline, &ours, &ours),
            "an unchanged candidate has no user edit to forward-merge"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_writes_snapshot_when_blocked_and_safe() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // The drift guard fired this cycle and adopted content_ours.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "recovery should return the adopted snapshot content"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the working-tree file should now carry the full response"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("live_prompt_drift_auto_recovered"),
            "auto-recovery must leave an observable ops.log marker:\n{log}"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_prefers_editor_ipc_when_listener_active() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = start_live_prompt_drift_ack_listener(dir.path(), snapshot.clone());
        wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "recovery should accept the editor-applied snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the fake editor listener should converge the working tree through IPC"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("[jbstalecache] editor_convergence_attempt")
                && log.contains("[jbstalecache] editor_convergence_succeeded"),
            "active listener recovery should be observable as editor convergence:\n{log}"
        );
        assert!(
            log.contains("live_prompt_drift_auto_recovered")
                && log.contains("transport=editor_ipc")
                && log.contains("ipc_listener_active=true"),
            "recovery marker should name the editor transport:\n{log}"
        );
        assert!(
            !log.contains("auto_recovery_disk_write_during_ipc_listener")
                && !log.contains("transport=disk_fallback"),
            "successful editor convergence must not take the stale-cache disk fallback:\n{log}"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_skips_without_blocked_flag() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // A cycle exists but the drift guard never fired (flag stays false) →
        // not the wedge we own.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "without the drift flag this is not the auto-recovery case"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when recovery does not apply"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_skips_when_dropped_prompts_recorded() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // A genuine dropped user prompt was recorded this cycle → session-check
        // owns the fail-closed; auto-recovery must NOT paper over it.
        crate::cycle_state::record_dropped_exchange_prompts(&doc, &["do #dropped".to_string()])
            .unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "recorded dropped prompts must block auto-recovery (fail closed)"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when a dropped prompt was recorded"
        );
    }

    #[test]
    fn snapshot_contains_dropped_prompt_matches_consumed_and_active() {
        let snapshot = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~do [#consumed]~~\n",
            "- do [#active]\n",
            "<!-- /agent:queue -->\n",
        );
        // Consumed (struck) item still present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#consumed]"));
        // Active item present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#active]"));
        // Genuinely absent → real loss.
        assert!(!snapshot_contains_dropped_prompt(snapshot, "do [#gone]"));
    }

    // #exch-intermix-falsedrop: a queue item consumed (struck) this cycle is
    // recorded as "dropped" by the drift-time candidate-vs-content_ours heuristic,
    // but it survives struck in the adopted snapshot, so auto-recovery must STILL
    // fire (it is a false-positive drop record, not real user-content loss). This
    // is the exact live wedge from agent-doc-bugs2 #opsproof-falsepos closeout.
    #[test]
    fn try_auto_recover_live_prompt_drift_fires_when_dropped_prompt_is_consumed_in_snapshot() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        // Snapshot consumed the queued `do [#fix]` (struck) and carries the full
        // `### Re:` response; the fragmented disk file also struck it but lost the
        // response body → wedge shape.
        let snapshot = drift_content_ours().replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        let fragmented = drift_baseline().replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // The drift heuristic recorded the consumed item as a dropped queue prompt.
        crate::cycle_state::record_dropped_queue_prompts(&doc, &["do [#fix]".to_string()]).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_some(),
            "a dropped prompt that survives (struck) in the snapshot must not block recovery"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "auto-recovery must write the adopted snapshot to disk"
        );

        // `#jbstalecache`: the recovery write records the IPC-listener state so the
        // operator can correlate a stale-cache dialog with this disk write. No live
        // listener exists in the test env, so the canonical marker reports
        // `ipc_listener_active=false` and the dedicated stale-cache-risk line stays
        // silent (it only fires when a listener is genuinely active).
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("live_prompt_drift_auto_recovered")
                && ops_log.contains("ipc_listener_active=false"),
            "recovery marker must record the IPC-listener state:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("[jbstalecache]"),
            "the stale-cache-risk marker must stay silent without an active listener:\n{ops_log}"
        );
    }

    #[test]
    fn ipc_snapshot_adoption_allowed_logs_benign_recheck() {
        // Every adoption that the fail-closed guards did NOT block must still leave
        // a diagnostic so a corruption slipping through as "allowed" is traceable.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n",
            "<!-- /agent:exchange -->\n",
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n### Re: Q — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n",
        );
        let decision = IpcRepairDecision::content_ours(content_ours.to_string());

        log_ipc_snapshot_adoption_allowed(
            &doc,
            "socket_ack_content",
            Some("pid-allowed"),
            Some(baseline),
            Some(content_ours),
            &decision,
            false,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_snapshot_adoption_allowed")
                && log.contains("source=socket_ack_content")
                && log.contains("patch_id=pid-allowed")
                && log.contains("drift_recheck=false")
                && log.contains("dup_growth_recheck=0"),
            "allowed adoption must log a benign re-check:\n{log}"
        );
    }

    #[test]
    fn ipc_snapshot_adoption_allowed_is_silent_when_blocked() {
        // Blocked adoptions log their own rich diagnostic; the allowed line must not
        // also fire (it would falsely report an unguarded adoption).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let decision = IpcRepairDecision::content_ours("snapshot".to_string());
        log_ipc_snapshot_adoption_allowed(
            &doc,
            "file_ipc",
            Some("pid-blocked"),
            None,
            None,
            &decision,
            true,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipc_snapshot_adoption_allowed"),
            "allowed diagnostic must stay silent once a guard fired:\n{log}"
        );
    }

    #[test]
    fn ipcfullprompt_corruption_logged_on_deleted_response() {
        // #ipcfullprompt-recur2: a live editor buffer (candidate) that dropped a
        // previously-committed `### Re:` block must leave a forensic ops.log line
        // and preserve the baseline + candidate for analysis — default-on capture,
        // no manual editor debug opt-in required.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );
        // candidate dropped the second response block.
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_ack_content",
            Some("pid-corrupt"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("source=socket_ack_content")
                && log.contains("patch_id=pid-corrupt")
                && log.contains("deleted=1")
                && log.contains("response_deleted(### Re: second — opus-4-8:1->0)"),
            "deleted prior response must be captured:\n{log}"
        );
        let forensic_dir = agent_doc_dir.join("logs/ipcfullprompt");
        let preserved: Vec<_> = fs::read_dir(&forensic_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            preserved.iter().any(|n| n.ends_with(".baseline.md"))
                && preserved.iter().any(|n| n.ends_with(".candidate.md")),
            "forensic baseline + candidate must be preserved: {preserved:?}"
        );
    }

    #[test]
    fn ipcfullprompt_scaffold_duplication_logged_without_baseline() {
        // The brandon-cinquegrana.md shape: a full-tail duplication leaves two
        // `<!-- /agent:exchange -->` markers around an in-progress prompt. This is
        // a self-check on the candidate, so it must fire even with no baseline.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let candidate = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: prior — opus-4-8\nAnswer.\n",
            "<!-- agent:boundary:709a41ae -->\n",
            "Is the issue still happening?\nCan it be re\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "Can it be rep11ro\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_ack_content",
            Some("pid-x"),
            None,
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("scaffold_duplicated=")
                && log.contains("scaffold_duplicated(<!-- /agent:exchange -->:1->2)"),
            "full-tail scaffold duplication must be captured without a baseline:\n{log}"
        );
    }

    #[test]
    fn ipcfullprompt_corruption_silent_on_clean_candidate() {
        // A candidate that only *adds* a new response (expected growth) must not
        // be flagged — no false positive on normal cycles.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "file_ipc",
            Some("pid-clean"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipcfullprompt_corruption_suspected"),
            "clean growth must not be flagged as corruption:\n{log}"
        );
    }

    #[test]
    fn file_ipc_ack_content_post_exchange_comment_drift_uses_content_ours_snapshot() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Typing a new prompt below exchange during closeout. #next-steps\n",
            "-->\n"
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n"
        );
        let ack_content = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Typing a new prompt below exchange during closeout. #next-steps\n",
            "-->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let ack_for_watcher = ack_content.to_string();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    if let Ok(text) = fs::read_to_string(entry.path())
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                    {
                        let _ = fs::write(&doc_for_watcher, &ack_for_watcher);
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("patch-post-exchange-comment-drift"),
        )
        .unwrap();
        watcher.join().unwrap();

        assert!(
            result.success,
            "IPC delivery itself should remain successful"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "snapshot must not absorb post-exchange comment text typed after preflight"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            ack_content,
            "visible post-exchange comment text should remain in the working tree for the next cycle"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("flow=document_mutation")
                && log.contains("stage=ipc_snapshot_adoption")
                && log.contains("reason=live_prompt_drift_after_preflight")
                && log.contains("ipc_snapshot_adoption_blocked"),
            "unsafe post-exchange drift adoption should be logged:\n{log}"
        );
    }

    #[test]
    fn file_ipc_post_dedupe_unchanged_exchange_requires_ack_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let plugin_after = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    let _ = fs::write(&doc_for_watcher, plugin_after);
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = crate::template::PatchBlock::new("exchange", "live prompt\n");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            None,
            None,
            Some("patch-post-dedupe"),
        )
        .unwrap();

        assert!(
            !result.success,
            "file IPC must fall back when final deduped exchange is unchanged without ack-content proof"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "unacknowledged post-dedupe no-op IPC must not become the saved snapshot"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("file_ipc_live_exchange_unacknowledged")
                && log.contains("patch_id=patch-post-dedupe"),
            "post-dedupe unacknowledged live-edit IPC should be logged:\n{log}"
        );
        assert!(
            !log.contains("snapshot_saved_file_ipc"),
            "post-dedupe unacknowledged live-edit IPC must not save a file-IPC snapshot:\n{log}"
        );
    }

    #[test]
    fn try_ipc_full_content_returns_false_when_disabled() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let result = try_ipc_full_content(&doc, "new content").unwrap();
        assert!(
            !result,
            "full-content IPC is disabled and should return false"
        );
    }

    // --- sanitize_component_tags tests ---

    #[test]
    fn sanitize_escapes_open_agent_tag() {
        let input = "Here is an example: <!-- agent:exchange --> marker.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "open agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- agent:exchange -->"),
            "raw open agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_escapes_close_agent_tag() {
        let input = "End marker: <!-- /agent:pending --> done.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- /agent:pending --&gt;"),
            "close agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- /agent:pending -->"),
            "raw close agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_does_not_escape_patch_markers() {
        let input = "<!-- patch:exchange -->\nsome content\n<!-- /patch:exchange -->\n";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "patch markers must not be escaped");
    }

    #[test]
    fn sanitize_passes_normal_content_through() {
        let input = "Just some normal markdown content.\n\nWith paragraphs and **bold**.";
        let result = sanitize_component_tags(input);
        assert_eq!(
            result, input,
            "normal content should pass through unchanged"
        );
    }

    #[test]
    fn sanitize_preserves_utf8_em_dash() {
        // Em dash U+2014 is 3 bytes in UTF-8: 0xE2, 0x80, 0x94
        let input = "This is a test \u{2014} with em dashes \u{2014} in content.";
        let result = sanitize_component_tags(input);
        assert_eq!(
            result, input,
            "em dashes must survive sanitization unchanged"
        );

        // Verify at the byte level
        assert_eq!(
            result.as_bytes(),
            input.as_bytes(),
            "byte-level content must be identical"
        );
    }

    #[test]
    fn sanitize_preserves_mixed_utf8_and_agent_tags() {
        // Content with UTF-8 characters AND agent tags that need escaping
        let input = "Response with \u{2014} em dash and <!-- agent:exchange --> tag reference.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("\u{2014}"),
            "em dash must be preserved, got: {:?}",
            result
        );
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "agent tag must be escaped"
        );
    }

    #[test]
    fn sanitize_preserves_various_unicode() {
        // Test various multi-byte UTF-8 characters
        let input = "Caf\u{00E9} \u{2019}quotes\u{2019} \u{2014} \u{2026} \u{1F600}";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "all unicode must survive sanitization");
    }

    #[test]
    fn sanitize_unmatched_escapes_exchange_markers_in_response() {
        let mut unmatched =
            "### Re: deploy\n\nDone.\n\n<!-- agent:exchange -->\nExtra\n<!-- /agent:exchange -->\n"
                .to_string();
        sanitize_unmatched(&mut unmatched);
        assert!(
            !unmatched.contains("<!-- agent:exchange -->"),
            "agent exchange markers must be escaped in unmatched text, got: {unmatched}"
        );
        assert!(
            unmatched.contains("&lt;!-- agent:exchange --&gt;"),
            "escaped markers expected, got: {unmatched}"
        );
    }

    #[test]
    fn apply_patches_sanitize_unmatched_prevents_duplicate_exchange_block() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Existing answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        let unmatched = "### Re: deploy — gpt-5\n\nDeployed.\n\n<!-- agent:exchange -->\nLeaked content\n<!-- /agent:exchange -->\n";
        let mut sanitized_unmatched = unmatched.to_string();
        sanitize_unmatched(&mut sanitized_unmatched);

        let result = crate::template::apply_patches(doc, &[], &sanitized_unmatched, &file).unwrap();

        let exchange_opens = result.matches("<!-- agent:exchange").count();
        assert_eq!(
            exchange_opens, 1,
            "must have exactly one exchange opener, got {exchange_opens}:\n{result}"
        );
        assert!(
            !result.contains("<!-- agent:exchange -->\nLeaked content"),
            "leaked exchange markers must be escaped, not create a second block"
        );
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "escaped markers should appear in result"
        );
    }

    #[test]
    fn try_ipc_snapshot_saves_disk_state() {
        // Verify that after IPC succeeds, the snapshot contains the actual post-write
        // disk state (file read after the 200ms flush delay), NOT content_ours.
        // Using the actual disk state prevents stale baselines from perpetuating
        // ghost diffs cycle after cycle.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "---\nsession: test\n---\n\n<!-- agent:exchange -->\noriginal content\n<!-- agent:boundary:test-boundary-123 -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response content");

        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\n<!-- /agent:exchange -->\n";

        // Simulate the plugin applying the patch and performing a safe boundary
        // reposition in the same editor-visible write.
        let after_plugin_write = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\n<!-- agent:boundary:plugin-boundary -->\n<!-- /agent:exchange -->\n";

        // Spawn "plugin" thread that watches for patch files, writes content, then deletes
        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let after_plugin_write_owned = after_plugin_write.to_string();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            // Simulate plugin applying patch + leaving user edits in file
                            let _ = fs::write(&doc_for_watcher, &after_plugin_write_owned);
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),     // baseline
            Some(content_ours), // content_ours (no longer used for snapshot)
            None,               // normalize_prefix_lines
            None,               // reuse_patch_id
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        // KEY ASSERTION: snapshot must match actual disk state (includes user edits)
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("agent response content"),
            "snapshot must contain agent response, got: {}",
            snap
        );
        assert!(snap.contains("plugin-boundary"));
        assert_eq!(
            snap, after_plugin_write,
            "snapshot must exactly match post-write disk state"
        );
    }

    #[test]
    fn ipc_json_preserves_utf8_em_dash() {
        // Verify that serde_json serialization preserves em dashes in IPC payloads
        let content = "Response with \u{2014} em dash.";
        let payload = serde_json::json!({
            "file": "/tmp/test.md",
            "patches": [{
                "component": "exchange",
                "content": content,
            }],
            "unmatched": "",
            "baseline": "",
        });

        let json_str = serde_json::to_string_pretty(&payload).unwrap();
        // Parse it back and verify the content is preserved
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let parsed_content = parsed["patches"][0]["content"].as_str().unwrap();
        assert_eq!(
            parsed_content, content,
            "em dash must survive JSON round-trip"
        );

        // Also verify the raw JSON contains the UTF-8 bytes, not escaped sequences
        assert!(
            json_str.contains("\u{2014}"),
            "JSON should contain raw UTF-8 em dash"
        );
    }

    // --- is_append_mode_component tests ---

    #[test]
    fn append_mode_component_exchange() {
        assert!(is_append_mode_component("exchange"));
        assert!(is_append_mode_component("findings"));
    }

    #[test]
    fn replace_mode_components_not_append() {
        assert!(!is_append_mode_component("pending"));
        assert!(!is_append_mode_component("backlog"));
        assert!(!is_append_mode_component("status"));
        assert!(!is_append_mode_component("output"));
        assert!(!is_append_mode_component("todo"));
    }

    #[test]
    fn find_boundary_id_skips_code_blocks() {
        // Boundary-looking text inside a fenced code block must not be returned
        let content = "<!-- agent:exchange -->\n```\n<!-- agent:boundary:fake-id -->\n```\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert!(
            result.is_none(),
            "boundary inside code block must not be found, got: {:?}",
            result
        );
    }

    #[test]
    fn find_boundary_id_finds_real_marker() {
        let content = "<!-- agent:exchange -->\nSome text.\n<!-- agent:boundary:real-uuid-5678 -->\nMore text.\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert_eq!(result, Some("real-uuid-5678".to_string()));
    }

    #[test]
    fn build_ipc_node_patches_json_tracks_strike_and_insert_by_node_key() {
        let before = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let after = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- ~~do [#beta]~~
- do [#gamma]
<!-- /agent:queue -->
";

        let patches = build_ipc_node_patches_json(Some(before), Some(after));

        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:beta:0"
                && patch["op"] == "strike"
                && patch["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("- ~~do [#beta]~~"))
        }));
        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:gamma:0"
                && patch["op"] == "insert"
                && patch["after"] == "queue:0:beta:0"
                && patch["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("- do [#gamma]"))
        }));
    }

    #[test]
    fn build_ipc_node_patches_json_tracks_reorder_without_text_matching() {
        let before = "\
<!-- agent:queue priority go -->
- do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let after = "\
<!-- agent:queue priority go -->
- do [#beta]
- do [#alpha]
<!-- /agent:queue -->
";

        let patches = build_ipc_node_patches_json(Some(before), Some(after));

        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:beta:0"
                && patch["op"] == "move"
                && patch["before"] == "queue:0:alpha:0"
        }));
    }

    #[test]
    fn stale_baseline_guard_prefix_check() {
        // Baseline that starts with snapshot content (user added text) = NOT stale
        let snapshot = "## Exchange\nResponse here.\n";
        let baseline_with_user_edit = "## Exchange\nResponse here.\nNew user question\n";
        let snap_clean = strip_boundary_for_dedup(snapshot);
        let base_clean = strip_boundary_for_dedup(baseline_with_user_edit);
        assert!(
            base_clean.starts_with(&snap_clean),
            "baseline with user edits should start with snapshot content"
        );

        // Baseline that doesn't contain snapshot content = STALE
        let stale_baseline = "## Exchange\nOld content only.\n";
        let stale_clean = strip_boundary_for_dedup(stale_baseline);
        assert!(
            !stale_clean.starts_with(&snap_clean),
            "stale baseline should not start with snapshot content"
        );
    }

    // --- is_stale_baseline tests ---

    #[test]
    fn stale_baseline_identical_content_not_stale() {
        let doc = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(doc, doc));
    }

    #[test]
    fn stale_baseline_user_appended_text_not_stale() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn stale_baseline_user_edited_replace_component_not_stale() {
        // User edits replace-mode component (status) — should NOT trigger stale guard
        let snapshot = "<!-- agent:status patch=replace -->\nOld status\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nEdited status by user\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\nNew question\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "user editing replace-mode status component should NOT trigger stale guard"
        );
    }

    #[test]
    fn stale_baseline_missing_committed_content_is_stale() {
        let snapshot = "<!-- agent:exchange patch=append -->\nCommitted response from agent.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content only.\n<!-- /agent:exchange -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "baseline missing committed content should be stale"
        );
    }

    #[test]
    fn stale_baseline_missing_append_component_is_stale() {
        // Missing an append-mode component = stale
        let snapshot =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:other patch=append -->\nDifferent.\n<!-- /agent:other -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "baseline missing an append-mode component should be stale"
        );
    }

    #[test]
    fn stale_baseline_missing_replace_component_not_stale() {
        // Missing a replace-mode component is fine — user can delete it
        let snapshot = "<!-- agent:status patch=replace -->\nActive\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "missing replace-mode component should NOT trigger stale guard"
        );
    }

    #[test]
    fn stale_baseline_boundary_markers_ignored() {
        let snapshot = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:xyz -->\nUser edit\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "different boundary marker IDs should not cause false stale detection"
        );
    }

    #[test]
    fn stale_baseline_non_template_fallback_to_prefix() {
        // Non-template (no components) falls back to prefix check
        let snapshot = "## Exchange\nResponse.\n";
        let baseline = "## Exchange\nResponse.\nNew question\n";
        assert!(!is_stale_baseline(baseline, snapshot));

        let stale = "## Exchange\nDifferent content.\n";
        assert!(is_stale_baseline(stale, snapshot));
    }

    #[test]
    fn stale_baseline_empty_snapshot_component_skipped() {
        // Empty append components in snapshot should not cause false positives
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nUser added content\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn stale_baseline_default_exchange_is_append() {
        // exchange without explicit patch attr defaults to append via is_append_mode_component
        let snapshot = "<!-- agent:exchange -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange -->\nOld stuff.\n<!-- /agent:exchange -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "exchange without patch attr should default to append-mode check"
        );
    }

    #[test]
    fn strip_boundary_for_dedup_removes_markers() {
        let with_boundary = "Hello\n<!-- agent:boundary:abc123 -->\nWorld\n";
        let without = strip_boundary_for_dedup(with_boundary);
        assert!(!without.contains("agent:boundary"));
        assert!(without.contains("Hello"));
        assert!(without.contains("World"));
    }

    // --- build_ipc_patches_json / synthesis dedup tests ---

    #[test]
    fn synthesis_dedup_skips_when_content_already_present() {
        // If the unmatched content already exists in the target component,
        // synthesis should be skipped (idempotent write guard).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let existing = "This is the agent response.";
        let doc_content = format!(
            "<!-- agent:exchange patch=append -->\n{}\n<!-- /agent:exchange -->\n",
            existing
        );
        fs::write(&doc, &doc_content).unwrap();

        // No explicit patches (simulates skill sending raw content)
        let patches: Vec<crate::template::PatchBlock> = vec![];
        // Unmatched content is identical to what's already in the exchange
        let result = build_ipc_patches_json(&doc, &patches, existing, None, None).unwrap();

        assert!(
            result.is_empty(),
            "synthesis should be skipped when content already exists in target component, \
             got {} patches: {:?}",
            result.len(),
            result
        );
    }

    #[test]
    fn synthesis_proceeds_when_content_is_new() {
        // When unmatched content is NOT present in the target component,
        // synthesis should create an IPC patch.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nExisting content.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let new_content = "Completely new agent response.";
        let result = build_ipc_patches_json(&doc, &patches, new_content, None, None).unwrap();

        assert_eq!(
            result.len(),
            1,
            "synthesis should produce one patch for new content"
        );
        assert_eq!(
            result[0]["component"].as_str().unwrap(),
            "exchange",
            "synthesized patch should target exchange"
        );
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            new_content,
            "synthesized patch content should match unmatched"
        );
    }

    #[test]
    fn build_ipc_patches_json_preserves_leading_code_fence_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ show fenced prompt\n",
            "```\n",
            "prompt body\n",
            "```\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, doc_content).unwrap();

        let patches = vec![crate::template::PatchBlock::new(
            "exchange",
            "```\nresponse body\n```\n",
        )];
        let result = build_ipc_patches_json(&doc, &patches, "", None, None).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["component"].as_str().unwrap(), "exchange");
        assert_eq!(result[0]["op"].as_str().unwrap(), "append");
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            "```\nresponse body\n```\n",
            "IPC payload must keep a leading code fence byte-for-byte"
        );
    }

    #[test]
    fn synthesis_normalizes_prefix_lines_for_unmatched_exchange_content() {
        // Regression for the JB-plugin bare `do #expatch...` shape: when IPC
        // synthesizes an exchange patch from unmatched content, it must bake the
        // computed `normalize_prefix_lines` into that synthesized patch because
        // the plugin normalizes before applying patches.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nPrevious response.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let unmatched = "do #expatch. spec-test-build-install-commit-push\n### Re: #expatch — gpt-5\n\nImplemented.\n";
        let prefix_lines = vec!["do #expatch. spec-test-build-install-commit-push".to_string()];
        let result = build_ipc_patches_json(
            &doc,
            &patches,
            unmatched,
            Some(prefix_lines.as_slice()),
            None,
        )
        .unwrap();

        assert_eq!(
            result.len(),
            1,
            "synthesis should still produce one exchange patch"
        );
        assert_eq!(
            result[0]["component"].as_str().unwrap(),
            "exchange",
            "synthesized patch should target exchange"
        );
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            "❯ do #expatch. spec-test-build-install-commit-push\n### Re: #expatch — gpt-5\n\nImplemented.",
            "synthesized unmatched exchange content must carry the prefixed prompt line"
        );
    }

    #[test]
    fn build_ipc_patches_json_seeded_boundary_is_stable_across_rebuilds() {
        // #finalize-visible-buffer-ipc-timeout-race ROOT-CAUSE REGRESSION:
        // a single write builds its IPC patches more than once (socket attempt →
        // file-IPC fallback → run_stream timeout re-write). Each rebuild used to
        // mint a FRESH random boundary, so the plugin saw the same response under
        // two different boundary IDs and appended it twice — doubling the editor
        // buffer (live repro: 57970 → 107235 bytes). Seeding the boundary from the
        // stable patch_id must make every rebuild carry an IDENTICAL boundary.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("agent-doc-bugs2.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nPrior response.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches = vec![crate::template::PatchBlock::new(
            "exchange",
            "### Re: fix\n\nNew response body.",
        )];
        let seed = "2ffa57c0-24e8-441c-aca9-46e6aa6f1c2a";

        // Two rebuilds of the SAME write (same seed) → identical boundary.
        let build_a = build_ipc_patches_json(&doc, &patches, "", None, Some(seed)).unwrap();
        let build_b = build_ipc_patches_json(&doc, &patches, "", None, Some(seed)).unwrap();
        let bid_a = build_a[0]["boundary_id"].as_str();
        let bid_b = build_b[0]["boundary_id"].as_str();
        assert!(
            bid_a.is_some(),
            "patch should carry a boundary_id: {build_a:?}"
        );
        assert_eq!(
            build_a[0]["op"].as_str(),
            Some("append"),
            "append-mode patches should carry an explicit IPC op"
        );
        assert_eq!(
            build_a[0]["node_id"].as_str(),
            bid_a,
            "append-mode patches should address the boundary node"
        );
        assert_eq!(
            bid_a, bid_b,
            "same patch_id seed must yield the SAME boundary across rebuilds (no double-apply)"
        );
        assert_eq!(
            bid_a,
            Some("2ffa57c0:agent-doc-bugs2"),
            "boundary must derive from the patch_id hex prefix + doc-stem slug"
        );

        // A different write (different seed) must NOT collide on one boundary.
        let other_seed = "99887766-1111-2222-3333-444455556666";
        let build_c = build_ipc_patches_json(&doc, &patches, "", None, Some(other_seed)).unwrap();
        assert_ne!(
            build_c[0]["boundary_id"].as_str(),
            bid_a,
            "distinct writes must derive distinct boundaries"
        );
    }

    #[test]
    fn file_ipc_synthesized_exchange_patch_omits_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let prompt = "do #ipcfull. spec-test-build-install-commit-push";
        let original = format!(
            "---\nagent_doc_format: template\n---\n\n\
<!-- agent:exchange patch=append -->\n{prompt}\n<!-- /agent:exchange -->\n"
        );
        let unmatched = "### Re: ipc full-content guard - gpt-5\n\nDone.";
        let after_plugin_write = format!(
            "---\nagent_doc_format: template\n---\n\n\
<!-- agent:exchange patch=append -->\n❯ {prompt}\n{unmatched}\n<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, &original).unwrap();

        let seen_payload = std::sync::Arc::new(std::sync::Mutex::new(None));
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let seen_for_watcher = seen_payload.clone();
        let after_for_watcher = after_plugin_write.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = fs::read_to_string(&path)
                            && let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text)
                        {
                            if let Some(pid) = payload.get("patch_id").and_then(|v| v.as_str()) {
                                let _ = fs::write(
                                    ack_dir.join(format!("{pid}.md")),
                                    &after_for_watcher,
                                );
                            }
                            *seen_for_watcher.lock().unwrap() = Some(payload);
                        }
                        let _ = fs::write(&doc_for_watcher, &after_for_watcher);
                        let _ = fs::remove_file(path);
                        return;
                    }
                }
            }
        });

        let prefix_lines = vec![prompt.to_string()];
        let result = try_ipc(
            &doc,
            &[],
            unmatched,
            None,
            Some(&original),
            Some(&after_plugin_write),
            Some(prefix_lines.as_slice()),
            Some("patch-synth-no-full-content"),
        )
        .unwrap();

        assert!(
            result.success,
            "file IPC should accept the synthesized exchange patch"
        );
        let payload = seen_payload
            .lock()
            .unwrap()
            .clone()
            .expect("watcher should capture the IPC payload");
        assert!(
            payload.get("fullContent").is_none(),
            "template response IPC with a synthesized component patch must not send fullContent: {payload}"
        );
        assert_eq!(
            payload["unmatched"], "",
            "synthesized exchange patch must consume unmatched text instead of sending it twice"
        );
        let patches = payload["patches"]
            .as_array()
            .expect("payload patches should be an array");
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["content"], unmatched);
        assert_eq!(payload["normalize_prefix_lines"][0], prompt);
    }

    #[test]
    fn template_normalization_only_file_ipc_omits_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let prompt = "do #norm-only. spec-test-build-install-commit-push";
        let original = format!(
            "---\nagent_doc_format: template\n---\n\n\
<!-- agent:exchange patch=append -->\n{prompt}\n<!-- agent:boundary:test -->\n<!-- /agent:exchange -->\n"
        );
        let normalized = original.replace(prompt, &format!("❯ {prompt}"));
        fs::write(&doc, &original).unwrap();

        let seen_payload = std::sync::Arc::new(std::sync::Mutex::new(None));
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let normalized_for_watcher = normalized.clone();
        let seen_for_watcher = seen_payload.clone();
        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(3) {
                let Ok(entries) = fs::read_dir(&patches_dir) else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("json") {
                        continue;
                    }
                    let text = fs::read_to_string(&path).unwrap();
                    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
                    let patch_id = payload
                        .get("patch_id")
                        .and_then(|value| value.as_str())
                        .unwrap()
                        .to_string();
                    fs::write(&doc_for_watcher, &normalized_for_watcher).unwrap();
                    fs::write(
                        ack_dir.join(format!("{patch_id}.md")),
                        &normalized_for_watcher,
                    )
                    .unwrap();
                    *seen_for_watcher.lock().unwrap() = Some(payload);
                    fs::remove_file(path).unwrap();
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            false
        });

        let prefix_lines = vec![prompt.to_string()];
        let result = try_ipc(
            &doc,
            &[],
            "",
            None,
            Some(&original),
            Some(&normalized),
            Some(prefix_lines.as_slice()),
            Some("patch-template-norm-only-no-full-content"),
        )
        .unwrap();

        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");
        assert!(
            result.success,
            "normalization-only template IPC should accept a narrow payload"
        );
        let payload = seen_payload
            .lock()
            .unwrap()
            .clone()
            .expect("watcher should capture the IPC payload");
        assert!(
            payload.get("fullContent").is_none(),
            "template normalization-only IPC must not send fullContent: {payload}"
        );
        assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
        assert_eq!(payload["unmatched"], "");
        assert_eq!(payload["normalize_prefix_lines"][0], prompt);

        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("scope=template_frontmatter"),
            "template fullContent rejection should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn effective_unmatched_cleared_when_synthesis_consumes_content() {
        // When synthesis consumes the unmatched content (patches input was empty,
        // ipc_patches output is non-empty), effective_unmatched should be "".
        // This prevents the plugin from applying the content twice (IPC duplicate bug).
        let patches: Vec<crate::template::PatchBlock> = vec![];
        let unmatched = "some response content";

        // Case 1: synthesis happened (patches empty → ipc_patches non-empty)
        let ipc_patches: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": unmatched,
        })];
        let effective = if patches.is_empty() && !ipc_patches.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective, "",
            "effective_unmatched must be empty when synthesis consumed content"
        );

        // Case 2: explicit patches (no synthesis) — unmatched passes through
        let explicit_patch = crate::template::PatchBlock::new("exchange", "response");
        let patches_explicit = [explicit_patch];
        let ipc_explicit: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": "response",
        })];
        let effective2 = if patches_explicit.is_empty() && !ipc_explicit.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective2,
            unmatched.trim(),
            "effective_unmatched should pass through when explicit patches exist"
        );

        // Case 3: no patches, no synthesis (empty doc or dedup skipped it) — unmatched passes through
        let ipc_empty: Vec<serde_json::Value> = vec![];
        let effective3 = if patches.is_empty() && !ipc_empty.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective3,
            unmatched.trim(),
            "effective_unmatched should pass through when no synthesis occurred"
        );
    }

    // ── normalize_user_prompts_in_exchange ──────────────────────────────────

    #[test]
    fn normalize_user_prompts_new_line_gets_prefix() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\nOld content.\n<!-- /agent:exchange -->\n";
        // baseline = user added "Hello" but agent hasn't responded yet
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- /agent:exchange -->\n";
        // content_ours = baseline + agent response appended (boundary at end after pre-patch)
        let content = "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- agent:boundary:abc123 -->\n### Re: response\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Hello"),
            "user line should get ❯  prefix: {}",
            result
        );
        assert!(
            result.contains("Old content."),
            "old content should be preserved"
        );
        assert!(
            result.contains("### Re: response"),
            "agent response should be preserved"
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should not get prefix: {}",
            result
        );
    }

    #[test]
    fn exchange_write_diagnostic_logs_live_edit_provenance() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content.\n<!-- /agent:exchange -->\n";
        let before = "<!-- agent:exchange patch=append -->\nOld content.\nlive prompt\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\nOld content.\n❯ live prompt\nlive prompt\n### Re: response\nDone.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, before).unwrap();
        let patches = vec![template::PatchBlock::new(
            "exchange",
            "### Re: response\nDone.\n",
        )];

        log_exchange_write_diagnostic(
            &doc,
            "test_source",
            "test_mode",
            Some("patch-123"),
            Some(baseline),
            before,
            after,
            &patches,
            "",
        );

        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("exchange_write_diagnostic"));
        assert!(log.contains("source=test_source"));
        assert!(log.contains("write_mode=test_mode"));
        assert!(log.contains("patch_id=patch-123"));
        assert!(log.contains("live_exchange_edited=true"));
        assert!(log.contains("prompt_text_duplicated=true"));
        assert!(log.contains("prompt_text_normalized=true"));
        assert!(log.contains("normalized_prefix_delta=1"));
        assert!(log.contains("before_hash="));
        assert!(log.contains("after_hash="));
        assert!(log.contains("writer_pid="));
    }

    #[test]
    fn ipc_snapshot_dedupes_extra_prompt_copy_against_before_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = "<!-- agent:exchange patch=append -->\nOld content.\nlive prompt\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\nOld content.\n❯ live prompt\nlive prompt\n### Re: response\nDone.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), after, "test_ipc").unwrap();

        assert!(changed);
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(
            !repaired.contains("❯ live prompt\nlive prompt"),
            "duplicate unprefixed prompt should be removed: {repaired}"
        );
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get("live prompt")
                .copied(),
            Some(1)
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("ipc_prompt_duplicate_repaired"));
        assert!(log.contains("ipc_snapshot_deduped"));
    }

    #[test]
    fn prompt_dedupe_skips_assistant_response_quotes() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:old -->\n",
            "quote this exact line\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ quote this exact line\n",
            "### Re: response — gpt-5\n\n",
            "quote this exact line\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), after, "test_ipc").unwrap();

        assert!(
            !changed,
            "assistant response quotes must not be treated as duplicate prompt text"
        );
        assert_eq!(repaired.matches("quote this exact line").count(), 2);
    }

    #[test]
    fn duplicate_prompt_artifact_repair_runs_canonical_pipeline() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "Please keep this duplicate prompt around for canonical cleanup coverage #spec-test-build-install-commit-push";
        let prefix_short = "agent-doc on corky running opencode, the arrow key functionality works at first but once a turn starts the key log shows re ";
        let prefix_long = "agent-doc on corky running opencode, the arrow key functionality works at first but once a turn starts the key log shows received ";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "{prompt}\n",
                "<!-- agent:boundary:old -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let after = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "<!-- agent:boundary:new -->\n",
                "{prefix_short}\n",
                "{prefix_long}\n",
                "### Re: duplicate prompt cleanup — gpt-5\n\n",
                "Done.\n",
                "### Re: duplicate prompt cleanup — gpt-5\n\n",
                "Done.\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt,
            prefix_short = prefix_short,
            prefix_long = prefix_long
        );
        fs::write(&doc, &before).unwrap();

        let (repaired, report) = repair_duplicate_prompt_artifacts(
            &after,
            &doc,
            DuplicatePromptRepairOptions::new("test-canonical")
                .with_before(Some(&before))
                .preserving(Some(&before)),
        )
        .unwrap();

        assert_eq!(
            report,
            DuplicatePromptRepairReport {
                response_blocks: true,
                answered_tail: false,
                post_exchange_comments: true,
                prompt_lines_against_before: true,
                live_prefix_variants: true,
            }
        );
        assert_eq!(
            repaired.matches("### Re: duplicate prompt cleanup").count(),
            1,
            "duplicate response block should be removed:\n{repaired}"
        );
        assert!(repaired.contains(&format!("❯ {prompt}\n<!-- agent:boundary:new -->")));
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get(prompt)
                .copied(),
            Some(1)
        );
        assert!(
            !repaired.contains(&format!("❯ {prompt}\n{prompt}")),
            "before-content prompt duplicate should be removed:\n{repaired}"
        );
        assert!(!repaired.contains(prefix_short));
        assert!(repaired.contains(prefix_long));
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "post-exchange duplicate prompt comment should keep the comment shell:\n{repaired}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("duplicate_prompt_artifact_repair"));
        assert!(log.contains("source=test-canonical"));
        assert!(log.contains("response_blocks=true"));
        assert!(log.contains("post_exchange_comments=true"));
        assert!(log.contains("prompt_lines_against_before=true"));
        assert!(log.contains("live_prefix_variants=true"));
    }

    #[test]
    fn commit_prompt_repair_dedupes_exact_prefixed_raw_prompt_copy() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "lucas-huang may not have the necessary packages to use the runbooks. Please add development dependencies so any programmer can use the runbooks.";
        let snapshot = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let current = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        fs::write(&doc, &current).unwrap();

        let repaired =
            repair_commit_prompt_artifacts_against_snapshot(&doc, &snapshot, &current).unwrap();

        assert!(repaired.contains(&format!("❯ {prompt}\n#spec-test-commit-push")));
        assert!(
            !repaired.contains(&format!("❯ {prompt}\n{prompt}")),
            "commit pre-stage repair should remove the raw duplicate:\n{repaired}"
        );
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get(prompt)
                .copied(),
            Some(1)
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("source=commit-pre-stage"));
        assert!(log.contains("prompt_lines_against_before=true"));
    }

    #[test]
    fn normalize_final_template_content_dedupes_direct_merge_prompt_copy() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- agent:boundary:old -->\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n",
        );
        let merged = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "❯ live prompt\n",
            "live prompt\n",
            "### Re: response — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, before).unwrap();

        let repaired = normalize_final_template_content(
            &doc,
            before,
            Some(snapshot),
            Some(before),
            merged,
            Some("### Re: response — gpt-5\n\nDone.\n"),
        )
        .unwrap();

        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get("live prompt")
                .copied(),
            Some(1)
        );
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(repaired.contains("Done."));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("ipc_prompt_duplicate_repaired"));
    }

    #[test]
    fn normalize_template_structure_repairs_live_prompt_prefix_variant_after_boundary() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert_eq!(repaired.matches("sent + re \n").count(), 0);
        assert_eq!(repaired.matches("sent + received \n").count(), 1);
    }

    #[test]
    fn normalize_template_structure_rejects_mixed_duplicate_scaffold_prompt_prefix_variant() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        fs::write(&doc, content).unwrap();

        let err = normalize_template_structure_or_fail(content, &doc).unwrap_err();

        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=mixed_duplicate_scaffold_tail"));
    }

    #[test]
    fn normalize_template_structure_keeps_prefix_variant_inside_response_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: example — gpt-5\n\n",
            "This sentence is a prefix\n",
            "This sentence is a prefix variant in assistant prose\n",
            "<!-- agent:boundary:613974fd -->\n",
            "<!-- /agent:exchange -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(repaired.contains("This sentence is a prefix\n"));
        assert!(repaired.contains("This sentence is a prefix variant in assistant prose\n"));
    }

    #[test]
    fn ipc_snapshot_dedupes_live_prompt_prefix_variant() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "<!-- /agent:exchange -->\n"
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), after, "test_ipc").unwrap();

        assert!(changed);
        assert_eq!(repaired.matches("sent + re \n").count(), 0);
        assert_eq!(repaired.matches("sent + received \n").count(), 1);
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("live_prompt_prefix_variant_repaired"));
        assert!(log.contains("ipc_snapshot_deduped"));
    }

    #[test]
    fn ipc_snapshot_scrubs_post_exchange_duplicate_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = before
            .replace(
                "<!-- /agent:exchange -->",
                "### Re: duplicate prompt cleanup — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
            )
            .replace(
                "<!-- agent:backlog -->",
                "<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n-->\n\n<!-- agent:backlog -->",
            );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), &after, "test_ipc").unwrap();

        assert!(changed);
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "IPC ack-content dedupe must scrub duplicate post-exchange prompt text:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "IPC ack-content dedupe must preserve the ordinary HTML comment shell:\n{repaired}"
        );
        assert!(repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("post_exchange_duplicate_prompt_comment_removed"));
        assert!(log.contains("duplicate_prompt_artifact_repair"));
        assert!(log.contains("post_exchange_comments=true"));
    }

    #[test]
    fn ipc_snapshot_preserves_owned_post_exchange_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "The post-exchange IPC handoff scratch comment should not be deleted. #spec-test-build-install-commit-push";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Keep this owned scratch note visible.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let after = before.replace(
        "<!-- /agent:exchange -->",
        "### Re: IPC handoff — gpt-5\n\nHandled.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
    );
        fs::write(&doc, &before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(&before), &after, "test_ipc").unwrap();

        assert!(
            !changed,
            "owned post-exchange comments should not force IPC snapshot repair"
        );
        assert!(
        repaired.contains(&format!(
            "<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nKeep this owned scratch note visible.\n-->"
        )),
        "IPC ack-content dedupe must preserve owned mixed scratch comments:\n{repaired}"
    );
    }

    #[test]
    fn ipc_snapshot_rejects_plain_markdown_duplicate_prompt_residue() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let after = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: response — gpt-5\n\n",
                "Done.\n",
                "<!-- agent:boundary:new -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );
        fs::write(&doc, &before).unwrap();

        let err = dedupe_ipc_snapshot_content(&doc, Some(&before), &after, "test_ipc").unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue"),
            "IPC snapshot dedupe must fail closed on duplicate prompt Markdown residue: {err}"
        );
    }

    #[test]
    fn normalize_user_prompts_agent_response_not_prefixed() {
        // Regression: agent response lines in content_ours (before boundary) must NOT get ❯  prefix.
        // Before the fix, apply_patches_with_overrides moves the boundary to the end of exchange,
        // so the agent's response lines ended up in the "user region" and were incorrectly prefixed.
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        // baseline: user added "My question"
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld.\nMy question\n<!-- /agent:exchange -->\n";
        // content_ours: boundary at end (after pre-patch), agent response before it
        let content = "<!-- agent:exchange patch=append -->\nOld.\nMy question\nAgent answer here.\n<!-- agent:boundary:xyz -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ My question"),
            "user question should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Agent answer"),
            "agent response should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("Agent answer here."),
            "agent response should be preserved: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_replaced_response_body_under_existing_heading_not_prefixed() {
        // Regression #repair-orphan-prefix-bug: when an orphaned response is
        // applied by replacing a placeholder body UNDER AN EXISTING `### Re:`
        // heading (e.g. a direct Edit-based patchback swapping a "Hello world"
        // placeholder for the real multi-line body), the heading line is Equal
        // in the snapshot→baseline diff. The replacement body lines are Insert
        // lines and must still be recognized as assistant-response body, not
        // user prompts — they must NOT receive the `❯ ` prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nHello world\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nReal answer line one.\nReal answer line two.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nReal answer line one.\nReal answer line two.\n<!-- agent:boundary:xyz -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ Real answer line one."),
            "replaced response body line one must NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Real answer line two."),
            "replaced response body line two must NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("Real answer line one.") && result.contains("Real answer line two."),
            "response body must be preserved verbatim: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_blank_line_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // blank line should not get prefix
        assert!(
            !result.contains("❯ \n"),
            "blank line should not be prefixed: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_heading_treated_as_agent_content() {
        // Headings in the exchange are agent response markers. A standalone heading
        // (not ❯-prefixed) is treated as agent content and does NOT get the ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\n### My heading\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### My heading\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ### My heading"),
            "heading should NOT get prefix (treated as agent content): {}",
            result
        );
        assert!(
            result.contains("### My heading"),
            "heading should be preserved: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_hash_ref_prefixed() {
        // Regression for agent-doc-bugs #vnxg: a bare hash reference like `#zj6s` inside
        // the exchange user region was being skipped by the old `starts_with('#')` guard.
        // Under Option 2, the line is user input and must receive the ❯ prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\nprior turn\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nprior turn\n#zj6s\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nprior turn\n#zj6s\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ #zj6s"),
            "hash-ref line must get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_already_prefixed_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ❯"),
            "should not double-prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Already prefixed"),
            "prefix should be preserved"
        );
    }

    #[test]
    fn normalize_user_prompts_existing_content_unchanged() {
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // Previous question already prefixed — should not double-prefix
        assert!(
            !result.contains("❯ ❯"),
            "should not double-prefix existing content: {}",
            result
        );
        // New question should get prefix
        assert!(
            result.contains("❯ New question"),
            "new line should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_keeps_inserted_assistant_question_bare() {
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ do #old
<!-- /agent:exchange -->
";
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Why did this happen?
This should stay answer prose.
<!-- /agent:exchange -->
";
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Why did this happen?
This should stay answer prose.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);

        assert!(
            result.contains("\nWhy did this happen?\nThis should stay answer prose.\n"),
            "assistant question/prose must stay bare:\n{result}"
        );
        assert!(
            !result.contains("\n❯ Why did this happen?")
                && !result.contains("\n❯ This should stay answer prose."),
            "inserted assistant response lines must not be prompt-prefixed:\n{result}"
        );
    }

    #[test]
    fn normalize_user_prompts_still_prefixes_real_followup_after_inserted_response() {
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ do #old
<!-- /agent:exchange -->
";
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Done.

do #next. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Done.

do #next. spec-test-build-install-commit-push
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);

        assert!(
            result.contains("\n❯ do #next. spec-test-build-install-commit-push\n"),
            "canonical prompt-target extraction must still prefix the follow-up:\n{result}"
        );
        assert!(
            result.contains("\nDone.\n"),
            "assistant response prose must stay bare:\n{result}"
        );
    }

    #[test]
    fn extract_normalization_targets_preserves_duplicate_lines() {
        let before = "<!-- agent:exchange patch=append -->\nQuestion?\nspec-test-build-install-commit-push\nQuestion?\nspec-test-build-install-commit-push\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\n❯ Question?\n❯ spec-test-build-install-commit-push\n❯ Question?\n❯ spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n";

        let targets = extract_normalization_targets(before, after);

        assert_eq!(
            targets,
            vec![
                "Question?".to_string(),
                "spec-test-build-install-commit-push".to_string(),
                "Question?".to_string(),
                "spec-test-build-install-commit-push".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_user_prompts_code_fence_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ```"),
            "code fence marker should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ echo hello"),
            "code fence interior should not get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Some text."),
            "regular user line should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_code_fence_interior_skipped() {
        // Multi-line code block with text before and after — only non-fence lines get prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nQuestion here.\n```rust\nlet x = 1;\nlet y = 2;\n```\nFollow-up.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nQuestion here.\n```rust\nlet x = 1;\nlet y = 2;\n```\nFollow-up.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Question here."),
            "text before fence should get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Follow-up."),
            "text after fence should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ let x"),
            "fence interior should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ let y"),
            "fence interior should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ```"),
            "fence marker should not get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_tilde_fence_interior_skipped() {
        // ~~~ fences must be tracked the same as ``` fences.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nBefore.\n~~~sh\necho hello\n~~~\nAfter.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nBefore.\n~~~sh\necho hello\n~~~\nAfter.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Before."),
            "text before tilde fence should get prefix: {result}"
        );
        assert!(
            result.contains("❯ After."),
            "text after tilde fence should get prefix: {result}"
        );
        assert!(
            !result.contains("❯ echo hello"),
            "tilde fence interior should not get prefix: {result}"
        );
        assert!(
            !result.contains("❯ ~~~"),
            "tilde fence marker should not get prefix: {result}"
        );
    }

    #[test]
    fn normalize_user_prompts_quoted_string_prefixed() {
        // Option 2 invariant: a quoted string the user typed is still user input,
        // so it gets the ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ \"Merge conflict"),
            "quoted user line should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_patch_content_applies_prefix_to_matching_lines() {
        let patch_content =
            "transferred line 1\ntransferred line 2\n### Re: Response\nAgent answer\n";
        let prefix_lines = vec![
            "transferred line 1".to_string(),
            "transferred line 2".to_string(),
        ];
        let result = normalize_patch_content(patch_content, &prefix_lines);
        let expected =
            "❯ transferred line 1\n❯ transferred line 2\n### Re: Response\nAgent answer\n";
        assert_eq!(
            result, expected,
            "prefix lines should get ❯  in patch content"
        );
    }

    #[test]
    fn normalize_patch_content_idempotent_already_prefixed() {
        let patch_content = "❯ already prefixed\nnot prefixed\n";
        let prefix_lines = vec!["already prefixed".to_string(), "not prefixed".to_string()];
        let result = normalize_patch_content(patch_content, &prefix_lines);
        let expected = "❯ already prefixed\n❯ not prefixed\n";
        assert_eq!(
            result, expected,
            "already-prefixed lines should not get double prefix"
        );
    }

    #[test]
    fn normalize_patch_content_empty_prefix_lines_passthrough() {
        let patch_content = "some line\nanother line\n";
        let result = normalize_patch_content(patch_content, &[]);
        assert_eq!(
            result, patch_content,
            "empty prefix_lines should leave content unchanged"
        );
    }

    #[test]
    fn normalize_patch_content_non_matching_lines_unchanged() {
        let patch_content = "agent response line\n### heading\n";
        let prefix_lines = vec!["user line".to_string()];
        let result = normalize_patch_content(patch_content, &prefix_lines);
        assert_eq!(
            result, patch_content,
            "non-matching lines should pass through unchanged"
        );
    }

    #[test]
    fn normalize_patch_content_counts_duplicate_targets() {
        let patch_content = "spec-test-build-install-commit-push\nspec-test-build-install-commit-push\nspec-test-build-install-commit-push\n";
        let prefix_lines = vec![
            "spec-test-build-install-commit-push".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        let result = normalize_patch_content(patch_content, &prefix_lines);

        assert_eq!(
            result,
            "❯ spec-test-build-install-commit-push\n❯ spec-test-build-install-commit-push\nspec-test-build-install-commit-push\n"
        );
    }

    #[test]
    fn normalize_prefix_lines_skipped_for_replace_mode_components() {
        // Regression: normalize_patch_content was applied to ALL patches including agent:pending.
        // When a line from the exchange user_added set also appeared in a pending patch, it would
        // incorrectly receive the ❯  prefix. The fix gates normalization on is_append_mode_component.
        let pending_content =
            "- [ ] Build Gutenberg replacement HTML for home page\n- [ ] Update page content\n";
        let prefix_lines = vec!["- [ ] Build Gutenberg replacement HTML for home page".to_string()];
        // Simulate the guard: only apply normalize_patch_content for exchange (append-mode) components.
        // For pending (replace-mode), content must pass through unchanged.
        let is_pending = !is_append_mode_component("pending");
        assert!(is_pending, "pending should not be an append-mode component");
        // If the guard is respected, pending content is not normalized.
        let result = if is_append_mode_component("pending") {
            normalize_patch_content(pending_content, &prefix_lines)
        } else {
            pending_content.to_string()
        };
        assert_eq!(
            result, pending_content,
            "agent:pending content must NOT receive ❯  prefix"
        );
        assert!(
            !result.contains("❯ "),
            "no ❯  prefix should appear in pending patches"
        );
    }

    #[test]
    fn normalize_user_prompts_no_exchange_passthrough() {
        let content = "No exchange here.\n";
        let baseline = "No exchange here.\n";
        let snapshot = "";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert_eq!(
            result, content,
            "document without exchange should pass through unchanged"
        );
    }

    #[test]
    fn normalize_user_prompts_restores_prefix_lost_in_file() {
        // Regression: snapshot has ❯ do but the editor file (baseline) has do without prefix.
        // This happens when the IPC normalization fails to update the editor file.
        // The binary must restore ❯  so the snapshot stays correct and the
        // next IPC write carries normalize_prefix_lines with the correct prefix target.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ done\n❯ do\n- [ ] task\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- agent:boundary:abc123:doc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ do"),
            "❯  prefix must be restored when snapshot had it but file lost it: {}",
            result
        );
        assert!(
            !result.contains("\ndo\n"),
            "bare do line must not remain without prefix: {}",
            result
        );
        // ❯ done must not be double-prefixed
        assert!(!result.contains("❯ ❯"), "no double-prefix: {}", result);
    }

    #[test]
    fn normalize_user_prompts_heading_replacement_does_not_swallow_next_prompt() {
        // Regression: commit-time `(HEAD)` churn replaces an existing response heading,
        // which shows up as Delete+Insert in snapshot→baseline. That replacement must
        // not reopen an agent block and suppress ❯ prefixing for the following user line.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4\nAgent answer.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4 (HEAD)\nAgent answer.\nfix #vedj. add spec + tests. build + install for local testing\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4 (HEAD)\nAgent answer.\nfix #vedj. add spec + tests. build + install for local testing\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("### Re: topic — gpt-5.4 (HEAD)"),
            "replacement heading should be preserved: {}",
            result
        );
        assert!(
            result.contains("Agent answer."),
            "existing agent body should be preserved: {}",
            result
        );
        assert!(
            result.contains("❯ fix #vedj. add spec + tests. build + install for local testing"),
            "new user prompt should get prefix despite heading replacement: {}",
            result
        );
        assert!(
            !result.contains("❯ Agent answer."),
            "existing agent body should not be prefixed: {}",
            result
        );
        assert!(
            !result.contains("❯ ### Re: topic"),
            "replacement heading should not be prefixed: {}",
            result
        );
    }

    // ── agent-response-block tracking ────────────────────────────────────────

    #[test]
    fn normalize_user_prompts_agent_table_rows_not_prefixed() {
        // Core bug: stale snapshot causes agent response table rows (inside ### Re: blocks)
        // to appear as Insert lines and incorrectly receive ❯ prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Question\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Question\n### Re: analysis — opus-4-6\n| model | score |\n|-------|-------|\n| gpt-4 | 85.0 |\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Question\n### Re: analysis — opus-4-6\n| model | score |\n|-------|-------|\n| gpt-4 | 85.0 |\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ |"),
            "table rows inside agent response should NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("| model | score |"),
            "table content should be preserved: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_agent_subheadings_not_prefixed() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n### Re: topic\nSome text.\n#### Details\nMore text.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### Re: topic\nSome text.\n#### Details\nMore text.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ "),
            "no lines should get prefix — all are agent content: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_user_text_after_equal_heading() {
        // Heading is Equal (in snapshot), user adds text after it. User text gets ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\nNew user input\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\nNew user input\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ New user input"),
            "user text after Equal heading should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_agent_block_ends_at_prompt() {
        // Agent block (Insert heading) ends when ❯-prefixed line appears.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n### Re: answer\nAgent text.\n❯ New question\nFollow-up text.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### Re: answer\nAgent text.\n❯ New question\nFollow-up text.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ Agent text"),
            "agent text should NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ New question"),
            "already-prefixed line should be preserved: {}",
            result
        );
        assert!(
            result.contains("❯ Follow-up text."),
            "user text after ❯ should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_heading_in_fence_not_agent_block() {
        // A heading inside a code fence is code, not an agent response marker.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nBefore.\n```md\n### Not a real heading\nSome code.\n```\nAfter.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nBefore.\n```md\n### Not a real heading\nSome code.\n```\nAfter.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Before."),
            "text before fence should get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ After."),
            "text after fence should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "heading inside fence should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Some code"),
            "code inside fence should not get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_multiline_prompt_after_stale_response_gets_prefix() {
        // Regression for #pfxstrip2: when a stale snapshot makes the previous
        // assistant response appear as inserted content, the normalizer enters
        // agent-block mode. A blank-separated fresh prompt run after that
        // response is still user input, and every nonblank prompt line needs
        // the prompt prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Previous prompt\n<!-- /agent:exchange -->\n";
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Previous prompt\n",
            "### Re: previous — gpt-5\n",
            "Implemented and verified.\n",
            "\n",
            "Please increment version to v0.1.1. Release to github. Create a plan for rollout.\n",
            "Miguel will be integrating the demo into the partner workspace.\n",
            "\n",
            "Please rename the gh repo ClaudeScore/buildparty-investor-demo to the final name.\n",
            "Also, please draft slack instructions for robert-ross and miguel-mendez.\n",
            "\n",
            "spec-test-news-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = baseline.replace(
            "<!-- /agent:exchange -->",
            "<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->",
        );

        let result = normalize_user_prompts_in_exchange(&content, baseline, snapshot);

        for expected in [
            "❯ Please increment version to v0.1.1. Release to github. Create a plan for rollout.",
            "❯ Miguel will be integrating the demo into the partner workspace.",
            "❯ Please rename the gh repo ClaudeScore/buildparty-investor-demo to the final name.",
            "❯ Also, please draft slack instructions for robert-ross and miguel-mendez.",
            "❯ spec-test-news-commit-push",
        ] {
            assert!(
                result.contains(expected),
                "missing expected prefixed prompt line {expected:?}:\n{result}"
            );
        }
        assert!(
            !result.contains("❯ Implemented and verified."),
            "stale assistant response body must stay unprefixed:\n{result}"
        );
    }

    // ── safety rail: normalize_user_prompts_in_exchange_safe ────────────────

    #[test]
    fn normalize_safe_passes_through_under_threshold() {
        // Small diff (1 user-added line) — should behave exactly like the pure function.
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();

        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("❯ Hello"),
            "under threshold, ❯ prefix should still be applied: {result}"
        );
    }

    #[test]
    fn normalize_safe_preserves_unprefixed_agent_lines_from_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "run follow-up\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "run follow-up\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("\nDone:\n- build passed\n"),
            "committed agent response lines from HEAD must stay unprefixed:\n{result}"
        );
        assert!(
            result.contains("\n❯ run follow-up\n"),
            "new user prompt should still be prefixed:\n{result}"
        );
    }

    #[test]
    fn normalize_safe_preserves_prior_response_tail_before_new_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "do [#pfxleak3]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "do [#pfxleak3]. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains(
                "\n- All 506 assertions pass.\nCommitted + pushed buildparty-investor-demo and session-share.\n❯ do [#pfxleak3]. spec-test-build-install-commit-push\n"
            ),
            "prior response tail must stay bare and only the new prompt may be prefixed:\n{result}"
        );
        assert!(
            !result.contains("\n❯ - All 506 assertions pass.\n")
                && !result.contains(
                    "\n❯ Committed + pushed buildparty-investor-demo and session-share.\n"
                ),
            "assistant tail lines from HEAD must not gain prompt prefixes:\n{result}"
        );
    }

    #[test]
    fn normalize_safe_preserves_prefixed_user_lines_from_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please increment version to v0.1.1.\n",
            "❯ Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = head;
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Please increment version to v0.1.1.\n",
            "Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Please increment version to v0.1.1.\n",
            "Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("❯ Please increment version to v0.1.1."),
            "HEAD-prefixed first prompt line must regain its prefix:\n{result}"
        );
        assert!(
            result.contains("❯ Miguel will be integrating the demo."),
            "HEAD-prefixed continuation line must regain its prefix:\n{result}"
        );
        assert!(
            !result.contains("\nPlease increment version to v0.1.1.\n"),
            "bare first prompt line must not remain:\n{result}"
        );
    }

    #[test]
    fn normalize_safe_bails_over_threshold() {
        // Construct a baseline with >50 unique "user-added" lines relative to the snapshot.
        // The safety rail should refuse to apply ❯ prefix and return content unchanged.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();
        let file = root.join("doc.md");
        std::fs::write(&file, "initial\n").unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();
        let head_before = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;

        let mut baseline_lines = String::new();
        let mut content_lines = String::new();
        for i in 0..60 {
            baseline_lines.push_str(&format!("user line {i}\n"));
            content_lines.push_str(&format!("user line {i}\n"));
        }
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = format!(
            "<!-- agent:exchange patch=append -->\n{baseline_lines}<!-- /agent:exchange -->\n"
        );
        let content = format!(
            "<!-- agent:exchange patch=append -->\n{content_lines}<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n"
        );

        let result = normalize_user_prompts_in_exchange_safe(&content, &baseline, snapshot, &file);
        // No ❯ prefix should be applied — content should be returned unchanged.
        assert_eq!(
            result, content,
            "over threshold, content should pass through unchanged"
        );
        assert!(
            !result.contains("❯ user line"),
            "no ❯ prefix should be applied when threshold exceeded"
        );
        let head_after = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(
            head_after, head_before,
            "normalization overrun must not force-commit the working tree"
        );
    }

    #[test]
    fn normalize_safe_threshold_exact_boundary() {
        // Exactly 50 lines — at threshold, still applies prefix (strictly greater-than check).
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();

        let mut lines = String::new();
        for i in 0..50 {
            lines.push_str(&format!("line {i}\n"));
        }
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            format!("<!-- agent:exchange patch=append -->\n{lines}<!-- /agent:exchange -->\n");
        let content = format!(
            "<!-- agent:exchange patch=append -->\n{lines}<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n"
        );

        let result = normalize_user_prompts_in_exchange_safe(&content, &baseline, snapshot, &file);
        // At exactly 50, prefix should be applied (> is strict).
        assert!(
            result.contains("❯ line 0"),
            "at threshold, first line should get prefix: {result}"
        );
        assert!(
            result.contains("❯ line 49"),
            "at threshold, last line should get prefix: {result}"
        );
    }

    // --- exchange shrink guard tests ---

    #[test]
    fn shrink_guard_blocks_truncation() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        let long_exchange = "a]".repeat(250); // 500 bytes
        let old = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            long_exchange
        );
        let new = "<!-- agent:exchange -->\n.\n<!-- /agent:exchange -->\n";

        let result = check_exchange_shrink_guard(&old, new, &doc);
        assert!(
            result.is_err(),
            "shrink guard should block truncation from 500 to ~1 byte"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("shrink"), "error should mention shrink: {msg}");
    }

    #[test]
    fn shrink_guard_allows_normal_write() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        let old_text = "x".repeat(200);
        let new_text = "y".repeat(100); // 50% — well above 10%
        let old = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            old_text
        );
        let new = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            new_text
        );

        let result = check_exchange_shrink_guard(&old, &new, &doc);
        assert!(
            result.is_ok(),
            "shrink guard should allow 50% reduction: {:?}",
            result.err()
        );
    }

    #[test]
    fn shrink_guard_skips_small_exchange() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        // Old exchange is only 50 bytes — below SHRINK_GUARD_MIN_BYTES
        let old =
            "<!-- agent:exchange -->\nSmall content here, not much.\n<!-- /agent:exchange -->\n";
        let new = "<!-- agent:exchange -->\n.\n<!-- /agent:exchange -->\n";

        let result = check_exchange_shrink_guard(old, new, &doc);
        assert!(
            result.is_ok(),
            "shrink guard should skip small exchanges: {:?}",
            result.err()
        );
    }

    #[test]
    fn shrink_guard_passes_no_exchange() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        // No exchange component at all
        let old = "# Just a heading\nSome content.\n";
        let new = "# Just a heading\n.\n";

        let result = check_exchange_shrink_guard(old, new, &doc);
        assert!(
            result.is_ok(),
            "shrink guard should pass when no exchange component exists"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_large_snapshot_only_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let stale_exchange = "duplicated response\n".repeat(20);
        let snapshot = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{}<!-- /agent:exchange -->\n",
            stale_exchange
        );
        let current = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\nclean\n<!-- /agent:exchange -->\n";

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write");

        let message = result
            .expect_err("stale larger snapshot must fail closed")
            .to_string();
        assert!(
            message.contains("agent-doc reset --from-current"),
            "recovery guidance should name deterministic sidecar reset: {message}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_allows_small_size_delta() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = "a".repeat(1000);
        let current = "b".repeat(940);

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "stream write");

        assert!(
            result.is_ok(),
            "minor snapshot/file size drift should not block writes"
        );
    }

    #[test]
    fn extract_exchange_content_len_works() {
        let doc = "<!-- agent:exchange -->\nHello world\n<!-- /agent:exchange -->\n";
        assert_eq!(extract_exchange_content_len(doc), "Hello world".len());

        let empty = "<!-- agent:exchange -->\n\n<!-- /agent:exchange -->\n";
        assert_eq!(extract_exchange_content_len(empty), 0);

        let no_exchange = "Just text.";
        assert_eq!(extract_exchange_content_len(no_exchange), 0);
    }

    #[test]
    fn splice_pending_replaces_content_when_both_have_pending() {
        // target has stale/empty pending (built from pre-mutation baseline)
        let target = "\
<!-- agent:exchange -->
response content
<!-- /agent:exchange -->
<!-- agent:pending -->
- [ ] [#aaaa] old item
<!-- /agent:pending -->
";
        // source is the current on-disk file with a pending-done mutation applied
        let source = "\
<!-- agent:exchange -->
original content
<!-- /agent:exchange -->
<!-- agent:pending -->
- [x] [#aaaa] old item
<!-- /agent:pending -->
";
        let result = splice_pending_component(target, source);
        // exchange content from target is preserved
        assert!(
            result.contains("response content"),
            "exchange content should come from target"
        );
        // pending content from source (with [x]) is used
        assert!(
            result.contains("- [x] [#aaaa] old item"),
            "pending done state should come from source"
        );
        // old pending from target is gone
        assert!(
            !result.contains("- [ ] [#aaaa] old item"),
            "stale open pending should be replaced"
        );
    }

    #[test]
    fn splice_pending_noop_when_source_has_no_pending() {
        let target = "\
<!-- agent:exchange -->
response
<!-- /agent:exchange -->
<!-- agent:pending -->
- [ ] [#bbbb] task
<!-- /agent:pending -->
";
        let source = "\
<!-- agent:exchange -->
original
<!-- /agent:exchange -->
";
        let result = splice_pending_component(target, source);
        assert_eq!(
            result, target,
            "target should be returned unchanged when source has no pending"
        );
    }

    #[test]
    fn splice_pending_warns_when_target_missing_pending() {
        // target has no pending component; source does — should return target unchanged
        let target = "\
<!-- agent:exchange -->
response
<!-- /agent:exchange -->
";
        let source = "\
<!-- agent:exchange -->
original
<!-- /agent:exchange -->
<!-- agent:pending -->
- [x] [#cccc] done item
<!-- /agent:pending -->
";
        let result = splice_pending_component(target, source);
        assert_eq!(
            result, target,
            "target should be returned unchanged when target has no pending"
        );
    }
