---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 689773f9-f4f9-4f01-9e40-63a7fb025c1f
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
queue: go
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
<!-- agent:boundary:8c608826 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#anchorsubmodulesurfaces]~~
- do [#coinedguardledgerasymmetry]
- ~~do [#hookhashcodetsworkaround]~~
- ~~do [#hookhashanchortags]~~
- ~~do [#hookhashjsprivatefield]~~
- do [#operatorverifylive]
- do [#run3rdpaneswitchverify]
- ~~do [#hookhashfalsepositive]~~
- do [#caswriteracechurn]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#coinedguardledgerasymmetry] #agent-doc-bug The two coined-id guards disagree about what the done archive vouches for, and neither is documented as authoritative. The PreToolUse guard (known_ids_for_document, agent-doc-hooks-io) runs coined_ids::extract_tags over the WHOLE archive text, so any tag ever mentioned in archived prose is vouched-for. The post-commit session-check guard (known_ids_for_coined_guard -> external_done_archive_ids, agent-doc-element-backlog-io) runs extract_pending_ids_from_text, which only matches the leading id of an archive entry. Same document, same archive, different answers: this is why #ci-no-closeout-wait passed the hook while session-check reported it. The hook side is also too broad -- it is close to the vouch-for-anything-ever-typed rule #hookhashanchortags explicitly rejected, because a coined id abandoned in archived prose vouches for itself forever. Pick ONE predicate, put it in one place like instruction_surface_anchors, and make the narrow item-id reading the default. See #percellconverge: consolidating the wording while the predicate drifts is the failure mode.
- [/] [#clearsubmitunobservedverify] [operator-verify] Live re-proof for #clearsubmitunobserved (0.35.225): reproduce the reported haiven-dev.md sequence — let the supervisor go dead (or `session restart-supervisor`), then Run Agent Doc so the flow cold-starts a replacement and immediately runs Clear Session Context on the fresh pane. Expect it to SUCCEED instead of failing with `result=submission_unobserved ... unblocker=clear_submission_unobserved`, and expect `session_clear_submit_cleared_state_probe ... result=cleared_state_proven retained_lines=<small>` in ops.log. Then the half that must not regress: on a pane holding a real conversation, break submission (e.g. leave the composer wedged) and confirm the clear still fails closed with `result=conversation_retained`. Unit tests pin the predicate against synthetic captures; only a live pane exercises the tmux scrollback capture and the real harness prompt shapes.
- [ ] [#caswriteracechurn] #agent-doc-bug The `compare_and_swap_raced` settle-wait is recurring across concurrent sessions, not a one-off. Operator-reported on sdk.md 2026-08-09 ("Same CAS-race path as last cycle — the write landed; finishing with commit"), and the SAME cycle on agent-doc-bugs2.md hit it twice in one closeout: `[write] Waiting for typing/CRDT versions to settle ... (reason=compare_and_swap_raced, elapsed=9945ms)` then again at 4694ms, inside a 32.7s closeout (commit_authority 14.9s, session_check 16.7s). Outcome is correct every time — the write lands and the cycle commits — so this is latency/churn, not loss. Worth measuring before fixing: instrument how often a closeout takes the CAS-race branch and what the contending writer is (concurrent supervisor idle-watch? replica propagation? another session on the same state.db?). If the contention is self-inflicted (agent-doc writing the same cell twice per closeout), the fix is to collapse the writes; if it is genuine cross-session contention on a shared project root, the settle wait is doing its job and the item is a metric, not a bug. Do not "fix" it by shortening the wait -- the wait is what makes the write land.
- [ ] [#run3rdpaneswitchverify] [operator-verify] Live-editor proof for #run3rdpaneswitch (0.35.198): with a two-column editor layout, Run Agent Doc on document A, let its turn keep running, then switch the editor to document B. Expect the agent-doc window to hold exactly two panes, A's pane to be moved into the 1:stash window still alive with its turn running (ops shows 'Stashed busy pane %N from window @M'), and NO third pane. The fix is proven deterministically by switching_documents_while_the_outgoing_turn_runs_stays_two_panes against live tmux with an injected busy probe; this item is only the real-IDE eyeball, since that test stages tmux directly rather than driving the JetBrains layout path. Unblocks on a human watching a real IDE + tmux session.
- [ ] [#operatorverifylive] [operator-verify] #panewindowdriftverify Live-pane proof for #panewindowdrift (0.35.187): with a two-column agent-doc layout, move the focused document's pane into the stash window, then trigger an editor tab/focus change for that document. Expect NO select-pane onto the stashed pane (ops.log shows pane_layout_projection ... focus=focus_pane_not_co_visible:...), the structural reconcile surfaces the pane, and the retry focuses it in the agent-doc window. Unblocks on a human watching a real IDE + tmux session.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
