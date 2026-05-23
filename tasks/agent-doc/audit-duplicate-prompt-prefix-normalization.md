# Audit: duplicate prompt and prompt-prefix normalization paths

Date: 2026-05-23
Backlog item: `#caud`

## Scope

This audit classifies the duplicate-prompt and prompt-prefix fixes that accumulated while the repair/redelivery split was being stabilized. It follows the current split from `#rdec`, `#eprf`, `#nfix`, and `#rlog`: compute a typed repair decision, repair snapshot/disk state first, redeliver to the editor only with a non-stale bad-state proof, and prefer a narrow normalization/reposition patch before full-content redelivery.

## Classification

| Path | Classification | Disposition |
| --- | --- | --- |
| `diff::prompt_prefix_normalization_targets` and `diff::first_bare_prompt_prefix_target` | Invariant | Keep as the canonical diff-derived source of prompt-prefix targets. This prevents write, repair, and session-check from using unrelated line-shape heuristics. |
| `write::normalize_user_prompts_in_exchange` | Invariant | Keep. It diffs `snapshot -> baseline`, not `snapshot -> content_ours`, so assistant response lines added by the agent do not get prompt prefixes. |
| `write::extract_normalization_targets` | Invariant | Keep. It preserves duplicate target occurrences by position, which is required when repeated preset lines appear in the same exchange tail. |
| `write::preserve_head_exchange_prompt_prefix_state` | Invariant | Keep. It prevents `content_ours` fallback from mutating prefix state that was already committed in HEAD. |
| `write::exchange_prompt_prefix_eligible_lines` and response-label helpers | Invariant | Keep but centralize later. They encode the current ownership boundary between prompt runs and assistant response prose. |
| `write::verify_sidecar_normalization` | Invariant | Keep. Ack-content sidecars are acceptable only when every required prompt-prefix occurrence is present in the eligible exchange user region. |
| `write::normalize_exchange_prefixes_for_targets` | Invariant | Keep as the shared repair primitive for narrow editor repair and post-commit convergence. |
| `write::IpcRepairDecision` and `write::repair_ipc_decision_visible_state` | Invariant | Keep. This is the durable repair/redelivery boundary: snapshot source, disk repair reason, bad editor-state proof, normalization targets, and `redeliver_editor`. |
| `write::try_ipc_normalization_repair_patch` | Fallback | Keep as the preferred editor redelivery fallback for prefix-only sidecar divergence. It is narrower than `fullContent` and still requires the current buffer to match the bad-state proof. |
| `write::redeliver_full_content_repair_to_editor` | Fallback | Keep as last resort only. It must remain proof-gated by the rejected bad-state buffer and must not become the default repair path again. |
| `write::content_ours_merged_with_disk_edits` and `write::content_ours_with_pending_from_disk` | Fallback | Keep while sidecar ack-content can diverge. These preserve already-applied response deltas, pending mutations, and concurrent disk edits when the sidecar is rejected. |
| `write::dedupe_consecutive_response_blocks` | Fallback | Keep for now as a final duplicate-response repair guard. It should be folded into the canonical duplicate cleanup path during `#cuni`, not removed independently. |
| `write::dedupe_prompt_lines_against_before` | Fallback | Keep until `#cuni`. It repairs IPC snapshots that duplicated a live prompt already present before the patch. |
| `write::dedupe_live_prompt_prefix_variants_in_tail` | Fallback | Keep until `#cuni`. It is intentionally narrow: only the live tail after the latest boundary, only adjacent prefix variants, and it prefers the longer completed prompt. |
| `write::remove_post_exchange_duplicate_prompt_comments_with_log` | Fallback | Keep until `#cuni`/`#crem`. It repairs a historical post-exchange HTML-comment residue shape, but it should not remain a standalone cleanup branch forever. |
| `write::enforce_no_duplicate_prompt_residue` | Invariant | Keep. This is the fail-closed guard that prevents duplicate prompt text from surviving outside `agent:exchange`. |
| `write::extract_post_commit_normalization_targets` | Fallback | Keep while editor post-commit convergence still uses repair payloads. It restores prefixes that were present in the committed document but missing from the working tree. |
| `write::MAX_NORMALIZE_USER_LINES` threshold path | Obsolete workaround candidate | Replace during `#crem`. The logging signal is useful, but force-committing from inside prefix normalization is a broad recovery action now that typed repair decisions exist. |
| `sidecar_normalization_fallback*`, `ipc_repair_decision`, `*_redelivery_*`, and duplicate-count ops-log events | Logging | Keep through `#cobs`. These are the current observability hooks for proving the split is stable before reducing noise. |

## Follow-on boundaries

- `#cuni` should consolidate the duplicate-prompt fallbacks into one canonical repair/invariant path.
- `#cspe` should keep the editor specs aligned with the typed decision and narrow-repair contract.
- `#ctes` should rewrite any tests that still assume over-eager full-content redelivery.
- `#crem` should remove obsolete workaround branches after the canonical path and coverage are in place.
- `#cobs` should define when the transition logs can be reduced.
