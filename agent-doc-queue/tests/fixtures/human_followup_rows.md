---
agent_doc_session: 00000000-0000-4000-8000-000000000000
agent_doc_format: template
agent_doc_write: crdt
agent: claude
queue: go
---

<!-- Representative live rows copied verbatim from a session document whose
     Stop hook looped over human-owned follow-up rows (#humanfollowupowner,
     2026-11-13). -->

## Exchange <!-- agent:exchange patch=append -->
No worker cells.
<!-- /agent:exchange -->

## haiven-docs-prs  <!-- agent:run id="haiven-docs-prs" subagents preset="#push" keep -->
- ### https://github.com/haiven-dev/haiven-docs/pull/98
  - %% docs(standards): permit SDK workflow promotion %%
	- #adversarial-pr [#q-4ae8c3d35ca1]
- ### https://github.com/haiven-dev/haiven-docs/pull/100
  - %% ci(docs): gate pending Markdown documents %%
  - ~~We should be pinning on `haiven-dev/haiven-tools/.github/workflows/pr-automation.yml@main`, not a commit sha. What needs to be done to ensure `haiven-dev/haiven-tools/.github/workflows/pr-automation.yml@main` + addressing the review? ⟦published⟧~~
- ### 📥 follow up on an action item from queue item “We should be pinning on `haiven-dev/haiven-tools/.github/workflows/pr-automation…”: tmstorm needs to re-review and approve PR #100 at head `38075e75137be394ddc736024dd02f1ef5c33d71`. [#q-fe6b724a7a1a]
- ### https://github.com/haiven-dev/haiven-docs/pull/101
  - %% docs(decisions): propose ADR 0052 on game pricing and regional prices %%
<!-- /agent:run -->

## haiven-api-prs  <!-- agent:run id="haiven-api-prs" subagents preset="#ip: #push" keep -->
- ### https://github.com/haiven-dev/haiven-api/pull/855
  - %% chore: promote sdk-dev into dev %%
  - ~~#rebase [#q-e07fde6a7c8b]~~
- 📥 follow up on an action item from `q-e07fde6a7c8b`: A non-author approves https://github.com/haiven-dev/haiven-api/pull/855 at head `97aa07c5` once the `preflight` check is green. [#q-4e308828ff85]
- ### https://github.com/haiven-dev/haiven-api/pull/893
  - %% ci(promotion): keep the dev to main release-day pull request open %%
- ### 📥 follow up on an action item from queue item “Now that https://github.com/haiven-dev/haiven-sdk/pull/232 is merged, can we do …”: An org owner makes `HAIVEN_RELEASE_APP_ID` and `HAIVEN_RELEASE_APP_PRIVATE_KEY` available to haiven-sdk-unity and haiven-sdk-unity-bossroom (failing now) and every public target, and confirms the haiven-release app is installed on all 9 targets with contents and pull-request write access. [#q-f53ecfa1f90c]
- 📥 A non-author (tmstorm) approves https://github.com/haiven-dev/haiven-sdk/pull/245 at head `02a39a79` (adds only `promote-public-targets.yml` to `main`); once approved and checks are green, squash-merge it. [#q-aab2966c2f87]
- ### 📥 follow up on an action item from queue item “Now that https://github.com/haiven-dev/haiven-sdk/pull/232 is merged, can we do …”: Decide who owns each public README: move the target README work into `sdks/unity/README.md` and `sdks/unreal/README.md`, or remove `README.md` from those targets' mappings. [#q-32f08c709a10]
- ### ~~follow up on an action item from queue item “Now that https://github.com/haiven-dev/haiven-sdk/pull/232 is merged, can we do …”: Build `sdk_promotion.py status` and the standing "SDK release" PR workflow in haiven-sdk. [#q-6611848c5002]~~
- ### 📥 follow up on an action item from queue item “Now that https://github.com/haiven-dev/haiven-sdk/pull/232 is merged, can we do …”: Run a first promotion as a CI dry-run, then publish it, then let each target's promotion PR and Release Please take over. [#q-0a73d63cfbf2]
- ### ~~During a haiven-sdk P2P session, several players join a game, but they were not able to access the game room. [#q-becb4210adbc]~~
- 📥 follow up on an action item from queue item “During a haiven-sdk P2P session, several players join a game, but they were not …”: Confirm this was the operator's failure: with AWS SSO signed in again, search the relay logs (`/ec2/haiven-<env>-turn`) from the session time for `486` / `Allocation Quota Reached`. Also ask the operator which environment it was, how many players joined, and whether the room was public. [#q-b63cca3b38b1]
- 📥 follow up on an action item from queue item “During a haiven-sdk P2P session, several players join a game, but they were not …”: Get a non-author approval on https://github.com/haiven-dev/haiven-infra/pull/336, merge it, and apply it from GitHub in `sbx` and `stg` while few sessions are live. The config change restarts the relay and drops every relayed connection. [#q-c5654f8354e1]
- 📥 follow up on an action item from queue item “During a haiven-sdk P2P session, several players join a game, but they were not …”: Get a non-author approval on https://github.com/haiven-dev/haiven-api/pull/896 and merge it into `dev`. [#q-e3886692df1d]
- 📥 follow up on an action item from queue item “During a haiven-sdk P2P session, several players join a game, but they were not …”: Fix the same wrong assumption in ADR 0036 in haiven-docs: the quota section and summary row 25 say the quota bounds one issuance. [#q-f7989716012d]
<!-- /agent:run -->

## haiven-sdk-prs  <!-- agent:run id="haiven-sdk-prs" subagents preset="#ip: #push" keep priority -->
- ### https://github.com/haiven-dev/haiven-sdk/pull/226
  - %% ci: run the sub-minute Linux checks as one job %%
  - %% author: jeffason %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/231
  - %% fix(version): reject empty SDK stamp %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/236
  - %% feat(release): promote the JavaScript SDK to haiven-sdk-js %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/237
  - %% test(p2p): prove the session chat channel filter in gaming-p2p-client/v0 %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/238
  - %% fix(chat): hold session chat sends until the participation resumes %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/239
  - %% fix(unreal): recheck the player before loot for a settled saved claim %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/240
  - %% feat(bossroom): port the public Boss Room work and keep its Unity 6 files target-owned %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/241
  - %% feat(release): coalesce promotion PRs and make them release %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/242
  - %% fix(sdk): port public-only target edits into the source tree %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/243
  - %% fix(release): keep private references out of promoted SDK files %%
- ### https://github.com/haiven-dev/haiven-sdk/pull/246
  - %% ci(release): head the sdk-dev promotion PR with a promote/sdk-dev mirror %%
- 📥 follow up on an action item from queue item “Why does the haiven-actions promote-pr delete the branch? Can we keep the branch…”: A haiven-sdk repo admin stops GitHub deleting `sdk-dev` on merge. btakita has `push` but not `admin` on haiven-sdk, so this needs someone else. Either option works: **Recommended:** create the "Protect sdk-dev" ruleset with a `deletion` restriction on `refs/heads/sdk-dev`. This is the existing queued row; it keeps auto-delete on for normal PRs and covers future long-lived branches through the ruleset. Or turn `delete_branch_on_merge` off for haiven-sdk. That matches every public SDK repo, but normal PR branches then need deleting by hand. [#q-08eb0663d8e3]
- 📥 follow up on an action item from queue item “follow up on an action item from queue item '#address then #adversarial-pr ⟦publ…”: Check that hosted CI on #240 at `34bd2999` finishes green, then merge #240 into `sdk-dev`. Merging was not authorized for this item. [#q-9678969ac9f3]
- 📥 follow up on an action item from queue item “follow up on an action item from queue item '#address then #adversarial-pr ⟦publ…”: After #240 merges, rebase #241, #242 and #243, which edit some of the same files. [#q-8f52ce1e58ff]
- 📥 follow up on an action item from queue item “#address then #adversarial-pr ⟦published⟧”: Decide whether `haiven-contracts`/`haiven-api` mentions, contract-pin headers and HAVN ids count as private. Public `sdk-dev` scrubbed them in unreal `d77aa4d`, unreal-lyra `c117cf6`, unreal-chat `d9854f7` and godot-demo `e8e2ab8`, and promotion puts them back. If they are private: scrub the source, then widen the scanner or wire a sensitive-terms secret into the workflow. [#q-89d680a2fe78]
- 📥 follow up on an action item from queue item “follow up on an action item from queue item '#address then #adversarial-pr ⟦publ…”: Get a human approval on https://github.com/haiven-dev/haiven-sdk/pull/242 from tmstorm or pickuperast, who are already requested. btakita authored it and cannot approve it. [#q-47d1b13aeb50]
- 📥 follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”: Address tmstorm's CHANGES_REQUESTED review on #238 (left on `6879d6ed`, still in effect): https://github.com/haiven-dev/haiven-sdk/pull/238#pullrequestreview-5887945703 [#q-6d5ef8ddcba7]
- 📥 follow up on an action item from queue item “Promotion PR should not delete sdk-dev on merge since sdk-dev is a long-lived br…”: Decide how promotion PRs merge into haiven-sdk `main`, then fix either the repo settings or the text. Merge commits aren't possible today: `allow_merge_commit: false`, and the `main` ruleset requires linear history. Yet the workflow note and CONTRIBUTING.md both say to merge "with a merge commit". With squash, `main` never contains `sdk-dev`, so each new promotion PR will list commits that are already on `main`. [#q-f418c06606c1]
<!-- /agent:run -->

## haiven-sdk-godot-prs <!-- agent:run id="haiven-sdk-godot-prs" subagents preset="#ip: #push" keep -->
- ### https://github.com/haiven-dev/haiven-sdk-godot/pull/6
  - %% fix: remove private repository paths %%
- ### https://github.com/haiven-dev/haiven-sdk-godot/pull/7
  - %% ci(release): export desktop players and publish a source archive %%
- ### https://github.com/haiven-dev/haiven-sdk-godot/pull/9
  - %% chore: promote sdk-dev into main %%
  - ~~#adversarial-pr [#q-fc36a4bdbcd3]~~
    - **Response [#r-7474f3183304]**
      - **Timestamp:** 2026-11-13T23:28:59.000Z
      - **Summary:** REVIEW-CLEAN https://github.com/haiven-dev/haiven-sdk-godot/pull/9#pullrequestreview-5899914623 base=1e4d0ba251d4f2ad4dd3819bfd044cdf9dfe2fb9 head=28ec3b170f92bd36983eabd76735ff67ca84c3ac cycles=4
      - **State:** Complete.
      - **Guidance:** This row is complete; the item is complete and has no remaining tracked or unpublished work. Its nested rows and tree were not checked, so confirm they are struck before deleting it.
- 📥 follow up on an action item from `q-fc36a4bdbcd3`: Approve and merge https://github.com/haiven-dev/haiven-sdk-godot/pull/9 with a merge commit (not squash or rebase). It is MERGEABLE and blocked only by REVIEW_REQUIRED. [#q-cbfe04f527a4]
- 📥 follow up on an action item from `q-fc36a4bdbcd3`: Decide whether the first release-please release ships as stable `0.1.0` (marked Latest) or as a preview. A `v0.1.0-preview.1` pre-release with older addon contents already exists; for a preview, add a `Release-As: 0.1.0-preview.2` footer before merging the release PR. [#q-6ea401c72701]
- 📥 follow up on an action item from `q-fc36a4bdbcd3`: Make the Release workflow's `Build release assets` job a required check on `main`, or add "merge only after Release passes" to the README release steps. Without either, a red release PR can still merge and produce a Latest release with no assets, and `README.md` currently says the build "fails before anything is published". [#q-e3f5efeac02e]
- 📥 follow up on an action item from `q-fc36a4bdbcd3`: Either change the Python requirement in `README.md` and `ci/package_release.py` from 3.9+ to 3.10+, or drop the `Path.write_text(newline=)` call that needs 3.10. This predates this PR. [#q-850420ff17e2]
- 📥 follow up on an action item from `q-fc36a4bdbcd3`: Update the upstream haiven-sdk generator and API descriptions to emit the same public wording as `02bf02e`, or the next addon sync brings the non-public references back. [#q-750cb82fd457]
- 📥 follow up on an action item from queue item “follow up on an action item from `adversarial-pr`: Add an `sdk-dev` ruleset that…”: A haiven-sdk-godot admin creates the `sdk-dev` ruleset from `/home/user/worktrees/haiven-dev/haiven-sdk-godot/sdk-dev-ruleset/.local/sdk-dev-ruleset.json`. [#q-ea9eca4f8dac]
- 📥 follow up on an action item from `adversarial-pr`: After the first release, confirm that `release.yml` ran on the `v0.1.0` tag and attached both zips and `SHA256SUMS`. [#q-d62b170ddc86]
<!-- /agent:run -->

## haiven-sdk-js-prs <!-- agent:run id="haiven-sdk-js-prs" subagents preset="#ip: #push" keep -->
- ### https://github.com/haiven-dev/haiven-sdk-js/pull/2
  - %% docs(contributing): invite community contributions %%
- 📥 follow up on an action item from queue item “Add Apache-2 license [#q-364356ca1e53] ⟦published⟧”: Get a non-author approval on https://github.com/haiven-dev/haiven-sdk-js/pull/2 at the new head `42152a0` (btakita cannot approve). Any approval given at `6dd0abe` no longer covers the current head.
- 📥 follow up on an action item from queue item “#address then #adversarial-pr [#q-0ddbdb248b70] ⟦published⟧”: Retarget https://github.com/haiven-dev/haiven-sdk-js/pull/2 from base `main` to `sdk-dev`, per today's rule that SDK PRs target `sdk-dev`. Alternatively, explicitly accept `main` and merge `main` back into `sdk-dev` right after.
- 📥 follow up on an action item from queue item “#address then #adversarial-pr [#q-0ddbdb248b70] ⟦published⟧”: Get an approval on https://github.com/haiven-dev/haiven-sdk-js/pull/2 at head `6dd0abe` from someone other than the author; btakita cannot approve.
- ### https://github.com/haiven-dev/haiven-sdk-js/pull/5
  - %% ci(release): dispatch sdk-dev promotion from main %%
- ### 🙋⏳ follow up on an action item from queue item “haiven-sdk-js: Setup Release Please workflow to merge sdk-dev into main: see htt…”: Obtain an eligible non-author approval on PR 5 at exact head 4f558009338ac60aee9530f53c6a5595809ce72c. [#subagent-operator-request-js-pr5-review]
  - **Summary:** Arrange tmstorm, pickuperast, or another eligible non-author to approve haiven-sdk-js PR 5 at exact head 4f558009338ac60aee9530f53c6a5595809ce72c.
    - **Context:** The PR is open and non-draft with REVIEW_REQUIRED. Exact-head Analyze and CodeQL checks pass. The authenticated account authored the PR and cannot self-approve.
    - **Recommendation:** **Request tmstorm approval** — tmstorm is eligible to provide the exact-head review.
    - **Choices:**
      - **Request**: [#subagent-operator-request-js-pr5-review]
      - **Request tmstorm approval**: tmstorm approves the exact head. — **Recommended**
      - **Use another eligible reviewer**: another non-author approves.
      - **Leave PR blocked**: keep PR 5 open.
<!-- /agent:run -->

## haiven-cli-prs  <!-- agent:run id="haiven-cli-prs" subagents preset="#ip: #push" keep -->
- ### https://github.com/haiven-dev/haiven-cli/pull/16
  - %% chore: promote sdk-dev to main %%
- ### https://github.com/haiven-dev/haiven-cli/pull/18
  - %% ci(release): dispatch sdk-dev promotion from main %%
  - ~~#adversarial-pr  [#q-83f9a79acdf1]⟦published⟧~~
- 📥 follow up on an action item from `adversarial-pr`: Someone other than the author approves and merges PR 18 at exact head `b50633a4b2ebc99e6acc4073976c3447ac18a113`. This replaces the earlier action item that named head `be4162c`. [#q-6cd633c0b2fa]
- 📥 follow up on an action item from `adversarial-pr`: Add a byte-identical copy of `.github/workflows/promote-sdk-dev.yml` (from head `b50633a`) to `sdk-dev` before the first dispatch, and update the manual promotion and `BEGIN_COMMIT_OVERRIDE` section of `sdk-dev`'s `CONTRIBUTING.md` in the same change. Until then every dispatch is refused on purpose. [#q-a050a82beac9]
- 📥 follow up on an action item from `adversarial-pr`: After PR 18 and the `sdk-dev` copy land, dispatch `promote sdk-dev` with the exact live `sdk-dev` head at that time (currently `89527b1f3638441db1320c316ac9178c39d1c853`). The earlier action item's `b5f3f7c0…` is stale. Then close PR 16 by hand, since the workflow does not close it. [#q-fda9800f03c0]
- 📥 follow up on an action item from queue item “haiven-cli should be added to haiven-release”: An org owner adds haiven-cli (repo id `1413929956`) to the haiven-release installation if it uses selected repositories. In the UI: https://github.com/organizations/haiven-dev/settings/installations → haiven-release → Configure → Repository access → Only select repositories → add `haiven-cli` → Save. By API: `id=$(gh api orgs/haiven-dev/installations --jq '.installations[]|select(.app_slug=="haiven-release").id'); gh api -X PUT user/installations/$id/repositories/1413929956`. [#q-be6a36198e47]
- 📥 follow up on an action item from queue item “haiven-cli should be added to haiven-release”: An org owner gives the haiven-release app Repository permission → Workflows: Read and write. Path: https://github.com/organizations/haiven-dev/settings/apps/haiven-release/permissions → Save. Then accept the new permissions on the haiven-release installation page (Review request → Accept). [#q-6c4b80169930]
- 📥 follow up on an action item from queue item “haiven-cli should be added to haiven-release”: An org owner adds haiven-cli to the repository access of org variable `HAIVEN_RELEASE_APP_ID`. In the UI: Org Settings → Secrets and variables → Actions → Variables → HAIVEN_RELEASE_APP_ID → Repository access. By API: `gh api -X PUT orgs/haiven-dev/actions/variables/HAIVEN_RELEASE_APP_ID/repositories/1413929956`. [#q-4a314bae3b34]
- 📥 follow up on an action item from queue item “haiven-cli should be added to haiven-release”: An org owner adds haiven-cli to the repository access of org secret `HAIVEN_RELEASE_APP_PRIVATE_KEY`, under the Secrets tab in the same UI. By API: `gh api -X PUT orgs/haiven-dev/actions/secrets/HAIVEN_RELEASE_APP_PRIVATE_KEY/repositories/1413929956`. Then verify with `gh api repos/haiven-dev/haiven-cli/actions/organization-variables --jq '.variables[].name'` and `.../organization-secrets --jq '.secrets[].name'`. [#q-832fc66b934a]
- ### 🙋⏳ follow up on an action item from queue item “haiven-cli: Setup Release Please workflow to merge sdk-dev into main: see https:…”: An eligible non-author reviewer approves and merges PR 18 at exact head be4162cffbf76cc33406c0d82900fdf9bd50ee83. [#subagent-operator-request-cli-pr18-review]
  - **Summary:** Arrange an eligible non-author approval of haiven-cli PR 18 at exact head be4162cffbf76cc33406c0d82900fdf9bd50ee83, then allow the authorized squash merge.
    - **Context:** PR 18 has no formal reviews. GitHub reports REVIEW_REQUIRED and BLOCKED, while the PR is mergeable. The authenticated account authored the PR and cannot self-approve.
    - **Recommendation:** **Approve exact head, then merge** — preserve the checked head and satisfy branch protection.
    - **Choices:**
      - **Request**: [#subagent-operator-request-cli-pr18-review]
      - **Approve exact head, then merge**: an eligible reviewer approves, then the authorized squash merge proceeds. — **Recommended**
      - **Leave PR blocked**: keep PR 18 open.
- ### 🙋⏳ follow up on an action item from queue item “haiven-cli: Setup Release Please workflow to merge sdk-dev into main: see https:…”: After PR 18 merges, dispatch promote sdk-dev with exact SHA b5f3f7c0ef3a477943170ba2d781a4033e8faea2; the generated promotion supersedes manual PR 16. [#subagent-operator-request-cli-promotion-prereqs]
  - **Summary:** Complete PR 18 approval and merge plus the haiven-release Workflows permission update, then resume promotion dispatch for exact sdk-dev SHA b5f3f7c0ef3a477943170ba2d781a4033e8faea2.
    - **Context:** Current main does not yet contain the promotion workflow. The GitHub App lacks Workflows permission. The exact sdk-dev commit exists and is ready once both prerequisites land.
    - **Recommendation:** **Complete prerequisites and dispatch** — preserves the exact authorized promotion input.
    - **Choices:**
      - **Request**: [#subagent-operator-request-cli-promotion-prereqs]
      - **Complete prerequisites and dispatch**: merge PR 18, grant permission, then dispatch. — **Recommended**
      - **Leave promotion blocked**: do not dispatch.
<!-- /agent:run -->

## Main <!-- agent:run id="main" subagents preset="#ip" priority -->
- ### ~~[from:coordinator-repair key=previewtagmove-requeue] do [#previewtagmove]~~
- ### ~~I have Demo today at 4PM ET. Can we get the Lyra demo at parity with BossRoom by then? ⟦published⟧~~
- ### 📥 follow up on an action item from queue item “I have Demo today at 4PM ET. Can we get the Lyra demo at parity with BossRoom by…”: Rehearse before 4 PM: PLAY → eliminate a bot → confirm the reward claim and item arrive, against stg. [#q-972ca0bce0b4]
- ### 📥 follow up on an action item from queue item “I have Demo today at 4PM ET. Can we get the Lyra demo at parity with BossRoom by…”: Get a non-author approval and merge https://github.com/haiven-dev/haiven-sdk-unreal-lyra/pull/10. [#q-ce2931c4c5c3]
- ### 📥 follow up on an action item from queue item “I have Demo today at 4PM ET. Can we get the Lyra demo at parity with BossRoom by…”: Get a non-author approval and merge https://github.com/haiven-dev/haiven-sdk/pull/249. [#q-8cc07feef5fd]
<!-- /agent:run -->

## Todo <!-- agent:todo priority -->
- [ ] [#subagent-operator-request-cli-pr18-review] Answer the blocking subagent question for queue item ‘follow up on an action item from queue item “haiven-cli: Setup Release Please workflow to merge sdk-dev into main: see https:…”: An eligible non-author reviewer approves and merges PR 18 at exact…’ in agent:exchange: Arrange an eligible non-author approval of haiven-cli PR 18 at exact head be4162cffbf76cc33406c0d82900fdf9bd50ee83, then allow the authorized squash merge. (choices: Approve exact head, then merge, Leave PR blocked) ⟦decision:dec-3a4cf9dc74⟧
- [ ] [#subagent-operator-request-cli-promotion-prereqs] Answer the blocking subagent permission request for queue item ‘follow up on an action item from queue item “haiven-cli: Setup Release Please workflow to merge sdk-dev into main: see https:…”: After PR 18 merges, dispatch promote sdk-dev with exact SHA…’ in agent:exchange: Complete PR 18 approval and merge plus the haiven-release Workflows permission update, then resume promotion dispatch for exact sdk-dev SHA b5f3f7c0ef3a477943170ba2d781a4033e8faea2. (choices: Complete prerequisites and dispatch, Leave promotion blocked) ⟦decision:dec-43f6e749d8⟧
- [ ] [#subagent-operator-request-js-pr5-review] Answer the blocking subagent question for queue item ‘follow up on an action item from queue item “haiven-sdk-js: Setup Release Please workflow to merge sdk-dev into main: see htt…”: Obtain an eligible non-author approval on PR 5 at exact head…’ in agent:exchange: Arrange tmstorm, pickuperast, or another eligible non-author to approve haiven-sdk-js PR 5 at exact head 4f558009338ac60aee9530f53c6a5595809ce72c. (choices: Request tmstorm approval, Use another eligible reviewer, Leave PR blocked) ⟦decision:dec-d33c6a187e⟧
- [ ] [#subagent-operator-request-docs-pr98-review] Answer the blocking subagent question for queue item ‘follow up on an action item from queue item “follow up on an action item from queue item 'haiven-sdk-godot-demo: Setup Releas…”: Approve and merge PR 98.’ in agent:exchange: Arrange eligible non-author approval of haiven-docs PR 98 at exact head 2ff06ef19dac4fbfd21b723e777b122fd0701a9c, then resume the authorized merge. (choices: Approve exact head, then merge, Leave PR blocked) ⟦decision:dec-dcbfaf00a8⟧
- [ ] [#subagent-operator-request-docs-app-workflows-after-pr98] Answer the blocking subagent permission request for queue item ‘follow up on an action item from queue item “follow up on an action item from queue item 'haiven-sdk-godot-demo: Setup Releas…”: Have an organization owner grant the haiven-release App Workflows…’ in agent:exchange: First approve and merge PR 98 at exact head 2ff06ef19dac4fbfd21b723e777b122fd0701a9c; then have a haiven-dev organization owner grant haiven-release Workflows read/write and approve the installation… (choices: Approve and merge, then grant permission, Leave blocked) ⟦decision:dec-ef046b4742⟧
- [ ] [#subagent-operator-request-gh013-scrub-branch-unpublished] Authorize a non-force push of local scrub/private-refs and pull requests into main on the nine repositories, or leave the branches local. (choices: push-new-branch-then-pr, do-not-push) (stays open: nine scrub/publish-gh013 branches are published and pull requests into main were rejected because the histories do not meet) ⟦decision:dec-15c9c2868c⟧
- [ ] [#subagent-operator-request-plugin-luau-consume] Choose how plugin-roblox consumes the haiven-sdk moderation modules for [#pluginluauport]: Rojo map of server/src, a moderation rbxm, a copy of the four generated sources, or leave the plugin unchanged (source: agent:backlog [#pluginluauport])
<!-- /agent:todo -->
