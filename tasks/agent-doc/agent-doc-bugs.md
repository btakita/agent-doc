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
---

## Status

<!-- agent:status patch=replace -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261015-024625.md`*

Compacted content:
- Archived 4 response topic(s): agent-doc fixes, release, and tmux auto-sync; GitHub issue 259 closeout; GitHub issue 260 closeout; 1 more
- Prior summary/context: prior compacted content: Archived 17 response topic(s): strict-template failure and steering stall; retained closeout...

### Re: dispatched #gh-fix 261–264 + release batch — opus-5.5 · 2026-10-14T22:49-04:00

I picked up the four `#gh-fix` queue items and gave each one to its own background subagent. They all use the queue preset `#spec-test-build-install-commit-push`. Every subagent works in its own worktree outside agent-loop, branched from `origin/main` at v0.35.503. The local `src/agent-doc` checkout is 662 commits behind `origin/main`, so nothing branches from it.

| item | subagent | worktree / branch | scope |
|---|---|---|---|
| GH #261 | `subagent:gh261` | `~/work/btakita/agent-doc-wt/gh261` · `fix/gh261` | After promotion, the old controller stops its watchdog and orphan-drain ticks. Its drain gets a time limit so idle pooled connections can't hold it open, and its frozen hub stops answering reads as current. |
| GH #262 | `subagent:gh262` | `~/work/btakita/agent-doc-wt/gh262` · `fix/gh262` | The fallback watches Remote Dev `CONTROLLER` sessions (`ClientKind.REMOTE`), logs which sessions it observes at install, and gets a test that a `CONTROLLER` session is observed |
| GH #263 | `subagent:gh263` | `~/work/btakita/agent-doc-wt/gh263` · `fix/gh263` | Process start time comes from `/proc/<pid>/stat` `starttime` + `btime`, not the `/proc/<pid>` mtime. Every call site moves to one shared helper, and the tests that faked the mtime are rewritten. |
| GH #264 | `subagent:gh264` | `~/work/btakita/agent-doc-wt/gh264` · `fix/gh264` | Component scope uses only top-level body ranges, so nested `agent:salient-response` no longer panics. A panic in a controller request comes back as an error that names the document. |

Subagents test in their own worktrees (targeted tests, clippy, `make check-fast`) and push their own branches. They do not run `make install`, close issues, or touch `main`.

**`release + publish` (added while this turn was running):** I claimed it for myself (`coordinator:release-after-gh261-264`) instead of giving it to a subagent. It can only run after the four fixes are merged, and subagents may not cut releases. Next steps once the subagents report:

1. Merge `fix/gh261`…`fix/gh264` into `main` one at a time.
2. Run one full `make check`, then one `make install`.
3. Push `main`, close #261–#264 with the fix commits, and release v0.35.504.
4. Release the queue claims and close the items.

### Re: Subagent completed — agent-doc: `subagent:gh262` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/262”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=b0207481330a0cca237e9ba4c3fc75f6dcabb3d58fca9a2fca77c4dd101cb501 -->

### Re: Subagent completed — agent-doc: `subagent:gh264` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/264”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=235197fb29a2b8aa99889c8cd2d40bea9f5f810c8004a4c8fd89beb377468851 -->

### Re: Subagent completed — agent-doc: `subagent:gh263` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/263”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a60233b7c14ac95ec8b58f4707e265a6b96c0541600746e475077e13edc18940 -->

### Re: Subagent completed — agent-doc: `subagent:gh261` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/261”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7dce203b65ae7d8dcdb756b4de8fd2e0e3f45efa09612bfc4582397b23830773 -->
> **Chat prompt (#chatprompt):** can we use formal logic to ensure the tmux auto-sync class of error does not occur?

### Re: formal guarantee for the tmux auto-sync wedge — opus-5.5 · 2026-10-14T23:50-04:00

Yes. The repo already has the model this needs. What missed the bug is the link between that model and the code.

**Why the existing model stayed green.** `formal/tla/PassiveTmuxSyncNet.tla` already models editor focus → tmux over the adversarial `NetChannel` (messages can be delayed, reordered, dropped, duplicated or cut by a reconnect). It proves the liveness property `VisibleEventuallyMatchesEditor == <>[](visible = desired)`. It also has wedge configs that TLC must refute: `AdvanceOnEffect=FALSE` (F14/F15) and `SeqFence=FALSE`. But the model has no *retire/forget* transition. The editor can only send `Focus(doc, seq)`. The regression added an action the model does not contain: the plugin called `forgetEditorSurface` on its own frame when a reply came back after a newer observation. On the controller side, `ControllerEditorSurfaceGraph::observe` rejects a retired endpoint permanently at its unchanged per-process generation. TLC proved liveness for a protocol that no longer matched the shipped one.

**What would prevent this class of error:**

1. **Model the retirement channel.** Add `Retire(frame, gen)` messages, a per-process generation that only a reload changes, the controller's `retired && gen <= current` fence, and frontend handoff. Add the property *a live endpoint is never permanently muted*: `LiveEndpointNeverMuted == \A e : [](Live(e) /\ Refocused(e) => <>Accepted(e))`. The existing `<>[](visible = desired)` must hold with retirement enabled. Following the repo's pattern, add a knob `RetireOnNewerReply`, set TRUE in a `PassiveTmuxSyncNetRetireWedge.cfg`. `scripts/run_tla.sh` asserts that every wedge violates its property, so TLC has to reproduce the 02:54Z wedge as a counterexample, which proves the model can see this bug class. Add a Reach config so the fixed model isn't green only because it never retires anything.
2. **Make retirement recoverable by construction.** The safest rule removes the class rather than detecting it: a retirement is terminal only for a generation that has already ended. A client whose live frame is rejected with `rejected=retired_endpoint` re-registers under a fresh generation. Then no client-side mistake can mute a live endpoint for longer than one round trip, and that recovery is itself an action in the model. The fix now on `fix/tmux-autosync` removes the bad trigger and names the rejection reason, but a future bad trigger could still wedge it.
3. **Bind the model to the code.** This is the step whose absence let the bug ship.
   - **Action-set conformance test:** enumerate every RPC and plugin call that changes surface-graph state (`observe`, `forget`/retire, handoff) and assert each one is listed against a model action. Adding an unmodeled transition then fails `make check` instead of silently voiding the proof.
   - **Trace validation:** replay `controller_editor_surface_observed/forgotten` sequences from SimWorld runs, and from the 00:45Z and 02:54Z wedge windows in `ops.log`, through a checker for the model's next-state relation. The real wedge trace becomes a permanent regression fixture.
   - **Runtime monitor of the liveness property:** if the same connected endpoint gets `accepted=false rejected=retired_endpoint` K times in a row, warn, or heal by forcing re-registration. A model-to-code gap then shows up within seconds instead of after an operator notices that tabs stopped syncing.

**Limits.** TLC checks a bounded model, so it proves the protocol, not the Kotlin or Rust. The guarantee is only as strong as the conformance step in (3). A model is not proof of the code until something mechanically ties the code's transitions to it. Lean already holds the wait-machine and authority-ladder proofs. It could also prove the pure `ControllerEditorSurfaceGraph::observe` fence function, but the cross-process wedge belongs in TLA+.

If you want this, I'd queue it as one item after the v0.35.504 release: (1) the TLA+ extension with wedge and reach configs, (2) re-registration on `retired_endpoint`, (3) the action-set conformance test, trace fixtures for the two wedge windows, and the runtime monitor.

### Re: dispatched the formal-logic work for the tmux auto-sync class — opus-5.5 · 2026-10-14T23:43-04:00

I handed the new queue item ("do your formal logic suggestions") to its own subagent (`subagent:surfaceformal`). It works in the worktree `~/work/btakita/agent-doc-wt/surfaceformal` on branch `feat/surface-retire-formal`. The branch starts from the integration branch `batch-gh261-264`, so the tmux auto-sync fix is already in it. It covers all three parts of the proposal:

1. **TLA+:** `PassiveTmuxSyncNet` gets a retire/forget step, the per-process generation, the controller's retired-endpoint check, and frontend handoff. It also gets the property "a live editor window is never permanently ignored".
   - A **wedge** config with the shipped behaviour (`RetireOnNewerReply = TRUE`) must make TLC find the 02:54Z stall.
   - A **reach** config shows the fixed model actually exercises retirement and re-registration.
2. **Recovery built into the design:** when the controller rejects a still-connected window as retired, the plugin re-registers under a fresh generation, so no forget bug can keep a live window ignored for more than one round trip. This covers the classic plugin and the Remote Dev handoff case that is still open.
3. **Tying the model to the code:**
   - A test maps every call that changes the controller's editor-window state to a step in the model, so a step the model lacks fails `make check`. The subagent must show the test fails when it adds a fake unmodeled call.
   - The 00:45Z and 02:54Z stall traces, anonymized, become replay fixtures.
   - A runtime warning, plus automatic recovery, fires when a connected window is rejected as retired K times in a row.

**`release + publish`** comes after this item in the queue, so v0.35.504 waits for this branch and for the duplicate-title fix. I'll then merge both and run one full `make check`, which includes `make tla`, followed by `make install`. Last, I'll push `main`, close #261–#264, and release.

The three `semantic_completion_match` warnings from preflight are false matches with old done items, so I ignored them.
> **Chat prompt (#chatprompt):** is the release published?

### Re: release status (v0.35.504 not yet published) — opus-5.5 · 2026-10-15T00:17-04:00

No. The latest published release is still **v0.35.503** (GitHub release at 2026-10-15T02:20:56Z), and `origin/main` is still at `748c9381a7 chore: release v0.35.503`. No v0.35.504 tag exists.

The release is held on purpose. "release + publish" comes last in the queue, after "do your formal logic suggestions", and that subagent is still running. Everything else is merged into the local integration branch `batch-gh261-264`, which is not pushed yet:

- GH #261, #262, #263, #264
- `priority` on `agent:review`
- `agent:todo` auto-creation and `--todo-add`
- the tmux auto-sync regression fix
- the duplicate-heading `report-complete` fix

Once the formal branch lands, I'll merge it and run one full `make check`, which includes `make tla`, then `make install`. Then I'll push `main`, close #261–#264, and release v0.35.504.

If you'd rather ship now, I can release the integrated batch without the formal work and put the formal work in v0.35.505.
> **Chat prompt (#chatprompt):** ship now

> **Chat prompt (#chatprompt):** Is the next version of agent-doc installed? tmux sync not working

> **Chat prompt (#chatprompt):** is the release locally installed?

> **Chat prompt (#chatprompt):** b/c I want tmux auto sync to work

### Re: v0.35.504 installed locally, tmux auto-sync restored, release re-running — opus-5.5 · 2026-10-15T01:00-04:00

> **Queue prompt:**
>
> do [#sensemixdeterministic]: Is there any precedence of agentic programming languages? How about an embedded programming language for markdown? How about a template-driven approach? Perhaps we can support functions + a built in `prompt("...")` function, support conditional logic? Could we use fenced python or js? If we support functions, could we setup events or hooks? Would Jupyter be a good inspiration or since agent-doc is more of a graph, some sort of event-driven graph programming language?


**Local install: yes.** I ran `make install` from the integrated tree, so `agent-doc --version` now reports **0.35.504**. The JetBrains plugin hot-upgraded in your running IDE (classic 0.2.615). 27 supervisors and 4 controllers are marked to recycle at their next idle boundary. The 262 package (0.2.616) went to the Rider install, which loads it on next start.

**tmux auto-sync works again.** The cause was the tmux auto-sync regression described earlier. Since the install, `ops.log` shows the plugin publishing under a new generation (`1403614970817234`). Every tab switch is `accepted=true`, and at 04:49:28Z a `caller_kind=automatic` layout published and converged with `effect=applied focus=focus_applied`.

**Published release: not yet.** "ship now" means v0.35.504 goes out without the formal-logic branch (that work moves to v0.35.505). The first `make release` passed preflight and the nextest suite (10,563 passed). It then failed in the batched controller tests on the new #261 test `a_promoted_predecessor_releases_the_editors_authority_streams`, before any tag or push.
- **Cause:** the code is correct, but the test was timing-dependent. The stream's own subscription publishes its initial projection asynchronously, so the stream can write a real projection frame before the retirement wake, and the late-subscriber probe passes or fails depending on that timing.
- **Fix:** the test now asserts the real invariant: at most one genuine projection frame, then at most one wake, and nothing after the wake. The late subscriber now uses a cursor ahead of every frame, so only retirement can return it early. It passed all 40 concurrent stress runs.
- **Now:** `make release` is running again on the fixed tree: full `make check`, then `tmux-ci`, then tag and push `v0.35.504`, then `install-full`.

**Pre-existing timing flakes:** the same stress run also showed two older controller tests failing under heavy concurrency. Both are on `main` already and passed both full `make check` runs. I filed `#ctrltimingflakes` to replace their wall-clock assertions with structural ones. I did not rerun them to green.

> **Operator note:** do [#sensemixdeterministic]: Is there any precedence of agentic programming languages? How about an embedded programming language for markdown? How about a template-driven approach? Perhaps we can support functions + a built in `prompt("...")` function, support conditional logic? Could we use fenced python or js? If we support functions, could we setup events or hooks? Would Jupyter be a good inspiration or since agent-doc is more of a graph, some sort of event-driven graph programming language?

**Verdict:** yes, and agent-doc already does it with a fixed set of built-in deterministic directives: presets, queue grammar, ids, slash commands, orchestrate DAGs and closeout guards. What's missing is a way for a project to add its own. Don't build a language. Add **named steps**: commands registered in a committed file, run by the binary, and recorded as receipts tied to the exact repository contents. Documents refer to steps by name only.

- **Precedent:** yes, plenty, in two families.
  - LLM calls as steps inside ordinary code: LMQL, Guidance, SGLang, APPL, IBM PDL, BAML, Jac `by llm()`, DSPy, Semantic Kernel.
  - The model interprets the language itself: SudoLang, OpenProse.
  - Older relatives: org-babel (its `:cache` hash is the receipt idea), Jupyter/marimo, Markdoc.
  - None of them has agent-doc's shape: a co-edited, committed document whose deterministic parts a separate binary enforces.
- **Embedded markdown language:** no. Text that two authors edit concurrently through the CRDT, including half-typed edits, must not change what a program does. Keep the small declarative vocabulary and add named directives one at a time, as Markdoc does.
- **Template-driven:** only as plain substitution into preset bodies, such as the preset argument and fields from a step's receipt. No `if`/`each` inside templates.
- **Functions + `prompt("...")`:** functions yes, as named things. Presets already are the agentic functions (a preset is a named `prompt()`), and steps would be the deterministic ones. A script calling `prompt()` is the wrong way round, because the turn lifecycle belongs to the binary.
- **Conditionals:** only over a fixed set of typed facts: step passed or failed, a receipt exists for this tree, queue drained, time reached. No general `if <expr>`; branches that need judgement stay with the agent.
- **Fenced python/js:** no. A sandbox takes away the git, filesystem and network access that useful steps need. A fence is CRDT-edited text, so replay would need pinned revisions and would repeat side effects. Making fences executable also turns every pasted log into an injection risk. Put the code in `scripts/x.py` and register it as a step.
- **Events/hooks:** yes, and this is the strongest idea in the list. Hooks should subscribe to the typed lifecycle events agent-doc already records (`CommitObserved`, `QueueHeadCompleted`, …). Each hook runs once per event, never inside a critical section, and never for install/push-type steps.
- **Jupyter vs an event-driven graph:** the graph, which already exists (the controller event log, idempotent reducers, and lazily `Source`/`Computed`/`Effect`). Jupyter's linear cells and hidden kernel state are the wrong model; its one lesson is to show results next to what produced them.

**Smallest first step:** an `agent-doc-steps.toml` registry, for example `[steps.check-fast] argv=["make","check-fast"] effect="check" inputs="worktree" timeout_secs=1800`, plus `agent-doc step run <FILE> <name>`. The command runs directly, with no shell and no document text in it. Each run stores a receipt keyed by the step, the registry entry's hash and the working tree's content hash; the same key returns the stored receipt, and a failure is never auto-retried.

**Phases**, each one queue item:
1. Registry, `step run`, receipts.
2. Presets declare `requires: [check-fast]`, and closeout refuses to commit without a passing receipt for the exact tree.
3. `/step <name>` runs as a queue item with no agent turn.
4. `/step x else '#preset'` queues the preset on failure.
5. `on=` hooks for read/check steps.
6. Step results shown in `agent:signals`.
7. Optional read-only steps at preflight.
8. Optional `{{arg}}` / `{{step.<name>.tail}}` placeholders.

The full proposal, with precedent tables and an inventory of the existing deterministic mechanisms, is at `specs/proposals/deterministic-escape.md` on branch `design/deterministic-escape` (`798a46bb91`). It is merged into this release batch. Its `make check-fast` passed (10,563 tests).
> **Chat prompt (#chatprompt):** does agent-doc have agent:todo implemented?

### Re: agent:todo is implemented in the installed 0.35.504 — opus-5.5 · 2026-10-15T01:15-04:00

**Yes, in the build installed locally now (0.35.504). It isn't in a published release yet: the v0.35.504 release is still running.**

Before this batch (0.35.503 and earlier), `agent:todo` was only half-built. It was a registered component, and a guard stopped a `patch:todo` from removing items. But nothing added the component to an existing document, and there was no write flag. A `patch:todo` sent to a document without the component was quietly dumped into `agent:exchange`. That is why you never saw todos.

What 0.35.504 adds:
- **`--todo-add "<action>"`** works on `write`, `finalize`/`respond` and the MCP finalize tool, and can be repeated. It appends `- [ ] <action>` and never rewrites, reorders or removes your items. Adding an action that is already listed (open, checked or gated) does nothing, so a replayed closeout never duplicates a todo or reopens one you checked off.
- **The section is created when missing:** `## Todo` + `<!-- agent:todo -->` goes after `agent:review`. If there's no review, it goes after backlog, then queue, then exchange, then at the end of the document. `patch:todo` creates it the same way instead of spilling into the exchange.
- **Reminder:** when a closeout gates an item for review and no todo mentions that `#id`, the write prints a `[todo] advisory (#operatortodo)` line with the exact `--todo-add` command. It doesn't add the todo itself, because a gate can be waiting on CI or a third party rather than you.
- **Agent instructions:** the skill and runbooks now tell agents to use `--todo-add`, and every harness copy was regenerated.

This response uses it. It adds one real operator action, a live Remote Dev check for the GH #262 fix, so agent-doc should create the `## Todo` section in this document now.

### Re: v0.35.504 released — closing the batch — opus-5.5 · 2026-10-15T01:26-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/261

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/262

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/263

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/264

> **Queue prompt:** tmux pane auto-sync not working

> **Queue prompt:** Add a `priority` attribute to `agent:review`.

> **Queue prompt:** Is `agent:todo` implemented? I don't see it being used. Please fix. There should be todo's when the operator needs to take an action. If a agent:todo is not in the document, agent-doc should add it.

> **Queue prompt:** release + publish

**v0.35.504 is released.**
- `make release` passed release preflight (version projection and editor parity), one full `make check` (nextest: 10,563 passed, 167 skipped, plus the batched controller tests and `make tla`) and `make tmux-ci`.
- It then tagged `v0.35.504` and pushed `main` (now `3f763c4649`), and the local `install-full` completed. `agent-doc --version` reports 0.35.504.
- The tag push started the GitHub `Release v0.35.504` workflow, which publishes to GitHub Releases and PyPI. It was still running when this was written. CI on `main` is also running.

**Spec + tests, build + install, commit + push (`#spec-test-build-install-commit-push`), fix then close (`#gh-fix`).** Each item updated its spec text and added regression tests, verified with targeted tests, clippy and `make check-fast`. The batch then had one full `make check` and was built and installed locally. Everything is committed and pushed to `main`. **GH #261, #262, #263 and #264 are closed**, each with a comment naming its fix commit.

| item | fix |
|---|---|
| GH #261 | A promoted predecessor controller stops its watchdog and orphan drain, releases the editor's authority streams, retires its frozen relay hubs, and bounds its drain (`58b8d5467a`). The new stream test was then made independent of timing (`d61cc95fc8`). |
| GH #262 | The Remote Dev fallback observes `CONTROLLER` sessions as well as guests (`9f62473873`). |
| GH #263 | Process start time comes from `/proc/<pid>/stat`, not the `/proc/<pid>` mtime (`a6b585f71e`). |
| GH #264 | A nested component no longer panics component scope; a panicked controller request returns an error naming the document (`14baa6eecc`). |
| tmux pane auto-sync | The JetBrains plugin no longer retires its own live surface after a quick tab switch, and rejections name their reason (`9ad4fdcd04`). After the install, `ops.log` shows every observation accepted and `caller_kind=automatic` layouts converging. |
| `agent:review` `priority` | Sorted like backlog and icebox; new `agent-doc review set-attr` / `unset-attr` (`774258e24e`). |
| `agent:todo` | `--todo-add`, the section created when missing, and an advisory when a review gate has no todo (`0f889d24d7`). This document now has its own `## Todo` section. |
| release + publish | Done as above. Also in this release: the fix that stops several `report-complete` entries from blocking the document (`9bfa3cf4d9`) and the deterministic-escape proposal (`798a46bb91`). |

**Still open:**
- **"do your formal logic suggestions":** done on `feat/surface-retire-formal` (`3eaf5a0dfb`), but not in v0.35.504 because you said "ship now". It includes the TLA+ retirement model (wedge and reach configs, `make tla` 145 checks as declared), recovery by re-registering on `retired_endpoint`, the model-to-code conformance test, replay of the two wedge traces, and the muted-endpoint monitor. It will be merged and released in v0.35.505.
- **`#ctrltimingflakes`:** a subagent is still fixing the two older timing-dependent controller tests.
- **Wording note:** the subagent's spec drafts use "precedent" where your question said "precedence".

### Re: formal retirement model + timing-flake fixes on main; #265 dispatched — opus-5.5 · 2026-10-15T01:47-04:00

> **Queue prompt:** do your formal logic suggestions

> **Queue prompt:** do [#ctrltimingflakes]

Both branches are integrated into `main` (`d0f92c9fe5`) and installed locally. One full `make check` passed on the integrated tree: nextest 10,568 passed, 167 skipped, and every TLA+ config, including the new `PassiveTmuxSyncRetireNet`, behaved as declared. `make install` hot-upgraded the JetBrains plugin to 0.2.618. tmux auto-sync works on the new protocol: `ops.log` shows the plugin under a fresh generation, `accepted=true` on every observation, and an automatic layout converging at 05:45:37Z.

**Formal logic (`3eaf5a0dfb`):**
- **TLA+ model:** `PassiveTmuxSyncRetireNet` adds the retire channel, per-process generations, the controller's fence, and the Remote Dev handoff, with the properties `LiveEndpointNeverMuted` and `NoRetiredResurrection`.
  - **Wedges** (must fail): the 02:54Z shape, the Remote Dev fallback mute, and an exact-forget resurrection the fix would otherwise have caused.
  - **Reach check:** the fixed model must re-register at least once.
- **Recovery in code:** a rejected live window gets a typed `retired_endpoint` reply that names the generation to re-register above, and the plugin re-registers above it, in both classic and Remote Dev. A forget now raises the controller's cutoff, so a retired window cannot come back.
- **Tying the model to the code:**
  - A `syn`-based test fails the build if a code path that changes editor-window state has no matching model step. It caught three fake unmodeled paths.
  - The 00:45Z and 02:54Z wedges are anonymized replay fixtures.
  - A `controller_editor_surface_muted_live_endpoint` warning fires after 3 rejections in a row.
- **Gap:** VS Code receives the typed rejection but doesn't re-register yet.

**Timing flakes (`b01828c053`):**
- **actor-binding test:** it now proves "no retry" by counting connections, which is exactly one, instead of measuring elapsed time.
- **`mark_write_applied` test:** this was a real test-isolation bug. Process-wide state readers could answer for another in-process controller. Each reader now records its project root, and lookups check it; production runs one controller per process, so its behaviour is unchanged.
- **Results:** in 40 concurrent stress runs, both tests went from up to 3 failures to 0, as did a third, `plugin_publications_project_into_the_publishing_clients_navigation_lane`. That third test was not investigated, so I can't say this fix is why.

**New queue items:**
- **GH #265:** claimed and dispatched to a subagent in its own worktree. The fix makes `getActionUpdateThread() = BGT` explicit on the JetBrains actions that are missing it, and adds a test derived from the source that fails if any action lacks it.
- **"release + publish":** I claimed it for myself. v0.35.505 will ship once #265 is merged.

### Re: closing the formal-logic queue item — opus-5.5 · 2026-10-15T01:48-04:00

> **Queue prompt:** do your formal logic suggestions

This one is done; the details are in the response above. The previous closeout didn't strike it because its worker claim was still active, so I released the claim (`subagent:surfaceformal`) and am closing it here.

### Re: integrating the #265 subagent result — opus-5.5 · 2026-10-15T02:26-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/265

This is fixed and released, and the issue is closed.

- **Fix:** commit `e75e83f641`, `fix(jetbrains): declare BGT update thread on every action (#265)`. All eight actions named in the issue (Submit, Claim, ForceClaim, FixDocument, InitSession, LintDocument, RunWithJunie, LoadTmuxWindow) now override `getActionUpdateThread()` to return `BGT`. The runtime-built `DumbAwareGroup` does too.
- **Guard against regressions:** `ActionUpdateThreadTest` builds the action list from the compiled plugin classes and from `plugin.xml`. It fails if any action falls back to the platform's update-thread default or returns `OLD_EDT`. `SPEC.md` now has an "Action update threading" rule.
- **Landed:** the fix is on `main` through the `release-v0.35.505` merge and is tagged `v0.35.505`. The local `agent-doc` binary is already 0.35.505. Plugin builds: JetBrains 0.2.621, 262 0.2.622, 263 0.2.623.
- **Closed:** GitHub issue #265 is closed with a comment that cites the commit and the release. I also released the `subagent:gh265` claim.

> **Queue prompt:** release + publish

**Deferred:** the `v0.35.505` tag is pushed and the Release workflow (run 38690867071) is still running. Its `plugins` job and the linux-gnu build have passed; the musl, aarch64, macOS and Windows builds are still going. The tag push publishes the GitHub Release and PyPI automatically, so nothing is left to do by hand. Once that run is green, this item can be closed. The claim `coordinator:release-v0.35.505` is still held, and I've kept it so that a fresh drain doesn't start a second release. It expires about two hours after it was taken.

One more thing for you: preflight reports that the running JetBrains plugin is 0.2.618, and that's also the version installed on disk. That's behind the build this binary expects. To get the #265 fix into the IDE you'll need to install the new plugin (`agent-doc plugin install jetbrains`) and restart the IDE.

### Re: dispatching the Todo-placement item — opus-5.5 · 2026-10-15T02:39-04:00

The new queue item, "Place the TODO: below the backlog in the agent-doc template.", is claimed by `subagent:todoplace`. It is running in the background in its own worktree, `~/work/btakita/agent-doc-wt/todoplace`, on branch `feat/todo-below-queue`, which is based on `v0.35.505`. You changed the item from "below the queue" to "below the backlog" before I dispatched it, so the subagent is working from the backlog version. The branch name still says "queue".

The subagent's brief:
- Put `## Todo` / `<!-- agent:todo -->` directly after `agent:backlog`, which is before `agent:review`. Today it goes after review.
- Apply that in three places: the default template, the logic that creates the section when a document is missing it, and the `patch:todo` create path.
- Write down the fallback order for documents with no backlog, and add tests that pin the placement.
- Update the spec and any runbook text that still describes the old placement.

When it reports back, I'll integrate it, run one `make install`, and close the item. This document's own `## Todo` section is still below `## Review`. I'll move it once the new placement ships.

> **Queue prompt:** release + publish

**Deferred:** the v0.35.505 Release workflow (run 38690867071) is still running. The GitHub Release and PyPI are both still on 0.35.504. The `coordinator:release-v0.35.505` claim stays in place until that run finishes, so that a fresh drain doesn't cut a second release.

### Re: Todo placement merged and v0.35.505 published — opus-5.5 · 2026-10-15T02:58-04:00

> **Queue prompt:** Place the TODO: below the backlog in the agent-doc template.

This is done and installed. The subagent's commit `4afd57595c` ("feat(todo): place agent:todo directly below agent:backlog") fast-forwarded onto `main`, and `make install` from that commit exited 0.

- **Default template order:** Status, Exchange, Queue, Backlog, **Todo**, Review, Icebox, Done, Notes. Before this change, Todo came after Done.
- **When a document is missing the Todo section:** `--todo-add` and `patch:todo` share one placement function. It puts the section directly after the last `agent:backlog` (or the older name `agent:pending`). If there is no backlog, it goes after the queue. If there is no queue, it goes after the exchange. Otherwise it goes at the end of the document.
  - `agent:review` is no longer used as an anchor. Placing Todo after it would put Todo below Review. Placing it before the review marker would separate the `## Review` heading from its section.
  - A Todo section that already exists is not moved.
- **Specs and runbooks:** `specs/02-document-format.md` now has a "Default template order" paragraph. `specs/pending-system.md` has the updated `--todo-add` row. `runbooks/respond.md` and the 5 harness copies of it now say "right below `agent:backlog`".
- **Tests:** new tests pin these cases:
  - With a backlog and a review, Todo lands between them.
  - With several backlogs, Todo goes after the last one.
  - The fallbacks: queue, then exchange, then end of document.
  - A document with only a review section gets Todo at the end.
- **Verification:** the subagent's `make check` passed 10569 tests with exit 0, 167 skipped. CI for the new `main` (run 38695290461) is still running. The previous CI run is green.
- **Not done:** I didn't add a `VERSIONS.md` entry. That goes in with the next release.

This document's own `## Todo` section is still below `## Review`. The new rule only places Todo when a document is missing the section, and it never moves an existing one. I didn't hand-move it either, because the editor has this document open. To match the new order, cut the `## Todo` block and paste it right after the backlog in the editor.

> **Queue prompt:** release + publish

This is done. The v0.35.505 Release workflow (run 38690867071) finished successfully. The GitHub Release `agent-doc v0.35.505` is marked Latest (published 2026-10-15T06:40:40Z), and PyPI shows `agent-doc` 0.35.505. That release includes the #265 JetBrains fix. The Todo-placement change above landed after the tag, so it will ship in the next release.
<!-- agent:boundary:77c3a41f -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority -->
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
<!-- /agent:todo -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
