---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: codex
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
---



## Status

<!-- agent:status patch=replace -->

✅ Verified and landed delayed-recovery patchback provenance in `src/agent-doc`. Durable captures now preserve whether a response only reached the document/commit boundary later via recovery replay, instead of flattening that history into a generic final `committed` state.
The latest `src/agent-doc` change is built, installed locally, committed, and pushed. Remaining open work in this bug doc is again only the gated multi-user pending auto-create policy follow-up (`#ha3h`).

In src/session-share, `agent-doc tasks/claudescore-3.md` runs seem to add a \n between the frontmatter and the beginning of the document content `## Status`. Why?
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: Investigated and fixed multiple `agent-doc` closeout and patchback bugs in `src/agent-doc`, including audit-docs root selection (#3sd5), Codex Stop-hook closeout across nested `.agent-doc` roots (#hj0x), repo-scoped git closeout serialization (#73tv), `repair` crossing the git commit boundary (#f3nj), stale-snapshot committed-head protection (#y9d6), delayed recovery patchback provenance (#vezj), and natural-language imperative pending detection (#3gwn). Also tightened post-commit local-drift explanation and documented the relevant closeout invariants. Each shipped change was verified with targeted tests plus local build/install steps and pushed on `20260422/codex-agent-backend`. Remaining open work in this doc is still the gated multi-user pending auto-create follow-up (#ha3h).
<!-- agent:boundary:abf70992 -->
<!-- /agent:exchange -->

<!--
In src/boost-client, agent-doc tasks/monsterrodholders.md did not patchback. update spec + tests. build + install for local testing. commit + push

```
  It also left tasks/monsterrodholders.md dirty again. The remaining uncommitted diff is:

  - the broken line split before .invalid
  - the added question I see https://boost-health.monsterrodholder74.workers.dev/ having a 401 response. You see a 503 from where?
  - a new boundary marker < !-- agent:boundary:2fd364cc -- >

  So the writeback commit exists, but there is still a follow-on working-tree edit in tasks/monsterrodholders.md:38.
```
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!-- 
Why are all of the commits done by Codex in agent-doc authored by `Test <test@test.com>`?
-->
