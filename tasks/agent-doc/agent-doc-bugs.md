---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a08ca6-2d17-7822-948a-aaff42a157ac
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

Editor-route layout supersede, controller bind-before-serve, the JB accept-leg bound, and the queued-clear mislabel are fixed and pushed (5577cfeeb, b0e9530a6, 232c6c103); make check 9316 passed. #fixrunlazily is gated on one IDEA restart to load plugin 0.2.380. Open: #tmuxautosyncwedge, #cargotokenrevoke.
<!-- /agent:status --> 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260910-212720.md`*

Compacted content:
- Archived 20 response topic(s): Compact commits, editor routes, and controller timeout; GH #49/#51 and the extra shell panes; #runfpecodex; 17 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): Response; #fixqueueitem; #fixhttpsgithub; 3 more

### Re: #actionable-review — review triage — opus-5

> **Queue prompt:** #actionable-review

Audited all 8 gated `agent:review` items against live state rather than their prose. Three were not actually "awaiting review" — they were concrete actions with the action itself buried in a paragraph — and one was a future feature filed in the wrong component. Those four are now out of `agent:review` (8 → 5), with the three real actions promoted to `agent:backlog`.

**Promoted to actionable backlog:**

- **`#restartideaplugin`** — IDEA PID 1035669 has 17h57m uptime and preflight still reports plugin 0.2.378, so the staged 0.2.379 package has never loaded. This single restart is the shared prerequisite of `#tmuxfocussyncverify` and `#activateinstalledjetbrai`, and satisfies the plugin floor for `#harnessswitchverify`. It was implicit in three separate review paragraphs and named as a step in none.
- **`#pypiquotafix`** (was `#pypi345quota`) — the old text was stale in both directions. PyPI's latest is **0.35.343**, not 0.35.358, and 0.35.343 itself uploaded only **1 of 4 wheels**. Every PyPI run since has failed identically (`34521723248`: `400 Project size too large`). Measured live: **10.726 GB across 165 releases, avg 65.0 MB**. Two real paths — request a limit increase (non-destructive), or delete the 34 oldest releases `0.35.0`–`0.35.112` (all pre-2026-08-02) to free 1.75 GB → 8.98 GB, ~27 releases of headroom, permanent and version numbers unreusable. Shrinking wheels is not a third path: `[profile.release]` already runs `lto = "fat"`, `codegen-units = 1`, `strip = true`.
- **`#cargotokenrevoke`** (was `#cargo355token`) — tagged `[operator-verify]` but the outstanding work is the rotation, not a verification. Checked the blast radius so the revoke can proceed without hesitation: agent-doc has no `cargo publish` step and no `CARGO_REGISTRY_TOKEN` in any of its five workflows, so revoking cannot red an agent-doc job.

**Moved to icebox:** `#ha3h` → `#ha3hpolicy`. Issue #40 is open and explicitly says not to ship before multi-user workspace support exists. That is an unstarted future feature, not code-complete work awaiting validation, so it does not belong in `agent:review`.

**Left gated, blockers confirmed real:** `#tmuxfocussyncverify`, `#activateinstalledjetbrai`, `#harnessswitchverify` (all need a live IDE plus human interaction after the restart), `#grokmcpverify` (external quota), `#coder7` (needs a reachable Coder workspace).

**No agent-drainable work came out of this.** All three promoted items are operator-executed, so they carry `[operator-verify]` and go-mode will correctly defer them rather than churn the drain. The value here is that three one-line actions are now visible in the actionable list instead of buried in eight paragraphs of gated prose.

No code, test, or build surfaces changed this cycle — the mutations are confined to this document's tracked-work components, so no suite run applies. The standing `stale_plugin` warning (live 0.2.378 vs shipped 0.2.379) is exactly what `#restartideaplugin` now tracks.

### Re: #restartideaplugin restarted, do #pypiquotafix, do #cargotokenrevoke — opus-5

Three queue heads, oldest-first — plus an agent-doc defect that this turn tripped over and fixed, because the first `finalize` refused the cycle.

**`#restartideaplugin` — done.** Verified rather than taken on faith: PID 1035669 is gone, IDEA is now PID 13006 started `Thu Sep 10 18:41:13`, and the staged `agent-doc-jetbrains-0.2.379.jar` was written at `15:38:33`, so the running IDE loaded it. This turn's preflight carried **no `stale_plugin` warning** — that check is the binary's own live-vs-shipped comparison and it had been firing every cycle until now. The plugin floor for `#tmuxfocussyncverify`, `#activateinstalledjetbrai`, and `#harnessswitchverify` is satisfied; those three still need the human-interaction half of their proofs.
❯ 52
**`#pypiquotafix` — took path (a), the non-destructive one. Filed https://github.com/pypi/support/issues/12214** ("Project Limit Request: agent-doc - 25 GiB"). I did **not** touch path (b): deleting 34 releases is irreversible and burns those version numbers permanently, so that stays your call.

Re-measured live before filing, and one number is worse than the backlog text:

| | |
|---|---|
| Total | **9.990 GiB** across 165 releases (10.726 GB) |
| Average release | 62.0 MiB (4 wheels); largest 68.1 MiB (`0.35.327`) |
| Latest | `0.35.343` — **1 of 4 wheels**, macOS x86_64 only |
| First upload | 2026-07-17 |
| **Releases in the last 30 days** | **82** |

That last row is the actual problem. 82 × 62 MiB is **~5 GiB/month**, so a granted 25 GiB buys roughly three months and then we are back here. The limit increase is a stopgap, not the fix — I said so plainly in the issue and committed to reducing cadence, so that is now a public commitment attached to the request. I did not change the publish policy myself: `pypi.yml` fires on every `v*` tag and that is yours to set, so it is queued as `#pypicadence` at the top of the backlog with three concrete options. I verified before asserting it publicly that `[profile.release]` really does carry `lto = "fat"`, `codegen-units = 1`, `strip = true`, so there is no wheel-shrinking path left to claim.

**`#cargotokenrevoke` — blocked one click short.** `https://crates.io/settings/tokens` is open in the bridged browser (tab 13, the spare New Tab — your other 19 tabs are untouched) and returns **"This page requires authentication"**: that Chromium profile has no crates.io session and there is no saved `chromium-bridge state` snapshot. crates.io token management is session-cookie-gated, so the exposed token cannot revoke itself through the API either. Logging in is a GitHub OAuth handshake and I am not driving that on your account. **Click "Log in with GitHub" in tab 13** — OAuth lands you straight back on `/settings/tokens` — then re-queue the item and I will enumerate and revoke in-session. Blast radius is still nil: no `cargo publish` step and no `CARGO_REGISTRY_TOKEN` in any of the five workflows.

### The defect this turn hit — fixed, not filed

The first `finalize` of this response was refused:

> `[finalize] pre-write gate: selected free-text queue prompt lacks response evidence: #actionable-review.`

`#actionable-review` was **last** turn's queue head. It was answered and committed last cycle, and you removed it from the queue in this turn's edit — preflight's own `node_events` record the removal. No response written this turn could ever quote it, so the cycle was wedged asking for a quote of a prompt that no longer exists.

Root cause, `agent-doc-write-runtime-io/src/run_entry.rs`: the guard is `selected_free_text_heads_missing_response_evidence(baseline, content, response)` and it deliberately takes baseline and current content as **separate** arguments — selection is read from `content`. Four of the five call sites pass `(baseline, &current_content)`. The fifth, on the mutation-bearing strict-stream path, passes `pre_capture_content`, which is `baseline.or(pre_capture_current_content)` — so whenever a preflight baseline exists, the gate reads the **previous committed turn's queue** and treats its heads as this turn's selection. That path is the one that runs whenever the turn carries tracked-work mutations, which is nearly every drain cycle.

The obvious repair — resolve live authority there — is wrong: that call site sits deliberately *before* authority resolution, because moving the editor delivery barrier ahead of the mutation capture is exactly the response-loss this function exists to prevent. The witness is instead the durable on-disk document, which is the same surface preflight diffed against the snapshot, with the baseline as fallback.

Regression coverage, and I proved the test detects the bug rather than trusting it: reverting just the one-line witness swap makes `strict_stream_closeout_gates_free_text_queue_on_current_document_not_baseline` fail with the exact production error text, and restoring it passes. The test also asserts the converse — the same response is still refused while the head is live on disk — so the guard is relocated, not weakened. A second test in `queue_closeout_guard.rs` pins the two-argument contract that every existing test had been collapsing by passing the same string twice.

**Second red, also fixed:** `make check` was already failing on `main` before my change, independent of it — `tmux_layout_observation_prefers_effect_assignment_over_partial_actor_store` asserted `/repo/nested/tasks/secondary.md` and got `/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md`. The precedence it tests lives entirely in the pure resolver `layout_sync_state_actual_document_from_observations`, but the test routed through the IO wrapper with `configured_tmux()`. On a machine that really has a pane `%2`, that resolved a live process owner — which correctly outranks every projection — so the test read this machine's own tmux. Pointed at the pure resolver with `process_owner: None`; the live-owner-wins case was already covered by the adjacent test, so no coverage was lost. It passes on CI only because CI has no such pane.

**Verification:** `make check` → **9305 passed, 0 failed** (the first run had stopped after the tmux failure with 7081 tests unrun). Built and installed. Latest CI run `34521712512` is green. Committed `7ccf1c822` in `src/agent-doc` and pushed; parent gitlink `1b6ff49e4`.

### Re: #deferredmutdrop, #pypicadence, GH #52 + release + publish — opus-5

Three heads, oldest-first. All three landed; **v0.35.363 is released and its assets are verified**, and one of them found a second defect that only a tagged CI run could surface.

### Re: #deferredmutdrop — fixed, and the report's own root-cause hypothesis was wrong — opus-5

The backlog text asked whether the retained canonical content is computed before the mutation plan is folded in, or whether the resume replays only the response capture. **Neither.** The capture is fine: `closeout_mutation_plan_captured_before_authority_resolution` fired at 23:04:53 with `plan_hash=49ecccc2`, so the plan was attached to the same capture exactly as `capture_validated_stream_closeout_before_authority_resolution`'s doc comment promises.

The ops log names the real path. `resume_captured_finalize` has two branches, and only one of them knows about mutations:

```
23:04:53  captured_finalize_resume_started      → captured_finalize_resume_waiting_for_state
23:04:55  captured_finalize_resume_started      → captured_finalize_resume_waiting_for_state
23:04:57  captured_finalize_resume_started
23:04:59  commit_success  len=17002
23:05:00  captured_finalize_resume_committed  repair_outcome=ContinuedWriteAppliedCapture
```

`ContinuedWriteAppliedCapture` is the `WriteApplied | Committed` branch — `resume_materialized_captured_finalize`. Its `#writeappliedcontinuation` guard is correct about the *response*: once that phase is durable, replaying the command would recapture after pending intent was cleared. But a capture reaches `write_applied` the moment the response cell is durable, and the tracked-work half runs **after** that write, where the same no-live-editor barrier retained it — and the retained document write carries only the response target. So the branch that continues straight to commit is precisely the branch with no mutation plan in hand. Only the other branch, `resume_captured_finalize_intent`, decodes `capture.mutation_plan_json`. Every mutation-bearing drain cycle whose response lands first takes the wrong one.

**The fix replays the tracked-work half before the commit continuation**, as a `--backlog-only` write with `CommitMode::None`, so the continuation still commits both halves as one transaction rather than producing a second commit. Two things keep it safe:

- It is gated on `recorded_tracked_work_is_unlanded` — the same `#retainedmutdrop` predicate `commit` and `session-check` already share. A closeout whose mutations *did* apply is left alone, and a resume that fires repeatedly cannot double-apply.
- A replay that cannot land is propagated, not swallowed: the resume reports `WaitingForSignal` instead of `Committed`, which is the fail-closed behaviour the item asked for.

**I proved the test detects the bug rather than trusting it.** Removing just the replay call reproduces the production shape exactly — response committed, backlog byte-identical:

```
the captured --backlog-add must not be dropped by the resume:
&lt;!-- agent:backlog --&gt;
- [ ] [#fix1] original next action
&lt;!-- /agent:backlog --&gt;
## Assistant
Response whose tracked-work half never applied.
```

One narrow gap I am not hiding: the witness predicate only inspects `--done` and add ids, so a captured plan carrying *only* gates or edits still slips through. Last cycle's plan carried a `--done` and an add, so it fires here — but that is luck, not coverage. Queued as `#mutplanwitness`.

### Re: #pypicadence — option (a), plus the check that makes drift loud — opus-5

Took **(a)**: `pypi.yml` now fires automatically only on milestone tags (`v[0-9]+.[0-9]+.0`). Every tag still produces a GitHub Release from `release.yml`, and any single tag publishes deliberately with `gh workflow run PyPI --ref <tag>`. I did not touch tag frequency (c) or add a destructive prune (b) — both change things that are yours.

Proof the gate works, not just that it is written: pushing `v0.35.363` triggered **Release** and **did not** trigger PyPI.

The cadence gate alone would have made GH #52's part-2 symptom *worse* — PyPI drifting further behind with nobody noticing is the whole complaint — so it ships with the reporter's own suggestion: a `verify` job asserts the tagged version is resolvable on `pypi.org` **after** the publish job reports success. A quota rejection or a silently skipped upload now reddens the run at the moment it happens instead of surfacing 20 versions later through a user's `pip install --upgrade` downgrading them.

At the measured 82 releases / 30 days this cuts PyPI uploads from ~5 GiB/month to roughly one minor bump's worth, which is what I committed to publicly in pypi/support#12214.

### Re: GH #52 — released, verified against the published assets — opus-5

**Part 1 is fixed end to end.** Every archive now carries the platform cdylib beside the binary, and a build whose cdylib is missing fails closed rather than shipping a binary-only asset. Verified by downloading the published v0.35.363 assets:

| asset | contents |
|---|---|
| `agent-doc-x86_64-unknown-linux-gnu.tar.gz` | `agent-doc`, `libagent_doc.so` |
| `agent-doc-x86_64-unknown-linux-musl.tar.gz` | `agent-doc`, `libagent_doc.so` |
| `agent-doc-aarch64-apple-darwin.tar.gz` | `agent-doc`, `libagent_doc.dylib` |
| `agent-doc-x86_64-pc-windows-msvc.zip` | `agent-doc.exe`, `agent_doc.dll` |

The issue's exact failing command, run against the unpacked published tarball:

```console
$ ./unpack-gnu/agent-doc lib-path; echo "rc=$?"
/tmp/relverify/unpack-gnu/libagent_doc.so
rc=0
```

**The first tagged run failed, and that was the fail-closed guard doing its job.** Five of six targets packaged their library; `x86_64-unknown-linux-musl` did not, because musl defaults to `+crt-static` and Rust emits **no cdylib at all** for a statically linked CRT — that release directory held `libagent_doc.rlib` and nothing else. I confirmed the deciding flag on a throwaway musl cdylib crate before touching the workflow: `.rlib` only by default, `.so` under `-C target-feature=-crt-static`. Relaxing the CRT for the whole target would cost the static binary that is the entire point of the musl asset, so only the library is built that way, in its own target directory. The published asset shows the split held:

```console
$ file unpack-musl/agent-doc
ELF 64-bit LSB pie executable, x86-64, static-pie linked, stripped
$ file unpack-musl/libagent_doc.so
ELF 64-bit LSB shared object, x86-64, dynamically linked, stripped
```

Nothing was published under the failed attempt, so I moved the `v0.35.363` tag rather than burning a version number.

**The remedy text was the other half of part 1.** `build with: cargo build --release` was unusable advice for exactly the audience that saw it — a tarball or `pip` user has neither a toolchain nor a source tree. It now branches on how the binary was installed, and the source-checkout path is unchanged:

```console
$ /tmp/pkgonly/agent-doc lib-path; echo "rc=$?"
[lib-path] library not found at /tmp/pkgonly/libagent_doc.so
[lib-path] this agent-doc was installed as a package, so there is no source tree to build from; re-install from a release asset that ships libagent_doc.so beside the binary (…releases/latest — assets from v0.35.363 onward include it)
[lib-path] or point agent-doc at a copy you already have: agent-doc lib-install --source <dir containing libagent_doc.so>
rc=1
```

Stdout stays empty, diagnostics stay on stderr, exit stays nonzero — the 0.35.349 protocol the JetBrains resolver depends on is untouched.

**Part 2's cause was not a skipped workflow.** The reporter guessed the publishes failed or were skipped and said so was unverified. They were *rejected*: every run since 0.35.343 returns `400 Project size too large`, and 0.35.343 itself landed 1 of 4 wheels. The fix is the cadence gate plus the verify job above.

**Publish is the one thing that did not happen, and deliberately so.** v0.35.363 is a patch tag, so PyPI did not fire — and it could not have succeeded anyway while the project sits at 9.990 GiB against a 10 GiB limit. So the wheel change (maturin `data` directory → `libagent_doc.*` installs into the same `bin/`, with a pre-upload assertion that it is in the wheel) is written and locally proven at the maturin-layout level, but has not run in CI. I verified the layout against a scratch maturin project rather than asserting it from the docs: `data = "wheel-data"` puts `wheel-data/scripts/libagent_doc.so` at `<name>.data/scripts/`, which pip installs into `bin/`. That remaining validation is folded into `#pypiquotafix` rather than filed as a new item, since it unblocks on the same event.

I commented the full status on GH #52 and left it **open**: part 1 is done, but PyPI still carries 0.35.343, and closing it while `pip install --upgrade agent-doc` is still a downgrade would be false.

**Verification:** `make check` → **9309 passed, 0 failed** (three runs: one caught a missing `editors/release-parity.tsv` row for 0.35.363, one after the musl fix). `make install-full`, then `cargo build --release` to restore `src/agent-doc/target/release/{agent-doc,libagent_doc.so}` that install-full's artifact cleanup had removed from under the repo-root-relative `.bin/agent-doc` symlink. Installed 0.35.363 resolves its library. Commits `070e6703c` + `cdfc5513c`, tag `v0.35.363`, GitHub Release live with six assets.

### Re: #donequeuestrike, #mutplanwitness, Run Agent Doc, layout supersede, auto-sync — opus-5

Five prompts, oldest-first. Three are fixed and shipped in `5577cfeeb`; two are still open with the findings written into their backlog items so the next cycle starts from evidence instead of from scratch.

### Re: #donequeuestrike — fixed, and the report's lead was a red herring — opus-5

The backlog item pointed at `'[queue] snapshot recovery warning during queue consume done-id snapshot sync: snapshot is missing'` as the lead. **That warning is benign.** The projection call site passes `None` for the snapshot deliberately — it plans against virtual transaction content — and the document strike is computed and returned in the plan regardless. Only the baseline sync is skipped. The line now says so, because it cost a diagnosis.

The real cause is one branch, `agent-doc-write-runtime-io/src/lib.rs`:

```rust
let planned_reap_outcome = if reap_done_in_same_write {
    backlog_cmd::done_and_reap_many_with_target_projection(file, &options.pending_done, /* queue strike */)?
} else {
    for id in &options.pending_done { backlog_cmd::done(file, id)?; }   // no queue strike
    DoneAndReapOutcome { removed_ids: Vec::new(), target_content: None }
};
```

`reap_done_in_same_write` is `write_result.is_ok() && commit_mode != CommitMode::None`. The observed cycle's response write was **retained**, so it took the else branch: the backlog items were transitioned to `[x]` in place and `agent:queue` was never touched. The reap that printed `completed and reaped 3 item(s) atomically` and archived them to `agent-doc-bugs.done.md` was a **later** pass over those `[x]` items, and it does not strike queue heads either. That is why `git diff acca219ab5 eb4fe58186` shows the three heads unchanged — and the committed backlog in that diff shows the smoking gun the report did not quote: `- [x] [#fixhttpsgithub]`, `- [x] [#deferredmutdrop]`, `- [x] [#pypicadence]`, marked done *in place*, not reaped.

`write --pending-only` without `--commit` reaches the same branch deterministically, so the repro needs no editor barrier at all:

```console
$ agent-doc write doc.md --pending-only --done alpha
$ grep -n 'alpha' doc.md
14:- do [#alpha]          # still a live queue head
19:- [x] [#alpha] ...     # backlog says done
```

Two changes. The queue-strike projection is now composed into the tracked-work transaction on **both** branches — the same `with_pending_write_transaction` envelope, so it is still one atomic publish. And it no longer fails silently: after projecting, a witness (`live_queue_prompt_done_ids`) enumerates live heads naming a completed id and the whole transaction fails closed if any survive. Previously nothing failed: session-check reported ok/committed while the visible queue kept serving finished work.

**I proved the test detects the bug.** Removing just the new projection call reproduces the production shape exactly:

```
&lt;!-- agent:queue --&gt;
- do [#dqsa]
- do [#dqsb]
&lt;!-- /agent:queue --&gt;
&lt;!-- agent:backlog --&gt;
- [x] [#dqsa] first tracked item
```

### Re: #mutplanwitness — fixed with the explicit marker, not a wider text guess — opus-5

The item offered two paths: extend the text witnesses to gate/edit/status intents, or record an explicit per-cycle "mutations applied" marker. I took the marker, because the text route cannot be made honest — a `--backlog-edit` witness has to guess whether canonicalization preserved the string it looks for, and guessing wrong here either drops the mutation or swallows an operator prompt.

Cycle state now carries two facts: `requested_tracked_work_mutations`, recorded in the same `#mutprovenancepreresponse` block that already records the intent (so it survives a failure at any position), and `tracked_work_mutations_applied`, set only **after** `with_pending_write_transaction` returns. A retained or failed mutation write leaves the second unset. `recorded_tracked_work_is_unlanded` asks that directly before touching any id witness, so a plan carrying only gates, ungates, edits, reorders, review-edits or a `--status` change is now visible to it.

All three deciding sites consume the same predicate — `commit`, `session-check`, and the captured-finalize resume. That third site was the one `#deferredmutdrop` fixed and the one this gap re-opened; it was missing from the structural guard that exists precisely to stop these sites drifting apart, so it is in the guard now.

Mutation-checked: deleting the two-line early return reddens `a_mutation_only_closeout_with_no_done_or_add_id_is_still_unlanded`, and nothing else caught it.

### Re: editor route layout did not converge before dispatch: superseded_by_newer_layout_state — fixed — opus-5

This one is a plain race and it fired four times today (14:07:29, 14:13:24, 14:13:31, 14:14:04):

```
editor_command_async_completed command=editor_route exit_code=1 applied=false
  output=editor route layout did not converge before dispatch: superseded_by_newer_layout_state
```

`handle_editor_route_rpc` publishes a fresh layout intent, then awaits *that generation*. It shares the pane-layout plane with the passive editor-surface observation lane, which publishes its own desired state on tab activation and geometry drift. A surface observation landing inside the route's await window bumps the generation, `pane_layout_await_outcome` sees `current.generation != requested_generation`, and the operator's click is **refused** — for a race that says nothing about the layout being wrong.

The route now republishes its intent once against the newer state and re-awaits. Bounded to exactly one retry on purpose: a supersede that repeats is real contention the operator should see, not something to spin on against a lane that publishes on every tab activation. Every existing guard re-runs against the new receipt — focus-matches-routed-document before republication, `tmux_layout_command_applied`, and routed-document-in-columns after — so the retry can only succeed on a layout that satisfies the same preconditions as a first-pass success. Only a supersede qualifies; `operator_owned_layout`, `projection_effect_not_started`, `projection_retry_pending` and `desired_layout_state_absent` are verdicts about the layout itself and still fail closed.

Two tests, both mutation-checked: the predicate rejects every non-supersede reason, and a structural guard asserts the recovery stays a single guarded retry and never becomes a loop.

### Re: #fixrunlazily — two hypotheses measured and disproved, not fixed — opus-5

I am not going to claim this one. Here is what the evidence actually says, so the next cycle does not re-walk it.

The failure is the **accept leg**, not the work leg. The message prefix is `command-plane editor_route request failed`, and 60000ms is `SOCKET_REQUEST_TIMEOUT_MS`, used by `sendAcceptedCommandSubmitToSocket`. `awaitCommandSubmitTerminal` uses `waitForReadySeconds * 1000 + grace` instead. So the controller failed to **acknowledge** a command submit for 60 seconds — and that acknowledgement is supposed to be a few hundred microseconds of enqueue work.

Two hypotheses, both killed by measurement rather than reasoning:

- **Serialized accepts.** Disproved by reading the loop: `listener.accept()` spawns a thread per client.
- **A slow pre-serve window.** The socket is bound at `ListenerOptions::create_sync()` and the accept loop starts ~50 lines later, so anything synchronous in between leaves clients connected and hanging — exactly this shape, and exactly the `ss -lxp` backlog signature. The candidate was `restore_reliable_sync_liveness`, whose durable fold reads `reliable_sync_liveness_journal`. That table holds **59,671 rows / 6.5 MB** on this project, which looked decisive. Measured: **0.08s** total for load plus decode of all 60,582 ops. Not it. (The recovery effects after that fold are already spawned asynchronously, with a comment naming this exact failure mode — someone has been here before.)

What the ops log does show at the failure window: `ffi_ensure_controller_running_failed ... Connection refused (os error 111)` seven times at 14:00:58–59, a controller adopted at 14:01:44, and a *separate* client still timing out at 14:02:23 (`timed out after 5.0s waiting for project controller response`). So the controller was unresponsive for at least 40s after adoption. The next lead is what holds a lock across `handle_request_locked`'s `runtime.bootstrap_snapshot()` during a controller's first seconds — not the journal fold. The item now carries all of this.

### Re: tmux pane auto-sync is wedged — filed with the signature, not fixed — opus-5

> **Queue prompt:**
>
> do [#donequeuestrike]
>
> do [#mutplanwitness]


Related to the layout fix above but not the same defect, and I do not have a verified cause. The signature worth keeping is this line, because it is self-contradictory:

```
pane_layout_projection generation=17 attempt=1 phase=retry_pending observation=synced effect=applied
  focus=focus_superseded_by_newer_layout_state
  expected=[tsift.md, infra.md] actual=[tsift.md, infra.md]
  expected_panes=[%10, %14] panes=[%10, %14]
  expected_focus_pane=Some("%10") active_pane=Some("%10")
```

Every observed value matches every expected value — documents, panes, focus pane — and the projection still sits in `retry_pending` because the *focus receipt* was superseded. Today's counts say the same thing: 39 `pane_layout_projection_cancelled` against 31 `pane_layout_projection`, so more layout generations are being cancelled than projected. `pane_layout_effect_worker` responds to a focus supersede with `attempt = 0; continue`, which re-reads the newest generation at the loop top and is therefore not the infinite loop it looks like — I checked that before filing, so nobody re-checks it. The open question is why a projection whose observation is byte-identical to its desired state cannot reach `Converged`. Filed as `#tmuxautosyncwedge` with the live pane state it needs.

**Verification.** `make check` → **9314 passed, 0 failed**. One run before that reported a single failure — `stop_codex_continuation_suppresses_clear_when_token_count_crosses_threshold`, `file is not a database: Error code 26` on a freshly created temp `state.db` under 174 parallel test binaries. It passes in isolation and in its own package's full suite (50/50), and did not recur; I am recording it rather than calling it clean. `make install` plus `cargo build --release` so the repo-root-relative `.bin/agent-doc` symlink target is not stale — both binaries and `libagent_doc.so` carry the fix. Committed `5577cfeeb`, pushed. CI run `34612322320` is queued, not yet started; local evidence is what this closeout rests on.

### Re: #fixrunlazily, tmux auto-sync, Clear Session Context — opus-5

Draining `#fixrunlazily`, plus the `session_clear` failure you added mid-turn. Three commits: `b0e9530a6` and `232c6c103` this cycle, on top of `5577cfeeb`.

### Re: #fixrunlazily — narrowed to a real window, closed from both ends, gated on one restart — opus-5

Last cycle I killed two hypotheses by measurement. This cycle the window itself turned out to be the answer, and I fixed it from both sides.

**The window.** `serve_project_controller` binds its socket at `ListenerOptions::create_sync()` and starts its accept loop ~50 lines later. A client that connects into that gap gets a **successful** connect, writes its request, and then waits — nothing is accepting, and no error is ever returned. At the editor that is indistinguishable from a wedged controller, and it is exactly the reported message.

There is already a comment in `restore_reliable_sync_liveness` naming this failure mode, which is why its *recovery effects* are spawned. But its **durable fold** is still synchronous and still inside the window: a SQLite read plus a JSON decode of a journal with no bound on its size — 59,671 rows / 6.5 MB on this project, against a 150 MB `state.db` that a **departing** controller may still be writing through WAL, which is precisely the handoff case here. The fold now runs before the bind. The effects still run after the listener is accepting, because they contact editor routes that call straight back in; the structural guard now asserts both halves rather than just the second.

**What is left of the window now measures itself**: `controller_listener_serve_ready ... bind_to_accept_ms=`. If a stall reappears, that line says so in one field instead of costing another forensic session.

**The client end is the half that actually changes the operator's experience.** The 60000ms is `SOCKET_REQUEST_TIMEOUT_MS`, and the leg that burned it is `sendAcceptedCommandSubmitToSocket` — the command-plane **accept** leg, which is a pure enqueue: the controller validates the payload, publishes `Accepted`, hands the work to a worker thread, and replies. I measured a live controller at **25–50ms** for exactly that round trip. It had no business sharing a ceiling with legs that legitimately block on server-side work — turn admission awaits 30s on purpose, which is why `SOCKET_REQUEST_TIMEOUT_MS` itself may not be lowered and there is a test pinning it above 30s.

So the accept leg gets its own 10s bound, and on an unacknowledged accept it takes the **same** `ensure_controller_running` recovery as a failed connect — a controller that cannot serve the request is operationally the same either way. A dead minute becomes a fast retry through a path that already existed. JetBrains plugin `0.2.380`.

**Gated, not done.** The plugin half cannot be verified from here: IDEA is still running `0.2.378` with `0.2.379` staged and never loaded, so `0.2.380` needs the same restart `#restartideaplugin` already asks for. One restart, then `Run Agent Doc` on `lazily.md`.

### Re: tmux pane auto-sync is wedged — filed, one dead end already ruled out — opus-5

Filed as `#tmuxautosyncwedge` last cycle with the signature that makes it worth keeping — a projection whose observation is byte-identical to its desired state and still sits in `retry_pending`:

```
pane_layout_projection generation=17 attempt=1 phase=retry_pending observation=synced effect=applied
  focus=focus_superseded_by_newer_layout_state
  expected=[tsift.md, infra.md]  actual=[tsift.md, infra.md]
  expected_panes=[%10, %14]      panes=[%10, %14]
  expected_focus_pane=Some("%10") active_pane=Some("%10")
```

Today: 39 `pane_layout_projection_cancelled` against 31 `pane_layout_projection` — more generations cancelled than projected. I checked the obvious suspect before filing so nobody re-checks it: `pane_layout_effect_worker` answers a focus supersede with `attempt = 0; continue`, and the loop top re-reads `pending_revision()` and `pane_layout_desired()`, so it is **not** an infinite loop on a stale generation. The open question is why a projection whose observation already matches cannot reach `Converged`, and it needs the live pane state.

### Re: Clear Session Context "whether the clear ran is unknown" — fixed — opus-5

> **Queue prompt:**
>
> 📌 do [#fixrunlazily]


The message asks you to go look at the pane. The binary had already looked.

`src/haiven-dev/tasks/sdk.md`, pane `%1`, 15:01:07 and again at 15:01:09. I captured the pane:

```
  ❯ /agent-doc …/src/haiven-dev/tasks/sdk.md
────────────────────────────────────────────
❯ Press up to edit queued messages
────────────────────────────────────────────
  Opus 5 (1M context) ctx:17% … · ← 2 agents
```

That composer line is Claude Code's **busy** state — input is being queued, not executed. Two subagents running. The `/clear` went into the queue, the command-repair resend 2s later went into the same queue, and the probe's own log already said so: `conversation_retained retained_lines=1641`. Retrying, which is what the message asks for, queues a third one.

The cause is a conflation that is correct in one direction and wrong in the other. `❯ Press up to edit queued messages` is treated as a dispatch-ready prompt, and for **dispatch** that is right — the trigger was accepted and the harness will run it when the turn ends. For a command whose contract is an **immediate state transition** it is wrong: acceptance into a queue is not execution.

`is_queued_input_placeholder_line` splits that case out. It deliberately excludes `❯ describe a task for a new session`, the other empty-composer placeholder, because that one means the pane is **idle** — the two share a function today and mean opposite things about busyness. An unobserved clear is checked against it before the cleared-state probe.

**The acceptance rule is unchanged.** A queued clear is still not accepted and still fails closed — the same contract `#clearsubmitlabel` set when it split the earlier mislabel. What changes is the label and the unblocker: `wait_for_turn_or_interrupt_then_retry`, instead of sending you to inspect a pane the binary had already read.

**Verification.** `make check` → **9316 passed, 0 failed**. JetBrains `CpSocketDeadlineTest`: 5 run, 0 skipped, 0 failures (I read the JUnit XML rather than trusting the Gradle exit code — a skipped task reads as a pass through the log filter). All three fixes are mutation-checked: removing the accept-leg wiring reddens 2 of the 5 plugin tests, and neutering the queued-composer detector reddens its own test. `make install` plus `cargo build --release` so the repo-root-relative `.bin/agent-doc` symlink target is current; both binaries carry the changes.

One process note. `src/session_actor_cmd.rs` carries another session's uncommitted `#panewindowbindingrebind` work. I committed my two hunks by hashing a hand-built blob of `HEAD` plus my edits and committing the index directly, rather than `--only` on a shared file — their 23 uncommitted lines are still in the working tree untouched. Exchange is at 270 lines / 13 response sections, noted and still draining.

### Re: #tmuxautosyncwedge, the response_captured wedge, and backend.md pane sync — opus-5

> **Queue prompt:**
>
> do [#tmuxautosyncwedge]


Both are fixed and shipped in **0.35.364** (312d8341d, tagged and pushed). `make check` → **9321 passed, 0 failed**.

**#tmuxautosyncwedge — the open question had a one-line answer, and it was not the supersede.** Convergence in `pane_layout_effect_worker` read

```rust
let synced = report.synced && (!focus_required || focus_receipt.applied);
```

`observation_invocation.focus` carries the focus target *exactly when* `focus_required`, and `layout_sync_state_result` then refuses `synced` unless `expected_focus_pane == active_pane`. So `report.synced` had already proven the focused pane was active — and the `focus_receipt.applied` term re-gated that proof on *which generation* called `select-pane`. Under editor-navigation churn every pass is superseded before its focus effect, comes back `focus_superseded_by_newer_layout_state` with `applied: false`, and parks a fully-converged layout in `retry_pending`. That is why documents, panes and focus pane all matched in your gen-17 capture.

The evidence is unambiguous: of the 9 `phase=retry_pending ... focus=focus_superseded_by_newer_layout_state` lines in today's superproject ops.log, **all 9 carry `observation=synced`**. Every single instance of this shape was a converged layout being denied.

`pane_layout_projection_converged` now decides from the observation. The reusable-structure branch also stopped short-circuiting on the supersede before it observed — it now computes the fresh report first and falls through to the shared pre-structural cancel only if the layout genuinely has not converged, which is where the 39-vs-31 cancel surplus was coming from. `focus_applied` survives as the fallback for the one case the observation cannot speak to: a required focus the observation invocation did not carry. The supersede guard still refuses to *move* focus for a stale generation — that part was never wrong.

**The `response_captured` wedge is an ownership lie, not a delivery failure.** `RetainedWriteVerdict::Deferred` promises "the same intent commits itself once delivery converges". That promise is kept by a captured-finalize resume worker, and its only two drivers are the supervisor idle watch and the Codex `Stop` hook. Neither was running for `src/haiven-dev/tasks/backend.md` — the controller was concurrently logging `controller_orphan_drain_dispatch ... reason=no_supervisor_idle_watch` and `adstatechart_snapshot ... supervisor.sup_stale`, with `delivery_converged=true` and `live_editors=1`. The edge it was waiting for had already happened and would never happen again. Meanwhile `doctor` said `agent-doc write --commit`, `session-check` said "retry only session-check", and `commit` refused with the deferral — three answers, no next move. That is exactly the failure `write_ownership.rs` was written to eliminate; the predicate simply never asked whether anything was *driving* the capture, only whether one existed.

So `RetainedWriteOwnership` now carries `capture_resume_unowned`, proven in the shared IO shell from the same supervisor-liveness predicate the controller's orphan drain already uses (and treating this process as an owner when it *is* the supervisor, since `supervisor_pid_for_doc` skips its own pid). A durable capture with no driver yields a new `CaptureResumeUnowned` verdict instead of `Deferred` — between the two existing readings, because the response is not lost and must not be re-sent, but waiting is equally useless. Its remedy names `agent-doc repair --resume-capture <FILE>`, which I added in the same change: it resumes that exact retained capture once, never captures a new response, and never elects a disk-authority fallback. The "do NOT re-send / force disk / `admin recycle`" guidance is preserved verbatim in the new wording, because those recoveries are still wrong here.

One honesty note on verification: `backend.md` recovered on its own before I could install the fixed binary against it — it now reports `cycle-1789142928908 committed`. So the new command was exercised only on its no-retained-capture path live; the wedge itself is covered by regression tests over the predicate and the remedy, not by a live re-wedge.

**backend.md pane auto-sync** is the same `#tmuxautosyncwedge` defect. Worth noting where it was: haiven-dev's own controller was converging (`effect=focus_only focus=focus_applied:...backend.md:%12`) — the failures are in the **superproject** controller, which is the one arranging the cross-root columns you actually see. The recycle is marked, so the next idle boundary picks up the fixed build.

Follow-up captured: `#supervisoridlewatchmissing`. The ownership fix makes the wedge escapable, but a live session whose controller reports `no_supervisor_idle_watch` is the upstream cause and is still unexplained.

Exchange is at 335 lines / 17 response sections, noted and still draining. CI and Release runs for the tag are queued (34621201563, 34621204777); reported from local evidence rather than waited on.

### Re: #captureresumereplaynotidempotent — opus-5

> **Queue prompt:**
>
> do [#captureresumereplaynotidempotent]


Fixed and shipped in **0.35.365** (`eb3eef781`, tagged and pushed, installed, recycle marked). `make check` → **9324 passed, 0 failed**.

**The guard could not tell its own prior success from a collision.** A captured closeout records its tracked-work mutations so a resume can replay them, and the replay reaches `ensure_new_item_explicit_id_available` carrying the item its own earlier run already inserted. `#preset-item-id-collision-enforce` fails closed on that, so the resume reported `refusing to commit a half-applied cycle` — for a document that was fully applied. A bare `agent-doc commit` closing the same cycle is the proof it was never half-applied.

`explicit_add_already_satisfied` now makes an explicit-id add a logged no-op (`backlog_add_already_satisfied`) when that id is **already active with the same text**, on all three add paths (`add`, `add_many_to_list`, `add_at_to_list`). The scope is deliberately narrow, because the guard is worth keeping:

| shape | verdict |
|---|---|
| same id, same text, active | satisfied — no-op |
| same id, same text modulo line wrapping | satisfied — no-op |
| same id, **different** text | still the `#preset-item-id-collision-enforce` refusal |
| id already reaped (`[x]` / archived) | not satisfied — a real request for new work |
| no explicit id | untouched; the derived-id suffixing path owns that |

The `--done` half was already idempotent (`done_noops_when_item_was_already_archived`), which matches the live failure being an add and nothing else.

The end-to-end regression replays a capture whose `--backlog-add` already landed and asserts `Committed` plus exactly one copy of the item. It is **mutation-checked**: deleting the filter from `add_many_to_list` reddens it, so it is testing the fix rather than the fixture.

This closes the gap 0.35.364 opened one release earlier — the `CaptureResumeUnowned` remedy named `repair --resume-capture`, and that command then broke on exactly the documents it exists for: one whose add applied before delivery stalled.

**CI: the 0.35.364 run (34621201563) went red** on `agent-doc-controller-io project_controller::tests::retained_resume_reacts_when_the_intent_arrives_after_delivery` — `left: None, right: Some("retained_settled_delivery_reactive")`. I do not believe it belongs to that change, and I checked rather than assumed: neither edit is reachable from that test. `pane_layout_projection_converged` lives in the pane-layout worker, which this test never drives, and the `capture_resume_unowned` bit is in `agent-doc-capture-io`, which `agent-doc-controller-io`'s lib test binary does not link. It did not reproduce in ~900 local executions, including pinned to one core at `--test-threads=8`; that CI run took 805s against 166s locally, so the runner was heavily contended. The test's synchronization looks like the real fragility — `subscribe_state_plane(..., None, 0, 2s)` returning is used as a proxy for "this specific wake landed", and `record_captured_finalize_wake` inserts into the map *before* it publishes, so an absent key means the subscribe was satisfied by something that was not this frame. Filed as `#retainedresumewakeflake` with that analysis; the 0.35.365 run (34623772732) was still in progress at closeout and will confirm or refute it.

Exchange is at 400 lines / 19 response sections, noted and still draining.

### Re: #supervisoridlewatchmissing — opus-5

> **Queue prompt:**
>
> do [#supervisoridlewatchmissing]


Root-caused, fixed, and **verified live**. Shipped in **0.35.366** (`afedc44bb`, tagged and pushed, installed, recycle marked). `make check` → **9326 passed, 0 failed**.

**The supervisors were there the whole time. The parser could not see them.**

```
$ pgrep -fa "agent-doc start"          # twelve live route-owned supervisors
13952    agent-doc start --route-owned --route-owned-reap-policy keep-alive \
                        --route-owned-start-purpose layout-provision tasks/agent-doc/agent-doc-bugs.md
1827015  ... --route-owned-start-purpose layout-provision --resume -- tasks/backend.md
```

`start_route_owned_doc_from_args` walked positionally from `start`: skip `-`-prefixed tokens, skip exactly one hand-listed `--flag VALUE` pair (`--route-owned-reap-policy`), return the first bare token. So `--route-owned-start-purpose` — a value-taking flag added after that list was written — fell through the `starts_with('-')` arm, and the very next token was returned as the document. **Every one of those twelve supervisors resolved to `layout-provision`.** `supervisor_doc_canonical` then failed to canonicalize a file that does not exist, and `supervisor_pid_for_doc` answered `None` for documents whose supervisor was running the entire time.

That is the whole chain to the wedge: no supervisor found ⇒ no idle watch ⇒ the controller falls back to `controller_orphan_drain_dispatch ... reason=no_supervisor_idle_watch`, and captured-finalize resume — whose only drivers are that idle watch and the Codex `Stop` hook — genuinely has no driver. It also means 0.35.364's `capture_resume_unowned` bit was reading `true` on documents that *were* owned. The verdict logic was right; its input was being lied to by this parser.

The fix keys on the `.md` token after `start`, which is flag-order- and flag-set-independent. That rule was not invented here — `agent_doc_controller::command_line::start_supervisor_document_from_args` already used it and already read these exact command lines correctly. Two crates answering the same question, one right and one wrong, for however long `--route-owned-start-purpose` has existed.

They cannot collapse into one function: `agent-doc-controller` depends on `agent-doc-supervisor`, so the shared predicate would have to move down along with `agent_doc_start_arg_index` and its shell-sentinel helpers — more than this cycle should take. The guarantee instead is a **cross-crate agreement test** over every command-line shape agent-doc itself launches, including both orderings (`start_reexec_command` puts the document *first*; the controller-side launcher puts it *last*) and an unknown future flag. Both new tests are mutation-checked: restoring the old parser reddens them with `parsers disagree on ...`.

**Live proof after install** — `admin kill-supervisor --dry-run` resolves the pid without signalling:

| document | pid | matches `pgrep` |
|---|---|---|
| `src/haiven-dev/tasks/backend.md` | 1827015 | ✅ |
| `src/haiven-dev/tasks/infra.md` | 1928885 | ✅ |
| `tasks/software/lazily.md` | 2477504 | ✅ |
| `tasks/software/tsift.md` | 2475272 | ✅ |
| `tasks/agent-doc/agent-doc-bugs.md` | 13952 | ✅ (`refused_self_ancestor` — this session's own supervisor, which it could only refuse by first resolving it) |

Before the fix every one of those was `None`.

CI: run 34623772732 (0.35.365) has been in progress for ~40 minutes without completing, so `#retainedresumewakeflake` is still unconfirmed; 34626410869 (0.35.366) is queued behind it. Reported, not waited on.

Exchange is at 430 lines / 20 response sections, noted. Only `#cargotokenrevoke` remains in the queue and it is operator-gated on the crates.io login click, so the drain ends here.
Fix docs.md turn not closing properly...the response is uncommitted.
```
● No cycle ran this turn. Preflight refused admission because the previous cycle's document write is still reconciling,
  and session-check confirms the same controller-owned divergence as before.

  State

  - The response to "Address all reviews in PR 20" is committed in git (commit b1063aa on tasks/docs.md) and the PR
    work itself is done and pushed (haiven-docs 283c17b).
  - The editor buffer for docs.md still holds a different exchange and queue than disk, so the controller has requested
    a native editor save and owns the next closeout. Disk now also contains the response, but session-check still
    reports divergence on the exchange and queue components.
  - Per the standing rule for this state I did not patch, restore, or resubmit anything.

  What clears it

  Save the docs.md buffer in the editor, then re-invoke /agent-doc tasks/docs.md. If session-check still reports
  divergence after the save, that is a controller defect to report rather than something to work around by editing the
  file.

  The two open items from the last cycle still stand: your unsubmitted pending review on PR 20 blocks inline replies
  from your account, and Sanat's questions on the issuance shape, its owner, and the handoff date are yours to answer.
```

### Re: docs.md turn not closing, plus three more operator-text-loss reports — opus-5

**`#fixdocsturn` is fixed, tested, installed and pushed** (0.35.366, commit `0f959e11f`). The other three reports you added mid-turn are diagnosed with reproductions but not fixed — details and my reasoning below.

#### Why the docs.md turn could never close

`session-check` on `src/haiven-dev/tasks/docs.md` reported **6 concurrent operator steering directives**. Four of them were not yours. Steering 1/6–4/6 were verbatim paragraphs of the committed `### Re: #plcite` response cell — a retained-capture replay had spliced that whole cell back into the live buffer a second time (plus three reversed copies of the PR-20 bullet list).

The write pipeline was doing the right thing: it resolved those bytes away as a duplicate, so they never reached a commit. But the steering detector reads the raw buffer, and classified the replayed paragraphs as new `PromptTarget`s. That is a trap with no exit — answering a phantom prompt only appends another response, the replayed bytes stay, and the next `session-check` INTERRUPTs again. Hence "the response is uncommitted": the cycle committed, and the document still looked unanswered forever.

The fix classifies by **provenance, not shape**. A block is a replay only if the whole thing matches a *contiguous run* of the baseline exchange — so every line is already committed and none of it is new operator input — **and** that run overlaps a response body, which is where an operator prompt is never committed. A re-surfaced operator prompt block has no response-body overlap and still counts as steering.

Proof, against the real wedged pair (`b1063aa` plus the observed duplicate cell):

| | directives |
|---|---|
| before | **4** `prompt_target` — byte-identical to live steering 1/6…4/6 |
| after | **0** |

Two unit tests plus two steering-level tests, mutation-checked (removing the filter reddens `replayed_committed_response_cell_is_not_realtime_steering`). `make check` green locally; 446 passed on the two touched crates.

#### The three reports you added mid-turn

I did **not** fix these, and I want to be straight about why rather than quietly hand you four half-fixes.

**Your queue items and exchange typing being destroyed is the most serious thing here**, and it is now reproducing across two documents. I found the exact event on this document — `cross_component_raw_union` at 17:22:09, three seconds before this turn admitted, with `recovery=canonical_restored_and_member_reprojected` — and on `infra.md` I found something sharper: your queue line survives as `- do [#fpeidentitysecondclient]`**`U`**. That stray `U` does not exist anywhere in the text that was removed, so it arrived from a *different component*. That is character-granularity cross-component damage, not a lost line. I walked the path to `crdt_relay.rs:1168` and found two things worth your attention: the three-way merge base is a **synthetic** `CrdtDoc::from_text`, so all op identity is discarded before the merge runs, and the canonical-with-delta-applied text is computed and then thrown away. Fixing that properly needs a SimWorld case with a one-generation-stale editor projection, and the recovery currently drops your bytes **without logging what it dropped** — so step one is instrumentation, not a patch. It is at the top of the backlog as `#queuelineclobber` with everything I found, so the next cycle starts from evidence rather than from scratch.

**The `prompt_presets` blanking I did reproduce deterministically.** When you type the key and have not typed the value yet, the parser reads the YAML null as an empty string and the next write emits an explicit empty value — something you never wrote, which then races the value you are still typing. I stopped short of landing the clean fix because it needs `prompt_presets` to round-trip null the way the `env` field already does, and that is ~200 references across 23 files — a refactor I was not willing to start at the end of a long turn in a tree another session is writing to. There is also a trap worth recording: "just drop the null key at parse" would make `write()` **delete your half-typed line**, which is the same harm wearing a different hat. Filed as `#presetnullblank` with the reproduction and both findings, including the separate discovery that agent-doc re-serialized your frontmatter (stripping your quotes) without changing a single value.

**The stash-window focus is confirmed live.** `session doctor` on `infra.md` says pane `%14` is recorded against window `@0` but actually lives in `@3` — and `@0` is the stash. I ruled out `select_pane` (tmux resolves the pane's real window) and found that the focus-*mirror* direction is already guarded live-vs-record with a sensitivity proof, so the remaining suspect is a path issuing `select-window` against the **recorded** window. The durable fix is to resolve pane→window at use time and stop persisting an id tmux already owns; `doctor --repair` is a band-aid and should not be the answer. Filed as `#stashfocuswindow`.

#### One more thing you should know

CI run **34626410869 is red** on `compact_exchange_returns_retained_success_when_editor_ack_stays_delayed`. It predates my commit and is not reachable from it, and local `make check` on the same tree is green — but it is the same delayed-editor-ack shape as `#retainedresumewakeflake`, whose diagnosis probably applies directly. Filed as `#compactackflake`.

I also could not `--done` the `#fixdocsturn` backlog item: it had already been destroyed by `#queuelineclobber` before I could close it.

### Re: your exchange edits being reverted — the isolation merge is ruled out — opus-5

> **Queue prompt:** As I was typing in this file's agent:exchange when the turn started, my changes in agent:exchange were reverted by agent-doc.

I went after this one properly rather than re-filing it, and the useful result is a **disproof**: the component-isolation merge is not where your text is lost. That matters more than another hypothesis, because it stops the next cycle re-walking the path I just walked.

**What I tested.** I probed `merge_by_component` with the exact base/canonical/editor triple the reconcile builds, across seven shapes:

| shape | operator text survived |
|---|---|
| operator appends queue line vs agent replaces a different one | ✅ |
| operator edits their own line in place vs agent replaces it (the `infra.md` shape) | ✅ |
| operator types in exchange vs agent appends a response | ✅ |
| operator appends vs agent deletes the queue head | ✅ |
| operator edits in place vs agent rewrites the whole queue body | ✅ |
| line already in canonical vs stale editor relaying elsewhere | ✅ |
| line already in canonical vs stale editor editing the queue | ✅ |

All seven converge correctly. My first regression test passed **with the fix removed** — it proved nothing, so I threw it out rather than keep a green test that binds to nothing.

**Where I think it actually is.** `rebuild_component_isolated_epoch` does not merely replace the canonical: it resets every member's `replica` and `observed_replica` from a fresh bootstrap, calls `member.pending.clear()`, and queues every live member for re-bootstrap. Anything a replica holds that has not reached the hub is discarded by construction — and the editor is then told to replace its buffer. That is a much better fit for "I was typing and my changes were reverted" than a merge losing lines, and it is consistent with the standing `stale_plugin` warning (live JetBrains plugin 0.2.379 against the 0.2.380 this binary ships). The next step is the rebootstrap and canonical-projection delivery, not this merge.

**What I landed** (commit `73795693b`, 0.35.366 installed): a safety post-condition on the reconcile. A non-blank line you added since your own pre-edit buffer has been seen by no other replica, so nothing can legitimately have deleted it — if the reconciled text does not carry it, the raw union is published instead. A character landing on the wrong side of a component marker is visible and repairable; a silent deletion is neither. Refusals now land in ops.log as `crdt_component_isolation_reconcile_refused`, so this class stops being invisible — which is the whole reason today's three sightings had no forensic trail.

I want to be exact about its status: **this is a fail-safe, not a proven fix.** It has no reproduction, and on every shape I could construct it never fires. I kept it because it is the instrumentation this investigation needed and it cannot change behaviour on any path I was able to exercise. `make check`: 9332 passed, 0 failed. The post-condition helper is mutation-checked; the hub-level test is labelled a characterization test so nobody mistakes it for proof.

Your two new tmux reports (auto-focus on selecting the opposite pane in IDEA) are folded into `#stashfocuswindow`, which already has the confirmed stale `%14 → @0` binding — same root, and worth fixing as one piece of work rather than three.

### Re: JB "Focus Agent Terminal" navigating to nothing — fixed — opus-5

> **Queue prompt:** The JB "Focus Agent Terminal" does not navigate to anything.

Fixed, tested, built and installed — plugin **0.2.381**, commit `50bc06de5`.

**It was a literal no-op.** The notification action called `IdeTerminalHost.focusExisting(project)`, which returned `Unit` and did this:

```kotlin
val (manager, content, _) = liveTab(project) ?: return
```

`liveTab` bails on **five** separate conditions — no terminal tool window, no tab named `agent-doc`, a widget that is not a `ShellTerminalWidget`, a null tty connector, or a disconnected one. Any of them and the click did nothing and told you nothing. There was no logging, no notification, no return value a caller could react to.

**What changed.** Focus now resolves to a `TerminalFocusOutcome` the caller acts on:

| state | before | now |
|---|---|---|
| `agent-doc` tab present | focus it | focus it |
| tool window, no `agent-doc` tab | **nothing** | activate the tool window — you land in the terminal and can see the tab is missing |
| no terminal tool window | **nothing** | a notification saying so, pointing you at the tmux pane |

I also took the two liveness conditions **out of the focus path entirely**. A tab whose shell has died is still the thing you asked to look at — requiring a connected tty is a precondition for `attachExisting`, which executes a command in the tab, not for showing it. That alone removes two of the five silent-miss paths.

**On the testing**, two things worth noting because both are traps in this repo. Gradle reported `BUILD SUCCESSFUL` with `compileKotlin UP-TO-DATE`, which is exactly the shape where a skipped test task reads as a passing one — so I confirmed from the JUnit XML that all four cases actually executed (`tests="4" skipped="0" failures="0"`). And the decision helper is mutation-checked: collapsing `TOOL_WINDOW_ONLY` back to `NOTHING` reddens two cases, so the tests bind to the fix rather than to the enum existing.

**One thing I did not fix**, because it is a different bug wearing the same clothes: the notification is per-document (`Agent input required for $fileName`) but `focusExisting(project)` ignores the file entirely and focuses a single project-wide tab. In your multi-document tmux layout the correct target is the pane that owns *that* document. The plugin already has the machinery (`TmuxPaneFocusSync`), but routing it there is the same pane→window resolution work as `#stashfocuswindow`, and doing it twice in two places is how these three tmux reports became three reports. Filed as `#jbfocusdocroute`, adjacent to it.

Exchange is at 529 lines / 22 sections; noted, still draining.

### Re: queue drained — stopping the loop — opus-5

Nothing agent-drainable is left. Preflight reports `queue_drainable_head_count = 0` and `queue_continuation_required = false`; the two remaining heads are both non-drainable in-session:

- `⏭️ do [#retainedresumewakeflake]` — skip-marked.
- `do [#cargotokenrevoke]` — `[operator-verify]`, blocked on one click: log in with GitHub at `https://crates.io/settings/tokens` in bridged Chromium tab 13, then re-queue it and the revoke happens in-session.

This cycle is a closeout only — the diff was bookkeeping (the `queue: go` → `queue: start` flip and the `#ftstrike` auto-strike of the JB head), not a new request. I am writing it because preflight had already opened the cycle, not to add a section: the document is at 559 lines across 23 sections with three no-op closeouts in the last half hour, and a fourth would be exactly the `#qchurn` the skill warns about.

**Landed across this drain:** `#fixdocsturn` (`0f959e11f`), the reconcile safety post-condition plus its ops.log signal (`73795693b`), and the JB Focus Agent Terminal no-op (`50bc06de5`, plugin 0.2.381, staged into IntelliJ — it needs an IDE restart to activate, which also clears the standing `stale_plugin` warning).

**Top of the backlog, with evidence rather than hypotheses:** `#queuelineclobber` (P0, your text being destroyed — the isolation merge is ruled out across seven probed shapes, next lead is the rebootstrap path), `#presetnullblank` (P1, reproduced deterministically), `#stashfocuswindow` + `#jbfocusdocroute` (P1/P2, one pane→window binding bug behind all the tmux focus reports), `#compactackflake` (P2, pre-existing CI red).

### Re: #presetnullblank — a preset key typed without a value never gains one — opus-5

Fixed, tested, installed and pushed — commit `1920285e6`.

**The mechanism, reproduced deterministically.** When you type `'#rebase-conflicts':` and haven't typed the value yet, that is a YAML null. `serde_yaml::from_value::<IndexMap<String, String>>` read it as `""`, so the very next `write()` emitted `'#rebase-conflicts': ''` — a value you never typed, written into your document mid-keystroke, where it competed with the real one. That is why the value you were typing came back empty.

Last cycle I said the clean fix was ~200 references across 23 files and declined to start it. **That estimate was wrong** — it counted references, not edit sites. Classifying them showed **one** assignment, **two** constructions, and everything else a `&IndexMap<String, String>` parameter. So a newtype that `Deref`s to the resolved map satisfies every consumer for free:

```
prompt_presets: PromptPresets   // declared keys in order, Option values
    └─ Deref → IndexMap<String, String>   // only the ones with a value
```

The workspace compiles with **zero call-site changes**.

**What it does now:**

| you typed | before | now |
|---|---|---|
| `'#k':` (mid-typing) | became `'#k': ''` | key preserved, no value invented |
| `'#k': ''` (deliberate) | `''` | `''`, still distinct from unset |
| `'#k': value` | value | value |

A key still being typed no longer resolves as a preset with an empty body, and it is no longer serialized away — `is_empty` keys off *declared* entries, not resolved ones, or `skip_serializing_if` would have deleted your line. Repeated write cycles are stable.

I also want to name the trap I avoided, because it is the obvious fix and it is wrong: dropping the null key at parse would make `write()` **delete your half-typed line** on the next cycle. Same harm, different hat.

**What still isn't right**, and I'd rather say it than let you find it: `write()` re-serializes frontmatter it did not change. It strips your quoting, and it emits `'#k': null` rather than a bare `'#k':`. Same YAML document, different bytes — so agent-doc is still touching a line you're editing, just no longer with a value that can shadow yours. The durable fix is a byte-preserving frontmatter write, and `frontmatter.rs:1435-1529` already documents that pattern ("Every other byte is preserved verbatim") — some write path is simply bypassing it. That is the remaining half of `#presetnullblank`, kept open.

`make check`: 9336 passed, 0 failed. Mutation-checked — restoring the null-to-empty coercion reddens two of the four new tests.

### Re: #stashfocuswindow — opus-5

**The item had the two windows backwards, and that inversion is what made the prescribed fix wrong.** Live tmux right now:

```
0:0 @0 agent-doc  active=1
0:1 @3 stash      active=0
```

`@0` is the **visible** window; `@3` is the stash. So `session doctor src/haiven-dev/tasks/infra.md` — which still reports the drift verbatim — is saying the record names the *visible* window while pane `%14` is actually *stashed*, not the other way round. The item's durable fix ("resolve pane→window at use time and stop persisting an id tmux already owns") would therefore have made symptom 1 **worse**: resolving live gives you `@3`, which is exactly the window you must not activate.

**The remaining suspect in the item is also dead.** There is no production caller of `Tmux::select_window(window_id)` anywhere in agent-doc or tmux-router — only a test at `src/tmux-router/src/tmux.rs:2179`. Nothing issues `select-window` against a recorded window.

**What actually does it.** `Tmux::select_pane` batches `select-window -t <pane>` with `select-pane` (`src/tmux-router/src/tmux.rs:647`), and tmux resolves that `-t` target to whatever window the pane lives in *now*. Selecting a pane that is still parked in the stash therefore drags the client into the stash window. Two selection sites did that unguarded:

- `select_reconcile_pane` — the reconcile `SELECT` step, on both call sites (`sync.rs:1408`, `sync.rs:1515`), whose target is `focus_pane.unwrap_or(first_pane)`;
- the phase-6 focus reselect (`sync.rs:1211`), which *computes* `focus_pane_already_placed` and then throws it away — `should_reselect_focus(passive, _focus_pane_already_placed) = !passive`, neutered in `be338c5` so that an explicit sync "always reselects the requested pane, whether or not it is already placed."

Neither a recorded window id (which names `@0`) nor a live one (which names `@3`) can decide this. Stash **membership** is the predicate. `restore_operator_focus` in the same file already applies exactly that rule — *"Skips the restore when the captured pane died or is now stashed out of view (so we never surface the stash window chasing a stale selection)"* — it was simply never applied at the other two sites.

**Landed** (`3c41b85`, tmux-router): a pure `selection_may_activate_window(passive, pane_is_stashed)` decision, consumed at both sites. A stashed pane degrades to `select_pane_preserving_window`, so the selection still lands and the stash is never activated; surfacing stays where it belongs, in the structural reconcile's swap/attach.

**I proved the test detects the bug rather than trusting it.** The regression runs against a live tmux server (`IsolatedTmux`), parks the focus pane in a real `stash` window, and asserts the active window is unchanged; then it runs the pre-fix effect on the same pane and asserts that one *does* surface the stash. Replacing the guard with `if true` reddens it:

```
assertion `left == right` failed: an explicit reconcile selection must not activate the stash window
  left: Some("@2")   right: Some("@0")
```

147 tmux-router tests green, clippy and fmt clean. agent-doc `make check` green: **9336 passed, 0 failed**. `make install` rebuilt tmux-router 0.3.21 into the binary, so the fix is live (`agent-doc 0.35.366`).

**I also fixed tmux-router's CI, which has been red since 2026-08-27** (`cf96c63`). It was not a test failure: clippy 1.98 rejects `normalize_harness`'s `"claude-code" => "claude-code"` identity match under `-D warnings`. My local clippy does not flag it, which is why it survived; the arms it was written for are gone, so it is a passthrough now.

**What I did not close.** Reports 2 and 3 (auto-sync/auto-focus not following when you select the opposite pane in IDEA) are the same event seen from the operator's side — tmux went to the stash instead of following — and the same guard covers them, but confirming that takes your IDE, not a test. Do the navigation once and tell me if tmux still misbehaves. I deliberately did not run it live: `agent-doc focus` on `infra.md` would have resumed that document's session, and I was not going to inject a turn into a live pane to prove a point.

I also did **not** run `session doctor --repair` on the live drift. Rebinding the record to `@3` would tell the layout join that infra.md's window *is* the stash — see the new `#layoutbindstale` below, which is the one place a recorded window id still steers behaviour.

### Re: #queuelineclobber — opus-5

**It is not the CRDT. It is preflight, and preflight says so on stdout.**

```
[preflight] queue: cleared drained queue body
[preflight] queue: set queue: stop
```

Reproduced deterministically from a seeded document in five commands — seed a queue holding one operator line, gate a backlog item, flip `queue: go`, run preflight, and the line is gone. No editor, no live replica, no merge.

**The mechanism.** `run_queue_maintenance` decides a queue is "drained" from two predicates, and neither can see operator text. `need_strip_auto` asks only whether an actionable `do` prompt remains. `queue_entries_are_drained_residue` classifies by entry kind — `Completed | Preset | Dispatch`. An operator's `re [#id]: <question>` line parses as **`Freeform`**, which is deliberately *not* an actionable prompt, so it satisfies "no prompts" while being nothing like residue. The code then blanks the **whole** body:

```rust
current_content = q.replace_content(&current_content, "");
```

That is your `infra.md` sighting exactly. `c0fae3b` → `c42c94c`: the queue holding `- re [#fpeidentitysecondclient]: Since we moved over to a Secret Bearer Token…` is replaced, and both your question and the `queue: go` you had just set are gone.

**The item's "next lead" is a dead end, and I can rule it out with evidence rather than more probing.** The clobbering write is `crdt_cp_write … source=pending_write content_len=36073 content_hash=ca7f498d…` at 17:46:44Z — and 36073 is exactly the byte length of the `c42c94c` blob, so that transition *is* the damage. `rebuild_component_isolated_epoch` never ran for it: the only `crdt_replica_update_corruption_rejected reason=cross_component_raw_union` on `infra.md` in the entire ops.log is at 17:26:24Z, twenty minutes earlier and five minutes *before* `c0fae3b`, which still has your line intact. The isolation merge was ruled out by seven probed shapes; the rebootstrap is ruled out by the log.

**Landed** (`5983df246`): the drain clear is gated on `queue_body_clear_is_lossless`. I gated the whole decision rather than only the body write — if the clear would be lossy the queue is not drained, so the auto-strip and the `queue: stop` revocation it drives are wrong for the same reason. Structural separators (blank, `---`, `~~~`) carry no content and still clear, so a genuinely drained queue still deactivates. Your line now surfaces through the existing `inactive_queue_residue` warning, which is what an unanswered queue line should do.

**Mutation-checked end to end, not just in the unit test.** Replacing the gate with `&& true` reddens the new preflight regression *and*, against a rebuilt binary running the live repro, reproduces the deletion:

```
MUTATED BINARY result:   queue_body= []
FIXED   BINARY result:   queue_body= ['- re [#bbb]: Since we moved over to a Secret Bearer Token…']
```

**One half of sighting (a) is still open.** "Free-text queue item and its promoted backlog item vanished" is this bug. "Operator exchange typing reverted" is not — that one does carry a real `cross_component_raw_union` event, and it is a different loss site in a different component. Filed as `#exchangetypingrevert` rather than folded in here, because folding them is what sent the last cycle down the rebootstrap path.

### Re: dispatch refused into pane %15 mid-recycle — opus-5

> **Queue prompt:**
>
> do [#queuelineclobber]


**The unblocker it handed you cannot happen.** `InFlight` is published by the supervisor just before it `execve`s onto the fresh binary and cleared by the replacement's watch loop, and nothing bounded that window. A supervisor that dies between those two points pins the document `InFlight` permanently — so the route waits its full budget and refuses, forever, pointing at the event that was lost.

Confirmed in `ops.log`, and it is not hypothetical:

```
18:50:52Z  supervisor_recycle_graph_started  …8ff4e889…  reason=auto_install_reexec  phase=InFlight
18:50:55Z  supervisor_recycle_graph_settled  …e972f5f6…   (three others settle within 7s)
19:09:39Z  route_dispatch_only_recycle_inflight_wait      file=…/dev.md pane=%15
19:09:49Z  route_dispatch_only_recycle_inflight_unsettled waited_ms=10049
```

dev.md's document hash started and never settled. Nineteen minutes later the route was still waiting 10s for it — my `make install` earlier in this session started that recycle, which is how you caught it.

**Landed** (`70b588e18`): the sibling `Requested` phase has carried a TTL all along (`recycle_request_ttl`, 900s); `InFlight`, the phase that actually gates dispatch, now has the same discipline. Past the settle TTL the transition is treated as lost rather than pending and the gate proceeds, logging `route_dispatch_only_recycle_inflight_abandoned` with the stamp, TTL and epoch. It is checked *before* the wait, so a wedged document no longer pays a 10s stall on every dispatch either.

It still fails closed where it should: inside the TTL, and for an unstamped projection (`marked_secs == 0`), which is unknown rather than stale. 120s default against a real reexec observed at 3–7s, overridable via `AGENT_DOC_RECYCLE_INFLIGHT_TTL_SECS`. Mutation-checked on the observed 19-minute shape.

`make check`: **9339 passed, 0 failed** (three new tests). Installed, so both fixes are live — dev.md's pane should accept a reopen now without waiting on a settle that was never coming.

Exchange is at 640 lines / 26 sections; noted, still draining.

### Re: #presetnullblank — the write path no longer edits lines you are typing on — opus-5

Fixed and pushed (`e13404d9c`). The remaining half was exactly where the backlog note said to look: every `parse → mutate one field → write` wrapper called `write(&fm, body)`, which re-serialises the **whole** `Frontmatter` struct. So a cycle that only advanced `agent_doc_pipeline` still rewrote every other line — stripping your quoting, and emitting `'#k': null` where you had typed a bare `'#k':`.

`write_preserving(original, fm, body)` now owns that path. It diffs the parsed old mapping against the newly serialised one, key by key:

- value unchanged → **the original block's bytes go back verbatim** (your quoting, a bare `key:` with no value, comments, indentation);
- value changed → only that key's block is re-rendered, in place;
- key gone from the new mapping → dropped, so `queue_active` and friends still retire;
- key new → rendered and appended before the closing fence.

It is deliberately conservative: no frontmatter region, unparseable YAML, a duplicate top-level key, or a block whose own bytes do not round-trip to the parsed value all fall back to `write`. One subtlety cost a debugging pass and is now encoded: a `|` block scalar clips differently depending on whether a newline follows it, so each block is parsed exactly as the region presents it — with the separator newline for every block but the last.

Every in-module wrapper (`set_session_id`, `set_resume_id`, `set_resume_id_for_harness`, `set_pipeline_state`, `clear_pipeline_state`, `set_format_and_write`, `merge_fields`, `merge_queue_state`, `merge_queue_control`, `set_tmux_session`, `ensure_session_with_ssh_resolver`) plus `reset.rs` and the start-runtime resume-clear now go through it. `write` stays for genuinely new documents.

Five regression tests, the sharpest being the round-trip one: `set_pipeline_state` then `clear_pipeline_state` on a document carrying a comment, a single-quoted value, a half-typed preset and a block scalar returns the file **byte-identical**, and five such cycles in a row still do. And I kept the trap you recorded — dropping null keys at parse is still not the fix, because `write()` would then delete the half-typed line.

Not yet proven live: the deterministic tests are the evidence I have. The live confirmation point is this document's own frontmatter — it currently holds three single-quoted `prompt_presets` values, and the next preflight runs on the newly installed binary, so those quotes surviving is the real check.

### Re: Run Agent Doc on docs.md — "controller not authoritative (handoff_state=Preparing)" — opus-5

Fixed in the same commit (`#handoffrefusalretry`), and the cause is mine: I ran `make install` mid-session, which recycled every live controller. `src/haiven-dev/.agent-doc/logs/ops.log` shows haiven-dev churning through controller generations **1176 → 1177 → 1178 in 24 seconds** starting 20:24:16Z, each replacement reporting `handoff_state=Preparing`. Your Run Agent Doc landed in that window.

The refusal itself was a "not yet" dressed up as a failure. Every `controller not authoritative` bail happens at the *top* of its handler — before pane state, the CRDT, or the dispatch record is touched — so the request provably did nothing, and a `Preparing` generation either promotes to `Stable` or gets reaped by the watchdog. So the client now waits it out: up to 10s, re-asking every 200ms, **reconnecting on each attempt** so the retry reaches the promoted generation instead of re-asking the process that already said no. It logs `controller_rpc_handoff_retry` on the first refusal and `controller_rpc_handoff_retry_settled` with the wait when a generation finally answers; if the handoff genuinely never promotes, the real refusal still surfaces, bounded. This sits in `request_controller_with_timeout` / `request_existing_controller_with_timeout`, so it covers the layout observation, the CRDT replica refusals, and dispatch alike.

Four tests, including the bound — a controller that never promotes must not turn a refusal into a hang.

### Re: `/clear` was not proven submitted in pane %16 — opus-5

> **Queue prompt:**
>
> do [#presetnullblank]


Same 24-second window, and I am **not** claiming this one fixed. What the log actually shows, at 20:24:15Z on `%16` and 20:25:01Z on `%15`: `result=submission_unobserved`, `command_visible=false`, `elapsed_ms≈908`, captures of 2146 and 1138 bytes. Those captures are far under the 200-line cleared-state threshold, so `#clearsubmitunobserved`'s idempotent-clear escape should have resolved them — it did not, which means neither capture contained a dispatch-ready prompt line. The panes were mid-restart from the recycle, not busy and not queueing (no `#clearqueuedcomposer` label).

So the guard did its job: it refused to call an unobservable clear "submitted" and gave you the exact unblocker. The defect is upstream — the clear was attempted at all while the controller was churning. The handoff retry above plausibly covers it, but I cannot prove that from this evidence and I am not going to say it is fixed. Filed as `#clearunobservedrecycle` at the top of the backlog with the repro (`make install` against live supervisors, then Clear Session Context within ~30s), plus `#clearsubmitpanesettle` for the smaller thing: a restarting pane and a pane that swallowed the clear currently produce the same message, and you cannot tell them apart.

**Verification:** full suite green — 9348 tests, 0 failures (up 9 from the 9339 baseline: 5 frontmatter + 4 handoff). Built, installed, and both commits pushed (`e13404d9c` in the submodule, `f5b35c7e2` in the superproject). CI run 34644974999 was still queued when I checked, so this rests on local evidence. One housekeeping note: `src/session_actor_cmd.rs` carries another session's uncommitted doctor-message change in this shared tree — I left it alone and committed only my four files.

Exchange is at ~700 lines across 28 sections; not stopping for it.

### Re: #exchangetypingrevert — the CP write was the loss site, not the reconcile — opus-5

The item was right that `crdt_replica_update_corruption_rejected` is a detector. The thing that produced the divergent member replica is **agent-doc's own controller write**, one second earlier.

The ops.log window around `17:22:09` reads in order:

```
17:22:08  queue_maintenance_rebased  expected_len=60856 current_len=60867 rebased_len=60967
17:22:09  crdt_cp_write  source=queue_maintenance  content_len=60967  update_bytes=3370  targets=1
17:22:09  crdt_document_op_delta_ingested  delta_len=98        ← the operator, still typing
17:22:09  crdt_replica_update_corruption_rejected  reason=cross_component_raw_union
```

`3370` update bytes for a net `+100` character change is the tell. `RelayHub::apply_canonical_replace` turned that whole-document write into CRDT ops through `minimal_char_span_edit`, which peels **one** shared prefix and **one** shared suffix — so every difference collapses into a *single* delete+insert span running from the first differing character to the last. `queue_maintenance` strikes a queue line **and** rewrites the status line: two disjoint regions, with `exchange` sitting between them. That one span therefore tombstoned the entire untouched `exchange` component and both of its markers, and reinserted them as brand-new characters.

That is the whole mechanism. An insertion the operator makes concurrently inside that range is anchored among those tombstones, so the raw union materializes it on the far side of a component marker; `relay_update_capture` notices the cross-component union and `rebuild_component_isolated_epoch` rebootstraps the canonical *and every member replica*. The operator watches their own paragraph revert. `merge_by_component` and `rebuild_component_isolated_epoch` were correctly ruled out — they are what runs after the damage, and neither could have caused it.

**The fix is one span per changed region.** `minimal_char_span_edits` peels the identical head and tail linearly, runs line-level Myers only over what is left, trims each hunk by its own shared prefix/suffix, and returns the spans highest-offset-first so each one still addresses the pre-edit text. A component the write left byte-identical keeps its original character identities, so a concurrent insertion inside it merges natively and there is no cross-component union to reconcile at all. The head/tail peel also keeps Myers off the 60KB whole-document case, where it would otherwise be O(N·D) over the entire file.

**What I proved, and what I did not.** The mechanism is proven — two tests go red without the fix. What I could not recover from the log is the exact span of that one 17:22:09 write. `update_bytes=3370` for a `+100` net change implies a span of roughly 3000 characters, which fits `queue`+`backlog` (adjacent in this document) and does **not** reach up into `exchange`. I wrote a fixture at that exact geometry and it passes *both* before and after the fix, so I deleted it rather than ship a test that cannot fail — that is a false green, not evidence.

So the honest chain is: the CP write is the only cross-component writer in the window; the single-span edit provably manufactures cross-component tombstones; and once `component_isolation_reconciled` fires for any reason, `rebuild_component_isolated_epoch` rebootstraps the canonical **and every member replica** — which is the only thing in the system that resets an operator's editor wholesale, and therefore the only thing that can look like "my typing reverted." My fix removes a large class of spurious triggers. It does not remove the rebootstrap itself, and the reconcile still decides to fire by comparing against a merge over a **synthetic** base that the code's own comment says "cannot be trusted." That residue is queued as `#reconcilesyntheticbase`, at the top of the backlog.

**I proved the tests detect the defect rather than trusting them.** Collapsing the hunk merge back to a single span (`if true ||` on the adjacency condition) reddens both regression tests with the production shape, and restoring it greens them:

| test | what it pins |
|---|---|
| `a_two_region_cp_write_must_not_tombstone_the_untouched_component_between_them` | behavioural — the operator's text survives **and** no isolation reconcile fires |
| `a_two_region_write_leaves_the_component_between_them_untouched` | the mechanism, CRDT-free: no returned span may contain another component's bytes |
| `split_spans_reproduce_the_target_text_exactly` | the split stays a faithful replacement (no trailing newline, pure insert, pure delete, multi-byte) |

The third stays green under the mutation on purpose — it is a faithfulness invariant, not the defect.

**One existing test changed, and I want to be explicit about it.** `notes_delete_cannot_splice_a_concurrent_exchange_response` asserted `packet.component_isolation_reconciled` and `pending_rebootstrap_members() == [2, 3]`. Both now fail — because in that scenario the CP write leaves `notes` byte-identical, so with per-region spans `notes` is never tombstoned, the concurrent delete merges natively, and there is nothing cross-component left to repair. Its content assertions are untouched and still pass: the canonical is byte-identical to the same `expected`, and the response still never reaches `notes`. I inverted the two mechanism assertions rather than deleting them, because the reconcile repairs by rebootstrapping the canonical *and every member replica* — which is precisely the visible revert this whole item is about. Never needing it is the stronger outcome, so the test now asserts it did not run.

**On the `stale_plugin` cross-check the item asked for:** not implicated. The divergence is produced hub-side, inside the binary, by a span this binary computes; the plugin's `agent_doc_replica_diff` update at `delta_len=98` is an ordinary well-formed 9-character insertion. The 0.2.379-vs-0.2.381 warning is still standing and still wants an IDE restart, but it is a different problem.

### Re: Run Agent Doc on sdk.md wedged — two causes, both fixed — opus-5

The pane itself was not deadlocked, but the first cycle really did die, and its scrollback names the cause outright:

```
❯ /agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md
  ⎿  UserPromptSubmit hook timed out after 30s — output discarded.
● I'll start by checking the cycle contract from the preflight hook.
✻ Worked for 2s · done
```

A 2-second no-op turn, no contract, no reason. `session-check` confirmed `cycle-1789161041234` stuck at `preflight_started`. Because that turn never closed, the queue stayed active and the supervisor idle watch re-dispatched — four `/agent-doc` triggers were stacked in the composer by the time you flagged it.

**The runtime cause.** `src/haiven-dev/.claude/settings.json` wires the preflight hook with **no `timeout`**, so Claude Code applies its 30s default and *discards the output* on expiry. The superproject's settings beside it carry `timeout: 120`. Preflight is nowhere near 30s-safe on these documents — my own `session-check` on `sdk.md` just measured `resolve_current_document_content: 2878ms across 8 calls`, and preflight does far more than eight. I repaired that file; it was the only stale one in the tree.

**The agent-doc cause — and it is the one that matters, because the repair above only ever happens when someone notices.** `PREFLIGHT_HOOK_TIMEOUT_SECS = 120` is written exclusively by `merge_claude_turn_status_hooks`, which runs on an explicit install *in that directory*. A submodule initialized by an older binary therefore keeps Claude's 30s default forever, and nothing in the product ever looks. Worse, under a 30s deadline the binary's own 90s `HOOK_ADMISSION_BUDGET_SECS` is not a budget at all: the harness kills the hook and discards its output long before the binary reaches the line that would have named its overrun. The entire `#hookcontractlost` design — "the binary always gets to name its own overrun before the harness kills it silently" — was inert in exactly the checkouts that needed it.

Two changes, one per session:

- **This session:** `admission_budget_under_harness_deadline` clamps the budget under the deadline the settings actually wire, with headroom to print and a floor so a pathological value cannot refuse turns a healthy controller would admit. Under a 30s deadline the budget becomes 25s, so a slow preflight now produces the `UNAVAILABLE` marker *with a reason* instead of silence.
- **The next session:** `repair_claude_preflight_hook_timeout` writes the full timeout back onto a preflight entry the file already wires. Deliberately narrower than the merge — it adds no hook the file does not have and touches no other key, so it cannot resurrect something you removed on purpose.

Verified end to end through the real hook binary against a scratch project, not just the unit tests:

```
before: [None]
[agent-doc] repaired …/.claude/settings.json preflight hook timeout 30s -> 120s;
           the 30s deadline still applies until Claude Code reloads settings
after:  [120]
other hooks preserved: 1
```

Four regression tests cover the clamp (untimed, correctly-installed, unknown-layer, pathological), the repair, and the must-not-touch case.

The trigger stacking is downstream of the no-op turn — a turn that never closes leaves the queue active, so the idle watch keeps re-dispatching. That resolves itself now that the contract arrives. The general shape (a failing turn stacking N triggers with no composer-level dedup) is queued as `#idledispatchstack`.

### Re: Clear Session Context on backend.md — yes, we were double sending — opus-5

> **Queue prompt:**
>
> do [#exchangetypingrevert]


Confirmed from the log, not inferred. One operator Clear produced two submits:

```
21:31:56  session_clear_sent            delivery=supervisor_ipc
21:31:57  session_clear_submit_observation  phase=supervisor_ipc_acceptance
                                           result=submission_unobserved capture_len=1157 capture_hash=95c262b69155
21:31:57  tmux_input_event  source=session_clear.supervisor_ipc_resubmit
                            transform=clear_resubmit_full_command key=text bytes=6   ← "/clear" again
21:31:57  tmux_input_event  ... key=Enter
21:31:58  session_clear_submit_observation  phase=supervisor_ipc_command_repair_acceptance
                                           result=submission_unobserved capture_len=1183 capture_hash=81857010df9f
21:31:58  session_clear_submit_blocked
```

The two capture hashes differ and the capture grew 26 bytes, so **the resend visibly landed** — the pane had been accepting input the whole time. You got two clears and a `blocked` verdict. Pane `%12` is sitting at `ctx:11%` as I write this, which settles it: the clear worked, and the report that it had not was wrong.

**Why it resent.** `context_clear_submit_retry_action` fired `ResendCommand` on `status == Unobserved && !command_visible`, with the comment "`/clear`-style commands are idempotent, so repair the lost delivery with the full command." But `Unobserved` does not mean the delivery was lost — it means *the observation window closed with no evidence either way*. That is precisely the collapse this codebase's own `#idlerevisionreactive` rule forbids ("a cheap probe that gates expensive work must keep 'I did not look', 'I looked and got no answer', and 'here is the answer' as distinct outcomes"), and it inverts behaviour at the worst moment: the state where we cannot tell is exactly the state where a second submit is most likely to be a duplicate. And `/clear` is idempotent in *effect*, not in *delivery* — a second one still consumes a submit.

**The evidence was already being computed and thrown away.** The poll loop derives `content_changed_since_delivery` per frame to recognise acceptance, then discarded it on timeout, so the retry decision could only see that acceptance was unproven and had to guess why. It now rides on `ContextClearSubmitObservation`, and the full-command resend requires the pane to have **never moved** since delivery — the one state a lost delivery actually produces. Mutation-checked: removing the guard reddens `a_pane_that_visibly_moved_since_delivery_is_never_resent_the_full_command`.

I deliberately left the `SubmitKey` retry alone. A command sitting *visibly* in a split-submit composer is proven un-submitted, so pressing the harness submit key duplicates nothing; `a_visible_stuck_draft_still_earns_a_submit_key_even_after_the_pane_moved` pins that.

**What this does not fix, and I would rather say so than let it look closed:** the false `submission_unobserved` verdict itself. `retained_lines=2041` in that log is a red herring — it counts non-empty lines in the whole tmux *scrollback*, which survives a `/clear` and is only logged, never used as the gate. The real verdict comes from `context_clear_history_proves_cleared_state`, which needs a settled dispatch-ready prompt it could not find. That stays tracked as `#clearunobservedrecycle` and `#clearsubmitpanesettle`. What is fixed is that failing to prove it no longer costs you a second `/clear`.

**Verification.** Full suite green: **9359 tests run, 9359 passed, 0 failed**, `make check` exit 0 including the editor-parity release gate. All three fixes are mutation-checked — I reverted each guard in turn and confirmed the matching test reddens with the production shape, then restored it.

Two things worth naming rather than burying. Earlier in the turn two `orchestrate::dag` tests failed with `controller_binary_stale (running controller binary 0.35.367 differs from caller 0.35.366)` — version skew from bumping mid-run against live controllers, not a regression; all 15 pass on the aligned build. And `src/session_actor_cmd.rs` is also being edited by another session right now, so rather than sweep their in-progress doctor-message change into my commit I staged a hand-built blob of `HEAD` plus only my four hunks. Their work is untouched in the tree.

Released as **0.35.367**: committed `ff337b269`, pushed, tagged `v0.35.367`, built, installed (`agent-doc 0.35.367` confirmed live), supervisor recycled. The haiven-dev hook-timeout repair is its own commit (`a933132`) in that submodule. The Release run is in flight; per `#ci-no-closeout-wait` I am reporting it rather than waiting on it.

One thing you should see: the superproject's **Agent Doc workflow has failed to start on its last five runs** — `"The job was not started because recent account payments have failed or your spending limit needs to be increased"`. That is a billing block, not a code regression, but nothing in that workflow has actually executed. agent-doc's own CI is unaffected and green.

### Re: do [#reconcilesyntheticbase] — opus-5

> **Queue prompt:**
>
> do [#reconcilesyntheticbase]


Done, and the reproduction the earlier investigation said did not exist is now a test.

**The synthetic-base arbiter was firing, and on nothing.** I probed the live hub with a matrix of stale-projection shapes and found six that make `isolated != after_text` — the old trigger — reachable through the current write path. In **four** of them the disagreement is deletion-shaped, so `member_insertions_lost_by` saw no lost insertion, `lost.is_empty()` held, and `rebuild_component_isolated_epoch` ran: canonical rebuilt, lineage rotated, **every member replica rebootstrapped**. A stale member deleting a body the CP just replaced does it. A member two generations behind typing into `exchange` does it (saved only by the insertion net). Not one of the six carries any cross-component evidence — all six are ordinary intra-component convergence, exactly the class where resetting every editor's buffer is pure loss.

That is now `a_synthetic_base_merge_disagreement_alone_no_longer_resets_every_replica`, which asserts both halves: the arbiter still disagrees on each shape (otherwise the test stops covering the regression), the recorded pre-fix outcome is still the one each shape produces, and nothing is repaired or rebootstrapped now.

**(a) The trigger is positive evidence.** `cross_component_union_damage` asks the question the firewall exists to answer instead of consulting a merge. The union is `canonical_before` plus exactly one delta — the ops pulled from this member's mirror — so a region that changed while the member's own before/after buffers agree it was untouched can only have changed because the member's characters materialized there. Documents split into stably keyed regions: `component:<name>:<occurrence>` for bodies, plus the framing runs between them keyed by the component they follow, so markers and text outside every component are a region of their own rather than folded into a neighbour. A region the member *did* edit is native single-component convergence and is never reported. A region with no comparable counterpart yields no verdict — an unprovable suspicion must not authorize a repair that can cost operator bytes, and structural breakage that reaches the canonical is already owned by the `agent-doc-crdt-relay-io` parse guard. `merge_by_component` and the synthetic `CrdtDoc::from_text` base are gone from this path entirely.

**(b) The repair is one region.** Only the regions carrying evidence are restored to the content the canonical already held, through targeted local edits that ride the normal delta and name `origin` among the targets, so the editor that can see the damage converges from that delta like any peer. No lineage rotation, no rebootstrap. Relay-io no longer forces a whole-document canonical projection for it either — overwriting that one editor's buffer is the same wholesale reset, for one member instead of all of them. And the `#queuelineclobber` post-condition is checked on the *prospective* text before any op reaches the canonical, so a refusal costs nothing.

**Both holes in the net are closed.** It compared survivors against the inserted *delta*, so a duplicate of a line the document already held read `1 < 1` and reported no loss — it now compares against the member's post-edit count. And it skipped blank lines entirely, so a whitespace-only insertion could never be reported — blank lines are counted under their own key. Under a region-scoped repair neither change can produce a false refusal, because the repair never touches the region the member was editing.

**The positive path has an end-to-end test too.** The controller removes `notes` while the operator types inside it; every character around the caret is tombstoned and the insertion materializes in the framing run between `exchange` and `queue`. The detector names that region, the restore is correctly refused because it would delete the operator's line, and the raw union is published — text misplaced but present, no replica reset. That refusal is honest but incomplete: nothing moves the line back. Queued as `#isolationrelocate` at the top of the backlog, with the relocation shape spelled out.

What I could not construct at hub level is a shape where the restore *applies*: every damage shape I could reach is the member's own insertion, which the loss guard refuses, and the one deletion-shaped candidate (a stale buffer deleting a component's open marker) breaks `element::parse` and is owned by the parse guard instead. So the applied restore is fail-safe code proven by unit tests, not by a live shape — saying that plainly rather than implying more coverage than exists.

`make check` green: 9366 passed, 0 failed. `cargo build --release` green. Mutation-checked cold: dropping the member-intent guard reddens the six-shape regression test *and* the pre-existing `notes_delete_cannot_splice_a_concurrent_exchange_response`; restoring either loss-guard hole reddens its own assertion. Committed `837a6187e`, pushed, installed (`0.35.367`, binary mtime 19:43), supervisor recycle requested. CI run `34659107871` was still in progress at closeout — the prior run on `ff337b269` was green.

### Re: ❯ 52 — opus-5

**`❯ 52` is not something I can act on.** It landed at line 62, inside the committed `### Re: #restartideaplugin…` section, between the `#restartideaplugin` and `#pypiquotafix` paragraphs — a bare shell prompt plus a number, with no directive, id, or question. It reads as an accidental paste from a terminal. I left it in place rather than deleting operator text. Tell me what you meant and I'll act on it; say "strip it" and I'll remove the line.

Since there was nothing else to execute this turn, I chased the thing that actually matters here: **why ten queued `do [#id]` heads produced zero drain.**

**The queue is in `start`, not `go`.** That is the whole answer, and it is not a defect. Measured directly against this document's bytes rather than inferred from the prose:

| frontmatter | `drainable_head_count` | in-session loop head |
|---|---|---|
| `queue: start` (current) | **0** | `None` |
| `queue: go` | **9** | `idledispatchstack` |

`drainable_head_count` resolves activation through `active_queue`, which requires go-mode; `drainable_head_prompt_for_scope` resolves it through `active_queue_for_supervisor_start`, which accepts `start`. So with `queue: start` the supervisor scope finds `#idledispatchstack` drainable while the in-session `/loop` scope correctly finds nothing — preflight's `queue_drainable_head_count: 0` / `queue_continuation_required: false` is that distinction being reported honestly, not a stranded queue. **Flip the frontmatter to `queue: go` if you want this session to drain the nine.** The tenth, `#cargotokenrevoke`, stays deferred in both scopes — it is the only `[operator-verify]` item in the set, and the deferral set confirmed it: `DeferredBacklogIds { all: {"cargotokenrevoke"} }`.

I verified this with a throwaway integration test against the live document and removed it; nothing from the probe is left in the tree.

**`stale_plugin` is back, and it is not a regression of the restart.** Last cycle's claim ("no `stale_plugin` warning") was true when it was made — the IDE had just loaded 0.2.379. Then `make install` at 19:43, alongside binary 0.35.367, staged 0.2.382 into `~/.local/share/JetBrains/IntelliJIdea2026.1/agent-doc-jetbrains/lib/`. The live IDE is still on 0.2.379, so the warning fired again on the same treadmill: every agent-doc install stages a newer plugin jar, and the floor for `#tmuxfocussyncverify` / `#activateinstalledjetbrai` / `#harnessswitchverify` breaks until you restart. Filed as `#jbplugin382restart` at the top of the backlog, tagged `[operator-verify]` so go-mode defers it rather than churning.

**One more thing `session doctor` reported, worth a look before you trust editor focus.** Pane `%0` — this session's pane — now lives in window `@3`, while the actor and registry records both still say `@0`. I read the window names live rather than trusting the bullet: `@0` is named `agent-doc` and is the **active** window; `@3` is named `stash`, inactive, 13 panes. So the pane really did get moved into the stash and the recorded binding is genuinely behind. The named repair is `agent-doc session doctor --repair`, but rebinding focus mirroring onto a stashed window is exactly the case worth a deliberate decision, so I did not run it — say the word and I will. This is the same recorded-vs-live binding that `#layoutbindstale` already describes in `derive_layout_actor_bindings`; no new item for it.

No code, test, or build surfaces changed this cycle, so no suite run applies. The exchange is at 879 lines across 35 sections — noted, not acted on, since compacting is not opted in here.

### Re: do #cpwritecomponentscoped — opus-5

Done and pushed — `875a48a6e`, `make check` 9385 passed / 0 failed (up 19 from 9366), installed at 0.35.367.

**What was actually wrong.** Every CP write handed `RelayHub::apply_canonical_replace` a whole-document target and let a text diff rediscover which components changed. `#exchangetypingrevert` made those derived spans per-region, so an untouched component between two changed ones is no longer tombstoned — but that is still a diff noticing after the fact, and it is the *wrong* diff: the relay compares against the **canonical**, while the CP's intent is a delta from the baseline it planned against. Those are different comparisons taken at different times, and only the second one knows what the write meant.

**What the scope now does.** `RelayHub::apply_canonical_replace_scoped` never takes a whole-document diff. It resolves each scoped component's body range in both documents, diffs only those bodies, and shifts the resulting spans into document space — so the write is structurally incapable of *emitting* an edit for another component, not merely audited for having done so. Everything outside the scoped bodies must match exactly; a target that moved text outside its own scope is refused before a single replica sees it.

Three cases, all tested:

| scope | outcome |
|---|---|
| resolves in both documents | `Enforced` — edits confined to the scoped bodies |
| target changed text outside the scope | refused, canonical untouched |
| a scoped component absent on one side (the write creates or removes one) | `Unresolvable` — falls back, logged, still lands |

That third row matters: `review_add` materializes a missing component, and a scope cannot bound a write that changes the component *layout*. Refusing there would have broken a working path, so it fails open and says so in the ops log (`crdt_cp_write_component_scope outcome=unresolvable`).

**How the scope reaches the relay.** `agent_doc_element::component_scope` owns the type, the ambient thread-local, and `changed_component_scope`, which derives a mutation's own component delta. The ambient is deliberate rather than a parameter: the scope has to survive `persist_pending_write` → `converge_or_disk_write` → the CRDT CP write, a chain every other writer shares, and that path already carries `FORCE_DISK_PENDING_WRITE` the same way. Threading it explicitly would have touched a trait method plus about twenty-five call sites across six crates for no additional guarantee.

The tracked-work transaction accumulates the **union** of every staged mutation's declared scope and installs it around the publish. One unscopable mutation poisons the envelope to unscoped — an incomplete scope must never be the reason a legitimate write is refused. Over the RPC it travels as an optional `(name, occurrence)` list with `#[serde(default)]`, so a client predating the field keeps the unscoped path.

This sits a layer below the `#backlogeditcorruptsreview` guard (`ensure_only_component_changed`), which already checks one command's target before staging. That guard proves one mutation's intent; this one bounds what the relay may do with the *published envelope*, which is where the canonical-versus-baseline mismatch lives.

**Verification.** Mutation-checked cold: replacing the outside-scope comparison with `false` reddens both `a_scoped_cp_write_that_would_touch_another_component_is_refused` and `an_empty_scope_refuses_every_change`; restoring it (from a scratch copy, with a `touch` so cargo could not reuse the mutated build) returns all six hub tests to green. `an_unscoped_cp_write_still_publishes_a_cross_component_target` is deliberately in the suite as the negative control — it pins the old behaviour, so a future change that made the scope a no-op would not read as a pass. CI `34659107871` was green on the parent commit; this commit's run is new.

Two notes from this cycle's preflight, neither blocking: the `queue_maintenance_not_persisted` warning means the previous queue text is still in effect, so a newly added backlog id may not have reached the `agent:queue` mirror yet; and the exchange is at 902 lines across 36 sections.
<!-- agent:boundary:330692a0:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
- 🚧 do [#idledispatchstack]
- ~~do [#cpwritecomponentscoped]~~
- do [#clearunobservedrecycle]
- do [#clearsubmitpanesettle]
- do [#queuegorevokedonfreetext]
- do [#layoutbindstale]
- do [#jbfocusdocroute]
- do [#compactackflake]
- do [#retainedresumewakeflake]
- do [#cargotokenrevoke]
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#jbplugin382restart] [operator-verify] [P2] Restart IntelliJ to load JetBrains plugin 0.2.382. `make install` at 19:43 (binary 0.35.367) staged `~/.local/share/JetBrains/IntelliJIdea2026.1/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.382.jar`; the live IDE is still on 0.2.379, which is what preflight's standing `stale_plugin` warning reports. Same shape as the closed `#restartideaplugin` — it re-broke the plugin floor for `#tmuxfocussyncverify`, `#activateinstalledjetbrai`, and `#harnessswitchverify`, and for the `#jbfocusdocroute` half already shipped in 0.2.381.
- [ ] [#isolationrelocate] [P2] `#reconcilesyntheticbase` proved a member insertion CAN materialize outside every component — the controller removes a component while the operator is typing inside it, every character around the caret is tombstoned, and the line lands in the framing run between two components. The narrow repair correctly REFUSES there: restoring that framing run would delete the operator's line, and no other replica has seen it yet, so the raw union is published with the text misplaced but present and `component_isolation_refused_lossy` logged. Nothing then moves it back — the operator repositions it by hand. The repair that closes this is relocation rather than restoration: restore the damaged region from the canonical AND re-insert the member's lost lines at the position its own intent shows, inside the component it was editing (or the nearest surviving component when the CP deleted that one), then re-check the `#queuelineclobber` post-condition on the result. Reproduction already in tree: `a_member_insertion_that_lands_outside_every_component_is_flagged_not_deleted` in `agent-doc-document-realtime/src/crdt_relay.rs`.
- [ ] 🚧 [#idledispatchstack] [P2] A turn that fails to close leaves the queue active and the supervisor idle watch re-dispatches with no dedup against a trigger already sitting in the target composer. Observed 2026-09-11 on `src/haiven-dev/tasks/sdk.md`: after the `#hookcontractlost` no-op turn, FOUR `/agent-doc .../sdk.md` messages were stacked in pane %1 (`queue_dispatch_invoked` at 21:07:26 and 21:10:23, `command_kind=dispatch_only_reopen actor_state=ready`). Fixing the contract removed this instance's trigger, but the shape is general — any turn that dies without closeout re-arms the watch. `pane_composer_has_pending_trigger` (`agent-doc-controller/src/dispatch.rs:1488`) already classifies exactly 'this trigger is queued unsubmitted in that composer' for the `#jbtsiftnosub2` fresh-pane path; reuse it as an idle-watch precondition so the watch defers instead of stacking.
- [ ] [#clearunobservedrecycle] [P2] `/clear` reported `submission_unobserved` on two haiven-dev panes (%16 docs.md, %15 dev.md) at 2026-09-11T20:24:15Z / 20:25:01Z, both `command_visible=false`, `elapsed_ms≈908`, `capture_len=2146`/`1138`. Evidence: `src/haiven-dev/.agent-doc/logs/ops.log` shows the same window churning controller generations 1176→1178 (`controller_active_non_stable_deferred ... handoff_state=Preparing`), so the panes were mid-restart and held no dispatch-ready prompt line — which is why `context_clear_history_proves_cleared_state` (#clearsubmitunobserved) could not resolve the idempotent case and the guard fell through to blocked. The guard itself is correct (fail-closed, exact unblocker). What is missing is the preceding step: the clear was attempted against a controller that had already refused as non-authoritative. Check whether the client-side #handoffrefusalretry landed in e13404d9c now defers the clear until the generation promotes; if it does not cover the supervisor_ipc_command_repair_acceptance path, extend it there. Repro: run `make install` against live route-owned supervisors, then issue Clear Session Context within ~30s.
- [ ] [#clearsubmitpanesettle] [P3] [recommended] `context_clear_history_proves_cleared_state` requires a dispatch-ready prompt line, so a pane that is restarting reads identically to a pane that swallowed the clear. Consider a distinct `PaneRestarting` status (harness banner present, no prompt yet) whose unblocker is "wait for the harness to finish starting" rather than "verify pane state then retry" — the operator currently cannot tell those two apart from the message.
- [ ] [#queuegorevokedonfreetext] [P2] After #queuelineclobber, an operator who writes a free-text `re [#id]: <question>` queue line and flips `queue: go` keeps their text but still gets the activation revoked (`[preflight] queue: stripped activation token (drained)` then `queue: set queue: stop`) — that path is separate from the now-gated need_clear_drained_body. Defensible as-is (a `re` line is not a drainable head, and the inactive_queue_residue warning routes it to the next cycle's prompt), so this is a design question, not a proven defect: decide whether an operator activation should survive a body that holds only non-drainable operator text, or whether the revoke plus the residue warning is the intended answer. Decide before changing it — go-mode churn (#qchurn) is the failure mode on the other side.
- [ ] [#layoutbindstale] [P2] `derive_layout_actor_bindings` (src/agent-doc/agent-doc-controller-io/src/project_controller.rs:746-784) builds `desired_windows` from the RECORDED `record.window_id`, then binds every other actor whose recorded window is in that set as a co-window sibling of the layout. The record is provably untrustworthy: `agent-doc session doctor src/haiven-dev/tasks/infra.md` reports actor+registry `window=@0` for pane %14 while tmux has %14 in @3 (@0=agent-doc visible, @3=stash). So a stashed pane can be joined into a layout it is not part of, and after a `session doctor --repair` (which rebinds the record to the LIVE window) every stashed pane would share the stash window id and be bound together. Decide whether this join should consume a window id at all, or key on the desired document set alone. NOTE from #stashfocuswindow: resolving live is NOT automatically the right answer — for a selection effect the live window is the stash, which is precisely what must not be used. Status: a reading of the join, not an observed symptom; needs a repro before changing it.
- [ ] [#jbfocusdocroute] [P2] The JB "Focus Agent Terminal" notification is per-document ("Agent input required for $fileName", TurnStateBannerRefresher.notifyInputRequired) but IdeTerminalHost.focusExisting(project) ignores the file and focuses a single project-wide terminal tab named "agent-doc". In a multi-document tmux layout the correct target is the pane that OWNS that document. The plugin already has the machinery (TmuxPaneFocusSync.recordEditorFocusIntent takes a documentPath, though it only records a lease — the actual routing goes through the command plane via EditorFocusSyncListener). Do this together with #stashfocuswindow: both are pane->window/document resolution, and solving it in two places separately is how three tmux focus reports became three separate reports. The silent-no-op half of this is already fixed in plugin 0.2.381 (commit 50bc06de5); what remains is routing to the right pane rather than merely always navigating somewhere.
- [ ] [#compactackflake] [P2] CI run 34626410869 (0.35.366, workflow CI) is RED on agent-doc-document-realtime-io tests::compact_exchange_returns_retained_success_when_editor_ack_stays_delayed, panicking at agent-doc-document-realtime-io/src/lib.rs:9472 (3183 passed / 1 failed / 259 skipped, 1025s). It predates the #fixdocsturn commit 0f959e11f and is not reachable from it (that change touches agent-doc-diff + baseline_comparison only), and local `make check` on the same tree is green. Same delayed-editor-ack shape as #retainedresumewakeflake, and that item already carries the diagnosis to apply here: the test synchronises on a subscribe RETURNING rather than on the condition holding, so a contended runner returns from subscribe for something other than the awaited event. Check whether :9472 waits on a subscribe/timeout proxy and convert it to a bounded poll of the actual condition.
- [ ] [#retainedresumewakeflake] CI run 34621201563 (0.35.364) went red on agent-doc-controller-io `project_controller::tests::retained_resume_reacts_when_the_intent_arrives_after_delivery` — `assertion left == right failed; left: None, right: Some("retained_settled_delivery_reactive")` at project_controller.rs:15843. Already checked, do not re-walk: it does NOT reproduce locally (~900 executions, including `taskset -c 0` with --test-threads=8 and a 2-core taskset), and neither 0.35.364 change is reachable from it — pane_layout_projection_converged is in the pane-layout worker this test never drives, and capture_resume_unowned is in agent-doc-capture-io, which agent-doc-controller-io's lib test binary does not link. That CI run took 805s vs 166s locally, so the runner was heavily contended. Lead: the test's synchronization is a proxy, not a wait. It asserts `!subscribe_state_plane(CAPTURED_FINALIZE_WAKE_STATE_CHANNEL, None, 0, 2s).timed_out` and then reads runtime.captured_finalize_wakes immediately. record_captured_finalize_wake (rpc.rs:18944) inserts into that map BEFORE publishing the frame, so an absent key proves the subscribe returned for something other than this wake — check what state_plane_effective_after_version(None, 0, gen) plus the generationless-cursor reset makes `subscribe` return when the channel has no frames yet. Fix: wait on the condition (poll captured_finalize_wakes for the expected reason under a bounded deadline) rather than on subscribe-return, and apply it to all three sites using this pattern (15694, 15843, 15898). Keep both assertions — that settlement publishes the receipt AND that it records the reason.
- [ ] [#cargotokenrevoke] [operator-verify] Revoke and re-issue the crates.io API token exposed in diagnostic tool output during the pane-removal investigation, then store the replacement via passage. BLOCKED ON ONE CLICK: https://crates.io/settings/tokens is open in bridged Chromium tab 13 and returns 'This page requires authentication' — that profile has no crates.io session and no chromium-bridge state snapshot exists. crates.io token management is session-cookie-gated, so the exposed token cannot revoke itself via API. Unblocks: click 'Log in with GitHub' in tab 13 (OAuth returns to /settings/tokens), then re-queue this item and the agent enumerates and revokes in-session. Blast radius nil: agent-doc has no cargo publish step and no CARGO_REGISTRY_TOKEN in any of its five workflows (book, ci, mdbook, pypi, release).
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [/] [#pypiquotafix] [operator-verify] PyPI 10 GiB project-size block: non-destructive path (a) EXECUTED 2026-09-10 — filed https://github.com/pypi/support/issues/12214 requesting 25 GiB. Awaiting PyPI maintainer response; every agent-doc publish since 0.35.343 fails with 400 Project size too large, and 0.35.343 itself landed only 1 of 4 wheels (macosx_10_12_x86_64). Measured at filing: 9.990 GiB across 165 releases, avg 62.0 MiB. Unblocks: (1) PyPI grants or counters the increase, or (2) you authorize destructive path (b) — delete the 34 oldest releases 0.35.0 through 0.35.112 (all pre-2026-08-02) to free 1.75 GiB, permanent and those version numbers can never be reused. The publish cadence fix landed in 0.35.363 (#pypicadence): milestone tags publish automatically, other tags publish on demand, and a verify job now asserts the tagged version is resolvable on PyPI. ALSO GATED ON THIS: the GH #52 wheel-FFI path is written and proven at the maturin layout level (data = wheel-data puts libagent_doc.* in .data/scripts/ so pip lands it beside the binary, with a pre-upload assertion) but has never run in CI, because no publish has been possible since the quota block. On the first successful publish after this unblocks, confirm the wheel ships the library and then close GH #52.
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
