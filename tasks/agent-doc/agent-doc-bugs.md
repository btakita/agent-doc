---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0a767-3bf6-7c23-ab95-5df73c4a916b
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Editor-route layout supersede, controller bind-before-serve, the JB accept-leg bound, and the queued-clear mislabel are fixed and pushed (5577cfeeb, b0e9530a6, 232c6c103); make check 9316 passed. #fixrunlazily is gated on one IDEA restart to load plugin 0.2.380. Open: #tmuxautosyncwedge, #cargotokenrevoke.
<!-- /agent:status --> 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260914-210034.md`*

Compacted content:
- Archived 33 response topic(s): #timestampresponseheader; #idledispatchstack; preflight stall fixed; docs.md reverse-order lists traced to one cycle; 30 more
- Prior summary/context: prior compacted content: Archived 38 response topic(s): #actionable-review; #restartideaplugin restarted, do #pypiquo...
❯ Fix agent-doc causes to lazily.md stall:
```
› agent-doc /home/brian/work/btakita/agent-loop/tasks/software/lazily.md


• Agent-doc admission still fails because the document snapshot differs from HEAD without a recoverable cycle.

  From the session-owning pane, run:

  agent-doc commit /home/brian/work/btakita/agent-loop/tasks/software/lazily.md
  agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/software/lazily.md

  Then retry. Do not run agent-doc preflight or agent-doc claim.
```

### Re: #fixcauseslazily — gpt-5.6-sol · 2026-09-14 17:18 EDT

> **Queue prompt:**
>
> do [#fixcauseslazily]


Fixed the `lazily.md` admission stall in agent-doc and installed the corrected build.

The root cause was the closeout drift classifier comparing tracked-work component bodies as raw text. When queue projection added the ephemeral `🚧` marker to an otherwise unchanged backlog item, the classifier labeled the snapshot/HEAD difference as user-content drift. That bypassed the existing safe preflight auto-commit path and produced the repeated manual `commit` / `session-check` instruction.

The invariant is now explicit: a parsed `PendingItem.in_progress` difference is binary-owned projection metadata, while item id, checkbox state, task text, continuation, list shape, surrounding prose, and exchange content remain significant and fail closed. The policy owner remains `agent_doc_turn::closeout_recovery::classify_snapshot_head_drift`; it consumes the backlog crate's semantic signature instead of rebuilding tracked-work equality from raw strings.

Transition policy:

- boundary artifacts only → `BoundaryOnly`, recover;
- queue/status/frontmatter drift plus backlog `🚧` projection only → `MetadataOnly`, recover through the existing preflight auto-commit effect;
- exchange or real tracked-work content changes → `Content`, block;
- missing snapshot/HEAD proof → remain fail-closed.

Reactive topology is unchanged: baseline and HEAD are source facts, the shared pure classifier computes the recovery decision, preflight owns the auto-commit effect, and commit/snapshot receipts establish convergence. `session-check` consumes the same classification but remains diagnostic rather than silently committing a foreign live session. No polling, caches, or queue-state-machine redesign were added.

Implementation commit: `5e2e18d22` (`fix: classify backlog progress markers as metadata`). It adds the semantic backlog signature, classifier and preflight regression coverage, and the closeout specification update.

Verification passed:

- focused backlog signature regression;
- focused closeout classifier regression, including fail-closed id/state/text cases;
- admission regression reproducing snapshot/HEAD metadata drift with a projected backlog `🚧`;
- full `make check`;
- `make install` (`agent-doc 0.35.395`);
- latest GitHub Actions `CI` run: success.

The installed binary now diagnoses the live `lazily.md` mismatch as document-only drift. Its current `session-check` still requests the owning pane's commit, as designed; the next normal admission from that pane exercises the corrected preflight auto-recovery path rather than stalling on the marker as user content.

### Re: rename-safe panes and route-layout supersession — Codex · 2026-09-14 19:06 EDT

> **Queue prompt:**
>
> I renamed backend.md to api.md but agent-doc removed the tmux pane. Please fix agent-doc causes that prevent agent-doc from supporting file renames. Also, Run Agent Doc on this file is wedged or running somewhere else: editor route layout did not converge before dispatch: superseded_by_newer_layout_state

Fixed both causes and released agent-doc 0.35.396.

The pane loss came from an unreachable compatibility path: sync looked up the registry only by the new canonical path key, while the still-live pane remained keyed by `backend.md`. It therefore never reached its existing rename check. Exact-visible passive sync compounded that miss by suppressing ordinary ownership diagnostics, so it treated the pane as unresolved and let reconciliation remove it.

Sync now resolves the new path first, then considers a same-session old-path candidate only when exactly one registry row matches. It resolves relative registry paths against the document's owning project, requires the old path to be absent, and independently requires either the authoritative actor binding or live pane ownership proof against that old path. Only then does the existing idempotent registration effect rekey the same pane to `api.md`. A live old path, duplicate session rows, a conflicting new-path row, or weakened ownership remains fail-closed.

The route wedge was a separate generation race. `Run Agent Doc` published a layout, but passive editor observations could supersede it repeatedly. The RPC handler republished once and then failed if another observation arrived. Route readiness now belongs to the retained pane-layout graph: it follows newer generations while they still contain the routed document, waits through `NeedsEffect` / `Applying` / `RetryPending`, and accepts a covering `Converged` receipt. A newer layout that removes the document, an absent projection, or operator ownership still refuses dispatch. The RPC publishes exactly one intent.

Reactive ownership is now explicit: editor/path/layout observations are Sources; registry matching and route readiness are pure derived decisions; tmux re-registration and reconciliation remain the existing idempotent Effects; registry/effect receipts plus physical layout observation prove convergence. The only compatibility fallback is the bounded, fail-closed recovery for a missed editor rename event.

Regression coverage includes unique old-key rename recovery, live-old-path refusal, duplicate-session ambiguity refusal, covering newer route generations, non-covering supersession, operator-owned refusal, and the single-publication route handler. Specifications, README, release parity, and generated skill/version surfaces were updated.

Verification passed:

- focused `agent-doc-sync`, `agent-doc-sync-io`, and `agent-doc-controller-io` tests;
- full `make check`;
- live `make tmux-ci`;
- `make install`, including native library and editor packages;
- forced two-controller recycle; the root controller is stable on 0.35.396 with a matching installed inode.

Implementation: `1d8fd3996` (`fix: preserve pane ownership across document renames`), pushed to `origin/main` and tagged `v0.35.396`. The new CI, Release, PyPI, Book, and Pages workflows were triggered and were queued/running at the required one-time external check.

### Re: #fixbackloglog — gpt-5.6-sol · 2026-09-15T09:28:18-04:00

> **Queue prompt:**
>
> do [#fixbackloglog]


Implemented and released both fixes in agent-doc 0.35.397 / JetBrains plugin 0.2.386.

Backlog backfill now removes only complete top-level fenced terminal transcripts carrying the `› agent-doc ...` invocation signature. It preserves ordinary fenced prose, unmatched fences, indented continuation, headings, and real work items. The live transcript was removed through the supported backlog command, and a second pass proved the document already canonical.

Split-editor activation now enters through IntelliJ's global editor event multicaster, is filtered to the owning project, and reuses the existing generation-guarded focus-only command-plane path. This avoids restored-editor attachment timing gaps without adding polling or tmux policy to the plugin.

Verification passed: targeted Rust and JetBrains regressions, `make check`, `make tmux-ci`, plugin packaging, clean-tree local installation, and version checks. Source commit `ebe8d0a54`, superproject commit `e3a9fbcce`, and tag `v0.35.397` are pushed; release workflows were in progress at inspection.

The installed IntelliJ package requires an IDE restart before the running process can load this listener generation, so the remaining human eyeball check is recorded as non-blocking `[operator-verify]` follow-up `#panefocussplitverify`.

### Re: #realtimequeuedupe and the FPE closeout race — gpt-5.6-sol · 2026-09-15T12:20-04:00

> **Queue prompt:**
>
> The queue item `- Fix the duplicated agent:queue item that occurred as I was typing in the queue item. The duplicate is "- Please continue the fpe query optimization. How can we reduce the number". Fix the race condition so it cannot occur again. Previous attempts have been made but this issue is still occurring.` in this document and `- Write the plan into a document. Can we make a ADR? The ADR belongs in the fpe repo since it is internal to the fpe. Use the decisions directory format that we use in haiven-docs.` in fpe.md was not removed once completed. Fix the agent-doc causes.

Fixed and released as agent-doc 0.35.398.

Your correction identified the cross-document provenance: the FPE turn wrote `[#realtimequeuedupe]` into this document. Commit `d7b45df768` captured that new backlog item and its queue prompt. The FPE ADR prompt was a newer editor-buffer item, however, not work completed by the preceding query-optimization response; the FPE closeout remained interrupted by queue-projection divergence, with its editor buffer authoritative. That ADR item therefore correctly remains pending for the controller's next generation-fenced closeout rather than being falsely consumed.

The agent-doc defect was at the commit boundary. In the FPE cycle, the controller logged `answered_free_text_strike_projected` before commit, but that receipt meant only that a detached document-actor job had been enqueued. The main closeout committed, then the actor applied the queue strike after the commit and reported that no cycle/capture owned the resulting visible edit. This stranded the strike outside HEAD and let the completed-looking queue item survive.

The fix makes the invariant explicit:

- the controller records an applied receipt only after the exact document-actor editor/CRDT projection completes;
- closeout refuses to commit unless that receipt matches the current projection ID;
- the projection no longer launches a detached second git commit;
- after the queue strike advances the relay canonical, closeout repeats the generation-fenced native-save proof and then performs one git commit.

A regression advances the editor buffer ahead of the projected authority and proves that closeout fails instead of treating admission as application. The existing success regression and architecture guard now require actor `submit`, the exact applied receipt, and no projection-owned commit.

Verification passed: focused receipt and success regressions, the queue architecture guard, editor parity, cross-editor SimWorld, and the full `make check`. Source commit `c68f2b92f` is pushed to `origin/main` and tag `v0.35.398` is published. The clean release install completed for `agent-doc 0.35.398`, native library 0.35.398, JetBrains plugin 0.2.386, and VS Code extension 0.2.70; controllers and supervisors were scheduled for two-phase idle recycle. GitHub CI and PyPI workflows were in progress at the required one-time inspection.

The IDEA restart is accepted as the operator completion for `#panefocussplitverify`, so that item is reaped in this closeout as requested.

### Re: cheaper macOS release builds — gpt-5 · 2026-09-15T13:46-04:00

> **Queue prompt:**
>
> The gh bill the OSX build are too high. Please propose a cheap alternative to the current build process.

The first important finding is that this repository is not currently accruing paid macOS Actions time. `btakita/agent-doc` is public, both Darwin legs use standard hosted labels (`macos-14` and `macos-latest`), and the latest release run's timing API reports `MACOS.total_ms = 0` under `billable`. GitHub's current policy says standard hosted runners are free for public repositories; the listed private-repository rates are $0.062/minute for standard macOS and $0.006/minute for standard Linux. Sources: [GitHub Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions), [runner pricing](https://docs.github.com/en/billing/reference/actions-runner-pricing), and [latest release usage](https://github.com/btakita/agent-doc/actions/runs/34994074751/usage).

The gross compute is nevertheless high. There were 77 Release runs from September 1 through September 15. In the latest ten releases, the 20 Darwin jobs used 306.45 wall-clock minutes and 316 rounded runner minutes. At the published macOS rate that is a nominal $19.59, or about $1.96 per release, although this public repository received a full discount. The latest run also shows why: the two Cargo builds consume roughly 14.5 and 15.5 minutes; setup and packaging are negligible.

My recommended cheap process is:

1. Batch releases. Merge fixes continuously, but cut at most one tagged binary release per day (or on an explicit release cutoff) instead of publishing every individual fix. At the recent rate, 15 daily releases instead of 77 would remove about 80% of the Darwin build executions without changing toolchains or artifact trust. This requires deliberately replacing the current “release every shippable fix” project policy.
2. If keeping every-tag releases is required, move the two Darwin compile-only legs into one Ubuntu job using a digest-pinned `cargo-zigbuild` container. Build `x86_64-apple-darwin` and `aarch64-apple-darwin` separately and retain the existing per-target archive names, executable-plus-cdylib packaging guards, and `SHA256SUMS` contract. `cargo-zigbuild` explicitly supports both targets and a universal2 mode: [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild).
3. Keep one scheduled and manually dispatchable native macOS smoke workflow. It should download the newest two archives on a real Mac, run `agent-doc --version`, verify `lib-path` resolves the sibling `.dylib`, and exercise one FFI call. That preserves platform confidence without paying for two cold native compilations on every tag.

If the repository later becomes private, the Linux cross-build reduces the runner rate by about 90.3% per minute. At the sampled workload and equal runtime, the nominal ten-release compute would fall from $19.59 to about $1.90, plus the small periodic native-smoke cost. I would prove both cross-built archives on Intel and Apple Silicon before removing the native release legs; confidence is high in the billing diagnosis and medium in cross-build compatibility until that spike passes.

No release/build files were changed in this proposal-only cycle. The authenticated CLI can inspect repository run usage but lacks the account-level `user` scope, so it could not audit the overall personal billing report. Any nonzero macOS charge on the account should therefore be traced by repository in that report; it is not supported by this public repository's current run-level billable data.

### Re: FPE queue update after IDEA restart — gpt-5 · 2026-09-15T14:04-04:00

> **Queue prompt:**
>
> fpe.md agent:queue was not updated when the turn completed. Fix agent-doc causes.

The IDEA restart completed the runtime activation and the FPE queue is now converged. No additional source patch was needed in this cycle: the underlying defect was already fixed and released in agent-doc 0.35.398 by `c68f2b92f` (`fix: make queue strike closeout atomic`), while the originally observed turn was still attached to the pre-restart JetBrains/native runtime generation.

The invariant is owned by the controller closeout gate: a turn may not commit until the exact queue projection has an applied receipt from the document actor. Admission/enqueue alone is not application. The covered transitions are: an enqueued projection without its matching applied receipt blocks commit; a matching receipt permits closeout; after a queue strike advances canonical authority, closeout repeats the generation-fenced native-save proof and performs one git commit; stale or mismatched receipts remain fail-closed.

The reactive path is `queue projection intent Source → document-actor submit Effect → exact applied-receipt Source → closeout Computed gate → native-save proof Effect → commit receipt`. The fix removed the detached projection-owned commit, so there is one closeout owner and no polling or second commit path.

Current proof after the restart:

- `tasks/fpe.md` has an empty `agent:queue` on disk.
- `agent-doc doctor tasks/fpe.md` reports supervisor freshness OK, editor/CRDT convergence OK, and closeout commit OK.
- Installed CLI reports agent-doc 0.35.398; the agent-doc source HEAD is `c68f2b92f` and matches `origin/main`.
- GitHub Actions CI run 34994060782 for `c68f2b92f` completed successfully.

The restart therefore satisfies the remaining runtime/operator verification. There is no further migration or operator-verify work for this incident.

### Re: Response

> **Queue prompt:** Batch OSX releases to 1 week.

Implemented weekly batching as one complete release train rather than publishing
tags whose macOS assets arrive later.

- Added `make release-cadence-check` and made `make release` depend on it. The
  gate queries the latest published GitHub Release, computes the next eligible
  instant at exactly seven days, and fails closed before tagging when GitHub
  state is missing, invalid, or too recent.
- Kept all six archives atomic: both Darwin targets remain in the same tag-driven
  workflow as Linux and Windows. Normal fixes are still checked, installed,
  committed, pushed, and recycled immediately; only version projection,
  `VERSIONS.md`, the full install, tag, and publication are batched.
- Documented `#weekly-release-batch` in the command spec and development release
  procedure, and added a regression test for the Makefile gate, seven-day
  constant, fail-closed GitHub query, and complete six-target matrix.

Architecture: the latest GitHub `publishedAt` value is the external source,
`next_release_at` is the pure derived state, and the Make target is the effect
boundary that may tag only while the computed window is open.

Verification passed: the focused Rust regression, the deterministic Python
self-test, Python bytecode compilation, and the full `make check`. The live gate
correctly refuses another tag after `v0.35.398` until
`2026-09-22T16:41:52Z`. The local binary/library/skills and both editor packages
were installed. Commit `5635590d9` (`release: batch complete releases weekly`)
is pushed to `origin/main`; no tag was created, and GitHub registered no workflow
run for this non-tag push.

### Re: The last fpe.md turn did not commit. Fix agent-doc causes. — gpt-5 · 2026-09-15T16:56:53-04:00

> **Queue prompt:** The last fpe.md turn did not commit. Fix agent-doc causes.

Fixed the agent-doc causes and recovered the missing FPE commit.

The failure had two linked causes. First, an answered free-text strike deferred on editor delivery was not reactive to the retained-delivery frontier, so delivery could converge without retrying the strike Effect or recording its application receipt. Second, older terminal cycles had no safe commit recovery when that delayed strike landed after the response commit—or when normal queue cleanup reaped the completed row before recovery ran.

The controller now re-drives the same captured strike when retained delivery advances. Commit recovery is fenced to the terminal capture and committed `HEAD`: it accepts only the exact derived strike or the exact committed queue with those captured node keys removed. A later recurring prompt or any other queue edit still fails closed.

The real `tasks/fpe.md` case is recovered and clean at commit `6b058f3`. Its direct session check reports cycle `cycle-1789496338218` committed with `commit_success` and no drainable work.

The Infra retained response was retried only through `session-check`, as required. Delivery recovery advanced, then correctly stopped on the separate `#mergeorderremove` guard because runnable queue work was removed while its backlog item remains open. I did not resubmit the response or force disk.

Regression coverage includes the initial delivery-CAS failure/retry, terminal late-strike recovery, committed-`HEAD` fencing, exact post-strike reap recovery, and rejection of an identical recurring prompt. Full `make check` passed after the final change. Source commits `bc18f121c`, `27b77801b`, `8b1e18117`, `d7e2eb1f6`, and `e94411dd8` are pushed to `main`; the corrected binary is installed and the relevant controllers were recycled.

Per the weekly macOS release cadence, this was installed and pushed without creating another tag inside the one-week batch window. The session accretion warning remains informational; only an explicit operator clear resets it.

### Re: FPE turn completion wedge — gpt-5 · 2026-09-15T17:40:55-04:00

> **Queue prompt:**
>
> fpe.md turn did not complete. Fix agent-doc causes for the wedge.

Fixed and pushed as `eb13e70dc` (`fix: classify compressed stash panes as unobservable`).

The FPE turn had actually completed: `cycle-1789496338218` was committed with `commit_success`, its response was persisted, and its PR checks were green. The apparent wedge was an observation defect. Pane `%5` had been moved to the dense tmux `stash` window and compressed to one row; at that height Codex's composer was outside the captured viewport. `agent-doc session status` interpreted the absent prompt chrome as positive busy evidence and emitted `owned_pane_ready_busy_conflict`, even though the actor, supervisor, and controller lease all said `ready`.

The pane-evidence policy now distinguishes three transitions:

- a visible dispatch-ready composer is `alive-idle`;
- an explicit draft or busy cue remains `alive-busy`;
- a blank or sub-four-row viewport without positive busy evidence is `alive-unobservable`, with `prompt_ready=unknown`.

This keeps completion authority with the committed document cycle and supervisor/controller lifecycle instead of deriving it from lossy tmux geometry. The new state is deliberately fail-closed for clear and restart operations, so the fix removes the false wedge signal without permitting input into a pane whose composer cannot be observed. Redesigning stash storage/layout was out of scope because stashing itself behaved correctly.

Verification completed:

- focused classification, draft-preservation, normal-busy, status-conflict, and clear fail-closed tests pass;
- `make check` passes: 9,472 tests passed and 259 skipped;
- the installed `v0.35.398` development build reports the real one-row FPE pane as `alive-unobservable`, actor/supervisor `ready`, with no ready/busy warning;
- restoring the pane to eight rows reports its Codex composer normally, and `agent-doc session-check` still proves the FPE cycle committed with no drainable work;
- GitHub Actions run `35026894043` passed;
- the supervisor/controller recycle handoff was requested after installation.

No release was cut because the latest tag was published today and the seven-day release cadence is still closed. Unrelated in-progress JetBrains and stale-window-remedy edits were left uncommitted.

### Re: $20 GitHub billing cap and operator-built macOS releases — gpt-5 · 2026-09-15T19:17:45-04:00

> **Queue prompt:**
>
> I updated my payment info in Github. Please apply a budget of $20/month to all GH billing so I don't get another large bill. Also remove the build for OSX. I'll run the builds on my mac mini & upload periodically.

Applied the safest account-wide equivalent of a $20 monthly GitHub metered-spend cap.

GitHub budgets are product-scoped rather than one combined account-total budget. The personal account now has GitHub Actions set to a **$20/month hard budget**, with **Stop usage when budget limit is reached** and threshold alerts enabled. The other configured metered products—AI credits, Codespaces, Packages, and Git LFS—remain hard-limited to **$0**. That permits at most $20 of configured metered overage instead of raising five product budgets to $20 each. I did not loosen any organization budgets that were already hard-stopped at $0. GitHub's budget behavior and hard-stop option are documented in [Setting up budgets](https://docs.github.com/en/billing/how-tos/set-up-budgets).

Removed hosted macOS release builds from `btakita/agent-doc`. The release matrix now contains only four GitHub-hosted targets: three Linux archives and one Windows archive. Neither `macos-14`, `macos-latest`, nor an Apple-Darwin target remains in the Actions workflow.

Added the operator-side replacement:

```bash
make release-macos-assets TAG=v<version>
```

Run that on the Mac mini when Darwin artifacts should be refreshed. It builds both `x86_64-apple-darwin` and `aarch64-apple-darwin`, requires `agent-doc` plus `libagent_doc.dylib` in each archive, downloads the release's existing platform archives, regenerates `SHA256SUMS` across the complete asset set, and uploads both Darwin archives plus the refreshed manifest with clobber semantics. The release specification and maintainer instructions now make the hosted-vs-operator ownership explicit.

Verification and delivery:

- focused release-policy tests passed;
- full `make check` passed: 9,472 tests passed, 0 failed, 259 skipped;
- clean-worktree `make install` succeeded for the binary, native library, skills, JetBrains package, and VS Code package;
- private-name denylist scan found no matches;
- source commit `59896e4cf` (`ci: move macOS release builds off hosted runners`) is pushed to `origin/main`;
- parent pointer commit `f74aead31` records the source update;
- GitHub Actions CI run [35034937058](https://github.com/btakita/agent-doc/actions/runs/35034937058) was in progress at the required one-time inspection. Local verification is the closeout gate, so it was not polled.

No release tag was created; the existing weekly release cadence remains in force.
❯ Fix agent-doc causes for wedged fpe.md run:
```
› agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md


• Agent-doc admission failed because the previous cycle is still preflight_started with the #lookslikemerge prompt
  unresolved and no captured response.

  Wait until that cycle becomes stale, or restart the harness pane and rerun:

  agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md

  Do not run agent-doc preflight manually.
```

### Re: fix causes of wedged agent-doc run — gpt-5

> **Queue prompt:**
>
> do [#fixcauseswedged]


Fixed the admission race that wedged the `fpe.md` queue run.

The trace showed two preflights for the same dispatch. Queue admission opened `cycle-1789514467953` as `preflight_started`; the subsequent Codex `UserPromptSubmit` hook re-entered preflight about 30 seconds later. That hook subprocess did not expose a Codex ambient marker, so ownership detection resolved its harness as `default`, rejected the otherwise-correct owner-pane match, and treated the fresh cycle as an interrupted foreign cycle. With no captured response and the stale-cycle TTL not yet elapsed, recovery correctly failed closed—but the admission path had misclassified its own cycle.

The policy boundary is now explicit:

- preflight options carry a typed invocation origin: direct CLI, Claude hook, or Codex hook;
- hook admission supplies its known harness provenance instead of relying on ambient subprocess variables;
- recovery still requires both the authoritative actor pane and harness to match before preserving a fresh uncaptured cycle;
- direct CLI behavior remains ambient, while foreign-pane and harness-handoff cases remain fail-closed.

This is a one-shot admission-boundary correction; it adds no polling, timer, or parallel recovery path. I also documented the invariant as `#hook-owned-cycle-reentry` and added a regression assertion for explicit hook provenance.

Verification passed:

- focused provenance and Codex hook tests;
- workspace compile across all targets;
- full `make check`;
- `make install`, including binary/library, skills, and editor artifacts.

The installed generation emitted its normal auto-recycle/reload event; an explicit root-level recycle found no root controller to recycle. Source commit `28240ec14` is pushed to `agent-doc/main`; parent gitlink commit `414a7ccd7` is ready to push with this response closeout. The latest CI run visible at the required one-time check was green; the new push had not yet appeared in the listing.

I left the large exchange intact because this document still has active queue work.

### Re: Response

Yes—that explains both symptoms. This is effectively a zombie-owner lease, not a genuine concurrent-write conflict:

1. The original writer acquired ownership.
2. Its PTY detached before response capture.
3. The writer process remained alive and continued renewing the lease.
4. New closeout attempts therefore could not expire or steal the lease.
5. Meanwhile, the editor delivery registration disappeared, leaving CRDT state retained but no worker capable of applying it.

Recovery takes so long because the system treats “process alive + renewed lease” as stronger evidence than “no forward progress.” Normal lease expiration never happens, and retries use backoff while waiting for a state transition that the detached writer cannot produce. The `no_live_registration` result means the requested editor re-registration also had nobody to notify.

So the real defect is missing progress-sensitive lease revocation: a writer that has neither captured a response nor advanced its closeout phase should eventually lose ownership even if its process remains alive. Terminating that writer breaks the false ownership proof and lets the prepared closeout proceed safely.
<!-- agent:boundary:93b90753:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 Fix all underlying causes of agent-doc churn on fpe.md.
- Fix Run Agent Doc wedge on fpe.md: authoritative actor generation 16 for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md owns pane %5 but route could not drain the active closeout before dispatch: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: resume durable checkpoint cycle=cycle-1789527700856 phase=response_captured capture_id=cycle-1789527700856; the response is already captured; retained write and commit recovery own the continuation; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md
<!-- /agent:queue -->


<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#release035376verify] [release] Verify GitHub Release run 34737823202 completes green and v0.35.376 publishes all six platform assets; all matrix jobs were still externally queued at closeout.
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#fpecompactheal] [operator-verify] Confirm fpe.md's corrupted `<dynamic_context_ref>` block is gone after one compact on the FIXED binary (installed 0.35.367, ~/.cargo/bin/agent-doc at 23:01:39). Must be run FROM THE PANE THAT OWNS src/haiven-dev/tasks/fpe.md — that is a live session (b69e5aeb, `queue: go`) and this cycle deliberately did not hand-align its disk. Expected after `agent-doc compact src/haiven-dev/tasks/fpe.md`: exactly ONE block, with the two self-referential handles (absolute `.../src/haiven-dev/tasks/fpe.md` and relative `tasks/fpe.md`) gone, leaving at most the four instruction-file handles; and the `Prior summary/context:` line still summarising the prior compact list rather than quoting XML.
- [/] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate installed JetBrains plugin 0.2.379, then keep agent-doc-bugs.md and the cross-root infra.md visible, click each embedded tmux pane, and verify editor selection follows without stealing focus from an actively focused editor component. Automated lifecycle and transition regressions plus make check tmux-ci passed; source v0.35.362 is published. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] Installed JetBrains plugin 0.2.379 is staged while running IDEA PID 1035669 remains on 0.2.378; one restart activates both the reverse tmux-focus fix and the current package generation. Remaining splice proof: captured local edits recover across an independently advanced canonical response in a disposable live editor document. The original retained capture committed as 1b35dcb028, and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Current prerequisites verified: IntelliJ PID 1035669 runs plugin 0.2.378; CLI/native 0.35.361 is installed. Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal/respawn storm. Unblocks: provide the disposable document path and perform the real unsaved editor edit; persisted CLI writes cannot substitute for this proof. The unrelated Stop-hook gate-selection defect is fixed in v0.35.361; plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ha3hpolicy] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml receive.auto_create_components = ["pending"] (or = false); flip notify/write default from auto-create to consult-recipient-config. Hard-blocked until multi-user workspace support exists; issue #40 (open) explicitly says not to ship before that prerequisite. Moved out of agent:review because it is an unstarted future feature, not code-complete work awaiting validation. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
