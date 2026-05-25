# Plan: `agent-doc finalize/write --commit-sibling <repo>`

**Bug ID:** `#agent-doc-commit-sibling`
**Class:** CLI feature / cross-repo session provenance
**Related:** `#cross-repo-sessiondoc-trailer` (companion convention item in `tasks/docs.md`)
**Severity:** medium — every cross-repo commit driven by an agent-doc session loses the `Session-Doc:` trailer today, so `backfill-updates.sh` falls back to the `manual` branch and the cross-repo audit trail is incomplete.

## Problem

A session-share session document in repo A (e.g. `tasks/<doc>.md` under the session-share repo) can drive commits in sibling repos B/C/D (e.g. `build-party`, `claudescore.com`, `claudescore-plugin`). Today `agent-doc finalize` and `agent-doc write --commit` only commit inside the session document's own submodule. Cross-repo sibling commits are created by the agent calling `git commit` directly, and the `Session-Doc:` git trailer that ties the sibling commit back to the driving session is easy to forget.

Observed effect: `backfill-updates.sh` in build-party (and three sibling copies) parses commit subjects with `^agent-doc\([^)]+\):` to detect agent-doc-driven commits. Sibling commits with subjects like `scripts: …` fall through to the `manual` branch and render bare-text instead of the dual link `last touched <date> by [tasks/docs.md](session-share-url) [<short>](sibling-url)`.

Three existing sibling commits (`build-party@9a0be9a`, `claudescore.com@bf73644`, `claudescore-plugin@f1daedb`) were retroactively amended with `Session-Doc:` trailers and force-pushed; the convention is proven in production for the trailer half but not for the script-parser or the agent-doc CLI half.

## Trailer contract

Format:

```
Session-Doc: https://github.com/<session-share-org>/<session-share-repo>/blob/<session-share-cycle-sha>/<session-doc-path>
```

- `<session-share-org>` / `<session-share-repo>` — owning org + repo of the session document (e.g. `ClaudeScore/SessionShare`).
- `<session-share-cycle-sha>` — the commit sha in the session-share repo that owns the cycle the sibling commit is part of (typically the most recent `agent-doc finalize` commit on the session document immediately preceding the sibling commit).
- `<session-doc-path>` — the document path inside the session-share repo (e.g. `tasks/docs.md`).

The trailer must round-trip through `git log --format='%(trailers:key=Session-Doc,valueonly)'` so `backfill-updates.sh` can parse it without re-parsing the body.

## Phases

### Phase 1 — Author plan + split backlog *(this cycle)*

Bug item `#agent-doc-commit-sibling` split into binary, sibling-scripts, and convention phases. Parent gated until all phases land. Plan file = this document.

### Phase 2 — `--commit-sibling` CLI flag in `agent-doc` (`#adcs-binary`)

**Files:** `src/agent-doc/src/main.rs` (clap struct), `src/agent-doc/src/write.rs` (`CommandOptions`, finalize/commit path), new `src/agent-doc/src/session_share.rs` or extend `src/agent-doc/src/git.rs` for trailer construction.

Behavior:

1. Accept repeated `--commit-sibling <repo-path>` flags on `agent-doc finalize` and `agent-doc write --commit`. `<repo-path>` is the absolute or `agent-loop`-relative path to a sibling git working tree with staged changes ready to commit.
2. For each sibling, resolve the session-share metadata triple `(org, repo, sha, path)`:
   - `org` + `repo` from the session document's git remote (`origin`), parsed from the URL.
   - `path` from the session document path relative to that remote's working tree.
   - `sha` from the freshly-created session document commit produced by this same `finalize` / `write --commit` invocation.
3. Run `git -C <sibling> commit -m '<existing message>' --trailer 'Session-Doc: <url>' --trailer 'Co-Authored-By: …'` so the trailer round-trips through `git log --format=%(trailers:…)`.
4. Order is: stage and commit the session document first (binary-owned closeout boundary). After that commit's sha is known, stage+commit each sibling with the trailer pointing at that sha. If any sibling commit fails, surface the error and leave the session document committed — siblings can be retried with `agent-doc commit-sibling <repo>` (new flat subcommand, future scope).
5. Fail closed if `<sibling>` is not a git working tree, has no staged changes, or shares the same `.git` directory as the session document (sibling must be a different repo, not the same submodule).

Tests in `src/agent-doc/src/write.rs::tests` (or new `tests/commit_sibling.rs`):

- `commit_sibling_injects_session_doc_trailer` — temp repo A with session doc, temp repo B with staged changes; assert the post-finalize sibling commit subject is unchanged and `git log --format='%(trailers:key=Session-Doc,valueonly)' -1` returns the expected URL.
- `commit_sibling_uses_session_doc_commit_sha` — assert the trailer URL's sha matches the session-doc commit, not an earlier or later sha.
- `commit_sibling_fails_closed_on_same_repo` — assert pointing `--commit-sibling` at the session document's own repo fails with a clear error.
- `commit_sibling_fails_closed_on_no_staged_changes` — assert empty staging in the sibling fails with a clear error and the session document still commits cleanly.

### Phase 3 — Sibling `backfill-updates.sh` script parser (`#adcs-scripts`)

**Files (sibling repos, not in agent-loop):**

- `build-party/scripts/backfill-updates.sh`
- `claudescore.com/scripts/backfill-updates.sh`
- `claudescore-plugin/scripts/backfill-updates.sh`
- *(fourth copy — confirm canonical home in session-share)*

Extend `generate_updates_row_value()` and `generate_updates_table()` to read `git log --format='%H|%aI|%s|%(trailers:key=Session-Doc,valueonly)'` and prefer the trailer URL over the `agent-doc(name):` subject heuristic. Output line shape:

```
last touched <date> by [tasks/docs.md](session-share-blob-url) [<short>](sibling-commit-url)
```

Backfill audit: grandfather the three existing amended commits (`build-party@9a0be9a`, `claudescore.com@bf73644`, `claudescore-plugin@f1daedb`); from this convention forward, all sibling commits driven by an agent-doc session must carry the trailer.

This phase lives in sibling repos and cannot be implemented from within `agent-loop`. Track here for cross-repo coordination; execute in each sibling repo's own session.

### Phase 4 — Convention doc (`#adcs-convention`)

**Files:** `tasks/docs.md` (in session-share; file does not exist in `agent-loop`).

Author the cross-repo session-doc trailer convention: trailer format, when to use, when to skip, how `backfill-updates.sh` consumes it, the migration story for pre-trailer commits. Cross-link this plan and `#cross-repo-sessiondoc-trailer`.

## Out of scope

- Retroactive trailer injection for pre-convention sibling commits beyond the three already amended. Those stay grandfathered as `manual`.
- Multi-sibling atomic commits: each `--commit-sibling` is independent. A mid-run failure leaves the session document committed and partial sibling commits as-is.

## Verification

- `cargo test --bin agent-doc commit_sibling_` (Phase 2 tests).
- End-to-end: in a temp repo, run `agent-doc finalize <doc> --commit-sibling <other-repo>` against a sibling with staged changes; verify the trailer with `git -C <other-repo> log --format='%(trailers:key=Session-Doc,valueonly)' -1`.
- After Phase 3 lands in any sibling: run that sibling's `backfill-updates.sh` and confirm the new row renders the dual link instead of the `manual` fallback.
