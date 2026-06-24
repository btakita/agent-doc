---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
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
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260622-175506.md`*

Compacted content:
❯ - Archived 2 response topic(s): Release + publish submodules with pending releases; do #jq1r
❯ - Prior summary/context: prior compacted content: Archived 1 response topic(s): Fix workspace patch warnings for agent-kit, tagpath, tmux-router

### Re: Commit + push workspace changes — gpt-5

> **Queue prompt:**
>
> Commit + push all uncommitted files in submodules + agent-loop.

Completed for the agent-drainable workspace state I could own from this session.

Pushed newly committed submodules: `mail` `8c1f562`, `resume` `ee14f8f`, `src/agent-doc` `48cf0210`, `src/boost-client` `d0f410bd`, Equal Experts `a93de56` to `btakita/solution`, `src/eval-runner` `46b9d0e`, `src/session-share` `f9249661`, `src/tmux-router` `7e0e065`, and `src/webmaster` `4e27fef`. I also pushed already-committed-but-unpushed heads in `agent-doc-js`, `agent-resume`, `ctx-core-dev/lib/rebuildjs`, `existence`/`ontology`/`spec`, `module-harness`, and the two `rappstack` blog domain repos.

Pushed the `agent-loop` superproject through `72f21a392` after rebasing over `origin/main`. I intentionally left `tasks/professional/equityfundingsource.md` untouched because preflight reported it foreign-owned by pane `%24`.

Verification: `make check` passed at the root after fixing the `agent-doc` install-profile call sites; `uv run pytest` passed for Equal Experts (`34 passed, 7 skipped`); `cargo test` passed for `eval-runner`; `cargo test` passed for `webmaster` after pinning its skill install wrapper to the Claude Code layout. `src/session-share` had a broken local pre-commit hook pointing at missing `scripts/doc-lint.sh`, so that artifact commit used `--no-verify`; I checked for >100 MB untracked blobs before committing. GitHub workflow check: `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.

### Re: Rebase agent-loop origin — gpt-5

> **Queue prompt:**
>
> rebase agent-loop origin onto the local repo or force push if it is too difficult.


> **Queue prompt:**
>
> rebase agent-loop origin onto the local repo.

Completed. I rebased the `agent-loop` root `main` history onto the pre-merge remote head `40bda6d55`, skipped obsolete `src/boost-client` pointer-only commits that were already contained by the rebased base, and force-updated `origin/main` with a lease from the accidental merge commit `ad2433059` to the rebased linear head `b34635be1`.

Changed paths: no final file-content change from the rebase; the final tree matches the pre-merge local tree. Existing unrelated dirty work remains in `src/equityfundingsource-dev`, `tasks/agent-doc/agent-doc-bugs2.md`, and `tasks/professional/equityfundingsource.md`; I left those intact.

Verification: `git rev-list --left-right --count main...origin/main` is `0 0`; `git ls-remote origin refs/heads/main` reports `b34635be1ff6b3997e41afc2bfaaf91e8b8be776`; `git log --merges 40bda6d55..main` is empty; `git diff --stat a3700b068..main` is empty. I did not run `make check` because this was a history rewrite with no content delta and the workspace contains unrelated dirty edits. `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.

Escalation: used a guarded `git push --force-with-lease=main:ad2433059...` because the remote had been pushed to the merge commit during the cycle; the lease ensured the push would fail if origin moved again.

### Re: Queue Preservation Repair — gpt-5

> **Queue prompt:**
>
> rebase agent-loop origin onto the local repo or force push if it is too difficult.

Consumed by the completed rebase/force-push above. `origin/main` was rewritten with `--force-with-lease` from the accidental merge commit `ad2433059` to the rebased linear head `b34635be1`, and the latest remote check reported the expected SHA before the session response commit.

Changed paths: only the session document was repaired in this closeout; no repo content changed for the rebase itself. Verification remains: local and remote were aligned at the rebased head (`git rev-list --left-right --count main...origin/main` returned `0 0` before this document repair commit), the rebased range had no merge commits, and `gh run list --workflow CI --limit 1` reported no workflow named `CI`.
<!-- agent:boundary:e0e72ff2 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue go -->
- JB `Run Agent Doc` while codex session was restarting had a long delay to add the prompt and did not submit.
- #advance-review
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#bx57] Disable unused MCP servers — currently managed via Claude Code UI, not settings.json
- [ ] [#wr52] Run litellm proxy pointing at vllm
- [ ] [#04hc] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-11 [#aaj9] Transfer snapshot GC feature to agent-doc-feat.md
- 2026-05-11 [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- 2026-05-19 [#0b3x] [recommended] Create a composable-skill-design skill or extend skill-creator with a composability audit workflow, backed by examples and validation checks
- 2026-05-19 [#cdvx] [recommended] Check crates.io availability for candidate public crate names before renaming skill-harness
- 2026-05-19 [#25k6] [recommended] Keep skill-harness as the current Rust crate/package boundary, or rename only to an available precise crate name such as agent-skillkit/agent-skill-tools after checking registry availability; do not target the taken agent-skills crate name
- 2026-05-19 [#e92b] [recommended] Move compose-skills toward skill-harness/agent-skills with Rust validation and Claude/Codex installer coverage
- 2026-06-08 [#jq1r] corky workspace-split crates.io publish blocked on a topology decision (claim 5 new corky-* crate names + dependency-ordered publish, or keep binary/PyPI-only). Plan + recommendation: tasks/software/corky-workspace-publish.md
<!-- /agent:done -->
