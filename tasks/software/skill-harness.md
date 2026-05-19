---
agent_doc_session: a1a5456d-df6d-4c12-bbf9-f25dcf3811f1
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
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
❯ Add #next-steps re: composable skills...which was recently added via agent-loop.md
### Re: compose-skills next steps — gpt-5

I would treat the recent `agent-loop.md` thread as already having completed the first implementation step: `compose-skills` now exists in `skill-harness`, the validator is Rust-based, and Claude/Codex directory install coverage exists.

The next useful layer is lifecycle hardening:

1. Make the bundled `src/skill-harness/skills/compose-skills/` tree the explicit source of truth, then add a directory-aware check/sync command so installed `.claude/skills/compose-skills` and `.codex/skills/compose-skills` copies can be verified byte-for-byte against it.

2. Expand validator coverage from the happy-path example into negative and boundary fixtures: missing required sections, invalid candidate names, no candidate skill entries, duplicate/overlapping candidates, and a real agent-doc decomposition plan.

3. Broaden install coverage past Claude and Codex. OpenCode and the generic `.agent/skills` layout are the next targets because they prove this is portable skill lifecycle tooling, not a Codex-only prototype.

4. Keep the package name `skill-harness` for now, but sharpen the docs around what "harness" means: install, validate, sync, test, and compose portable skills across agent runtimes.

I added the first three as backlog items here. I did not treat the frontmatter prompt preset definitions as commands to commit or push; they are preset definitions, while the exchange request was specifically for `#next-steps` on composable skills.
<!-- agent:boundary:bf93cd21 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#b4ps] [recommended] Make src/skill-harness/skills/compose-skills the canonical source and add a directory-aware check/sync path so Claude and Codex installs can be verified against the bundled skill tree.
- [ ] [#jw8x] [recommended] Add compose-skills validation fixtures that cover malformed plans, ambiguous one-skill-vs-many splits, and a real agent-doc workflow decomposition example.
- [ ] [#nztw] [recommended] Extend skill-harness install coverage beyond Claude/Codex for compose-skills, prioritizing OpenCode and the generic .agent/skills layout.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
