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

Grok v0.35.359 installed and pushed; full local verification passed. Authenticated Grok MCP closeout awaits renewed quota. Next: fix extra tmux pane with two editor panes.
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
<!-- agent:boundary:d53e8ec0:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#donequeuestrike]~~
- 📌 do [#fixrunlazily]
- ~~do [#mutplanwitness]~~
- do [#cargotokenrevoke]
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#tmuxautosyncwedge] tmux pane auto-sync is wedged: a pane-layout generation whose observation is byte-identical to its desired state never reaches Converged. Signature (2026-09-11 13:13:54): `pane_layout_projection generation=17 attempt=1 phase=retry_pending observation=synced effect=applied focus=focus_superseded_by_newer_layout_state expected=[tsift.md, infra.md] actual=[tsift.md, infra.md] expected_panes=[%10, %14] panes=[%10, %14] expected_focus_pane=Some("%10") active_pane=Some("%10")` — documents, panes and focus pane ALL match, and the projection still sits in retry_pending because the focus receipt was superseded. Today: 39 pane_layout_projection_cancelled vs 31 pane_layout_projection, so more generations are cancelled than projected. Already checked, do not re-check: pane_layout_effect_worker responds to a focus supersede with `attempt = 0; continue`, and the loop top re-reads `state.lock().pending_revision()` plus `runtime.pane_layout_desired()`, so it is NOT an infinite loop on a stale generation. Open question: why apply_pane_layout_focus_effect keeps returning focus_superseded_by_newer_layout_state (agent-doc-controller-io/src/project_controller/rpc.rs:19386, `state.is_superseded(generation)`) while the observed layout already matches, and why that blocks Converged when focus_required_for_pane_layout_generation could treat an already-active focus pane as satisfied. Needs the live pane state to verify; related to but distinct from the editor-route supersede fix in 5577cfeeb.
- [ ] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [ ] [#cargotokenrevoke] [operator-verify] Revoke and re-issue the crates.io API token exposed in diagnostic tool output during the pane-removal investigation, then store the replacement via passage. BLOCKED ON ONE CLICK: https://crates.io/settings/tokens is open in bridged Chromium tab 13 and returns 'This page requires authentication' — that profile has no crates.io session and no chromium-bridge state snapshot exists. crates.io token management is session-cookie-gated, so the exposed token cannot revoke itself via API. Unblocks: click 'Log in with GitHub' in tab 13 (OAuth returns to /settings/tokens), then re-queue this item and the agent enumerates and revokes in-session. Blast radius nil: agent-doc has no cargo publish step and no CARGO_REGISTRY_TOKEN in any of its five workflows (book, ci, mdbook, pypi, release).
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
