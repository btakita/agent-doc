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
  '#do-backlog': '/goal do backlog items'
  '#gh-fix': 'fix then close'
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
> **Queue prompt:** do [#stalesupresumedeadlock]

### Re: #stalesupresumedeadlock — a stale supervisor waiting on itself — opus-5.5 · 2026-09-30T10:42-04:00

**Fixed.** agent-doc `d08e3e7fd`, superproject `26780458b1`, installed with `make install`.

**Why it deadlocked.** The retry that finishes a captured response runs inside the supervisor process. An old supervisor therefore judges the retry with old code. Between 06:17 and 06:35 UTC the retry stopped with `needs_operator` roughly every 6s. The reason was a "heading already committed" check that the current build handles. Meanwhile the recycle was deferred with `reason=agent_doc_cycle_open`. An open cycle may only be crossed when something proves the response is safely captured. The recycle policy (`supervisor_recycle_action`) had exactly two such proofs: a proven editor write wedge and a stale-editor-delivery request. A captured response whose retry is stuck was neither. The same supervisor (pid 19503) also produced the 2-byte editor/controller mismatch at the start of the last closeout.

**Fix.**
- A new pure helper, `stale_generation_capture_resume_needs_recycle`, is true when a recycle is already due (old binary or a pending recycle request) and the in-process retry has stopped at `needs_operator`.
- The idle watch passes it into the policy's captured-response slot, renamed from `editor_delivery_stale` to `capture_backed_refresh`. That slot already lets a write wedge cross an open cycle.
- The recycle still waits until no supervisor request is in flight, and the response stays durably captured throughout.
- The new process starts without the `needs_operator` stop and without the recycle request, so this fires at most once per episode.
- If the fresh build also stops at `needs_operator`, it stays the operator's call.
- The idle watch now logs `supervisor_stale_capture_resume_recycle`. `specs/supervisor.md` now lists the three cases that may cross an open cycle.

**Tests.**
- The incident shape now recycles immediately. Without the fix the same inputs defer on the open cycle. A request still in flight still defers.
- The helper is true only when both facts hold. Making it always return false fails the test.
- A source guard in `idle_watch.rs` keeps the wiring from being edited away.

`make check` passed 10207/10207. CI run 36731096456 was in progress at last check; run 36728882098 for the previous fix was still in progress too.

**Note:** this install triggers a recycle request, so pid 19503 should pick up the new build at its next safe checkpoint. That is also the first real test of this fix.

The exchange is past 210 lines. Compaction is left for an explicit opt-in while the queue drains.

### Re: #jbunloadsig + route focus, running-task indicator, Stop-hook fail-closed — opus-5-5 · 2026-09-30T11:36-04:00

> **Queue prompt:**
>
> fix https://github.com/btakita/agent-doc/issues/65


> **Queue prompt:**
>
> do [#jbunloadsig]

Five fixes landed in two agent-doc commits (`d769cf5e3`, `a829b5ae5`): installed, pushed, and `make check` green (Rust 10215/0, JetBrains 599/0, VS Code 181/0). One more bug you reported is not fixed; it is now `#fpereplaydup`, top of the backlog.

**#jbunloadsig — JetBrains dynamic upgrade across IDE versions.** The upgrade now looks up `checkCanUnloadWithoutRestart` and `unloadPlugin` by name when it runs, instead of binding them at compile time. It uses whichever version of each method accepts the live plugin descriptor; failing that, it calls Kotlin's `$default` helper with every optional parameter defaulted. If nothing matches, the error names the IDE build and every signature it found. The plugin was bumped to 0.2.451. The install hot-swapped the running IDE without a restart, using the new lookup.

**Editor route refused: "focused document does not match routed document" (api.md).** The editor sent the selected tab (`fpe.md`) as the focus while you ran `tasks/api.md`, and api.md wasn't in either visible column. Now the controller makes the routed document the focus: it moves focus to it if it is visible, or puts it in the selected document's slot if not. The refusal remains only when there's no slot to use.

**Running indicator not visible.** The controller showed an open cycle as idle whenever the tmux pane read `ready` rather than `busy`. That pane status flips back and forth while the agent works, so this document showed "awaiting response" for one frame and then idle for the rest of the turn. Now the cycle's own state decides; only a closed pane ends a turn.

**One-line view of the running task.** The turn state now carries the task (the first prompt target, else the turn id). The JetBrains banner and the VS Code status bar show `⟳ agent-doc: awaiting response · do [#jbunloadsig]`, truncated to 60 characters, with the full text in the tooltip. VS Code extension is now 0.2.75.

**Stop hook "failed closed" on fpe.md.** When the hook's repair step failed, the error went straight to the hook's "failed closed" handler. Here the response write was only being held for the editor, and it committed 29 s later. The hook now treats this as a cycle still in progress: it blocks with instructions based on whether anything is still holding the write, and "failed closed" is kept for real hook failures.

**Not fixed: fpe.md duplicate response and two lost queue items (`#fpereplaydup`).**
- **What happened:** the editor briefly had no registered copy of the document, so the response write was held. The recovery path then wrote the same response a second time (haiven-dev commit `6735e15e`).
- **The damage:** the second copy lists your two newer queue items ("Configure the SBX + STG to have 16 vCPUs…" and "I can't login.") as the prompts it answers, and both are gone from the queue. Neither was actually answered.
- **Why it's still open:** the logs show about 9 repeated recovery writes, a baseline refresh, and then a full rewrite of the response, but I haven't pinned which step re-read the current queue.
- **Your document:** fpe.md is live (editor open, owned by the Codex pane), so I did not edit it. It still needs the two queue items put back and the `(HEAD)` duplicate removed through the editor.

The exchange here is long (warn level), but the queue is active, so I'm continuing rather than pausing to compact. CI for `a829b5ae5` is still running.

### Re: release + publish 0.35.422 — opus-5-5 · 2026-09-30T12:08-04:00

> **Queue prompt:**
>
> release + publish in a subagent so I can test it on my coder project

A subagent released **agent-doc 0.35.422**: release commit `95bfe6ada` on main, with tag `v0.35.422` pushed to origin. The tag started the GitHub **Release** and **PyPI** workflows, both still running when I checked at 12:08. No macOS files were uploaded.

It includes today's fixes: the JetBrains upgrade signature lookup (`#jbunloadsig`), the editor route focus retarget, the running-task indicator with its one-line task label, and the Stop hook no longer reporting "failed closed" when a write is only held.

To test on your coder project once the PyPI job is green:

```bash
uv tool install agent-doc==0.35.422   # or: pip install agent-doc==0.35.422
```

PyPI now ships a small bootstrap wheel. On first run it downloads and checks the matching GitHub Release archive, so the Release job must finish too.

Continuing the queue: `#queuefreetextdup` is next. The exchange is large, but the queue is active, so I'm not stopping to compact.

### Re: #semmergeargorder + lazily.md Stop hook — opus-5-5 · 2026-09-30T12:24-04:00

> **Queue prompt:**
>
> do [#semmergeargorder]

**Fixed in `fd711bef4`: the semantic merge had the operator and the agent swapped.** Installed and pushed; `make check` passes (Rust 10216/0, JetBrains 599/0, VS Code 181/0).

- **The bug:** `try_semantic_merge_convergence(base, candidate, content_ours)` sent the live editor text into the merge's agent slot and the agent's document into the operator slot. The only production caller passes the editor buffer as `candidate`, so every same-line conflict went to the agent.
- **The fix:** the merge now gets (base, agent, operator). New response headings are read from the agent's document. The check on text outside components now requires the agent to leave it alone and requires the merge to keep the operator's version. Before, it refused any operator frontmatter edit.
- **Tests:** three existing guard tests were written to match the bug: they passed the operator text as `content_ours`. They now use the roles production uses.
- **Regression:** the agent strikes a queue line while the operator deletes it, and the deletion must win. I checked the test catches the bug: with the old argument order, the agent's strike puts the deleted line back and the test fails. My first version of the test missed this, because edits to different lines merge the same way whichever side is which.

**lazily.md "Stop hook failed closed" (11:20).** This is the same failure as fpe.md, already fixed by `a829b5ae5`, which was installed at 11:34, after this happened. The Stop hook reopened a cycle, saved the response, and its write was held; the error escaped as "failed closed". The held write settled 6 s later and committed as `53b068e086`. lazily.md passes session-check now, with only an operator-verify item waiting in its queue. Nothing more to do there.

CI is still running on the 0.35.422 release commit. The exchange here is long, but the queue is active, so I'm continuing.

### Re: #queuefreetextdup — opus-5-5 · 2026-09-30T12:36-04:00

> **Queue prompt:**
>
> do [#queuefreetextdup]: I wrote the second `release + publish` because I wanted another release + publish scheduled after the 3 queue items were complete. I see the queue as a scheduling FIFO.

**Fixed in `bc6cb52ef`: a repeated queue line now means "run it again"**, as you intended. Installed and pushed; `make check` passes (Rust 10218/0, JetBrains 599/0, VS Code 181/0).

Two separate paths were erasing your second `release + publish`:

1. **The strike hit every copy.** When a response answered a free-text queue line, every identical line in the queue was struck (`nodes=2` in the 01:47 cycle). Now one answer strikes only the first unstruck copy; later copies stay for their own cycle. With the old behaviour the new test fails.
2. **Cleanup then deleted the repeat.** After the first copy's strike was saved, any live copy of a struck line was treated as a stale replay and removed. So even with the strike fixed, your repeat would have vanished on the next save. My first test of this failed exactly there. Now, if the saved document itself had a live copy next to the struck one, that copy is yours and stays. A replayed copy with no such saved repeat is still removed, and an old version that un-strikes the first copy is struck again; the test covers both.

**`#qstruckbaredup` checked, no change needed.** It only removes a bare line with no leading `- ` that matches a struck item. An authored `- …` repeat is never affected, and the new test pins that down.

Next in the queue: `#fpecapturedresponse`, the fpe.md duplicated response and lost queue items. CI is still running on `fd711bef4`. The exchange is long, but the queue is active, so I'm continuing.

### Re: #fpecapturedresponse (fpe.md duplicate + lost queue items) — opus-5-5 · 2026-09-30T12:58-04:00

> **Queue prompt:**
>
> In the frontmatter, alias `presets` to `prompt_presets`.


> **Queue prompt:**
>
> do [#fpecapturedresponse]

**Root cause found and fixed in `eb3d09af1`.** Installed and pushed; `make check` passes (Rust 10221/0, JetBrains 599/0, VS Code 181/0).

What happened on fpe.md, from the state ledger and ops log:

1. **15:26:** Codex answered your question once. That write consumed `do [#awsapproval]` and inserted its quote above the answer, which became copy one. The editor had no registered copy of the document at that moment, so the write was **held** until it could be delivered.
2. **15:31:36:** while that held write was still pending, a **repair** looked at the live document, which did not show copy one yet. It decided the response was missing and wrote it again from scratch.
3. That second write's `#awsapproval` was already struck, so its queue step used a fallback that consumes the first free-text items **by count**. By then the first items were your newer ones: "Configure the SBX + STG to have 16 vCPUs…" and "I can't login." Both were marked answered and quoted above a second, identical copy of the answer.

Codex made only one `respond` call; the second copy came entirely from the binary.

**The fixes:**
- **Repair no longer re-sends a response that a held write already carries.** It waits for the held write to be delivered instead. `--force-disk` is unaffected.
- **The fallback now needs proof.** When a replay's own items were already struck, it may consume only the queue items its saved response actually quotes. Ordinary closeouts keep their current behaviour: my first version applied the rule everywhere and broke two finalize tests, so I narrowed it to this case.
- For each fix, I disabled it and its new test failed. I also added a test showing the "already present" check tolerates the inserted quote; it already did, which ruled out that theory.

**Still needed in fpe.md itself.** It is a live document owned by the Codex pane, so I didn't touch it. Through the editor:
- Put "Configure the SBX + STG to have 16 vCPUs for the FPE instance. Update the haiven-infra costs." and "I can't login." back in the queue.
- Remove the second `### Re: what a Haiven admin can do about AWS GPU access (HEAD)` section.

Next in the queue: the `presets` → `prompt_presets` frontmatter alias, then `release + publish`. CI is still running on `bc6cb52ef`.

### Re: GH #65 — `[crdt-perf]` at WARN — opus-5-5 · 2026-09-30T13:10-04:00

> **Queue prompt:**
>
> fix https://github.com/btakita/agent-doc/issues/65

**Fixed in `76a44fe34` (JetBrains plugin 0.2.452). GH #65 is closed.** Installed, hot-swapped into the running IDE, and pushed; `make check` passes (Rust 10221/0, JetBrains 601/0, VS Code 181/0).

Both `[crdt-perf]` log helpers already logged at WARN only above a threshold, but the thresholds were 50 or 100 ms. Normal background CRDT work takes 118–395 ms in the issue's own numbers, so nearly every keystroke batch landed in `idea.log` at WARN.

- **Background work now warns only at 1 s or more.** On the UI thread the existing tight thresholds (10–100 ms) still apply, since any stall there is visible to you. In the reporter's session this would have produced no warnings.
- **The full per-delta timing is still there at DEBUG.** Turn it on with `Help > Diagnostic Tools > Debug Log Settings` → `#com.github.btakita.agentdoc`. That's the opt-in the issue asked for, with no new setting.
- **A new test pins the rule:** the issue's measured latencies stay quiet off the UI thread, and a real stall still warns.

The install noted that 2 open haiven-dev documents hadn't re-registered with the new plugin yet; the plugin retries each one itself.

Next: GH #66, then #67, then `release + publish`.

### Re: GH #66 — `queue: go` flip suppressed its own drain — opus-5-5 · 2026-09-30T13:22-04:00

> **Queue prompt:**
>
> fix https://github.com/btakita/agent-doc/issues/66

**Fixed in `41773c0b6`. GH #66 is closed.** Installed and pushed; `make check` passes (Rust 10223/0, JetBrains 601/0, VS Code 181/0).

- **The bug:** the filter that separates bookkeeping from real prompts only knew `queue_active:`. Your `queue: stop` → `queue: go` flip and the `prompt_presets:` map in the same edit counted as a fresh prompt, which stopped the drain that flip was meant to start.
- **The fix:** instead of adding a fourth spelling to that list, any change that lies entirely inside the document's frontmatter no longer counts as a prompt. Frontmatter is settings and binary bookkeeping, never an instruction, so this covers `queue:`, `prompt_presets:` and any future key.
- **The mislabel:** the diff summary also listed `exchange` as changed for any prompt-like edit, wherever it was, which is why the guidance called a frontmatter edit an "exchange prompt". Such an edit is now listed under `frontmatter`.
- **Tests:** one replays the issue's exact edit and fails if the fix is removed; it also checks that a real exchange prompt in the same edit still counts. A second covers the component label.

Next: GH #67, then `release + publish`.

### Re: GH #67 — `plugin_bytes_superseded` advice that can never converge — opus-5-5 · 2026-09-30T13:36-04:00

> **Queue prompt:**
>
> fix https://github.com/btakita/agent-doc/issues/67

**Fixed in `3ee81039b` (JetBrains plugin 0.2.453). GH #67 is closed.** Installed (hot-swapped into the running IDE here) and pushed; `make check` passes (Rust 10224/0, JetBrains 602/0, VS Code 181/0).

1. **The advice now remembers a refusal.**
   - When an install has to fall back to replacing files because the running IDE refused the no-restart upgrade, it writes the reason to `.agent-doc-jetbrains-restart-required` in the plugins directory.
   - The file sits next to the plugin folder, not inside it. Inside, the "already current" check would count it as a changed file and every install would look new.
   - Any later install that succeeds without a restart deletes it.
   - If that record is newer than the running IDE process, the `plugin_bytes_superseded` warning now says the files are current and to **restart the IDE**, instead of telling you to install again.
2. **The reason prints once.** The fallback line no longer repeats the error; the final restart message carries it.
3. **`plugin cannot unload dynamically: false` was my regression from `#jbunloadsig` this morning.**
   - Newer IDE builds report whether the plugin *can* unload as `true`/`false`; older builds return a text reason or nothing. My lookup treated any non-empty answer as a refusal and printed the raw `false`.
   - It now accepts both shapes. `false` becomes a proper sentence, and a `true` (can unload) is no longer refused.
   - I couldn't check which way the reporter's build (IU-263) means its answer, since only 252/261 builds are installed here. It reported `false`, so on that IDE a restart is genuinely required, and now it will be told so.

Tests cover: the marker being written, not breaking the "already current" check, and being cleared; the warning choosing restart only when a refusal is recorded; and the upgrade step reading both answer shapes.

Next and last in the queue: `release + publish`.

### Re: GH #62 still open — opus-5-5 · 2026-09-30T13:47-04:00

> **Queue prompt:**
>
> https://github.com/btakita/agent-doc/issues/62 is still open on github. Please close it if it is done. If it's not yet done, fix it then close the issue.

**Mostly done already; I finished the remaining piece and closed GH #62.**

What the issue asked for, and where it stands:
- **See whether auto-sync is working without turning on debug logging.** Done in `9d84795d5`. `agent-doc reliable-sync-status` shows the editor's last reported layout and the last pane-layout attempt, and lists which tmux window holds each document's pane (agent-doc / stash / none).
- **A check for the exact mismatch.** The same command already reported it (an open document's pane parked in stash or missing, or a hidden document in the agent-doc window), but only as printed text. The issue wanted a check that *fails*, so `e033224bc` adds `reliable-sync-status --check`, which exits non-zero on a mismatch, with or without `--json`.
- **What triggered it:** the plugin was stuck at 0.2.392 because upgrades were failing (#63), which is fixed. On the live layout here, `--check` exits 0 and reports no mismatch.

Installed and pushed; `make check` passes (Rust 10225/0, JetBrains 602/0, VS Code 181/0).

Last in the queue: `release + publish`.

### Re: release + publish 0.35.423 — opus-5-5 · 2026-09-30T14:10-04:00

> **Queue prompt:**
>
> release + publish

**agent-doc 0.35.423 is released.** Release commit `e1c4bd611` is on main, and tag `v0.35.423` is pushed. `make check` passed (Rust 10225/0, and the editor-parity check accepts 0.35.423). `make release` exited 0, and `agent-doc --version` now reports 0.35.423 locally. The tag started the GitHub **Release** and **PyPI** workflows; both were still running at 14:10. No macOS files were uploaded.

What's in it (all fixed today):
- **fpe.md duplicate and lost queue items:** repair no longer writes a response again while an earlier write of it is still waiting for the editor. A replayed write can't mark newer queue items answered.
- **Repeated queue lines:** an identical line queued twice runs twice.
- **Merge sides:** the semantic merge had operator and agent swapped.
- **Frontmatter edits aren't prompts (GH #66):** flipping `queue: stop` → `queue: go` no longer stops the drain it starts.
- **Quieter logs (GH #65):** routine background CRDT timing logs at DEBUG instead of WARN.
- **Restart advice (GH #67):** when the IDE refuses the no-restart upgrade, preflight now says to restart instead of reinstalling every cycle.
- **`reliable-sync-status --check` (GH #62):** exits non-zero when tmux panes don't match the editor.
- JetBrains plugin 0.2.453.

Install on another project once the PyPI job is green: `uv tool install agent-doc==0.35.423` (or `pip install agent-doc==0.35.423`).

The queue is now empty.

### Re: Did you publish the release? — opus-5-5 · 2026-09-30T14:32-04:00

> **Queue prompt:**
>
> do [#publishrelease]

**Yes, 0.35.423 is fully published.** When I wrote the last response the two publish jobs were still running; both have since finished successfully:

- **GitHub Release** ([v0.35.423](https://github.com/btakita/agent-doc/releases/tag/v0.35.423)): Linux x86_64 (gnu and musl), Linux aarch64 and Windows archives, `SHA256SUMS`, the JetBrains plugin `agent-doc-jetbrains-0.2.453.zip`, the VS Code extension `agent-doc-0.2.75.vsix`, and `EDITOR-PACKAGES.sha256`. No macOS files, as usual for an on-demand release.
- **PyPI:** the job passed, and the PyPI index now lists 0.35.423.

On your coder project: `uv tool install agent-doc==0.35.423` (or `pip install agent-doc==0.35.423`). On first run it downloads the matching Linux or Windows archive from the Release above and checks it against `SHA256SUMS`.

### Re: #gh-fix agent-doc#68 — opus-5-5 · 2026-09-30T15:03-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/68

Fixed and closed [#68](https://github.com/btakita/agent-doc/issues/68) (`c1dfad9ee`, pushed; submodule pointer `8c667de111`). All four findings:

1. **`plan` false `No changes` blocker (`#plansealedturn`).** When preflight has already sealed the turn (open `PreflightStarted` cycle, nothing captured), `plan` no longer reports the blocker. It recovers the free-text queue head preflight selected, and otherwise returns `task_class: preflight_sealed` with no blocker. SKILL.md step 0d and `runbooks/planning-dispatch.md` are updated to match.
2. **backlog+review split (`#gatecollapse`).** I could not reproduce the gate itself leaving the id in both lists: plain and queue-mirrored gates both move it cleanly. So the fix makes the split self-repairing. `--backlog-gate <id>` on an id already in `agent:review` drops the stale backlog copy and does not add a second review entry. The collision warning now names that flag for this shape instead of "rename the preset".
3. **Success receipts from rolled-back writes (`#txreceipt`).** Receipts such as `review-resolve: archived …` and `completed and reaped …` are now held in the tracked-work transaction and printed only after the writes publish. A dry run or an aborted write drops them.
4. **`snapshot is missing` on every queue consume (`#snapmissingquiet`).** This case is expected (transaction planning passes no snapshot on purpose), so it now goes to ops.log only. Real snapshot-sync failures still print. The regression test fails when the old stderr line is put back.

Each fix has a regression test. `make check` passes: all 10231 tests, audit-docs (after `make install` refreshed the superproject skill copies), editor-parity, lean and tla. The build is installed. CI for the push is still running; the previous run was green.

Not fixed here: #67 was already handled in 0.35.423 (`3ee81039b`). The #60 recurrences cannot be diagnosed until the IDE process is restarted, because its loaded plugin is older than the installed one.

Exchange is large (465 lines); continuing the drain.
<!-- agent:boundary:baee6279 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/68~~
- release + publish
<!-- /agent:queue -->


<!-- agent:backlog -->
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02 local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
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
