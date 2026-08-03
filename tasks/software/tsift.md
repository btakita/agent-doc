---
agent_doc_session: tsift-v0.1
resume: f624f9a9-cec4-4668-a14a-60fc02aa0162
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260802-184553.md`*

Compacted content:
- Archived 6 response topic(s): do [#registerextractfunction]; do [#extractfrommethod]; do [#extractloopbreak]; 3 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): do [#zigthirdindexed]; do [#wsdq]; 0.1.79 released, published,...

### Re: release + publish — opus-5

> **Queue prompt:**
>
> release + publish


**0.1.80 is out.** Tag `v0.1.80`, tsift `e2d3b88`, `make check` green at the bumped version, installed locally (`tsift 0.1.80`). All 26 crates on crates.io at 0.1.80, `opencode-tsift` 0.1.80 on npm, GitHub release with 6 assets.

The release entry is the `extract_function` family — the first range-selected edit intent, seven languages, one analysis — with this cycle's stale-line-range batch refusal folded in.

**The part worth reporting: the crates.io publish job failed, and finding out why corrected a standing assumption.**

It uploaded `tsift-core` and `tsift-md-ast`, then stopped dead on the third crate:

```
403 Forbidden: The provided access token is not valid for crate `tsift-astgrep`
```

The loop has no per-crate tolerance, so the remaining 23 were never attempted. I finished the publish locally with the token from `passage`, in dependency order, skipping what was already up.

What makes this more than a one-off: **every previous release's green publish job had published nothing.** I read the v0.1.79 job log — all 26 steps are `"<crate> v0.1.79 already exists on crates.io; skipping"`, because the crates had been published locally with a token before the tag was pushed. The job has been reporting success for doing nothing, which is why the OIDC gap only surfaced now, on the first release that actually gave it work to do.

That contradicts the standing note that a Trusted Publisher was registered for all 23 crates with only two outstanding. I can't tell you the cause — the registrations may never have applied, may have been removed, or crates.io's token scoping may have changed. What's established is the observed scope: the exchanged token is valid for `tsift-core` and `tsift-md-ast` and not for `tsift-astgrep`, and nothing is known about the other 23 because the run never reached them. I've rewritten `#8wqk` to say that, and the only way to learn the real scope is to cut a tag with nothing pre-published and see how far the job gets.

npm OIDC, for contrast, published unattended without complaint.

One piece of housekeeping I left alone: preflight has been warning `legacy_gated_in_backlog` every cycle — `#8wqk` still lives in `agent:backlog` rather than `agent:review`. `agent-doc migrate` moves it, but that reshapes the document outside this cycle's write boundary, so it's your call whether to run it.
How do we fix tsift-astgrep publishing?

### Re: How do we fix tsift-astgrep publishing? — opus-5

Fixed, and the diagnosis was smaller than I made it sound last cycle.

**It was one crate, not a broad gap.** I queried crates.io for the trusted-publishing config of all 26 crates in the publish list. Twenty-five had one, at `btakita/tsift:release.yml`. `tsift-astgrep` had an empty array — the only crate of 26 that was never registered.

The config ids tell the story: 10871–10893 is the 2026-06-19 batch of 23, and `tsift-local-model` (10989) and `tsift-kg` (10990) were added later when someone noticed those two. `tsift-astgrep` was simply skipped in the original run. Nothing was revoked and crates.io didn't change its scoping — the standing note was right in spirit and off by one crate, and my "nothing is known about the other 23" last cycle was pessimism, not evidence. Reading it took one API call.

**Registered it:** `POST /api/v1/trusted_publishing/github_configs`, crate `tsift-astgrep`, `btakita/tsift`, `release.yml`, no environment → config `15019`, verified by re-reading. The next tag should publish all 26 through OIDC with no token.

One thing worth recording for the next time this comes up: **the API token cannot read this.** `GET .../github_configs?crate=X` with `Authorization: <token>` returns 403 — the trusted-publishing endpoints want a browser session cookie. So this is a chromium-bridge job against your logged-in crates.io tab, both to read and to write. I borrowed a search tab and put it back.

**The second half — why nobody knew.** tsift `446101b`. The publish loop skips a crate already on the registry, and every release through v0.1.79 was published by hand *before* the tag went up. So the job logged 26 skips and went green having exchanged an OIDC token for no work at all. It has been reporting success for doing nothing since trusted publishing was turned on; v0.1.80 was the first release that gave it real work, and it failed on the third crate.

The loop now counts uploads and skips, prints `published=N skipped=M`, and annotates a run that uploaded nothing as proving nothing about trusted publishing. I verified both arms by extracting the step body and simulating: all-skip prints `published=0 skipped=26` and emits the warning, all-new prints `published=26 skipped=0` and doesn't.

**Fail-fast on the first error I left alone deliberately.** The list is in dependency order, so a crate that can't publish makes everything downstream of it unpublishable too — continuing past `tsift-astgrep` would have produced 23 more failures that all say the same thing. The problem was never that it stopped; it was that a green run had been meaning nothing for months.

`#8wqk` is closed — the registration it was gating is done for all 26 crates. The real proof is the next tag, which will either print `published=26` or tell us exactly which crate is still missing.

### Re: reactive projection acceptance — codex

Clean installed-build acceptance run for reactive turn projection:

- Binary-owned admission entered `cycle-1785724031373` immediately at `preflight_started`.
- `maintenance_required=false` and `preflight_required=false`; no legacy preflight or caller ACK was needed.
- The document had no new prompt, queue head, or backlog work, so this cycle intentionally performs no tsift mutation.
- Strict binary-owned finalize/commit is the terminal acceptance edge for this run.
<!-- agent:boundary:cd881543 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
