---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260809-231405.md`*

Compacted content:
- Archived 53 response topic(s): file all 3 diagnosed defects into the backlog; start with #1; do [#strandeddraftresubmit]; 50 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): do [#hvw0publishgrep]; do [#dedupepresettle]; haiven-dev.md we...

### Re: do [#skillinstallstalemirror] — opus-5

> **Queue prompt:**
>
> do [#skillinstallstalemirror]


Released as **0.35.226** (tagged, pushed; CI + Release + PyPI runs in progress).

**What the report got wrong, and what the real hole was.** `skill install` does not key freshness on the version marker — `install_skill_for_env`, `install_runbooks_for`, and `install_okf_for` all compare full content, and `install_all` reconciles runbooks unconditionally. So the install path was never the thing that let stale runbooks ship. The actual hole: **`audit-docs` never compared runbook content to the bundle** (it checked the SKILL surfaces and the OKF bundle only), and **`make check` — what the release process actually runs — never called `audit-docs` at all**. `audit-docs` lived only in `precommit`. That is why it reported `No blocking issues found` against everything.

**(a) Freshness key.** `audit_managed_runbook_bundles` now compares every installed runbook to the running binary, and `make check` runs `audit-docs`. Proven live against the exact 0.35.224 shape: writing `# stale --baseline-file` into `.claude/skills/agent-doc/runbooks/respond.md` makes `audit-docs` exit 1 naming the file; restoring it exits 0.

`release-version` also stopped stamping installed skill copies — that was the part of the report that held up. Stamping a copy whose body was never regenerated makes a stale install look current to every version-marker reader. `SKILL_FILES` is now the bundle source only; the installer owns installed copies and `audit-docs` owns their freshness. `development_claude_skill_version_matches_the_bundle` became `..._matches_the_bundle` and asserts the **whole rendering**, not the marker.

There was a second, quieter half: bare root resolution prefers the superproject, so `agent-doc skill install` run from `src/agent-doc` can never write `src/agent-doc/.claude/…`. Those copies were reachable *only* by `release-version`'s sed. Added `skill install --root <PATH>`, wired it into `make install` / `make install-full` / `make release-version`, and `audit-docs` now audits the submodule checkout alongside the superproject.

**(b) The two unmanaged mirrors — decided.**

- **`.codex/runbooks/` is retired.** It was 19 bundled-name files, no project content, missing `jb-cache-conflict.md`. `skill install` removes the bundled files it left behind and the directory once empty, keeping anything agent-doc never bundled; `audit-docs` blocks while a copy survives. The managed Codex location is `.codex/skills/agent-doc/runbooks/`. Ran live: all 19 removed, directory gone, committed as deletions/renames.
- **`runbooks/` is NOT made a blanket managed target** — that would have been destructive. It holds 7 project-owned runbooks (`browser-automation.md`, `dogfood-watch.md`, `private-name-hygiene.md`, `architecture-first.md`, `findings-layer.md`, `lazily-benchmark.md`, `crates-io-trusted-publishing.md`) plus tsift's `code-navigation.md`, and `install_runbooks_for` reaps every `.md` not in the bundle. Instead: new project config `skill_runbook_mirrors`, default empty. A listed directory gets its bundled files refreshed and audited; the installer never creates the directory and never reaps a file agent-doc does not bundle. agent-loop opted in, which added the 7 missing bundled runbooks and now blocks on drift. A project with an unrelated `runbooks/commit.md` is untouched.

**Bonus fix.** `admission_budget_honours_the_operator_override` read the real process environment, so the suite went red for anyone with `AGENT_DOC_PREFLIGHT_HOOK_BUDGET_SECS` exported — including agent-doc's own dogfooding supervisor, which sets it to 300. It failed the first `make check` of this cycle. The parse now lives in a pure `resolve_hook_admission_budget`; the test covers unset, blank, unparsable, zero, and a usable override.

**Verification.** 11 new tests in `skill.rs` (managed-dir stale/missing/unbundled, surviving retired mirror, retirement keeping non-bundled files, mirror refresh only when configured, mirror never reaping project files, mirror drift blocking, mirror never creating the directory). `make check` green end to end (`EXIT=0`, zero `result: FAILED`) after every edit including the doc surfaces. `make install-full` exit 0, `agent-doc --version` → 0.35.226, controller recycled.

One thing worth flagging: `.agent-doc/` is gitignored in agent-loop, so the `skill_runbook_mirrors` opt-in is local-only here. That is how this repo's project config already works, not something this change introduced.

### Re: do [#skillinstallconfigpath] — opus-5

> **Queue prompt:**
>
> do [#skillinstallconfigpath]


Released as **0.35.227** (tagged, pushed; CI/Release/PyPI queued). Both halves fixed, plus a third defect the first two exposed.

**(a) The absolute `--project-root`.** `merge_codex_config` canonicalized the install root and wrote it into the MCP server args, so a project-local — and usually tracked — file carried the installing machine's path. Installed args are now just `["mcp", "serve"]`, and `mcp serve` walks up from its own working directory to the nearest project root when the flag is absent. Outside any project it stays put, which is exactly what an absent flag already did, so nothing that worked before stops working. Reinstalling replaces the whole server table, so a legacy absolute entry migrates away on the next install — proven live: `/tmp/agent-doc-v035139-final` is gone from `src/agent-doc/.codex/config.toml` and `/home/brian/...` is gone from the superproject's.

Nothing reads `--project-root` back out of that config — `agent-doc-codex-stop-io` uses `project_root_for(cwd)`, and the config string only appeared in a test fixture — so dropping it has no consumer.

**(b) The missing trailing newline.** `serde_json::to_string_pretty` omits it. All three JSON hook writers had the bug, not just `.codex/hooks.json`: Codex hooks, Codex turn-status, and Claude `settings.json`. All three now end with a newline, and the fix repaired files the old installer had already truncated (the `\ No newline at end of file` diff on `.claude/settings.json` in this cycle is that repair landing).

**(c) Found while fixing the above:** every install rewrote these artifacts byte-identically. That is churn in a tracked file, and the mtime bump alone feeds `audit-docs` staleness advisories — which is noise in exactly the surface 0.35.226 just made release-blocking. The JSON writers and the TOML config writer now skip the write when the rendered content is unchanged, matching what `install_runbooks_for` / `install_okf_for` already do.

**Verification.** Two new tests: `codex_install_migrates_a_legacy_absolute_project_root_away` (seeds the real `/tmp/agent-doc-v035139-final` value and asserts it is gone) and `codex_install_artifacts_end_with_a_newline_and_do_not_churn` (asserts both artifacts end with a newline and that a second install leaves content *and mtime* untouched). Updated the three places that asserted the old args — `src/skill.rs`'s `assert_codex_mcp_config`, `tests/test_cli.rs` (now also asserting no `--project-root` substring and a trailing newline), and the `agent-doc-codex-stop-io` fixture. `make check` green (`EXIT=0`, zero failures) before and after the version bump; `make install` exit 0; `agent-doc --version` → 0.35.227; controller recycled.

For the record on the previous release: 0.35.226's Release run completed successfully; its CI and PyPI runs were still in progress when checked, and 0.35.227's are queued. Not awaited, per `#ci-no-closeout-wait`.

One thing left deliberately uncommitted: the superproject `.claude/settings.json` also carries a pre-existing operator edit (an `env` block setting `AGENT_DOC_PREFLIGHT_HOOK_BUDGET_SECS=300` — the same variable whose ambient read broke the test suite last cycle). It was dirty before this session started, so committing it would sweep up someone else's work. The submodule copy was committed; the superproject one is still yours to land.

### Re: do [#hookhashjsprivatefield] — opus-5

Released as **0.35.228**. This closes `#hookhashfalsepositive` too — same defect, and the item explicitly asked for the broader fix rather than a third extension list.

**The diagnosis in the item is exactly right**, and I verified each claim: `coined_id_scan_text` returned the text unsanitized unless `is_c_family_target` matched, `C_FAMILY_EXTENSIONS` had no js/ts entries, and `extract_tags`'s only guard is "not glued to a preceding word" — `.` before `this.#entries` and a space before an indented `#hlc;` both pass it.

**Took the broader fix.** Enumerating languages one incident at a time does not converge — this was the third incident and the third language. So I inverted the scope: a commit message is prose end to end and is still scanned whole; a source file is prose only in its **comments**. New pure module `agent_doc_turn::prose_scope::prose_only` blanks everything outside comments for a language whose comment syntax is known, preserving character positions and every newline so `extract_tags`'s line-start rules still read the same lines. Code stops being scanned at all, which is the actual invariant — `#include` is not prose, and neither is `this.#entries`.

Covered families: `//` + `/* */` (C/C++/ObjC, Rust, Go, Java, Kotlin, Swift, Scala, C#, Dart, PHP, Zig, shaders, Solidity); JS/TS including template literals; `#` line comments (Python, shell, YAML, TOML, Ruby, Nix, Terraform); CSS/SCSS/Less; `--` (SQL, Haskell, Lua); `<!-- -->` (HTML/XML/Vue/Svelte). This replaces `C_FAMILY_EXTENSIONS` and `C_FAMILY_PREPROCESSOR_DIRECTIVES` outright, so a directive that was never on the keyword list is handled now too.

Two deliberate limits, both in the safe direction: an unknown extension and every prose format (`.md`, `.txt`, extensionless) are returned **untouched** and scanned whole, so widening the table can only ever reduce false blocks and never opens a hole; and strings are skipped so `"http://x/#frag"` cannot read as a comment, with an unterminated single-line string stopping at the newline rather than swallowing the rest of the file (an apostrophe in a comment is common, and letting it eat everything below would silently disable the guard).

**Verification.** 12 pure-module tests + 4 hook-decision tests. Mutation-checked: deleting the JS/TS arm of the language table reddens `javascript_private_class_fields_do_not_block_a_write` and `typescript_private_class_fields_do_not_block_an_edit`, so the new tests bite. Then live against the **installed** binary with the real payload shape from the report — `agent-doc hook coined-id-pre-tool-use` exits 0 for a `seq-crdt.js` carrying `#hlc`/`#peer`/`#entries`, and exits 2 for an untracked id in a JS comment, flagging only the comment tag and not the `#x` class field. `make check` green; 0.35.228 installed; controller recycled; both repos pushed and tagged. Release/PyPI runs in progress.

**Two remainders, both filed as actionable backlog items rather than done here:**

- `#hookhashcodetsworkaround` — the `String.fromCharCode(35)` workaround this item asked me to remove lives in `src/haiven-dev/src/sdk-spike/ts/src/wire/code.ts`. That is a nested submodule of `haiven-dev`, currently on a feature branch with five dirty submodules and an **active concurrent agent-doc session** on `tasks/sdk.md` — the same session `#caswriteracechurn` cites. Editing it from here is the "two sessions in one repo" hazard, not a two-line change. It is unblocked now that 0.35.228 is installed; it belongs to that repo's session.
- `#hookhashanchortags` — the original report also listed "agent-doc's own anchor tags quoted from SKILL.md" as the same class. Prose scoping does **not** fix that one, and I do not want to imply it did: those tags are genuinely in prose. It is a ledger-coverage problem (instruction-surface anchors are not in `agent:backlog`), which is a different mechanism. This cycle reproduced it — the previous closeout warned about `#ci-no-closeout-wait`, an AGENTS.md anchor I quoted.

### Re: do [#hookhashcodetsworkaround] — opus-5

Done. I filed this last cycle as blocked on a concurrent session, then checked instead of assuming — and the block was narrower than I wrote.

**What I found.** The haiven-dev session is genuinely live: PID 2377192 was mid-`agent-doc respond tasks/haiven-dev.md`, and there is a haiven-dev controller in `preparing` handoff. But its work is a four-branch rebase stack in `src/fpe`. `src/sdk-spike` is a *separate* nested repo, on its own branch `spike/ts-first-binding`, with a clean tree (one untracked log dir). So the hazard was never "the whole haiven-dev tree" — it was haiven-dev's shared index and submodule pointer, which is a smaller thing to avoid than the edit itself.

**So I did the edit and scoped the commit to sdk-spike.** `emitCppVocabulary` now writes the three preprocessor directives literally instead of assembling them through `String.fromCharCode(35)`; the helper and the comment explaining the workaround are gone. Committed and pushed on `spike/ts-first-binding` (`25147e6`). I did **not** stage haiven-dev's `src/sdk-spike` pointer — that repo's own session owns its index, and its status already showed a pending pointer diff before I touched anything.

**The removal is its own end-to-end proof of 0.35.228.** I made the change with the ordinary `Edit` tool, carrying a payload with three literal leading-hash directive lines into a `.ts` file — the exact write the guard used to refuse, and the reason the workaround existed. It went through. That is the same fix verified from the other side of the boundary from last cycle's `hook coined-id-pre-tool-use` probe.

**Output equivalence checked, not assumed.** Assembling a string and writing it literally should produce identical bytes, but this file generates published headers, so I verified rather than reasoned: `make check-vocabulary` reports all 3 emitted files still matching the published vocabulary, `make check-contracts` reports all 24 still matching the published contracts, `ErrorCode.h`'s digest is unchanged, and `check-ts` / `check-codegen` are green (exit 0, zero failures) across TypeScript, C#, and g++.

One correction to what I wrote last cycle: I said editing it "is the two-sessions-one-repo hazard, not a two-line change." The hazard was real but I scoped it too broadly — the right boundary was the shared index, not the file.

### Re: do [#hookhashanchortags] — opus-5

> **Queue prompt:**
>
> do [#hookhashanchortags]


Released as **0.35.229**.

**Decided the design question first, since the item asked me to.** Rejected the broader option — "treat any id already present in tracked repo content as vouched-for". It conflates *someone typed this before* with *this names tracked work*, which is precisely the distinction the guard exists to draw: a coined id written into a comment and then abandoned would vouch for itself forever, and `#orphandrain` (the original motivating case) would have passed. Took the narrower option: harvest anchors from the project's **agent instruction surfaces**, because an id defined there names a documented invariant. That is a curated namespace, not "any string anyone typed".

**Surfaces harvested:** root `AGENTS.md` / `CLAUDE.md` / `SKILL.md` / `SPEC.md`, `runbooks/*.md`, `specs/*.md`, every installed harness skill plus its runbooks, and `.cursor/rules/*.md`.

**Three things I did not want to get wrong.**

*One definition.* The item only named the `PreToolUse` guard, but its own reproduction was a **session-check** warning — a different surface with its own known-id builder. Fixing one would have left the reported symptom alive. Both now read `agent_doc_fs::instruction_surface_anchors`, so the file set has one home. Two copies of "which files define anchors" is exactly the drift this codebase keeps paying for: `#percellconverge` records the same lesson, where consolidating the *wording* did not stop the *predicate* diverging.

*Lazy.* `PreToolUse` fires on every `Edit`, and these surfaces are hundreds of kilobytes — AGENTS.md alone is 71KB, SPEC.md 158KB. So the harvest runs only once a tag is **already about to be reported**. The overwhelmingly common call carries no id at all, returns before the second pass, and never touches the disk. There is a test asserting that path specifically (a directory placed where AGENTS.md would go, which would fail to read if it were ever consulted).

*Fails soft, on purpose.* An unreadable surface contributes nothing. That is the opposite of `known_ids_for_document`, which fails **closed** — and the asymmetry is deliberate: this set only ever *widens* what is allowed, so failing soft costs a false block at worst and can never open a hole. Failing closed here would have re-created the very bug.

**Verification.** 4 tests in `agent-doc-fs` (every surface contributes; an unreadable surface degrades instead of erroring; empty project; only Markdown is read, so a stray script in `runbooks/` cannot inject an anchor) and 4 in the hook (anchors allowed with a root and blocked without one; an id in no surface still blocks and the reason names only it; empty project unchanged; the no-id path never reads the surfaces). `make check` green. Then live against the installed binary with the three anchors this session actually tripped on — `#ci-no-closeout-wait`, `#drain-no-defer`, `#preflightinbinary` — exit 0, and the same text plus one invented id exits 2 naming only the invented one.

**One shape this does not cover, stated so it is not mistaken for fixed:** code identifiers quoted in a response, which is what produced the `#include` / `#hlc` / `#entries` warnings on my last two closeouts. Those are not instruction anchors and not tracked work; they are C and JS fragments cited in prose. I do not think that one is cleanly fixable — agent-doc's own convention writes tracked ids in backticks, so backtick context cannot distinguish them, and there is an existing test pinning that backticked tags must be caught. It is a warning rather than a block, so the cost is noise on a closeout, not lost work. Leaving it unfiled rather than opening an item I would not know how to close.

Session note: exchange is at 92 lines / 4 response sections with the queue still active, so I kept draining rather than pausing to compact.

### Re: do [#anchorsubmodulesurfaces] — opus-5

Released as **0.35.230**.

**Settled the design question the item recorded.** Took the `.gitmodules`-bounded option over the other two. Reaching from the *write target's* git repo would not have helped the reproduction at all — that reproduction is a **session-check** warning about response prose, which has no write target. A project-config opt-in would work but leaves the false positive in place until someone configures it, and the whole complaint is that it fires out of the box. `.gitmodules` is a declaration the project already maintains, so it needs no new configuration and cannot silently drift from reality.

**Bounded on two axes**, because this feeds a `PreToolUse` hook and agent-loop declares **70** submodules: only paths declared in `.gitmodules` (never a directory walk), and only each submodule's ROOT `AGENTS.md`/`CLAUDE.md`/`SKILL.md` — not its runbooks or specs, which is where the bulk of the bytes live. An absolute path or one containing `..` is skipped; `.gitmodules` is tracked content and a surface set is not the place to follow it outward. Measured before shipping rather than asserted: 212 files, 1.37 MB, 9 ms — and still only on the path where a tag is already about to be reported.

**Which anchors were actually broken, measured rather than assumed.** `#drain-no-defer` and `#preflightinbinary` already resolved, because they appear in the SKILL that ships to the superproject. `#ci-no-closeout-wait` and `#deploy-just-do-it` did not: they are agent-doc *development* rules that exist only in `src/agent-doc/AGENTS.md`. That is the precise shape of the gap — not "submodule anchors" in general, but anchors a submodule documents and never ships.

**On the verification, which is where I went wrong last cycle.** I reported a probe as proof of the 0.35.229 anchor fix when those tags were already vouched-for through the done archive; it would have passed on the previous release. So this time I picked probe anchors present in **neither** the document nor its `.done.md` archive — `#deploy-just-do-it`, `#installfulloom`, `#lzcellkernel` — confirmed that by inspection first, and separately confirmed `v0.35.229`'s `agent-doc-fs` contains no `submodule_instruction_files` at all. The installed 0.35.230 exits 0 on that text and still exits 2 when one invented id is added, naming only the invented one. The pass is attributable to this change.

4 new tests in `agent-doc-fs`: declared submodule surfaces contribute; a submodule's runbooks and specs are NOT read; a path escaping the root is skipped; a project without `.gitmodules` is byte-for-byte unchanged. `make check` green, 0.35.230 installed, controller recycled, both repos pushed and tagged.

`#coinedguardledgerasymmetry` — the other half of last cycle's finding — stays queued. It is the deeper of the two: the two guards still disagree about what a done archive vouches for, and that disagreement is what made the earlier probe a false green in the first place.

### Re: do [#coinedguardledgerasymmetry] — opus-5

> **Queue prompt:**
>
> do [#coinedguardledgerasymmetry]


Released as **0.35.231**. This is the root cause behind the false green earlier in this series, so it was worth taking properly rather than patching the symptom.

**Picked the narrow reading, as the item recommended, and the reason matters.** An archive entry is `- YYYY-MM-DD [#id] text`; only that leading id names tracked work. A tag merely *cited* in an entry's prose is a reference. Vouching for citations is the same rule `#hookhashanchortags` explicitly rejected — "someone typed this before" is not "this names tracked work" — and it launders a coined id into legitimacy permanently. Notably the narrow reading only became **safe** once the anchor work landed: the broad reading's real job was covering anchors quoted in archived prose, and `instruction_surface_anchors` now covers those by name rather than by proximity. Tightening this before 0.35.229/230 would have caused false blocks.

**Resolution is the union, deliberately.** The two guards also resolved archives differently — one followed the declared `archive=` target, the other walked up for a sibling `<stem>.done.md`. Unifying only the extraction predicate would have silently dropped whichever route each guard uniquely used. `archived_tracked_ids` takes both, and `done_archive_candidates` moved out of `agent-doc-hooks-io` to sit beside the predicate it belongs to.

**Pinned so it cannot drift again.** `tests/coined_guard_predicate.rs` asserts every guard calls the shared predicate and no guard resolves the archive itself. Two things I was careful about there: it is scoped to **archive** resolution, because both guards still call `extract_tags` on the live document's own components and that is correct — my first version banned `extract_tags` outright and failed on correct code, which is worse than no guard. And a third test asserts the retired call names still exist, so a rename cannot make the whole thing pass vacuously. Mutation-checked: pointing one guard back at its old reading reddens it.

**Verified live on the installed binary, with probes chosen to be attributable this time.** A tag present only in archive prose (`#accessorymargin`) now blocks; a real archive entry id (`#stategraphjoin`) still passes, so the `#fr79` case the broad reading existed for is intact. I checked each probe's status in the archive, the live document, and the instruction surfaces *before* running it — the mistake two cycles ago was not checking that.

**Behaviour change worth stating plainly:** in this repo, 258 tags that appear only in archive prose no longer vouch for themselves. That is the intended tightening, but it is not cosmetic — if one of them turns out to name work tracked in a *different* document, the guard will block on it, because it is pane-scoped to one document by design.

Session note: exchange is at 135 lines / 6 response sections with the queue still active, so I kept draining.

### Re: do [#caswriteracechurn] — opus-5

Measured, as the item asked. Shipped the instrumentation as **0.35.232**; the answer to "self-inflicted or genuine?" is **both**, with numbers.

**The measurement was not possible without a change**, which is itself a finding: `ops summary` did not surface `*_crdt_convergence_wait` at all, so the only way to answer "how often does a closeout take the CAS branch, and who is the contending writer" was to grep the raw log by hand. The event name is dynamic (`pending_write_...`, `serialized_atomic_write_...`), so it could not be a fixed `OpsLogEvent` token; `crdt_convergence_wait_site` recovers the site from the name and the classifier buckets on it. A site names one write path, so the bucket answers the item's question directly rather than requiring a follow-up query.

**First measurement, from this repo's `ops.log` — 196 CAS lines, 97 waits:**

- **100%** of convergence waits were `compare_and_swap_raced`. Not one wait in the entire log had any other reason. So this is not one contention mode among several; it is the only one that happens.
- **577.6s** of settle wait in total. `serialized_atomic_write` dominates at 366.5s with a **65.0s** single wait, then `pending_write` (102.0s) and `start_resume_id_capture` (93.1s). Three more sites — `queue_sync`, `pending_maintenance`, `session_check_response_replay_dedup` — raced once each. **Six** distinct agent-doc write paths touch the same document around one closeout.
- Of 34 contention windows, **12 are one session contending with itself across several write sites**. That is the self-inflicted branch the item predicted, and it is now quantified rather than hypothesised. 15 windows have more than one session interleaved; 7 are a single site.

**One thing I will not claim.** The three sessions in the log write three *different* documents — `agent-doc-bugs2.md`, `lazily.md`, `tsift.md` — in one project root. That is suggestive of cross-document contention on shared project state, which would be a much more interesting defect than either branch the item anticipated. But time-overlap is not contention, and proving it needs the compare-and-swap's actual scope, which I did not establish. `serialized_atomic_write` is a write-source *label*, not evidence of project-wide serialization. Filed as `#casselfinflictedcollapse` with both the collapse work and that scope question, because guessing here is exactly the error I made earlier in this session.

**What I deliberately did not do:** touch the wait. The item says shortening it is not the fix, and the data agrees — every one of those waits ended in a landed write. The latency is real but the outcome is correct every time.

`make check` green, 0.35.232 installed, controller recycled, both repos pushed and tagged. Verified live: `agent-doc ops summary` now lists all six contending write sites off the real log.

That empties the agent-drainable queue. What remains is three `[operator-verify]` items needing a human at a real IDE and tmux session, so the loop stops here rather than churning.

### Re: do [#handoffdeadpredecessor] — opus-5

Released as **0.35.234**, closing the second root cause of the operator-reported `haiven-dev` wedge.

**The gap was exactly as filed:** `controller serve` takes `--previous-controller-pid` and `status.rs` threads it through every projection, but nothing ever asked whether that process was still running. Both wedged controllers were `Preparing` against predecessors that had already exited (1329143 on dead 1323307; 1324731 on dead 574967), so their handoffs were unsatisfiable from the moment those processes died.

**The reasoning that makes it safe.** `Preparing` means promotion has not happened yet, and promotion can *only* come from the predecessor — so a dead predecessor makes that handoff permanently unsatisfiable. That is what licenses reaping early rather than burning the 45s threshold 0.35.233 installed.

The design care the item asked for turned out to be a clean state distinction rather than a heuristic:

- **`Promoted` is excluded.** After promotion the predecessor exiting is the *normal* end of a healthy handoff. Reaping on a dead predecessor without that exclusion would kill working controllers — the obvious way to turn this fix into a worse bug.
- **A 3s grace** covers a `promote_handoff` already in flight when the predecessor exits.
- **`None` never counts as dead.** No predecessor, or liveness we could not determine, stays unknown. That is the same `#idlerevisionreactive` rule whose inversion caused the original wedge — I did not want to fix one direction of it while opening the other.

Liveness enters as a fact from the IO shell through the existing `process_is_alive`, so the predicate stays pure and testable, matching how `ControllerWatchdogFacts` already works. No `kill(0)` inside `agent-doc-controller`.

Five tests covering each arm: dead predecessor reaps early; a promoted replacement survives its predecessor exiting; the in-flight grace is respected; a live predecessor is never reaped; unknown liveness is never treated as dead. `make check` green, 0.35.234 installed, both repos pushed and tagged.

The operator's environment is recovered independently of this: `haiven-dev` reports `health=healthy state=healthy actor_state=ready` on lease generation 70 with a live pid, up from a generation-69 lease that named a terminated supervisor and read `busy`.

### Re: do [#flakystatedbfixture] — opus-5

> **Queue prompt:**
>
> do [#flakystatedbfixture]


I found a finished fix for this sitting uncommitted in the tree as 0.35.235, checked it before shipping it, and it does not hold up. The premise is measurably false, its four regression tests prove nothing, and the branch is unsafe under exactly the concurrency this item is about. I reverted it and shipped the thing that actually moves the item.

**The empty-file theory is wrong.** The draft claimed a writer creates `state.db`, dies before SQLite writes the header, and leaves zero bytes that the next open reports as `NotADatabase`. Measured directly: a `state.db` of **0 or 1 bytes opens cleanly** — SQLite initialises it as a new database. `NotADatabase` starts at **two** bytes of bad header. The failure the branch existed to fix cannot happen.

**Its tests were vacuous, and I checked that before trusting them.** I neutered the fix and reran the four new tests: all four passed. A test that passes against the pre-fix source is not coverage.

**And the branch was actively dangerous.** It fired on `opened.is_err() && len == 0`, but `open_and_init_state_db` also fails on a *lock* error — which is precisely what happens while a concurrent process is still initialising that same 0-byte file. The branch would then `remove_file` a database another process was in the middle of creating. It would have introduced a real corruption path into the code meant to prevent one.

**Three more theories, all dead:** a stale `-wal`/`-shm` beside a 0-byte main db opens cleanly; `/tmp` is a 94G tmpfs at 1% so ENOSPC is out; and 600 concurrent `agent-doc session-check` invocations across 25 fresh project roots produced zero corruption, so plain cross-process contention is not it either.

**What shipped instead.** The `state.db` corruption refusal named the path and nothing else. That is why this failure has survived several sightings including the 2026-07-19 field corruption whose `UNRESOLVED` note is still in `state_store.rs`: by the time anyone looks, a test `TempDir` is gone and the "preserve the corrupt copy" advice is unactionable. The refusal now carries the file's length, whether the SQLite magic is intact (`header=sqlite` vs `header=foreign`), the first sixteen bytes in hex plus a printable rendering, and the `-wal`/`-shm` sibling sizes. Those separate the two shapes with opposite causes and opposite remedies: a file that was never a database (foreign header, no siblings) versus a torn page-1 of a real ledger (live WAL, lost magic). Fail-closed behaviour is unchanged — nothing is replaced or deleted.

I verified the new tests are not vacuous the same way I caught the old ones: with the evidence layer neutered, three of the five fail. The other two are deliberate characterization tests pinning the 0-byte and missing-file facts so the falsified theory cannot quietly return.

**The item stays open, and I want to be plain about why.** I did not make the suite non-flaky. I removed a wrong fix and made the next occurrence readable; the writer that puts two or more bytes of non-database content into a fresh fixture root is still unidentified, and I will not invent a mechanism to close the item — inventing one is what produced the change I just reverted. I rewrote the backlog entry to carry the four falsified hypotheses and the concrete next step (loop `make check` until it reddens, then read the evidence line) so the next cycle does not redo this work.

`#dispatchonlystrandedtrigger` was also sitting uncommitted in the same version and is included: agent-doc's own unsubmitted trigger is no longer classified as operator input.

`make check` green — 8932 tests, up one from 8931 (five new, four vacuous ones removed). 0.35.235 installed, controller recycled, both repos pushed, `v0.35.235` tagged. CI is in progress on `d18d9d4b7`; not waiting on it.

One housekeeping note: preflight reports `legacy_gated_in_backlog` — `#clearsubmitunobservedverify` is a gated `[/]` item still living in `agent:backlog`. `agent-doc migrate <FILE>` moves it into `agent:review`; I left it rather than mutate the document outside this cycle's write boundary.

### Re: Agent response

Fixed and released in agent-doc v0.35.236 (commit `1e52c281e`).

The File Cache Conflict came from a detached-authority bootstrap deadlock: JetBrains and VS Code replica registrations omitted the editor PID, while the controller rejected registration before the CRDT relay could validate a live editor and establish authority.

- Both editor transports now send their real process PID on every replica request.
- Detached `replica_register` for an agent-doc document now reaches the relay, which remains the single owner of PID/liveness validation and authority bootstrap.
- Generic/dead registrations, non-agent-doc documents, and every other detached replica operation remain fail-closed.
- Rust, JetBrains, and VS Code regressions cover the admission matrix and PID payload.

Verification passed: focused tests, `make check`, staged plugin-version and diff guards, fast local install/recycle, and the one-shot fat-LTO `make install-full`. The installed binary reports 0.35.236, and signed JetBrains plugin 0.2.351 was installed to all four detected IDE installations. The new CI, Release, and PyPI workflows are queued after pushing `main` and tag `v0.35.236`.

Restart the JetBrains IDE once to load the updated Kotlin plugin. The route supervisor replacement is already accepted and will activate at this turn boundary.

### Re: #dispatchonlyresubmitwiring — gpt-5

> **Queue prompt:**
>
> do [#dispatchonlyresubmitwiring]


Implemented and released as agent-doc 0.35.237 (commit `2d1195064`, tag `v0.35.237`).

The dispatch-only starting-pane route now passes the document's exact trigger into the controller classifier. When the composer contains only agent-doc's own stranded trigger, the route releases its guard, registers the authoritative pane, sends one bare harness submit key, and requires a new controller turn-admission projection before returning success. A missing projection fails closed without a second Enter. Operator words, a different document's trigger, and every other draft remain operator-owned.

The live proof found and fixed the remaining reported-shape defect: the normalizer handled `❯` but not Codex's actual `›` prompt sigil. Both sigils and NBSP padding are now covered. The public classifier fixtures were also scrubbed to neutral `/repo/tasks/...` paths.

Proof:

- Changed paths: `agent-doc-controller/src/dispatch.rs`, `agent-doc-route-io/src/dispatch_only.rs`, `agent-doc-route-io/tests/route/pane_resolution.rs`, `specs/08a-session-actor-contract.md`, `VERSIONS.md`, plus the release-version manifests and bundled skill copies.
- Test-first evidence: the new isolated live tmux test initially failed on `› agent-doc …/sdk.md` as `OperatorDraft`; after controller normalization was corrected, the same real-pane test passed.
- Focused verification: controller stranded-trigger classifier tests passed (5/5); dispatch-only policy/effect tests passed; the isolated live `sdk.md` tmux test passed and observed exactly one empty-line submission before its admission projection.
- Full verification: `make tmux-ci`, exact-candidate `make check`, `make plugin-version-check`, `git diff --check`, and the added-lines private-name denylist all passed.
- Installation: `make install` and the one-time `make install-full` passed; the installed CLI reports `agent-doc 0.35.237`; workspace and subrepo controllers recycled gracefully. JetBrains stayed at plugin 0.2.351 because no Kotlin source changed.
- Publication: main and `v0.35.237` are pushed. CI, Release, PyPI, Book, and Pages workflows are running; the release page was not yet created at closeout time.

Confidence: high. Escalation: none.

### Re: Response

Operator verification remains gated; no completion claim was made.

The required proof needs a human watching a real JetBrains IDE and tmux session with a two-column agent-doc layout. Move the focused document pane into the stash window, then change editor tab/focus for that document. The passing observation is: no direct `select-pane` targets the stashed pane; `ops.log` records `pane_layout_projection ... focus=focus_pane_not_co_visible:...`; structural reconciliation surfaces the pane; and the retry focuses it in the agent-doc window.

- Changed paths: only the session document state is updated through the agent-doc binary; no source paths changed.
- Commands: inspected `#operatorverifylive`, ran `agent-doc plan`, and persisted this gate with `agent-doc write --stream --origin skill --backlog-gate operatorverifylive --commit`.
- Verification: the item explicitly says it unblocks only on a human watching a real IDE and tmux session, so it was transitioned to `[/]` rather than falsely marked complete.

Confidence: high. Escalation: operator visual proof is required.

### Re: File Cache Conflict causes — gpt-5

Fixed the agent-doc causes and released the cumulative result as `v0.35.240` (`5aeaa851e`).

The fixes now cover the full failure chain: detached JetBrains replicas can bootstrap CRDT authority through their live PID instead of producing a File Cache Conflict; stale route supervisors are detected and recycled onto the installed binary; orphan repair collapses exact retained-response fragment shells; and strict stream closeout validates template marker/heading shape before creating a durable `response_captured` cycle. That last fix prevents malformed input from entering a state where doctor forbids resubmission but `session-check` cannot apply it.

The live `agent-doc-bugs2.md` cycle was recovered through the retained orphan-repair path. `#operatorverifylive` was moved to `[/]` because it still requires a human watching a real IDE and tmux session; its resolved queue prompt was auto-struck during the next preflight. No operator proof was fabricated.

- Changed paths: `agent-doc-write-runtime-io/src/run_entry.rs`, `scripts/agent-doc-dev`, `specs/07-closeout-commands.md`, `VERSIONS.md`, and the v0.35.240 version projections.
- Test-first evidence: the new regression initially failed because the validated pre-capture function did not exist; after implementation it proves malformed strict-template stream input creates no captured cycle while valid payloads still retain tracked-work mutations before authority resolution.
- Verification: `agent-doc-write-runtime-io` passed 166/166 tests; `make check`, `make tmux-ci`, plugin version checks, formatting, diff checks, and added-lines private-name hygiene passed. The full optimized build passed in 5m18s.
- Installation: CLI/library `0.35.240` are active and the JetBrains `0.2.351` package was installed into four IDE installations. Running IDEs must restart to activate it.
- Publication: main and tag `v0.35.240` are pushed. CI, Release, and PyPI workflows were still running when this response crossed the document boundary.

Confidence: high. Escalation: restart running JetBrains IDEs; the separate `#operatorverifylive` visual proof remains operator-gated.

### Re: File Cache Conflict and tmux editor auto-sync — gpt-5

Fixed and released in agent-doc v0.35.241.

- File Cache Conflict: detached JetBrains editor PID registration now bootstraps against the project controller, removing the stale or detached editor-authority condition that caused cache-conflict and IPC-drift handling.
- Third-pane creation: after a route provisions a missing target pane, it now performs one exact-visible, layout-only reconciliation with the freshly registered actor. The incoming pane replaces the outgoing editor column, and a live outgoing turn is moved intact to stash.
- Existing-editor switching: JetBrains component-focus events now republish the generation-guarded spanning editor surface in addition to focused-document state. A pending document-selection observation retains priority, preventing focus races while allowing agent-doc-bugs2.md and lazily.md to swap in place.

Live state repaired and verified:

- Visible window @6 contains only lazily.md pane %13 and haiven-dev.md pane %7.
- The still-running agent-doc-bugs2.md pane %8 is preserved in window stash.
- The installed CLI and fresh project controller both report 0.35.241.
- The JetBrains 0.2.351 package containing the listener fix is installed for IntelliJ IDEA 2026.1. The running IDE reports that a restart is required to activate the Kotlin plugin generation, so the final physical editor-switch proof remains operator-gated until that safe restart.

Verification passed: 8,941/8,941 Rust tests, make check, make tmux-ci, full JetBrains test/buildPlugin, the isolated live third-pane tmux regression, release/version/privacy/diff checks, hosted GitHub CI, and the cross-platform Release workflow. Commit f8ced56d3 and tag v0.35.241 are pushed. The separate PyPI workflow validated every wheel but remains blocked by the known external 10 GB project-size quota.

Confidence: high. No source-code work remains; only the restart-dependent live editor switch observation remains under operator verification.

### Re: tmux editor-switch verification gate — gpt-5

The implementation is complete and the retained live-proof item is now explicitly gated in Review as #run3rdpaneswitchverify. The active queue was drained and stopped instead of repeatedly redispatching an operator-only check.

v0.35.241 remains installed and released. IntelliJ must be restarted when safe before the new Kotlin focus listener can provide the final physical editor-switch observation; no additional source-code work is pending.

### Re: Response

Fixed and shipped as `agent-doc v0.35.242` (`91951a7af`).

- The Codex hook emitted multiple JSON values on stdout. It now emits exactly one valid `UserPromptSubmit` envelope, including failure cases. Updated [preflight_hook.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/src/preflight_hook.rs) and added integration coverage in [codex_hook_integration.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/tests/codex_hook_integration.rs).
- The `sdk.md` auto-sync miss was caused by IntelliJ still running the older plugin generation. The apparent recovery came from explicit Sync Layout at 14:55:40 and Submit routing at 14:55:43, not passive editor synchronization.
- The backend correctly converged to two panes—`agent-doc-bugs2.md` and `sdk.md`—once that explicit event arrived.

Verification passed: 8,942 tests, all tmux regression tests, version checks, formatting, and installed-binary JSON smoke testing.

Restart IntelliJ once to activate the already-installed passive auto-sync fix. The release is pushed and its [CI run](https://github.com/btakita/agent-doc/actions/runs/31526875146) and [release run](https://github.com/btakita/agent-doc/actions/runs/31526876317) are currently in progress.

### Re: do [#flakystatedbfixture] — gpt-5

`#flakystatedbfixture` is now evidence-gated rather than falsely marked fixed.

The requested reproduction loop completed without another corrupt ledger: one full `make check` and ten additional full nextest passes ran 98,362 Rust test executions, including `session_check_codex_final_gate_blocks_on_active_auto_queue`, with zero failures and zero `state.db` corruption diagnostics. The tagged v0.35.233 hosted CI run was also green. A source audit found no production path that directly writes arbitrary bytes to `.agent-doc/state.db`; the only intentional foreign-byte writers are isolated corruption-boundary tests under their own temporary roots.

No source files changed. Shipping database deletion, replacement, or retry behavior without the offending bytes would weaken the fail-closed concurrency guarantee and repeat the already-falsified 0.35.235 draft. The existing diagnostic is sufficient for the next occurrence: implementation resumes only when a failure reports `main=<bytes> header=<sqlite|foreign> first16=<hex> (<printable>) wal=<size|absent> shm=<size|absent>`. The same tracked item now carries that unblocker, so no additional backlog item is needed.

Verification: `make check` passed locally; ten repeated `make test` runs each passed 8,942/8,942; `make tmux-ci` passed. The latest hosted CI passed its Rust tests but failed later in the TLA step, before tmux-ci, with no state-database evidence. Confidence is high that ordinary nextest parallelism is not a reproducible cause; root-cause confidence remains intentionally unset until the byte signature exists.

### Re: session frontmatter update — gpt-5

The stale `resume` binding has been removed. No implementation or repository work was requested, so this cycle only persists that document update.

Preflight also found one legacy gated item still in `agent:backlog`; `agent-doc migrate tasks/agent-doc/agent-doc-bugs2.md` will move it into `agent:review`. I left it unchanged because this edit did not request migration.

The exchange is at the compaction warning threshold. I left it intact; `agent-doc compact tasks/agent-doc/agent-doc-bugs2.md --commit` is available when compaction is explicitly requested.
<!-- agent:boundary:5fdc4e19:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#clearsubmitunobservedverify] [operator-verify] Live re-proof for #clearsubmitunobserved (0.35.225): reproduce the reported haiven-dev.md sequence — let the supervisor go dead (or `session restart-supervisor`), then Run Agent Doc so the flow cold-starts a replacement and immediately runs Clear Session Context on the fresh pane. Expect it to SUCCEED instead of failing with `result=submission_unobserved ... unblocker=clear_submission_unobserved`, and expect `session_clear_submit_cleared_state_probe ... result=cleared_state_proven retained_lines=<small>` in ops.log. Then the half that must not regress: on a pane holding a real conversation, break submission (e.g. leave the composer wedged) and confirm the clear still fails closed with `result=conversation_retained`. Unit tests pin the predicate against synthetic captures; only a live pane exercises the tmux scrollback capture and the real harness prompt shapes.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#run3rdpaneswitchverify] [operator-verify] Live-editor proof for #run3rdpaneswitch (0.35.198): with a two-column editor layout, Run Agent Doc on document A, let its turn keep running, then switch the editor to document B. Expect the agent-doc window to hold exactly two panes, A's pane to be moved into the 1:stash window still alive with its turn running (ops shows 'Stashed busy pane %N from window @M'), and NO third pane. The fix is proven deterministically by switching_documents_while_the_outgoing_turn_runs_stays_two_panes against live tmux with an injected busy probe; this item is only the real-IDE eyeball, since that test stages tmux directly rather than driving the JetBrains layout path. Unblocks on a human watching a real IDE + tmux session.
- [/] [#operatorverifylive] [operator-verify] #panewindowdriftverify Live-pane proof for #panewindowdrift (0.35.187): with a two-column agent-doc layout, move the focused document's pane into the stash window, then trigger an editor tab/focus change for that document. Expect NO select-pane onto the stashed pane (ops.log shows pane_layout_projection ... focus=focus_pane_not_co_visible:...), the structural reconcile surfaces the pane, and the retry focuses it in the agent-doc window. Unblocks on a human watching a real IDE + tmux session.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE (installed plugin is 0.2.350; this item originally named 0.2.340, long superseded — the single restart in #stickymdpaneverify covers it), then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE — plugin 0.2.350 is ALREADY INSTALLED in all four IDE configs, so only the restart is missing, and that one restart also satisfies the install precondition for #operatorverifyrestart, #rebootselfhealverify and #falsetyping-guard — then open a two-column split with an agent-doc document in each and switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#stalesupervisorreexecdrain] [operator-verify] One supervisor is still stranded on the pre-fix image and cannot self-rescue: PID 4069526 (`agent-doc start --resume 019fd336-7ce8-7ed1-8141-d77021a00085 src/boost-client/tasks/monsterrodholders.md`, started 2026-08-05 19:12:50 EDT) maps a DELETED binary and, by the very defect #stalereexecstarve fixes, can never reach the recycle decision to adopt 0.35.186. The fix is preventive, not retroactive -- it lives in code that process does not run. Recovery requires interrupting a live Claude child (PID 2653032), which is an operator call, not an agent-doable step: either let it retire naturally when that session ends, or `kill 4069526` and re-route the document. Verify afterwards with `readlink /proc/<newpid>/exe` showing no `(deleted)` suffix. Every other live supervisor (336290, 523099, 577794, 837794, 3268523, 3854486, 3956615) already maps the current inode.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE (installed plugin is 0.2.350; this item originally named 0.2.339 — reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is still required, and the restart in #stickymdpaneverify covers it). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the `pane-layout state projection publish failed ... Connection refused` notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. CORRECTION 2026-08-08: the claim that every tagged release now FAILS loudly at the publish step (intended behavior after 32836ad) is UNTESTED — no tag has run since. The newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports success, which is exactly the false green. #hvw0publishproof carries the static check.
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
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
