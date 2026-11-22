---
agent_doc_session: loose-ends-fixture
agent_doc_format: template
---

## Status <!-- agent:status patch=replace -->
<!-- /agent:status -->

## Active <!-- agent:active priority -->
- **Running** (2)
  - `queue-free:902aadf5a66d` — follow up on an action item from queue item “[from:chat key=orphannest] fix: session-check orphan gu…
    - **Status:** running
<!-- /agent:active -->

## Run <!-- agent:run id="main" subagents go -->
- 🚧 [#subagent:recapids#1] The recap says `- 4 decisions for you`, `- 4 cleanup rows`, `- 3 blocked`. The ids should all be listed in the recap. Update agent-doc.
- ### ⛔ ⟦blocked by prompt-blocker-b0ddfbae2f94⟧ do [#jbfrontendswap]: approved
- ### ⛔ do [#ghrecvpolicy]: #advance whatever is actionable.
- ### 🙋 [from:recovery key=q-0e29d5fdf1c4-restore] follow up on an action item from `jbdocdrift`: operator: Decide whether to retain separate 262 and 263 modular ZIPs or authorize the combined `262` through `263.*` ZIP. [operator-verify] [#q-0e29d5fdf1c4]
- ### ⛔ [#prompt-blocker-b0ddfbae2f94]: Use chromium-bridge to perform the steps.
- ### agent-doc duplicated my prompt `      - ~~Should we integrate these?~~` above this prompt.
  - **Response [#r-ed4566860530]**
    - **Timestamp:** 2026-11-21T08:31:41.000Z
    - **Summary:** Fixed and pushed to agent-doc `main`.
    - **Action items:** <!-- agent-doc:response-detail -->
      - agent: find and fix the exact step that doubled the struck `~~Should we integrate these?~~` line, with a regression test. <!-- agent-doc:response-detail -->
    - **State:** Follow-up work remains.
    - **Guidance:** More work remains. Keep this tree and follow the action items in this response.
    - 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'complete the work for this tree ⟦pu…”: operator: decide whether to refresh the VS Code package (`make install` skipped it because `vsce` is not installed; fix with `npm install` in `editors/vscode` or a global `@vscode/vsce`, then `make install-editor-plugins`). [operator-verify] [#q-cb3ac1276fd3]
    - 🙋 follow up on an action item from queue item “complete the work for this tree ⟦published⟧”: operator: apply edits 1–3 above in the editor if you want the old titles cleaned (cosmetic; they no longer collide), then strike the "decide whether to clean those up by hand" and "delete the extra struck copies" rows. [operator-verify] [#q-dade5157fe53]
- ### 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'agent-doc duplicated my prompt ` - …”: operator: the live `sample-bugs.md` has struck `running [#subagent:effectvoid#1] amend core-v1` copies and follow-up titles quoting the label; once installed these parse as `amend core-v1`, but text already written into titles stays; decide whether to clean those up by hand. [operator-verify] [#q-8eeadf7937c8]
- ### 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”: operator: in the live `tasks/agent-doc/sample-bugs.md` queue, delete the extra struck copies of `running [#subagent:effectvoid#1] amend core-v1` (there are 3) and any doubled `~~Should we integrate these?~~`; these are real duplicate rows, so the keyed merge still correctly declines on those cuts and uses the fallback merge until cleaned up. [operator-verify] [#q-e107b04c1cd5]
- ### 🙋 follow up on an action item from queue item “[from:chat key=pypitrust2] finish sample-py PyPI trusted publishing now that the…”: operator: decide whether to revoke the PyPI API token named **`publish 2026-11-13`** (scope **All projects**, created Nov 13, 2026, last used Nov 14, 2026). It is the only API token on the account. [operator-verify] [#q-5d1c0f2e9a77]
- ### 🙋 follow up on an action item from queue item “[from:chat key=keyring] fix the gh keyring hang”: operator: unlock the `login` keyring once by running `gnome-keyring-unlock-login` in a terminal. [operator-verify] [#q-a1b2c3d4e5f6]
- ### 🙋 follow up on an action item from queue item “[from:chat key=keyring] fix the gh keyring hang”: agent: run the single `make install` and confirm `agent-doc --version`. [#q-0f9e8d7c6b5a]
- ### ~~[from:chat key=strayctrl] fix the stray runbooks controller. ⟦published⟧~~
- ### ~~follow up on an action item from queue item “[from:chat key=strayctrl] fix the stray runbooks controller.”: agent: run `make install` in the agent-doc submodule.~~
- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->
  - ⟦you: 1 · agent: 0 · done: 0⟧ [#ghrecvpolicypark] Decide whether backlog #ghrecvpolicy stays open in the backlog or moves to the icebox. (source: agent:backlog [#ghrecvpolicy]) <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for ▫️
- 🚧 [#subagent:ci53#1] follow up on an action item from queue item “[from:chat key=orphannest] fix: session-check orphan guard refuses closeout beca…”: agent: check the agent-doc CI run for `53fa578ad6` on `main`.
- 🚧 [#subagent:sampledevhook#1] [from:chat key=sampledevhook] In a subagent: fix agent-doc issues in sample-dev.md: Stop hook timed out after 5.0s waiting for the sample-app project controller. #spec-test-build-install-commit-push
<!-- /agent:run -->

## Backlog <!-- agent:backlog -->
- [/] [#ghrecvpolicy] GH #40 recipient-side auto_create_components policy; blocked until multi-user workspace access control lands.
<!-- /agent:backlog -->

## Todo <!-- agent:todo -->
- [ ] [#ghrecvpolicypark] Decide whether backlog #ghrecvpolicy stays open in the backlog or moves to the icebox.
<!-- /agent:todo -->

## Exchange <!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
