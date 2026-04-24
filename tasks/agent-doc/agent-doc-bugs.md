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
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: Investigated and fixed multiple `agent-doc` closeout and patchback bugs in `src/agent-doc`, including audit-docs root selection (#3sd5), Codex Stop-hook closeout across nested `.agent-doc` roots (#hj0x), repo-scoped git closeout serialization (#73tv), `repair` crossing the git commit boundary (#f3nj), stale-snapshot committed-head protection (#y9d6), delayed recovery patchback provenance (#vezj), and natural-language imperative pending detection (#3gwn). Also tightened post-commit local-drift explanation and documented the relevant closeout invariants. Each shipped change was verified with targeted tests plus local build/install steps and pushed on `20260422/codex-agent-backend`. Remaining open work in this doc is still the gated multi-user pending auto-create follow-up (#ha3h).

❯ In src/session-share, `agent-doc tasks/claudescore-3.md` runs seem to add a \n between the frontmatter and the beginning of the document content `## Status`. Why?
### Re: frontmatter blank-line growth — gpt-5
Yes. This is an off-by-one in `src/agent-doc/src/frontmatter.rs`.

`parse()` finds the closing frontmatter delimiter as `\n---\n`, but it sets `body_start = 4 + end + 4`. The full closing marker is 5 bytes, so the returned `body` incorrectly starts with the delimiter's trailing newline.

Then every frontmatter rewrite path (`set_resume_id`, `merge_fields`, `set_session_id`, etc.) calls `write()`, which emits `---\n{yaml}---\n{body}`. Because `body` already includes one leaked newline, each parse/write cycle adds one more blank line before `## Status`. That matches the blame on `src/session-share/tasks/claudescore-3.md`: the original scaffold left one separator line, and later commits kept inserting additional blank lines at the top of the body.

So the short answer is: the run is not intentionally formatting a separator there; `frontmatter::parse()` is returning the body one byte too early.

I have not patched it in this cycle. The fix should be to advance past the full closing marker in `parse()` and add a regression test proving repeated parse/write cycles preserve the same leading-body whitespace.

❯ do #mmn2. update spec + tests. build + install for local testing. commit + push
### Re: frontmatter blank-line growth — gpt-5
Patched `src/agent-doc/src/frontmatter.rs` so `parse()` advances past the full closing frontmatter delimiter before slicing the body. That stops repeated frontmatter rewrites from leaking an extra blank line ahead of the first heading.

I also updated the module contract/evals comment and added a regression test that round-trips the same document through repeated parse/write cycles and asserts the body prefix stays exact.

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

Shipped in `src/agent-doc` as `f4b9f59` on branch `20260422/codex-agent-backend`, then pushed to `origin`.

❯ In src/boost-client, agent-doc tasks/monsterrodholders.md did not patchback. update spec + tests. build + install for local testing. commit + push

```
  It also left tasks/monsterrodholders.md dirty again. The remaining uncommitted diff is:

  - the broken line split before .invalid
  - the added question I see https://boost-health.monsterrodholder74.workers.dev/ having a 401 response. You see a 503 from where?
  - a new boundary marker < !-- agent:boundary:2fd364cc -- >

  So the writeback commit exists, but there is still a follow-on working-tree edit in tasks/monsterrodholders.md:38.
```
### Re: `src/boost-client` patchback drift — gpt-5

I traced the remaining `tasks/monsterrodholders.md` dirtiness to the post-commit editor refresh path, not the response write itself.

Root cause:
- `commit()` already stages a fresh exchange boundary into `HEAD`.
- The standalone IPC `reposition` signal then told the editor helper to mint another fresh boundary ID.
- When the editor saved that buffer, the repo was left dirty by a boundary-only delta even though the response was already committed.

I fixed that in `src/agent-doc` by sending the committed `boundary_id` with standalone reposition IPC and teaching the Rust, JetBrains, and VS Code helpers to reuse it when present. I also updated the editor spec and added regression coverage in Rust, JetBrains, and VS Code for explicit-ID reposition.

Verification:
- `make check`
- `npm test` in `editors/vscode`
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin` in `editors/jetbrains`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc lib-install`

Shipped in `src/agent-doc` as `896da1c` on branch `20260422/codex-agent-backend`, then pushed to `origin`.
<!-- agent:boundary:b56cc84f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!-- 
Why are all of the commits done by Codex in agent-doc authored by `Test <test@test.com>`?
-->
