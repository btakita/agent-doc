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

Logged three fresh log-triage follow-ups. No open backlog items.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260522-233540.md`*

Compacted content:
- Archived 1 response topic(s): compact prompt duplication
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. tsift.md had the duplicate prompt issue. The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?
❯ #spec-test-build-install-commit-push
### Re: mixed scratch comment deletion — gpt-5
Yes, line-by-line was the right way to look at this one. This specific deletion was not just a generic race symptom: the duplicate-prompt cleanup was applying a fuzzy whole-comment match to a multiline ordinary HTML comment. When the comment contained one duplicate prompt line plus unrelated scratch/log-triage lines, the whole comment body could be emptied.

I changed the cleanup to scrub multiline post-exchange comments line-by-line. A one-line duplicate comment can still be emptied, but unrelated lines mixed into the same multiline comment now remain user-owned and outside `agent:exchange`.

Changed paths:
- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/specs/04-diff-computation.md`
- `src/agent-doc/VERSIONS.md`

Verification:
- Reproduced the failing editor-visible mixed-comment case before the fix.
- `cargo test normalize_editor_visible_template_structure_preserves_mixed_prompt_comment_scratch_lines`
- `cargo test preflight_preserves_unrelated_lines_in_mixed_post_exchange_duplicate_prompt_comment`
- `make check`
- Latest GitHub CI run `26316547978` was green, including `Run make tmux-ci`.
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`

Commits so far:
- `src/agent-doc`: `078c6981 fix: preserve mixed prompt scratch comments`
- parent repo gitlink: `774467f59 chore: update agent-doc submodule`

Push is intentionally after this `finalize` closeout so the session-document response commit is included in the same push boundary.
<!-- agent:boundary:242725bc -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
