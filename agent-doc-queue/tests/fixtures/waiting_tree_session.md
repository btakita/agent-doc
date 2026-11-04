# Waiting-on-you session fixture

Trimmed copy of a live session document (2026-11-03): its run queues, Todo and Backlog.

## Collaboration <!-- agent:run id="collaboration" subagents go -->
- %% Queue completed and cleared at 2026-11-04T02:16:51.000Z. %%
- ☑️ [#subagent:c012craigdraft#1] Draft a response to https://haivengroup.slack.com/archives/D0BM13YETUK/p1793755182148869
<!-- /agent:run -->

## haiven-infra-prs  <!-- agent:run id="haiven-infra-prs" subagents keep -->
- ### ~~https://github.com/haiven-dev/haiven-infra/pull/318~~
  - ~~#adversarial-pr: https://github.com/haiven-dev/haiven-infra/pull/318 [#q-20b5b87a683c]~~ — auto-struck: addressed this cycle ⟦addressed-at:2026-11-04T00:35:27.000Z⟧
<!-- /agent:run -->

## haiven-sdk-prs  <!-- agent:run id="haiven-sdk-prs" subagents keep -->
- ### https://github.com/haiven-dev/haiven-sdk-unity/pull/1
  - ☑️ [#subagent:c768unity#1] #adversarial-pr [#q-16b6cac571e5]
  - 📥 #address [#q-12166e629839] [#subagent-operator-request-unity-pr1-2fa]
    - **Response [#r-5f57579575b3]**
      - **Timestamp:** 2026-11-03T22:52:49.000Z
      - **Summary:** **Outcome:** Unity pull request 1 has local address commit `53337f635b2bf1c2e349cd2d0a6ff12361f070a0` on `ci/release-artifacts` in `/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk-unity/release-artifacts`. The remote head is still `8dd0a8004645e11b79791a55d20129107a86b5d6`. Boss Room pull request 1 has an unposted COMMENTED review draft at head `72d4f17791d63995490203a9a939d9ba63a17738` in `/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk-unity-bossroom/pr1-review/.review-pr1.md`. You chose enable secure 2FA and rerun for both. `gh api user` returns `btakita`, and `gh pr view` on both repositories still returns the haiven-dev secure-2FA 403, so the push and the review post have not run. GitHub is rejecting btakita for haiven-dev until the account has a secure 2FA method (authenticator app, GitHub Mobile, security key, or passkey). Push of 53337f635b2bf1c2e349cd2d0a6ff12361f070a0 and the pull-request comment both fail with 403. Enable that 2FA, or name another credential that can push ci/release-artifacts and comment on haiven-sdk-unity#1.
      - **State:** Waiting on operator input.
      - **Guidance:** More work remains. Keep this tree and answer the operator request below.
      - **Choices:**
        - **Request**: [#subagent-operator-request-unity-pr1-2fa]
        - **Enable secure 2FA and rerun**: push 53337f635b2bf1c2e349cd2d0a6ff12361f070a0 and post the review response.
        - **Supply another publisher**: a non-btakita credential that can push this branch and comment on the pull request.
- 📥 https://github.com/haiven-dev/haiven-sdk-unity-bossroom/pull/1
  - 📥 #review [#q-fe85961f88a2] [#subagent-operator-request-pr1-review-2fa]
    - **Closed by:** [#r-5f57579575b3]
      - **Choices:**
        - **Request**: [#subagent-operator-request-pr1-review-2fa]
        - **fix-btakita-2fa**: Enable a secure 2FA method on `btakita`, re-authenticate `gh`, and redispatch this review post.
        - **supply-writer-token**: Put a token that can write reviews on this repo where the worker can read it without printing it, then redispatch the post.
<!-- /agent:run -->

## haiven-cli-prs  <!-- agent:run id="haiven-cli-prs" subagents keep -->
- ### 🙋☑️ [#subagent:c505releaseplease#1] Setup Release Please workflow to merge sdk-dev into main. [#q-de25f07c91b5] [#subagent-operator-request-cli-release-please-sdk-dev]
  - **Closed by:** [#r-2857e6546ec9]
    - **Choices:**
      - **Request**: [#subagent-operator-request-cli-release-please-sdk-dev]
      - **Doc fix only**: commit and push the `dev` to `sdk-dev` edits on a new branch from `origin/sdk-dev`, and open a pull request into `sdk-dev`. Do not open or merge a `main` promotion.
      - **Doc fix and promotion PR**: do the doc-fix pull request, then open, without merging, a `sdk-dev` to `main` pull request titled `chore: promote sdk-dev to main` with the `BEGIN_COMMIT_OVERRIDE` block.
      - **Promotion only**: open, without merging, the `sdk-dev` to `main` promotion pull request with the current `dev` wording left in place.
      - **Stop**: leave haiven-cli unchanged.
- ### https://github.com/haiven-dev/haiven-cli/pull/7
  - ☑️ [#subagent:c505clipr7#1] #adversarial-pr [#q-398d46a305e0]
- ### https://github.com/haiven-dev/haiven-cli/pull/9
  - ☑️ [#subagent:c505clipr9rebase#1] #rebase [#q-3d2bbe420d92] ⟦published⟧
<!-- /agent:run -->

## Docs <!-- agent:run id="docs" subagents priority -->
- ### ~~docs.sbx.haiven.gg + docs.haiven.gg~~
  - ~~Follow https://github.com/haiven-dev/haiven-docs/blob/main/decisions/0042-sdk-docs-site.md to provision a docs.sbx.haiven.gg docs.haiven.gg site. [#q-b18a1a65b5bd]~~
    - **Response [#r-300dc1e67e08]:** `subagent:docs-site-provision` finished. Everything for **docs.sbx.haiven.gg** is up for review. Nothing was applied, no DNS changed, and no variables were set. **docs.haiven.gg** is untouched: Pages is off and `SITE_PUBLISH` is unset until the cutover.
<!-- /agent:run -->

## Main <!-- agent:run id="main" subagents priority -->
- ### In a haiven-contracts worktree, draft gaming-auth-handoff/v0 conformance scenarios for an exchange with expires_in: null and for a client reusing a saved credential before any handoff (ADR 0041, SDK-HANDOFF-11/12); local commit only, no push [#subagent-operator-request-adr0041-auth-owner-acceptance] <!-- agent-doc:waiting-item -->
  - **Response:** ADR 0041 credential storage: hybrid-cap-inject chosen 2026-11-01 (SDK store seam in progress). Still to choose: the haiven-api live-credential cap N (5 or 10) and eviction rule (oldest-issued or least-recently-used), plus the recipients and approved channel for the ADR 0041 brief. <!-- agent-doc:waiting-metadata -->
    - **Timestamp:** 2026-11-03T09:58-05:00 <!-- agent-doc:waiting-metadata -->
    - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
    - **Next actions:** <!-- agent-doc:waiting-metadata -->
      - **Action:** ADR 0041 credential storage: hybrid-cap-inject chosen 2026-11-01 (SDK store seam in progress). Still to choose: the haiven-api live-credential cap N (5 or 10) and eviction rule (oldest-issued or least-recently-used), plus the recipients and approved channel for the ADR 0041 brief. <!-- agent-doc:waiting-metadata -->
  - 📥 Complete the in-progress work for the parent item. [#q-ec93586f6279]
- ### Waiting on you — answer by adding a child line under an item
  - [#tdcraigunityhandoff] Monday 2026-11-02 public Alpha ⟦effect:1⟧
    - Complete the remaining work for the parent item. ⟦effect:2⟧ [#q-4d7fa65f3012]
      - **Closed by:** [#r-3b817c02c2e8]
        - **Choices:**
                      - ☑️ [#subagent:craigaccept#1] **Action:** Craig accepts both Read invitations. The Alpha choice stays on the existing todo [#subagent-operator-request-tdcraigunityhandoff-d1d4]. The ADR 0046 sibling stays on the existing todo [#subagent-operator-request-adr0046-sibling-pick].
          - **Recommended bundle**: D1 `v0.1.0-alpha.1`, D2 Path B, D3 haiven-cli matches the SDK, release owner and Alpha report owner Brian, external access stays Craig-only until Brian approves anyone else, cutoff 2026-11-02 23:59 America/Chicago. A later item must say `go` before bump pull requests or tags.
          - **Different answers**: reply with your own D1, D2, D3, owners, and cutoff.
          - **No-go**: do not tag. Keep Craig on the internal ring at `6cd0e6df66370f43ba76de31149597d3587eb79c`.
          - ☑️ [#subagent:alphabundle#1] Finish your recommended bundle. [#q-0a3b9df623ef]
          - **Request**: [#subagent-operator-request-tdcraigunityhandoff-d1d4]
            - ~~Brian Takita is the Alpha release. [#q-90799291121f]~~
  - [#tdsdkreleaseapp] haiven-release App on haiven-sdk
    - %% steps: an org owner (tmstorm recommended) adds haiven-sdk to the haiven-release App's repository access, then re-runs run 40635565681. decision: who asks/does it. %%
      - tmstorm [#q-569d77146187]
        - **Response [#r-30853087f858]**
          - **Timestamp:** 2026-11-02T00:20:07.000Z
          - **Summary:** **`[#tdsdkreleaseapp]`: the Slack draft to tmstorm is ready, not sent.** It's in `/home/brian/work/btakita/agent-loop/src/haiven-dev/notes/haiven-release-app-slack-draft.md`. PR #88 merged with "Approver: Luke Richey" and an approval from Dillon (`tmstorm`). Should Dillon still be added to the ADR as an approver? If so, may I open a new haiven-docs PR from main to do it?
            - **Choices:**
              - ~~**Request**: [#subagent-operator-request-craigsbx-admin-role] ⟦effect:1⟧~~
              - ☑️ [#subagent:0f24a54c#1] **UI role**: create a "Craig SDK Admin" studio role with `studio.manage_app` (plus `app.edit` if he should edit game info) and assign it to Craig once he joins. [#q-0f24a54c9ec9]
              - ☑️ [#subagent:ac3fc2f9#1] **Narrow app role**: leave Craig as a plain member and add an API-made app role on `6cea29d1` with `app.view_sdk` and `app.manage_integrations` (plus `app.manage_rewards` if needed). [#q-ac3fc2f9bace]
              - ☑️ [#subagent:3864daff#1] **Tester only**: no admin; Craig only signs in as a player. Studio membership isn't needed for that. [#q-3864daff801a]
              - ☑️ [#subagent:78f0f743#1] **Grant both**: run both PUTs. This fully unblocks path (a). [#q-78f0f74330fd]
              - ☑️ [#subagent:fba006f7#1] **Grant haiven-sdk only**: he can read docs and pull the SDK, but can't build the CLI. The build-from-source step fails at `git clone`. [#q-fba006f725d0]
              - **Operator does it in the UI**: Settings → Collaborators → Add people → `craigsterATX` → Read, on each repo. [#q-29de5653a46a]
                - **Closed by:** [#r-3b817c02c2e8]
                  - **Choices:**
                    - **Request**: [#subagent-operator-request-craig-ui-read]
                    - **UI role**: create a "Craig SDK Admin" studio role with `studio.manage_app` (plus `app.edit` if he should edit game info) and assign it to Craig once he joins. [#q-0f24a54c9ec9]
                    - **Narrow app role**: leave Craig as a plain member and add an API-made app role on `6cea29d1` with `app.view_sdk` and `app.manage_integrations` (plus `app.manage_rewards` if needed). [#q-ac3fc2f9bace]
                    - **Tester only**: no admin; Craig only signs in as a player. Studio membership isn't needed for that. [#q-3864daff801a]
                    - **Grant both**: run both PUTs. This fully unblocks path (a). [#q-78f0f74330fd]
                      - ~~pick~~
                        - **Closed by:** [#r-c2bddcd86f40]
                    - **Grant haiven-sdk only**: he can read docs and pull the SDK, but can't build the CLI. The build-from-source step fails at `git clone`. [#q-fba006f725d0]
                    - **Operator does it in the UI**: Settings → Collaborators → Add people → `craigsterATX` → Read, on each repo. [#q-29de5653a46a]
                - ☑️ [#subagent:29de5653#1] Resolve the open operator action to complete the parent item. [#subagent-operator-request-craig-ui-read] ⟦effect:3⟧
            - **Done as-is**: Dillon's approval on #88 counts as his review. Leave the ADR naming Luke and close `#tdadr0046approvers`. [#q-865c445b9b8a]
              - **Closed by:** [#r-3b817c02c2e8]
                - **Choices:**
                  - **Request**: [#subagent-operator-request-adr0046-sibling-pick]
                  - **Done as-is**: treat Dillon's approval on #88 as his review, leave the ADR naming Luke, and close `#tdadr0046approvers`.
                  - **Add Dillon by follow-up PR**: branch from `origin/main` in that worktree, name Luke Richey and Dillon Storm on line 76, then commit, push, open a haiven-docs pull request, and request Luke's review.
                    - ~~running🚧 do [#q-c2010d98af04]~~
                  - **Something else**: say how to record his role, for example as a reviewer rather than an approver.
              - ☑️ [#subagent:865c445b#1] Resolve the open operator action to complete the parent item. [#subagent-operator-request-adr0046-sibling-pick] ⟦effect:4⟧
            - ☑️ [#subagent:229564d1#1] **Add Dillon by follow-up PR**: in the worktree `/home/brian/work/btakita/worktrees/haiven-dev/haiven-docs/adr0046-dillon`, branch from `origin/main` and change line 76 to name Luke Richey and Dillon Storm. Commit, push, open a haiven-docs PR and request Luke's review. [#q-229564d17c90]
            - ☑️ [#subagent:5e113a73#1] **Something else**: say how to record his role, for example as a reviewer rather than an approver. [#q-5e113a733fa3]
        - ☑️ [#subagent:dillonrepos#1] Dillon configured the repos. Please evaluate all the open items and progress anything that this configuration unblocks. [#q-aa7da63e6060]
    - ☑️ [#subagent:c252releaseapp#1] do [#q-c4dbe51b8e36]
  - ~~[#tddocssbxgolive] Make docs.sbx.haiven.gg live~~
    - %% steps: approve+merge haiven-docs #87 and haiven-infra #317 → start "SBX -- Full Run" on GitHub → Porkbun CNAME docs.sbx → output cname_answer → set DOCS_SBX_ROLE_ARN/BUCKET/DISTRIBUTION_ID → approve+merge haiven-sdk #194. decision: approvers/owners for each step. %%
  - [#tdcontractsscenariospush] haiven-contracts #190 suite revision
    - %% decision: bump gaming-auth-handoff/v0 revision 0.1.0 → 0.2.0, or leave it. %%
    - ☑️ [#subagent:contracts-suite-leave#1] leave it [#q-0e54f7500801]
- ### ⛔ ⟦blocked by tdclireleasedownloads⟧ do [#clidocsreleasecleanup]
- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->
  - [#tdprovauth] Implement and reconcile accepted ADR 0041 durable game credentials across docs, API, contracts, and maintained SDKs: update the source status to accepted; remove the fixed 12-hour lifetime; complete issuance/logout serialization and request-time logout-watermark checks; retain valid credentials across reconnect and transport loss; and implement secure cross-restart storage or host injection. Anonymous (unconnected) interactions are supported on every surface per proposed ADR 0046 (haiven-docs PR #88), using the distinct chat-only credential (#implementanonymouschat). Keep HAVN-1178 install-ID privacy/legal work separate. (source: agent:review ) <!-- agent-doc:waiting-item -->
    - **Response:** Implement and reconcile accepted ADR 0041 durable game credentials across docs, API, contracts, and maintained SDKs: update the source status to accepted; remove the fixed 12-hour lifetime; complete issuance/logout serialization and request-time logout-watermark checks; retain valid credentials across reconnect and transport loss; and implement secure cross-restart storage or host injection. Anonymous (unconnected) interactions are supported on every surface per proposed ADR 0046 (haiven-docs PR #88), using the distinct chat-only credential (#implementanonymouschat). Keep HAVN-1178 install-ID privacy/legal work separate. (source: agent:review ) ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T14:54-05:00 ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T13:45-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Implement and reconcile accepted ADR 0041 durable game credentials across docs, API, contracts, and maintained SDKs: update the source status to accepted; remove the fixed 12-hour lifetime; complete issuance/logout serialization and request-time logout-watermark checks; retain valid credentials across reconnect and transport loss; and implement secure cross-restart storage or host injection. Anonymous (unconnected) interactions are supported on every surface per proposed ADR 0046 (haiven-docs PR #88), using the distinct chat-only credential (#implementanonymouschat). Keep HAVN-1178 install-ID privacy/legal work separate. (source: agent:review ) ▫️ <!-- agent-doc:reply -->
          - 📥 Go: HAVN-1178 should follow the #chat-policy. [#q-4ad6f250e52d]
  - [#sdkparitygate] Complete and verify full parity across Browser, Node, React Native, Unity, Godot, and Unreal; Roblox/Luau is excluded. Require target-native proof for all applicable normative protocol and shared product rows, plus install/configure/connect docs, samples, supported-version evidence, and exact-SHA qualification. (source: agent:backlog ) <!-- agent-doc:waiting-item -->
    - **Response:** Complete and verify full parity across Browser, Node, React Native, Unity, Godot, and Unreal; Roblox/Luau is excluded. Require target-native proof for all applicable normative protocol and shared product rows, plus install/configure/connect docs, samples, supported-version evidence, and exact-SHA qualification. (source: agent:backlog ) ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T13:45-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Complete and verify full parity across Browser, Node, React Native, Unity, Godot, and Unreal; Roblox/Luau is excluded. Require target-native proof for all applicable normative protocol and shared product rows, plus install/configure/connect docs, samples, supported-version evidence, and exact-SHA qualification. (source: agent:backlog ) ▫️ <!-- agent-doc:reply -->
          - 📥 verified [#q-79076461751e]
      - **Timestamp:** 2026-11-03T13:45-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
  - [#subagent-operator-request-craigunityfirstconnection-inputs] Provide Craig actual Unity game absolute path, branch and HEAD, Unity version and named target, sanitized environment/public IDs, exact registered redirect, a second ordinary user, an interactive browser-sign-in operator, and confirm the accepted SDK revision (PR #190 head 97e108c28a75f6601595c4055b1725c99d981c0e or successor). (source: agent:review [#craigunityfirstconnection]) <!-- agent-doc:waiting-item -->
    - **Response:** Provide Craig actual Unity game absolute path, branch and HEAD, Unity version and named target, sanitized environment/public IDs, exact registered redirect, a second ordinary user, an interactive browser-sign-in operator, and confirm the accepted SDK revision (PR #190 head 97e108c28a75f6601595c4055b1725c99d981c0e or successor). (source: agent:review [#craigunityfirstconnection]) ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T13:45-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Provide Craig actual Unity game absolute path, branch and HEAD, Unity version and named target, sanitized environment/public IDs, exact registered redirect, a second ordinary user, an interactive browser-sign-in operator, and confirm the accepted SDK revision (PR #190 head 97e108c28a75f6601595c4055b1725c99d981c0e or successor). (source: agent:review [#craigunityfirstconnection]) ▫️ <!-- agent-doc:reply -->
          - 📥 moot [#q-b5edacb717a0]
  - [#subagent-operator-request-adr0030-o2-o3-closeout] Obtain and record named-owner O2 privacy/legal and O3 anonymous Essential-event decisions, then decide whether to accept ADR 0030. <!-- agent-doc:waiting-item -->
    - **Response:** Obtain and record named-owner O2 privacy/legal and O3 anonymous Essential-event decisions, then decide whether to accept ADR 0030. ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T13:45-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Obtain and record named-owner O2 privacy/legal and O3 anonymous Essential-event decisions, then decide whether to accept ADR 0030. ▫️ <!-- agent-doc:reply -->
          - 📥 Product (Eddie) [#q-c7db2ecebb84]
  - [#subagent-operator-request-sdkparitygate-scope] Choose whether to retain the six-target SDK parity scope or explicitly narrow it by named targets, rows, and version floors. <!-- agent-doc:waiting-item -->
    - **Response:** Choose whether to retain the six-target SDK parity scope or explicitly narrow it by named targets, rows, and version floors. ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T13:45-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Choose whether to retain the six-target SDK parity scope or explicitly narrow it by named targets, rows, and version floors. ▫️ <!-- agent-doc:reply -->
          - 📥 Keep 6 target SDK parity scope [#q-ee9dbbe60557]
  - [#tddocs85gate] Get haiven-docs PR #85 (accepts ADR 0041) non-author-approved and merged before anyone sets GAME_CREDENTIAL_DURABLE=true; haiven-api #815 already merged into sdk-dev as b1ac8650 ahead of it, so its enablement prerequisite 3 is unmet <!-- agent-doc:waiting-item -->
    - **Response:** Get haiven-docs PR #85 (accepts ADR 0041) non-author-approved and merged before anyone sets GAME_CREDENTIAL_DURABLE=true; haiven-api #815 already merged into sdk-dev as b1ac8650 ahead of it, so its enablement prerequisite 3 is unmet ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T13:45-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Get haiven-docs PR #85 (accepts ADR 0041) non-author-approved and merged before anyone sets GAME_CREDENTIAL_DURABLE=true; haiven-api #815 already merged into sdk-dev as b1ac8650 ahead of it, so its enablement prerequisite 3 is unmet ▫️ <!-- agent-doc:reply -->
          - 📥 Done [#q-85ac1a1ea5ba]
  - ☑️ [#subagent:c7685t43#1] [#5t43] Merge https://github.com/haiven-dev/haiven-infra/pull/318 so main drops the colliding docs-site IAM role tags. <!-- agent-doc:waiting-item -->
    - **Response:** Merge https://github.com/haiven-dev/haiven-infra/pull/318 so main drops the colliding docs-site IAM role tags. ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T21:19-05:00 ▫️ <!-- agent-doc:reply -->
      - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Merge https://github.com/haiven-dev/haiven-infra/pull/318 so main drops the colliding docs-site IAM role tags. ▫️ <!-- agent-doc:reply -->
  - [#subagent-operator-request-craigbot-dm-1793755182] Provide a user-scoped Slack read path for DM D0BM13YETUK message 1793755182.148869, or connect a Slack reader who is in that DM. Do not paste a token into chat. <!-- agent-doc:waiting-item -->
    - **Response:** Provide a user-scoped Slack read path that can call `conversations.history` and `conversations.replies` on DM `D0BM13YETUK` for message ts `1793755182.148869`, then redispatch this same queue item. Do not paste a token into chat. ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-03T20:34-05:00 ▫️ <!-- agent-doc:reply -->
      - **Context:** No Slack MCP read tool is connected. `search_tool` only found agent-doc, tasks, unity, and unreal. No Slack message CLI is on PATH. `/home/brian/.local/bin/slack` is the desktop app launcher, not a history client. ▫️ <!-- agent-doc:reply -->
      - **Question:** Provide a user-scoped Slack read path that can call `conversations.history` and `conversations.replies` on DM `D0BM13YETUK` for message ts `1793755182.148869`, then redispatch this same queue item. Do not paste a token into chat. ▫️ <!-- agent-doc:reply -->
      - **Choices:** ▫️ <!-- agent-doc:reply -->
        - **export-user-read-token**: Put a user token with `im:history` and `im:read` in the worker environment, or add it as a passage item, then redispatch this same queue item. The existing `haiven_pr_status` bot token cannot see this DM. Do not paste the token into chat. ▫️ <!-- agent-doc:reply -->
        - **connect-slack-read-mcp**: Connect a Slack MCP server that can call `conversations.history` and `conversations.replies` as a user who is in `D0BM13YETUK`, then redispatch this same queue item. ▫️ <!-- agent-doc:reply -->
        - **skip**: Close the queue item with the Craigbot message unread. ▫️ <!-- agent-doc:reply -->
        - > **Chat prompt (#chatprompt:c663invoke):** agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Provide a user-scoped Slack read path for DM D0BM13YETUK message 1793755182.148869, or connect a Slack reader who is in that DM. Do not paste a token into chat. ▫️ <!-- agent-doc:reply -->
- ### [#tdoauthcodestream] Check whether the SBX roles worker's printed fragment of a local OAuth callback code (127.0.0.1:34515/oauth/callback?code=eyJ...) reached the livestream; if so, trim that VOD segment <!-- agent-doc:waiting-item -->
  - **Response:** Check whether the SBX roles worker's printed fragment of a local OAuth callback code (127.0.0.1:34515/oauth/callback?code=eyJ...) reached the livestream; if so, trim that VOD segment <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Merge https://github.com/haiven-dev/haiven-infra/pull/318 so main drops the colliding docs-site IAM role tags. <!-- agent-doc:waiting-metadata -->
        - **Action:** Authorize a commit and a haiven-sdk pull request into sdk-dev for #jstelemetryannotation from /home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/jstelemetryannotation on docs/js-telemetry-annotation. <!-- agent-doc:waiting-metadata -->
  - ~~Authorize a commit and a haiven-sdk pull request into sdk-dev for #godotexportpreset from /home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/godotexportpreset on feat/godot-demo-export-presets. <!-- agent-doc:waiting-item --> [#tkzv]~~
    - **Response:** Authorize a commit and a haiven-sdk pull request into sdk-dev for #godotexportpreset from /home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/godotexportpreset on feat/godot-demo-export-presets. <!-- agent-doc:waiting-metadata -->
      - **Action:** Check whether the SBX roles worker's printed fragment of a local OAuth callback code (127.0.0.1:34515/oauth/callback?code=eyJ...) reached the livestream; if so, trim that VOD segment <!-- agent-doc:waiting-metadata -->
  - [#tdhandoffdocrelock] As release owner, decide whether to update the auth.failure row in haiven-sdk docs/game-auth-handoff.md for ADR 0041 and re-lock the digest-pinned 0.1.0-preview integration-guide release metadata <!-- agent-doc:waiting-item -->
    - **Response:** As release owner, decide whether to update the auth.failure row in haiven-sdk docs/game-auth-handoff.md for ADR 0041 and re-lock the digest-pinned 0.1.0-preview integration-guide release metadata <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Authorize a commit and a haiven-sdk pull request into sdk-dev for #godotexportpreset from /home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/godotexportpreset on feat/godot-demo-export-presets. <!-- agent-doc:waiting-metadata -->
    - ☑️ [#subagent:c238handoff#2] Resolve the open operator action to complete the parent item. [#tdhandoffdocrelock] ⟦effect:1⟧
  - [#0xjc] Authorize a commit and a haiven-sdk pull request into sdk-dev for #godotmotdintegration from /home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/godotmotdintegration on feat/godot-demo-motd-integration. <!-- agent-doc:waiting-item -->
    - **Response:** Authorize a commit and a haiven-sdk pull request into sdk-dev for #godotmotdintegration from /home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/godotmotdintegration on feat/godot-demo-motd-integration. <!-- agent-doc:waiting-metadata -->
        - **Action:** As release owner, decide whether to update the auth.failure row in haiven-sdk docs/game-auth-handoff.md for ADR 0041 and re-lock the digest-pinned 0.1.0-preview integration-guide release metadata <!-- agent-doc:waiting-metadata -->
        - 📥 do [#q-c1a250d0313a]
  - [#tdadvprterminalpush] Review and merge agent-doc PR https://github.com/btakita/agent-doc/pull/386 (goal-preset terminal gate, redispatch-same-row; rebased on 927d84657d), then release and make install from the integrated checkout (cites #adversarialprloopterminal) (source: agent:review [#adversarialprloopterminal]) <!-- agent-doc:waiting-item -->
    - **Response:** Review and merge agent-doc PR https://github.com/btakita/agent-doc/pull/386 (goal-preset terminal gate, redispatch-same-row; rebased on 927d84657d), then release and make install from the integrated checkout (cites #adversarialprloopterminal) (source: agent:review [#adversarialprloopterminal]) <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Authorize a commit and a haiven-sdk pull request into sdk-dev for #godotmotdintegration from /home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/godotmotdintegration on feat/godot-demo-motd-integration. <!-- agent-doc:waiting-metadata -->
        - **Action:** Review and merge agent-doc PR https://github.com/btakita/agent-doc/pull/386 (goal-preset terminal gate, redispatch-same-row; rebased on 927d84657d), then release and make install from the integrated checkout (cites #adversarialprloopterminal) (source: agent:review [#adversarialprloopterminal]) <!-- agent-doc:waiting-metadata -->
          - 📥 In progress in agent-doc-bugs.md. close [#q-a2b4221ee27a]
  - [#tddocssbxgolive] Make docs.sbx.haiven.gg live: approve/merge haiven-docs #87 (ADR 0044 sandbox docs site; Dillon + engineering manager) and haiven-infra #317 (S3+CloudFront in SBX), start haiven-infra 'SBX -- Full Run' from GitHub, add Porkbun CNAME docs.sbx -> docs_site cname_answer (TTL 600), set haiven-sdk vars DOCS_SBX_ROLE_ARN/DOCS_SBX_BUCKET/DOCS_SBX_DISTRIBUTION_ID, then get haiven-sdk #194 approved by @tmstorm or @pickuperast and merge <!-- agent-doc:waiting-item --> ⟦effect:2⟧
    - **Response:** Make docs.sbx.haiven.gg live: approve/merge haiven-docs #87 (ADR 0044 sandbox docs site; Dillon + engineering manager) and haiven-infra #317 (S3+CloudFront in SBX), start haiven-infra 'SBX -- Full Run' from GitHub, add Porkbun CNAME docs.sbx -> docs_site cname_answer (TTL 600), set haiven-sdk vars DOCS_SBX_ROLE_ARN/DOCS_SBX_BUCKET/DOCS_SBX_DISTRIBUTION_ID, then get haiven-sdk #194 approved by @tmstorm or @pickuperast and merge <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Make docs.sbx.haiven.gg live: approve/merge haiven-docs #87 (ADR 0044 sandbox docs site; Dillon + engineering manager) and haiven-infra #317 (S3+CloudFront in SBX), start haiven-infra 'SBX -- Full Run' from GitHub, add Porkbun CNAME docs.sbx -> docs_site cname_answer (TTL 600), set haiven-sdk vars DOCS_SBX_ROLE_ARN/DOCS_SBX_BUCKET/DOCS_SBX_DISTRIBUTION_ID, then get haiven-sdk #194 approved by @tmstorm or @pickuperast and merge <!-- agent-doc:waiting-metadata -->
          - 📥 Merged. Make site live [#q-bfed81caa65e]
  - [#tdclireleasedownloads] Decide how private haiven-cli serves release downloads (make the repo public, add token support to install.sh/install.ps1/pin, or a public mirror via HAIVEN_DOWNLOAD_BASE), and confirm the haiven-release GitHub App is installed on haiven-cli (release-please must publish with the App token for the release-binaries workflow to trigger) <!-- agent-doc:waiting-item -->
    - **Response:** Decide how private haiven-cli serves release downloads (make the repo public, add token support to install.sh/install.ps1/pin, or a public mirror via HAIVEN_DOWNLOAD_BASE), and confirm the haiven-release GitHub App is installed on haiven-cli (release-please must publish with the App token for the release-binaries workflow to trigger) <!-- agent-doc:waiting-metadata -->
    - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
    - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Decide how private haiven-cli serves release downloads (make the repo public, add token support to install.sh/install.ps1/pin, or a public mirror via HAIVEN_DOWNLOAD_BASE), and confirm the haiven-release GitHub App is installed on haiven-cli (release-please must publish with the App token for the release-binaries workflow to trigger) <!-- agent-doc:waiting-metadata -->
          - 📥 haiven-cli will be public + open source [#q-f09f08bcf721]
  - [#tdgodotredirect] Register the Godot demo sign-in redirect http://127.0.0.1:47616/haiven/callback in allowedRedirectUris for the sbx game integration (haiven-api GAME_AUTH_INTEGRATIONS), or pick an already-registered port for HAIVEN_DEMO_REDIRECT_URI (#godotdemosignin) <!-- agent-doc:waiting-item -->
    - **Response:** Register the Godot demo sign-in redirect http://127.0.0.1:47616/haiven/callback in allowedRedirectUris for the sbx game integration (haiven-api GAME_AUTH_INTEGRATIONS), or pick an already-registered port for HAIVEN_DEMO_REDIRECT_URI (#godotdemosignin) <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Register the Godot demo sign-in redirect http://127.0.0.1:47616/haiven/callback in allowedRedirectUris for the sbx game integration (haiven-api GAME_AUTH_INTEGRATIONS), or pick an already-registered port for HAIVEN_DEMO_REDIRECT_URI (#godotdemosignin) <!-- agent-doc:waiting-metadata -->
          - 📥 Use haiven-frontend standard http ports. Add a haiven-frontend PR if necessary. [#q-555e7dac42af]
  - [#tdapitelemetrybase] Decide for the #apitelemetryrequired haiven-api change (fix/telemetry-annotation-required-fields, uncommitted): PR base sdk-dev (CONTRIBUTING § SDK work) or dev, and whether to add a new gaming v0 surface revision after 0.3.0-preview.1 for the property-became-required break; then say commit + push + PR to start the publish → contracts PR → pin-advance chain <!-- agent-doc:waiting-item -->
    - **Response:** Decide for the #apitelemetryrequired haiven-api change (fix/telemetry-annotation-required-fields, uncommitted): PR base sdk-dev (CONTRIBUTING § SDK work) or dev, and whether to add a new gaming v0 surface revision after 0.3.0-preview.1 for the property-became-required break; then say commit + push + PR to start the publish → contracts PR → pin-advance chain <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Decide for the #apitelemetryrequired haiven-api change (fix/telemetry-annotation-required-fields, uncommitted): PR base sdk-dev (CONTRIBUTING § SDK work) or dev, and whether to add a new gaming v0 surface revision after 0.3.0-preview.1 for the property-became-required break; then say commit + push + PR to start the publish → contracts PR → pin-advance chain <!-- agent-doc:waiting-metadata -->
          - 📥 Target sdk-dev. Keep 0.3.0-preview.1 [#q-a3abacb5f56c]
  - Before 2026-12-02T14:24:21Z, log in to desktop Unity Hub so the cachyos-unity-2022 runner Unity Personal license can be renewed, or provide a self-renewing license (Pro/Plus serial or floating license server) <!-- agent-doc:waiting-item --> ⟦effect:1⟧ [#q-0057ac2f0f7c]
  - Before 2026-12-02T14:24:21Z, log in to desktop Unity Hub so the cachyos-unity-2022 runner Unity Personal license can be renewed, or provide a self-renewing license (Pro/Plus serial or floating license server) <!-- agent-doc:waiting-item --> [#tdunityrunner20261202] ⟦effect:2⟧ [#subagent-operator-request-unity-runner-license]
    - **Response [#r-6b754fc3918c]**
      - **Timestamp:** 2026-11-04T01:03:30.000Z
      - **Summary:** **Deferred:** the operator logs in to desktop Unity Hub before 2026-12-02T14:24:21Z, or provides a self-renewing license (Pro/Plus serial or floating license server). Log in to desktop Unity Hub before 2026-12-02T14:24:21Z so the cachyos-unity-2022 runner Unity Personal license renews, or provide a self-renewing license (Pro/Plus serial or floating license server).
      - **State:** Waiting on operator input.
      - **Guidance:** More work remains. Keep this tree and answer the operator request below.
      - **Choices:**
        - **Request**: [#subagent-operator-request-unity-runner-license]
        - **Log in to desktop Unity Hub**: sign in before 2026-12-02T14:24:21Z so the cachyos-unity-2022 Unity Personal license can be renewed.
        - **Provide a self-renewing license**: supply a Pro/Plus serial or a floating license server so the runner can renew without a desktop Unity Hub login.
    - **Response:** Before 2026-12-02T14:24:21Z, log in to desktop Unity Hub so the cachyos-unity-2022 runner Unity Personal license can be renewed, or provide a self-renewing license (Pro/Plus serial or floating license server) <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Before 2026-12-02T14:24:21Z, log in to desktop Unity Hub so the cachyos-unity-2022 runner Unity Personal license can be renewed, or provide a self-renewing license (Pro/Plus serial or floating license server) <!-- agent-doc:waiting-metadata -->
    - 📥 Complete the remaining work for the parent item. ⟦effect:3⟧ [#q-0a0d75ffd699]
  - Craig accepts the pending Read invitations on haiven-sdk (341074422) and haiven-cli (341074420). Active permission stays none until both are accepted. <!-- agent-doc:waiting-item --> [#craig-accept-read]
    - **Response:** Craig accepts the pending Read invitations on haiven-sdk (341074422) and haiven-cli (341074420). Active permission stays none until both are accepted. <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Craig accepts the pending Read invitations on haiven-sdk (341074422) and haiven-cli (341074420). Active permission stays none until both are accepted. <!-- agent-doc:waiting-metadata -->
    - 📥 Complete the remaining work for the parent item. ⟦effect:1⟧ [#q-26e30d0d3cf4]
- ### ☑️ [#subagent:openquestions#1] Draft a consolidate list of Open Questions for Dillon + Eddie in a md file.  [#q-21230b62be31]⟦local⟧
<!-- /agent:run -->

## Admin <!-- agent:run id="admin" subagents priority -->
- ### ~~What items are done? [#q-5c14697df69f]~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-11-02T06:31:40.000Z⟧
  - **Closed by:** [#r-3b817c02c2e8]
<!-- /agent:run -->

## Todo <!-- agent:todo priority -->
- [ ] [#tdprovauth] Implement and reconcile accepted ADR 0041 durable game credentials across docs, API, contracts, and maintained SDKs: update the source status to accepted; remove the fixed 12-hour lifetime; complete issuance/logout serialization and request-time logout-watermark checks; retain valid credentials across reconnect and transport loss; and implement secure cross-restart storage or host injection. Anonymous (unconnected) interactions are supported on every surface per proposed ADR 0046 (haiven-docs PR #88), using the distinct chat-only credential (#implementanonymouschat). Keep HAVN-1178 install-ID privacy/legal work separate. (source: agent:review [#tdprovauth])
- [ ] [#sdkparitygate] Complete and verify full parity across Browser, Node, React Native, Unity, Godot, and Unreal; Roblox/Luau is excluded. Require target-native proof for all applicable normative protocol and shared product rows, plus install/configure/connect docs, samples, supported-version evidence, and exact-SHA qualification. (source: agent:backlog [#sdkparitygate])
  - %% parked %%
- [ ] [#subagent-operator-request-slack-stg-rollout] Choose the STG human operator identity and authorize the staged Slack PR status rollout
  - %% 11/1 %% 
- [ ] [#deploy-schedule-prereqs] Arrange non-author approval for haiven-tools pull request 27, authenticate an authorized STG AWS session for targeted schedule deployment and four SSM updates, then arrange non-author approval for revendored haiven-infra pull request 307. (source: agent:queue [#subagent-operator-request-deploy-schedule-prereqs])
  - %% 11/1 %%
- [ ] [#subagent-operator-request-adr0041-auth-owner-acceptance] ADR 0041 credential storage: hybrid-cap-inject chosen 2026-11-01 (SDK store seam in progress). Still to choose: the haiven-api live-credential cap N (5 or 10) and eviction rule (oldest-issued or least-recently-used), plus the recipients and approved channel for the ADR 0041 brief.
- [ ] [#tdcraigunityhandoff] Record the Monday 2026-11-02 public-Alpha decision: recommended no-go on a public v0.1.0-alpha.1 tag (keep Craig on the internal-evaluation ring at a pinned full SHA), or set a cutoff for a conditional go. Record Brian (@btakita) as release owner and Craig as the independent clean-room tester and go/no-go co-signer; name the non-author approver (@tmstorm or @pickuperast) for the sdk-dev to main promotion and Release Please PRs, the Alpha report owner, and who approves invited external projects' access to the private haiven-sdk repo.
  - %% 11/2 %%
- [ ] [#subagent-operator-request-craigsbx-webgl-scope] Decide whether Spectre/WebGL remains in Craig SBX scope; if yes, provide the exact deployed HTTPS origin and project revision for narrow redirect/CORS/WebSocket configuration, otherwise record explicit deferral.
- [ ] [#subagent-operator-request-craigunityfirstconnection-inputs] Provide Craig actual Unity game absolute path, branch and HEAD, Unity version and named target, sanitized environment/public IDs, exact registered redirect, a second ordinary user, an interactive browser-sign-in operator, and confirm the accepted SDK revision (PR #190 head 97e108c28a75f6601595c4055b1725c99d981c0e or successor). (source: agent:review [#craigunityfirstconnection])
- [ ] [#subagent-operator-request-adr0030-o2-o3-closeout] Obtain and record named-owner O2 privacy/legal and O3 anonymous Essential-event decisions, then decide whether to accept ADR 0030.
- [ ] [#subagent-operator-request-sdkparitygate-scope] Choose whether to retain the six-target SDK parity scope or explicitly narrow it by named targets, rows, and version floors.
- [ ] [#tddocs85gate] Get haiven-docs PR #85 (accepts ADR 0041) non-author-approved and merged before anyone sets GAME_CREDENTIAL_DURABLE=true; haiven-api #815 already merged into sdk-dev as b1ac8650 ahead of it, so its enablement prerequisite 3 is unmet
- [ ] [#tdoauthcodestream] Check whether the SBX roles worker's printed fragment of a local OAuth callback code (127.0.0.1:34515/oauth/callback?code=eyJ...) reached the livestream; if so, trim that VOD segment
- [ ] [#tdsdkreleaseapp] Ask an haiven-dev org owner (tmstorm recommended; others: oceanwhit, ErikRDavis, lukerichey-haiven, edwardhaiven) to add haiven-sdk to the haiven-release GitHub App's repository access (org settings → Installations → haiven-release → Configure), then re-run haiven-sdk run 40635565681
- [ ] [#tdhandoffdocrelock] As release owner, decide whether to update the auth.failure row in haiven-sdk docs/game-auth-handoff.md for ADR 0041 and re-lock the digest-pinned 0.1.0-preview integration-guide release metadata
- [ ] [#tdcontractsscenariospush] Decide whether to bump the haiven-contracts gaming-auth-handoff/v0 suite revision 0.1.0 -> 0.2.0 for the null-expiry/resume scenarios (push + PR authorized 2026-11-01 via #push). SDK runners need the 4 new step verbs before the SDK pin advances past it
- [ ] [#tdadvprterminalpush] Review and merge agent-doc PR https://github.com/btakita/agent-doc/pull/386 (goal-preset terminal gate, redispatch-same-row; rebased on 927d84657d), then release and make install from the integrated checkout (cites #adversarialprloopterminal) (source: agent:review [#adversarialprloopterminal])
- [ ] [#tddocssbxgolive] Make docs.sbx.haiven.gg live: approve/merge haiven-docs #87 (ADR 0044 sandbox docs site; Dillon + engineering manager) and haiven-infra #317 (S3+CloudFront in SBX), start haiven-infra 'SBX -- Full Run' from GitHub, add Porkbun CNAME docs.sbx -> docs_site cname_answer (TTL 600), set haiven-sdk vars DOCS_SBX_ROLE_ARN/DOCS_SBX_BUCKET/DOCS_SBX_DISTRIBUTION_ID, then get haiven-sdk #194 approved by @tmstorm or @pickuperast and merge
- [ ] [#tdadr0046approvers] Name the approver(s) for haiven-docs PR #88 (ADR 0046, Haiven rules for unknown players; candidates Luke Richey, Dillon Storm, plus Eddie as owner of the overturned sign-in rule), request their review, and ask the ADR 0036 (sanat@) and ADR 0040 owners to replace their restated signed-out rules with a link to ADR 0046
- [ ] [#tdclireleasedownloads] Decide how private haiven-cli serves release downloads (make the repo public, add token support to install.sh/install.ps1/pin, or a public mirror via HAIVEN_DOWNLOAD_BASE), and confirm the haiven-release GitHub App is installed on haiven-cli (release-please must publish with the App token for the release-binaries workflow to trigger)
- [ ] [#tdgodotredirect] Register the Godot demo sign-in redirect http://127.0.0.1:47616/haiven/callback in allowedRedirectUris for the sbx game integration (haiven-api GAME_AUTH_INTEGRATIONS), or pick an already-registered port for HAIVEN_DEMO_REDIRECT_URI (#godotdemosignin)
- [ ] [#tdapitelemetrybase] Decide for the #apitelemetryrequired haiven-api change (fix/telemetry-annotation-required-fields, uncommitted): PR base sdk-dev (CONTRIBUTING § SDK work) or dev, and whether to add a new gaming v0 surface revision after 0.3.0-preview.1 for the property-became-required break; then say commit + push + PR to start the publish → contracts PR → pin-advance chain
- [ ] [#subagent-operator-request-tdcraigunityhandoff-d1d4] Name Alpha release owner, report owner, external-access approver, cutoff, and D1–D3 before any Monday prerelease. No tag was cut.
- [ ] [#subagent-operator-request-adr0046-sibling-pick] Pick one ADR 0046 sibling: Done as-is, Add Dillon by follow-up PR, or say how to record Dillon's role.
- [ ] [#tdunityrunner20261202] Before 2026-12-02T14:24:21Z, log in to desktop Unity Hub so the cachyos-unity-2022 runner Unity Personal license can be renewed, or provide a self-renewing license (Pro/Plus serial or floating license server)
- [ ] [#5t43] Merge https://github.com/haiven-dev/haiven-infra/pull/318 so main drops the colliding docs-site IAM role tags.
- [x] [#subagent-operator-request-unity-runner-license] Answer the blocking subagent question for tracked work tdunityrunner20261202 for queue item ‘Before 2026-12-02T14:24:21Z, log in to desktop Unity Hub so the cachyos-unity-2022 runner Unity Personal license can be renewed, or provide a self-renewing license (Pro/Plus serial or floating…’ in agent:exchange: Log in to desktop Unity Hub before 2026-12-02T14:24:21Z so the cachyos-unity-2022 runner Unity Personal license renews, or provide a self-renewing license (Pro/Plus serial or floating license server). (choices: Log in to desktop Unity Hub, Provide a self-renewing license)
- [ ] [#subagent-operator-request-cli-release-please-sdk-dev] Choose the haiven-cli Release Please next step: a doc-fix pull request into sdk-dev, that plus an unmerged sdk-dev to main promotion pull request, a promotion pull request only, or stop.
- [ ] [#subagent-operator-request-craigbot-dm-1793755182] Provide a user-scoped Slack read path for DM D0BM13YETUK message 1793755182.148869, or connect a Slack reader who is in that DM. Do not paste a token into chat.
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue="main" -->
- [ ] ☑️ [#q-c4dbe51b8e36] Is this complete?
- [ ] [#q-c1a250d0313a] Update the `auth.failure` row and re-lock at the current version.
- [ ] [#clidocsreleasecleanup] docs site haiven CLI page (haiven-sdk site/docs/install/haiven-cli.md, PR #195): after haiven-cli's first published release, remove the caution box and the build-from-source fallback and point the one-liners at the real release (stays open: blocked behind #tdclireleasedownloads)
- [/] [#sdkactionscapacity] [blocked: GitHub Actions organization budget and artifact storage capacity] Restore the haiven-sdk Actions budget and artifact-storage capacity, wait for quota recalculation, then rerun the required exact-head hosted checks for SDK PRs #189, #190, and #191. (todos: [#tdsdkactionscapacity])
- [/] [#sdkparitygate] [operator-verify] [blocked: shared baseline + engine evidence gaps] #139/#141/#142/#143 are not parity. Gaps: Unity Boss Room current-source, identity UI, Unity 6/Git-URL install; Unreal partial account linking; Godot browser linking, samples, export targets; Roblox published realtime, real Studio/Open Cloud, package. No parity claim until each is met or narrowed. (todos: [#tdsdkparitygate])
- [/] [#sdkadvconformancegate] [operator-verify] [blocked: real Roblox + Windows evidence] #136 and #137 are merged and their prior branch gates are satisfied. Remaining: real Roblox HttpService decoder proof (#quirkrobloxaccess) and a Windows PYTHONUTF8=0 path run, then refresh the evidence. (todos: [#tdsdkadvconformancegate])
<!-- /agent:backlog -->

## Parked <!-- agent:run id="parked" subagents go -->
- ~~[#subagent-operator-request-havn1177-release-owners] [#subagent-operator-request-chat-policy-supersession]~~
  - %% tracking web ui work %%
<!-- /agent:run -->

## Parked <!-- agent:run id="parked-waiting" -->
- ### 📥 Waiting on you — answer by adding a child line under an item
  - 🙋 Choose the STG human operator identity and authorize the staged Slack PR status rollout <!-- agent-doc:waiting-item --> [#subagent-operator-request-slack-stg-rollout]
    - **Response:** Choose the STG human operator identity and authorize the staged Slack PR status rollout <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Choose the STG human operator identity and authorize the staged Slack PR status rollout <!-- agent-doc:waiting-metadata -->
          - %% parked %%
  - 🙋 Arrange non-author approval for haiven-tools pull request 27, authenticate an authorized STG AWS session for targeted schedule deployment and four SSM updates, then arrange non-author approval for revendored haiven-infra pull request 307. (source: agent:queue [#subagent-operator-request-deploy-schedule-prereqs]) <!-- agent-doc:waiting-item --> [#deploy-schedule-prereqs]
    - **Response:** Arrange non-author approval for haiven-tools pull request 27, authenticate an authorized STG AWS session for targeted schedule deployment and four SSM updates, then arrange non-author approval for revendored haiven-infra pull request 307. (source: agent:queue [#subagent-operator-request-deploy-schedule-prereqs]) <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Arrange non-author approval for haiven-tools pull request 27, authenticate an authorized STG AWS session for targeted schedule deployment and four SSM updates, then arrange non-author approval for revendored haiven-infra pull request 307. (source: agent:queue [#subagent-operator-request-deploy-schedule-prereqs]) <!-- agent-doc:waiting-metadata -->
          - %% parked %%
  - 🙋 Decide whether Spectre/WebGL remains in Craig SBX scope; if yes, provide the exact deployed HTTPS origin and project revision for narrow redirect/CORS/WebSocket configuration, otherwise record explicit deferral. <!-- agent-doc:waiting-item --> [#subagent-operator-request-craigsbx-webgl-scope]
    - **Response:** Decide whether Spectre/WebGL remains in Craig SBX scope; if yes, provide the exact deployed HTTPS origin and project revision for narrow redirect/CORS/WebSocket configuration, otherwise record explicit deferral. <!-- agent-doc:waiting-metadata -->
      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->
      - **Next actions:** <!-- agent-doc:waiting-metadata -->
        - **Action:** Decide whether Spectre/WebGL remains in Craig SBX scope; if yes, provide the exact deployed HTTPS origin and project revision for narrow redirect/CORS/WebSocket configuration, otherwise record explicit deferral. <!-- agent-doc:waiting-metadata -->
          - %% parked %%
<!-- /agent:run -->
