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

❯ Please edit all commits authored by `Test <test@test.com>` to be authored by `Brian Takita <brian.takita@gmail.com>`. force push
### Re: commit author rewrite — gpt-5
Rewrote the `src/agent-doc` history on `20260422/codex-agent-backend` so every commit in `origin/main..HEAD` that still used `Test <test@test.com>` is now authored and committed as `Brian Takita <brian.takita@gmail.com>`.

Verification:
- `git -C src/agent-doc rev-list --count --author='Test <test@test.com>' origin/main..HEAD` returned `0`
- force-pushed the rewritten branch with `git -C src/agent-doc push --force-with-lease origin HEAD:20260422/codex-agent-backend`
- new branch tip: `49e75f9`

❯ In src/session-share, `agent-doc tasks/claudescore-3.md` had the following in the Claude Code session. Please explain what `[recover] Response already present in document — skipping apply, cleaning up pending file` means? Is there an issue with the run state?
```
● Bash(agent-doc preflight tasks/claudescore-3.md)
  ⎿  [preflight] step 0: layout check
     [preflight] step 1: recover
     [recover] Response already present in document — skipping apply, cleaning up pending file
```

### Re: recover dedup message — gpt-5
That log line means `preflight` found a saved pending/captured response from an earlier cycle, checked the current document before replaying it, and saw that the response text was already present. So it deliberately did **not** apply the response a second time, and instead cleaned up the stale pending/capture state.

In `src/agent-doc/src/recover.rs`, that is the `is_already_applied(...)` dedup guard returning true. The usual shape is: the prior run got far enough that the editor/file write succeeded, but the process was interrupted before cleanup fully finished, so the pending file or durable capture was left behind.

So: this does indicate the **previous cycle did not finish its bookkeeping cleanly**, but it does **not** mean the document is corrupted or that the current run state is bad. In the normal healthy recovery path, `preflight` sees that leftover state, skips double-applying the response, clears the stale pending file, updates cycle state, and then continues.

I would treat the snippet you pasted as a safe self-heal, not as an active bug by itself. It becomes a real problem only if you see it repeatedly for the same document without the cycle closing afterward, or if it is followed by `session-check` / commit failures.

Concretely:
- Safe/expected after interruption: response is already visible in the doc, and `preflight` or `session-check` finishes cleanly.
- Worth investigating: the message appears over and over for the same cycle, pending files keep reappearing, or the next steps fail to reach `committed`.

❯ This "self-heal" runs regularly. Can we address the root cause that forces the "self-heal" to the document, snapshots, and other agent-doc bookkeeping is in a consistent state?
### Re: repeated recover self-heal root cause — gpt-5
Yes. I patched the root cause in `src/agent-doc`.

The issue was not that the document or snapshot was inconsistent after a clean closeout. The issue was that `recover` treated any cycle-addressable durable capture as replayable, even when that capture was already in a terminal state like `Committed`.

That meant a later `preflight` could still do this:
- load the committed capture via the cycle state's `capture_id`
- enter `recover`
- notice the response was already in the document
- emit the dedup/self-heal message and clean up again

So the repeated self-heal was largely a recovery-gating bug, not evidence that the document/snapshot/HEAD state was still wrong.

I changed `src/agent-doc/src/recover.rs` so recovery now ignores terminal durable-capture states (`Committed`, `Discarded`) unless there is still an actual pending response file to reconcile. In other words: later preflights no longer try to "recover" captures that have already crossed the closeout boundary.

I also added regression coverage for the exact shape: committed capture, no pending file, later `preflight`/`recover` run should return `Noop` instead of taking the dedup path. I updated `src/agent-doc/VERSIONS.md` to record the behavior change.

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

I did **not** commit or push this `src/agent-doc` change in this cycle.

❯ Please rename the `recover` method to `repair`. Make sure all docs and specs are in sync. update spec + tests. build + install for local testing. commit + push
### Re: `recover` -> `repair` rename — gpt-5
Renamed the primary repair surface in `src/agent-doc` so `repair` is now the canonical command/module/logging path and `recover` remains only as the legacy CLI alias.

Shipped changes:
- renamed `src/recover.rs` to `src/repair.rs`, updated the Rust call sites, and switched the primary command branch / outcome type to `repair`
- updated the command/spec/doc surfaces (`README.md`, `CLAUDE.md`, `specs/07-commands.md`, `docs/reference/reactive-stream.md`, `examples/agent-doc/task.md`, `VERSIONS.md`) so `repair` is the primary term and `recover` is explicitly documented as compatibility-only
- tightened CLI coverage so `--help` asserts the canonical `repair` surface while the alias test still covers `recover`

Verification:
- `make check`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc --version`
- `agent-doc repair --help`

Committed and pushed in `src/agent-doc` as `4f4a21d` on `20260422/codex-agent-backend`.

❯ Please reconcile `20260422/codex-agent-backend` so it can be merged into the `main` branch. I prefer to cherry-pick or rebase onto main. A force push is on the table.
### Re: branch reconciliation — gpt-5
I fetched `origin` in `src/agent-doc` and checked `20260422/codex-agent-backend` against `origin/main`.

Result:
- `git rev-list --left-right --count origin/main...HEAD` returned `0 46`, so the branch is 46 commits ahead of `origin/main` and 0 behind.
- `git rebase origin/main` was a no-op: `Current branch 20260422/codex-agent-backend is up to date.`
- `origin/20260422/codex-agent-backend` already matches local `HEAD` at `4f4a21d`.

So there was nothing to rewrite. The branch is already reconciled onto the current `main` tip and should merge as a fast-forward from the Git graph's perspective. No force-push was needed, and I did not make or push any new commits in this cycle.

❯ Please merge 20260422/codex-agent-backend into main.
### Re: merge `20260422/codex-agent-backend` into `main` — gpt-5
Merged it in `src/agent-doc`.

What I did:
- fetched `origin`
- checked out local `main`
- fast-forward merged `origin/20260422/codex-agent-backend` into `main`
- pushed `main` to `origin`
- checked back out to `20260422/codex-agent-backend`

Result:
- `git push origin main` updated `origin/main` from `cf56772` to `4f4a21d`
- local `main`, `origin/main`, and `20260422/codex-agent-backend` now all resolve to `4f4a21d`

So the branch is merged, and `main` now contains the full `20260422/codex-agent-backend` history.

❯ Please clean up the git history to exclude any committed .agent-doc file. No .agent-doc should be in the history.
### Re: remove `.agent-doc` from `src/agent-doc` history — gpt-5
Done in `src/agent-doc`.

I rewrote the git history to strip the tracked `.agent-doc` runtime files out of every reachable ref, removed the local `refs/original` backups, expired reflogs, ran GC, and force-pushed the rewritten refs back to `origin`. `.gitignore` already had `.agent-doc/`, so the remaining fix was historical cleanup rather than a working-tree ignore change.

This touched many descendant refs, not just the recent releases, because the polluted commits were in shared ancestry.

Verification:
- `git -C src/agent-doc rev-list --all -- .agent-doc` now returns nothing
- local and remote `main` / `20260422/codex-agent-backend` now resolve to `bcc1044`
- `20260421/release-0.33.10` now resolves to `4ae8620`
- rewritten release tags:
  - `v0.33.10` -> `c4fb197`
  - `v0.33.11` -> `4ae8620`
  - `v0.33.12` -> `b2c4379`
  - `v0.33.13` -> `d9f27ce`

Any other clone of `btakita/agent-doc` now needs a fresh `git fetch --prune --tags` and then a reset/rebase onto the rewritten refs, or a fresh clone, before pushing more work.

❯ In src/boost-client, `agent-doc tasks/monsterrodholders.md` response patchback does not have a `❯ ` prefix for user prompts. See my inquiry:
```
• Because I treated the patchback as an assistant-only response block, not as a transcript entry.

  The local evidence shows two different patterns:

  - agent-doc capture response_body values are response-only and do not include ❯.
  - The archived/baseline session docs do use ❯  to mark user prompts inline in the transcript.

  I followed the first pattern while writing directly into tasks/monsterrodholders.md, but that was the wrong fit for
  this document. In this file’s exchange history, ❯  is part of the transcript format, so omitting it made the
  patchback inconsistent with the surrounding record.

  So the short answer is: I excluded it because I was patching “response text” rather than reconstructing the full
  prompt/response transcript. That was a format mismatch on my side, not an intentional convention change. If you want,
  I can normalize the recent entries to use the same ❯  prompt style as the rest of the archived exchange.
```
### Re: `Test <test@test.com>` commit authorship — gpt-5

The short answer is: those commits were created while `git` thought the author/committer identity was `Test <test@test.com>`. Codex did not invent that name on its own.

What I checked in this workspace:
- `git -C src/agent-doc config --show-origin --get user.name` returns `Brian Takita`
- `git -C src/agent-doc config --show-origin --get user.email` returns `brian.takita@gmail.com`
- global git config is the same
- there are no live `GIT_AUTHOR_*` / `GIT_COMMITTER_*` env vars in this session
- the `Test` strings inside `src/agent-doc` are in test setup/helpers that create isolated throwaway repos, not in the production commit path

So the evidence points to a historical config/environment issue at commit time: either the repo/global git config was `Test <test@test.com>` then, or the harness shell exported that identity for those commit commands. The commits now in history still preserve that metadata.

One concrete sign of that: current `src/agent-doc` history still has recent real commits like `5484c1b` authored and committed as `Test <test@test.com>`, even though the live config is now Brian. So the earlier author rewrite fixed a range of history, but it did not eliminate the source that kept producing new `Test`-authored commits afterward.

If you want, I can do the next step in a follow-up cycle:
1. trace where that historical identity was being injected
2. lock `src/agent-doc` to an explicit local `user.name` / `user.email`
3. rewrite the remaining `Test`-authored commits again

❯ Why did you not address `❯ In src/boost-client, `agent-doc tasks/monsterrodholders.md` response patchback does not have a `❯ ` prefix for user prompts. See my inquiry:`?

❯ Is the contents of this agent-doc confusing you?
### Re: missed `src/boost-client` question — gpt-5

No, the document is not confusing me. I missed that prompt.

The failure mode was simpler than that: I answered the newest explicit question about `Test <test@test.com>` and treated it as the active turn, but I did not also close the earlier unresolved `src/boost-client` prompt that was still present immediately above it in the exchange.

So the bug was in my turn handling, not in your document:
- the `❯ In src/boost-client ... does not have a ❯ prefix ...` prompt was still unanswered in this session doc
- I should have answered that first, or answered both in the same cycle
- instead I jumped to the later authorship question and left the earlier thread dangling

My earlier explanation inside the quoted inquiry was also not sufficient as a session response, because it was just quoted material you pasted into the document, not an actual `### Re:` response from me in this exchange.

So the right answer is: yes, I overlooked an outstanding question in the transcript. That was my mistake, not a formatting problem in your `agent-doc` file.

If you want the substantive follow-up on that `src/boost-client` `❯`-prefix issue, I can answer it directly in the next cycle and keep it scoped to whether the bug is in transcript reconstruction, patchback formatting, or document normalization.

❯ Why were you confused? The prompt was in the agent-doc diff. There seems to be a root cause to this problem. This is a bug.
### Re: root cause of the missed prompt — gpt-5

Yes. This is a bug.

The root cause was not that the prompt was absent from the diff. The root cause was that I used the wrong turn-selection rule when reading the document delta.

What happened:
- the diff contained multiple unresolved user-authored items in the same exchange tail
- I latched onto the most recent explicit question (`Test <test@test.com>`) as if it were the whole turn
- I failed to run the stricter check that asks: "are there any earlier unresolved prompts in the newly changed exchange that still require a `### Re:` response?"
- because of that, I left the earlier `src/boost-client` `❯`-prefix question unanswered even though it was plainly present in the diff

So the bug is basically: latest-question selection overrode unresolved-prompt reconciliation.

A correct `agent-doc` response pass should do both of these before answering:
- scan the changed exchange region for all newly introduced or still-unanswered user prompts
- only treat the turn as complete once each of those prompts is either answered or explicitly grouped into one response

I did not do that here. So your diagnosis is right: this was a response-completeness bug, not a missing-context problem.

If you want, the next step is to treat this as a real `agent-doc` product bug and specify the invariant more concretely: pre-response validation should fail if the changed exchange tail still contains an unanswered user prompt ahead of the candidate response target.
❯ Yes. Specify the invariant more concretely. update spec + tests. build + install for local testing. commit + push
### Re: response-completeness invariant — gpt-5
I tightened the invariant at the prompt-contract layer in `src/agent-doc`, because that is the layer where this bug is actually enforceable without pretending the binary can semantically prove a freeform answer covered every question.

Concrete invariant now:
- resumed turns restate ordered `required response targets` extracted from user request blocks in the diff
- that list is oldest-first, so the agent sees the full changed exchange tail instead of anchoring only on the newest question
- the prompt explicitly says the turn is incomplete until each listed target is answered or explicitly grouped into one response

What changed:
- added diff-side extraction/formatting helpers in `src/diff.rs` for ordered required-response blocks
- updated `run` and `stream` prompt builders to inject that section on resumed turns
- updated `SKILL.md`, `SPEC.md`, and `specs/05-agent-backend.md` so the instruction/spec surface matches the binary behavior
- added regression coverage for multi-prompt tails, code-fence preservation, prompt rendering, and bundled skill text

Verification:
- `make check`
- `cargo build --release`
- `cargo install --path .`

Committed and pushed in `src/agent-doc` as `38942ad` on `20260422/codex-agent-backend`.
❯ Would the new invariant also cover inline prompts? The diff is the prompt. The surrounding content, using the markdown AST, is important for provide context for inline prompts. Can we have the invariant ensure that all inline prompts are applied as prompts?

❯ We should also have a special process for editing content in the agent doc. The agent should address the change and figure out whether it is prompt or an edit. The edit could be correcting a previous proposal or fixing a typo. The edit could also be caused by an agent-doc turn bug where part or all of the response is uncommitted.
### Re: inline prompts and agent-doc edits — gpt-5
Yes, but I would specify it one level higher than "inline prompts."

Right now `agent-doc` has two separate concepts:
- ordered `required response targets` for explicit prompt blocks in the diff
- `inline_annotations` for some user edits inside prior agent text

That split is why the current invariant does **not** fully guarantee that every prompt-bearing inline edit is treated as a prompt. It only guarantees explicit prompt-block coverage, plus a narrower side-channel for some inline edits.

The stronger invariant should be:
- every user-authored change in the changed exchange/document context is classified as one of: `prompt_target`, `content_edit`, `structural_edit`, `recovery_artifact`, or `boundary_artifact`
- the turn is incomplete until every `prompt_target` is answered or explicitly grouped into one response
- `content_edit` is not auto-answered as a new prompt; it becomes the new source of truth the agent must reconcile against
- `recovery_artifact` / `boundary_artifact` must be normalized by the binary and must not be promoted into prompt targets unless residual user-authored content remains after normalization

So yes: inline prompts should be covered, but the spec should say **prompt-bearing inline changes**, not all inline changes. A typo fix, a wording correction, and a partially-uncommitted response repair are different classes and should not all be forced through the same response path.

I also agree that the diff alone is not enough for this classification. The diff is the trigger, but the surrounding markdown structure is the context. For inline edits, the classifier should look at where the change landed: exchange prose, blockquote, list item, heading, code fence, component boundary, or edited prior response text. That is the level where `agent-doc` can distinguish "this is a new question inserted into prior prose" from "this is a correction to existing content" or "this is a closeout artifact."

The processing order I would specify is:
1. normalize known binary artifacts first
2. classify remaining user-authored changes with markdown-context awareness
3. emit ordered response targets for every `prompt_target`
4. surface `content_edit` items as edits the agent must incorporate, not necessarily answer separately
5. if an edit looks like a missed/uncommitted response, route through repair/session-check logic before treating it as ordinary conversation

So the root change I would want in the spec is: replace the current split between "required response targets" and "inline annotations" with one unified `user-authored prompt-bearing change` contract, then let prompt-vs-edit-vs-artifact be explicit subtypes under that contract.

❯ do #3xmb. update spec + tests. build + install for local testing. commit + push
### Re: unified prompt-bearing change classifier — gpt-5
Shipped `#3xmb` in `src/agent-doc`.

What changed:
- replaced the split between explicit required-response targets and `inline_annotations` with a canonical `prompt_bearing_changes` classifier in `src/diff.rs`
- added typed kinds: `prompt_target`, `content_edit`, `recovery_artifact`, and `boundary_artifact`
- updated preflight, run/stream prompt builders, backend prompt instructions, and the spec/skill/docs surfaces so inline prompt-bearing edits and response-artifact triage use the same contract

Verification:
- `make check`
- `cargo build --release`
- `cargo install --path .`

Committed and pushed in `src/agent-doc` as `3fb65d8` on `20260422/codex-agent-backend`.

❯ In src/session-share, the `agent-doc tasks/claudescore-3.md` response had recommendations but did not add the recommendations 
```
❯ Why did you not add all of the actionalbe recommendations into agent:pending?

● You're right — the report has 7 concrete, actionable recommendations and the pending capture rule requires tracking
  those. Let me fix that now.
```
### Re: pending capture enforcement — opus-4-6

The agent failed to add 7 actionable recommendations to `agent:pending` during the `finalize` call in `tasks/claudescore-3.md`. The prompt contract says to capture these, but there's no binary-level enforcement — only LLM memory, which is unreliable for this pattern.

**Spec written:** [`tasks/agent-doc/spec-pending-capture-guard.md`](spec-pending-capture-guard.md)

The design adds a post-commit heuristic to `session-check` that detects uncaptured recommendations in the committed response:

- **Detection patterns:** priority labels (P0/P1/P2), numbered action lists with imperative verbs, recommendation headers, imperative sentences near "recommend" keywords
- **Guard behavior:** warn by default (stderr, exit 0), strict mode via frontmatter `pending_capture_guard: strict` (exit 1)
- **Suppression:** `<!-- no-pending-capture -->` in the response for intentional skips (e.g., mutually exclusive options)
- **Threshold:** only triggers when `estimated_count >= 2` and `confidence >= 0.5` to avoid noise on single action items

Implementation touches: `session_check.rs` (guard logic), `cycle_state.rs` (track `had_pending_mutations`), `write.rs` (set flag on pending-add), `config.rs` (parse guard setting), new `heuristics.rs` module (isolated detection logic). 9 test cases specified.

The guard goes in `session-check` rather than `finalize` so the agent gets a self-correction window — it can add pending items via a follow-up `write --commit --pending-add` in the same turn.

❯ do #pb76. update spec + tests. build + install for local testing. commit + push
### Re: pending capture guard — gpt-5

Shipped `#pb76` in `src/agent-doc`.

What changed:
- added a pending-capture guard to `session-check`, backed by a new `heuristics.rs` classifier and cycle-state tracking for `--pending-add` / `--pending-add-gated`
- added `pending_capture_guard` support in document frontmatter plus project `.agent-doc/config.toml` `[guards] pending_capture = "warn|strict|off"`
- updated the shipped docs/spec surface in `src/agent-doc` (`specs/07-commands.md`, `SKILL.md`, `VERSIONS.md`) and refreshed the local task spec at `tasks/agent-doc/spec-pending-capture-guard.md`
- added regression coverage for heuristic detection, warn/strict behavior, suppression, frontmatter-overrides-project precedence, and the new cycle-state/config parsing

Verification:
- `make check`
- `cargo build --release`
- `cargo install --path .`

Shipped in `src/agent-doc` as `6fa4c8e` on `20260422/codex-agent-backend`, then pushed to `origin`.

❯ In src/boost-client, `agent-doc tasks/monsterrodholders.md` response patchback does not have a `❯ ` prefix for user prompts. Please fix. See my inquiry:
```
• Because I treated the patchback as an assistant-only response block, not as a transcript entry.

  The local evidence shows two different patterns:

  - agent-doc capture response_body values are response-only and do not include ❯.
  - The archived/baseline session docs do use ❯  to mark user prompts inline in the transcript.

  I followed the first pattern while writing directly into tasks/monsterrodholders.md, but that was the wrong fit for
  this document. In this file’s exchange history, ❯  is part of the transcript format, so omitting it made the
  patchback inconsistent with the surrounding record.

  So the short answer is: I excluded it because I was patching “response text” rather than reconstructing the full
  prompt/response transcript. That was a format mismatch on my side, not an intentional convention change. If you want,
  I can normalize the recent entries to use the same ❯  prompt style as the rest of the archived exchange.
```
### Re: `src/boost-client` `❯` prompt prefix normalization — gpt-5

Fixed the specific transcript issue in `src/boost-client/tasks/monsterrodholders.md`.

I normalized the four recent user prompt lines in the exchange tail so they now use the same `❯ ` prefix as the surrounding session history:
- `❯ do #h7cq. commit + push` (both occurrences)
- `❯ I see https://boost-health.monsterrodholder74.workers.dev/ having a 401 response. You see a 503 from where?`
- `❯ What needs to be done with https://boost-health.monsterrodholder74.workers.dev/ to make it work for the monit job? Are using the proper auth headers + post data?`

I did not change the surrounding answer text, pending list, or any `agent-doc` product code in this cycle. This was a document-format repair in an already-dirty session file, not a shared formatter change. The `src/boost-client` repo still has uncommitted local changes, and I did not commit or push there in this cycle.

❯ Please analyze how the agent-doc harness + deterministic binary needs to change to fix the monsterrodholders.md not having a `❯ ` issue.
### Re: `monsterrodholders.md` `❯` prefix root cause — gpt-5

This needs two changes: one in the harness, one in the deterministic binary.

Harness side:

The `monsterrodholders.md` failure came from a bypassed repair/writeback path. The live file shows direct document prose like "I wrote this response directly into the task doc instead" plus status-only narration mixed into `agent:exchange`, which means the harness fell out of the normal `agent-doc finalize` / `agent-doc write --commit` path and reconstructed exchange text manually.

That should fail closed. In Codex/manual repair, once the user prompt already exists in the document, the only allowed response path should be `agent-doc write --commit <FILE>` or the normal `agent-doc finalize <FILE> ...`, followed immediately by `agent-doc session-check <FILE>`. No direct assistant patchback into `exchange`.

Binary side:

`src/agent-doc/src/diff.rs:339-410` and `:512-614` already classify ordered `prompt_target` blocks deterministically. But `src/agent-doc/src/write.rs:578-760` still restores `❯ ` only from the `snapshot -> baseline` added-line diff. That is enough for normal user-typed additions, but it is not strong enough for repair/reconstruction paths that rewrite exchange text and need transcript canonicalization.

So the invariant should be stronger: before commit, every prompt-bearing user line in the pre-boundary exchange region must either preserve an existing `❯ ` prefix from snapshot or receive one during canonicalization. That should be driven from the same prompt-bearing classifier, not from a separate added-line heuristic.

`session-check` should then enforce the same rule. Right now `src/agent-doc/src/session_check.rs:339-361` only flags likely bypassed writes when snapshot drift inserts `### Re:` or `## Assistant`. It should also fail when the changed exchange tail contains bare prompt-target lines that should have been canonicalized as `❯ ...`.

Concretely, I would change it this way:

1. Move transcript canonicalization into a binary-owned exchange normalizer that runs on every finalize/write path, including repair/manual writeback paths, not only the normal snapshot→baseline user-add path.
2. Feed that normalizer from `prompt_bearing_changes` / `extract_prompt_target_blocks`, so the same classifier that says "this is a user prompt" also decides it needs transcript prefixing.
3. Extend `session-check` with a transcript-format bypass guard: if post-snapshot drift adds a `### Re:` response and leaves matching bare prompt-target lines in the same exchange tail, fail closed instead of accepting the write.

I would treat this as a real product invariant, not agent guidance. The current split is the bug: prompt classification is deterministic, but transcript formatting is still partly heuristic and partly harness-discipline.

I did not patch `src/agent-doc` in this cycle.
<!-- agent:boundary:3d2f034e -->
<!-- /agent:exchange -->

<!--
In src/boost-client, `agent-doc tasks/monsterrodholders.md` response patchback does not have a `❯ ` prefix for user prompts. See my inquiry:
```
• Because I treated the patchback as an assistant-only response block, not as a transcript entry.

  The local evidence shows two different patterns:

  - agent-doc capture response_body values are response-only and do not include ❯.
  - The archived/baseline session docs do use ❯  to mark user prompts inline in the transcript.

  I followed the first pattern while writing directly into tasks/monsterrodholders.md, but that was the wrong fit for
  this document. In this file’s exchange history, ❯  is part of the transcript format, so omitting it made the
  patchback inconsistent with the surrounding record.

  So the short answer is: I excluded it because I was patching “response text” rather than reconstructing the full
  prompt/response transcript. That was a format mismatch on my side, not an intentional convention change. If you want,
  I can normalize the recent entries to use the same ❯  prompt style as the rest of the archived exchange.
```
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#3fjy] [recommended] Make  prompt-prefix preservation a binary-enforced exchange invariant tied to prompt-bearing change classification
- [ ] [#tswc] [recommended] Harden Codex/manual-repair writeback so response persistence cannot bypass / plus
- [ ] [#8cmb] [recommended] Add edit triage for agent-doc diffs: prompt target vs content edit vs recovery artifact/boundary artifact
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!-- 
Why are all of the commits done by Codex in agent-doc authored by `Test <test@test.com>`?
-->
