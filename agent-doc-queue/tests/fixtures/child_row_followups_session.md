---
agent_doc_session: child-row-followups-fixture
---

<!-- agent:exchange -->
<!-- /agent:exchange -->

<!-- agent:run id="main" subagents -->
- ### ⛔ ⟦blocked by prompt-blocker-b0ddfbae2f94⟧ do [#jbfrontendswap]: approved
- ### 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”: operator: in the live `tasks/sample/sample-bugs.md` queue, delete the extra struck copies of `amend core-v1` (there are 3) and any doubled `~~Should we integrate these?~~`; these are real duplicate rows, so the keyed merge still correctly declines on those cuts and uses the fallback merge until cleaned up. [operator-verify] [#q-e107b04c1cd5]
    - 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'complete the work for this tree ⟦pu…”: operator: decide whether to refresh the VS Code package (`make install` skipped it because `vsce` is not installed; fix with `npm install` in `editors/vscode` or a global `@vscode/vsce`, then `make install-editor-plugins`). [operator-verify] [#q-cb3ac1276fd3]
      - 📥 #justdoit
    - 🙋 follow up on an action item from queue item “complete the work for this tree ⟦published⟧”: operator: apply edits 1–3 above in the editor if you want the old titles cleaned (cosmetic; they no longer collide), then strike the "decide whether to clean those up by hand" and "delete the extra struck copies" rows. [operator-verify] [#q-dade5157fe53]
      - 📥 clean up the titles
- ### follow up on an action item from queue item “sample-app release”: agent: check CI for `sample-app`.
<!-- /agent:run -->
