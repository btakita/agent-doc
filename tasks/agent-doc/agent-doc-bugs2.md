---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 1e2c3e0e-c994-4c09-907c-675541486d3c
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find all contributing factors to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260808-080026.md`*

Compacted content:
- Archived 5 response topic(s): do [#runsubmitclaude]; closing out #runsubmitclaude; restoring the two queue items I deleted; 2 more
- Prior summary/context: prior compacted content: Archived 11 response topic(s): tmux auto-sync reactive conversion; do [#opencodeharnessswitc...

### Re: queuemirrororder — opus-5

All three parts fixed and released as **0.35.169**.

**(1) The mirror now honours the anchor.** `enqueue_actionable_ids_in_content_with_anchors` splices an anchored id directly after its anchor's live queue head instead of at the placement edge.

The anchored set is **recorded at the mutation site** (`CycleState::pending_anchored_ids`, written by the `--backlog-add-after` / `--backlog-add-before` arms in `agent-doc-write-runtime-io`), not inferred from backlog position. My first cut inferred it — "any fresh id sitting below an already-queued id is anchored" — and it reddened `closeout_sync_prepends_same_cycle_pending_add_in_go_mode`, correctly: `--backlog-add-back` produces exactly that shape and explicitly did *not* ask for the slot. Position is not evidence of intent, so the intent is recorded. Unanchored ids keep the `#queueatcreate` head default unchanged; an anchor with no live slot (struck, or completed-only) falls back to `placement` rather than vanishing. The splice stays insert-only, so operator free-text lines survive byte-for-byte.

**(2) Recovery exists.** `backlog_cmd::reorder` cascades into the mirror via `reorder_queue_mirror_in_content`, which permutes **only the named live heads among the slots they already occupy** — every other queue line keeps its exact byte offset, so `#qauthorder` holds for anything the reorder did not name. You no longer need `--done` / `--backlog-gate` to move a queued id. `queue sync` stays append-only as designed.

**(3) The metadata-only rejection is gone.** `WriteOptions::has_metadata_only_mutation` (reorder / text-metadata edits) flows into `enforce_imperative_response_contract_with_mutation_evidence`. A pure reorder has no command output, no code diff, and no commit hash to cite, so the prose heuristics rejected every honest response it could write. The binary applied the mutation, which is stronger evidence than any prose scan, so the guard defers to it.

**Changed paths:** `agent-doc-queue/src/backlog_sync.rs`, `agent-doc-element-backlog-io/src/backlog_cmd.rs`, `agent-doc-cycle-state-io/src/lib.rs`, `agent-doc-write-runtime-io/src/{lib,run_entry}.rs`, `agent-doc-write-command-io/src/lib.rs`, `agent-doc-template-io/src/{lib,write_normalize}.rs`, `agent-doc-preflight-io/src/lib.rs`, `agent-doc-start-runtime-io/src/lib.rs`, `tests/pending_integration.rs`, `SKILL.md`, `runbooks/pending-ops.md`, `VERSIONS.md`.

**Coverage added:** anchor honoured over placement; two ids sharing an anchor keep backlog order behind it; unanchored id below a queued one still takes the head; non-live anchor falls back; reorder permutes only named heads (free-text line asserted at a fixed slot); reorder is a no-op when already ordered; end-to-end `closeout_sync_places_anchored_same_cycle_add_after_its_anchor`; CLI-level `backlog_reorder_cascades_to_queue_mirror`; guard defers to the mutation but still rejects the same prose without it.

**Verification:** `make check` — 8707 passed, 0 failed, 248 skipped. `make install` + `agent-doc admin recycle` (binary now reports 0.35.169). Committed `eefc45681`, pushed, tagged `v0.35.169`.

**CI:** run `31248836014` was still queued at closeout — not awaited per `#ci-no-closeout-wait`.

### Re: watchsighupwin — opus-5

Verified. The Windows wheel built and shipped for 0.35.167 — nothing to fix.

**The gate is real and correct.** `agent-doc-watch-io/src/daemon.rs:347-353` — `install_shutdown_signal_handlers()` has a `#[cfg(unix)]` body installing `SIGTERM`/`SIGINT`/`SIGHUP` handlers and a `#[cfg(not(unix))]` no-op, because `libc::SIGHUP` does not exist on `*-pc-windows-msvc`.

**Artifact proof, not a green job.** A green publish leg proves nothing on its own, so I checked the published artifact:

- PyPI run `31246515464` (tag `v0.35.167`) succeeded, and `build (x86_64-pc-windows-msvc, windows-latest)` is `success` in its job matrix.
- PyPI serves `agent_doc-0.35.167-py3-none-win_amd64.whl` (17,446,595 B) alongside the three unix wheels.
- Downloaded it: it contains `agent_doc-0.35.167.data/scripts/agent-doc.exe`, 47,143,424 B, header `MZ` / `PE\0\0`, COFF machine `0x8664` (`IMAGE_FILE_MACHINE_AMD64`). So the cfg-gated code compiled *and linked* into a real AMD64 Windows binary — an empty or stub wheel would not.
- No regression since: 0.35.168 also publishes `win_amd64`. 0.35.169's PyPI run (`31248835628`) is still in progress, not awaited (`#ci-no-closeout-wait`).

**Separate finding, filed not fixed.** While checking runs I found the **agent-loop superproject** `Agent Doc` workflow's `check` job red for 8 consecutive runs: `collect2: fatal error: ld terminated with signal 7 [Bus error], core dumped` linking `agent-doc-commit-io` test `"git"` and `agent-doc-stream-io` lib test. `tmux-ci` passes in the same runs and local `make check` is green (8707/8707), so this is the runner exhausting memory/disk linking a 144-crate workspace's test targets — a CI-infra class, not a code regression, and a different repo's workflow from the one this head asked about. Filed at the top of `agent:backlog` rather than landing a speculative runner-tuning change mid-drain.

Session note: exchange accretion is at `warn` (2 no-op closeouts in 30 min); queue is active so I'm not stalling to compact.

### Re: loopsuperprojectjob — opus-5

Root-caused and fixed. It was never a code regression.

**Diagnosis.** `collect2: fatal error: ld terminated with signal 7 [Bus error], core dumped` linking `agent-doc-commit-io` test `"git"` and `agent-doc-stream-io` lib test. SIGBUS from a linker is the ENOSPC signature: `ld` mmaps its output file, and a write into a mapping that can't be backed raises SIGBUS rather than returning an error, so a full disk surfaces as a core dump instead of "no space left on device". Building the ~156-crate workspace with `--all-targets` while Cargo retains incremental state and full debug info for every test binary exceeds the ~14 GB free on `ubuntu-latest`.

**The controlled comparison made it conclusive.** agent-doc's own `.github/workflows/ci.yml` runs the *same* `make check` on the *same* runner image and has never produced this failure — because it already carries the mitigation, with a comment naming this exact cause:

```yaml
CARGO_INCREMENTAL: "0"
CARGO_PROFILE_DEV_DEBUG: "0"
CARGO_PROFILE_TEST_DEBUG: "0"
```

The superproject workflow simply never inherited it. (The one red run in agent-doc's own CI, `31245808892`, was a transient `install-action` failure — a different signature.)

**Fix** — `.github/workflows/agent-doc.yml`, commit `8b01b6fe50`:
- All three env vars at **workflow scope**, so `tmux-ci` gets them too. It builds the same `--all-targets` graph and is sitting on the same cliff; it just hasn't tipped yet.
- `cargo-nextest` install on `check`. Without it `make test` silently falls back to plain `cargo test`, so the superproject was running with **no per-test deadlock guard** — `.config/nextest.toml`'s 60s x 3 timeout was inactive there. A lock/IPC deadlock would have hung to the job cap instead of failing fast with a named test.
- `timeout-minutes: 60` on both jobs as the backstop for non-test hangs.
- A comment tying the two workflows together, since a divergence here reads as a code regression.

**Verification:** `make check` with all three env vars set locally — **8707 passed, 0 failed, 248 skipped**. That proves the flags don't red the suite; it can't prove the disk headroom, which is runner-specific. Pushed; run `31249383368` is in progress, observed once and not awaited (`#ci-no-closeout-wait`). Filed an actionable follow-up to confirm it went green, with the next escalation levers if the reduction turns out to be insufficient.

Session note: exchange accretion is `warn` (6 cycles / 30 min); queue is active so I'm not stalling to compact.

### Re: tmuxsyncslow — opus-5

> **Queue prompt:**
>
> do [#syncdupespelling]
>
> do [#tmuxsyncslow]


Fixed and released as **0.35.170**. The cause was not in the reactive plane at all, which is why items 1-5 of the plan all shipped and the switch was still slow.

**I measured instead of continuing down the plan.** `agent-doc sync --col A,B --focus …` reproduces the tab switch, and the binary indicts itself:

```
[sync] auto-start check: /home/brian/…/tasks/agent-doc/agent-doc-bugs2.md   ← absolute
[sync] auto-start check: tasks/software/lazily.md
[sync] auto-start check: tasks/agent-doc/agent-doc-bugs2.md                  ← relative, SAME doc
[sync] latency budget exceeded: phase ownership_per_file_loop took 4120ms (budget 750ms)
[sync] … unresolved files remain blocked: /home/brian/…/agent-doc-bugs2.md, tasks/agent-doc/agent-doc-bugs2.md
```

Three ownership passes for two documents, and one document listed twice as blocked under two spellings.

**Root cause.** `auto_start_candidate_files` (`agent-doc-tmux/src/lib.rs`) deduped on the raw `PathBuf`. The column set is a merge of the caller's `--col` arguments with remembered column memory, and the two disagree on spelling — memory retains absolute paths, the editor/CLI passes root-relative ones. So `/abs/root/tasks/x.md` and `tasks/x.md` keyed as two candidates, and the second pass ran the full stale-binding fallback (`alive but no longer proves ownership` → supervisor process-tree recovery), which costs seconds.

The sibling `projected_sync_pane_count`, fifteen lines below, already did `path.canonicalize().unwrap_or(path)` for exactly this reason. The codebase knew the right key; this function just never got it.

**Fix.** Key dedup on the canonical path. The *emitted* path stays the first-seen spelling — session ids and operator-facing output derive from it — so only the dedup identity changed. An uncanonicalizable path falls back to itself, leaving the not-yet-created auto-start case untouched.

**Before → after**, same command, same layout:

| | passes | budget | worst switch | steady state |
|---|---|---|---|---|
| before | 3 for 2 docs | **4120ms** / 750ms exceeded | 2829ms | ~550ms |
| after | 2 for 2 docs | no violation | 802ms | ~450ms |

The `unresolved files remain blocked` line is gone too — that document was only "blocked" because its duplicate twin failed the ownership proof.

**Verification:** `make check` — 8709 passed, 0 failed (2 new tests: absolute/relative collapse, and uncanonicalizable paths staying distinct so auto-start still works). Built, installed, recycled. Commits `88b33fe65` (fix) and `6dea99840b` (plan record), tagged `v0.35.170`. CI `31250556000` in progress, observed once, not awaited.

I recorded the finding as item 7 in `tasks/agent-doc/plan-tmux-auto-sync-reactive.md` with the lesson attached: items 1-5 were genuine reactive improvements, but the duplicated work sat *upstream* of the reactive graph, in how the candidate set was keyed — no amount of further conversion would have found it. Plan item 4 (`ThreadSafeDependencyMap`) remains open and is still correctly marked lower priority.

Also filed and closed `#syncdupespelling` so the marker in the shipped code and `VERSIONS.md` resolves to a real tracked item — the coined-id guard caught that I'd have left it dangling.

### Re: runpromptverbose — opus-5

> **Queue prompt:**
>
> do [#runpromptverbose]


Fixed and released as **0.35.171** — though the report was half-stale, and the half that remained was worse than described.

**The paragraph was already gone.** `#qcontprose` removed it; the quoted text ("Agent Doc queue state advanced for … do not invoke agent-doc recursively … Persist and finalize this cycle") now survives only in a doc comment explaining what it used to be. Nothing to do there.

**What was actually still wrong:** the one-line trigger that replaced it was hardcoded to the bare `agent-doc <file>` form for *every* harness, on **both** dispatch paths:

1. `owned_pane_queue_continuation_prompt` (supervisor idle drain) ignored the harness and returned `format!("agent-doc {}", …)`. That silently violated this project's own spec — `specs/07-session-tmux-commands.md`: *"Claude/OpenCode drains inject the normal slash-command harness reopen, and Codex drains inject the bare `agent-doc <FILE>` reopen"* — and contradicted an existing test, `idle_queue_drain_payload_keeps_trigger_for_non_codex_harnesses`, which asserts exactly that split against `HarnessConfig::trigger_command`. The pure helper was harness-aware and correctly tested; the production caller handed it a hardcoded string, so the test passed while the behavior was wrong.
2. The editor route (`Run Agent Doc`) called `apply_plain_trigger_override` whenever `plain_trigger` was set, rewriting Claude's and OpenCode's `/agent-doc {file}` template to the bare form.

**Root of it:** `plain_trigger` was carrying two meanings — "attach no prompt-bearing context" (correct, still in force) and "use a different trigger than this harness's own" (wrong). That conflation is precisely what let the two dispatch paths disagree for the same harness, which is the divergence `#qcontprose` set out to eliminate in the first place.

Both paths now go through `HarnessConfig::trigger_command`: Claude/OpenCode get `/agent-doc <file>`, Codex keeps the bare form that *is* its harness-native entrypoint. Absolutization (`#qcontabspath`) is unchanged.

**Why this is more than tidiness.** The bare form *is* admitted by the `UserPromptSubmit` hook — which is exactly why the divergence never failed loudly and survived a spec that said otherwise. But only `/agent-doc <file>` also loads the skill deterministically, and the drain re-dispatches into panes the supervisor has just `/clear`ed, where the agent has no prior workflow context. I hit the mirror image of this myself earlier in this drain: a wakeup carrying the wrong prompt shape produced a turn with no cycle contract at all.

`apply_plain_trigger_override` now has no production caller. I left it (shared static-guard tests in `tests/test_cli.rs` assert its shape, and churning that file risks cross-session contamination) but documented at its definition why adding a caller needs a reason. Spec updated so line 65 and line 69 agree.

**Verification:** `make check` — 8710 passed, 0 failed (new: all three harnesses asserted on the continuation, plus the Claude/OpenCode/Codex trigger split). Built, installed, recycled, commit `c95cb6559`, tagged `v0.35.171`. CI `31251470362` in progress, observed once.

**Status correction on the previous item:** the superproject CI run `31249383368` from `#loopsuperprojectjob` came back **FAIL**, so that fix was not sufficient. `#confirmsuperprojectjob` is the next head and exists for exactly this — I'm leaving the diagnosis to that cycle rather than pre-empting it here.

Filed one `[operator-verify]` follow-up: the live JetBrains pane render is the only part I can't verify from this side.

### Re: runpromptverbose — closeout addendum — opus-5

The closeout of the response above was **interrupted by an agent-doc defect**, which I fixed rather than retried. Released as **0.35.172**.

**What happened.** `respond` committed the mutations and the response reached HEAD, then session-check failed with `INTERRUPTED: … 7 concurrent operator steering directives` — quoting seven lines of *my own response body* back at me as operator prompts.

**Why.** The working tree held the `### Re: runpromptverbose` block **twice**: the committed copy carrying the drain's `> **Queue prompt:**` blockquote, and a replay without it. Response-block dedup normalizes `<!-- agent:boundary: -->`, the `(HEAD)` suffix and the `❯` prefix, then compares byte-for-byte — so the inserted quote (and the blank-line spacing around it) made the two copies compare as *different* blocks. `agent-doc dedupe` reported "no duplicates found" and the replay survived. Session-check then diffed the working tree, found assistant prose it could not attribute to a `### Re:` heading, and applied the plain-prose steering heuristic to it.

Worth naming: the duplication was the cosmetic part. The damage was a **committed, successful cycle reported as interrupted** — the closeout equivalent of a false alarm that costs a recovery cycle every time it fires.

**Fix.** `normalized_response_block` now ignores the queue-prompt blockquote and blank-line spacing when comparing two blocks — the same treatment the boundary marker and `(HEAD)` already get, since all three are binary-inserted decoration rather than response content. Blocks whose prose genuinely differs still compare as distinct; both directions are regression-tested.

**Proof on the live document that produced it:** `dedupe` removed 3675 bytes, kept the copy carrying the queue-prompt quote, and session-check went `INTERRUPTED` → `ok`. Remaining working-tree delta was a `resume:` id and a transient boundary marker — zero content lines absent from HEAD, which is what made the recovery provably lossless.

`make check` — 8712 passed, 0 failed. Commit `6037ab4f7`, tagged `v0.35.172`, installed and recycled.

### Re: confirmsuperprojectjob / lzktemptydir — opus-5

> **Queue prompt:**
>
> do [#lzktemptydir]
>
> do [#confirmsuperprojectjob]


**The disk fix worked.** `#confirmsuperprojectjob` asked whether the Bus error recurred after `8b01b6fe50`. It did not: run `31249383368` shows **no `signal 7` and no `Bus error`** anywhere in its log. The Rust build and full test suite ran to completion and `version-sync` passed — the job got further than it had in eight previous attempts.

**It failed later, on something the crash had been hiding.** `make dev-harness-test`:

```
Could not resolve io.github.lazily:lazily:0.39.0
  > No matching variant of project :lazily-kt ... No variants exist.
```

**Root cause — the same defect shape as everything else in this drain: a check that cannot distinguish "absent" from "present but empty."** `editors/jetbrains/settings.gradle.kts` composite-builds the sibling `lazily-kt` when present, falling back to the published `io.github.lazily:lazily` when absent. The guard tested directory *existence* — and an uninitialized git submodule **is an existing empty directory**. So the guard passed, `includeBuild` substituted the dependency with a build containing no projects, and Gradle emitted that opaque variant error. A standalone checkout has no directory at all, which is exactly why only a *partial* monorepo checkout could hit it.

Fixed on both sides:
- **The guard** (`agent-doc 0.35.173`) now keys on the sibling's `settings.gradle{,.kts}`, so "absent" and "present but uninitialized" take the identical fallback. Any partial checkout now degrades to the published artifact instead of that error.
- **The superproject CI** now initializes `src/lazily-kt` and `src/lazily-spec`. Its submodule list had every sibling except those two; agent-doc's own `ci.yml` clones both, which is why that workflow stayed green while this one did not.

Belt and braces on purpose: CI gets the composite build it intends, and the guard stops being a trap for everyone else.

**Verification:** `make dev-harness-test` green with the composite present; guard verified to reject an empty directory (`isFile` on the settings file → false where `exists()` on the directory → true). `make check` — 8712 passed, 0 failed. Commits `ea5e80085` (agent-doc) and `372f52aed5` (superproject), tagged `v0.35.173`, installed and recycled. Run `31253205086` in flight, observed once, not awaited (`#ci-no-closeout-wait`).

**On `#operatorverifyeyeball`, the head I was handed:** I am deliberately not closing it. It requires a human watching a real JetBrains pane confirm the injected trigger renders as `/agent-doc /abs/path.md`; `--done` from here would assert something I did not do. The binary already agrees — it tagged the item `[operator-verify]` and excluded it from `queue_drainable_head_count`, which is why preflight handed me no `selected_queue_prompts` this turn. With the two drainable items now closed, only that operator-gated head remains, so the drain should report `deferred_for_operator_proof` and stop rather than churn.

Session note: exchange is at 161 lines / 6 sections (`warn`); queue is active so I have not stalled to compact.

### Re: the queue head that was never recognized, and Run Agent Doc not submitting — opus-5

> **Queue prompt:**
>
> do [#operatorverifyeyeball]: verified. looks good


> **Queue prompt:**
>
> In the last turn, the queue item was not recognized and agent-doc turn resulted in `Nothing to respond to this cycle — preflight reports no_changes: true.` even though `do [#operatorverifyeyeball]: verified. looks good` was in the queue. Also, Run Agent Doc added the prompt to Claude Code but did not submit.


Both are real defects. Diagnosed, fixed, and shipped in **0.35.174** this cycle.

**1. An answered `[operator-verify]` head stayed deferred forever (`#opverifyanswered`).**
`[operator-verify]` defers a head because it needs a human to look at something. When you supplied that verdict in the head itself — `do [#operatorverifyeyeball]: verified. looks good` — nothing consumed it. `deferred_backlog_ids` still held the id, `head_is_drainable` still refused it, so `queue_drainable_head_count` stayed `0` and `queue_continuation_required` stayed `false`. Every following turn correctly reported "no drainable work" against a queue whose head was your answer. The verdict is the proof the tag was waiting for, so a head carrying one now drains in both the in-session loop and the supervisor drain. The rule is shape-based, not sentiment-based — `still broken` is as actionable as `verified`, and agent-written heads are bare `do [#id]`, so a trailing annotation is operator-authored by construction. `[focused-cycle]` is deliberately unaffected: it asks for a fresh cycle, which no amount of human confirmation supplies.

**2. Your bug report inherited the gating of the head it quoted (`#ftquotedid`).**
`extract_head_id` scanned raw head text, so the free-text line containing `` `do [#operatorverifyeyeball]: verified. looks good` `` in backticks was classified as *that* head — and inherited its operator-verify deferral. The bug report stalled the queue it was reporting on. Id extraction now masks balanced inline-code spans first; an unbalanced backtick stays prose, so real directives still resolve.

That pair is why the `no_changes: true` turn produced nothing: preflight reported the document unchanged relative to the snapshot, and the queue that *was* active had zero heads it would admit.

**3. "Run Agent Doc added the prompt but did not submit" (`#runsubmitclaude`, second pass).**
Found it in `ops.log`, not by inference. Pane `%25`, two clicks 71 seconds apart:

- `16:22:28Z` — `route_pass_through_submit_draft outcome=cleared enters_sent=1 elapsed_ms=306`. The draft was visible on the first settled look, one bare submit key landed it, the turn ran.
- `16:23:39Z` — `route_pass_through_submit_draft outcome=cleared enters_sent=0 elapsed_ms=153`. One settled look, nothing visible, verdict `cleared`, repair over. That is the click you watched sit unsubmitted.

0.35.164 fixed the 1ms version of this by gating the verdict on a 150ms settle window. 150ms lands *on* the render boundary — same pane, same harness, two different answers a minute apart. It is the `#idlerevisionreactive` collapse again: "I looked and saw nothing" was still being read as "there is nothing." An **idle** pane showing no draft is ambiguous between "the turn already finished" and "the keystrokes have not rendered", so that verdict now requires a second consecutive idle-and-empty observation. A **busy** pane showing no draft needs none — a working harness is positive evidence the trigger crossed — so the successful path costs nothing extra and only the genuinely ambiguous case pays one more window.

**Verification.** Nine new regression tests (three drainability, two id-extraction, four replaying the whole pass-through repair against a scripted pane including the 16:23:39 frame sequence). `make check` green. `make install` + `agent-doc admin recycle` done on both roots; binary is 0.35.174. Pushed, tagged `v0.35.174`; the Release and PyPI workflows are queued — recorded, not waited on.

One caveat worth naming: fix 3 makes the repair *see* the strand it was missing. Whether Claude Code's composer can also swallow the submit key in a way no bare Enter recovers is not settled — `#passthroughsplitprofile` is the open review item for that, and it now has real post-0.35.164 measurements to read, including one `enter_resubmit` at 16:22:28Z. That is evidence the single-call text+Enter send *is* dropping submits, which is the item's stated unblock condition.

Session note: exchange is at 193 lines / 7 sections (`warn`); queue is active, so I did not stall to compact.

> **Queue prompt:**
>
> Go through all review items and add anything actionable to the backlog + queue.

### Re: review triage — opus-5

Went through all ten. Four produced actionable, agent-drainable work. Six of the rest are genuinely operator-gated, but almost all of them carried a stale precondition that I corrected in place.

**Correction to my last turn first.** I wrote that ops.log held "one `enter_resubmit` at 16:22:28Z". It does not. All eight `route_pass_through_submit_draft` lines read `outcome=cleared`; 16:22:28Z is `outcome=cleared enters_sent=1`. The conclusion survives, but for a different reason than I gave — and that difference is the finding.

**#passthroughsplitprofile could never have been unblocked as written.** Its condition is "a nonzero `outcome=enter_resubmit` rate". `EnterResubmit` is a *non-terminal* action — `pass_through_stranded_draft_action_is_terminal` admits only `Cleared`, `DeferredPaneBusy`, `ExhaustedStillStranded` — and production logs the terminal line only. So that counter is structurally always zero no matter how many submits drop. It was a criterion measuring nothing, which is why an item with a real signal in it looked like an item with no data.

The signal that does carry information is `enters_sent` on the terminal line. `enters_sent += 1` happens only inside the `EnterResubmit` branch (`agent-doc-controller/src/dispatch.rs:6070`), so `enters_sent > 0` on a `cleared` line means the draft was **still visible after the initial send** — the submit did not take and the repair had to press Enter again. Read that way, the post-0.35.164 lines (the ones with a real settle window, `elapsed_ms >= 150`) number 6, and **5 of the 6 needed at least one resubmit; one needed two.** That is the item's own "the single-call send is the cause" condition, at a much higher rate than it anticipated.

Six samples is not the ~20 the item asks for, so I did not touch the submit profile. But it no longer waits on a counter that cannot move: `#ptsubmitmetric` fixes the criterion and makes the signal readable, `#claudesplitsubmit` makes the call once the floor is reached.

**#hvw0 carries a second untested claim.** It asserts that every tagged release now fails loudly at the publish step after 32836ad. No tag has run since. The newest Release run on skill-harness is v0.1.3 from 2026-07-18, and its `Publish to crates.io` step reports **success** — that is precisely the false green 32836ad was written to kill. The loud-failure path has never executed. Noted on the item; `#hvw0publishproof` is the static check that does not need the secret.

**#operatorverifyresolve removed, converted to backlog.** Its premise was "the *current* JetBrains File Cache Conflict". There is not one: recent session-checks read `status=current authority=cp_model live_editors=1 delivery_converged=true`, and the retained closeouts it was waiting on are committing. The only genuinely untested half — whether a later Compact Exchange still trips on the historical stranded duplicate heading — needs no human at all, so it is `#compactstrandedheading` in the backlog.

**Six stale version preconditions corrected.** Several items instructed the operator to install artifacts *older* than what is already on disk: 0.2.205 (#falsetyping-guard), 0.2.339 (#rebootselfhealverify) and 0.2.340 (#operatorverifyrestart) against **0.2.350, installed in all four JetBrains IDE configs**; and 0.35.6 (#mtds) against the installed 0.35.174. Each now names the installed artifact, and the four restart-gated items record that **one** IDE restart satisfies all of their install preconditions together. #falsetyping-guard's JB install step is marked done; its VS Code 0.2.38 half is recorded as unverified rather than complete, because no agent-doc extension is present under `~/.vscode/extensions`.

Review is 10 → 9, and what remains is real operator work: live IDE/pane proofs (#stickymdpaneverify, #operatorverifyrestart, #rebootselfhealverify, #c8tb, #operatorverifylive, #mtds, #falsetyping-guard), one repo-secret rotation (#hvw0), and #passthroughsplitprofile pending its corrected measurement. The single highest-leverage operator action is one JetBrains restart onto 0.2.350 — it unblocks four of them.

Backlog and queue now hold four items, topmost first: `#ptsubmitmetric`, `#claudesplitsubmit`, `#compactstrandedheading`, `#hvw0publishproof`.
<!-- agent:boundary:22e8b690:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do [#ptsubmitmetric]
- do [#claudesplitsubmit]
- do [#compactstrandedheading]
- do [#hvw0publishproof]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#ptsubmitmetric] #agent-doc-bug Fix the unmeasurable #passthroughsplitprofile criterion. `outcome=enter_resubmit` is a NON-terminal action (`pass_through_stranded_draft_action_is_terminal` admits only Cleared/DeferredPaneBusy/ExhaustedStillStranded), so production ops.log can never record it and that counter is structurally always zero no matter how many submits drop. The real signal is `enters_sent > 0` on a terminal `outcome=cleared` line — `enters_sent += 1` happens only inside the EnterResubmit branch (agent-doc-controller/src/dispatch.rs:6070). Emit an explicit terminal field (for example `resubmit_required=true|false`) or a rollup command so the decision is readable without hand-parsing ops.log, restate the unblock criterion in those terms on #passthroughsplitprofile, and add a regression test pinning that a repair which resubmitted logs the new field.
- [ ] [#claudesplitsubmit] #agent-doc-bug Decide and implement whether claude joins `harness_uses_split_text_submit` (the split text+Enter profile codex/opencode use, `tmux_submit_profile_for_harness`, 80ms), reading the corrected counter from #ptsubmitmetric. Current reading: 6 post-0.35.164 harness=claude repairs (elapsed_ms >= 150), 5 of which needed at least one resubmit and one of which needed two — that is the stated single-call-send-is-the-cause condition, but on 6 samples rather than the ~20 sample floor. Gather to the floor with the corrected metric first, then either add claude to the split profile or record that the settle-gated repair alone is sufficient. Do not change the profile without the measurement.
- [ ] [#compactstrandedheading] #agent-doc-bug Run `agent-doc compact exchange tasks/agent-doc/agent-doc-bugs2.md` and confirm it no longer fails on the historical stranded duplicate heading. Converted from the removed operator-verify item #operatorverifyresolve, whose premise (a live JetBrains File Cache Conflict awaiting a human Cancel / Keep-memory-changes) is stale: recent session-checks read `status=current authority=cp_model live_editors=1 delivery_converged=true` and the retained closeouts it was waiting on are committing. Only this half was ever untested, and it is agent-testable.
- [ ] [#hvw0publishproof] #agent-doc-bug Verify statically (read the release workflow on github.com/btakita/skill-harness) that the crates.io publish step propagates failure after 32836ad. The loud-failure behavior has never actually executed: the newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports SUCCESS even though the CARGO_REGISTRY_TOKEN 403s and v0.1.3 was published by hand — that success line is the false green 32836ad was meant to kill. Until a tag runs, the every-tagged-release-now-fails-loudly claim on #hvw0 is untested.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#passthroughsplitprofile] #agent-doc-bug [blocked-on-evidence] Decide from evidence whether Claude also needs the split text+Enter submit profile that codex/opencode use (`tmux_submit_profile_for_harness`, 80ms). CRITERION CORRECTED 2026-08-08: the old unblock condition (a nonzero `outcome=enter_resubmit` rate in ops.log) is UNMEASURABLE — EnterResubmit is a non-terminal action and production logs only terminal outcomes, so the counter is structurally always zero. The load-bearing signal is `enters_sent > 0` on a terminal `outcome=cleared` line (dispatch.rs:6070 increments only in the EnterResubmit branch), which means the draft was still visible after the initial send. Current reading: 6 post-0.35.164 harness=claude lines, 5 with enters_sent>0 (one with 2). UNBLOCKED when the corrected counter (#ptsubmitmetric) reaches ~20 post-0.35.164 harness=claude repairs; #claudesplitsubmit then makes the call. Do not change the submit profile without the measurement — the mechanism is inferred, not proven.
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE — plugin 0.2.350 is ALREADY INSTALLED in all four IDE configs, so only the restart is missing, and that one restart also satisfies the install precondition for #operatorverifyrestart, #rebootselfhealverify and #falsetyping-guard — then open a two-column split with an agent-doc document in each and switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE (installed plugin is 0.2.350; this item originally named 0.2.340, long superseded — the single restart in #stickymdpaneverify covers it), then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE (installed plugin is 0.2.350; this item originally named 0.2.339 — reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is still required, and the restart in #stickymdpaneverify covers it). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the `pane-layout state projection publish failed ... Connection refused` notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. CORRECTION 2026-08-08: the claim that every tagged release now FAILS loudly at the publish step (intended behavior after 32836ad) is UNTESTED — no tag has run since. The newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports success, which is exactly the false green. #hvw0publishproof carries the static check.
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare prompt) still refuses as busy. Fixed in 17848a927. This item named binary 0.35.6 and a pending idle-boundary recycle; the installed binary is now 0.35.174 and has recycled many times, so only the live eyeball remains.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green. JB install step DONE — the installed plugin is 0.2.350 in all four IDE configs, superseding the 0.2.205 zip this item named; only an IDE restart is missing, covered by #stickymdpaneverify. VS Code 0.2.38 install is UNVERIFIED — no agent-doc extension found under ~/.vscode/extensions. REMAINING (operator): restart the IDE, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#agentsignals] #agent-doc-feature [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
