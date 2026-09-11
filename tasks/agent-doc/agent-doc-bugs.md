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
queue: go
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
<!-- agent:boundary:1406b99d:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~As I was typing in this file's agent:exchange when the turn started, my changes in agent:exchange were reverted by agent-doc.~~
- ⏭️ do [#retainedresumewakeflake]
- do [#cargotokenrevoke]
- 🚧 The JB "Focus Agent Terminal" does not navigate to anything.
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#queuelineclobber] [P0] agent-doc destroys operator-authored text in a live document mid-turn. Sightings 2026-09-11: (a) agent-doc-bugs.md 17:22:09, ops.log `crdt_replica_update_corruption_rejected ... reason=cross_component_raw_union recovery=canonical_restored_and_member_reprojected` — operator exchange typing reverted, their free-text queue item and its promoted backlog item both vanished; (b) src/haiven-dev/tasks/infra.md — HEAD c0fae3b queue line `- re [#fpeidentitysecondclient]: Since we moved over to a Secret Bearer Token...` replaced by `- do [#fpeidentitysecondclient]U`, operator question text gone plus a stray U that exists nowhere in the removed text. RULED OUT 2026-09-11, DO NOT RE-WALK: the component-isolation merge is NOT the loss site. merge_by_component was probed with the exact base/canonical/editor triple the reconcile builds, across seven shapes — append-vs-replace, in-place-edit-vs-replace (the infra.md shape), exchange-typing-vs-append, append-vs-delete, in-place-edit-vs-body-rewrite, and both canonical-only-line-vs-stale-editor variants — and ALL SEVEN preserved the operator text. A regression test written against that hypothesis passed with the fix removed (mutation survived), so it was discarded rather than kept green. NEXT LEAD, start here: rebuild_component_isolated_epoch (agent-doc-document-realtime/src/crdt_relay.rs) does not merely replace the canonical — it resets every member replica and observed_replica from a fresh bootstrap, calls member.pending.clear(), and queues every live member for re-bootstrap, then the editor is told to replace its buffer. Anything a replica holds that has not reached the hub is discarded by construction, which fits "I was typing and my changes were reverted" far better than a merge dropping lines. Cross-check against the standing stale_plugin warning (live JetBrains plugin 0.2.379 vs the 0.2.380 shipped with this binary) — a rebootstrap handler in a stale plugin is a plausible contributor. Landed in 73795693b: a fail-safe post-condition (member_insertions_lost_by) that refuses a reconcile which would drop member insertions and logs `crdt_component_isolation_reconcile_refused` to ops.log. It is instrumentation, NOT a proven fix — it has no reproduction and never fires on any constructed shape.
- [ ] [#presetnullblank] [P1] agent-doc materializes a frontmatter value the operator never typed, which then clobbers the value they are typing. Reported on src/haiven-dev/tasks/docs.md: operator wrote a prompt_presets entry #rebase-conflicts and agent-doc reverted its value "rebase + fix merge conflicts" to the empty string. REPRODUCED DETERMINISTICALLY (probe over agent_doc_frontmatter::frontmatter parse/write): a preset key typed with no value yet parses to ("#rebase-conflicts", "") and writes back as an explicit empty-string value. So a mid-typing YAML null becomes an agent-authored empty string in the operator document, and that agent write then competes with — and can win over — the real value. Parse site: agent-doc-frontmatter/src/frontmatter.rs:1656-1662. SECOND, RELATED FINDING from the same document: HEAD 3f777fe holds the preset value single-quoted and the worktree holds it UNQUOTED — agent-doc re-serialized the operator YAML without changing any value, so write() is rewriting frontmatter it does not own. The clean fix for the null case needs prompt_presets to round-trip null (the env field at frontmatter.rs:766 already uses IndexMap<String, Option<String>> for exactly this) — measured blast radius is ~200 references across 23 files, so it is a real refactor, not a one-liner. Do NOT fix it by dropping null keys at parse: write() would then delete the half-typed line out from under the operator, which is the same harm. Prefer instead making the write path byte-preserving for frontmatter it did not change — frontmatter.rs:1435-1529 already documents that pattern (every other byte is preserved verbatim) and some write path is bypassing it.
- [ ] [#stashfocuswindow] [P1] Three related tmux focus reports, one root: (1) navigating to src/haiven-dev/tasks/infra.md focuses the tmux STASH window; (2) tmux auto-sync does not follow when selecting the opposite pane in IntelliJ; (3) the same for auto-FOCUS. CONFIRMED LIVE, do not re-diagnose: `agent-doc session doctor src/haiven-dev/tasks/infra.md` reports "actor window @0 for pane %14 is stale — the pane now lives in @3" and the same for the registry entry, so the recorded binding points at @0 (the stash) while the pane really lives in @3. Already ruled out: select_pane is NOT the culprit — tmux resolves %14 to its real window, proven by agent-doc-route-io/tests/route/startup.rs:3167 select_pane_switches_window. Already present and passing: the focus-MIRROR direction is guarded live-vs-record by src/sim_world.rs:198 a_stashed_column_never_receives_mirrored_focus with a record-only sensitivity proof at :240. Remaining suspect: a path that issues select-window against the RECORDED window instead of resolving the pane live. Durable fix is to resolve pane->window at use time and stop persisting a window id tmux already owns; `agent-doc session doctor --repair` is the manual band-aid and must not be the answer. Fix all three together — they are one binding bug, not three. Needs a live pane repro; see memory reference_agent_doc_live_pane_repro_harness_gotchas for the three traps.
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
