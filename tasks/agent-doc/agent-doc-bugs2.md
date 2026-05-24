---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-140957.md`*

Compacted content:
- Archived 1 response topic(s): full-document IPC duplicate prompt corruption
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ equityfundingsource.md had the duplicate/corrupt full-document IPC occur and the duplicate prompt occur. Typing in this prompt had the duplicate/corrupt full-document IPC occurand the duplicate prompt occur. Why a
❯ re you unable to fix these issues? Is there a multiplicity of issues with the logic flow? Please isolate each issue on the logic chain and fix. Please create a integration test that for this issue. I want this issue entirely gone. Do not stop until this issue is fixed. Your previous "high confidence" remarks are useless because the two issues still persist. 
❯ #spec-test-build-install-commit-push
### Re: duplicate IPC/prompt corruption — gpt-5

I isolated this as a chain of related but separate closeout failures, not one missing guard.

The full-document IPC side already has real guard coverage: template documents reject `fullContent` on the normal component-patch path, and the focused `cargo test full_content` suite stayed green. The live `equityfundingsource.md` failure was the later commit-closeout path: the snapshot contained one raw prompt line, while the visible file contained an adjacent `❯ `-prefixed copy plus the same raw line. The commit path deduped duplicate response blocks, but did not run prompt duplicate artifact repair before staging. It then logged positive drift from a stale length and still allowed closeout.

I fixed that chain in `src/agent-doc`:

- `src/write.rs`: added commit pre-stage prompt artifact repair against the snapshot, plus an exact adjacent prefixed/raw prompt duplicate collapse.
- `src/git.rs`: runs that repair before staging, writes repaired working-tree content atomically, updates the snapshot only when the repaired file is prompt-prefix-equivalent to the snapshot, logs `commit_pre_stage_prompt_duplicate_repaired`, and computes drift from the repaired file content.
- `SPEC.md` and `specs/07-closeout-commands.md`: documented the new snapshot-staged commit invariant.
- `VERSIONS.md`: added the unreleased change note.

❯ Please elaborate on the fixes.

Regression coverage added:

- `write::tests::commit_prompt_repair_dedupes_exact_prefixed_raw_prompt_copy`
- `git::tests::commit_repairs_prompt_prefix_duplicate_drift_before_staging`

Verification completed:

- `cargo fmt`
- `cargo test commit_prompt_repair_dedupes_exact_prefixed_raw_prompt_copy`
- `cargo test commit_repairs_prompt_prefix_duplicate_drift_before_staging`
- `cargo test duplicate_prompt`
- `cargo test full_content`
- `git diff --check`
- `make check` locally: passed, including 2320 Rust unit tests, integration suites, and the wider deterministic closeout simulation.
- `cargo install --path .`: installed local `agent-doc 0.33.16`.
- GitHub CI after push: run `26363927689` passed in 9m15s, including `Run make check` and `Run make tmux-ci`.

Commits pushed:

- `btakita/agent-doc` branch `20260422/codex-agent-backend`: `09ff45c5` (`fix: repair commit prompt duplicates`)
- parent `btakita/agent-loop` branch `main`: `15479736b` (`agent-doc: update prompt duplicate repair`)

No escalation was needed. Confidence is tied to the reproduced failure shape: the new integration test models the exact snapshot/raw plus working-tree prefixed/raw duplicate drift that occurred in `equityfundingsource.md`, and current local plus remote CI passed with the tmux leg included.
### Re: fix elaboration — gpt-5

The fix is not just a local dedupe at the symptom. It closes the route where the duplicate could still be committed after the IPC/write protections had already done their job.

The failure chain was:

1. The editor-visible document ended up with two adjacent copies of the same user prompt line: one normalized as `❯ prompt`, one still raw as `prompt`.
2. The snapshot represented the same prompt only once, but with different prompt-prefix state.
3. Commit closeout compared snapshot and working tree, saw positive length drift, logged it as possible out-of-band write, but still staged the snapshot path without running the prompt-artifact repair pipeline.
4. Existing duplicate response repair did not cover this because it only removed duplicated assistant response blocks, not adjacent prefixed/raw user prompt variants.
5. Existing full-document IPC tests stayed green because this specific bug was after IPC consumption, at snapshot-staged commit closeout.

The wide part of the fix is that commit closeout now participates in the prompt-artifact repair invariant before staging. The path is:

- `dedupe_snapshot_and_worktree_before_commit` no longer returns after response-block dedupe. It also invokes commit prompt repair against the current snapshot.
- `repair_commit_prompt_artifacts_against_snapshot` runs a narrow, commit-safe subset of prompt repair: remove duplicate prompt lines already represented by the snapshot, collapse adjacent prefixed/raw copies of the same prompt, and handle the live tail prefix-variant case.
- `dedupe_adjacent_prompt_prefix_duplicates` specifically catches the live failure shape: normalized prompt text is equal on adjacent lines and exactly one line has the `❯ ` prefix. It keeps the normalized prefixed prompt and removes the raw duplicate.
- The working tree repair uses the same atomic write helper used elsewhere in the write path, rather than a direct `std::fs::write`.
- The snapshot is updated only when the repaired working tree is prompt-prefix-equivalent to the snapshot. That keeps the commit path from absorbing arbitrary user edits while still letting prefix-only repair close cleanly.
- Drift logging now uses the repaired file length. So a repaired duplicate no longer leaves a stale `out_of_band_write` / `drift_warning` record behind.

The full-document IPC side is still intentionally constrained:

- Template/component documents prefer component patches and synthesized exchange patches instead of `fullContent`.
- Template-mode documents, legacy template/stream paths, and documents containing `agent:*` components reject full-content IPC before socket/file payload emission.
- Any remaining full-document replacement must carry source-buffer proof before editor apply.
- The `full_content` test suite covers that guard surface; it passed after the closeout fix.

The two new regression tests cover both layers of the bug:

- `write::tests::commit_prompt_repair_dedupes_exact_prefixed_raw_prompt_copy` proves the commit-safe repair function removes the adjacent raw duplicate and leaves one normalized prompt.
- `git::tests::commit_repairs_prompt_prefix_duplicate_drift_before_staging` builds the snapshot/working-tree mismatch from the real failure: snapshot has one raw prompt, working tree has `❯ prompt` plus raw `prompt`. It asserts the commit succeeds with one prompt, repairs the working tree and snapshot, logs `commit_pre_stage_prompt_duplicate_repaired snapshot_updated=true`, and does not log `out_of_band_write`.

Changed paths this cycle: none beyond this session-document response. The source/spec changes remain the prior pushed commits: `agent-doc` `09ff45c5` and parent `agent-loop` `15479736b`, with current parent `main` containing the `src/agent-doc` pointer at `09ff45c5`.

Commands this cycle: `tsift status`, `tsift index --workspace .`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, and this `agent-doc finalize` closeout. Verification this cycle is explanatory only because no source code changed; the fix being explained was already verified by local `make check`, focused regression tests, `cargo install --path .`, and GitHub CI run `26363927689` including `make tmux-ci`. Escalation: none.
❯ Please create a document that contains a mermaid with this process. Please create a documentation INDEX.md that will serve as the entry-point to the repo documentation, specs, prds(if any), etc.
### Re: documentation index and process diagram — gpt-5

Created the requested documentation entry point and Mermaid process document in the `agent-doc` submodule.

Changed paths:

- `src/agent-doc/docs/INDEX.md` - new documentation index for the repo docs, guides, references, specs, development docs, examples, ontology, and PRD status. I did not find any PRD/product-requirements files in the repo, so the PRD section explicitly records that none are currently present.
- `src/agent-doc/docs/reference/prompt-duplicate-closeout-repair.md` - new reference page with a Mermaid flowchart for the duplicate-prompt/full-content IPC closeout repair process, plus the key invariants and regression tests.
- `src/agent-doc/docs/SUMMARY.md` - links the new index and process reference into the mdBook navigation.

The Mermaid page documents the full process from user edit through preflight, component-patch/full-content scope choice, IPC materialization, duplicate-prompt repair, commit pre-stage repair, prompt-prefix-equivalent snapshot update, drift measurement, staging, commit, and fail-closed paths.

Verification:

- `git diff --check` passed for the documentation changes.
- `mdbook build` passed and wrote the HTML book locally; I removed the generated untracked `book/` directory after the build.
- `cargo run --quiet -- audit-docs` did not pass because it reports a pre-existing managed instruction-surface issue: `/home/brian/work/btakita/agent-loop/AGENTS.md` is stale and asks to run `agent-doc skill install --all` or reinstall the active harness before release. I did not change that instruction surface in this docs-only turn.

Commands used this cycle included `tsift status`, `tsift index --workspace .`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, documentation layout inspection, `git diff --check`, `cargo run --quiet -- audit-docs`, and `mdbook build`. Escalation was used only to remove the generated `book/` directory created by the docs build. Confidence is high for the requested docs because the mdBook build includes the new navigation and the new files are linked from `docs/SUMMARY.md`.
<!-- agent:boundary:4e8addae -->
<!-- /agent:exchange -->
###
<!--
Re: your chain of thought from the previous turn...I do not want "narrow fixes". I want "wide fixes" to make this issue go away. If there is blowback from restricting the intended usage of the full-document IPC, that is ok. I want the duplicate prompt + full-document IPC corruption to go away using a "wide fix". If the entire full-document IPC path needs to be effectively be disabled, do it. We can fix the repair issues later.
> • I’m going to make the commit path participate in the duplicate-prompt repair pipeline. The narrow fix is: before
  staging a snapshot, if the live file only has an extra adjacent prefixed/raw copy of prompt text already represented
  in the snapshot, repair the working tree and advance the snapshot to the same prompt-prefix-equivalent content
  instead of logging drift and leaving the duplicate behind.
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#1mm8]
- do [#hg0n]
- do [#te1d]
- do [#pmz2]
- do [#f4xr]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#rpatch] Fix response patchback recovery so stale-cycle closeout cannot report already committed while a valid visible response patchback remains uncommitted; implement tasks/agent-doc/plan-response-patchback-uncommitted.md.
- [ ] [#qresidue] [recommended] Fix inactive agent:queue residue being read as active response context; implement tasks/agent-doc/plan-drained-non-auto-queue-cleanup.md.
- [ ] [#qinactive] [recommended] Prevent edits inside inactive agent:queue blocks, such as do [#gdbpropscan], from becoming prompt targets or cross-document #agent-doc-bug backlog captures; implement tasks/agent-doc/plan-inactive-queue-residue-prompt-target.md.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
