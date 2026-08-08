# agent-loop Memory

## agent-doc Recovery

- **Procedures** → [recovery index](reference_agent_doc_recovery_index.md) (stale-binary, HEAD corruption, queue/pending, no_ack, force-disk, authority/integrity wedges, retained-write states, `state.db` + mutation-flag corruption).
- **Two rules that override any recovery instinct**: never `--force-disk` when an editor holds authority; never hand-align disk to the authority.
- **Crash/handoff/spin/thrash**: [start-crash](reference_agent_doc_start_crash_cross_root_submodule_autostart.md), [handoff→IPC drift](feedback_stranded_handoff_socket_causes_ipc_snapshot_drift.md), [corrupt HEAD spin](feedback_wedged_controller_and_jb_ide_spin_on_corrupt_crdt_response_in_head.md), [editor thrash](reference_jb_editor_thrash_on_restart_indexing_and_debug_logcat.md), [bugs2.done.md=ARCHIVE](reference_agent_doc_bugs2_done_is_archive_not_session.md), [boundary accum worktree-only](feedback_boundary_accum_worktree_only_ipc_repair_checkout_head.md).
- **Concurrency/shared tree** — don't force; commit+push to OWN remote: [force-push](feedback_concurrent_sessions_force_push_shared_superproject.md), [refactor churn](feedback_concurrent_agent_doc_refactor_session_build_coordination.md), [JB pane](feedback_jb_run_agentdoc_route_owned_pane_processing_prompt_collision.md), [DEV stalls closeout](reference_concurrent_agentdoc_dev_session_stalls_closeout_recycle.md).
- **Stale-plugin wedges**: [buffer behind → restart/force-disk/commit/reset](reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery.md); [ACK loop → `reload-lib` + `write --commit`](reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge.md).
- **[Hand-committing the doc → snapshot AHEAD of HEAD → next preflight contract is EMPTY](reference_agent_doc_hand_commit_leaves_stale_snapshot_empty_preflight_contract.md)** — queue silently stops draining; `reset --from-current --preserve-session`.
- **[TWO degraded contract shapes, different bugs](reference_agent_doc_two_degraded_cycle_contract_shapes.md)** — NO marker = cwd left a submodule; marker+EMPTY = preflight errored to stderr. `missing_replica` + live IDE → **`admin reload-lib`**, never force-disk.
- **[`install-full` drops a JB plugin into RUNNING IDEs → replica storm](reference_install_full_editor_plugin_thrashes_running_ide_replica.md)** — only an IDE restart fixes it; skip `install-editor-plugins` when sessions are live.
- **[An editor paste folds N `do [#id]` lines into ONE head](reference_agent_doc_folded_queue_head_swallows_pasted_ids.md)** — extras become inert `Freeform`; fixed 0.35.120.
- **[`unmet=committed` with editor+delivery converged and actor idle = the process-local `needs_operator` latch](reference_agent_doc_needs_operator_latch_wedges_closeout_until_reexec.md)** — install/recycle reexec resets it and the capture replays itself. Fixed a739c20b6.
- **[`state.db is locked` + no session-check verdict = CONTROLLER RESPAWN STORM](reference_agent_doc_controller_respawn_storm_locks_state_db.md)**, not corruption — healthy=2 controllers per root; never pkill.

## agent-doc Project Intent

- [Patchboard markets FIRST](project_product_sequencing_patchboard_first_agentdoc_later.md); [Slint zoom + drag](reference_patchboard_app_slint_zoom_and_drag_technique.md); [merge expertise = alpha](project_realtime_merge_experience_marketable_alpha.md).
- [IPC root-cause audit](project_ipc_audit_and_stale_plugin_detect.md) (5 causes A-E); [provauth](project_provauth_series_status.md) 3b blocked on crdtauth6; [per-cell CRDT merge isolation](project_agent_doc_percell_crdt_merge_intent.md); [[feedback_stop_cutting_agent_doc_releases]].
- [Reliable-sync plane IS authority; sidecar = durability](project_reliable_sync_plane_is_authority_sidecar_durability_only.md). Done/gated: [live_editor S1–S6](project_live_editor_reactive_family_initiative.md), [supervisor 08b](project_agent_doc_supervisor_migration_intent.md), [carry-forward invariant](project_fintol_pcwc_carry_forward_invariant.md).
- [Decomposition](project_agent_doc_module_decomposition.md) — preflight/sync/session_check/git/start/template left; [split fn+tests](feedback_module_split_functionality_with_tests.md); [SimWorld over mocks](feedback_simworld_over_mocks_for_sync_tmux.md).

## agent-doc Releases

- [Release pause SUPERSEDED 2026-08-02](feedback_stop_cutting_agent_doc_releases.md) — tag push auto-publishes GitHub Release + PyPI. [0.34.x](reference_agent_doc_034x_publish_reality.md) = historical local-install-only record.
- **[ALWAYS install + recycle after any fix](feedback_always_install_and_recycle_agent_doc.md)** — never ask/defer; ≠ a release. [auto-install](feedback_supervisor_autoinstall_drives_install_not_manual.md).
- [FlowCore budget gate](reference_flowcore_token_budget_gate.md) — guard_/proof=/reason= breaks hot_path_guard. [Document is source of truth](feedback_agent_doc_document_is_source_of_truth.md) — go-mode queue IS authorization.
- **Unpublished tags**: move tag vs new version. [pyproject + VERSIONS.md first](feedback_versions_md_before_release.md); [PyPI auto via CI](reference_pypi_publish_via_ci.md).
- **cargo install + build --release after ANY edit** ([f](feedback_cargo_build_release_after_route_edit.md), [f](feedback_cargo_install_on_release.md)); [core/FFI via `lib-install`](feedback_jb_plugin_cdylib_hot_reload.md).
- [JB bump per .kt edit](feedback_jb_plugin_version_bump_per_edit.md); [build on bump](feedback_build_on_version_bump.md); [manual test first](feedback_testing_before_release.md).

## lazily / Design

- [8 primitive families](project_lazily_realtime_distributed_primitives.md) RELEASED. [RelayCell backpressure](project_relaycell_backpressure_design.md) — gap = js npm.
- **[NO observers — only Effects](reference_lazily_no_observers_only_effects.md)**; **[edge set vs observer registry](reference_lazily_edge_set_vs_observer_registry_test.md)** — anything surviving an invalidation is not a graph edge.
- **[Hybrid staleness loses writes at depth 2](reference_lazily_hybrid_staleness_cascade_defect.md)** — one-level mark + cache-trusting read.
- **[`signal`/`get_signal` deprecated; use `computed`+`get`](reference_lazily_signal_getsignal_is_prekernel_use_computed_get.md)** — only agent-doc's guard stops you.
- **[Edge index + teardown scopes](reference_lazily_edge_index_teardown_scope.md)** — O(1) dedup; thresholds differ per binding.
- **[Cell kernel v2 SHIPPED](project_lazily_memo_rename_plan_status.md)** — `Source`/`Computed`/`Effect`; [value-threaded Compute](project_lazily_fortified_compute_wrapper_spike.md); ambient gone in ALL 8 ([py](project_lazily_py_ambient_removal_blocked_on_bare_reads.md)); [MergeCell fixtures parked](project_lazily_mergefeed_fixtures_parked.md).
- **[ReactiveMap unification — rs Phase A DONE](project_lazily_reactivemap_unification_rs_reference.md)** — `KeyedOrder` core, reactivity in the shells; [ordering gate + 2 false greens](reference_lazily_collections_ordering_gate_rs.md).
- **[Queue family: 3 flavors in rs](project_lazily_queue_family_flavors_rs_reference.md)** — invalidate OUTSIDE the storage lock. **[lazily-cs = 9th binding](project_lazily_cs_binding.md)** — corpus replays with an EMPTY ledger.
- **[GDScript = chosen 10th binding, NOT GDExtension over cpp](project_lazily_gdscript_binding_direction.md)** — blocker is consoles; `ctx.scope()` is the SOLE strong owner of effects.
- **[lazily-cpp core is wasm-clean; the umbrella header is NOT](reference_lazily_cpp_wasm_clean_header_subset.md)** — `lazily.hpp` drags in `shm_open`/`mmap`.

## lazily / Releases

- **Records**: [queue-family](reference_lazily_queue_family_release_2026_07_26.md), [ReactiveMap core](reference_lazily_reactivemap_core_release_2026_07_26.md), [v2 all 8](reference_lazily_v2_release_2026_07_22.md), [Reliable Sync parity](reference_reliable_sync_full_parity_all_bindings.md), [ReactiveMap migration](project_reactivemap_keyed_collection_migration.md). **js npm needs an operator OTP publish.**
- **[lazily-go map data race — FIXED v0.23.2](reference_lazily_go_thread_safe_map_data_race_ci_red.md)** — every READ ran off the context lock.
- **Per-target publish** → [targets](reference_lazily_release_targets.md), [cargo token](reference_cargo_publish_token_passage.md), [py wheels](reference_lazily_py_wheels_ci.md), [coverage gen](reference_lazily_publish_and_coverage_generator.md), [js E404](reference_lazily_js_npm_trusted_publisher_e404.md), [kt immutable](reference_lazily_kt_release_immutable_packages.md), [cpp](reference_lazily_cpp_family_features_wire_level_model.md)/[zig](reference_lazily_zig_test_suite_and_coldrecalc_bottleneck.md) = tag + `gh release`.
- [zig master gutted std.posix](reference_zig_master_posix_file_api_moved_to_os_linux.md); **[verify ALL THREE pinned toolchains](reference_lazily_zig_verify_all_three_pinned_toolchains.md)**.
- [coverage.json is hot multi-session](reference_lazily_coverage_json_hot_multisession_file.md) — one commit per binding. [Queue `go` overrides queue:stop](feedback_queue_go_marker_overrides_stop_churn.md).
- **[Signal clause 3](reference_lazily_signal_clause3_welded_eagerness.md)** — welded eagerness into slot invalidation; go blocked on consuming cascade.
- **[3 ways to report green while testing nothing](reference_lazily_conformance_delivery_three_failure_modes.md)** — REPAIRED. **Always assert fixtures ran.**
- **[Corpus perturbation finds vacuity static passes cannot](reference_corpus_perturbation_finds_vacuity_static_passes_cannot.md)** — validate the detector against pre-fix sources first.
- **[Object-valued assertion keys live in `expect` blocks, not `assertions`](reference_lazily_object_valued_assertion_keys_live_in_expect_blocks.md)** — never scope this audit from a grep.
- **[Rung-0 bind ledger: shapes per binding](reference_lazily_rung0_bind_ledger_shapes_per_binding.md)** — declare from the BYTES, match by CONTENT digest; structural walk, never stringify.
- **[Perturbation harness per binding](reference_lazily_corpus_perturbation_harness_per_binding.md)** — kt's gradle workingDir needs `afterEvaluate` or it false-cleans.
- **[A new canonical fixture reddens ALL NINE bindings](reference_lazily_spec_new_canonical_fixture_reddens_all_nine.md)** — budget ~10 repos.
- **[A JSON fixture can't carry a >2^53 expectation as a number](reference_conformance_fixture_cannot_carry_its_own_expectation_as_a_json_number.md)** — wire=text/hex, expectation=decimal string.

## Decisions

- **[Runners assert the LIBRARY, not runner bookkeeping](feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping.md)** — counters inside the library's call path.
- **[A per-call-site count is not conformance](feedback_per_call_site_count_is_not_conformance.md)** — push the obligation into the tracker, then neuter its bookkeeping.
- **[Use the AST, never regex, for source edits](feedback_use_ast_not_regex_for_source_edits.md)** — `tsift edit-intents --apply --verify`.
- **[A "mutation survived" result is suspect until the build cache is proven cold](reference_mutation_check_false_green_from_stale_build_cache.md)**.
- **[A `lazily-spec` mutation probe reddens EVERY other binding](reference_lazily_spec_shared_corpus_concurrent_mutation_probes.md)** — scratch-copy + `LAZILY_SPEC_CONFORMANCE_DIR`.
- **[NEVER run two subagents against one repo](feedback_never_dispatch_concurrent_agents_into_one_repo.md)** — clean `git status` ≠ proof.
- **[Discuss naming to convergence BEFORE editing](feedback_discuss_naming_before_applying.md)**.
- [Refactor spirals](user_refactor_spiral_pattern_guardrails.md); [note ≠ endorsement](feedback_askuserquestion_no_option_confirm_actual_choice.md); [plain not jargon](feedback_explain_decisions_plainly_not_jargon.md); [no sycophancy](feedback_no_sycophancy.md); [no oversimplify](feedback_no_oversimplify.md); [priorities](user_communication_priorities.md).

## equityfundingsource

- **[NO CI workflow](reference_efs_repo_has_no_ci_workflow.md)** — local `make check` + live deploy are the only gates. **[Workers Logs need the ADMIN token](reference_efs_production_logs_admin_token_and_telemetry_query.md)** — count from `calculations`.
- **Removed**: [funding-commit stage](reference_efs_funding_commit_stage_removed.md), [closing gate + accreditation denorm](reference_efs_accreditation_denormalization_and_closing_gate_removed.md).
- **Traps**: [legacy `applications`](project_efs_legacy_applications_table.md), [soft-delete trigger](reference_efs_users_soft_delete_trigger.md), [deal-card stale read](reference_efs_deal_card_pending_annual_requirements_stale_read.md).
- **Loan Package**: [per-lender packet](reference_efs_per_lender_deal_packet_and_message_lender.md), [tab component](reference_efs_loan_package_tab_real_component_and_tmo_doc_sync.md), [TMO LOS API](reference_tmo_los_attachment_document_api.md).
- **Ops/UI**: [prod D1](reference_efs_d1_token_working.md), [SITE_URL→origin](reference_efs_site_url_unset_use_request_origin.md), [security_events≠badges](reference_efs_security_events_audit_log.md), [gold contrast](reference_efs_gold_palette_contrast_correction.md).
- **Concurrent-deploy (clobber-immune)**: [worktree as source](reference_efs_worktree_as_deploy_and_commit_source_clobber_immune.md), [grep-verify](reference_efs_verify_deploy_via_string_grep.md), [deploy from main](reference_efs_deploy_from_main_checkout_not_fresh_install_worktree.md), [commit only yours](feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap.md).
- **[Worktree deploy CAN carry worktree-local `lib/` edits](reference_efs_worktree_deploy_with_lib_local_workspace_links.md)** — proof = content-hashed asset names.
- **[Guard tests slice a FIXED byte window of source](reference_efs_fixed_window_source_grep_tests_break_on_comments.md)** — slice to the end of the function.
- **[Workers Logs drops an Error's header line; a sim endpoint the real service lacks is a permanent false green](reference_workers_logs_drops_error_header_and_sim_fake_endpoint_false_green.md)**.
- **[Pipeline routes ANY unknown loan status into "Available Deals"](reference_efs_pipeline_unknown_status_falls_into_available_deals.md)** — admin column ≠ lender feed.
- [Doc-type catalogue](reference_efs_document_type_catalogue_locations.md); [Drive portal](reference_efs_local_portal_drive_recipe.md); [lender PROD](reference_efs_prod_lender_login_verify_recipe.md).

## tsift

- [SKILL.md is primary source](feedback_tsift_skill_primary_source.md) — owns full API; AGENTS.md lean. **Publishing** — [crates.io list completeness](reference_tsift_release_publish_list.md), **[OIDC: a green publish job proves nothing](reference_tsift_cratesio_trusted_publishing.md)** (all 26 registered; read the config via browser, never infer), [opencode npm](reference_opencode_tsift_npm_publish.md).

## Security

- **No cleartext secrets** — livestreams; `pass`/`passage` subshell ([f](feedback_no_cleartext_secrets.md)). **Redact nested JSON** — `jq 'map_values(type)'` ([f](feedback_nested_json_redaction.md)).
- **[A `$(passage ...)` ref in a .env is NOT a secret](feedback_passage_substitution_in_env_is_not_a_secret.md)** — classify SHAPE; hash history vs the store to prove whether it's still live.
- **[REDACTED_PRODUCT]: passage [REDACTED_PRODUCT]/OPENROUTER_API_KEY**, not btak/ ([f](feedback_[REDACTED_PRODUCT]_openrouter_key.md)).

## Git Ops

- **[Shared superproject index: other panes PRE-STAGE files](reference_shared_superproject_index_contamination_commit_only.md)** — always `git commit --only -- <paths>`.
- **[`git commit --only` SILENTLY undoes `git rm --cached`](feedback_git_commit_only_silently_undoes_rm_cached.md)** — prove via `git ls-files`.
- [rebase cwd hazard](feedback_git_rebase_cwd_submodule_hazard.md); [recursive commits](feedback_recursive_submodule_commits.md) inside-out; [check working tree](feedback_check_working_tree.md).
- [RTK masks `make check` exit codes](feedback_rtk_masks_make_check_exit.md) — capture `$?` + grep `result: FAILED`. **[RTK `diff` says "identical"; `grep -rl` misses real hits](reference_rtk_diff_filter_reports_identical_for_differing_files.md)** — scan with `python3`, or run `rtk proxy <cmd>`.
- **[Bare `cargo check` intercepted → stale "0 crates compiled"](reference_cargo_check_intercepted_returns_stale_digest.md)** — `touch` + digest-runner.
- **[Restoring a backup keeps the OLD mtime → cargo reuses the MUTATED build](reference_cargo_stale_build_when_mutation_check_restores_mtime.md)** — `touch` after each restore.

## Misc

- **[JB plugin build/packaging](reference_jetbrains_plugin_build_conventions.md)** — ActionPromoter breaks .md intentions; K2/JVM21/242.
- **AGENTS.md canonical** — symlink CLAUDE.md → it ([f](feedback_agents_md_canonical.md)). [corky diarize](feedback_corky_diarize_feature.md); [bun](feedback_bun_not_pnpm.md); [file:// paths](feedback_file_protocol_paths.md).
- **Writing voice**: `...` not spaced em-dash; avoid delve/leverage/hedging ([f](feedback_writing_voice.md)). [Verify live system](feedback_verify_live_system.md), not just tests.
- **"do" = execute ALL now** ([f](feedback_do_means_execute_all.md)); [approval ≠ cut corners](feedback_approval_not_skip_processes.md); [precommit testing](feedback_precommit_testing.md); [no confirm on iteration](feedback_no_confirmation_iterate.md).
- **No hallucinating** — [say "I don't know"](feedback_no_hallucinate_uncertainty.md); [broken ≠ "correct"](feedback_no_hallucinate_correct_errors.md); [verify proxy first](feedback_verify_proxy_before_kill.md).
- [Distribution-aware reorg](feedback_distribution_aware_reorg.md); [cross-crate `#[cfg(test)]`](feedback_cross_crate_cfg_test_extraction.md); [scope to session doc](feedback_session_document_scope_discipline.md).
- **[Concurrency repros leak busyloops to PID 1](reference_leaked_busyloop_load_generators_saturate_cpu.md)** — kill via `xargs -n1`.
- **[Parallel subagents share ONE scratchpad dir](reference_parallel_subagents_share_one_scratchpad_dir.md)** — key filenames per repo.

## Email (corky)

- **No auto-send** — default Gmail Drafts ([f](feedback_no_auto_send_emails.md)); [wait for complete input](feedback_wait_for_complete_input.md); [YouTube default public](feedback_corky_youtube_public_default.md).
- **Reply threading**: mail/conversations/ → in_reply_to + `Re: <subject>`; verify facts vs mail/contacts/. **Gmail filter from+to is AND** — use query OR.

## Job Offers

- **[Haiven ask SENT 07-26; Acadian HOLD](project_job_offers_haiven_ask_sent_acadian_hold.md)** — $185k/$20k/1.0%; floor $180k+$15k+0.75%.

## References

- [mag7-viewer: NO CI + browser-verify](reference_mag7_viewer_no_ci_and_browser_verify_recipe.md) — brush needs native track-drag events.
- [`.bin/agent-doc` symlink is repo-root-relative](reference_bin_agent_doc_symlink_is_reporoot_relative.md) — don't "fix". [build-worktree needs ../tmux-router](reference_agent_doc_build_worktree_needs_sibling_tmux_router.md).
- **monsterrodholders**: [SSH](reference_monsterrodholders_ssh.md), [WAF/CRS](reference_monsterrodholders_waf_933120.md), ["down"=origin cert](reference_monsterrodholders_origin_cert_expiry_outage.md), [never CSF DOCKER=1](feedback_never_set_csf_docker_1_monsterrodholders.md), [monitoring](reference_monsterrodholders_monitoring_alerting_stack.md).
