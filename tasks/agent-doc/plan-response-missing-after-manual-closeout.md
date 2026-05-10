# Plan: Response Missing After Manual Closeout

## Problem

The Monster Rod Holders task document recorded the prompt:

```text
Please review stripe orders. Steve claims there were no Stripe orders since we took over the project in Mid April.
spec-test-commit-push
```

The work completed, including commits and pushes, but the assistant response was not written back into `src/boost-client/tasks/monsterrodholders.md`. The visible document ended with the user prompt inside the exchange block, so the session appeared unresolved even though the code/spec/test work was complete.

## Root Cause To Prove

The direct Codex path can complete substantial work, commit/push the site repo and parent repo, and still bypass the agent-doc response patchback/finalize contract. In this case, the parent repo was clean and pushed, but the response body never landed in the session document.

This may overlap with existing bugs:

- `#harnessprompt`: harness/native prompts can be actionable without a durable exchange response.
- `#sidecar-diverge`: successful finalize/commit can still fail to show the response in the editor.
- `#panehang`: panes can finish or idle without a reliable closeout lifecycle.

The distinct failure to preserve is: a successful `spec-test-commit-push` turn must not be allowed to end with only the prompt persisted in the exchange.

## Additional Evidence: Root Task Patchback Miss

On May 10, 2026, `src/session-share/tasks/root.md` recorded:

```text
Fix the test suite. The test suite must pass before completing the turn. Update the agent harness files to enforce this rule.
spec-test-commit-push
```

The work completed and was pushed (`BuildParty 33646bb`, `SessionShare 0223139e`), but the assistant closeout did not land in the document. The user had to ask for manual patchback repair. This is the same prompt-only exchange shape, but against the root session-share task document and after a nested submodule commit plus parent pointer/news commit.

## Additional Evidence: VT Agent Deploy Patchback Miss

On May 10, 2026, `src/session-share/tasks/root.md` recorded:

```text
do [#vt-agent-deploy]. spec-test-news-commit-push
```

The work completed and was pushed (`BuildParty af7b9b7`, `SessionShare ebc806c2`, parent `agent-loop` 8d972190b), but the final response block was not written into the exchange. The user had to request manual repair with: "The response patchback is not written into the document." This is another prompt-only exchange closeout after a nested submodule commit, session-share commit, and parent pointer commit all succeeded.

## Fix Plan

1. Add a regression fixture for a direct Codex/agent-doc turn that:
   - starts with a prompt already written into an exchange boundary,
   - performs a commit/push-like closeout path,
   - exits without calling finalize or without mutating the exchange body.
2. Add a session-check invariant: if the newest exchange boundary contains a prompt and no assistant response after it, fail closed with a diagnostic naming the document and boundary id.
3. Add a closeout/finalize guard that runs after commit/push operations and before process exit, verifying the response body is present in the document working tree and committed state.
4. If commit/push completed but response patchback failed, provide a recovery command that appends the last assistant response to the target exchange boundary without rerunning the completed site work.
5. Cover the recovery path with a test that preserves submodule discipline: site commit remains unchanged, parent pointer remains unchanged unless the recovered task doc commit requires a parent update.

## Exit Criteria

- A completed direct Codex `agent-doc <FILE>` turn cannot silently exit with a prompt-only exchange.
- The diagnostic distinguishes "work not done" from "work done but response patchback missing."
- The recovery path writes the missing response into the original document boundary and does not duplicate commits or rerun side effects.
