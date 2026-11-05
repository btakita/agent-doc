---
agent_doc_session: outside-waiting-fixture
---

## Main <!-- agent:run id="main" subagents priority -->
- ### Register https://sbx.example.test/connect/game/callback and http://127.0.0.1:3000/connect/game/callback in allowedRedirectUris for the sandbox demo integration. Do not add http://127.0.0.1:47616/demo/callback.
  - ~~do [#decidetelemetrybase]~~
    - **Response [#r-f0536e4f1003]**
      - **Timestamp:** 2026-11-04T05:06:11.000Z
      - **Summary:** **Outcome:** The recorded choice is sdk-dev, and the gaming v0 surface stays `0.3.0-preview.1`. This leaf is safe to delete.
    - **Next:** agent — integrate the reported result, then close the item or ask what remains (evidence: ☑️ `subagent:telemetrybase#1`) ▫️ <!-- agent-doc:reply -->
    - **Response:** Decide for the telemetry annotation change: PR base sdk-dev or dev, and whether to add a new gaming v0 surface revision after 0.3.0-preview.1 for the property-became-required break <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Decide for the telemetry annotation change: PR base sdk-dev or dev, and whether to add a new gaming v0 surface revision after 0.3.0-preview.1 for the property-became-required break <!-- agent-doc:waiting-metadata -->
          - ~~Target sdk-dev. Keep 0.3.0-preview.1 [#q-a3abacb5f56c]~~
              - ~~**Summary:** **Outcome:** The pull request targets `sdk-dev`. This leaf is safe to delete. [#q-80391ed27c62]~~
  - 🙋☑️ [#subagent:godot#45] ⟦☑️ you: 0 · agent: 1 · done: 0⟧ [#tdredirect] Register the demo sign-in redirect http://127.0.0.1:47616/demo/callback in allowedRedirectUris for the sandbox game integration, or pick an already-registered port for DEMO_REDIRECT_URI <!-- agent-doc:waiting-item --> ⟦published⟧
    - **Next:** agent — integrate the reported result, then close the item or ask what remains (evidence: ☑️ `subagent:godot#45`) ▫️
    - **Response:** Register the demo sign-in redirect http://127.0.0.1:47616/demo/callback in allowedRedirectUris for the sandbox game integration, or pick an already-registered port for DEMO_REDIRECT_URI <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Register the demo sign-in redirect http://127.0.0.1:47616/demo/callback in allowedRedirectUris for the sandbox game integration, or pick an already-registered port for DEMO_REDIRECT_URI <!-- agent-doc:waiting-metadata -->
          - Use the frontend standard http ports. Add a frontend PR if necessary. [#q-555e7dac42af]
  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#subagent-operator-request-tdredirect-allowlist] Answer the blocking subagent question for queue item ‘[#tdredirect]: Add http://127.0.0.1:47616/demo/callback’ in agent:exchange: Adding `http://127.0.0.1:47616/demo/callback` is not a worktree edit. (choices: Stop, Studio append, Infra apply, Local demo only) <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item with the decision or result it asks for (evidence: no answer or attempt in its packet yet) ▫️
- ### ~~do [#tdpush]~~
  - ~~push + pr? ⟦published⟧~~
- ### ~~[#tdrelock]: authorized~~
  - ~~I don't see an auth.failure role. Could you point it out to me?~~
<!-- /agent:run -->

## Todo <!-- agent:todo priority -->
- [ ] [#tdtelemetrybase] Decide for the telemetry annotation change: PR base sdk-dev or dev, and whether to add a new gaming v0 surface revision after 0.3.0-preview.1 for the property-became-required break
- [ ] [#tdredirect] Register the demo sign-in redirect http://127.0.0.1:47616/demo/callback in allowedRedirectUris for the sandbox game integration, or pick an already-registered port for DEMO_REDIRECT_URI
- [ ] [#subagent-operator-request-tdredirect-allowlist] Answer the blocking subagent question for queue item ‘[#tdredirect]: Add http://127.0.0.1:47616/demo/callback’ in agent:exchange: Adding `http://127.0.0.1:47616/demo/callback` is not a worktree edit. (choices: Stop, Studio append, Infra apply, Local demo only)
- [ ] [#tdpush] Push the local credential branch and open its pull request
- [ ] [#tdrelock] As release owner, decide whether to update the auth.failure row in the handoff guide and re-lock the release metadata
<!-- /agent:todo -->
