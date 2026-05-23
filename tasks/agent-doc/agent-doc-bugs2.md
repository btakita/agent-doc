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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-012547.md`*

Compacted content:
- Archived 1 response topic(s): stale full-content IPC guard
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Why is agent-doc doing Full-content IPC replacements as I am typing in a prompt? Is there any good reason? It's causing problems that corrupt the document that cannot be fixed apparently...
❯ Note that the html comment content below this agent:exchange was once again deleted.
### Re: full-content IPC replacements — gpt-5

There is no good reason for `agent-doc` to apply a full-content editor replacement while you are typing. The only legitimate use is a recovery/re-delivery path after `agent-doc` has already repaired disk content and needs the editor buffer to catch up. That path must fail closed if the live file has drifted.

I found a real gap in the response fallback path. Operator-owned full-content IPC already used an explicit source buffer proof, but response fallback without an explicit source was proving against the buffer read immediately before delivery. If you typed after the fallback content was computed, the stale replacement could be sent with a proof for the newer live buffer, so a compliant editor plugin could still accept the stale full-content replacement.

Fixed in `src/agent-doc/src/write.rs`: response fallback full-content IPC now treats the target fallback content as the expected source when no explicit source is supplied. If the current file contains live prompt drift, the binary rejects before socket/file IPC and logs `full_content_source_buffer_rejected source=response_fallback`; the editor never receives the stale full-content payload.

I also added `response_fallback_full_content_rejects_live_drift_before_socket_delivery`, which simulates a socket listener and proves the stale response fallback is rejected before delivery, leaves the live prompt intact, saves no snapshot, and emits the rejection log. The existing preflight/comment-preservation tests still pass.

Changed paths:
- `src/agent-doc/src/write.rs`

Commands run:
- `cargo fmt`
- `cargo test response_fallback_full_content_rejects_live_drift_before_socket_delivery -- --nocapture`
- `cargo test preflight_preserves_unrelated_lines_in_mixed_post_exchange_duplicate_prompt_comment -- --nocapture`
- `cargo test preflight_waits_for_typing_before_duplicate_prompt_comment_cleanup -- --nocapture`
- `cargo test preflight_does_not_relocate_prompt_text_inside_post_exchange_html_comment -- --nocapture`
- `make check`
- `gh run view 26319677438 --json conclusion,status,jobs,url`
- `cargo install --path .`

Verification:
- `make check` passed: clippy, full non-live test suite, and medium deterministic simulator.
- Latest GitHub CI run `26319677438` is green. Its `check` job included successful `Run make check` and successful `Run make tmux-ci` steps.
- Installed local `agent-doc` binary still reports `agent-doc 0.33.16`, now built from the fixed source.

Confidence: high for the identified stale response-fallback full-content path. The source change is still uncommitted because this visible prompt did not request a repo commit or push.

Escalation: none needed.
❯ Why was the html comment below this exchange element deleted? Full document replacement?
### Re: post-exchange HTML comment deletion — gpt-5

No, this specific deletion was not the full-content IPC replacement path.

The ops log has an explicit event for this file immediately before this follow-up was handed off:

```text
post_exchange_duplicate_prompt_comment_removed file=tasks/agent-doc/agent-doc-bugs2.md source=route
```

That points at route-time duplicate prompt cleanup, specifically `scrub_duplicate_prompt_comments_for_route` in `src/agent-doc/src/route.rs`, which calls `remove_post_exchange_duplicate_prompt_comments_preserving` in `src/agent-doc/src/template.rs`.

What happened: the route path saw the ordinary HTML comment after `&lt;!-- /agent:exchange --&gt;`, found lines that looked like duplicate prompt text already represented in `agent:exchange`, and stripped those lines. Because all comment body lines were classified as duplicate prompt residue and not baseline-owned, the helper replaced the body with an empty multiline comment container:

```text
<!--
-->
```

So the full comment wrapper was supposed to be preserved, but the body was deleted by the duplicate-prompt scrubber. The current Git diff also matches that: HEAD had the empty comment body, and your current local edit re-added the hidden prompt text plus the question.

The prior full-content IPC bug was still real, but it is not the best explanation for this particular comment-body deletion. This one is the route cleanup path doing exactly the class of mutation logged as `post_exchange_duplicate_prompt_comment_removed source=route`.

Changed paths: none.

Commands run:
- `agent-doc --version`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `git diff -- tasks/agent-doc/agent-doc-bugs2.md`
- `git status --short`
- `rg -n "full_content|full-content|agent-doc-bugs2|source=response_fallback|stale|replacement" .agent-doc src/agent-doc -g '*.log' -g '*.json' -g '*.rs'`
- `rg -n "post_exchange_duplicate_prompt_comment_removed|duplicate_prompt_comment" src/agent-doc .agent-doc/logs/ops.log`
- `sed -n '1340,1510p' src/agent-doc/src/route.rs`
- `sed -n '1019,1375p' src/agent-doc/src/template.rs`
- `sed -n '1375,1515p' src/agent-doc/src/template.rs`

Verification: no code changed, so I did not run the test suite. The diagnosis is backed by the exact ops-log event and the matching route/template cleanup code.

Confidence: high that this specific deletion was route duplicate-comment cleanup, not full-content IPC replacement.

Escalation: none needed.
<!-- agent:boundary:dc59729a -->
<!-- /agent:exchange -->
###
<!--
---
Please fix. This has taken >50 turns and you still fail to fix this issue. FIX THE DANM ISSUE. Please add logging so you can detect when this issue occurs. I'm talking about the duplicate corrupt document & the duplicate prompt issue. It happened as I was typing this prompt. WTF are you doing? Not fixing the issue apparently. Just wasting tokens and time. #spec-test-build-install-commit-push
---
Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.
---
#spec-test-build-install-commit-push
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
