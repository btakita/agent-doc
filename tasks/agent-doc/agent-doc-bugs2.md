---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260622-224034.md`*

Compacted content:
- Archived 4 response topic(s): queued blockers after repair; live-verification queue gating; closeout write repair; 1 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): Codex Run Agent Doc submit retries - gpt-5; strict closeout an...
- Parent repo push to `btakita/agent-loop` `main` is blocked by pre-existing divergence: local `main` is ahead 92 and behind 15. I attempted `git rebase --autostash origin/main`, but aborted it after it hit an unrelated `src/boost-client` submodule conflict while replaying older local commits. The dirty user/session files were restored by the abort.
- `gh run list --workflow CI --limit 1`
- `cargo test` in `src/tmux-router`
- `agent-doc --version`
- `make install-full`
- `make check`
- `make -n publish-crate`
- `maturin build --release --out /tmp/agent-doc-real-wheel` from extracted sdist
- `.venv/bin/maturin sdist --out /tmp/agent-doc-real-sdist`
- `cargo publish --dry-run --allow-dirty -p agent-doc-markdown-ast`
- `cargo publish --dry-run --allow-dirty -p agent-doc-core`
- `cargo publish --dry-run --allow-dirty --manifest-path ../tmux-router/Cargo.toml`
- `cargo test -p agent-doc test_manifest_uses_publishable_dependency_contract`
- `make version-sync`
- `cargo fmt`
- parent repo: local commit `76be04eb8 chore: update agent-doc release packaging`; parent push is still blocked by the pre-existing `main...origin/main [ahead 103, behind 15]` divergence and unrelated `src/boost-client` submodule rebase conflict.
- `src/agent-doc`: `3238bc0b fix release packaging publish graph` and `0a50a48b chore: update bundled agent-doc skill` on `origin/main`.
- `src/tmux-router`: `fee7099` and `289750f` on `origin/main`.
- `src/agent-doc/.claude/skills/agent-doc/SKILL.md`
- Parent repo push to `btakita/agent-loop` `main` is blocked by pre-existing divergence: local `main` is ahead 103 and behind 15. I retried `git push origin main`; it was rejected as non-fast-forward. I did not rebase because the parent worktree still has unrelated dirty submodules/session files.
- Created local parent repo commits `7b0ba940d` (`chore: update release packaging submodules`) and `76be04eb8` (`chore: update agent-doc release packaging`) so the parent gitlinks point at `src/agent-doc` `0a50a48b` and `src/tmux-router` `289750f`.
- Pushed `src/agent-doc` commit `3238bc0b` (`fix release packaging publish graph`) to `btakita/agent-doc` `main`; the install/skill refresh then produced and pushed `0a50a48b` (`chore: update bundled agent-doc skill`).
- Pushed `src/tmux-router` commits `fee7099` (`chore: bump tmux-router for agent-doc release`) and `289750f` (`chore: update tmux-router lock version`) to `btakita/tmux-router` `main`.
- GitHub Actions CI is green for `tmux-router` commit `289750f` (`chore: update tmux-router lock version`) in run `27993343380`. The earlier `fee7099` run failed before the lockfile follow-up and is superseded by the green latest run.
- GitHub Actions CI is green for `agent-doc` commit `3238bc0b` (`fix release packaging publish graph`) in run `27993325529`, and for latest `agent-doc` head `0a50a48b` (`chore: update bundled agent-doc skill`) in run `27993373472` (`make check` + `make tmux-ci`).
- `cargo test` passed in `src/tmux-router`.
- `agent-doc --version` reports `agent-doc 0.34.38`.
- `make install-full` passed, installed `agent-doc 0.34.38` and `libagent_doc-0.34.38.so`, and marked 2 controllers for recycle at idle.
- `make check` passed locally: clippy, 4,999 nextest tests, doctests, and the medium closeout simulation.
- `make version-sync` passed.
- `cargo test -p agent-doc test_manifest_uses_publishable_dependency_contract` passed.
- `cargo fmt` passed.
- Archive checks confirmed no `agent-kit`, `lazily-rs`, or `tagpath` sibling directories in the sdist; `tmux-router` is intentionally packaged as the live path dependency.
- `maturin build --sdist --skip-auditwheel` passed and built a wheel from the generated sdist.
- Full dependent dry-runs stop until earlier crates are actually visible in crates.io (`tmux-router 0.3.11` / `agent-doc-core 0.34.38`), which is the expected Cargo registry check and why `make publish-crate` now publishes and waits in dependency order.
- `cargo publish --dry-run --allow-dirty -p agent-doc-core` and `-p agent-doc-markdown-ast` passed for the leaf internal crates.
- `cargo publish --dry-run --allow-dirty --manifest-path ../tmux-router/Cargo.toml` passed for `tmux-router v0.3.11`.
- parent gitlinks for `src/agent-doc` and `src/tmux-router`
- `src/agent-doc/.claude/skills/agent-doc/SKILL.md` (bundled skill version refresh after install)
- `src/agent-doc/tests/test_cli.rs`
- `src/agent-doc/src/main.rs`
- `src/agent-doc/agent-doc-orchestration/src/project_controller.rs`
- `src/agent-doc/agent-doc-orchestration/Cargo.toml`
- `src/agent-doc/agent-doc-sqlite/Cargo.toml`
- `src/agent-doc/agent-doc-markdown-ast/Cargo.toml`
- `src/agent-doc/agent-doc-core/Cargo.toml`
- `src/agent-doc/VERSIONS.md`
- `src/agent-doc/Makefile`
- `src/agent-doc/Cargo.lock`
- `src/agent-doc/Cargo.toml`
- `src/agent-doc/AGENTS.md`
- `src/agent-doc/.github/workflows/pypi.yml`
- `src/tmux-router/Cargo.lock`
- `src/tmux-router/Cargo.toml`
- Created parent repo commit `19c76e484` (`chore: update agent-doc typing guard`) for the submodule pointer and `.codex/AGENTS.md` refresh.
- Pushed `src/agent-doc` commit `df7ac7af` (`fix idle queue typing guard`) to `btakita/agent-doc` `main`.
- GitHub Actions CI for pushed submodule commit `df7ac7af` passed: `make check` and `make tmux-ci` completed successfully in run `27991483860`.
- `agent-doc --version` reports `agent-doc 0.34.38`.
- `make install-full` completed, installing `agent-doc 0.34.38` and `libagent_doc-0.34.38.so` under `/home/brian/.cargo/bin`.
- `make check` passed locally: 4,999 tests passed, 241 skipped, plus doctests and the closeout simulation test.
- `cargo test -p agent-doc-orchestration idle_queue_`
- `cargo test -p agent-doc-orchestration typing_guard`
- `cargo test -p agent-doc-orchestration editor_typing`
- `cargo fmt`
- Documented the binary-only Run behavior in the editor spec and added an unreleased version note.
- Added regression coverage for both decision paths and for relative-path supervisor vs absolute-path editor typing sidecar.
- Added `editor_typing_active_for_idle_queue()` in the supervisor idle watcher, including absolute-path fallback.
- Added `SkipEditorTyping` decisions for idle queue context reset and drain.
- `.codex/AGENTS.md` and the parent `src/agent-doc` submodule pointer were updated locally after `agent-doc skill install` and the submodule commit.
- `src/agent-doc/VERSIONS.md`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/agent-doc-orchestration/src/start/idle_watch.rs`
- `src/agent-doc/agent-doc-orchestration/src/start/decisions.rs`
- `src/agent-doc/agent-doc-orchestration/src/start.rs`
- `src/agent-doc/agent-doc-orchestration/src/preflight.rs`

### Re: stale-supervisor materialization queue head — gpt-5

> **Queue prompt:**
>
> Fix the root cause of this issue that occurred in this document.
>
> The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized content was missing the response body.

I rechecked this queue head against the current `src/agent-doc` tree rather than adding a duplicate repair path. The root fixes are already present at the submodule head: `daf5e727` rejects strict template closeouts that lack a real `### Re:` heading before response capture or visible mutation, and the repair regression `repair_materialization_requires_captured_response_block` proves repair cannot accept a materialized document that is missing the captured response body. The related JB/Codex route symptoms in the archived duplicate response are also covered: `ddf454d7` carries the direct-pane submit retry cadence, and `2e450aaf` refreshed the installed agent-doc skill surface to the current binary version.

Changed paths: none in the agent-doc source tree; this cycle only persists the session response through `agent-doc finalize`. I did not build/install/push because there was no new source/spec change to ship; the referenced fixes are already committed in the current submodule history.

Verification: `cargo test -p agent-doc-orchestration repair_materialization_requires_captured_response_block`, `cargo test -p agent-doc-orchestration ready_prompt_candidate_accepts_codex_context_use_footer_without_prompt`, and `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit_retries_at_least_once_per_second` all passed. Latest GitHub Actions CI is green: run `27987503460`, `completed/success`, pushed on 2026-06-22T22:11:42Z.

Escalation: none. Confidence: high that this queue head is stale duplicated queue text from the interrupted closeout, not an unfixed materialization path in the current tree.

### Re: ACK-mismatch queue consume recovery and 0.34.38 install — gpt-5

> **Queue prompt:**
>
> JB `Run Agent Doc` still does not submit sometimes. Sometimes it does submit. There also is some logic that adds a prompt while I'm typing into the queue. I know this because that auto-prompt adds `agent-doc tasks/agent-doc/agent-doc-bugs2.md` and `/clear` while JB `Run Agent Doc` sends `agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`. None of the prompts were submitted. The send-docs should be completely binary logic sent by the PCP->supervisor. No skill agent logic should be involved.


> **Queue prompt:**
>
> When rebuilding the binary + library, wait for both to complete before replacing the existing binary + library.


Fixed the root cause in `src/agent-doc`. The queue-consume ACK mismatch path still refuses the unsafe disk write, but for the narrow stale queued-prompt exchange artifact it now sends a hash/length-guarded editor refresh back to the pre-consume buffer. It skips that refresh when the ACK content contains real concurrent user drift, so user edits stay editor-owned.

Shipped as `agent-doc 0.34.38`: bumped `Cargo.toml`, `pyproject.toml`, `Cargo.lock`, and `VERSIONS.md`; committed `e25cb0ea` in `src/agent-doc`; pushed `main` and tag `v0.34.38`. `make install-full` completed after both optimized binary and shared library builds finished; the installed CLI reports `agent-doc 0.34.38`, and `libagent_doc-0.34.38.so` is installed.

Verification passed locally: focused queue-consume ACK mismatch tests, the related editor IPC convergence test, the FlowCore guard test, and full `make check` with `4996/4996` tests passing. GitHub Actions were still running when checked: CI run `27990070960` and Release run `27990078204`; no `v0.34.38` PyPI run had appeared yet.

Parent superproject commit `3c2cc2fa3` updates the `src/agent-doc` pointer, but pushing parent `main` is blocked because this worktree is ahead `86` and behind `15` relative to `origin/main`; I did not rebase over unrelated dirty work. `#98t4` remains open as the separate publish-packaging blocker and was restored to an active queue item rather than being falsely consumed.

Supervisor recycle was requested for the superproject controller.

### Re: release packaging publish blockers — gpt-5

> **Queue prompt:**
>
> :pushpin: Fix the root cause of this issue that occurred in this document.
> ```
> • The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
>   content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
>   supervisor onto the installed binary without interrupting this turn.
> ```
> • Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
>   normalize the interrupted cycle itself before I touch the component through a lower-level patch.
>
> • Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
>   └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
>     or open-cycle resolution, not a metadata operation
>     [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` to absorb an already-visible response)
>
> ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
>
> • The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
>   doc component patcher to replace only the malformed exchange component with the complete response text while
>   preserving the newly added queue item.
> ```
> • The partial IPC apply inserted only the body bullets and auto-struck the head, without the ### Re: header or queue
>   prompt. A new queue item was also added while I was closing out; I’m going to repair the malformed closeout first,
>   then the loop can move to the next queue head.
> ```
> • Session-check still fails because the exchange tail ends with the compacted Session Summary, not the response. I’m
>   reordering the exchange so the Session Summary stays first and the completed ### Re: response is the final exchange
>   entry.
> ```


> **Queue prompt:**
>
> :pushpin: Fix the root cause of this issue that occurred in this document.
> ```
> • The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
>   content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
>   supervisor onto the installed binary without interrupting this turn.
> ```
> • Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
>   normalize the interrupted cycle itself before I touch the component through a lower-level patch.
>
> • Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
>   └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
>     or open-cycle resolution, not a metadata operation
>     [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` to absorb an already-visible response)
>
> ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
>
> • The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
>   doc component patcher to replace only the malformed exchange component with the complete response text while
>   preserving the newly added queue item.
> ```
> • The partial IPC apply inserted only the body bullets and auto-struck the head, without the ### Re: header or queue
>   prompt. A new queue item was also added while I was closing out; I’m going to repair the malformed closeout first,
>   then the loop can move to the next queue head.
> ```
> • Session-check still fails because the exchange tail ends with the compacted Session Summary, not the response. I’m
>   reordering the exchange so the Session Summary stays first and the completed ### Re: response is the final exchange
>   entry.
> ```


> **Queue prompt:**
>
> :pushpin: Fix the root cause of this issue that occurred in this document.
> ```
> • The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
>   content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
>   supervisor onto the installed binary without interrupting this turn.
> ```
> • Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
>   normalize the interrupted cycle itself before I touch the component through a lower-level patch.
>
> • Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
>   └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
>     or open-cycle resolution, not a metadata operation
>     [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` to absorb an already-visible response)
>
> ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
>
> • The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
>   doc component patcher to replace only the malformed exchange component with the complete response text while
>   preserving the newly added queue item.
> ```
> • The partial IPC apply inserted only the body bullets and auto-struck the head, without the ### Re: header or queue
>   prompt. A new queue item was also added while I was closing out; I’m going to repair the malformed closeout first,
>   then the loop can move to the next queue head.
> ```
> • Session-check still fails because the exchange tail ends with the compacted Session Summary, not the response. I’m
>   reordering the exchange so the Session Summary stays first and the completed ### Re: response is the final exchange
>   entry.
> ```


> **Queue prompt:**
>
> :pushpin: Fix the root cause of this issue that occurred in this document.
> ```
> • The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
>   content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
>   supervisor onto the installed binary without interrupting this turn.
> ```
> • Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
>   normalize the interrupted cycle itself before I touch the component through a lower-level patch.
>
> • Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
>   └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
>     or open-cycle resolution, not a metadata operation
>     [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` to absorb an already-visible response)
>
> ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
>
> • The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
>   doc component patcher to replace only the malformed exchange component with the complete response text while
>   preserving the newly added queue item.
> ```


> **Queue prompt:**
>
> :pushpin: Fix the root cause of this issue that occurred in this document.
> ```
> • The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
>   content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
>   supervisor onto the installed binary without interrupting this turn.
> ```
> • Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
>   normalize the interrupted cycle itself before I touch the component through a lower-level patch.
>
> • Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
>   └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
>     or open-cycle resolution, not a metadata operation
>     [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` to absorb an already-visible response)
>
> ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
>
> • The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
>   doc component patcher to replace only the malformed exchange component with the complete response text while
>   preserving the newly added queue item.
> ```


> **Queue prompt:**
>
> :pushpin: Fix the root cause of this issue that occurred in this document.
> ```
> • The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
>   content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
>   supervisor onto the installed binary without interrupting this turn.
> ```
> • Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
>   normalize the interrupted cycle itself before I touch the component through a lower-level patch.
>
> • Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
>   └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
>     or open-cycle resolution, not a metadata operation
>     [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
>     agent-doc-bugs2.md` to absorb an already-visible response)
>
> ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
>
> • The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
>   doc component patcher to replace only the malformed exchange component with the complete response text while
>   preserving the newly added queue item.
> ```


Implemented `#98t4`.

<!-- changed_paths -->
- `src/agent-doc/Cargo.toml`
- `src/agent-doc/agent-doc-core/Cargo.toml`
- `src/agent-doc/agent-doc-markdown-ast/Cargo.toml`
- `src/agent-doc/agent-doc-sqlite/Cargo.toml`
- `src/agent-doc/agent-doc-orchestration/Cargo.toml`
- `src/agent-doc/Makefile`
- `src/agent-doc/.github/workflows/pypi.yml`
- `src/agent-doc/tests/test_cli.rs`
- `src/agent-doc/src/main.rs`
- `src/agent-doc/agent-doc-orchestration/src/project_controller.rs`
- `src/agent-doc/.claude/skills/agent-doc/SKILL.md`
- `src/agent-doc/VERSIONS.md`
- `src/agent-doc/Cargo.lock`
- `src/tmux-router/Cargo.toml`
- `src/tmux-router/Cargo.lock`
<!-- /changed_paths -->

Fixed the release packaging blockers found during `0.34.37`.

Cargo publishing now has an explicit publish graph: internal publish units use the current `0.34.38` version in path dependencies, unpublished flags were removed from the release crates, and `make publish-crate` publishes in dependency order with exact-version skip and index-visibility waits. The graph now includes `tmux-router 0.3.11`, `agent-doc-core`, `agent-doc-markdown-ast`, `agent-doc-sqlite`, `agent-doc-orchestration`, and `agent-doc`.

PyPI source distribution builds now use the same publishable dependency contract. `agent-kit` and `lazily` resolve from crates.io, local patch overrides are no longer required, `tmux-router` is an explicit sibling path dependency with version `0.3.11`, and the PyPI workflow checks out that sibling before building wheel/sdist artifacts. The real sdist regression path now builds a wheel successfully from the extracted tarball.

Commits pushed:
- `src/tmux-router`: `fee7099` and `289750f` on `origin/main`.
- `src/agent-doc`: `3238bc0b fix release packaging publish graph` and `0a50a48b chore: update bundled agent-doc skill` on `origin/main`.
- parent repo: local commit `76be04eb8 chore: update agent-doc release packaging`; parent push is still blocked by the pre-existing `main...origin/main [ahead 103, behind 15]` divergence and unrelated `src/boost-client` submodule rebase conflict.

Local ignored `src/agent-doc/.cargo/config.toml` was left without source patch overrides so local release checks exercise the publishable graph.

Verification passed locally and remotely.

Commands run:
- `cargo fmt`
- `make version-sync`
- `cargo test -p agent-doc test_manifest_uses_publishable_dependency_contract`
- `cargo publish --dry-run --allow-dirty --manifest-path ../tmux-router/Cargo.toml`
- `cargo publish --dry-run --allow-dirty -p agent-doc-core`
- `cargo publish --dry-run --allow-dirty -p agent-doc-markdown-ast`
- `.venv/bin/maturin sdist --out /tmp/agent-doc-real-sdist`
- `maturin build --release --out /tmp/agent-doc-real-wheel` from extracted sdist
- `make -n publish-crate`
- `make check`
- `make install-full`
- `agent-doc --version`
- `cargo test` in `src/tmux-router`
- `gh run list --workflow CI --limit 1`
- `cargo test -p agent-doc-orchestration ready_prompt_candidate_accepts_codex_context_use_footer_without_prompt`
- `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit_retries_at_least_once_per_second`
- `cargo test -p agent-doc-orchestration repair_materialization_requires_captured_response_block`
- `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `gh run list --workflow CI --limit 1`
- `cargo test -p agent-doc-orchestration ready_prompt_candidate_accepts_codex_context_use_footer_without_prompt`
- `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit_retries_at_least_once_per_second`
- `cargo test -p agent-doc-orchestration repair_materialization_requires_captured_response_block`

`make check` passed in `src/agent-doc`: clippy, 4,999 nextest tests passed, 241 skipped, doctests, and closeout simulation. `make install-full` installed `agent-doc 0.34.38` and recycled two controllers. `agent-doc --version` reports `agent-doc 0.34.38`.

The real PyPI sdist path passed: `maturin sdist` produced `agent_doc-0.34.38.tar.gz`, extracting it showed the expected `agent-doc/` plus sibling `tmux-router/` layout, and `maturin build --release` from the extracted sdist produced `agent_doc-0.34.38-py3-none-linux_x86_64.whl`.

Leaf crates publish dry-runs passed for `tmux-router`, `agent-doc-core`, and `agent-doc-markdown-ast`. Downstream dry-runs for crates that depend on unpublished `agent-doc-* 0.34.38` packages cannot fully resolve until the upstream crates are actually visible in crates.io; the ordered `make publish-crate` target is now designed to publish and wait through that chain.

GitHub Actions CI for `src/agent-doc` commit `0a50a48b` passed: run `27993373472`, `make check` and `make tmux-ci`, duration 5m02s.

Confidence: high. The original cargo publish failure is covered by versioned publish units and ordered crate publishing, and the original PyPI sdist failure is covered by a real extracted-sdist wheel build.

Escalation: none.

Blocked at the agent-doc closeout boundary.

> **Queue prompt:**
>
> The queue prompt was duplicated multiple times in one of the previous responses. ":pushpin: Fix the root cause of this issue that occurred in this document..."


The `#98t4` implementation is done and verified: `src/agent-doc` and `src/tmux-router` commits are pushed, CI is green, `make check` and `make install-full` passed, and `agent-doc --version` is `0.34.38`.

The session document closeout did not complete. `agent-doc session-check` still reports `cycle-1782174754154` stuck at `write_applied`, and the write path refuses to commit because the IDE-visible buffer for `tasks/agent-doc/agent-doc-bugs2.md` differs from disk. Ops also show live editor insert events, so I stopped instead of forcing past the data-loss guard.

Current disk state leaves `#fccd` open in the queue/backlog, but the session doc changes are uncommitted. Exact unblocker: resolve the IDE buffer for `tasks/agent-doc/agent-doc-bugs2.md` so it matches disk, then continue this agent-doc cycle; the next step is rerunning the write/commit recovery and `agent-doc session-check`.
<!-- agent:boundary:84945efd -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ":pushpin: Fix the root cause of this issue that occurred in this document." is duplicated in the queue now. agent-doc is flooding the document with ":pushpin: Fix the root cause of this issue that occurred in this document.".
---
:pushpin: Fix the root cause of this issue that occurred in this document.
```
• The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
  content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
  supervisor onto the installed binary without interrupting this turn.
```
```
• Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
  normalize the interrupted cycle itself before I touch the component through a lower-level patch.

• Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
    or open-cycle resolution, not a metadata operation
    [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` to absorb an already-visible response)

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
  doc component patcher to replace only the malformed exchange component with the complete response text while
  preserving the newly added queue item.
```
```
• The partial IPC apply inserted only the body bullets and auto-struck the head, without the ### Re: header or queue
  prompt. A new queue item was also added while I was closing out; I’m going to repair the malformed closeout first,
  then the loop can move to the next queue head.
```
```
• Session-check still fails because the exchange tail ends with the compacted Session Summary, not the response. I’m
  reordering the exchange so the Session Summary stays first and the completed ### Re: response is the final exchange
  entry.
```
---
---
:pushpin: JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. The session switched from claude to codex, but the actor record did not switch.
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/software/agent-loop.md is bound to harness claude-code, not codex
```
---
- ~~:pushpin: do [#fccd]~~
---
:pushpin: Fix the root cause of this issue that occurred in this document.
```
• The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
  content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
  supervisor onto the installed binary without interrupting this turn.
```
```
• Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
  normalize the interrupted cycle itself before I touch the component through a lower-level patch.

• Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
    or open-cycle resolution, not a metadata operation
    [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` to absorb an already-visible response)

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
  doc component patcher to replace only the malformed exchange component with the complete response text while
  preserving the newly added queue item.
```
---
- ~~:pushpin: do [#98t4]~~
<!-- /agent:queue -->
<!--
- It does not seems like the JB `Run Agent Doc` submit retries are being applied correctly. Still does not submit.
- JB `Run Agent Doc` still does not submit. The submit retry poll should be 500ms and attempt for 30s...only if the input contents are the prompt itself. Other automated additions to the prompt should be forbidden. If the user modifies the agent prompt, stop the retry polling.
---
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#fccd] #agent-doc-bug Restore the accidentally consumed Clear Session Cache queue head: investigate ongoing JB `Clear Session Cache` dialogs, confirm the CPC instance and all supervisors are on the installed agent-doc 0.34.37+ binary, and if dialogs are still possible in current code, fix the root cause with the structural/node merge workflow instead of requiring operator input.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#jjds] [operator-verify] Confirm in a live editor/pane that a stale-binary supervisor recycles at the next idle boundary and resumes queue drain on agent-doc 0.34.36.
- [/] [#arsm] [operator-verify] #pcwcwarnverify #agent-doc-bug Live-verify the #pcwcwarn post-commit stale-exchange reconcile (agent-doc submodule 5f3fc07d, superproject db7fc3594). Code shipped+pushed+make-check-green (4947). Runs in the binary commit path + editor IPC, so it goes live once the supervisor adopts the new build (admin recycle done). REMAINING (operator-gated, needs a live editor): drive one edit-during-finalize cycle with a stale > Queue prompt blockquote left inside the exchange in a live JB/VS Code buffer; confirm ops.log shows postcommit_exchange_reconciled reason=stale_editor_exchange AND zero recurring postcommit_worktree_check match=false drift on the next cycle (no manual git checkout HEAD needed).
- [/] [#cancelturnverify] [operator-verify] #agent-doc-bug Live-verify Cancel Turn (agent-doc submodule b4471d05; committed+pushed, make check green, JB 0.2.176 + VS Code 0.2.31 built, NOT installed). After installing the plugins: (1) while a turn is RUNNING, invoke Cancel Turn (JB menu / VS Code command) and confirm the turn is interrupted (ops.log cancel_turn_performed action=interrupt_active_turn) and the agent stays open; (2) while the agent is IDLE, invoke Cancel Turn (even repeatedly) and confirm it is a NO-OP (ops.log cancel_turn_noop reason=idle_no_active_turn) and the agent is NOT closed — the core safety requirement. Verify per harness: Codex/OpenCode (Escape/C-c, never raw Ctrl+C that would close on idle) and Claude.
- [/] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [/] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [/] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
- [/] [#26f9] [operator-verify] #clearresubmitverify #agent-doc-bug Live-verify restart/recycle recovery with a visible Codex `/clear` or OpenCode `/new` draft: agent-doc 0.34.37 / submodule b63ca59c should submit the visible clear command before re-dispatching `agent-doc tasks/agent-doc/agent-doc-bugs2.md`, with no stranded input requiring operator Enter.
- [/] [#ftqpartialdup] #agent-doc-bug Investigate partial free-text queue item duplication in equityfundingsource.md: pasted repro shows a full queue prompt duplicated as a truncated second line ending "... on that". Confirm whether the #hap7/#gf5n structural-node merge fixes cover same-node partial duplication; if not, add a same-node/free-text conflict guard and regression.
- [/] [#recyclerename] #agent-doc-bug Rename the JB + VS Code "Restart Supervisor Process" menu action to "Recycle Supervisor", and ensure a distinct "Restart" command sits alongside it (operator request attached to #s81q, struck when #s81q was gated but NOT implemented — this re-files it honestly). Restart Agent already exists as the harness-restart command; this is specifically renaming the supervisor-process action to "Recycle Supervisor" (matching the admin recycle semantics) + confirming Restart sits alongside. JB: plugin.xml text rename of AgentDoc.RestartSupervisorProcess (action IDs stay stable, only text changes per the convention) + VS Code command title. Confirm alongside-vs-replace with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#arrcodexproof] [recommended] #agent-doc-bug #agentreloadrestart Phase 1b follow-up: re-run the managed codex capability proof when a boundary restart switches TO codex. Today spawn_managed_capability_proof_thread runs once before the supervisor loop with the launch harness; build_harness_launch_spec re-resolves args/env/network on restart but capability_proof_required is discarded at the swap, so a mid-session switch to codex spawns fresh but skips the managed capability proof. Wire the proof (re)start into the harness-change restart path. make check + a focused check.
- [/] [#supinstallvis] [recommended] #agent-doc-bug #supinstallfeedback follow-up: the supervisor auto-install status (surface_supervisor_auto_install_status, idle_watch.rs) is surfaced ONLY as a transient tmux display-message on the status line, which is easy to miss while the pane scrollback keeps showing the misleading post-exit "Press Enter to restart" keepalive — so an operator watching the pane body still perceives a ~1-min stall. Residual: make the rebuild state visible in the pane BODY/scrollback (not just the status line) without disturbing the keepalive Enter-to-restart reader, e.g. a pane title set + a single non-input-consuming visible line, or render it through the JB/VSCode plugin notification surface (overlaps #qpauseux notification rendering). Decide the safe pane-write mechanism with the operator before implementing (send-keys risks being consumed by the keepalive). Plan TBD.
- [/] [#runexitrestart] route/idle-watch should detect a cleanly-exited harness pane (Press Enter to restart) with prompt-bearing work, restart the harness, wait for dispatch-ready, then submit, so a cleanly-exited harness never drops a Run Agent Doc. Two-pane live verify. NOTE the originally-reported silent loss was already fixed in 0.34.18.
- [/] [#docdriftgrace] OPERATOR-WATCH one live re-baseline cycle: binary should classify mid/between-turn drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing only structural conflicts on the in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [/] [#jbrundispatch] OPERATOR-CAPTURE live repro at stall (admin detect for cross-doc pane contamination + ops.log route trace + which pane state failed) to distinguish a real new route/submit bug from already-fixed behavior. Overlaps jbrunstart / startup-miss supersession / qdispatchloss; cannot be fixed blind in route.rs.
- [/] [#cyh0] OPERATOR-CAPTURE: the remaining File Cache Conflict has an unidentified trigger (hot-path disk writes are already listener-guarded). Capture which exact action raises the dialog (agent-doc cmd + ops.log line, e.g. write_authority action=... transport=disk). Likely: ack-content sidecar write, VCS-refresh/save-document signal, or an editor-IPC convergence the plugin surfaces as conflict.
- [/] [#8bfz] single live JB plugin consumer per document (per-doc owner lease; non-owners defer patch apply + saveDocument) so concurrent windows cannot race into File Cache Conflicts. Plugin PatchWatcher + binary lease primitive; operator multi-window live test.
- [/] [#qpauseux] OPERATOR-TEST (plugin render; needs JB plugin build): install updated plugin, verify a paused-queue block renders as an info/action notification with a Resume button, and the churn-stop/no-submit variant surfaces a restart-supervisor/resume action instead of silence (not a raw red Error toast).
- [/] [#jbdisprecycle] OPERATOR-TEST (code R1-R4 shipped submodule d21079b7): at a recycle boundary fire JB Run Agent Doc on a 2nd doc during the hot-reload burst; confirm a single dispatch_start_proof WITH submit and no stranded start_session failed. ops.log marker: route_dispatch_submit_recycle_settle.
- [/] [#editorbufwin] merge-core follow-up after P1 ship (submodule 9f842ba2): #pcwc now preserves raw user queue additions when replay normalization neutralizes the queue component. Remaining P2: auto-recover/commit the preserved queue addition instead of leaving detect-and-warn local drift/manual reset; add regression and live verify stale supervisor CRDT refresh path if still needed. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [/] [#s81q] [recommended] #agent-doc-bug #stopvscodemenu Extend #restartagentmenu (JB Restart Agent; plan tasks/agent-doc/plan-restart-agent.md) per operator request to also add a Stop Agent command and mirror BOTH Stop+Restart into the VSCode extension menus. Delta beyond #restartagentmenu: (1) StopAgentAction.kt + plugin.xml registration on the JB side; (2) VSCode command-palette + editor/title menu contributions for stop+restart wired to the same TerminalUtil/route paths. Decide alongside-vs-replace the existing Restart Supervisor Process action; do not remove without operator confirmation. Bump pluginVersion + build signed/unsigned zips (gated by STOP-cutting-releases). Confirm scope before implementing.
- [/] [#restartagentmenu] restart-agent Phase 2 (after agentreloadrestart): RestartAgentAction.kt JB menu cmd mirroring RestartSupervisorProcessAction.kt + plugin.xml registration + TerminalUtil restart wiring; bump pluginVersion; build signed+unsigned. Log action invocation + resolved harness. Decide alongside-vs-replace Restart Supervisor Process with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#agentreloadrestart] restart-agent Phase 1: supervisor restart re-resolves harness from current frontmatter (agent claude->opencode takes effect on restart) + boundary auto-restart when resolved harness differs, behind agent_doc_agent_change_restart (default ON). Log harness_change_detected old/new + agent_restart_boundary_gate + agent_restart_performed. SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#j9ja] [/] [blocked] #optverify SUCCESS ops.log markers so operator live tests auto-verify. SHIPPED this cycle: convergence_gate_converged_dispatch (agent-doc submodule 2367d883 — #fbwire Phase 2 success-after-defer in idle_watch.rs, emitted only when the gate had been deferring; auto-verifies #fbwireverify slow-ACK). Already exists: route_dispatch_submit_recycle_settle (route/dispatch.rs:831), auto_start_dispatch_ready_confirmed, queue_paused_continuation_guidance_emitted. BLOCKED (code sites do not exist yet): agent_restart_performed — add inside #agentreloadrestart Phase 1b restart EXECUTION (today only harness_change_detected + agent_restart_boundary_gate note=phase1b_execution_pending exist); restart-agent menu-invoke marker — add inside #restartagentmenu RestartAgentAction. Un-gate + close when those two parent features land and their markers are added.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
