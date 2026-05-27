---
agent_doc_session: 5ea015b4-bc31-4877-b589-7c03825eff72
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: opus
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
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Create an architectural mermaid diagram of agent-doc and place in the README.md
❯ commit-push just the diagram
### Re: architecture diagram — gpt-5

Added an architectural Mermaid diagram to `src/agent-doc/README.md` under `## Architecture`.

Committed only the diagram hunk inside the `src/agent-doc` submodule as `8d88985 docs: add architecture diagram`, leaving the pre-existing README/SPEC/source edits unstaged. Also committed only the parent repo submodule pointer update as `044e647b4 docs: point agent-doc to architecture diagram`.

Verification: `git -C src/agent-doc diff --cached --check` passed for the staged diagram hunk. No code tests were run because this was a README-only diagram change.
❯ The README.md Key Features has a very long list. Can we organize the key features into a taxonomy? Should the list be so long? If yes, move the Key Feature to the bottom of the document.
❯ commit-push
### Re: README feature taxonomy — gpt-5

Reworked `src/agent-doc/README.md` so the long Key Features list is no longer the first major reader section.

I added a concise `## Feature Taxonomy` near the top, grouped around document model, write/merge pipeline, agent orchestration, tmux/editor coordination, and operational safety. The detailed `## Key Features` catalog is still intentionally long because it doubles as an operator/release capability ledger, but it now lives near the bottom of the README after Security and before License.

Committed the README-only submodule change as `6407c27 docs: organize README feature catalog`, then committed the parent repo submodule pointer as `ab5432349 docs: point agent-doc to README feature taxonomy`. Verification: `git -C src/agent-doc diff --cached --check` passed. No test suite was run because this was documentation-only.
<!-- agent:boundary:93e60b27 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
