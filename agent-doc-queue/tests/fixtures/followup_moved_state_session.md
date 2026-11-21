---
agent_doc_session: followup-moved-state-fixture
---

<!-- agent:exchange -->
### Re: Subagent completed: `subagent:ci` returned work for queue item “follow up on an action item from queue item 'follow up on an action item from queue item 'agent-doc duplicated my prompt ` - …': agent: check CI for `126597bd4a` (`gh run list -R btakita/agent-doc --workflow CI --limit 1`). [#q-1585395fd678]” — agent-doc

> **Queue prompt:**
>
> follow up on an action item from queue item “follow up on an action item from queue item 'agent-doc duplicated my prompt ` - …”: agent: check CI for `126597bd4a` (`gh run list -R btakita/agent-doc --workflow CI --limit 1`). [#q-1585395fd678]

<!-- /agent:exchange -->

<!-- agent:run id="main" subagents -->
- ### ~~follow up on an action item from queue item “agent-doc duplicated my prompt ` - ~~Should we integrate these?~~` above this pr…”: agent: find how a row's own claim label got written into its text (`🟡 running [#subagent:lzeffectvoid#1] amend core-v1`, original leaf `amend core-v1`, plus its 📥 and struck copies and derived follow-up titles). The bare state word "running" followed by a space and the owner tag doesn't match the existing fused `running🚧` repair (`fused_lifecycle_word_token` in `agent-doc-queue-icons`). It appeared between 07:36Z and 08:06Z. ⟦published⟧~~
  - **Response [#r-f63896610f78]**
    - **Timestamp:** 2026-11-21T08:31:41.000Z
    - **Summary:** Traced the leak and fixed it: commit `126597bd4a` fast-forward pushed to agent-doc `main`. The text `🟡 running [#subagent:lzeffectvoid#1] amend core-v1` entered the document at about 08:04:40Z, when JetBrains published its own buffer over the document while `queue start` was changing the row from 🟡 to 🚧. Which part of that merge produced the word "running" is not proven; what made the damage stick was that no parser recognized `running [#owner#gen]` as an agent-doc label, so it became the row's work text. Parsers now strip it and the next projection repairs the row.
    - **Action items:** <!-- agent-doc:response-detail -->
      - agent: find the merge step that inserts the state word: the JetBrains buffer publication merges against a controller repaint still in flight (likely also the earlier `running🚧` in `sample.md`); the three texts at the merge weren't kept in any log or state database, so this needs a reproduction harness. <!-- agent-doc:response-detail -->
      - agent: check CI for `126597bd4a` (`gh run list -R btakita/agent-doc --workflow CI --limit 1`). <!-- agent-doc:response-detail -->
      - operator: the live `sample-bugs.md` has struck `running [#subagent:lzeffectvoid#1] amend core-v1` copies and follow-up titles quoting the label; once installed these parse as `amend core-v1`, but text already written into titles stays; decide whether to clean those up by hand. <!-- agent-doc:response-detail -->
    - **Next actions:** <!-- agent-doc:response-detail -->
      - Coordinator installs and closes the item. No follow-up queue heads beyond the action items. <!-- agent-doc:response-detail -->
    - **State:** Follow-up work remains.
    - **Guidance:** More work remains. Keep this tree and follow the action items in this response.
- ### ~~follow up on an action item from queue item “follow up on an action item from queue item 'agent-doc duplicated my prompt ` - …”: agent: find the merge step that inserts the state word: the JetBrains buffer publication merges against a controller repaint still in flight (likely also the earlier `running🚧` in `sample.md`); the three texts at the merge weren't kept in any log or state database, so this needs a reproduction harness. [#q-735ae4d3b3fc] ⟦published⟧~~
- ### 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'agent-doc duplicated my prompt ` - …”: operator: the live `sample-bugs.md` has struck `running [#subagent:lzeffectvoid#1] amend core-v1` copies and follow-up titles quoting the label; once installed these parse as `amend core-v1`, but text already written into titles stays; decide whether to clean those up by hand. [operator-verify]
<!-- /agent:run -->
