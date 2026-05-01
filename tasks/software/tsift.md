---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260429-070822.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)

❯ #code-review
### Re: code review — gpt-5

Reviewed `src/tsift` with `cargo test` and `cargo clippy` and focused on indexing, search, lock handling, and summaries workflows.

#### Findings

**1. Summary extraction is not concurrency-safe and can duplicate API spend**
`cmd_summarize()` opens the writable summaries DB, checks `summary_db.is_current()`, then calls the model and only later commits via `replace_file()` (`src/tsift/src/main.rs:2605-2690`, `src/tsift/src/summarize.rs:181-191`). Two concurrent `tsift summarize --extract` runs on the same repo can both observe a miss, both call Anthropic for the same file, and then race last-writer-wins on the cache. In `--diff` mode they can also interleave `delete_by_file()` for removed paths with another extractor still rebuilding summaries. This is the highest-value workflow bug because it burns API budget and makes cache state nondeterministic.

**2. `search` still has a post-precheck read race that bypasses the resilient fallback**
`precheck_search_indexes()` validates freshness with `IndexDb::inspect_read_only()` before the main search path runs (`src/tsift/src/main.rs:3217-3233`), but `cmd_search()` then reopens the DB via raw `IndexDb::open_read_only()` for symbol hits (`src/tsift/src/main.rs:3303-3334`). If a rollback-journal lock appears after precheck but before symbol lookup, `search` can still fail with a raw SQLite lock even though the index layer already has `open_read_only_resilient()` and snapshot fallback support (`src/tsift/src/index.rs:346-395`). The existing tests cover the lock being present before precheck; they do not cover the race window after it.

**3. Summary/status read paths do not degrade the way index reads do**
`status::check_summaries()` and the summarize query/stats path call `SummaryDb::open_read_only()` directly (`src/tsift/src/status.rs:288-317`, `src/tsift/src/main.rs:2847-2855`, `src/tsift/src/summarize.rs:143-150`). Unlike the index code, summaries have no resilient read path, no snapshot fallback, and no user-facing lock diagnostics. A wedged or migrating `summaries.db` can therefore make `tsift status` and `tsift summarize --stats` or lookup queries fail even while the index side remains readable.

#### Verification

- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`417` unit tests and `42` integration tests).
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed.

#### Follow-up items added to backlog.
❯ do #afyn. spec-test-build-install-commit-push
### Re: #afyn spec-test-build-install-commit-push — gpt-5

Implemented the coarse summaries extraction lock in `src/tsift` so only one `tsift summarize --extract` writer can mutate a cache at a time. The command now claims `summaries.lock` before stale-row deletion, freshness checks, and LLM calls, which closes the duplicate-spend / last-writer-wins race from the review.

Added lock coverage in both `src/summarize.rs` and `src/main.rs`, updated `SPEC.md`, and ran the full requested verification/install flow:
- `cargo test` passed (`420` unit tests and `42` integration tests)
- `cargo clippy --all-targets --all-features -- -D warnings` passed
- `cargo build` passed
- `cargo install --path .` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `d84df20` (`fix: serialize summarize extractors`).

Top remaining backlog item: `#bjen`.
❯ do #bjen. spec-test-build-install-commit-push
### Re: #bjen spec-test-build-install-commit-push — gpt-5

The last tmux pane crash interrupted document closeout, but the `#bjen` work itself was already complete in `src/tsift`.

- `src/tsift` `main` already contains the fix as commit `bc0404a` (`fix: harden search post-precheck reads`), pushed to `origin/main`.
- The post-precheck search read path is hardened through the resilient read-only helper, and the rollback-journal race is covered by `search_cmd_uses_snapshot_fallback_when_rollback_journal_lock_appears_after_precheck`.
- `SPEC.md` already documents that hardened search behavior.

Verification I reran now against current `main`:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`421` unit tests and `42` integration tests)
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

`#bjen` has now been reaped from the backlog. Top remaining backlog item: `#8w9n`.

Separate follow-up from the pane crash is already logged in `tasks/agent-doc/agent-doc-bugs2.md` as `#tmuxcrash`, using `tasks/agent-doc/plan-tmux-pane-crash-stability.md`.
### Re: #bjen spec-test-build-install-commit-push — gpt-5

The last tmux pane crash interrupted document closeout, but the `#bjen` work itself was already complete in `src/tsift`.

- `src/tsift` `main` already contains the fix as commit `bc0404a` (`fix: harden search post-precheck reads`), pushed to `origin/main`.
- The post-precheck search read path is hardened through the resilient read-only helper, and the rollback-journal race is covered by `search_cmd_uses_snapshot_fallback_when_rollback_journal_lock_appears_after_precheck`.
- `SPEC.md` already documents that hardened search behavior.

Verification I reran now against current `main`:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`421` unit tests and `42` integration tests)
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

`#bjen` has now been reaped from the backlog. Top remaining backlog item: `#8w9n`.

Separate follow-up from the pane crash is already logged in `tasks/agent-doc/agent-doc-bugs2.md` as `#tmuxcrash`, using `tasks/agent-doc/plan-tmux-pane-crash-stability.md`.
❯ do #8w9n. spec-test-build-install-commit-push
### Re: #8w9n spec-test-build-install-commit-push — gpt-5

`#8w9n` was already complete in `src/tsift` on `main` as commit `30b60d8` (`fix: harden summary read-only paths`), and that commit is already pushed to `origin/main`.

That change set covers the summaries-side read-only hardening the backlog item asked for:
- `SummaryDb` now exposes the resilient read-only open path with snapshot fallback.
- `tsift status` carries summary-cache recovery state instead of failing hard when a rollback-journal lock wedges the live summaries DB.
- `tsift summarize --stats`, symbol lookups, and `--file` lookups reuse the resilient summaries read path.
- `SPEC.md` documents the read-only cache behavior and degraded recovery reporting.

Fresh verification I reran now against current `main`:
- `cargo test` passed (`427` unit tests and `42` integration tests)
- `cargo build` passed
- `cargo install --path .` passed
- `tsift --version` => `0.1.21`

No new commit was needed this cycle because the requested spec/test/build/install/commit/push work was already landed and published. The active backlog is now clear; only icebox items remain.
❯ #code-review
### Re: code review — gpt-5

Reviewed `src/tsift` with `cargo test` and targeted manual repros around summarize workflows, cache lifecycle, and path normalization.

#### Findings

**1. `tsift summarize --diff --extract` is unusable before the first commit**  
`git_changed_files()` shells out to `git diff ... HEAD` unconditionally, so an unborn branch aborts the whole extract before any file work starts (`src/tsift/src/summarize.rs:781-809`, `src/tsift/src/main.rs:2608-2610`). I reproduced this in a fresh temp repo: `tsift summarize --extract src --diff .` fails with `fatal: ambiguous argument 'HEAD'`. Fresh repos should degrade to "tracked changes = none, untracked files = current scope" instead of hard-failing.

**2. Full re-extracts never reap deleted summaries, so query mode can return summaries for files that no longer exist**  
Outside `--diff`, `cmd_summarize()` always sets `deleted_summary_paths` to empty and only deletes rows from that set (`src/tsift/src/main.rs:2628-2642`). Query mode then reads rows directly by `symbol_name` / `file_path` with no liveness check (`src/tsift/src/main.rs:2763-2799`, `src/tsift/src/summarize.rs:268-289`). That means a normal full `tsift summarize --extract` run can leave stale cache rows behind after source deletion, and later `tsift summarize <symbol>` still returns them. I verified the visible symptom by querying a temp `summaries.db` row whose `src/deleted.rs` file was gone; the CLI still printed the stale summary.

**3. `tsift summarize --file` does not normalize equivalent paths**  
Extract writes cache keys as root-relative paths like `src/lib.rs` (`src/tsift/src/main.rs:2663-2667`), but file query mode passes the user string straight into `get_by_file()` (`src/tsift/src/main.rs:2766-2768`, `src/tsift/src/summarize.rs:280-289`). I reproduced the mismatch against a temp cache: `--file src/lib.rs` hit, while `--file ./src/lib.rs` missed the same row. Absolute or nested-directory spellings will miss for the same reason.

#### Verification

- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`427` unit tests and `42` integration tests).
- Reproduced the unborn-`HEAD` `--diff` failure against a fresh temp repo.
- Reproduced the literal-path cache miss for `summarize --file` with a temp `summaries.db`.

Follow-up items added to backlog. Top remaining backlog item: `#ubhd`.
❯ do #ubhd. spec-test-build-install-commit-push
### Re: #ubhd spec-test-build-install-commit-push — gpt-5

Implemented `#ubhd` in `src/tsift` so `tsift summarize --diff --extract` no longer hard-fails on an unborn `HEAD`. The summarize diff path now checks whether the repo has a committed `HEAD`; if not, it skips tracked/deleted `git diff` queries and treats the run as untracked-only within the requested extract scope.

Added regression coverage in `src/main.rs` and `tests/exit_code.rs`, and updated `SPEC.md` to document the unborn-`HEAD` fallback.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`428` unit tests and `43` integration tests)
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `eabc640` (`fix: tolerate summarize diff on unborn HEAD`).

Top remaining backlog item: `#stle`.
### Re: #8w9n spec-test-build-install-commit-push — gpt-5

`#8w9n` was already completed earlier in this session as `src/tsift` commit `30b60d8` (`fix: harden summary read-only paths`), so the duplicated prompt line did not require any new code, verification, or git changes.

Top remaining backlog item stays `#stle`.
❯ do [#stle]. spec-test-build-install-commit-push
### Re: #stle spec-test-build-install-commit-push — gpt-5

Implemented `#stle` in `src/tsift` so normal `tsift summarize --extract <path>` runs now reap cached summary rows for files that no longer exist inside the requested extract scope, including the empty-scope case where the last live file was removed before re-extract. The extract path now computes stale cached file keys inside the scope before deciding there is no work to do, and `SPEC.md` documents that full re-extract behavior.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`429` unit tests and `43` integration tests)
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `f5e6d3b` (`fix: prune stale summaries on full extract`).

Top remaining backlog item: `#fqry`.
❯ do [#fqry]. spec-test-build-install-commit-push
### Re: #fqry spec-test-build-install-commit-push — gpt-5

Implemented `#fqry` in `src/tsift` so `tsift summarize --file` now normalizes equivalent path spellings before lookup. The query path first preserves an exact normalized user spelling such as `src/lib.rs` or `./src/lib.rs`, then resolves nested relative spellings like `../lib.rs` against the `--path` anchor and collapses them back to the canonical root-relative cache key used by `summaries.db`.

Added regression coverage in both `src/summarize.rs` and `tests/exit_code.rs`, and updated `SPEC.md` to document the normalized `--file` behavior.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`431` unit tests and `44` integration tests)
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `c6a49f8` (`fix: normalize summarize file lookups`).

The active backlog is now clear; only icebox items remain.
❯ #code-review
### Re: code review — gpt-5

#### Findings

1. `summarize --diff --extract` leaves stale summary rows behind on renames. The diff cleanup path only deletes paths gathered from `git diff --name-only --diff-filter=D HEAD`, so a `git mv` contributes the new path to `existing` but never contributes the old path to `deleted` ([summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:833), [main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2608)). I reproduced this with the installed `tsift` binary: after an initial extract, `git mv src/old.rs src/new.rs` plus `tsift summarize --extract src --diff` still leaves `tsift summarize --file src/old.rs` returning the stale summary. Root cause: rename tombstones never enter `GitChangedFiles.deleted`.

2. Absolute `summarize --file` queries miss when the repo is reached through a symlinked path. `resolve_extract_base()` canonicalizes `--path` to the real project root ([main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3073)), but `file_lookup_candidates()` only lexically normalizes an absolute `file_query` before `strip_prefix(project_root)` ([summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:658), [main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2775)). If the user passes an absolute symlinked path like `/tmp/link/src/lib.rs`, the candidate stays under `/tmp/link/...`, the prefix strip against the canonical root fails, and lookup never reaches the stored root-relative cache key. I reproduced this with the installed CLI: the real-path absolute query hits, while the symlinked absolute query prints `No cached summary for file`.

#### Verification

- Reproduced finding 1 with the installed `tsift` binary by extracting once, renaming `src/old.rs` to `src/new.rs`, rerunning `summarize --extract --diff`, then querying both old and new paths.
- Reproduced finding 2 with the installed `tsift` binary against a symlinked checkout path.

Follow-up items added to backlog. Top remaining backlog item: `#rnme`.
❯ do [#rnme]. spec-test-build-install-commit-push
### Re: #rnme spec-test-build-install-commit-push — gpt-5

Implemented `#rnme` in `src/tsift` so `tsift summarize --diff --extract` now reaps stale summary rows for git renames. The diff adapter now parses `git diff --name-status --find-renames HEAD`, keeps the new side of a rename in the extract set, and feeds the old side into summary-row deletion before extraction.

Added regression coverage in `src/main.rs` for both rename tracking and rename-row cleanup, and updated `SPEC.md` to document that `--diff` deletes the old side of `git mv` tombstones.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`433` unit tests and `44` integration tests)
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `ef88b69` (`fix: reap summarize rename tombstones`).

Top remaining backlog item: `#slnk`.
❯ do [#slnk]. spec-test-build-install-commit-push
### Re: #slnk spec-test-build-install-commit-push — gpt-5

Implemented `#slnk` in `src/tsift` so absolute `tsift summarize --file` queries now canonicalize through symlinked checkout paths before deriving the root-relative cache key. The lookup path still preserves the user's lexical spelling as a first candidate, but absolute existing paths now also resolve back to the canonical `src/...` key stored in `summaries.db`.

Added regression coverage in `src/summarize.rs` and `tests/exit_code.rs`, and updated `SPEC.md` to document that symlinked absolute paths hit the shared summary cache.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`434` unit tests and `45` integration tests)
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `2e25b29` (`fix: canonicalize symlinked summary file lookups`).

The active backlog is now clear; only icebox items remain.
❯ #code-review
The `agent-doc` skill was auto-updated from `0.33.12` to `0.33.16`, and the binary requested a reload boundary.

Run `/compact`, then re-invoke:

`agent-doc /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
❯ #code-review
### Re: code review — gpt-5

#### Findings

1. Windows summary cache lookups still disagree with the path format written during extract (`src/tsift/src/main.rs:2675`, `src/tsift/src/main.rs:3100-3105`, `src/tsift/src/summarize.rs:557-591`, `src/tsift/src/summarize.rs:624-677`, `src/tsift/src/main.rs:2775-2780`). Extraction derives `rel_path` with OS-native separators and persists it unchanged into `Summary.file_path`, while query mode normalizes every `--file` lookup candidate to forward slashes before calling `get_by_file()`. On Windows that means extracted rows land as `src\\lib.rs`, but lookups ask for `src/lib.rs`, so valid cached summaries can still miss even after the new symlink canonicalization fix.

#### Verification

- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`434` unit tests and `45` integration tests).
- `cargo test --manifest-path src/tsift/Cargo.toml summarize_file_query -- --nocapture` passed on this Linux workspace, which confirms the Unix/symlink flow but also shows the relevant regression coverage is Unix-only today.
- I did not reproduce the finding end-to-end because this workspace is Linux; the issue comes from the current read/write path mismatch in source.

Top remaining backlog item: `#wsep`.
❯ do [#wsep]. spec-test-build-install-commit-push
### Re: #wsep spec-test-build-install-commit-push — gpt-5

Implemented `#wsep` in `src/tsift` so summary cache file keys are now normalized to `/` separators at the storage boundary, and summarize file reads, stale-row deletes, and currentness checks also tolerate legacy `\` rows. New extracts now write the canonical root-relative key on every platform, while older Windows cache rows remain readable until they are rewritten.

Added regression coverage in `src/summarize.rs` and `tests/exit_code.rs`, and updated `SPEC.md` to document the canonical cache-key contract.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`436` unit tests and `46` integration tests)
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `5f38aa7` (`fix: normalize summary cache file keys`).

The active backlog is now clear; only icebox items remain.
❯ #code-review
### Re: code review — gpt-5

#### Findings

1. Relative summarize extract scopes are handled lexically instead of canonically, which breaks both `--diff` filtering and the cache keys written during extract. [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2605), [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3096), and [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3100) join `--extract` onto the anchor without normalization, filter changed files with a lexical `starts_with`, and derive the persisted key from that same path spelling. I reproduced the regression with a temp git repo: after modifying `src/lib.rs`, `tsift summarize --extract src/../src --diff --path <repo> --compact` incorrectly prints `No files to extract.` even though the file changed. The non-diff path leaks the same bug in the opposite direction: the extract error surface already reports `src/../src/lib.rs`, which means successful extracts under that scope would persist malformed keys that later canonical `--file src/lib.rs` lookups would not hit.

2. `tsift summarize --stats` still reports a fake stale count. [src/tsift/SPEC.md](/home/brian/work/btakita/agent-loop/src/tsift/SPEC.md:825) promises cache staleness in stats output, but [src/tsift/src/summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:341) hard-codes `stale_count: 0` for every database. I verified the user-visible symptom by creating a synthetic `summaries.db` containing only a row for a missing file and then running `tsift summarize --stats --json --path <repo>`; the CLI still returned `"stale_count":0`. That makes the JSON contract misleading precisely when the cache needs cleanup attention.

#### Verification

- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`436` unit tests and `46` integration tests).
- Reproduced finding 1 with a temp git repo by modifying `src/lib.rs` and running `tsift summarize --extract src/../src --diff --path <repo> --compact`, which returned `No files to extract.`.
- Reproduced finding 2 with a synthetic `summaries.db` containing a stale row; `tsift summarize --stats --json` still returned `"stale_count":0`.

Follow-up items added to backlog. Top remaining backlog item: `#scop`.
❯ do [#scop]. spec-test-build-install-commit-push
### Re: #scop spec-test-build-install-commit-push — gpt-5

Implemented `#scop` in `src/tsift` so summarize extract scopes now canonicalize existing paths and collapse lexical `.` / `..` segments before scope filtering and cache-key derivation. That fixes the false `No files to extract.` path for `tsift summarize --extract src/../src --diff --path <repo> --compact`, keeps the error surface on canonical `src/...` spellings, and also normalizes absolute symlinked scope paths.

Added regression coverage in `src/main.rs` and `tests/exit_code.rs`, exposed the shared lexical normalizer from `src/summarize.rs`, and updated `SPEC.md` to document normalized extract-scope handling.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`438` unit tests and `47` integration tests)
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `323e73c` (`fix: normalize summarize extract scopes`).

Top remaining backlog item: `#stct`.
Implemented `#scop` in `src/tsift` and pushed `main` as `323e73c` (`fix: normalize summarize extract scopes`). The summarize extract path now canonicalizes existing scope paths and collapses lexical `.` / `..` segments before diff filtering and cache-key derivation, so `src/../src` and symlinked scope spellings no longer yield false `No files to extract.` results or malformed `src/../src/...` keys.

Updated `SPEC.md`, added regressions in `src/main.rs` and `tests/exit_code.rs`, and ran the full requested flow: `cargo test` (`438` unit tests, `47` integration tests), `cargo clippy --all-targets --all-features -- -D warnings`, `cargo build`, `cargo install --path src/tsift`, and `tsift --version` (`0.1.21`). The session document was finalized successfully and `agent-doc session-check` passed. Top remaining backlog item is `#stct`.
❯ do #stct. spec-test-build-install-commit-push
### Re: #stct spec-test-build-install-commit-push — gpt-5

Implemented `#stct` in `src/tsift` so `tsift summarize --stats` now computes a real stale-file count from the project root instead of hard-coding `stale_count: 0`. The stats path now treats both missing source files and content-hash mismatches as stale, deduplicates canonical file keys before counting, and surfaces the stale count in compact and human-readable CLI output.

Added regression coverage in `src/summarize.rs` and `tests/exit_code.rs`, and updated `SPEC.md` to document the stale-count contract.

Verification:
- `cargo test` passed (`439` unit tests and `48` integration tests)
- `cargo clippy --all-targets --all-features -- -D warnings` passed
- `cargo build` passed
- `cargo install --path .` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `caa135b` (`fix: report summarize stale stats`).

The active backlog is now clear; only icebox items remain.
#code-review
### Re: code review — gpt-5

#### Findings

1. `summarize --stats` now trusts `Summary.file_path` values from `summaries.db` as filesystem paths and can walk outside the project root (`src/tsift/src/summarize.rs:356-379`). `cached_file_paths()` only normalizes separators, then `stale_file_count()` does `root.join(cached_path)` and reads that file. A crafted row like `../secret.rs` or an absolute path will make the stats command probe files outside the repo. I reproduced this with a synthetic cache row `../secret.rs`; `tsift summarize --stats --json --path <repo>` attempted to read `<repo>/../secret.rs` and failed with `Permission denied`. This is a real trust-boundary regression because `--stats` used to stay inside the cache DB and now its filesystem walk is driven by unvalidated DB rows.

2. `summarize --stats` fails hard on unreadable live files instead of counting them stale (`src/tsift/src/summarize.rs:372-381`). `stale_file_count()` propagates `std::fs::read()` failures directly, so a single `chmod 000` source file aborts the entire stats command. I reproduced this with a normal in-repo cached row for `src/lib.rs`; after removing read permission, `tsift summarize --stats --json` exited nonzero with `reading summary source ... Permission denied`. For a read-only diagnostic command, unreadable sources should be reported as stale or corrupt with a warning, not take down the whole stats report.

#### Verification

- Reviewed `caa135b` in `src/tsift/src/summarize.rs` and `src/tsift/tests/exit_code.rs`.
- Reproduced finding 1 with a synthetic `summaries.db` row using `../secret.rs`.
- Reproduced finding 2 with an unreadable `src/lib.rs` plus a cached summary row.

Follow-up items added to backlog. Top remaining backlog item: `#sesc`.
❯ do [#sesc]. spec-test-build-install-commit-push
### Re: #sesc spec-test-build-install-commit-push — gpt-5

Implemented `#sesc` in `src/tsift` so `tsift summarize --stats` now clamps cached summary file paths to the resolved project root before reading live source content. Absolute cache keys and lexical escape paths like `../secret.rs` are now counted as stale/corrupt instead of being opened from the filesystem.

Updated `src/summarize.rs`, added unit and CLI regression coverage, and documented the tightened stats contract in `SPEC.md`.

Verification:
- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`440` unit tests and `49` integration tests)
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path src/tsift/Cargo.toml` passed
- `cargo install --path src/tsift` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `c82e464` (`fix: clamp summarize stats cache paths`).

Top remaining backlog item: `#srdp`.
❯ do [#srdp]. spec-test-build-install-commit-push
### Re: #srdp spec-test-build-install-commit-push — gpt-5

Implemented `#srdp` in `src/tsift` so `tsift summarize --stats` no longer aborts when a cached source file exists but cannot be read. The stats walk now counts unreadable source files as stale, records warning metadata in JSON output, and emits a warning to stderr while still completing the report. `SPEC.md` now documents that degraded-read behavior.

Verification:
- `cargo test --manifest-path Cargo.toml` passed (`441` unit tests and `50` integration tests)
- `cargo clippy --manifest-path Cargo.toml --all-targets --all-features -- -D warnings` passed
- `cargo build --manifest-path Cargo.toml` passed
- `cargo install --path .` passed
- `tsift --version` => `0.1.21`

Committed and pushed `src/tsift` `main` as `5dc9236` (`fix: degrade summarize stats unreadable files`).

The active backlog is now clear; only icebox items remain.
<!-- agent:boundary:733e9457 -->
<!-- /agent:exchange -->

<!--
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
