---
agent_doc_session: release-fixture
---

## Main <!-- agent:run id="main" subagents priority -->
- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->
  - ⟦☑️ you: 2 · agent: 2 · done: 1⟧ [#tdalphahandoff] Monday public Alpha <!-- agent-doc:waiting-item --> ⟦effect:1⟧
    - **Next:** agent — integrate the reported results, then close the item or ask what remains ▫️
    - Complete the remaining work for the parent item. ⟦effect:2⟧ [#q-4d7fa65f3012]
      - **Closed by:** [#r-3b817c02c2e8]
        - **Choices:**
          - **Recommended bundle**: D1 `v0.1.0-alpha.1`, owner Pat, cutoff Monday 23:59.
          - ☑️ [#subagent:alphabundle#1] Finish your recommended bundle. [#q-0a3b9df623ef]
          - **Request**: [#subagent-operator-request-tdalphahandoff-d1d4]
            - ~~Pat Owner is the Alpha release. [#q-90799291121f]~~
          - Use the Recommended bundle
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#tdcontractsbump] Decide whether to bump the contracts suite revision 0.1.0 -> 0.2.0 for the null-expiry scenarios. SDK runners need the 4 new step verbs <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for ▫️
    - **Response:** Decide whether to bump the contracts suite revision 0.1.0 -> 0.2.0 for the null-expiry scenarios. ▫️
      - **Timestamp:** 2026-11-04T21:41-06:00 ▫️
      - **Question:** What decision or completion result should be recorded? ▫️
      - **Next actions:** ▫️
        - **Action:** Decide whether to bump the contracts suite revision 0.1.0 -> 0.2.0 for the null-expiry scenarios. ▫️
          - Keep at 0.1.0. Add the 4 new step verbs.
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#tdadrapprovers] Name the approver(s) for docs PR #88 (ADR 0046, rules for unknown players) and request their review <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for ▫️
    - **Response:** Name the approver(s) for docs PR #88 (ADR 0046, rules for unknown players) and request their review ▫️
      - **Next actions:** ▫️
        - **Action:** Name the approver(s) for docs PR #88 ▫️
          - Add Alex + Sam
          - Keep the approver as is. PR #88 already merged. Create a PR to accept all ADRs on main.
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#y2fn] Choose the ADR 0041 live-credential cap N (5 or 10), the eviction rule (oldest-issued or least-recently-used), and the brief recipients. <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for ▫️
    - **Response:** Choose the ADR 0041 live-credential cap N (5 or 10), the eviction rule (oldest-issued or least-recently-used), and the brief recipients. ▫️
      - **Next actions:** ▫️
        - **Action:** Choose the ADR 0041 live-credential cap N (5 or 10), the eviction rule (oldest-issued or least-recently-used), and the brief recipients. ▫️
          - JWS makes the cap moot. We don't store credentials.
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#subagent-operator-request-c690prov-cap] Answer the blocking subagent question for queue item ‘do [#tdprovauth]’ in agent:exchange: Choose the api live-credential cap N (5 or 10), the eviction rule (oldest-issued or least-recently-used), and the brief recipients and channel. (choices: N = 5, N = 10) (source: agent:review [#tdprovauth]) <!-- agent-doc:waiting-item -->
    - **Next:** you — answer: Choose the api live-credential cap N ▫️
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#tdcap20] Choose the ADR 0041 live-credential cap N (5 or 20), the eviction rule (oldest-issued or least-recently-used), and the brief recipients. <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for ▫️
- ### 🙋📥 [#subagent-operator-request-tdalphahandoff-d1d4]: Pat Owner
- ### ~~[#subagent-operator-request-tdgodot-allowlist]: Use the standard http ports.~~
- ### ☑️ [#subagent:c005game#1] do [#implementfeaturegame]
<!-- /agent:run -->

## Todo <!-- agent:todo priority -->
- [ ] [#tdprovauth] Implement and reconcile accepted ADR 0041 durable game credentials across docs, API, contracts, and SDKs.
- [ ] [#tdalphahandoff] Record the Monday public-Alpha decision: a no-go on a public v0.1.0-alpha.1 tag, or a cutoff for a conditional go, and the release owner.
- [ ] [#tdcontractsbump] Decide whether to bump the contracts suite revision 0.1.0 -> 0.2.0 for the null-expiry scenarios. SDK runners need the 4 new step verbs
- [ ] [#tdadrapprovers] Name the approver(s) for docs PR #88 (ADR 0046, rules for unknown players) and request their review
- [ ] [#tdgodot] Register the demo sign-in redirect in the allowed redirect list, or pick an already-registered port
- [ ] [#subagent-operator-request-tdalphahandoff-d1d4] Name Alpha release owner, report owner, cutoff, and D1–D3 before any Monday prerelease.
- [ ] [#subagent-operator-request-tdgodot-allowlist] Answer the blocking subagent question for queue item ‘[#tdgodot]: add the callback’ in agent:exchange: Adding the callback is not a worktree edit. (choices: Stop, Studio append)
- [ ] [#y2fn] Choose the ADR 0041 live-credential cap N (5 or 10), the eviction rule (oldest-issued or least-recently-used), and the brief recipients.
- [ ] [#subagent-operator-request-c690prov-cap] Answer the blocking subagent question for queue item ‘do [#tdprovauth]’ in agent:exchange: Choose the api live-credential cap N (5 or 10), the eviction rule (oldest-issued or least-recently-used), and the brief recipients and channel. (choices: N = 5, N = 10) (source: agent:review [#tdprovauth])
- [ ] [#tdcap20] Choose the ADR 0041 live-credential cap N (5 or 20), the eviction rule (oldest-issued or least-recently-used), and the brief recipients.
<!-- /agent:todo -->

## Done <!-- agent:done -->
- 2026-11-04 archived waiting item `#tdbranch` (its Todo closed): `#tdbranch` Choose the target branch — its 3 rows moved here from the Waiting on you tree of queue `main` <!-- agent-doc:waiting-archive -->
  >   - ⟦you: 1 · agent: 0 · done: 0⟧ [#tdbranch] Choose the target branch for the telemetry pull request: sdk-dev or dev <!-- agent-doc:waiting-item -->
  >     - **Next:** you — reply under this item ▫️
  >     - Target sdk-dev. Keep 0.3.0-preview.1 [#q-a3abacb5f56c]
<!-- /agent:done -->
