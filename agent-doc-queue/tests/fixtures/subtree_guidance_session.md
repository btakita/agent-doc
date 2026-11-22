---
agent_doc_session: subtree-guidance-fixture
---

<!-- agent:exchange -->
### Re: Subagent completed: `subagent:more368` returned work for queue item “do [#completetasksplans]” — agent-doc
<!-- agent-doc-subagent-response sha256=6294dccd1aeed44ca682262d4142da20b31d57d473630928223a42065d4ff55c -->
> **Subagent response:**
>
> **Summary**
>
> Yes. Accepting #368 is only the first of several human steps: a promotion, six public merges, GitHub App and key setup, environments, and runner access. Live runs can start only after all of them. No agent work is left before #368 is accepted. #368 is green and current with `sdk-dev`, so it needs no rebase. All six public PRs are still unreviewed drafts. None of the configuration exists yet, including the private `sdk-public-release` environment the workflow needs. Licensed runner access is also still missing.
>
> **What changed**
>
> - Repository `/work/sample-org`, no commit (the coordinator commits it): updated `/work/sample-org/tasks/plans/sdk-cli-public-shadow-builds.md` in place.
>   - Corrected #368's recorded head from `13eae778…` to `ecb6d37a22d8b49dffdc6320bf542ab3b229b328`.
>   - Added a "Status, 2026-11-21" section with the live state, the human gates in order, and the agent work that follows them.
> - PR https://github.com/sample-org/sample-sdk/pull/368, description only (no commits, no push; the branch already matched):
>   - Rollout step 1 now says to promote `sdk-dev` to `main` before the first live run.
>   - Rollout step 2 now says the `sdk-public-release` and `sdk-shadow-native-source` environments still have to be created, and that no public repo has a `shadow-source` environment.
>   - The C++ test note now says no run is waiting for approval.
> - No changes to any public repo or PR, and no dispatches, approvals, merges, or ready-marking.
> - Temporary detached worktrees were removed after the tests:
>   - `/work/worktrees/sample-org/sample-sdk/more368-verify`
>   - `/work/worktrees/sample-org/sample-sdk-cpp/more368-verify`
> - The test logs are kept at:
>   - `/work/worktrees/sample-org/sample-sdk/more368-verify.test.log`
>   - `/work/worktrees/sample-org/sample-sdk-cpp/more368-verify.test.log`
>
> **Verification**
>
> - **#368** (`gh pr view 368 -R sample-org/sample-sdk`): OPEN, draft, no reviews or review requests, MERGEABLE/CLEAN.
>   - Head is `feat/public-shadow-builds` at `ecb6d37a22d8b49dffdc6320bf542ab3b229b328` and base is `sdk-dev`. The rollup shows 24 SUCCESS and 8 SKIPPED.
>   - `compare sdk-dev...ecb6d37a` shows 5 commits ahead and 0 behind. `sdk-dev` is `93dfa2e6c806f9ce0d95b485515c4cd4114e2200` and `main` is `57b2f58999982960357c892741235ff231eb07e7`.
>   - No rebase was needed. The local delivery worktree `/work/worktrees/sample-org/sample-sdk/public-shadow-builds-queue` is clean at the same head.
> - **Coordinator tests**: `agent-doc build-slot -- python3 -B scripts/test_sdk_shadow_build.py -v` at `ecb6d37a`: 8 tests OK.
> - **Six public PRs**: each `#1` is OPEN, draft, REVIEW_REQUIRED, BLOCKED, from a `sample-author` fork. Every head matches the plan table, and each is 0 commits behind its `main`:
>   - sample-sdk-cpp `6de996a`
>   - sample-sdk-dotnet `eec19c5`
>   - sample-sdk-engkit `828bf68`
>   - sample-sdk-gamekit `abca7a6`
>   - sample-sdk-gamekit-demo `2b9cdd5`
>   - sample-cli `df141cc`
> - **Public PR checks**: CLI #1 has 10 passing checks. The other five have no checks.
> - **Fork-PR Actions approval**: `actions/runs?status=action_required` returns 0 in all six repos. The cpp head has 0 check suites, so nobody has a run to approve. The cpp `Shadow build tests` workflow never triggered on the fork PR, and with only `pull` access `sample-author` cannot see why.
> - **C++ builder tests**: `agent-doc build-slot -- python3 -B .github/shadow-build/test_build.py -v` at cpp `6de996a`: 5 tests OK.
> - **Private configuration** (`sample-author` has push but not admin on sample-sdk):
>   - `environments` lists only `github-pages`, `roblox-engine`, `sdk-release-writer`, and `sdk-engine-source`.
>   - `environments/sdk-public-release` and `environments/sdk-shadow-native-source` both return 404. `sdk-public-release` has no deployments ever, and `cli-public-release.yml`'s `publish` job has only ever been skipped.
>   - No repository variable is named `SDK_SHADOW_*`.
> - **Public configuration**: none of the 10 destination repos has any environment. Their variables need more than `sample-author`'s `pull` access, so they are unknown. There are 0 `ci/preview/*` refs.
> - **App and runner group**:
>   - `orgs/sample-org/installations` returns 404 (needs `admin:org`), and `orgs/sample-org/actions/runner-groups` returns 403. `sample-author` cannot see whether the shadow App or the `sdk-demo-builds` grant exists.
>   - Indirect evidence: demo runs 44846805818, 44847017145, 44847017129, and 44847017137 are still `queued` after about 23 hours. Their jobs wait on `demo-demoroom`-labelled self-hosted runners, so the runner grant is still missing.
>
> **Action items**
>
> - human (sample-sdk reviewer with write access): mark https://github.com/sample-org/sample-sdk/pull/368 ready, approve it, and merge it into `sdk-dev`. Then merge the `sdk-dev` → `main` promotion that carries it. The shadow workflow runs only from protected `main`.
> - human (sample-org maintainer with write access on the public repos): review, approve, and merge the six public workflow PRs. The author (sample-author) marks each draft ready first. Merge https://github.com/sample-org/sample-sdk-cpp/pull/1 first, because the others pin its shared builder. Then merge https://github.com/sample-org/sample-sdk-dotnet/pull/1, https://github.com/sample-org/sample-sdk-engkit/pull/1, https://github.com/sample-org/sample-sdk-gamekit/pull/1, https://github.com/sample-org/sample-sdk-gamekit-demo/pull/1 and https://github.com/sample-org/sample-cli/pull/1.
> - human (sample-org org owner): create and install the dedicated shadow GitHub App. Give it contents write on the 10 destination repos (only `ci/preview/` refs) and Actions read/write on the six hosted repos. Grant sample-sdk's `sdk-shadow-builds.yml` access to the `sdk-demo-builds` runner group. The Demo Room and Engine runs that have been queued for about 23 hours show the grant is still missing.
> - human (sample-sdk repo admin): create the `sdk-public-release` environment (protected `main` only, required reviewers) and the `sdk-shadow-native-source` environment, per `docs/sdk-promotion.md` § Shadow builds. Neither exists today.
>   - Put these in `sdk-public-release`: `SDK_SHADOW_APP_ID` and `SDK_SHADOW_APP_PRIVATE_KEY`, the output keys `SDK_SHADOW_AGE_RECIPIENT` and `SDK_SHADOW_AGE_IDENTITY`, the six `SDK_SHADOW_<TARGET>_SOURCE_AGE_RECIPIENT` values, and `SDK_SHADOW_NATIVE_SOURCE_AGE_RECIPIENT`.
>   - Put the native `SDK_SHADOW_SOURCE_AGE_IDENTITY` in `sdk-shadow-native-source`.
>   - After the public merges, set `SDK_SHADOW_BUILDER_SHA` and the six `SDK_SHADOW_<TARGET>_WORKFLOW_SHA` values to the SHAs an agent reports.
> - human (admin of each of the six hosted public repos): create a `shadow-source` environment with only `main` eligible, independent reviewers, and admin bypass off. Store that repo's `SDK_SHADOW_SOURCE_AGE_IDENTITY` secret there. Set the repo variables `SDK_SHADOW_BUILDER_SHA` (the landed cpp commit) and `SDK_SHADOW_DISPATCH_ACTOR_ID` (the App bot's numeric ID).
>
> **Notes**
>
> - The deleted "approve the fork-PR Actions run on sample-sdk-cpp" gate should stay deleted. No run is waiting for approval, and the five tests it would run pass locally.
> - The licensed targets (Demo Room, Engine SDK, Demo Shooter, Engine Chat) also depend on the Engine credential rows already in the queue (`#q-064cd322a7a3`, `#q-55d08afc94db`). I did not duplicate them here.
> - Each live run also needs a person to approve its environment review in `sdk-public-release` and in each public `shadow-source`. That happens during the run, not as a one-time setup step.
> - An agent can generate the age key pairs into `passage` on request. Installing them as secrets needs the admins listed above.
> - Setting the pin variables needs admin access on both sides, so after the merges the agent only supplies the exact SHAs.
> - `gh` ran under the operator-approved credential-fill `GH_TOKEN`, which was never printed. `gh` is a mise shim and fails inside an untrusted-mise worktree, so run it from a parent directory.
>
> **Next actions**
>
> - After the human gates, agent work in order:
>   1. Report the landed cpp builder SHA and each public `main` workflow SHA for the pins.
>   2. From protected `main`, dispatch `sdk-shadow-builds.yml` with `operation=stage`, `target=cpp`, and the approved `source_sha`.
>   3. Once the build is terminal, dispatch `operation=collect` with that stage run ID, within the three-day artifact window.
>   4. Verify the ciphertext-only ref, the reconstructed tree, the private `verified-shadow-build` artifact, exact-ref deletion, and that no tags, Releases, or Packages were written. Record the run links in the session document.
>   5. Repeat for .NET, Engkit core, Gamekit .NET, Gamekit Demo, and CLI, then for the four licensed targets once runners and the Engine credential exist.
>   6. Compare parity per target. Then open the private PR that removes duplicate hosted compute per target, after the gate rejects stale or mismatched runs.
> - No follow-up queue heads beyond the Action items above.
> - The coordinator still needs to commit the plan file update in `/work/sample-org`.
<!-- agent-doc-subagent-completion key=f5214b2520e68ec475da93954f95369b1e3fe4cd219e449b487044aa56d59e65 -->

<!-- /agent:exchange -->

<!-- agent:run id="main" subagents -->
- ### ~~https://github.com/sample-org/sample-sdk/pull/364~~
  - %% docs(site): apply branding to SDK docs %%
- ### https://github.com/sample-org/sample-sdk/pull/368
  - %% Stage IP-sanitized public shadow builds and verify private intake %%
  - ~~do [#completetasksplans]~~
    - **Response [#r-829e6ed2ad19]**
      - **Timestamp:** 2026-11-21T23:57:03.000Z
      - **Summary:** **#368: yes, there is more than accepting the PR, but all of it before the first live run belongs to humans.** No agent work is left before #368 is accepted. It is green (24 passing, 8 skipped), 0 commits behind `sdk-dev` at `ecb6d37a2`, so it needed no rebase. The plan and the PR description had gone stale; the worker refreshed both and the tests pass again (8 sample-sdk, 5 cpp builder).
      - **State:** Complete.
      - **Guidance:** Safe to delete this completed row; the item is complete and has no remaining tracked or unpublished work. Keep the tree under `https://github.com/sample-org/sample-sdk/pull/368`: `follow up on an action item from 'completetasksplans': huma…` is still open.
      - **What changed:** <!-- agent-doc:response-detail -->
        - Repository `/work/sample-org`, no commit (the coordinator commits it): updated `/work/sample-org/tasks/plans/sdk-cli-public-shadow-builds.md` in place. <!-- agent-doc:response-detail -->
          - Corrected #368's recorded head from `13eae778…` to `ecb6d37a22d8b49dffdc6320bf542ab3b229b328`. <!-- agent-doc:response-detail -->
          - Added a "Status, 2026-11-21" section with the live state, the human gates in order, and the agent work that follows them. <!-- agent-doc:response-detail -->
        - PR https://github.com/sample-org/sample-sdk/pull/368, description only (no commits, no push; the branch already matched): <!-- agent-doc:response-detail -->
          - Rollout step 1 now says to promote `sdk-dev` to `main` before the first live run. <!-- agent-doc:response-detail -->
          - Rollout step 2 now says the `sdk-public-release` and `sdk-shadow-native-source` environments still have to be created, and that no public repo has a `shadow-source` environment. <!-- agent-doc:response-detail -->
          - The C++ test note now says no run is waiting for approval. <!-- agent-doc:response-detail -->
        - No changes to any public repo or PR, and no dispatches, approvals, merges, or ready-marking. <!-- agent-doc:response-detail -->
        - Temporary detached worktrees were removed after the tests: <!-- agent-doc:response-detail -->
          - `/work/worktrees/sample-org/sample-sdk/more368-verify` <!-- agent-doc:response-detail -->
          - `/work/worktrees/sample-org/sample-sdk-cpp/more368-verify` <!-- agent-doc:response-detail -->
        - The test logs are kept at: <!-- agent-doc:response-detail -->
          - `/work/worktrees/sample-org/sample-sdk/more368-verify.test.log` <!-- agent-doc:response-detail -->
          - `/work/worktrees/sample-org/sample-sdk-cpp/more368-verify.test.log` <!-- agent-doc:response-detail -->
      - **Verification:** <!-- agent-doc:response-detail -->
        - **#368** (`gh pr view 368 -R sample-org/sample-sdk`): OPEN, draft, no reviews or review requests, MERGEABLE/CLEAN. <!-- agent-doc:response-detail -->
          - Head is `feat/public-shadow-builds` at `ecb6d37a22d8b49dffdc6320bf542ab3b229b328` and base is `sdk-dev`. The rollup shows 24 SUCCESS and 8 SKIPPED. <!-- agent-doc:response-detail -->
          - `compare sdk-dev...ecb6d37a` shows 5 commits ahead and 0 behind. `sdk-dev` is `93dfa2e6c806f9ce0d95b485515c4cd4114e2200` and `main` is `57b2f58999982960357c892741235ff231eb07e7`. <!-- agent-doc:response-detail -->
          - No rebase was needed. The local delivery worktree `/work/worktrees/sample-org/sample-sdk/public-shadow-builds-queue` is clean at the same head. <!-- agent-doc:response-detail -->
        - **Coordinator tests**: `agent-doc build-slot -- python3 -B scripts/test_sdk_shadow_build.py -v` at `ecb6d37a`: 8 tests OK. <!-- agent-doc:response-detail -->
        - **Six public PRs**: each `#1` is OPEN, draft, REVIEW_REQUIRED, BLOCKED, from a `sample-author` fork. Every head matches the plan table, and each is 0 commits behind its `main`: <!-- agent-doc:response-detail -->
          - sample-sdk-cpp `6de996a` <!-- agent-doc:response-detail -->
          - sample-sdk-dotnet `eec19c5` <!-- agent-doc:response-detail -->
          - sample-sdk-engkit `828bf68` <!-- agent-doc:response-detail -->
          - sample-sdk-gamekit `abca7a6` <!-- agent-doc:response-detail -->
          - sample-sdk-gamekit-demo `2b9cdd5` <!-- agent-doc:response-detail -->
          - sample-cli `df141cc` <!-- agent-doc:response-detail -->
        - **Public PR checks**: CLI #1 has 10 passing checks. The other five have no checks. <!-- agent-doc:response-detail -->
        - **Fork-PR Actions approval**: `actions/runs?status=action_required` returns 0 in all six repos. The cpp head has 0 check suites, so nobody has a run to approve. The cpp `Shadow build tests` workflow never triggered on the fork PR, and with only `pull` access `sample-author` cannot see why. <!-- agent-doc:response-detail -->
        - **C++ builder tests**: `agent-doc build-slot -- python3 -B .github/shadow-build/test_build.py -v` at cpp `6de996a`: 5 tests OK. <!-- agent-doc:response-detail -->
        - **Private configuration** (`sample-author` has push but not admin on sample-sdk): <!-- agent-doc:response-detail -->
          - `environments` lists only `github-pages`, `roblox-engine`, `sdk-release-writer`, and `sdk-engine-source`. <!-- agent-doc:response-detail -->
          - `environments/sdk-public-release` and `environments/sdk-shadow-native-source` both return 404. `sdk-public-release` has no deployments ever, and `cli-public-release.yml`'s `publish` job has only ever been skipped. <!-- agent-doc:response-detail -->
          - No repository variable is named `SDK_SHADOW_*`. <!-- agent-doc:response-detail -->
        - **Public configuration**: none of the 10 destination repos has any environment. Their variables need more than `sample-author`'s `pull` access, so they are unknown. There are 0 `ci/preview/*` refs. <!-- agent-doc:response-detail -->
        - **App and runner group**: <!-- agent-doc:response-detail -->
          - `orgs/sample-org/installations` returns 404 (needs `admin:org`), and `orgs/sample-org/actions/runner-groups` returns 403. `sample-author` cannot see whether the shadow App or the `sdk-demo-builds` grant exists. <!-- agent-doc:response-detail -->
          - Indirect evidence: demo runs 44846805818, 44847017145, 44847017129, and 44847017137 are still `queued` after about 23 hours. Their jobs wait on `demo-demoroom`-labelled self-hosted runners, so the runner grant is still missing. <!-- agent-doc:response-detail -->
      - **Action items:** <!-- agent-doc:response-detail -->
        - human (sample-sdk reviewer with write access): mark https://github.com/sample-org/sample-sdk/pull/368 ready, approve it, and merge it into `sdk-dev`. Then merge the `sdk-dev` → `main` promotion that carries it. The shadow workflow runs only from protected `main`. <!-- agent-doc:response-detail -->
        - human (sample-org maintainer with write access on the public repos): review, approve, and merge the six public workflow PRs. The author (sample-author) marks each draft ready first. Merge https://github.com/sample-org/sample-sdk-cpp/pull/1 first, because the others pin its shared builder. Then merge https://github.com/sample-org/sample-sdk-dotnet/pull/1, https://github.com/sample-org/sample-sdk-engkit/pull/1, https://github.com/sample-org/sample-sdk-gamekit/pull/1, https://github.com/sample-org/sample-sdk-gamekit-demo/pull/1 and https://github.com/sample-org/sample-cli/pull/1. <!-- agent-doc:response-detail -->
        - human (sample-org org owner): create and install the dedicated shadow GitHub App. Give it contents write on the 10 destination repos (only `ci/preview/` refs) and Actions read/write on the six hosted repos. Grant sample-sdk's `sdk-shadow-builds.yml` access to the `sdk-demo-builds` runner group. The Demo Room and Engine runs that have been queued for about 23 hours show the grant is still missing. <!-- agent-doc:response-detail -->
        - human (sample-sdk repo admin): create the `sdk-public-release` environment (protected `main` only, required reviewers) and the `sdk-shadow-native-source` environment, per `docs/sdk-promotion.md` § Shadow builds. Neither exists today. <!-- agent-doc:response-detail -->
          - Put these in `sdk-public-release`: `SDK_SHADOW_APP_ID` and `SDK_SHADOW_APP_PRIVATE_KEY`, the output keys `SDK_SHADOW_AGE_RECIPIENT` and `SDK_SHADOW_AGE_IDENTITY`, the six `SDK_SHADOW_<TARGET>_SOURCE_AGE_RECIPIENT` values, and `SDK_SHADOW_NATIVE_SOURCE_AGE_RECIPIENT`. <!-- agent-doc:response-detail -->
          - Put the native `SDK_SHADOW_SOURCE_AGE_IDENTITY` in `sdk-shadow-native-source`. <!-- agent-doc:response-detail -->
          - After the public merges, set `SDK_SHADOW_BUILDER_SHA` and the six `SDK_SHADOW_<TARGET>_WORKFLOW_SHA` values to the SHAs an agent reports. <!-- agent-doc:response-detail -->
        - human (admin of each of the six hosted public repos): create a `shadow-source` environment with only `main` eligible, independent reviewers, and admin bypass off. Store that repo's `SDK_SHADOW_SOURCE_AGE_IDENTITY` secret there. Set the repo variables `SDK_SHADOW_BUILDER_SHA` (the landed cpp commit) and `SDK_SHADOW_DISPATCH_ACTOR_ID` (the App bot's numeric ID). <!-- agent-doc:response-detail -->
      - **Notes:** <!-- agent-doc:response-detail -->
        - The deleted "approve the fork-PR Actions run on sample-sdk-cpp" gate should stay deleted. No run is waiting for approval, and the five tests it would run pass locally. <!-- agent-doc:response-detail -->
        - The licensed targets (Demo Room, Engine SDK, Demo Shooter, Engine Chat) also depend on the Engine credential rows already in the queue (`#q-064cd322a7a3`, `#q-55d08afc94db`). I did not duplicate them here. <!-- agent-doc:response-detail -->
        - Each live run also needs a person to approve its environment review in `sdk-public-release` and in each public `shadow-source`. That happens during the run, not as a one-time setup step. <!-- agent-doc:response-detail -->
        - An agent can generate the age key pairs into `passage` on request. Installing them as secrets needs the admins listed above. <!-- agent-doc:response-detail -->
        - Setting the pin variables needs admin access on both sides, so after the merges the agent only supplies the exact SHAs. <!-- agent-doc:response-detail -->
        - `gh` ran under the operator-approved credential-fill `GH_TOKEN`, which was never printed. `gh` is a mise shim and fails inside an untrusted-mise worktree, so run it from a parent directory. <!-- agent-doc:response-detail -->
      - **Next actions:** <!-- agent-doc:response-detail -->
        - After the human gates, agent work in order: <!-- agent-doc:response-detail -->
          1. Report the landed cpp builder SHA and each public `main` workflow SHA for the pins. <!-- agent-doc:response-detail -->
          2. From protected `main`, dispatch `sdk-shadow-builds.yml` with `operation=stage`, `target=cpp`, and the approved `source_sha`. <!-- agent-doc:response-detail -->
          3. Once the build is terminal, dispatch `operation=collect` with that stage run ID, within the three-day artifact window. <!-- agent-doc:response-detail -->
          4. Verify the ciphertext-only ref, the reconstructed tree, the private `verified-shadow-build` artifact, exact-ref deletion, and that no tags, Releases, or Packages were written. Record the run links in the session document. <!-- agent-doc:response-detail -->
          5. Repeat for .NET, Engkit core, Gamekit .NET, Gamekit Demo, and CLI, then for the four licensed targets once runners and the Engine credential exist. <!-- agent-doc:response-detail -->
          6. Compare parity per target. Then open the private PR that removes duplicate hosted compute per target, after the gate rejects stale or mismatched runs. <!-- agent-doc:response-detail -->
        - No follow-up queue heads beyond the Action items above. <!-- agent-doc:response-detail -->
        - The coordinator still needs to commit the plan file update in `/work/sample-org`. <!-- agent-doc:response-detail -->
  - 🙋 follow up on an action item from `completetasksplans`: human (sample-sdk reviewer with write access): mark https://github.com/sample-org/sample-sdk/pull/368 ready, approve it, and merge it into `sdk-dev`. Then merge the `sdk-dev` → `main` promotion that carries it. The shadow workflow runs only from protected `main`. [operator-verify]
  - 🙋 follow up on an action item from `completetasksplans`: human (sample-org maintainer with write access on the public repos): review, approve, and merge the six public workflow PRs. The author (sample-author) marks each draft ready first. Merge https://github.com/sample-org/sample-sdk-cpp/pull/1 first, because the others pin its shared builder. Then merge https://github.com/sample-org/sample-sdk-dotnet/pull/1, https://github.com/sample-org/sample-sdk-engkit/pull/1, https://github.com/sample-org/sample-sdk-gamekit/pull/1, https://github.com/sample-org/sample-sdk-gamekit-demo/pull/1 and https://github.com/sample-org/sample-cli/pull/1. [operator-verify]
  - 🙋 follow up on an action item from `completetasksplans`: human (sample-org org owner): create and install the dedicated shadow GitHub App. Give it contents write on the 10 destination repos (only `ci/preview/` refs) and Actions read/write on the six hosted repos. Grant sample-sdk's `sdk-shadow-builds.yml` access to the `sdk-demo-builds` runner group. The Demo Room and Engine runs that have been queued for about 23 hours show the grant is still missing. [operator-verify]
  - 🙋 follow up on an action item from `completetasksplans`: human (sample-sdk repo admin): create the `sdk-public-release` environment (protected `main` only, required reviewers) and the `sdk-shadow-native-source` environment, per `docs/sdk-promotion.md` § Shadow builds. Neither exists today. Put these in `sdk-public-release`: `SDK_SHADOW_APP_ID` and `SDK_SHADOW_APP_PRIVATE_KEY`, the output keys `SDK_SHADOW_AGE_RECIPIENT` and `SDK_SHADOW_AGE_IDENTITY`, the six `SDK_SHADOW_<TARGET>_SOURCE_AGE_RECIPIENT` values, and `SDK_SHADOW_NATIVE_SOURCE_AGE_RECIPIENT`. Put the native `SDK_SHADOW_SOURCE_AGE_IDENTITY` in `sdk-shadow-native-source`. After the public merges, set `SDK_SHADOW_BUILDER_SHA` and the six `SDK_SHADOW_<TARGET>_WORKFLOW_SHA` values to the SHAs an agent reports. [operator-verify]
  - 🙋 follow up on an action item from `completetasksplans`: human (admin of each of the six hosted public repos): create a `shadow-source` environment with only `main` eligible, independent reviewers, and admin bypass off. Store that repo's `SDK_SHADOW_SOURCE_AGE_IDENTITY` secret there. Set the repo variables `SDK_SHADOW_BUILDER_SHA` (the landed cpp commit) and `SDK_SHADOW_DISPATCH_ACTOR_ID` (the App bot's numeric ID). [operator-verify]
  - ~~follow up on an action item from queue item “Complete tasks/plans/sdk-cli-public-shadow-builds.md”: operator: Unlock the desktop login keyring so plain `gh` works. Or, if you want the workaround, tell this worker directly that it may export `GH_TOKEN` from `git credential fill` for gh calls. [operator-verify] [#q-d10df3cb7e84]~~
- ### Deploy the README changes from `sample-sdk` `main`.
<!-- /agent:run -->
