# agent-loop Memory

## agent-doc Recovery & Dogfooding

- **Editor thrash on restart** (~1295% CPU): [JB restart thrash = 400GB reindex + leftover DEBUG log-category](reference_jb_editor_thrash_on_restart_indexing_and_debug_logcat.md) — IDE-config fixes; sample `/proc/task/*/stat` for real CPU, jstack `emitAllInternal` is the coroutine-dump not a Flow storm.
- **Crash/handoff/spin**: [start-crash cross-root autostart](reference_agent_doc_start_crash_cross_root_submodule_autostart.md), [stranded handoff→IPC drift](feedback_stranded_handoff_socket_causes_ipc_snapshot_drift.md), [controller+JB spin on corrupt HEAD](feedback_wedged_controller_and_jb_ide_spin_on_corrupt_crdt_response_in_head.md).
- **force-disk/commit repair**: force-disk wedge + queue/pending recovery procedures → see [recovery index](reference_agent_doc_recovery_index.md) (covers this category).
- **Concurrency/shared tree**: [force-push superproject→verify+STOP](feedback_concurrent_sessions_force_push_shared_superproject.md), [refactor churns tree→commit selectively](feedback_concurrent_agent_doc_refactor_session_build_coordination.md), [JB route-owned pane→check owner](feedback_jb_run_agentdoc_route_owned_pane_processing_prompt_collision.md).
- **Concurrent agent-doc DEV session stalls your closeout**: [stale binary + 100s uncommitted src/agent-doc files → editor/disk divergence, auto-recycle never idles](reference_concurrent_agentdoc_dev_session_stalls_closeout_recycle.md) — don't force finalize/write/repair/force-disk (binary refuses correctly); commit+push real work to its OWN repo remote first, then report the stuck session-doc bookkeeping and stop.
- **Facts**: [bugs2.done.md=ARCHIVE (live agent-doc-bugs2.md)](reference_agent_doc_bugs2_done_is_archive_not_session.md), [boundary accum worktree-only](feedback_boundary_accum_worktree_only_ipc_repair_checkout_head.md) #boundaryaccum1.
- **Detailed failure/recovery pointers** → [recovery index](reference_agent_doc_recovery_index.md) (stale-binary, preflight/HEAD corruption, queue/pending, no_ack, force-disk wedge, misc).
- **Stale-plugin-heartbeat wedge (IDE alive, buffer behind)**: [restart controller → force-disk → manual commit → reset](reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery.md) — auto-heal skips (IDE pid alive); corruption in live plugin, disk clean.
- **Stale-plugin delivery-ACK wedge** (finalize loops `delivery_ack_pending`, `stale_plugin` warn): [`admin reload-lib` then re-run `write --commit`](reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge.md) — response already on reliable-sync plane; NON-destructive, prefer over force-disk; escalate to make install + recycle only if it fails.
- **Recovery replay duplicates boundary marker → integrity gate wedges everything**: [2 `agent:boundary` in editor authority; gate hard-fails preflight AND session-check first, so recovery can't run](reference_agent_doc_recovery_replay_duplicate_boundary_wedges_integrity_gate.md) — response in live buffer, absent on disk; no self-heal; don't force/repair.
- **Resurrected operator-deleted queue head** (dangling `[#id]`, e.g. #sy71): [stale baseline overlay.yrs + wedged controller IPC model](reference_agent_doc_dangling_queue_head_resurrect_baseline_overlay_and_controller_model_wedge.md) — `read`=live controller model not snapshot; `reset --from-current` skips `.baselines/<h>.overlay.yrs`; `admin recycle` defers→**kill `controller serve` pid**; commit clean HEAD first.

## agent-doc Project Intent

- [Patchboard markets FIRST; agent-doc long-term](project_product_sequencing_patchboard_first_agentdoc_later.md) — Signals Patchboard #spb1–#spb20 COMPLETE (ace0852; #spbwasm iceboxed); doc tasks/software/signals-patchboard.md.
- [patchboard-app Slint zoom + drag](reference_patchboard_app_slint_zoom_and_drag_technique.md) — logical zoom via zpx(); ancestor TouchArea wheel; drag hit-tested in Rust.
- [Realtime merge expertise = marketable alpha](project_realtime_merge_experience_marketable_alpha.md) — demand map in session-findings-2026-07-04.md.
- [IPC root-cause audit + #stale-plugin-detect](project_ipc_audit_and_stale_plugin_detect.md) — 5 causes A-E; patchboard NOT IPC.
- [provauth1/2/3 — 3b blocked on crdtauth6](project_provauth_series_status.md) — 1/2 DONE, 3 binary-half 0.34.57.
- [Per-cell CRDT merge isolation = deeper fix](project_agent_doc_percell_crdt_merge_intent.md) — #qcellmerge1. [[feedback_stop_cutting_agent_doc_releases]]
- [Reliable-sync plane IS authority; sidecar = durability only](project_reliable_sync_plane_is_authority_sidecar_durability_only.md) — directive 2026-07-12: `AGENT_DOC_RELIABLE_SYNC_AUTHORITY` always true; plane primary + sidecar cold-miss backstop.
- [live_editor→reactive-family initiative](project_live_editor_reactive_family_initiative.md) — S1–S5 done; S6 sidecar-retirement DONE 2026-07-12 (authority flip + #lzsync 3B wire flip + lazily-lib mirror retirement); release still gated. [[project_reliable_sync_plane_is_authority_sidecar_durability_only]]
- [Supervisor-in-process migration (08b) CODE-COMPLETE](project_agent_doc_supervisor_migration_intent.md) — rollback flags deleted.
- [#fintol2/#pcwc fight carry-forward invariant](project_fintol_pcwc_carry_forward_invariant.md) — need operator go + Phase-0 re-baseline.
- [agent-doc decomposition progress](project_agent_doc_module_decomposition.md) — preflight/sync/session_check/git/start/template left.
- [Split functionality + tests together](feedback_module_split_functionality_with_tests.md)
- [SimWorld over mocks for sync/tmux tests](feedback_simworld_over_mocks_for_sync_tmux.md) — deterministic model + shared pure decision fn; don't touch fixed-seed fuzz.

## agent-doc Release Conventions

- [0.34.x publish reality](reference_agent_doc_034x_publish_reality.md) — local cargo install + lib-install ONLY.
- [STOP cutting releases (2026-06-20)](feedback_stop_cutting_agent_doc_releases.md) — none until per-cell CRDT fixed; confirm first.
- [CPC+Supervisor auto-install](feedback_supervisor_autoinstall_drives_install_not_manual.md) — trust preflight stale_install.
- [FlowCore token budget gate](reference_flowcore_token_budget_gate.md) — guard_/proof=/reason= breaks hot_path_guard; bump budget.
- [Document is source of truth](feedback_agent_doc_document_is_source_of_truth.md) — go-mode queue IS authorization.
- **Unpublished tags**: move tag vs new version. **pyproject.toml + VERSIONS.md** before release ([f](feedback_versions_md_before_release.md)).
- **Publish all targets**: crates.io + GitHub Release; PyPI auto via CI. [[reference_pypi_publish_via_ci]]
- **cargo install + build --release after ANY edit** ([f](feedback_cargo_build_release_after_route_edit.md), [f](feedback_cargo_install_on_release.md)).
- **Ship core/FFI via `agent-doc lib-install`** (cdylib hot-reloads by mtime) ([f](feedback_jb_plugin_cdylib_hot_reload.md)).
- **JB version bump per .kt edit** ([f](feedback_jb_plugin_version_bump_per_edit.md)); **build on bump** ([f](feedback_build_on_version_bump.md)); **manual test before publish** ([f](feedback_testing_before_release.md)).

## lazily / Design

- [Realtime + distributed primitives — 8 families](project_lazily_realtime_distributed_primitives.md) — #lztime/#lzrateshape/#lzmemb/#lzcoord/#lzpresence/#lzwindow/#lzresilience/#lzservice RELEASED 2026-07-14 (rs 0.40.0, kt 0.29.0, js 0.19.0 npm-owed, spec/formal 0.34.0); Signal demoted core→derived; py/zig/dart/go/cpp mirroring.
- [RelayCell + algebra-backed backpressure design](project_relaycell_backpressure_design.md) — **FULL 8-BINDING PARITY 2026-07-13** (rs 0.36.0, py 0.26.0, kt 0.25.0, js 0.16.0, dart 0.15.0, go v0.11.0, zig v0.20.0, cpp v0.14.0). **ONLY gap = js npm publish (USER 2FA/OIDC)**. 6 invariants; Phase 0 = reader-kinds Cell→Slot; Signal→Slot.eager.

## lazily / Releases

- **[Reliable Sync full parity — all 8 bindings](reference_reliable_sync_full_parity_all_bindings.md)** — ✅ 2026-07-12: `#lzsync`+`#sync-driver` (ResyncCoordinator/DurableOutbox/OrSet/LWW/SyncDriver + FFI kinds 4/5). Released py 0.23.0, go v0.8.0, zig v0.17.0, dart v0.12.0. [[reference_lazily_release_targets]]
- **[ReactiveMap keyed-collection migration](project_reactivemap_keyed_collection_migration.md)** — ✅ COMPLETE 2026-07-12, all 10 repos (spec v0.27.0/formal v0.28.0/rs v0.33.0/kt v0.22.0/py v0.22.0/zig v0.16.0/cpp v0.11.0/dart v0.11.0/go v0.7.0). **js 0.15.0 npm publish still owed by USER** (2FA); superproject gitlinks not bumped.
- [lazily release targets](reference_lazily_release_targets.md) — per-lang versions/targets; **py 0.21.0 = FULL parity (25 rows)**; #lzbenchver bump BENCHMARKS.md on rs release. Detail in topic file.
- [lazily-cpp family/wire model](reference_lazily_cpp_family_features_wire_level_model.md) — **v0.10.0 reliable sync (#lzsync) = FULL parity** (reliable_sync.hpp from kt; CrdtPlaneRuntime wire-level); publish=tag+`gh release`. Detail in topic file.
- [lazily-js npm Trusted Publisher E404](reference_lazily_js_npm_trusted_publisher_e404.md) — #lzzcpy shipped js 0.9.0 (isomorphic); publish blocked E404-on-PUT (OIDC not configured on npm pkg) — owner fix then rerun.
- [cargo publish token in passage](reference_cargo_publish_token_passage.md) — credentials.toml stale (403); use `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`.
- [lazily publish + coverage generator](reference_lazily_publish_and_coverage_generator.md) — js npm 2FA; kt→GitHub Packages on v* tag; `make coverage-check`.
- [lazily-kt release = immutable GitHub Packages](reference_lazily_kt_release_immutable_packages.md) — check registry before tagging (409 kills publish); **kt 0.21.0 = FULL PARITY**. CI gotcha: bundle fixtures into `src/test/resources/conformance/` (CI has no lazily-spec sibling).
- [lazily-zig test suite + coldrecalc](reference_lazily_zig_test_suite_and_coldrecalc_bottleneck.md) — publish=tag+`gh release` (no CI). **v0.14.0 FULL PARITY 32/32**; v0.15.0 ThreadSafeContext + AsyncContext(V). Detail in topic file.
- [zig master gutted std.posix/std.fs file API](reference_zig_master_posix_file_api_moved_to_os_linux.md) — use `std.os.linux` raw syscalls for fd/shm; `mise run test` on master.
- [coverage.json is a hot multi-session file](reference_lazily_coverage_json_hot_multisession_file.md) — committing it swallows other sessions' pending flips; one-commit-per-binding. [[feedback_concurrent_sessions_force_push_shared_superproject]]
- [Queue `go` overrides queue:stop](feedback_queue_go_marker_overrides_stop_churn.md) — remove `go` AND set stop.
- [lazily-dart CI missing lazily-spec sibling](reference_lazily_dart_ci_missing_spec_sibling.md) — `test` skips `../lazily-spec` → sibling-only conformance red is baseline; copy fixtures locally.

## Decision / Communication

- [Refactor-spiral pattern + guardrails](user_refactor_spiral_pattern_guardrails.md) — spike-before-commit, conviction gates, recovery tripwire.
- [AskUserQuestion no-option + note](feedback_askuserquestion_no_option_confirm_actual_choice.md) — note ≠ endorsement; confirm actual choice.
- [Explain decisions plainly](feedback_explain_decisions_plainly_not_jargon.md) — plain-English + recommended default.
- [No sycophancy](feedback_no_sycophancy.md).
- [Don't oversimplify when asked "simpler"](feedback_no_oversimplify.md) — keep terse; elaborate the action.
- [Brian's Communication Priorities](user_communication_priorities.md) — bias-stripped prioritization.

## equityfundingsource

- [Gold palette contrast correction](reference_efs_gold_palette_contrast_correction.md) — #C9A227→#A67C00 split-by-surface; `/styleguide`.
- [Funding-commit stage REMOVED](reference_efs_funding_commit_stage_removed.md) — selected lender wires ENTIRE loan; emails lender. Deployed 2026-07-10.
- [Legacy `applications` table collision](project_efs_legacy_applications_table.md) — apply4 uses `full_applications`; don't reuse.
- [EFS prod D1 queryable (#cftk RESOLVED)](reference_efs_d1_token_working.md) — passage token D1-Edit scoped.
- [Closing gate REMOVED + accreditation denormalization](reference_efs_accreditation_denormalization_and_closing_gate_removed.md) — gate read stale `users.accreditation_status`.
- [users can't be hard-deleted — soft-delete trigger](reference_efs_users_soft_delete_trigger.md) — `UPDATE ... status='suspended'`.
- [SITE_URL unset in prod → request origin](reference_efs_site_url_unset_use_request_origin.md) — `env.SITE_URL?.trim() || new URL(c.req.url).origin`.
- [security_events audit log ≠ admin badge feed](reference_efs_security_events_audit_log.md) — mig 0121; dedup 5min, email ≥3/24h.
- [Deal card false "pending annual requirements"](reference_efs_deal_card_pending_annual_requirements_stale_read.md) — stale read; `COALESCE(acc.status,...)` mig 0123.
- [Loan Package (was Deal Packet) + Message Lender](reference_efs_per_lender_deal_packet_and_message_lender.md) — mig 0128; per-lender visibility DEAD; unread mig 0125.
- [Loan Package tab REAL component + #tmo-doc-sync](reference_efs_loan_package_tab_real_component_and_tmo_doc_sync.md) — tab=`AdminLoanDocumentsManager`; mig 0129; `Sync with TMO`.
- [TMO LOS attachment/document API](reference_tmo_los_attachment_document_api.md) — `LOS.svc/Get|AddAttachment`; base64; Postman `37774064/2sAXjGcE4E`.
- [EFS repo has NO CI workflow](reference_efs_repo_has_no_ci_workflow.md) — local `make check` + verified live deploy are the only gates; source-assertion tests pin exact markup (update when you change it).

## tsift

- [opencode-tsift npm publish = OIDC (DONE)](reference_opencode_tsift_npm_publish.md) — btakita npm 2FA browser/passkey.
- [crates.io publish list completeness](reference_tsift_release_publish_list.md) — all deps in both release.yml lists; lockstep bump.
- [crates.io OIDC: 23 configured; 2 new need reg](reference_tsift_cratesio_trusted_publishing.md) — v0.1.73 fully published (25 crates).
- [SKILL.md is primary source](feedback_tsift_skill_primary_source.md) — owns full API; AGENTS.md lean.

## Security

- **No cleartext secrets** — livestreams; `pass`/`passage` subshell ([f](feedback_no_cleartext_secrets.md)).
- **[REDACTED_PRODUCT]: passage [REDACTED_PRODUCT]/OPENROUTER_API_KEY**, not btak/ ([f](feedback_[REDACTED_PRODUCT]_openrouter_key.md)).
- **Redact nested JSON** — `jq 'map_values(type)'` ([f](feedback_nested_json_redaction.md)).

## Submodule / Git Ops

- [git rebase cwd/submodule hazard](feedback_git_rebase_cwd_submodule_hazard.md) — cd to superproject root before rebase/push.
- **Recursive submodule commits** inside-out; **check working tree + diff** ([f](feedback_recursive_submodule_commits.md), [f](feedback_check_working_tree.md)).
- [RTK masks `make check` exit codes](feedback_rtk_masks_make_check_exit.md) — capture `$?` + grep `result: FAILED`.

## JetBrains Plugins (general)

- **ActionPromoter breaks intentions** in .md — self-disabling `update()`. **python-intentions verifier false positives** (bare IU lacks PythonCore) — src ~/work/btakita/python-intentions. **Build both zips**; full uninstall only for structural class changes.
- **agent-doc JB plugin** = K2/JVM21/IntelliJ 2024.2 (sinceBuild 242); consume lazily-kt via **conditional Gradle composite** (`includeBuild(...).exists()` + dependencySubstitution + `mavenLocal()` fallback). **NEVER bundle platform transitives** — `exclude(group="org.jetbrains.kotlinx")` (breaks IDE dispatcher), kotlin/annotations/jna too; verify `unzip -l <zip>`.

## Misc Conventions

- **AGENTS.md canonical** — symlink CLAUDE.md → it ([f](feedback_agents_md_canonical.md)).
- **corky `--features diarize`** ([f](feedback_corky_diarize_feature.md)); **bun for briantakita.me** ([f](feedback_bun_not_pnpm.md)); **file:// paths** ([f](feedback_file_protocol_paths.md)).
- **Writing voice**: `...` not spaced em-dash; avoid delve/leverage/hedging ([f](feedback_writing_voice.md)).
- **Verify live system after fix**, not just tests ([f](feedback_verify_live_system.md)).
- **"do" = execute ALL now** ([f](feedback_do_means_execute_all.md)); approval ≠ cut corners ([f](feedback_approval_not_skip_processes.md)); precommit testing ([f](feedback_precommit_testing.md)); no confirm for obvious iteration ([f](feedback_no_confirmation_iterate.md)).
- **No hallucinating** — "I don't know" ([f](feedback_no_hallucinate_uncertainty.md)); don't call broken "correct" ([f](feedback_no_hallucinate_correct_errors.md)); verify proxy before killing ([f](feedback_verify_proxy_before_kill.md)).
- **Distribution-aware reorg** ([f](feedback_distribution_aware_reorg.md)); [cross-crate `#[cfg(test)]`](feedback_cross_crate_cfg_test_extraction.md); **scope to session doc's project** ([f](feedback_session_document_scope_discipline.md)).

## Email Drafting (corky)

- **No auto-send** — default Gmail Drafts ([f](feedback_no_auto_send_emails.md)); **wait for complete input** ([f](feedback_wait_for_complete_input.md)).
- **Reply threading**: mail/conversations/ → in_reply_to + `Re: <subject>`; **verify facts vs mail/contacts/<name>/CLAUDE.md**.
- **Gmail filter from+to is AND** — use query OR (v0.11.5). **YouTube default public** ([f](feedback_corky_youtube_public_default.md)).

## References

- [`.bin/agent-doc` symlink is repo-root-relative](reference_bin_agent_doc_symlink_is_reporoot_relative.md) — target `src/agent-doc/...`; don't "fix".
- **EFS concurrent-deploy (clobber-immune)** — winner: [worktree as deploy AND commit source](reference_efs_worktree_as_deploy_and_commit_source_clobber_immune.md) (edits into clean HEAD worktree, build/deploy/commit there, CAS main). Verify: [string-grep bundles](reference_efs_verify_deploy_via_string_grep.md) (admin 401s). Gotcha: [deploy from main, NOT fresh-install worktree](reference_efs_deploy_from_main_checkout_not_fresh_install_worktree.md) (fresh `bun install` breaks CSS). Related: [[feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap]] [[feedback_concurrent_session_clobbers_efs_worker_deploy]].
- [EFS document type catalogue locations](reference_efs_document_type_catalogue_locations.md) — new `dt_*` = seed migration + 4 client mirrors.
- [Drive EFS portal locally](reference_efs_local_portal_drive_recipe.md) — migrate:local, seed, mint `efs_session` JWT, CDP setCookie.
- [Verify EFS lender on PROD via curl login](reference_efs_prod_lender_login_verify_recipe.md) — POST /login, efs_session cookie, GET /api/investor/deals.
- [agent-doc build-worktree needs sibling ../tmux-router](reference_agent_doc_build_worktree_needs_sibling_tmux_router.md) — under src/, not /tmp.
- [monsterrodholders SSH](reference_monsterrodholders_ssh.md) — `ssh monsterrodholders-server`→root@50.28.2.199. WordPress/CRS ops → use [wp-liquidweb-ops skill](reference_monsterrodholders_waf_933120.md).
- [monsterrodholders "down" = expired origin cert](reference_monsterrodholders_origin_cert_expiry_outage.md) — Cloudflare→origin 443 leg; certbot-renew.timer was enabled-but-never-started (inactive 98d). Fixed: renew+reload, started timer, added httpd deploy hook. HTTP works / HTTPS hangs = check origin cert expiry.
- [NEVER set CSF DOCKER=1 on monsterrodholders VPS](feedback_never_set_csf_docker_1_monsterrodholders.md) — flushes Docker NAT chain → outage (May-4 mechanism). Keep `DOCKER="0"`; a Claude session set it to 1 on 2026-07-14 11:33 → down. Verify via `check-firewall-stack.sh`.
- [monsterrodholders monitoring+alerting stack](reference_monsterrodholders_monitoring_alerting_stack.md) — boost-health worker: 5-min public probe + email/SMS via CF `send_email` (NO MX change), `/alert` relay, `?selftest=1`; csf-drift-check cron auto-reverts DOCKER=1 + pages. Recipients brian(email/vtext)+Ron(att, pending verify). Worker/script deployed but UNCOMMITTED.
