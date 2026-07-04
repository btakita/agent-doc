# agent-loop Memory

## agent-doc Recovery & Dogfooding

- [Editor-less force-disk resurrects operator-deleted queue items](feedback_forcedisk_editorless_scramble_reset_fromcurrent_repair.md) — repair via edit + git commit + reset --from-current --force-disk; halt no-op churn with queue marker start→stop.

- [Stranded controller handoff → finalize timeout → IPC snapshot drift](feedback_stranded_handoff_socket_causes_ipc_snapshot_drift.md) — self-promotion fix (#stuckhandoffselfheal).
- [Concurrent sessions force-push the shared superproject](feedback_concurrent_sessions_force_push_shared_superproject.md) — collided session verifies on origin then STOPS.
- [Concurrent agent-doc refactor churns working tree + `make clear`](feedback_concurrent_agent_doc_refactor_session_build_coordination.md) — don't patch their WIP; commit selectively, build from clean worktree.
- [JB "Run Agent Doc" spawns live route-owned pane processing prompt](feedback_jb_run_agentdoc_route_owned_pane_processing_prompt_collision.md) — check owner via admin inspect + tmux capture before writing; `write` leaks to disk on nack.
- [bugs2.done.md is the ARCHIVE, not the session](reference_agent_doc_bugs2_done_is_archive_not_session.md) — live doc is tasks/agent-doc/agent-doc-bugs2.md.
- [agent:boundary repeated ONLY in worktree from ipc_visible_repair](feedback_boundary_accum_worktree_only_ipc_repair_checkout_head.md) — check HEAD boundary count first; #boundaryaccum1.
- [Hung --pending-add recovers via `agent-doc commit`](feedback_hung_pending_add_recover_via_agent_doc_commit.md) — commit absorbs on-disk pending write.
- [Stuck response_captured cycle → mv state/cycles/<dochash>.json aside](feedback_stuck_response_captured_cycle_mv_state_cycles_json.md) — then preflight clean, re-finalize --force-disk.
- [Stale-binary CRDT deadlock (already_applied + buffer-differs)](feedback_stale_binary_crdt_deadlock_direct_write_git_commit_reset.md) — direct Write + commit + reset --from-current; preserve operator edits.
- [stale_install FALSE alarm when concurrent WIP won't compile](feedback_stale_install_false_alarm_uncompilable_concurrent_wip.md) — binary actually fresh; verify buffer==disk, direct-write escape; route-owned child is your own tree.
- [JB SIGABRT = stale cdylib panic=abort hitting fixed panic](feedback_jb_sigabrt_stale_cdylib_panic_abort_coredump_diagnose.md) — `coredumpctl` for `panicked at`; root=stale_install; fix=`make install`. #cdylibpanic drops panic=abort.
- [Preflight "crash" = corrupt HEAD from no_liveness synthetic reap](feedback_synthetic_noliveness_reap_frontmatter_drop_suprecyclespin.md) — revert-to-scaffold + reset --from-current; #fmdrop self-heals frontmatter.
- [Repeated no_changes during drift = wedged IPC clobbering buffer](feedback_repeated_no_changes_means_wedged_ipc_clobbering_buffer_make_install.md) — edits not landing; make install + recycle + end turn.
- [force-disk COMMITTED but printed false closeout → check HEAD, STOP](feedback_forcedisk_committed_but_false_closeout_error_stop_retrying.md) — git authoritative; don't reset --from-current.
- [JB restart/reopen DON'T fix stale-cdylib corruption (7x dup)](feedback_jb_restart_reopen_dont_fix_stale_cdylib_corruption.md) — reload same stale cdylib; only `make install`/`force-disk`.
- [send_failed with NO IDE process = stale endpoint → force-disk](feedback_send_failed_no_ide_running_forcedisk_not_restart.md) — check `ps` first; STOP on concurrent HEAD advance.
- [suprecyclespin: ending turn does NOT persist response → force-disk](feedback_suprecyclespin_end_turn_does_not_persist_response_forcedisk.md) — checkout HEAD→reset --from-current→force-disk.
- [Complete a crashed agent-doc run](feedback_complete_crashed_agent_doc_run_recovery_sequence.md) — ship submodule fix + recover via preflight reap.
- [--pending-add embedded [#tag] sets the id → collides](feedback_pending_add_bracket_tag_sets_id_collides.md) — use `[#cleanid] text` alone or omit.
- [snapshot≠HEAD layout guard on queue-only edit](feedback_snapshot_vs_head_guard_stash_reset_to_head.md) — stash → reset --preserve-session → stash pop.
- [Do agent-doable release sub-steps on operator-verify items](feedback_operator_verify_do_agentdoable_release_substeps.md) — drainable=0 ≠ no work.
- [Check git before deferring a re-pinned [clean-session] item](feedback_check_git_before_deferring_repinned_clean_session.md) — `git log --grep <id>` first.
- [Stale-binary supervisor over-consumes queue heads](feedback_stale_supervisor_overconsumes_redo_struck_items.md) — re-do struck-but-undone; admin recycle.
- [Parent-supervisor stale CRDT: reset --from-current (no preserve)](feedback_parent_supervisor_stale_crdt_reset_nopreserve_then_restart.md) — drain inline per cycle.
- [queue-consume race → finalize --force-disk](feedback_queue_consume_race_forcedisk_recovery.md) — checkout HEAD then force-disk, don't retry socket.
- [start "timed out waiting for project controller" = corrupt 0-byte controller-state.json](feedback_corrupt_controller_state_blocks_start_move_aside.md) — move controller-state+socks aside → relaunch; DON'T make install.
- [Degraded IPC blocks granular --pending-add/-edit (not exchange)](feedback_degraded_ipc_blocks_granular_pending_writes.md) — response safe in HEAD.
- **Stale-binary supervisor** (compare vs INSTALLED binary; verify via `admin inspect` not ps lstart): [live CRDT bugs](feedback_stale_binary_supervisor_causes_live_crdt_edit_bugs.md), [recycle deadlock (FIXED 0.34.52)](feedback_recycle_deadlock_open_cycle_end_turn_dont_poll.md), [restart-supervisor --all](project_agent_doc_stale_supervisors_not_recycling_need_force_restart.md), [verify via admin inspect](feedback_verify_recycle_via_admin_inspect_freshness_not_lstart.md), [fleet recycle after install](feedback_fleet_wide_recycle_after_install_procedure.md), [no mid-session install](feedback_no_mid_session_install_against_live_supervisor.md), [live_prompt_drift=recycle NOT staleness (FIXED)](feedback_live_prompt_drift_wedge_is_midcycle_recycle_not_staleness.md).
- **no_ack / dead-editor / degraded-IPC**: [dead JB listener→STOP](feedback_dead_jb_listener_send_failed_blocks_finalize_only.md), [write-socket→write --commit](feedback_no_ack_write_socket_finalize_fails_writecommit_recovers.md), [file_ipc DROPS→recover from patch JSON](feedback_noack_file_ipc_patch_drops_content_recover_from_patch_json.md), [granular scrambles HEAD](feedback_dead_editor_granular_write_scrambles_head_forcedisk_from_clean_commit.md), [RETRY duplicates→write ONCE](feedback_degraded_ipc_write_retry_duplicates_response.md), [hung bg write→stale buffer](feedback_hung_agent_doc_write_stale_live_buffer.md), [lost response force-disk](feedback_ipc_crdt_response_loss_forcedisk_recovery.md), [wrong pane-binding + fleet recycle](feedback_finalize_from_wrong_pane_binding_during_fleet_recycle.md).
- [Operator edited response in live JB buffer before commit → reset --from-current + commit](feedback_operator_edited_response_live_buffer_reset_fromcurrent_commit.md) — live_prompt_drift double-append; NOT force-disk; wait for no_unsaved_operator_edits:true.
- **reset --from-current / checkout HEAD** (verify prompt in HEAD first): [checkout HEAD discards uncommitted prompt](feedback_checkout_head_discards_uncommitted_operator_prompt.md), [first-cycle ipc_dedupe](feedback_firstcycle_ipc_dedupe_no_head_prompt_recovery.md), [commit REGRESSES HEAD](feedback_agent_doc_commit_regresses_head_to_smaller_snapshot.md), [compaction stale-snapshot NOT commit](feedback_compaction_stale_snapshot_reset_not_commit.md), [stale capture blocks commit](feedback_agent_doc_stale_capture_blocks_commit.md), [post-commit drift→checkout HEAD](feedback_post_commit_worktree_drift_checkout_head_recovery.md), [supervisor re-commits corrupted CRDT](feedback_supervisor_crdt_replay_reset_from_current.md).
- **force-disk marker/wedge**: [marker corruption retry-on-clean](feedback_forcedisk_crdt_marker_corruption_retry_on_clean.md), [streaming-socket wedge](feedback_supervisor_streaming_socket_wedge_forcedisk.md).
- **Controller/handoff self-heals (don't pkill)**: [wedged preparing controller M1b 45s self-reap](feedback_ipc_drift_wedged_preparing_controllers.md), [supervisor self-race exit-75](feedback_agent_doc_supervisor_self_race_finalize_block.md).
- **Queue quirks**: [multiline heads un-strikable](feedback_qheadresidue_multiline_codeblock_heads_unstrikable.md), [go-marker churn](feedback_go_marker_churn_pause_resets_remove_go_set_stop.md), [go defers undrainable (#goqueuestall)](feedback_go_queue_defer_undrainable_tags.md), [preflight consumes queued question](feedback_preflight_repair_consumes_queue_question_no_answer.md), [queue:stop+preset strike drift](feedback_efs_queue_stop_preset_strike_drift.md), [dual patch:status+exchange](feedback_agent_doc_dual_patch_status_exchange_interleave.md), [operator-drive churn](feedback_operator_drive_items_on_go_queue_churn.md), [halt via admin queue pause](feedback_halt_flooding_queue_admin_pause.md).
- **Misc**: [backticks corrupt --pending-add](feedback_agent_doc_pending_flag_backtick_substitution.md), [lintfence→--lint off](feedback_lintfence_fenced_component_markers_break_lint_and_parser.md), [session-check marker anomaly→STOP](feedback_session_check_marker_error_contradicts_preflight.md), [isolated worktree (NO install/push)](feedback_agent_doc_isolated_worktree_supervisor_critical.md), [admin detect pane contamination](feedback_check_admin_detect_for_pane_contamination_first.md), [deep refactors→fresh cycle](feedback_deep_supervisor_refactor_fresh_focused_cycle.md), [non-exchange edits bypass wedge](feedback_nonexchange_mutation_commits_when_exchange_wedges_on_visible_guard.md), [AI recommended→confirm before push](feedback_auto_recommendations_autodrain_confirm_before_implement_push.md).

## agent-doc Project Intent

- [Patchboard markets FIRST; agent-doc is long-term play](project_product_sequencing_patchboard_first_agentdoc_later.md) — extraction = deliberate realtime-migration paydown; #spb1–9 planned, unstarted.
- [Realtime merge expertise = marketable alpha](project_realtime_merge_experience_marketable_alpha.md) — human+agent replica authority/provenance moat; demand map in tasks/session-findings-2026-07-04.md.
- [IPC root-cause audit + #stale-plugin-detect shipped](project_ipc_audit_and_stale_plugin_detect.md) — 5 causes A-E queued; detect committed 9b38d017; patchboard NOT an IPC failure.
- [provauth1/2/3 status — 3b blocked on crdtauth6](project_provauth_series_status.md) — 1/2 DONE, 3 binary-half shipped 0.34.57.
- [Per-cell CRDT merge isolation is the deeper fix](project_agent_doc_percell_crdt_merge_intent.md) — #qcellmerge1. [[feedback_stop_cutting_agent_doc_releases]]
- [Supervisor-in-process migration (08b) CODE-COMPLETE](project_agent_doc_supervisor_migration_intent.md) — rollback flags deleted (BREAKING).
- [#fintol2/#pcwc fight the carry-forward invariant](project_fintol_pcwc_carry_forward_invariant.md) — don't re-wire without operator go + Phase-0 re-baseline.
- [agent-doc decomposition progress](project_agent_doc_module_decomposition.md) — preflight/sync/session_check/git/start/template left.
- [Split functionality + tests together](feedback_module_split_functionality_with_tests.md)

## agent-doc Release Conventions

- [agent-doc 0.34.x publish reality](reference_agent_doc_034x_publish_reality.md) — local cargo install + lib-install ONLY.
- [STOP cutting agent-doc releases (2026-06-20)](feedback_stop_cutting_agent_doc_releases.md) — no ceremony until per-cell CRDT fixed; confirm first.
- [CPC+Supervisor auto-install drives install+recycle](feedback_supervisor_autoinstall_drives_install_not_manual.md) — trust preflight stale_install.
- [FlowCore token budget gate](reference_flowcore_token_budget_gate.md) — guard_/proof=/reason= breaks flowcore_hot_path_guard; bump budget.
- [Document is source of truth](feedback_agent_doc_document_is_source_of_truth.md) — go-mode queue IS authorization.
- **Unpublished tags**: move tag forward vs new version. **pyproject.toml sync** + **VERSIONS.md** before release ([feedback](feedback_versions_md_before_release.md)).
- **Publish all targets**: crates.io + GitHub Release; PyPI auto via CI. [[reference_pypi_publish_via_ci]]
- **cargo install + cargo build --release after ANY source edit** ([feedback](feedback_cargo_build_release_after_route_edit.md), [feedback](feedback_cargo_install_on_release.md)).
- **Ship core/FFI to live JB plugin via `agent-doc lib-install`** (cdylib hot-reloads by mtime) ([feedback](feedback_jb_plugin_cdylib_hot_reload.md)).
- **JB plugin version bump per .kt edit** ([feedback](feedback_jb_plugin_version_bump_per_edit.md)); **build on every version bump** ([feedback](feedback_build_on_version_bump.md)); **manual testing gate before publish** ([feedback](feedback_testing_before_release.md)).

## lazily / Releases

- [corky linkedin publish format](reference_corky_linkedin_publish_format.md) — mail/social/*-linkedin.md w/ frontmatter; escape little-text chars (FIXED e147ef9).
- [lazily release targets](reference_lazily_release_targets.md) — rs→crates.io v0.12.3; py→PyPI 0.10.2 (0.11.0 pending 2FA); js→npm; spec→Pages; zig→GitHub.
- [lazily publish mechanics + coverage generator](reference_lazily_publish_and_coverage_generator.md) — js npm needs 2FA (OIDC trusted-publisher unconfigured→404); kt→GitHub Packages on v* tag (vendor fixtures for offline CI); feature matrix single-source via coverage.json + sync-coverage.mjs + `make coverage-check`.
- [Queue `go` overrides queue:stop → churn](feedback_queue_go_marker_overrides_stop_churn.md) — remove `go` AND set stop.

## Decision / Communication

- [AskUserQuestion no-option + note → confirm actual choice](feedback_askuserquestion_no_option_confirm_actual_choice.md) — note ≠ endorsement.
- [Explain decisions plainly, not jargon](feedback_explain_decisions_plainly_not_jargon.md) — plain-English + recommended default.
- [No sycophancy](feedback_no_sycophancy.md) — no superlatives/promo.
- [Don't oversimplify when asked "simpler"](feedback_no_oversimplify.md) — keep terse; elaborate the action.
- [Brian's Communication Priorities](user_communication_priorities.md) — bias-stripped prioritization.
- [Robert Ross — engagement closeout](project_robert_ross_engagement_status.md) — Brian fired 2026-05-27.

## equityfundingsource

- [Legacy `applications` table collision](project_efs_legacy_applications_table.md) — apply4 uses `full_applications`; don't reuse.
- [EFS prod D1 queryable (#cftk RESOLVED)](reference_efs_d1_token_working.md) — passage token is D1-Edit scoped.

## tsift

- [opencode-tsift npm publish = OIDC (DONE)](reference_opencode_tsift_npm_publish.md) — btakita npm 2FA browser/passkey.
- [crates.io publish list completeness](reference_tsift_release_publish_list.md) — all deps in both release.yml lists; lockstep bump.
- [crates.io OIDC: 23 configured; 2 new need registration](reference_tsift_cratesio_trusted_publishing.md) — v0.1.73 fully published (25 crates).
- [SKILL.md is primary source](feedback_tsift_skill_primary_source.md) — owns full API; AGENTS.md lean; detail in runbooks.

## Security

- **No cleartext secrets** — livestreams; `pass`/`passage` subshell ([feedback](feedback_no_cleartext_secrets.md)).
- **[REDACTED_PRODUCT]: passage [REDACTED_PRODUCT]/OPENROUTER_API_KEY**, not btak/ ([feedback](feedback_[REDACTED_PRODUCT]_openrouter_key.md)).
- **Redact nested JSON** — `jq 'map_values(type)'` for structure only ([feedback](feedback_nested_json_redaction.md)).

## Submodule / Git Ops

- [git rebase cwd/submodule hazard](feedback_git_rebase_cwd_submodule_hazard.md) — cd to superproject root before rebase/push.
- **Recursive submodule commits** inside-out; **check working tree + diff** not just log ([f](feedback_recursive_submodule_commits.md), [f](feedback_check_working_tree.md)).
- [RTK masks `make check` exit codes](feedback_rtk_masks_make_check_exit.md) — capture `$?` + grep `result: FAILED`.

## JetBrains Plugins (general)

- **ActionPromoter breaks intentions** in .md — use self-disabling `update()`.
- **python-intentions verifier false positives** (bare IU lacks PythonCore) — informational; src ~/work/btakita/python-intentions.
- **Build both zips** (unsigned+signed); full uninstall only for structural class changes.

## Misc Conventions

- **AGENTS.md canonical** — symlink CLAUDE.md → it ([feedback](feedback_agents_md_canonical.md)).
- **corky: always `--features diarize`** ([f](feedback_corky_diarize_feature.md)); **bun for briantakita.me** ([f](feedback_bun_not_pnpm.md)); **file:// browser paths** ([f](feedback_file_protocol_paths.md)).
- **Writing voice**: `...` not spaced em-dash; avoid delve/leverage/hedging ([feedback](feedback_writing_voice.md)).
- **Verify live system after fix**, not just tests ([feedback](feedback_verify_live_system.md)).
- **"do" = execute ALL now** ([f](feedback_do_means_execute_all.md)); approval ≠ cut corners ([f](feedback_approval_not_skip_processes.md)); precommit testing mandatory ([f](feedback_precommit_testing.md)); no confirm for obvious iteration ([f](feedback_no_confirmation_iterate.md)).
- **No hallucinating** — "I don't know" ([f](feedback_no_hallucinate_uncertainty.md)); don't call broken "correct" ([f](feedback_no_hallucinate_correct_errors.md)); verify proxy before killing ([f](feedback_verify_proxy_before_kill.md)).
- **Distribution-aware reorg** ([f](feedback_distribution_aware_reorg.md)); [cross-crate `#[cfg(test)]` extraction](feedback_cross_crate_cfg_test_extraction.md) — test-support feature; **scope to session doc's project** ([f](feedback_session_document_scope_discipline.md)).

## Email Drafting (corky)

- **No auto-send** — default Gmail Drafts ([feedback](feedback_no_auto_send_emails.md)); **wait for complete input** ([feedback](feedback_wait_for_complete_input.md)).
- **Reply threading**: mail/conversations/ → in_reply_to + `Re: <subject>`; **verify facts vs mail/contacts/<name>/CLAUDE.md**.
- **Gmail filter from+to is AND** — use query OR (v0.11.5); no retroactive apply. **YouTube default public** ([feedback](feedback_corky_youtube_public_default.md)).

## References

- [cargo install vs workspace-patch divergence](reference_cargo_install_workspace_patch_divergence.md) — failing install often unrelated sibling work.
- [agent-doc build-worktree needs sibling ../tmux-router](reference_agent_doc_build_worktree_needs_sibling_tmux_router.md) — clean build-worktrees go under src/, not /tmp; RTK masks install-full exit code.
- [Resume build source](reference_resume_build_source.md) — resume/ builds from data.toml via agent-resume.
- [Markdown Annotations](reference_markdown_annotations.md) — iA Writer annotation spec as prior art.
- [monsterrodholders SSH](reference_monsterrodholders_ssh.md) — `ssh monsterrodholders-server`→root@50.28.2.199, /opt/monsterrodholders/app.
- [WordPress/CRS ops → wp-liquidweb-ops skill](reference_monsterrodholders_waf_933120.md) — use skill over memory.
