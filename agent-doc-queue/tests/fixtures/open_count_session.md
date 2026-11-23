---
agent_doc_session: open-count-fixture
agent_doc_format: template
---

## Status <!-- agent:status patch=replace -->
<!-- /agent:status -->

## Active <!-- agent:active priority -->
<!-- /agent:active -->

## Run <!-- agent:run id="main" subagents go -->
- ### [#prompt-blocker-b0ddfbae2f94]: Use chromium-bridge to perform the steps.
  - 🙋 follow up on an action item from queue item “#justdoit: do what you need to do to unblock this”: operator: close the stray LightEdit window in your running IntelliJ showing an empty `dump-launch-parameters` tab, without saving. My first launch attempt didn't isolate the profile directories (the remote-dev launcher ignores `IDEA_PROPERTIES`), so two launcher commands went to your running IDE. The first was refused harmlessly; the second opened that window at 17:55:59. No file was created on disk. [operator-verify] [#q-a269716fffc9]
    - ~~closed~~
- ### ⛔ ⟦blocked by prompt-blocker-b0ddfbae2f94⟧ do [#jbfrontendswap]: approved
- ### ⛔ do [#ghrecvpolicy]: #advance whatever is actionable.
- ### 🙋 [from:recovery key=q-0e29d5fdf1c4-restore] follow up on an action item from `jbdocdrift`: operator: Decide whether to retain separate 262 and 263 modular ZIPs or authorize the combined `262` through `263.*` ZIP. [operator-verify] [#q-0e29d5fdf1c4]
  - 🙋 follow up on an action item from queue item “I authorize the combined `262` through `263.*` ZIP. ⟦published⟧”: operator: restart IntelliJ IDEA 2026.3 and Rider 2026.2 after that install. The modular plugin can't be swapped while the IDE is running. [operator-verify] [#q-56fa509fdf08]
    - ~~Restarted~~
  - 🚧 [#subagent:cizip#1] follow up on an action item from queue item “I authorize the combined `262` through `263.*` ZIP. ⟦published⟧”: agent: check CI run 45085206369 on `3b3561a1b` once it finishes. The new `verifyPlugin` step downloads IntelliJ 263 and the plugin verifier on the CI runner. [#q-f2ec555891ae]
- ### 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”: operator: in the live `tasks/agent-doc/sample-bugs.md` queue, delete the extra struck copies of `running [#subagent:effectvoid#1] amend core-v1` (there are 3) and any doubled `~~Should we integrate these?~~`; these are real duplicate rows, so the keyed merge still correctly declines on those cuts and uses the fallback merge until cleaned up. [operator-verify] [#q-e107b04c1cd5]
    - **State:** Waiting on operator input.
    - **Guidance:** 🙋 3/5 done, 2 yours. Only you can finish this: 2 items for you, listed under Remaining. Strike or answer each row when done.
    - 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'complete the work for this tree ⟦pu…”: operator: decide whether to refresh the VS Code package (`make install` skipped it because `vsce` is not installed; fix with `npm install` in `editors/vscode` or a global `@vscode/vsce`, then `make install-editor-plugins`). [operator-verify] [#q-cb3ac1276fd3]
    - 🙋 follow up on an action item from queue item “complete the work for this tree ⟦published⟧”: operator: apply edits 1–3 above in the editor if you want the old titles cleaned (cosmetic; they no longer collide), then strike the "decide whether to clean those up by hand" and "delete the extra struck copies" rows. [operator-verify] [#q-dade5157fe53]
- ### 🚧 [#subagent:stalltally#1] `    - **Guidance:** 🙋 3/5 done, 2 yours. Only you can finish this: 2 items for you, listed under Remaining. Strike or answer each row when done.` should have been updated after the 2 remaining items were completed.
- ### 🚧 [#subagent:barestrike#1] `        - ~~#justdoit~~` under `      - 🙋 operator: apply edits 1–3 above in the editor if you want the old titles cleaned <!-- agent-doc:response-detail -->` should have had a response structure subtree.
- ### 🚧 [#subagent:nestbug#1] The subitem `            - ~~Present the password prompt and complete.~~` has incorrect nesting from it's parent `      - **Closed by:** [#r-8eb6516470b1]`.
- ### 👀 Subitems were struck but had no guidance: `        - ~~Do the next actions.~~` + `          - ~~What is the status of the parent `        - ~~Do the next actions.~~`? It is struck but there's no response structure.~~` with the parent `      - Coordinator installs and closes the item. No follow-up heads besides the CI check. <!-- agent-doc:response-detail -->`.
- ### 🙋 follow up on an action item from queue item “Is the previous tree `- ### follow up on an action item from queue item 'follow …”: operator: in the editor, delete the block from ` - **Response [#r-8eb6516470b1]**` (line 126) through the `- 🙋 follow up on … #q-8834c5a41276` row (line 174). The row text says the copy sits under `~~Complete the work. ⟦published⟧~~`. That row no longer exists. The copy now hangs under the root response's `**Guidance:**` line. The original is safe in `sample-bugs.exchange-archive.md:27080`. Then strike or delete `#q-6608c8ce85b2`. Once the block is deleted, `#q-8834c5a41276` goes with it. [operator-verify]
- ### 🙋 follow up on an action item from queue item “[from:chat key=pypitrust2] finish sample-py PyPI trusted publishing now that the…”: operator: decide whether to revoke the PyPI API token named **`publish 2026-11-13`** (scope **All projects**, created Nov 13, 2026, last used Nov 14, 2026). It is the only API token on the account. Local `make publish` still uses it through passage `btak/PYPI_PUBLISH_TOKEN`, and because it covers all projects, revoking it also cuts off local publishing for your other PyPI projects. It was left active. [operator-verify] [#q-797479c6911e]
- ### 🚧 [#subagent:sampledevhook#1] [from:chat key=sampledevhook] In a subagent: fix agent-doc issues in sample-dev.md: Stop hook timed out after 5.0s waiting for the sample-app project controller (busy with background maintenance, 'another owner is applying maintenance' for minutes) and session-check left pending self-heal resume_retained_pending_only_commit. #spec-test-build-install-commit-push
- ### 🙋 follow up on an action item from queue item “[from:chat key=tmuxsync] tmux pane auto-sync not working (operator report, 2026-…”: operator: restart IntelliJ after that install. This 2026.3 build cannot swap the plugin while running. [operator-verify] [#q-c87a98381c80]
- ### 👀 follow up on an action item from queue item “The guidance ` - **Guidance:** This row is done, but its root item `agent: stop …”: agent: check CI runs `45084512314` (`10fc057eb3`) and `45084760886` (`9454af0368`) once they finish. [#q-f9d79a3ba364]
- ### 👀 follow up on an action item from queue item “The guidance ` - **Guidance:** This row is done, but its root item `agent: stop …”: agent: track the new id `#treefinish` (used in source and specs) in the session document's backlog or done list. The commit-message hook refused it as untracked, and I can't edit the session document. [#q-32c856cea455]
- ### follow up on an action item from queue item “`- 🙋 follow up on an action item from `checkadragainst`` is too verbose: `- ### …”: agent: check that CI run 45084117554 for `412f6c45c5` finishes green. [#q-a4e16223c394]
- ### 👀 follow up on an action item from queue item “A general heuristic of response items is to avoid large walls of text. Use struc…”: agent: implement `specs/proposals/top-level-follow-up-for-nested-replies.md` (move nested operator replies under a response to top-level `[#<origin>] follow-up:` rows, leaving a link) with the tests listed there. [#q-276e358e5940]
- ### 🚧 [#subagent:ctrlleak#1] [from:chat key=ctrlleak] Tmux pane auto-sync is very slow: sample-loop and sample-app project controllers grew to 5.5 GB and 7.3 GB RSS within 58 min of start (others <140 MB) and answer editor requests in >1 s. Find and fix the memory growth / request-loop latency. #spec-test-build-install-commit-push
- 👀 The `    - **Guidance:** 🗑️ Safe to delete this tree.` `🗑️` indicator is good. The 🗑️ should also be added to the top-most item that can be deleted. The keep icon should be at the top-most item that should be kept, if applicable, to a tree that has subtrees that can be deleted.
- 🚧 [#subagent:orphanquote#1] [from:chat key=orphanquote] fix: session-check orphan guard again refuses closeout ([#r-4648c2dbc549], [#r-4fd1a32bc2c5], [#r-5222e742146c]): response trees deleted as 'safe to delete' are still pinned by ids quoted in exchange prose, and a live nested tree is not recognized. #spec-test-build-install-commit-push
- 🙋 The top-level item should have a summary count of what needs to be worked on in the subtrees. So `- ### 🙋 [from:recovery key=q-0e29d5fdf1c4-restore] follow up on an action item from `jbdocdrift`: operator: Decide whether to retain separate 262 and 263 modular ZIPs or authorize the combined `262` through `263.*` ZIP. [operator-verify] [#q-0e29d5fdf1c4]` should have `(1 desicion, 1 running)`. This status must be kept in sync and not drift from the actual state.
- [from:chat key=topcount] do the operator's top-level open-work count request (each top-level row shows a synced count like (1 decision, 1 running)), and fix: a row that quotes a [operator-verify] token inside a code span is misread as an operator-verify row and refused for dispatch. #spec-test-build-install-commit-push
<!-- /agent:run -->

## Exchange <!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
