# agent-loop Memory

## agent-doc Recovery

- **Procedures** → [recovery index](reference_agent_doc_recovery_index.md) (stale-binary, HEAD corruption, queue/pending, no_ack, force-disk, authority/integrity wedges, retained-write states, `state.db` + mutation-flag corruption).
- **[Pre-`88b21fea6` `IPC build mismatch` was a CLOCK, not a code difference](reference_ipc_build_mismatch_was_a_clock_not_a_code_difference.md)** — same-version mismatch was usually FALSE (`target/` vs `target/local-install`); a member-crate change was INVISIBLE. All-digits suffix = old, hex = fixed.
- **Two rules that override any recovery instinct**: never `--force-disk` when an editor holds authority; never hand-align disk to the authority.
- **[A mid-session `make install` strands an attached document's replica](reference_cdylib_reload_strands_attached_document_replica.md)** — only an editor tab reopen clears it; the terminal signature is `..._terminal_rebuild_failed` + `disk_read_refused`, NOT bare `missing_replica` (benign churn) or two `.so` files.
- **Crash/spin/thrash**: [start-crash](reference_agent_doc_start_crash_cross_root_submodule_autostart.md), [handoff→IPC drift](feedback_stranded_handoff_socket_causes_ipc_snapshot_drift.md), [corrupt HEAD spin](feedback_wedged_controller_and_jb_ide_spin_on_corrupt_crdt_response_in_head.md), [editor thrash](reference_jb_editor_thrash_on_restart_indexing_and_debug_logcat.md), [bugs2.done.md=ARCHIVE](reference_agent_doc_bugs2_done_is_archive_not_session.md), [boundary accum](feedback_boundary_accum_worktree_only_ipc_repair_checkout_head.md).
- **[autorecycle-on-install handoff wedged 3h05m on a CURRENT binary](reference_agent_doc_autorecycle_install_handoff_wedge_survives_m1b.md)** — a wedged `preparing` controller spins, it does not idle; `admin recycle` times out AND exits 0, only `kill -TERM` clears it.
- **[Test fixtures leak a controller + /tmp root per run](reference_agent_doc_test_fixture_controllers_leak_to_pid1.md)** — 214 procs / 4.2GB in ~6 days, orphaned to PID 1; accumulation, not a live storm.
- **Shared tree** — never force; commit+push to OWN remote: [force-push](feedback_concurrent_sessions_force_push_shared_superproject.md), [refactor churn](feedback_concurrent_agent_doc_refactor_session_build_coordination.md), [JB pane](feedback_jb_run_agentdoc_route_owned_pane_processing_prompt_collision.md), [DEV stalls](reference_concurrent_agentdoc_dev_session_stalls_closeout_recycle.md).
- **[Stale editor lease (`live_editors=0`) waits forever](reference_agent_doc_stale_editor_lease_walking_component_divergence.md)** — divergence WALKS components; the residue was a 7-byte `(HEAD)` marker.
- **[`reason=no_live_editor` can be a LIE — `live_editors=1` all along](reference_no_live_editor_reason_contradicts_live_editors_one.md)** — the real fault is a stale delivery-worker heartbeat; grep ops.log for `live_editors=` before theorising.
- **[`response_captured` + no supervisor idle watch = UNOWNED, not deferred](reference_capture_resume_unowned_and_replay_collision.md)** — `agent-doc repair --resume-capture`; pre-0.35.366 `no_supervisor_idle_watch` was usually a LIE.
- **Wedge signatures** (stale-plugin, hand-commit snapshot, empty-stdin `respond`, missing cycle contract, `install-full` replica storm, folded queue head, `needs_operator` latch, respawn storm, no-op-repair retain) → [recovery index](reference_agent_doc_recovery_index.md#wedge-signatures-moved-out-of-memorymd-2026-08-08).

## agent-doc Project Intent

- **[PyPI prunes old agent-doc releases — floor now 0.35.113](reference_pypi_prunes_old_releases_breaking_artifact_purge_premise.md)** — 105 artifact-purge candidates died in ONE day; a PyPI file is a perishable durable counterpart, and the handoff hard-expires at 7 days.
- [Patchboard markets FIRST](project_product_sequencing_patchboard_first_agentdoc_later.md); [Slint zoom + drag](reference_patchboard_app_slint_zoom_and_drag_technique.md); [merge expertise = alpha](project_realtime_merge_experience_marketable_alpha.md).
- [IPC root-cause audit](project_ipc_audit_and_stale_plugin_detect.md) (5 causes A-E); [provauth](project_provauth_series_status.md) 3b blocked on crdtauth6; [per-cell CRDT merge isolation](project_agent_doc_percell_crdt_merge_intent.md); [[feedback_stop_cutting_agent_doc_releases]].
- [Reliable-sync plane IS authority; sidecar = durability](project_reliable_sync_plane_is_authority_sidecar_durability_only.md). Done/gated: [live_editor S1–S6](project_live_editor_reactive_family_initiative.md), [supervisor 08b](project_agent_doc_supervisor_migration_intent.md), [carry-forward](project_fintol_pcwc_carry_forward_invariant.md).
- [Decomposition](project_agent_doc_module_decomposition.md) — preflight/sync/session_check/git/start/template left; [split fn+tests](feedback_module_split_functionality_with_tests.md); [SimWorld over mocks](feedback_simworld_over_mocks_for_sync_tmux.md).

- **[Cross-doc `--backlog-add-to` trips the coined-id guard](reference_agent_doc_cross_document_backlog_add_trips_coined_id_guard.md)** — the PreToolUse hook BLOCKS the commit; write the id without `#`.
- **[`agent:backlog` needs a `queue` attribute or nothing mirrors](reference_agent_doc_backlog_queue_attribute_required_for_any_mirror.md)** — no CLI can add one; never hand-edit the marker on a live document.
- **[`#queuelineclobber` was PREFLIGHT, not the CRDT](reference_agent_doc_preflight_drain_clear_deletes_operator_queue_text.md)** — a `re [#id]:` line parses as `Freeform`, satisfies "no prompts", and the drain blanked the WHOLE queue body; match the clobbered blob's BYTE LENGTH against `crdt_cp_write content_len=`.
- **[A supervisor that dies mid-execve pins `InFlight` forever](reference_agent_doc_inflight_recycle_wedges_dispatch_forever.md)** — every dispatch refuses with an unblocker that can never happen; pair `supervisor_recycle_graph_started`/`_settled` by `document_hash`.
- **[A SILENT editor replica held the delivery barrier forever — FIXED 6da64a97c](reference_agent_doc_silent_replica_barrier_never_releases.md)** — `delivery_converged` is AVAILABILITY, `visible_delivery_projected` is the receipt; firewall them BEFORE making the release reachable. `live_editors=1` is CORRECT here.
- **[A pasted report mentioning "a concurrent session" spawned 3 worktrees + 3 `claude -p` agents](reference_pasted_report_prose_spawns_parallel_fanout.md)** — `plan` ordered a `--mode parallel` fan-out beside its OWN `parallelizable: false`; the "tasks" were quoted diff lines. Class: guidance ordering what the receiver must not do.
- **[A barrier-released replica whose endpoint REFUSED vetoes `visible_delivery_projected` forever](reference_refused_replica_vetoes_visible_delivery_receipt.md)** — no save is ever *requested* (`request_sent=false`); the pull storm at ~4/s on a frozen hash is the tell. FIXED 0.35.418+.
- **[A /loop drain from a NON-OWNER pane is admitted, then dies at `respond`](reference_agent_doc_loop_drain_into_nonowner_pane_admits_then_fails.md)** — preflight+plan+closeout-lease all pass; check the owning pane FIRST. Never `agent-doc claim` to "fix" it, that hijacks a live session.

## agent-doc Releases

- [Release pause SUPERSEDED 2026-08-02](feedback_stop_cutting_agent_doc_releases.md) — tag push auto-publishes GitHub Release + PyPI. [0.34.x](reference_agent_doc_034x_publish_reality.md) = historical local-install-only record.
- **[ALWAYS install + recycle after any fix](feedback_always_install_and_recycle_agent_doc.md)** — never ask/defer; ≠ a release. [auto-install](feedback_supervisor_autoinstall_drives_install_not_manual.md).
- [FlowCore budget gate](reference_flowcore_token_budget_gate.md) — guard_/proof=/reason= breaks hot_path_guard. [Document is source of truth](feedback_agent_doc_document_is_source_of_truth.md) — go-mode queue IS authorization.
- **Unpublished tags**: move tag vs new version. [pyproject + VERSIONS.md first](feedback_versions_md_before_release.md); [PyPI auto via CI](reference_pypi_publish_via_ci.md).
- **cargo install + build --release after ANY edit** ([f](feedback_cargo_build_release_after_route_edit.md), [f](feedback_cargo_install_on_release.md)); [core/FFI via `lib-install`](feedback_jb_plugin_cdylib_hot_reload.md).
- [JB bump per .kt edit](feedback_jb_plugin_version_bump_per_edit.md); [build on bump](feedback_build_on_version_bump.md); [manual test first](feedback_testing_before_release.md).

## lazily / Design

- **[lazily-py imports mypyc `.so` files — kernel edits are INERT until `make compile`](reference_lazily_py_stale_so_shadows_kernel_edits.md)** — slot/cell/signal/effect/batch only; a repro can reproduce pre-fix behaviour against a 6-week-old build.

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

- **Records**: [queue-family](reference_lazily_queue_family_release_2026_07_26.md), [ReactiveMap core](reference_lazily_reactivemap_core_release_2026_07_26.md), [v2 all 8](reference_lazily_v2_release_2026_07_22.md), [Reliable Sync parity](reference_reliable_sync_full_parity_all_bindings.md), [migration](project_reactivemap_keyed_collection_migration.md). **js npm needs operator OTP.**
- **[lazily-go map data race — FIXED v0.23.2](reference_lazily_go_thread_safe_map_data_race_ci_red.md)** — every READ ran off the context lock.
- **Per-target publish**: [targets](reference_lazily_release_targets.md), [cargo token](reference_cargo_publish_token_passage.md), [py wheels](reference_lazily_py_wheels_ci.md), [coverage gen](reference_lazily_publish_and_coverage_generator.md), [js E404](reference_lazily_js_npm_trusted_publisher_e404.md), [kt](reference_lazily_kt_release_immutable_packages.md), [cpp](reference_lazily_cpp_family_features_wire_level_model.md), [zig](reference_lazily_zig_test_suite_and_coldrecalc_bottleneck.md).
- [zig master gutted std.posix](reference_zig_master_posix_file_api_moved_to_os_linux.md); **[verify ALL THREE pinned toolchains](reference_lazily_zig_verify_all_three_pinned_toolchains.md)**.
- [coverage.json is hot multi-session](reference_lazily_coverage_json_hot_multisession_file.md) — one commit per binding. [Queue `go` overrides queue:stop](feedback_queue_go_marker_overrides_stop_churn.md).
- **[Signal clause 3](reference_lazily_signal_clause3_welded_eagerness.md)** — welded eagerness into slot invalidation; go blocked on consuming cascade.
- **[4 ways to report green while testing nothing](reference_lazily_conformance_delivery_three_failure_modes.md)** — assert from what the run DID (`executed`), never what it LOADED.
- **[Corpus perturbation finds vacuity static passes cannot](reference_corpus_perturbation_finds_vacuity_static_passes_cannot.md)** — validate the detector first.
- **[A fixture cannot assert how its own step was interpreted](reference_conformance_fixture_cannot_assert_how_its_own_step_was_interpreted.md)** — a runner re-sorting `deliver.order` passes with the buffer GONE.
- **[Object-valued assertion keys live in `expect` blocks](reference_lazily_object_valued_assertion_keys_live_in_expect_blocks.md)** — never scope this audit from a grep.
- **[ci-reach pins a closure it DERIVED by awk-scanning the Makefile](reference_ci_reach_closure_pin_and_make_oracle.md)** — names-only pin describes nothing; add a make oracle. rs must NOT use anchors.
- **[A measurement must not be read from an exit status](reference_a_measurement_read_from_an_exit_status.md)** — ci-reach's `dry_run` false-greened in ALL TEN; `filter | grep -q` inverts on a MATCH, arming BELOW 64KiB.
- **[Attack the guard, do not reason about it](feedback_attack_the_guard_do_not_reason_about_it.md)** — 4 of 10 cleared it by correct reasoning, then retracted; check your instrument too.
- **[A stamp on an evidence file voids every byte-emptiness check over it](reference_stamped_evidence_defeats_byte_emptiness_checks.md)** — 9/10 still refused but ALL misdiagnosed a dead recorder (91-712 wrong findings).
- **[Rung-0 bind ledger: shapes per binding](reference_lazily_rung0_bind_ledger_shapes_per_binding.md)** — declare from the BYTES, match by CONTENT digest; never stringify.
- **[Perturbation harness per binding](reference_lazily_corpus_perturbation_harness_per_binding.md)** — kt's gradle workingDir needs `afterEvaluate`.
- **[A new canonical fixture reddens ALL NINE bindings](reference_lazily_spec_new_canonical_fixture_reddens_all_nine.md)** — budget ~10 repos; a new **step** reddens NOTHING (8/9 floors slack).
- **[A fixture can assert a correct equality CLASS without pinning the property](reference_conformance_fixture_can_assert_a_class_without_pinning_the_property.md)** — demonstrate a survivor benign, never argue it.
- **[A JSON fixture can't carry a >2^53 expectation as a number](reference_conformance_fixture_cannot_carry_its_own_expectation_as_a_json_number.md)** — wire=text/hex, expect=decimal.

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

- **All entries** (no CI, prod logs, removed stages, data traps, loan package, ops/UI, clobber-immune concurrent deploy) → [efs index](reference_efs_index.md).

## existence

- **[Ontology quote drift: PIN a verified Wayback snapshot beside the anchor, never requote](project_existence_ontology_quote_drift_pinned_to_wayback.md)** — `audit --sources --fix` searches CDX near 2015-01-01; a no-fix `quote_missing` means a person must requote.
- **[Source pins are per quote per era since existence 0.8.0 (released 2026-09-11); "split the blockquote" never worked](reference_existence_source_pins_per_quote_per_era.md)** — citation markers ignored too; a no-fix `quote_missing` means no snapshot carries it: requote by hand, verify in a scratch ontology of just the edited nodes.
- **[existence: tag push auto-publishes crates.io too](reference_existence_release_tag_autopublishes_cratesio.md)** — never `cargo publish` by hand. **[crates.io token: a `GET /api/v1/me` 403 is a FALSE NEGATIVE for a scoped publish token](reference_cargo_publish_token_passage.md)** — the stale half was the repo secret, not passage.

## tsift

- **[No harness scans `.agents/skills/` — tsift's personal-mode skill loaded nowhere](reference_agents_skills_dir_is_not_scanned_by_any_harness.md)** — `tsift status` said current because it only reads the canonical path. Fix: `tsift init --harness claude|codex|opencode|grok|all` (symlink, never copy).
- **[89% CPU in `walFindFrame` + huge `-wal` + tiny `.db` = ONE unbounded write transaction](reference_sqlite_walfindframe_thrash_unbounded_transaction.md)** — `wal_autocheckpoint` fires only on COMMIT. Fix = chunked commits, completion-marker row LAST.
- **[`~/.tsift` is tsift's OWN user state — it used to make `$HOME` a workspace root](reference_tsift_home_is_not_a_workspace_root.md)** — fixed; override `TSIFT_ALLOW_AMBIENT_ROOT=1`. **`cargo install --path packages/tsift-cli` installs `tsift-cli`, NOT `tsift`** — use `cargo install --path .`.
- [SKILL.md is primary source](feedback_tsift_skill_primary_source.md) — owns full API. **Publishing** — [a "failed" Release run may still have published crates.io/npm](reference_tsift_release_publish_list.md), **[OIDC: a green publish job proves nothing](reference_tsift_cratesio_trusted_publishing.md)**, [opencode npm](reference_opencode_tsift_npm_publish.md).

## Security

- **[passage btak/ANTHROPIC_API_KEY is an OAuth token (sk-ant-oat), NOT an API key](reference_passage_anthropic_api_key_is_an_oauth_token.md)** — 401 "API key is invalid" is a wrong-KIND credential, not a stale one; OPENAI_API_KEY also 401 (2026-09-11). Operator must mint a Console key.
- **[The publish tokens mise resolves into every agent-loop shell serve NOTHING automated](reference_ambient_publish_tokens_serve_nothing_automated.md)** — every CI path is OIDC or a repo secret; only ad-hoc `twine upload` uses them.
- **[zig `failed command:` prints only CHILD-ONLY env vars — the reported token leak did NOT reproduce](reference_lazily_zig_failed_command_dumps_env_secrets.md)** — a caller-side `env -u` scrub MANUFACTURES the diff zig prints.
- **No cleartext secrets** — livestreams; `pass`/`passage` subshell ([f](feedback_no_cleartext_secrets.md)). **Redact nested JSON** — `jq 'map_values(type)'` ([f](feedback_nested_json_redaction.md)).
- **[A `$(passage ...)` ref in a .env is NOT a secret](feedback_passage_substitution_in_env_is_not_a_secret.md)** — classify SHAPE; hash history vs the store to prove whether it's still live.
- **[REDACTED_PRODUCT]: passage [REDACTED_PRODUCT]/OPENROUTER_API_KEY**, not btak/ ([f](feedback_[REDACTED_PRODUCT]_openrouter_key.md)).

## Git Ops

- **[Shared superproject index: other panes PRE-STAGE files](reference_shared_superproject_index_contamination_commit_only.md)** — always `git commit --only -- <paths>`; **`-m` AFTER `--` commits nothing** (parsed as a pathspec) — use `-F msg.txt` before the `--`.
- **[`--only` still commits a file another session is editing](feedback_commit_one_hunk_of_a_file_another_session_is_editing.md)** — stage a hand-built blob (HEAD + your hunk), then bare `git commit`.
- **[`git commit --only` SILENTLY undoes `git rm --cached`](feedback_git_commit_only_silently_undoes_rm_cached.md)** — prove via `git ls-files`.
- **[Local `make check` green while HEAD is broken](reference_local_check_green_while_head_broken_shared_tree.md)** — your tree holds another session's uncommitted source; never commit `tests/test_cli.rs` wholesale.
- **[Concurrent `make test` deletes the nextest binary mid-run](reference_concurrent_make_test_deletes_nextest_binary_midrun.md)** — `[double-spawn] failed to exec` is a collision, not a red suite; idle-check with [`pgrep -x`, never `pgrep -f`](reference_pgrep_f_wait_loop_matches_its_own_command_line.md).
- [rebase cwd hazard](feedback_git_rebase_cwd_submodule_hazard.md); [recursive commits](feedback_recursive_submodule_commits.md) inside-out; [check working tree](feedback_check_working_tree.md).
- [RTK masks `make check` exit codes](feedback_rtk_masks_make_check_exit.md) — capture `$?` + grep `result: FAILED`. **[RTK `diff` says "identical"; `grep -rl` misses real hits](reference_rtk_diff_filter_reports_identical_for_differing_files.md)** — scan with `python3`, or run `rtk proxy <cmd>`.
- **[RTK strips Gradle task lines](reference_rtk_filters_gradle_task_lines.md)** — `UP-TO-DATE` invisible; a skipped test task reads as a passing one.
- **[Bare `cargo check` intercepted → stale "0 crates compiled"](reference_cargo_check_intercepted_returns_stale_digest.md)** — `touch` + digest-runner.
- **[Restoring a backup keeps the OLD mtime → cargo reuses the MUTATED build](reference_cargo_stale_build_when_mutation_check_restores_mtime.md)** — `touch` after each restore.
- **[A mutation script that restores with `git checkout --` reverts the fix under test](feedback_mutation_script_restore_must_not_use_git_checkout.md)** — restore from a scratch copy; assert each mutation applied; require a green M0.

## Misc
- **[A desk power reset does NOT drop the workstation — only USB re-enumerates](reference_desk_power_reset_spares_workstation_cycles_usb_only.md)** — check `uptime -s`/`last -x reboot` before assuming a reboot; extra `xinput` trackpad ids are normal.
- **[X11 wedge: climb the `unwedge` ladder](reference_x11_unwedge_ladder.md)** — rungs 0-6, works from a bare TTY; never the power button.
- **[Black bottom monitor = monitor INPUT, not X; TESmart blocks DDC/CI](reference_tesmart_kvm_blocks_ddcci_while_passing_edid.md)** — NVIDIA `/sys/class/drm` dpms/enabled is a LIE; use `nvidia-settings`.
- **[Frozen screen + stopped clock = kernel is STILL ALIVE](reference_x11_frozen_clock_kernel_stays_alive.md)** — Win+Shift+r and Ctrl+Alt+Backspace cannot work; `unwedge-watchdog` recovers with no input.
- **[Bare SAK leaves a DEAD CONSOLE](reference_sak_sigkill_leaves_console_unrepainted.md)** — SIGKILL means X never restores the video mode; always force a VT switch after.
- **[SliceMK firmware builds from JSON, no browser](reference_slicemk_configurator_build_endpoint.md)** — `slicemk-build`; builds serialise GLOBALLY, "in progress" = refused.
- **[ErgoDox cannot do Magic SysRq as mapped](reference_slicemk_ergodox_no_reachable_sysrq.md)** — PRINTSCREEN is on a held layer; use the Chicony kbd or `&panic`.
- **[TESmart KVM: port 11 DEFECTIVE, RMA'd; port 13 = known-good](reference_tesmart_kvm_reenumerates_usb_on_every_switch.md)** — KM ports drop keys for ANY keyboard; port 13 works. Return by 2026-09-19; each host needs HDMI **and** USB uplink.

- **[musl `+crt-static` emits NO cdylib](reference_musl_crt_static_emits_no_cdylib.md)** — release dir holds only `.rlib`, silently; build the lib alone with `-crt-static` into its own target-dir.
- **[Emscripten/wasm: 4 false-green traps](reference_emscripten_wasm_gotchas_lazily_family.md)** — `emcc` defaults to `-fignore-exceptions` (throw→abort); `<atomic>` is NOT threaded on wasm.
- **[A CI-generated table must not cite another job's manifest](reference_ci_matrix_must_not_depend_on_another_jobs_manifest.md)** — plus `LC_ALL=C`; `grep` under `pipefail` fails guards silently.

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

- **[Stale pane→window binding breaks editor auto-focus](reference_agent_doc_stale_pane_window_binding_breaks_autofocus.md)** — repair with `agent-doc session doctor --repair`; read window NAMES before trusting a drift bullet, and never resolve the window live for a selection effect.
- **[Preflight = 98 full CRDT materializations](reference_agent_doc_preflight_98_crdt_materializations.md)** — the whole 15-18s lag; kills the git/tmux/load hypotheses.
- **[Live pane-repro: 3 traps](reference_agent_doc_live_pane_repro_harness_gotchas.md)** — scratchpad path blows `sun_path` (controller never boots); Claude TUI keeps NO scrollback (count turns from session JSONL); `outcome=cleared` can be a false clear.
- [mag7-viewer: NO CI + browser-verify](reference_mag7_viewer_no_ci_and_browser_verify_recipe.md) — brush needs native track-drag events.
- [`.bin/agent-doc` symlink is repo-root-relative](reference_bin_agent_doc_symlink_is_reporoot_relative.md) — don't "fix". [build-worktree needs ../tmux-router](reference_agent_doc_build_worktree_needs_sibling_tmux_router.md).
- **monsterrodholders**: [SSH](reference_monsterrodholders_ssh.md), [WAF/CRS](reference_monsterrodholders_waf_933120.md), ["down"=origin cert](reference_monsterrodholders_origin_cert_expiry_outage.md), [never CSF DOCKER=1](feedback_never_set_csf_docker_1_monsterrodholders.md), [monitoring](reference_monsterrodholders_monitoring_alerting_stack.md).
- **[Disk reclaim: `rm -rf` is hook-blocked](reference_disk_reclaim_deletion_methods.md)** — `cargo clean --profile dev` / `rtk proxy find -delete`; 387G hides in `src/*/target/debug`.
