# agent-loop Memory

## agent-doc Recovery

- **Procedures** → [recovery index](reference_agent_doc_recovery_index.md) (stale-binary, HEAD corruption, queue/pending, no_ack, force-disk).
- **Crash/handoff/spin/thrash**: [start-crash](reference_agent_doc_start_crash_cross_root_submodule_autostart.md), [handoff→IPC drift](feedback_stranded_handoff_socket_causes_ipc_snapshot_drift.md), [corrupt HEAD spin](feedback_wedged_controller_and_jb_ide_spin_on_corrupt_crdt_response_in_head.md), [editor thrash](reference_jb_editor_thrash_on_restart_indexing_and_debug_logcat.md).
- **Concurrency/shared tree** — don't force; commit+push to OWN remote: [force-push](feedback_concurrent_sessions_force_push_shared_superproject.md), [refactor churn](feedback_concurrent_agent_doc_refactor_session_build_coordination.md), [JB pane](feedback_jb_run_agentdoc_route_owned_pane_processing_prompt_collision.md), [DEV stalls closeout](reference_concurrent_agentdoc_dev_session_stalls_closeout_recycle.md).
- [bugs2.done.md=ARCHIVE](reference_agent_doc_bugs2_done_is_archive_not_session.md); [boundary accum worktree-only](feedback_boundary_accum_worktree_only_ipc_repair_checkout_head.md).
- **Stale-plugin wedges**: [buffer behind → restart, force-disk, commit, reset](reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery.md); [ACK loop → `reload-lib` + `write --commit`](reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge.md).
- **Duplicate boundary markers** hard-fail preflight AND session-check — [no self-heal](reference_agent_doc_recovery_replay_duplicate_boundary_wedges_integrity_gate.md).
- **Compact commits, doc stays uncompacted**: [`patch <FILE> exchange` with HEAD's body](reference_agent_doc_compact_committed_but_visible_rewrite_deferred.md) — reload-lib does NOT flush; never `write --commit`.
- **`state.db` clobbered** ⇒ EVERY doc dies: [header bytes 0-103](reference_agent_doc_state_db_header_clobbered_by_tracing_on_controller_recycle.md), [debug tracing](reference_agent_doc_state_db_overwritten_by_debug_tracing.md) — check `head -c 16`; printed `pkill` kills OTHER sessions.
- **Retained `pending_write` on a SUPERSEDED hash**: [`commit <FILE>` clears it](reference_agent_doc_retained_pending_write_superseded_hash_agent_doc_commit.md) — everything else refuses; never force-disk.
- **"deferred: authority_unavailable" ⇒ mutations may ALREADY be on disk**: [read the doc before retrying](reference_agent_doc_deferred_write_may_have_already_applied_mutations.md) — blind retry dies on the duplicate-id guard; and `session-check ok` does NOT prove `--done`/`--backlog-add` applied.
- **Gate deadlock** (all three refuse): [`reset --from-current --preserve-session` + id-bearing `### Re:`](reference_agent_doc_gate_deadlock_reset_from_current_and_id_bearing_re_block.md).
- **Resurrected deleted queue head**: [stale overlay.yrs + wedged controller](reference_agent_doc_dangling_queue_head_resurrect_baseline_overlay_and_controller_model_wedge.md) — recycle defers → kill `controller serve` pid.
- **`write_applied`, commit refuses "direct patchback"**: [guard misfires on the binary's OWN write](reference_agent_doc_write_applied_no_terminal_patchback_guard_misfire.md) — hand `git commit --only`.

## agent-doc Project Intent

- [Patchboard markets FIRST](project_product_sequencing_patchboard_first_agentdoc_later.md). [Slint zoom + drag](reference_patchboard_app_slint_zoom_and_drag_technique.md). [Merge expertise = alpha](project_realtime_merge_experience_marketable_alpha.md).
- [IPC root-cause audit](project_ipc_audit_and_stale_plugin_detect.md) — 5 causes A-E. [provauth](project_provauth_series_status.md) — 3b blocked on crdtauth6.
- [Per-cell CRDT merge isolation = deeper fix](project_agent_doc_percell_crdt_merge_intent.md) — #qcellmerge1. [[feedback_stop_cutting_agent_doc_releases]]
- [Reliable-sync plane IS authority; sidecar = durability](project_reliable_sync_plane_is_authority_sidecar_durability_only.md). Done/gated: [live_editor S1–S6](project_live_editor_reactive_family_initiative.md), [supervisor 08b](project_agent_doc_supervisor_migration_intent.md), [#fintol2/#pcwc](project_fintol_pcwc_carry_forward_invariant.md).
- [Decomposition](project_agent_doc_module_decomposition.md) — preflight/sync/session_check/git/start/template left. [split fn+tests](feedback_module_split_functionality_with_tests.md); [SimWorld over mocks](feedback_simworld_over_mocks_for_sync_tmux.md).

## agent-doc Releases

- [STOP cutting releases](feedback_stop_cutting_agent_doc_releases.md) — none until per-cell CRDT fixed; confirm first. [0.34.x](reference_agent_doc_034x_publish_reality.md) = local install only.
- **[ALWAYS install + recycle after any fix](feedback_always_install_and_recycle_agent_doc.md)** — never ask/defer; ≠ a release. [auto-install](feedback_supervisor_autoinstall_drives_install_not_manual.md).
- [FlowCore budget gate](reference_flowcore_token_budget_gate.md) — guard_/proof=/reason= breaks hot_path_guard. [Document is source of truth](feedback_agent_doc_document_is_source_of_truth.md) — go-mode queue IS authorization.
- **Unpublished tags**: move tag vs new version. [pyproject + VERSIONS.md first](feedback_versions_md_before_release.md). crates.io + GH Release; [PyPI auto via CI](reference_pypi_publish_via_ci.md).
- **cargo install + build --release after ANY edit** ([f](feedback_cargo_build_release_after_route_edit.md), [f](feedback_cargo_install_on_release.md)); [core/FFI via `lib-install`](feedback_jb_plugin_cdylib_hot_reload.md) (cdylib hot-reloads by mtime).
- [JB bump per .kt edit](feedback_jb_plugin_version_bump_per_edit.md); [build on bump](feedback_build_on_version_bump.md); [manual test first](feedback_testing_before_release.md).

## lazily / Design

- [8 primitive families](project_lazily_realtime_distributed_primitives.md) RELEASED. [RelayCell backpressure](project_relaycell_backpressure_design.md) — full parity; gap = js npm.
- **[NO observers — only Effects](reference_lazily_no_observers_only_effects.md)** — `on_transition` is an Effect; streams are Topic.
- **[Hybrid staleness loses writes at depth 2](reference_lazily_hybrid_staleness_cascade_defect.md)** — one-level mark + cache-trusting read.
- **[`signal`/`get_signal` deprecated; use `computed`+`get`](reference_lazily_signal_getsignal_is_prekernel_use_computed_get.md)** — nothing in lazily stops you; agent-doc's guard does.
- **[Edge set vs observer registry](reference_lazily_edge_set_vs_observer_registry_test.md)** — *anything surviving an invalidation is not a graph edge*.
- **[Edge index + teardown scopes](reference_lazily_edge_index_teardown_scope.md)** — O(1) dedup; thresholds **differ per binding**. `ctx.scope()`, `disarm()`.
- **[Cell kernel v2 SHIPPED](project_lazily_memo_rename_plan_status.md)** — `Source`/`Computed`/`Effect`; [value-threaded Compute](project_lazily_fortified_compute_wrapper_spike.md); **ambient gone in ALL 8** ([py: the missing datum was the READER](project_lazily_py_ambient_removal_blocked_on_bare_reads.md)). [MergeCell fixtures parked](project_lazily_mergefeed_fixtures_parked.md).
- **[ReactiveMap unification — rs Phase A DONE](project_lazily_reactivemap_unification_rs_reference.md)** — `KeyedOrder` core, reactivity in the shells; reference shape. [ordering gate + 2 false greens](reference_lazily_collections_ordering_gate_rs.md).
- **[Queue family: 3 flavors in rs](project_lazily_queue_family_flavors_rs_reference.md)** — graph-agnostic cores + per-flavor shells; invalidate OUTSIDE the storage lock.
- **[lazily-cs = 9th binding](project_lazily_cs_binding.md)** — rows 0+15; corpus replays with an EMPTY ledger; `#lzcsconcurrency` first.
- **[GDScript = chosen 10th binding, NOT GDExtension over cpp](project_lazily_gdscript_binding_direction.md)** — blocker is **consoles**, not web; no cycle collector → WeakRef back-edges, so `ctx.scope()` is the SOLE strong owner of effects.
- **[lazily-cpp core is wasm-clean; the umbrella header is NOT](reference_lazily_cpp_wasm_clean_header_subset.md)** — `cell`/`collections`/`keyed_order`/`statechart` = zero POSIX+threads; `lazily.hpp` drags in `shm_open`/`mmap`. No emscripten CI.

## lazily / Releases

- **Release records**: [queue-family 07-26](reference_lazily_queue_family_release_2026_07_26.md) (rs 0.51.0 = only behaviour change), [ReactiveMap core](reference_lazily_reactivemap_core_release_2026_07_26.md), [v2 all 8](reference_lazily_v2_release_2026_07_22.md), [Reliable Sync parity](reference_reliable_sync_full_parity_all_bindings.md), [ReactiveMap migration](project_reactivemap_keyed_collection_migration.md). **js npm needs an operator OTP publish** (CI OIDC 404s = expected-red).
- **[lazily-go map data race — FIXED v0.23.2](reference_lazily_go_thread_safe_map_data_race_ci_red.md)** — every READ ran off the context lock; a `Get` mutates the graph.
- **Per-target publish** → [targets](reference_lazily_release_targets.md), [cargo token](reference_cargo_publish_token_passage.md), [py wheels](reference_lazily_py_wheels_ci.md), [coverage gen](reference_lazily_publish_and_coverage_generator.md), [js E404](reference_lazily_js_npm_trusted_publisher_e404.md), [kt immutable](reference_lazily_kt_release_immutable_packages.md), [cpp](reference_lazily_cpp_family_features_wire_level_model.md)/[zig](reference_lazily_zig_test_suite_and_coldrecalc_bottleneck.md) = tag + `gh release`.
- [zig master gutted std.posix](reference_zig_master_posix_file_api_moved_to_os_linux.md) — use `std.os.linux` syscalls. **[Verify ALL THREE pinned toolchains](reference_lazily_zig_verify_all_three_pinned_toolchains.md)**.
- [coverage.json is hot multi-session](reference_lazily_coverage_json_hot_multisession_file.md) — one commit per binding. [Queue `go` overrides queue:stop](feedback_queue_go_marker_overrides_stop_churn.md) — remove `go` AND set stop.
- **[Signal clause 3](reference_lazily_signal_clause3_welded_eagerness.md)** — welded eagerness into slot invalidation; 2–3× compute. go blocked on consuming cascade.
- **[3 ways to report green while testing nothing](reference_lazily_conformance_delivery_three_failure_modes.md)** — REPAIRED. **Always assert fixtures ran.**

## Decisions

- **[Runners assert the LIBRARY, not runner bookkeeping](feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping.md)** — counters inside the library's call path; mutation-check each.
- **[Use the AST, never regex, for source edits](feedback_use_ast_not_regex_for_source_edits.md)** — `tsift edit-intents --apply --verify`; regex ate match arms.
- **[NEVER run two subagents against one repo](feedback_never_dispatch_concurrent_agents_into_one_repo.md)** — one `git reset`s the other's work; clean `git status` ≠ proof.
- **[Discuss naming to convergence BEFORE editing](feedback_discuss_naming_before_applying.md)** — 5 mid-discussion renames caused cross-repo churn.
- [Refactor-spiral guardrails](user_refactor_spiral_pattern_guardrails.md); [note ≠ endorsement](feedback_askuserquestion_no_option_confirm_actual_choice.md); [plain not jargon](feedback_explain_decisions_plainly_not_jargon.md); [no sycophancy](feedback_no_sycophancy.md); [don't oversimplify](feedback_no_oversimplify.md); [priorities](user_communication_priorities.md).

## equityfundingsource

- **[NO CI workflow](reference_efs_repo_has_no_ci_workflow.md)** — local `make check` + live deploy are the only gates.
- **[Workers Logs need the ADMIN token](reference_efs_production_logs_admin_token_and_telemetry_query.md)** — `view: events` is sampled; count from `calculations`.
- **Removed**: [funding-commit stage](reference_efs_funding_commit_stage_removed.md), [closing gate + accreditation denorm](reference_efs_accreditation_denormalization_and_closing_gate_removed.md).
- **Traps**: [legacy `applications`](project_efs_legacy_applications_table.md) (apply4=`full_applications`), [soft-delete trigger](reference_efs_users_soft_delete_trigger.md), [deal-card stale read](reference_efs_deal_card_pending_annual_requirements_stale_read.md).
- **Loan Package**: [per-lender packet](reference_efs_per_lender_deal_packet_and_message_lender.md), [tab=`AdminLoanDocumentsManager`](reference_efs_loan_package_tab_real_component_and_tmo_doc_sync.md), [TMO LOS API](reference_tmo_los_attachment_document_api.md).
- **Ops/UI**: [prod D1](reference_efs_d1_token_working.md), [SITE_URL→origin](reference_efs_site_url_unset_use_request_origin.md), [security_events≠badges](reference_efs_security_events_audit_log.md), [gold contrast](reference_efs_gold_palette_contrast_correction.md).
- **Concurrent-deploy (clobber-immune)**: [worktree as source](reference_efs_worktree_as_deploy_and_commit_source_clobber_immune.md), [string-grep verify](reference_efs_verify_deploy_via_string_grep.md), [deploy from main](reference_efs_deploy_from_main_checkout_not_fresh_install_worktree.md), [commit only yours](feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap.md).
- **[Worktree deploy CAN carry worktree-local `lib/` edits](reference_efs_worktree_deploy_with_lib_local_workspace_links.md)** — mirror node_modules per-entry + rebuild `@btakita` links relative to the worktree at app/ AND every `lib/*/node_modules`; isolation proof = content-hashed asset names.
- **[Pipeline routes ANY unknown loan status into "Available Deals"](reference_efs_pipeline_unknown_status_falls_into_available_deals.md)** — prod stores `Application Declined`, code knows only `Denied`; admin column ≠ lender feed (which filters `investment_deals.status`, never loan status).
- [Doc-type catalogue](reference_efs_document_type_catalogue_locations.md) — `dt_*` = seed migration + 4 mirrors. [Drive portal](reference_efs_local_portal_drive_recipe.md), [lender on PROD](reference_efs_prod_lender_login_verify_recipe.md).

## tsift

- [SKILL.md is primary source](feedback_tsift_skill_primary_source.md) — owns full API; AGENTS.md lean. **Publishing** — [crates.io list completeness](reference_tsift_release_publish_list.md) (both release.yml lists; lockstep), [OIDC](reference_tsift_cratesio_trusted_publishing.md), [opencode npm](reference_opencode_tsift_npm_publish.md).

## Security

- **No cleartext secrets** — livestreams; `pass`/`passage` subshell ([f](feedback_no_cleartext_secrets.md)). **Redact nested JSON** — `jq 'map_values(type)'` ([f](feedback_nested_json_redaction.md)).
- **[REDACTED_PRODUCT]: passage [REDACTED_PRODUCT]/OPENROUTER_API_KEY**, not btak/ ([f](feedback_[REDACTED_PRODUCT]_openrouter_key.md)).

## Git Ops

- **[Shared superproject index: other panes PRE-STAGE files](reference_shared_superproject_index_contamination_commit_only.md)** — always `git commit --only -- <paths>`.
- [rebase cwd hazard](feedback_git_rebase_cwd_submodule_hazard.md) — cd to superproject root first. [Recursive commits](feedback_recursive_submodule_commits.md) inside-out; [check working tree](feedback_check_working_tree.md).
- [RTK masks `make check` exit codes](feedback_rtk_masks_make_check_exit.md) — capture `$?` + grep `result: FAILED`. **[RTK `diff` says "identical" for differing files](reference_rtk_diff_filter_reports_identical_for_differing_files.md)** — FALSE PASS.
- **[Bare `cargo check` intercepted → stale "0 crates compiled"](reference_cargo_check_intercepted_returns_stale_digest.md)** — `touch` + digest-runner.
- **[Restoring a backup keeps the OLD mtime → cargo reuses the MUTATED build](reference_cargo_stale_build_when_mutation_check_restores_mtime.md)** — `touch` after each restore, re-run the sweep.

## Misc

- **[JetBrains plugin build/packaging](reference_jetbrains_plugin_build_conventions.md)** — ActionPromoter breaks .md intentions; K2/JVM21/242; never bundle platform transitives.
- **AGENTS.md canonical** — symlink CLAUDE.md → it ([f](feedback_agents_md_canonical.md)). [corky diarize](feedback_corky_diarize_feature.md); [bun](feedback_bun_not_pnpm.md); [file:// paths](feedback_file_protocol_paths.md).
- **Writing voice**: `...` not spaced em-dash; avoid delve/leverage/hedging ([f](feedback_writing_voice.md)). [Verify live system](feedback_verify_live_system.md), not just tests.
- **"do" = execute ALL now** ([f](feedback_do_means_execute_all.md)); [approval ≠ cut corners](feedback_approval_not_skip_processes.md); [precommit testing](feedback_precommit_testing.md); [no confirm on iteration](feedback_no_confirmation_iterate.md).
- **No hallucinating** — [say "I don't know"](feedback_no_hallucinate_uncertainty.md); [broken ≠ "correct"](feedback_no_hallucinate_correct_errors.md); [verify proxy first](feedback_verify_proxy_before_kill.md).
- [Distribution-aware reorg](feedback_distribution_aware_reorg.md); [cross-crate `#[cfg(test)]`](feedback_cross_crate_cfg_test_extraction.md); [scope to session doc](feedback_session_document_scope_discipline.md).
- **[Concurrency repros leak busyloops to PID 1](reference_leaked_busyloop_load_generators_saturate_cpu.md)** — `kill $(jobs -p)` fails in non-interactive `zsh -c`; kill via `xargs -n1`.

## Email (corky)

- **No auto-send** — default Gmail Drafts ([f](feedback_no_auto_send_emails.md)); [wait for complete input](feedback_wait_for_complete_input.md). [YouTube default public](feedback_corky_youtube_public_default.md).
- **Reply threading**: mail/conversations/ → in_reply_to + `Re: <subject>`; verify facts vs mail/contacts/<name>/CLAUDE.md. **Gmail filter from+to is AND** — use query OR.

## Job Offers

- **[Haiven ask SENT 07-26; Acadian HOLD](project_job_offers_haiven_ask_sent_acadian_hold.md)** — $185k/$20k/1.0%; floor $180k+$15k+0.75%; fallback = decline+prototype.

## References

- [mag7-viewer: NO CI + browser-verify](reference_mag7_viewer_no_ci_and_browser_verify_recipe.md) — brush needs native track-drag events.
- [`.bin/agent-doc` symlink is repo-root-relative](reference_bin_agent_doc_symlink_is_reporoot_relative.md) — don't "fix". [build-worktree needs ../tmux-router](reference_agent_doc_build_worktree_needs_sibling_tmux_router.md).
- **monsterrodholders**: [SSH](reference_monsterrodholders_ssh.md), [WAF/CRS](reference_monsterrodholders_waf_933120.md), ["down"=origin cert](reference_monsterrodholders_origin_cert_expiry_outage.md), [never CSF DOCKER=1](feedback_never_set_csf_docker_1_monsterrodholders.md), [monitoring](reference_monsterrodholders_monitoring_alerting_stack.md).
