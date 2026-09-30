---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a0db52-4c0d-72d3-b2c9-0ac89aff42a0
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260930-035410.md`*

Compacted content:
- Prior summary/context: prior compacted content: Archived 14 response topic(s): Fix fpe.md issue; Fix fpe.md issue (paste landed; lint-gate f410.md`*

Compacted content:
- Prior summary/context: prior compacted content: Archived 14 response topic(s): Fix fpe.md issue; Fix fpe.md issue (paste landed; lint-gate f...
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260930-035...

### Re: Compact Exchange needed two tries (+ fpe.md stuck closeout) — opus-5.5 · 2026-09-30T00:15-04:00

> **Queue prompt:** Compact Exchange did not work the first time I called it. It worked the second time though.

**Cause (shared with the fpe.md failure you pasted): the binary and the editor library were installed from two different builds.** `make install` compiled the `agent-doc` binary and the JetBrains native library in two separate cargo runs. Each run stamps a build id from a hash of the source tree. Another session edited the shared tree between the two runs, so at 23:33/23:34 the install produced:

- binary `0.35.419+3d7dff…`
- library `0.35.419+c7a28f…`

After that, the editor and the controller refused every connection to each other. The automatic recovery ("reload the editor's library") reloaded the same mismatched library, so it could never fix anything. The log says so: `withheld reload_library (reload_already_requested_for_listener_build)`.

- **Compact:** the first compact at 03:53:37 was committed to the file, but its write reached **0** editor targets. It only showed up after a forced editor reload a few seconds later. Your second click at 03:54:10 then compacted the *summary itself*: a 663-byte archive and a summary whose only line points to the previous summary. The same pair happened on 09-29 at 00:39/00:40.
- **fpe.md:** the response was held with `editor_owner_without_registered_replica` for the same reason, and would have stayed held until the next install.

**Fixes (agent-doc `1fbb2e710`, pushed; superproject `83e1aa4753`):**
- `make install`, `make install-full` and `self-install` now build the binary and the library in **one** cargo run before installing either, so both get the same build id.
- `lib-install` now **refuses** a library whose build id doesn't match the binary it pairs with, and says how to fix it.
- Compact is now a no-op when the exchange contains nothing but a compact summary (the same handling as an already-empty exchange). No more summary-of-a-summary and no extra archive file. Operator text or a new response before the boundary still compacts.
- The spec is updated (`specs/07-core-commands.md`). New tests cover the build-id check (refuse/accept), the build-id scanner, the Makefile single-build rule, and compacting twice. Each regression test was checked by reintroducing the bug: the Makefile test fails with two builds, and the compact-twice test reproduces the exact nested output.

**Verification:** `make check` exit 0, 10161 tests passed. `make install` gave a matching pair (`0.35.419+ecd0acc…` for both). No build-mismatch rejections since 04:07:22. **fpe.md recovered on its own:** its held response committed at 04:08:41 and `session-check` is OK. The editor library reload was deferred to the next idle boundary because a document was mid-cycle. CI for `1fbb2e710` is still running.

**Leftover in this document:** the second compact's summary-of-a-summary is in your editor, and the disk still has the first compact's summary. I left it alone because your editor text is authoritative. It's harmless: the full archive `…-20260930-035337.md` is one link away.

### Re: Fix https://github.com/btakita/agent-doc/issues/60 (+ fpe.md Stop hook) — opus-5.5 · 2026-09-30T00:37-04:00

> **Queue prompt:** Fix https://github.com/btakita/agent-doc/issues/60

**#60 — after a zero-replica deferral, `session-check` reported a false INTERRUPTED.** `respond` exits nonzero when the editor has no registered replica, but the retained write still commits a few seconds later. The harness is allowed exactly one `session-check`, and that check sampled the state once at entry. So the answer depended on how quickly the harness typed: about 1s after `respond` it said `write_applied`, about 4s after it said `committed`, for the same cycle.

- **Fix (`#scsettlewindow`):** the `session-check` CLI now re-checks every 500ms, for up to 15s, whenever the interruption is one the binary clears on its own:
  - the cycle is still `write_applied`,
  - a retained delivery says it "will resume automatically",
  - a retained write delivery is still unsettled,
  - replica/projection settlement is already scheduled,
  - a captured closeout is still pending.

  It then reports the last state it saw. The window is configurable with `AGENT_DOC_SESSION_CHECK_SETTLE_SECS` (`0` = check once).
  - A real stall is still reported, just after the window.
  - Interruptions that need the agent to act (finalize, `write --commit`, answer a prompt) are reported immediately.
  - The Stop hooks keep their own time budget.
- **Tests:** re-check until committed, report immediately when action is needed, report a stall when the window closes, and a zero window checks once. A pin test ties each trigger phrase to the real message that emits it, so rewording a message fails the build instead of silently disabling the wait. I checked the pin test by breaking a phrase; it failed as expected.
- `respond` still exits nonzero for the deferral, and `session-check` gives the final answer. I commented on #60 (auto-closed by the commit).

**fpe.md (your second paste):** two separate things.
1. **The held closeout** (`editor_projection_pending` at about 04:07) was the tail of the build mismatch from the previous turn. It **committed at 00:08:40** once the matching binary and editor library were installed, and `session-check` is OK.
2. **`Stop hook failed closed: parse stop JSON`** was a real bug (`#stopnullmessage`). Your Codex turn ended on `sleep 60` with no final message, so Codex sent `"last_assistant_message": null`. The hook tolerated a *missing* field but not an explicit `null`, so the whole payload failed to parse. That also meant the hook's existing path for tool-only stops could never run.
   - I reproduced it against the installed binary (`parse stop JSON`).
   - The fix treats `null` as empty for `last_assistant_message` and `stop_hook_active`, with tests. The same payload now returns `{"continue":true}` from the installed binary.

**Verification:** `make check` exit 0, 10168 tests passed. Installed with the binary and editor library on one matching build (`0.35.419+305d8cd…` for both). Committed as agent-doc `91330f9f7` (pushed) and superproject `a25e9686cc`. After the install, one controller still reports the previous build; it recycles at the next idle boundary.

> **Queue prompt:** Fix https://github.com/btakita/agent-doc/issues/61

### Re: GH #61 — component markers in code spans escaped — opus-5.5 · 2026-09-30T00:45-04:00

Fixed in agent-doc `5611dc7bd` (pushed; `Fixes #61` closes the issue).

- **Cause:** `sanitize_component_tags` (`agent-doc-template/src/sanitize.rs`) escaped every agent marker byte-wise with no code awareness.
- **Fix:** it now computes `element::find_code_ranges` — the same pulldown-cmark helper the component parser uses to *ignore* code regions — and leaves markers inside inline code spans and fenced/indented blocks verbatim. Bare-text markers are still escaped, and an unmatched backtick (no span) still escapes. The materialization probe calls the same sanitizer, so writer/probe parity holds.
- **Tests:** new coverage for inline span, backtick + tilde fences, bare-beside-span, unclosed backtick, and a round-trip proving a sanitized code quote parses as no extra component. Two tests that pinned the old escaped-in-code shape were updated. `make check`: 10173 passed, 0 failed. `make install` done; CI run 36670256201 in progress.
- **Known edge:** a response captured by a pre-fix binary (written escaped) and replayed by the new binary would no longer match the materialization probe. That only affects an in-flight capture across the upgrade that quotes a marker in code; I did not add a legacy fallback across the six probe callers.
- **Controller:** it now records, per project, the last editor-surface observation (client, gen/seq, accepted, intent, focused, visible, running count) and the last pane-layout outcome (phase, expected vs actual). It returns them as `surface_sync` on `reliable_sync_status`. `None` means the controller predates this change; it never means "nothing observed".
- **`agent-doc reliable-sync-status`:** prints both records, where each registered/visible document's tmux pane lives (agent-doc / stash / none), and a divergence verdict. The verdict comes from a pure `pane_placement::surface_pane_divergence` fold that flags a visible document parked in stash, a visible document with no pane, or a hidden document holding the agent-doc window. `--json` also gains `pane_placements` and `surface_pane_findings`.
- **Live dogfood caught a false positive**, now fixed: the editor surface spans all projects, so `src/haiven-dev/tasks/fpe.md` was flagged as "no pane" from the agent-loop root. Visible documents under a nested project root are now excluded, with a regression test.
- **Verification:** 16 new tests. `make check` passed: 10186 tests, 0 failed. CI runs 36670256201 (#61) and 36671583167 (#62) were still in progress when I checked.
- **Not done:** promoting the plugin's `observe: published` log from `LOG.debug` to info. Another session has uncommitted JetBrains plugin edits (`CrdtReplicaManager.kt`, `gradle.properties`), and a plugin edit needs a version bump in that shared file. The controller-side receipt now answers the same question without that change.
- Note: `rustfmt src/main.rs` followed the `mod` declarations and reformatted 10 unrelated `src/` files. I reverted them (formatting-only) before committing.
> **Queue prompt:** Fix https://github.com/btakita/agent-doc/issues/62

### Re: GH #62 — visible pane stuck in stash, auto-sync unobservable — opus-5.5 · 2026-09-30T01:03-04:00

Landed agent-doc `9d84795d5` (pushed, `make install` done). This adds the issue's own ask: making auto-sync observable, plus the divergence check. The pane-placement **cause** is still unverified, and the reporter's plugin 0.2.392 skew (#63) confounds it. So #62 stays open; I commented there with the next measurement to take.

- **Controller:** it now records, per project, the last editor-surface observation (client, gen/seq, accepted, intent, focused, visible, running count) and the last pane-layout outcome (phase, expected vs actual). It returns them as `surface_sync` on `reliable_sync_status`. `None` means the controller predates this change; it never means "nothing observed".
- **`agent-doc reliable-sync-status`:** prints both records, where each registered/visible document's tmux pane lives (agent-doc / stash / none), and a divergence verdict. The verdict comes from a pure `pane_placement::surface_pane_divergence` fold that flags a visible document parked in stash, a visible document with no pane, or a hidden document holding the agent-doc window. `--json` also gains `pane_placements` and `surface_pane_findings`.
- **Live dogfood caught a false positive**, now fixed: the editor surface spans all projects, so `src/haiven-dev/tasks/fpe.md` was flagged as "no pane" from the agent-loop root. Visible documents under a nested project root are now excluded, with a regression test.
- **Verification:** 16 new tests. `make check` passed: 10186 tests, 0 failed. CI runs 36670256201 (#61) and 36671583167 (#62) were still in progress when I checked.
- **Not done:** promoting the plugin's `observe: published` log from `LOG.debug` to info. Another session has uncommitted JetBrains plugin edits (`CrdtReplicaManager.kt`, `gradle.properties`), and a plugin edit needs a version bump in that shared file. The controller-side receipt now answers the same question without that change.
- Note: `rustfmt src/main.rs` followed the `mod` declarations and reformatted 10 unrelated `src/` files. I reverted them (formatting-only) before committing.
- **Contents:** GH #61 (code-quoted markers no longer escaped), GH #62 diagnostics (`reliable-sync-status` shows surface auto-sync, pane placement, and a divergence verdict), GH #60 session-check settle window, the Codex Stop `null` message fix, the install build-skew guard, and **#respondsettle** (`52b067500`, below).
- **#respondsettle:** `respond` failed twice this session. Each time I had just run `make install`, which restarted the controller while the response was being saved. The save was refused, and `respond` exited nonzero and fired the dogfood fix prompt, although the retained worker committed the same cycle about 3s later. Now `respond`/`finalize`/`write --commit` watch that same cycle's phase after a failure that happens past capture, and succeed only if it reaches `committed`. A failure before capture still errors at once. 7 unit tests. The first version ran a full session-check and broke `finalize_prewrite_guard_failure_leaves_cycle_open_for_retry`, so it now reads cycle state only.
- **Release files:** version projection across 154 files (verified version-only), a `VERSIONS.md` entry, and the required `editors/release-parity.tsv` row (no plugin behavior change). `make check` (inside `make release`): 10193 passed, 0 failed.
- **Workflows at last check:** Release, PyPI, and CI in progress; Book and Pages green. Another session's uncommitted JetBrains edits are not part of this release.
> **Queue prompt:** release + publish

### Re: release v0.35.420 — opus-5.5 · 2026-09-30T01:47-04:00

Released **agent-doc v0.35.420**: tagged and pushed, with `make install-full` run once and installed (`agent-doc --version` = 0.35.420).

- **Contents:** GH #61 (code-quoted markers no longer escaped), GH #62 diagnostics (`reliable-sync-status` shows surface auto-sync, pane placement, and a divergence verdict), GH #60 session-check settle window, the Codex Stop `null` message fix, the install build-skew guard, and **#respondsettle** (`52b067500`, below).
- **#respondsettle:** `respond` failed twice this session. Each time I had just run `make install`, which restarted the controller while the response was being saved. The save was refused, and `respond` exited nonzero and fired the dogfood fix prompt, although the retained worker committed the same cycle about 3s later. Now `respond`/`finalize`/`write --commit` watch that same cycle's phase after a failure that happens past capture, and succeed only if it reaches `committed`. A failure before capture still errors at once. 7 unit tests. The first version ran a full session-check and broke `finalize_prewrite_guard_failure_leaves_cycle_open_for_retry`, so it now reads cycle state only.
- **Release files:** version projection across 154 files (verified version-only), a `VERSIONS.md` entry, and the required `editors/release-parity.tsv` row (no plugin behavior change). `make check` (inside `make release`): 10193 passed, 0 failed.
- **Workflows at last check:** Release, PyPI, and CI in progress; Book and Pages green. Another session's uncommitted JetBrains edits are not part of this release.

### Re: release re-queued; duplicated responses fixed — opus-5.5 · 2026-09-30T02:05-04:00

Understood: the release now runs **after** the #62–#64 fixes (v0.35.420 shipped before any of them). I see your later queue edits too: #62 struck, "commit the uncommitted files" added, and the tail renamed to "release + publish Issue 62-64". Those run next, in your order.

**Why your exchange has duplicated bullets.** The last two responses (#62 at 01:04 and release at 01:47) each landed with their bullet list **twice**: an orphan copy just before `> **Queue prompt:**`, then the full section.
- You were editing while each response was written, so the editor applied only the bullets (1423 of 1666 bytes). The queue-prompt quote, heading and intro were missing.
- The drift-repair path found no heading, concluded the response was missing, and appended the whole response after the fragment.
- **Fix (`#respsuffixdup`, agent-doc `d75c76157`):** before appending, materialization now checks whether the exchange already ends with a trailing run of the response's own lines. The run must be at least 2 lines and 120+ characters, and shorter than the whole response. If so, it replaces that run instead of adding a second copy. Short shared closing lines like `- Tests pass.` are left alone. Two tests; disabling the fix makes the regression test fail.
- **Your text is untouched:** the orphan bullets from those two earlier responses are still in your document. Delete them if you like; they sit directly above each `> **Queue prompt:**` line.
- **The doubled `release + publish` line** was not caused by the auto-strike: every strike logged `nodes=2`, so both copies already existed. It's filed as **#queuefreetextdup** with the evidence.

> **Queue prompt:** fix https://github.com/btakita/agent-doc/issues/63

**GH #63 — `plugin update jetbrains` failed on a remote-dev backend.** Fixed in agent-doc `93039df74` (`Fixes #63`, with a comment on the issue).
- **JVM:** the upgrader's JVM now comes from the target IDE first: the process executable if it is `java`, else `<ancestor>/jbr/bin/java`, which is the reporter's `<dist>/jbr/bin/java`. Then `JAVA_HOME`, then `PATH`. If none is found, the error lists every path it tried instead of a bare `No such file or directory`.
- **Fallback:** a failed dynamic upgrade (no JVM, attach refused, the `checkCanUnloadWithoutRestart` signature mismatch) no longer fails the update. The plugin files are still replaced, and the command succeeds with a restart-the-IDE warning that gives the reason.
- **`--no-dynamic`** (alias `--restart-required`) on `plugin install|update` skips the hot-swap explicitly.
- **Tests:** four new ones cover the IDE-first JVM order, the list of tried paths, the fallback, and `--no-dynamic` with and without a live IDE. The fallback test fails when the fallback is removed. The spec is updated (`specs/07-core-commands.md`).
- **Not done (#jbunloadsig):** making the plugin's own `checkCanUnloadWithoutRestart` call work across IDE versions needs a plugin release with a version bump in `gradle.properties`, which another session has uncommitted edits in. With the fallback in place it no longer blocks updating.
- `8b8547bc0` **#subsumedsplicereplay** (plugin 0.2.449): after re-registering, the editor sent the whole buffer and then replayed the same queued keystrokes, so typed text landed twice (infra.md). Sending the whole buffer now drops those older keystroke batches.
- `0c3b48408` **#admissionmergedup**: a merge can no longer end up with more list items than the baseline plus each side's additions. This stops two edits of one queue item from becoming two items. Includes a new TLA model, `AdmissionSplitMerge`.
- `09ae52d7f` **plugin/binary version mismatch now asks for operator action**: reloading the same binary and plugin can't clear it, so it is no longer treated as self-healing. The `RefusedSaveOperatorAction` TLA check is updated to match.
- `165ab9638` **#owneddeliverydefer**: the closeout now waits a bounded time for the editor to receive the write instead of failing hard (fpe.md).
- `b89f4d612` **#clearnewchattrailer**: Codex's composer is recognized again after `/clear`.
- `bd6ba2102` **#qstruckbaredup**: when a queue line is struck, an identical plain-text copy of it is dropped instead of being served again.
- `c4c3fd739` rustfmt-only reformatting in 6 files.

**Verification:** `make check` exit 0, 10199/10199 passed. `make install` done. Superproject commits `9518013aa5` and `b555114b5a`. CI run 36675992323 was queued at last check.
> **Queue prompt:** commit the uncommitted files

### Re: committed the pending agent-doc work — opus-5.5 · 2026-09-30T02:18-04:00

> **Queue prompt:**
>
> fix https://github.com/btakita/agent-doc/issues/64


The agent-doc submodule is now clean and pushed. Its uncommitted work was another session's, idle since 00:06. I committed it as seven commits, one per topic:

- `8b8547bc0` **#subsumedsplicereplay** (plugin 0.2.449): after re-registering, the editor sent the whole buffer and then replayed the same queued keystrokes, so typed text landed twice (infra.md). Sending the whole buffer now drops those older keystroke batches.
- `0c3b48408` **#admissionmergedup**: a merge can no longer end up with more list items than the baseline plus each side's additions. This stops two edits of one queue item from becoming two items. Includes a new TLA model, `AdmissionSplitMerge`.
- `09ae52d7f` **plugin/binary version mismatch now asks for operator action**: reloading the same binary and plugin can't clear it, so it is no longer treated as self-healing. The `RefusedSaveOperatorAction` TLA check is updated to match.
- `165ab9638` **#owneddeliverydefer**: the closeout now waits a bounded time for the editor to receive the write instead of failing hard (fpe.md).
- `b89f4d612` **#clearnewchattrailer**: Codex's composer is recognized again after `/clear`.
- `bd6ba2102` **#qstruckbaredup**: when a queue line is struck, an identical plain-text copy of it is dropped instead of being served again.
- `c4c3fd739` rustfmt-only reformatting in 6 files.
- **Cause:** on a fresh `init` project, the first `write --commit` wrote a full-UUID boundary (`agent:boundary:ebf9a71f-9986-…`). The bundled lint dialect rejects that as `malformed-boundary`, so the lint gate blocked the first closeout. Two writers built full UUIDs instead of calling the shared 8-hex id: the post-patch boundary re-insert and compaction's fallback. Both now use the short form.
- **Second bug on the same path:** with a relative path (`scratch.md`), a git probe after the commit ran in an empty directory and failed with ENOENT. An empty directory now means the working directory.
- **Test:** the issue asked for an end-to-end test of `init` → prompt → first `write --commit`. It asserts a commit lands with a short-hex boundary; restoring the UUID writer makes it fail with the issue's exact error.

**Verification:** `make check` passed twice this session with all of this in the tree (10199/10199). The TLA suite passed, including the new model. The Kotlin `CrdtLocalEditorSpliceTest` passed 8/8 on a fresh run. `make install` had already built from this tree, so the installed binary and plugin 0.2.449 already contain these changes.

**Superproject:** pointer `6ad04bd865`. I also committed agent-doc's installed skill/runbook files for v0.35.420 (`6d3a75d2fc`, 23 files). I left the other ~200 uncommitted superproject entries alone. They belong to other sessions and projects (memory notes, other submodules, `.gitmodules`, `.mise.toml`, other task documents, two deletions). Tell me if you want any of those committed too.

**Note:** #qstruckbaredup and #admissionmergedup address the same duplication class as **#queuefreetextdup**. I'll re-check that item against them rather than start a new fix.
> **Queue prompt:** release + publish Issue 62-64

### Re: release v0.35.421 (with GH #64 fixed first) — opus-5.5 · 2026-09-30T03:12-04:00

Released **agent-doc v0.35.421**: tagged, pushed, and installed with `make install-full` (`agent-doc --version` = 0.35.421). Superproject `846c0d91f7`.

**#64 first.** A closeout bug struck #64 in the queue without it being done (fixed as #qselectedheadonly), so I fixed it before releasing. `e5a21d01d`, `Fixes #64`, and the issue is closed.
- **Cause:** on a fresh `init` project, the first `write --commit` wrote a full-UUID boundary (`agent:boundary:ebf9a71f-9986-…`). The bundled lint dialect rejects that as `malformed-boundary`, so the lint gate blocked the first closeout. Two writers built full UUIDs instead of calling the shared 8-hex id: the post-patch boundary re-insert and compaction's fallback. Both now use the short form.
- **Second bug on the same path:** with a relative path (`scratch.md`), a git probe after the commit ran in an empty directory and failed with ENOENT. An empty directory now means the working directory.
- **Test:** the issue asked for an end-to-end test of `init` → prompt → first `write --commit`. It asserts a commit lands with a short-hex boundary; restoring the UUID writer makes it fail with the issue's exact error.

**What's in 0.35.421:** GH #63 (plugin update fallback), GH #64, the duplicated-response repair (tail and middle runs), the queue-quote cell fix that ended the resume spin, #qselectedheadonly, and the committed work from the other session. That work includes plugin 0.2.449 (`#subsumedsplicereplay`), `#admissionmergedup` with its TLA model, and `#owneddeliverydefer`. Also a `VERSIONS.md` entry and a `release-parity.tsv` row. `make check`: 10203/10203 passed.

**#62 stays open.** Its diagnostics shipped in 0.35.420, but the root cause is still unverified. #63 was confounding it: the reporter's plugin was stuck at 0.2.392, and 0.35.421's update fallback can now move it forward. That makes the next measurement on the #62 thread possible.

**Workflows at last check:** Release, PyPI and CI in progress; Book and Pages green.

The exchange is past 160 lines; compacting it would be a good next step.
> **Queue prompt:** do [#editorlistonlyproj]

### Re: #editorlistonlyproj — the editor dropped a response's prose — opus-5.5 · 2026-09-30T10:26-04:00

**Fixed.** agent-doc `dbcb62e57`, superproject `daad6059d8`, installed with `make install`.

**Root cause.** The editor merge was not at fault. The binary's IPC payload told the editor to write only the bullets:
- `build_ipc_node_patches_json` sent item-node patches for every component that has list items. The exchange counts as one whenever a response contains bullets.
- The JetBrains plugin skips a component's full patch once that component has node patches. So the editor wrote only the response's new bullets. Each bullet was placed after the previous bullet, which is the previous response's last bullet. The quote, heading and paragraphs were never sent to the editor.
- The binary then saw the response missing and appended it in full, so the bullets appeared twice (#respsuffixdup).
- Evidence from the 03:12 incident (patch `e097816b`): idea.log shows the plugin's own write going from 29880 to 30691 chars. That 811-char gain matches the three new bullets. Feeding the same document and response through the plugin's normal exchange-append path gives the full +2223.
- The old `…preserves_consecutive_insert_order` test was written for exchange bullets arriving in reverse order. That was the same bug seen from another angle.

**Fix.** A component gets node patches only when the item nodes carry its whole change:
- The exchange is never node-patched, because a response is headings and prose appended at the boundary.
- Any other component is node-patched only if its text outside the list items is unchanged. Component markers are left out of that check, so a queue strike that also flips the tag's `go` attribute still travels as node patches.

**Tests.** Two new regression tests:
- A response with a quote, heading, prose and bullets, plus a concurrent queue strike: the exchange gets no node patches and the strike still does.
- A component whose summary line changes alongside a new item gets no node patches; the same change without the summary edit still does.

Turning the guard off makes both tests fail. The ordering test now uses a backlog component. `make check` passed 10205/10205. CI run 36728882098 was in progress at last check.

**`#queuefreetextdup`, after your note.** Understood: the queue is a FIFO, and a repeated line means run it again. The duplicate was yours, so the bug is how strikes work:
- In that cycle, answering one copy struck both (`nodes=2`).
- `#qstruckbaredup` (`bd6ba2102`, from the previous cycle) drops a plain copy identical to a struck line. That deletes exactly the kind of repeat you scheduled.

I've rewritten the item to cover both. The fix will strike only the first unstruck copy and keep later repeats live.

**Filed at the end of the queue:** `#semmergeargorder`. `try_semantic_merge_convergence` gets its merge roles in the opposite order in production from its own test. It did not cause this incident, but it would put operator text in the agent's slot.

The exchange is past 180 lines. Compaction is left for an explicit opt-in while the queue is draining.
<!-- agent:boundary:c0e00e2e -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#editorlistonlyproj]~~
- do [#stalesupresumedeadlock]
- do [#jbunloadsig]
- do [#queuefreetextdup]: I wrote the second `release + publish` because I wanted another release + publish scheduled after the 3 queue items were complete. I see the queue as a scheduling FIFO.
- release + publish
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#stalesupresumedeadlock] [agent-doc] A stale route-owned supervisor (pid 19503, binary from 2026-09-28) ran captured_finalize resume every ~2s with old code that could not see its own materialized response (#queuequotecell), while its recycle stayed deferred with reason=agent_doc_cycle_open: the open cycle needed the new code, and the new code needed the recycle (2026-09-30 06:17-06:35; broken manually with an empty-stdin write --commit). A needs_operator retry loop on a stale binary should count as a recycle boundary, or the resume should run in the fresh controller.
- [ ] [#jbunloadsig] [agent-doc][jetbrains] GH #63 layer 2: JetBrainsPluginUpgradeAction.java:68 calls DynamicPlugins.checkCanUnloadWithoutRestart(IdeaPluginDescriptorImpl) at compile time; on IU-262.9437.185 that signature does not resolve, so the dynamic upgrade aborts (the file-replacement fallback in 93039df74 now covers it). Look the method up reflectively by name, accept the signatures the supported platform range exposes, and report the build + signatures found on a miss. Needs a plugin version bump in editors/jetbrains/gradle.properties (another session had it dirty 2026-09-30).
- [ ] [#queuefreetextdup] [agent-doc] Operator 2026-09-30: the second 'release + publish' was INTENTIONAL, because the queue is a scheduling FIFO and a repeat means run it again. The real defect is strike scope: in the 01:47 cycle every answered_free_text_strike_applied reported nodes=2, so answering one copy struck both. #qstruckbaredup (bd6ba2102) also drops an identical plain copy of a struck line, which deletes an intentional repeat. Fix: a free-text answer strikes only the first unstruck occurrence and later repeats stay live. Re-check #qstruckbaredup against the authored-multiplicity rule (#qdedup-directive-twin) so it collapses only merge re-emits, never operator-authored repeats. Add regression tests for both.
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02 local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
- [ ] [#semmergeargorder] [agent-doc] try_semantic_merge_convergence(base, candidate, content_ours) forwards to document_cell_merge_scoped(base, ours_agent, theirs_operator). Its production caller (agent-doc-write-converge-io lib.rs ~1133) passes the EDITOR text as candidate, which lands in the agent slot. Its own test (agent-doc-write-runtime-io ipc.rs ~983) passes the AGENT text there, so the two callers disagree. The operator text then becomes 'ours' and the agent response becomes the skeleton. The path is gated on !is_visible_write_proven() and did not run in the #editorlistonlyproj incident. Decide the intended order, fix the caller or the body (the post-checks also mix the roles), and add a regression test with a concurrent operator deletion.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [agent-doc][operator-verify] Run the captured-splice live proof to completion (#activateinstalledjetbrai). BLOCKER RESOLVED 2026-09-29: the canonical advance was refused while the captured edit was unsaved in the editor; #retainedadvanceadmit (54452093d) admits a new cycle when the authority already holds the retained write and only its disk projection is pending (closeout still waits for disk). Recipe re-verified offline: 'scripts/xdotool-live-verify.sh captured-splice --dry-run' now prints every step and exits 0 (42fe78bd2 stopped it dying on the capture-receipt wait). Harness fixes from 095d2af54 stand (XTEST typing, absolute-path focus guard, tmp/live-repro scratch, fresh doc-scoped receipts). REMAINING: the live run injects real keystrokes into the operator's desktop, so it runs only on the operator's go-ahead ('run the captured-splice live proof'). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or 'make pypi-quota-check' exiting non-zero. Rechecked 2026-09-29 21:5x EDT: exit 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 152 releases (0.35.113..0.35.418), ~49,630 further post-cutover releases fit. Nothing to act on or delete. pypi/support#12214 still open, 1 comment (acknowledgement bot), untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check reports (next tranche 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
