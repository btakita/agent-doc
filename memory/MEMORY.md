# agent-loop Memory

## agent-doc Recovery & Dogfooding

- **Crash/handoff/spin**: [start-crash cross-root autostart](reference_agent_doc_start_crash_cross_root_submodule_autostart.md) #sync-cross-root-autostart fixed 447e6c53, [stranded handoff→IPC drift](feedback_stranded_handoff_socket_causes_ipc_snapshot_drift.md) #stuckhandoffselfheal, [controller+JB spin on corrupt HEAD](feedback_wedged_controller_and_jb_ide_spin_on_corrupt_crdt_response_in_head.md).
- **force-disk/commit repair**: [editorless resurrects deleted queue items](feedback_forcedisk_editorless_scramble_reset_fromcurrent_repair.md), [reload-lib hang→commit](feedback_reload_lib_cant_unhang_wedged_plugin_handler_writecommit_hang_recover_via_commit.md), [live editor blocks commit→reset+force-disk+commit](feedback_turn_unfinalized_live_editor_authority_blocks_commit_forcedisk_gitcommit.md), [stuck response_captured→mv cycles json](feedback_stuck_response_captured_cycle_mv_state_cycles_json.md), [hung pending-add→commit](feedback_hung_pending_add_recover_via_agent_doc_commit.md).
- **Concurrency/shared tree**: [force-push superproject→verify+STOP](feedback_concurrent_sessions_force_push_shared_superproject.md), [refactor churns tree→commit selectively, clean worktree](feedback_concurrent_agent_doc_refactor_session_build_coordination.md), [JB route-owned pane→check owner](feedback_jb_run_agentdoc_route_owned_pane_processing_prompt_collision.md).
- **Facts**: [bugs2.done.md=ARCHIVE (live agent-doc-bugs2.md)](reference_agent_doc_bugs2_done_is_archive_not_session.md), [boundary accum worktree-only](feedback_boundary_accum_worktree_only_ipc_repair_checkout_head.md) #boundaryaccum1.
- **Detailed failure/recovery pointers** (stale-binary escapes, preflight/HEAD corruption, queue/pending recovery, no_ack/dead-editor, reset/checkout-HEAD, force-disk marker/wedge, queue quirks, misc) → [recovery index](reference_agent_doc_recovery_index.md).

## agent-doc Project Intent

- [Patchboard markets FIRST; agent-doc long-term](project_product_sequencing_patchboard_first_agentdoc_later.md) — #spb1–9 planned. Signals Patchboard #spb1–#spb20 now COMPLETE (patchboard ace0852; #spbwasm iceboxed); doc tasks/software/signals-patchboard.md.
- [patchboard-app Slint zoom + drag technique](reference_patchboard_app_slint_zoom_and_drag_technique.md) — logical zoom via zpx()/no transform:scale; ancestor TouchArea for wheel-over-nodes; drag-to-connect drop hit-tested in Rust.
- [Realtime merge expertise = marketable alpha](project_realtime_merge_experience_marketable_alpha.md) — demand map in tasks/session-findings-2026-07-04.md.
- [IPC root-cause audit + #stale-plugin-detect](project_ipc_audit_and_stale_plugin_detect.md) — 5 causes A-E; patchboard NOT IPC.
- [provauth1/2/3 — 3b blocked on crdtauth6](project_provauth_series_status.md) — 1/2 DONE, 3 binary-half 0.34.57.
- [Per-cell CRDT merge isolation = deeper fix](project_agent_doc_percell_crdt_merge_intent.md) — #qcellmerge1. [[feedback_stop_cutting_agent_doc_releases]]
- [Supervisor-in-process migration (08b) CODE-COMPLETE](project_agent_doc_supervisor_migration_intent.md) — rollback flags deleted (BREAKING).
- [#fintol2/#pcwc fight carry-forward invariant](project_fintol_pcwc_carry_forward_invariant.md) — need operator go + Phase-0 re-baseline.
- [agent-doc decomposition progress](project_agent_doc_module_decomposition.md) — preflight/sync/session_check/git/start/template left.
- [Split functionality + tests together](feedback_module_split_functionality_with_tests.md)

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

## lazily / Releases

- [corky linkedin publish format](reference_corky_linkedin_publish_format.md) — mail/social/*-linkedin.md w/ frontmatter; escape little-text (FIXED e147ef9).
- [lazily release targets](reference_lazily_release_targets.md) — rs→crates.io 0.12.3; py→PyPI 0.10.2 (0.11.0 pending 2FA); js→npm; spec→Pages; zig→GitHub.
- [cargo publish token in passage](reference_cargo_publish_token_passage.md) — credentials.toml stale (403); use `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`.
- [lazily publish + coverage generator](reference_lazily_publish_and_coverage_generator.md) — js npm 2FA; kt→GitHub Packages on v* tag; `make coverage-check`.
- [lazily-zig v1.1.0 — deadlock + cold_recalc fixed](reference_lazily_zig_test_suite_and_coldrecalc_bottleneck.md) — ParkingMutex lost-wakeup fix (`swap(CONTESTED)`); `#lzedgeinline`/`#lzinline`/`#lzcachegop`; `-Dfilters`; publish=tag+`gh release`.
- [Queue `go` overrides queue:stop](feedback_queue_go_marker_overrides_stop_churn.md) — remove `go` AND set stop.

## Decision / Communication

- [Refactor-spiral pattern + guardrails](user_refactor_spiral_pattern_guardrails.md) — spike-before-commit, conviction gates, recovery-rate tripwire.
- [AskUserQuestion no-option + note](feedback_askuserquestion_no_option_confirm_actual_choice.md) — note ≠ endorsement; confirm actual choice.
- [Explain decisions plainly](feedback_explain_decisions_plainly_not_jargon.md) — plain-English + recommended default.
- [No sycophancy](feedback_no_sycophancy.md).
- [Don't oversimplify when asked "simpler"](feedback_no_oversimplify.md) — keep terse; elaborate the action.
- [Brian's Communication Priorities](user_communication_priorities.md) — bias-stripped prioritization.
- [Robert Ross closeout](project_robert_ross_engagement_status.md) — Brian fired 2026-05-27.

## equityfundingsource

- [Gold palette contrast correction](reference_efs_gold_palette_contrast_correction.md) — #C9A227→#A67C00 (split-by-surface: gold=light bg, gold-light=navy, gold-dark=text); overlay tokens; hovers deepen; color-scheme:light; live `/styleguide` route.
- [Funding-commit stage REMOVED](reference_efs_funding_commit_stage_removed.md) — no lender `POST /funds`; selected lender wires ENTIRE loan; selection emails lender. Deployed 2026-07-10 (e4127f5).
- [Legacy `applications` table collision](project_efs_legacy_applications_table.md) — apply4 uses `full_applications`; don't reuse.
- [EFS prod D1 queryable (#cftk RESOLVED)](reference_efs_d1_token_working.md) — passage token is D1-Edit scoped.
- [Closing gate REMOVED + accreditation denormalization](reference_efs_accreditation_denormalization_and_closing_gate_removed.md) — gate read stale `users.accreditation_status`. No blockers now.
- [users can't be hard-deleted — soft-delete trigger](reference_efs_users_soft_delete_trigger.md) — `UPDATE ... status='suspended'`, not DELETE.
- [SITE_URL unset in prod → use request origin](reference_efs_site_url_unset_use_request_origin.md) — `env.SITE_URL?.trim() || new URL(c.req.url).origin`; #dpkt-portal-abs-url.
- [security_events audit log ≠ admin badge feed](reference_efs_security_events_audit_log.md) — mig 0121; deduped 5min, email ≥3/24h; panel /admin/security.
- [Deal card false "pending annual requirements"](reference_efs_deal_card_pending_annual_requirements_stale_read.md) — stale `u.accreditation_status`; fix `COALESCE(acc.status,...)` (mig 0123). Deployed.
- [Loan Package (was Deal Packet) — per-lender visibility REMOVED + Message Lender](reference_efs_per_lender_deal_packet_and_message_lender.md) — RENAMED Deal Packet→Loan Package 2026-07-10 (c312d0f, mig 0128 `loan_package_links`); **per-lender document visibility REMOVED 2026-07-10 (4dab80c): all interested lenders see all Required Documents; `deal_document_lender_visibility` gate now DEAD/unread**; Message Lender + unread cursor (mig 0125). "deal package" send-workflow term still pending.

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

- [`.bin/agent-doc` symlink is repo-root-relative](reference_bin_agent_doc_symlink_is_reporoot_relative.md) — target `src/agent-doc/...`; don't "fix" to `../src/...`.
- [Verify EFS deploy via string-grep](reference_efs_verify_deploy_via_string_grep.md) — admin 401s; grep unique literals in fetched bundles.
- [Concurrent session clobbers EFS deploy](feedback_concurrent_session_clobbers_efs_worker_deploy.md) — `deploy:worker` replaces worker wholesale; verify served hash.
- [Isolated EFS deploy via clean worktree](reference_efs_isolated_deploy_clean_worktree.md) — clean worktree@HEAD `no-migrate`; RACE→commit to HEAD, deploy once, STOP.
- [Commit to HEAD to stop concurrent deploy clobber](feedback_commit_to_head_to_stop_concurrent_deploy_clobber.md) — scoped-commit my files, deploy isolated.
- [Commit only your files via temp-index + compare-and-swap](feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap.md) — read-tree HEAD→add paths→commit-tree→`update-ref main <new> <old>`.
- [Worktree as deploy AND commit source (clobber-immune)](reference_efs_worktree_as_deploy_and_commit_source_clobber_immune.md) — port edits into clean HEAD worktree, build/deploy/commit there, CAS main.
- [EFS document type catalogue locations](reference_efs_document_type_catalogue_locations.md) — new `dt_*` needs seed migration + 4 client mirrors.
- [EFS deploy from main checkout, NOT fresh-install worktree](reference_efs_deploy_from_main_checkout_not_fresh_install_worktree.md) — fresh `bun install` breaks CSS; force-rebase can DROP your commit.
- [Drive EFS portal locally to verify](reference_efs_local_portal_drive_recipe.md) — migrate:local, seed, mint `efs_session` JWT, CDP setCookie.
- [Verify EFS lender changes on PROD via curl login](reference_efs_prod_lender_login_verify_recipe.md) — curl POST /login, efs_session cookie, GET /api/investor/deals; `investment_deals.status` drives spoken-for.
- [cargo install vs workspace-patch divergence](reference_cargo_install_workspace_patch_divergence.md) — failing install often unrelated sibling work.
- [agent-doc build-worktree needs sibling ../tmux-router](reference_agent_doc_build_worktree_needs_sibling_tmux_router.md) — build-worktrees under src/, not /tmp.
- [Resume build source](reference_resume_build_source.md) — resume/ builds from data.toml via agent-resume.
- [Markdown Annotations](reference_markdown_annotations.md) — iA Writer annotation spec as prior art.
- [monsterrodholders SSH](reference_monsterrodholders_ssh.md) — `ssh monsterrodholders-server`→root@50.28.2.199, /opt/monsterrodholders/app.
- [WordPress/CRS ops → wp-liquidweb-ops skill](reference_monsterrodholders_waf_933120.md) — use skill over memory.
