---
agent_doc_session: stale-fixture
---

## Main <!-- agent:run id="main" subagents priority -->
- ### 📥 Waiting on you — answer by adding a child line under an item [#q-4775b8697412]
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#tdalphahandoff] Record the Monday public-Alpha decision: a no-go on a public v0.1.0-alpha.1 tag, or a cutoff for a conditional go, and the release owner. <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for ▫️
  - 🙋 ⟦✅ you: 0 · agent: 0 · done: 1⟧ [#tdadrapprovers] Name the approver(s) for docs PR #88 (ADR 0046, rules for unknown players; candidates Alex and Sam), request their review, and ask the ADR 0036 and ADR 0040 owners to replace their restated signed-out rules with a link to ADR 0046 <!-- agent-doc:waiting-item -->
    - **Next:** you — check off `#tdadrapprovers` in Todo, or reply with what remains (evidence: every row under the item is closed) ▫️
    - **Response:** Name the approver(s) for docs PR #88 (ADR 0046, rules for unknown players; candidates Alex and Sam), request their review, and ask the ADR 0036 and ADR 0040 owners to replace their restated signed-out rules with a link to ADR 0046 ▫️
      - **Timestamp:** 2026-11-04T21:22-06:00 ▫️
      - **Question:** What decision or completion result should be recorded? ▫️
      - **Next actions:** ▫️
        - **Action:** Name the approver(s) for docs PR #88 (ADR 0046, rules for unknown players; candidates Alex and Sam) ▫️
          - ~~Keep the approver as is. PR #88 already merged. Create a PR to accept all ADRs on main.~~
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#tdpending] Choose the docs site launch date: this Friday or next Monday. <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for ▫️
    - Next Monday.
- ### ~~[#tdalphahandoff]: build the preview release~~
- ### ~~[#tdadrapprovers]: Make a PR to update the ADRs to link to ADR 0046 ⟦published⟧~~
- ### ~~[#subagent-operator-request-docs-port]: Use the standard http ports.~~
- ### ~~[#nextstepsdocs]: Run the go-live checklist first.~~
- ### ~~[#docsgolive]: PR #321 merged at c71880dc60e3; the full deploy ran green.~~
- ### ~~[#coldchoice]: Keep the plugin unchanged.~~
- ### 📥 [#tdprovauth]: Is this done?
<!-- /agent:run -->

## Todo <!-- agent:todo priority -->
- [ ] [#tdprovauth] Implement and reconcile accepted ADR 0041. The cap is moot: JWS, and credentials are not stored as a capped set. (source: agent:review [#tdprovauth])
- [ ] [#tdalphahandoff] Record the Monday public-Alpha decision: a no-go on a public v0.1.0-alpha.1 tag, or a cutoff for a conditional go, and the release owner.
- [ ] [#tdadrapprovers] Name the approver(s) for docs PR #88 (ADR 0046, rules for unknown players; candidates Alex and Sam), request their review, and ask the ADR 0036 and ADR 0040 owners to replace their restated signed-out rules with a link to ADR 0046
- [ ] [#subagent-operator-request-docs-port] Choose how to add the demo callback: stop, studio append, or local demo only
- [ ] [#tdpending] Choose the docs site launch date: this Friday or next Monday.
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue="main" -->
- [ ] [#nextstepsdocs] What are next steps to make the docs site public?
- [ ] [#buildfeature] Build the allowed-domains feature.
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#docsgolive] Merge PR #321, then run the full deploy and redispatch the docs site.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
- [ ] [#coldchoice] Choose how the plugin consumes the moderation modules: a module map, a packaged model, or leave the plugin unchanged
<!-- /agent:icebox -->
