## Status <!-- agent:status patch=replace -->
📤 Queue handoff: 12 reported items awaiting coordinator integration, verification, and closeout.
<!-- /agent:status -->

## Active <!-- agent:active priority -->
- **Queue** (11)
  - `[#q-run834]` — #adversarial-pr ⟦effect:1⟧
    - **Status:** running
  - `[#q-unity1]` — #adversarial-pr [#q-unity1]
    - **Status:** reported
  - `[#q-rebase9]` — #rebase [#q-rebase9]
    - **Status:** reported
  - `queue-free:0091285b994a` — What are the next steps for the docs websites?
    - **Status:** reported
  - `[#q-uirole]` — **UI role**: create a "Sample Admin" studio role and assign it once the collaborator joins.
    - **Status:** reported
  - `[#q-grantboth]` — **Grant both**: run both PUTs. This fully unblocks path (a).
    - **Status:** reported
  - `queue-free:90ea53d1afd2` — Resolve the open operator action to complete the parent item. [#subagent-operator-request-sample-r…
    - **Status:** reported
  - `[#q-leave]` — leave it
    - **Status:** reported
  - `[#q-release]` — do [#q-release]
    - **Status:** reported
  - `[#q-questions]` — Draft a consolidated list of open questions in a md file.
    - **Status:** reported
  - `[#tdhandoffrelock]` — Resolve the open operator action to complete the parent item. ⟦effect:1⟧
    - **Status:** reported
<!-- /agent:active -->

## Collaboration <!-- agent:run id="collaboration" subagents go -->
- ### https://example.test/sample-api/pull/834
  - 🚧 [#subagent:c497api834#1] #adversarial-pr ⟦effect:1⟧ [#q-run834]
<!-- /agent:run -->

## Sample PRs <!-- agent:run id="sample-prs" subagents keep -->
- ### https://example.test/sample-sdk-unity/pull/1
  - ☑️ [#subagent:c768unity#1] #adversarial-pr [#q-unity1]
- ### https://example.test/sample-cli/pull/9
  - ☑️ [#subagent:c505rebase#1] #rebase [#q-rebase9] ⟦published⟧
<!-- /agent:run -->

## Docs <!-- agent:run id="docs" subagents priority -->
- ☑️ [#subagent:c497docs#1] What are the next steps for the docs websites?
<!-- /agent:run -->

## Main <!-- agent:run id="main" subagents priority -->
- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->
  - [#tdsamplereleaseapp] Release app on the sample SDK <!-- agent-doc:waiting-item -->
    - %% steps: an org owner adds the sample SDK to the release app's repository access. %%
      - owner-one [#q-ownerone]
        - **Response [#r-30853087f858]**
          - **Timestamp:** 2026-11-02T00:20:07.000Z
          - **Summary:** The draft to owner-one is ready, not sent.
            - **Choices:**
              - ☑️ [#subagent:0f24a54c#1] **UI role**: create a "Sample Admin" studio role and assign it once the collaborator joins. [#q-uirole]
              - ☑️ [#subagent:78f0f743#1] **Grant both**: run both PUTs. This fully unblocks path (a). [#q-grantboth]
        - ☑️ [#subagent:29de5653#1] Resolve the open operator action to complete the parent item. [#subagent-operator-request-sample-read] ⟦effect:3⟧
    - ☑️ [#subagent:c252releaseapp#1] do [#q-release]
  - [#tdsamplecontracts] Sample contracts suite revision <!-- agent-doc:waiting-item -->
    - %% decision: bump the suite revision 0.1.0 -> 0.2.0, or leave it. %%
    - ☑️ [#subagent:contracts-suite-leave#1] leave it [#q-leave]
- ☑️ [#subagent:c238handoff#2] Resolve the open operator action to complete the parent item. [#tdhandoffrelock] ⟦effect:1⟧
- ### ☑️ [#subagent:openquestions#1] Draft a consolidated list of open questions in a md file. [#q-questions]
<!-- /agent:run -->

## Backlog <!-- agent:backlog -->
- [ ] [#q-questions] Draft a consolidated list of open questions in a md file.
<!-- /agent:backlog -->
