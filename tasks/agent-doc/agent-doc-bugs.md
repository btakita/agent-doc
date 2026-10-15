---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a13cac-54da-7403-9456-6fc418818870
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then close
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261015-071654.md`*

Compacted content:
- Archived 16 response topic(s): dispatched #gh-fix 261–264 + release batch; Subagent completed; Subagent completed; 13 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): agent-doc fixes, release, and tmux auto-sync; GitHub issue 259...

### Re: queue — #gh-fix GH #266 dispatched; release + publish waits — opus-5-5 · 2026-10-15T03:21-04:00

**GH #266** (Clear Session Context reports every Claude Code `/clear` as "not proven submitted") is claimed for `subagent:gh266` and dispatched to a background subagent, in its own worktree `~/work/btakita/agent-doc-wt/gh266` on branch `fix/gh266-claude-clear-proof` cut from `origin/main`.

Here is what the issue says is wrong, and what the subagent was asked to fix:
- `context_clear_history_proves_cleared_state` refuses once the scrollback passes 200 lines. Claude Code's `/clear` leaves the scrollback in place, so for Claude Code panes the fallback in `upgrade_unobserved_clear_from_pane_history` can never pass on a real session. The `fresh_chat_visible` proof (`#clearfreshchatproof`) is gated to Codex only.
- The fix is a Claude Code cleared-state proof that looks only at the pane tail after the `/clear` line. It must still fail closed when the old conversation is still visible.
- The probe should resolve the harness the same way the supervisor does, so a Claude Code pane is not probed as `harness=default`. The subagent will also look at the `harness_queued_input` misfire.
- It will update the spec, add regression tests, run `make check`, and commit and push the branch. Integrating it, `make install`, and closing #266 stay with the coordinator.

> **Queue prompt:** release + publish

**Deferred:** this release has to include the #266 fix, so it waits until the subagent reports and the branch is merged and installed. The next queue cycle cuts it.
❯ 🚧 Run Agent Doc on sdk.md caused queue item to partially duplicate. Note the "fork" between "not to" and "not f":
```
- Does gh support 2 types of PR? One for our dev which is not to be blocked on a review...and a PR type for main which is for review?
- Does gh support 2 types of PR? One for our dev which is not f
```

### Re: v0.35.506 released; #266 closed; fork-dup + compact false positive on main — opus-5-5 · 2026-10-15T04:31-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/266

**Fixed and closed.** The fix is commit `3165b6b11d`, shipped in v0.35.506.
- **The fix:** Claude Code `/clear` is now proven from the pane tail after the last startup banner that `/clear` redraws. The proof requires the `/clear` echo, followed by nothing but prompt/status chrome and a ready prompt. It still fails closed when the old conversation is visible, and it covers blank-viewport (`pane_not_rendered`) observations.
- **Queued input:** detection reads only the live input line, so stale `Press up to edit queued messages` copies left in scrollback no longer cause a misfire.
- **Harness:** session commands resolve the real harness (document `agent:` → actor record → supervisor harness → ambient) instead of `default`.
- **Coverage:** 43 `context_clear` tests, including a long Claude pane where `/clear` ran (proven) and nine still-visible shapes (blocked). End-to-end proofs ran under the `claude-code`, `claude` and `default` labels. A read-only replay across 51 live panes found no false proof.
- **Log change:** the probe now logs `result=fresh_chat_proven`, not `cleared_state_proven`.
- GitHub issue #266 is closed with a comment.

> **Queue prompt:** release + publish

**Released v0.35.506.**
- Release commit `a470c0dfa8`, a fast-forward on `main`, tagged `v0.35.506` and pushed. The tag starts the Release and CI workflows, which publish the GitHub Release and PyPI.
- **Checks:** the full `make check` on the final version-projected tree passed: exit 0, 10575 passed, 167 skipped, no failures. The first run had failed only because `editors/release-parity.tsv` had no row for 0.35.506. I added the row and re-ran.
- **Installed:** `make install-full` exited 0, and `agent-doc --version` reports `0.35.506`.

> **Queue prompt:** do [#runsdkcaused]

**Fixed, on `main`, and installed.** This answers the report "Run Agent Doc on sdk.md caused queue item to partially duplicate". The fix is commit `9fa07acab1`; this is unreleased work on top of v0.35.506.
- **What happened:** "Run Agent Doc" fired at 07:24:25Z while you were still typing. Preflight queue maintenance found a three-way split: the baseline, the live editor at `...which is not to be b`, and disk at `...which is not f`. The editor had saved that disk copy before you deleted the `f`. The merge adopted both lines as separate additions.
- **Why both guards missed it:**
  - **Prefix guard:** it only drops a disk line that is a prefix of the editor line, and `not f` is not a prefix of `not to be b`.
  - **Keystroke-history proof (`#admissionopancestor`):** it required replaying all recorded keystrokes to land exactly on the editor text. Keystrokes are recorded one at a time, while the editor text the merge reads updates in batches, and you were still typing. So the history was a few keystrokes ahead, the replay didn't match, and the proof gave up silently.
- **The fix:** the proof now accepts the editor text at any point in the keystroke replay, not only at the end. It still drops only a line that appeared during your typing and is in neither the editor text nor the end of the replay.
- **Tests:** two merge-crate unit tests plus an end-to-end preflight test with anonymized text. All three fail when the old check is put back. The spec is updated in `specs/14-realtime-workflow.md`.
- **Left for you:** the half-typed echo is still in sdk.md (~line 1167). I left it for you; it is in `agent:todo`.

**Also fixed this cycle: closeout refused the "Compact Exchange error" queue report (`qcompactnoun`).**
- **What happened:** this turn's first `respond` was rejected with `bare compact exchange directive detected`. The cause was your queue entry "Compact Exchange error on sdk.md: …": a multi-line `---`-separated free-text head starts with plain prose, so the bullet skip missed it, and the continuation word "error" was not recognized as a report.
- **The fix:** the detector now tracks the managed list components (queue, backlog, review, todo, icebox, done) from the marker lines in each hunk, and skips every line inside them. Report nouns after the prefix (`error`, `bug`, `issue`, `failure`, …) now read as a statement about compaction, not a request.
- **Tests:** two new tests. One covers the queue entry, and checks that exchange prose after the component closes, a new hunk, and `agent:exchange` itself still count as directives. The other covers the noun report. Removing the component guard fails the test. The spec is updated in `specs/07-closeout-commands.md`. The commit is `86e24508ce`.

**Batch verification:** the batch merges `9fa07acab1` and `86e24508ce` on top of v0.35.506.
- Full `make check`: exit 0, 10580 passed, 167 skipped, no `result: FAILED`.
- Pushed to `main` as a fast-forward (`a470c0dfa8..86e24508ce`), and `make install` exited 0.

**Still in flight (claimed):**
- **`agent:todo` completion API**, plus the sdk.md todo duplicate-on-done bug (`#apimigrationcollide` / `#publicbundleadopt`): one subagent, branch `feat/todo-complete-api`. Its full `make check` is running.
- **Compact Exchange "Unknown close tag `/agent:salient-response`"**: one subagent, branch `fix/salient-response-close-tag`. Its full `make check` is running.
<!-- agent:boundary:77c3a41f -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#runsdkcaused]~~
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/266
- 🚧 Make sure agent:todo items are marked complete as they are finished. Provide a similar api as agent:backlog + agent:icebox + agent:reviews.
---
🚧 In sdk.md the "Decide the haiven-api merge order of #645 vs #715" todo item was duplicated. The duplicate was marked done but the original not marked done:
```
- [ ] Decide the haiven-api merge order of #645 vs #715 (#apimigrationcollide)
...
- [x] Decide the haiven-api merge order of #645 vs #715 (#apimigrationcollide) — moot: #715 renumbered to 00115, either order works
```
Same with:
```
- [ ] Approve/merge haiven-contracts #160 and decide the next release version bump (#publicbundleadopt)
...
- [x] Approve/merge haiven-contracts #160 and decide the next release version bump (#publicbundleadopt) — #160 merged into sdk-dev; continues as #162 below
```
---
---
🚧 Compact Exchange error on sdk.md: `agent:salient-response cycle="cycle-1792051137843"` tag in the exchange.
```
Agent Doc couldn't compact this document because a document directive is malformed.

sdk.md, line 1325
Unknown agent-doc close tag `/agent:salient-response`.

Fix the directive, then try again.
```
---
- release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

## Todo

<!-- agent:todo -->
- [ ] Next Remote Dev session: confirm tab switches move tmux (GH #262 fix, plugin 263 0.2.617 after v0.35.504 is installed on the backend)
- [ ] sdk.md cleanup: delete the leftover half-typed `> **Queue prompt:** ... which is not f` echo (~line 1167; the full question is ~line 1147), and the open `[ ]` copies of #apimigrationcollide / #publicbundleadopt whose `[x]` duplicates exist
<!-- /agent:todo -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
