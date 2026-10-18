# GitHub Actions artifact durability audit

Generated: 2026-10-18T07:49:44.017228+00:00

Repository: [btakita/agent-doc](https://github.com/btakita/agent-doc)

No artifacts were deleted. This report is a read-only candidate handoff.

## Result

- Live nonexpired artifacts: 1,014, totaling 17,129,046,855 bytes (17.13 GB; 15.95 GiB).
- Verified delete candidates: 551, totaling 9,551,975,928 bytes (9.55 GB; 8.90 GiB).
- Not candidates: 463, totaling 7,577,070,927 bytes (7.58 GB; 7.06 GiB).
- Candidate proof requires an exact tagged version plus an exact target-specific GitHub Release asset or PyPI filename.
- The candidate CSV is sorted by workflow, run, artifact class, and artifact id; each row records id, size, expiry, run URL, counterpart URL, and counterpart durability.

## Authorized durable-only purge

Executed 2026-10-18T07:50Z with the explicit repository authorization and
`--durability durable`:

- Deleted all 18 GitHub-Release-backed candidates, totaling 542,393,884 bytes.
- Refused all 533 PyPI-backed candidates, totaling 9,009,582,044 bytes.
- Reported 0 skipped and 0 failed deletions during execution.
- A post-delete live verification found 0 remaining durable deletable artifacts;
  all 18 deleted ids were absent from the live API, and all 533 perishable rows
  remained refused.
- The 463 withheld artifacts were outside the candidate set and were never
  eligible for deletion.

The dry-run, execution, and post-delete reports are committed beside this
handoff as `purge-dry-run-durable.json`, `purge-execute-durable.json`, and
`purge-post-delete-verification.json`.

## Counterpart durability

Every candidate below has an exact durable counterpart **right now**. Durability records how long that proof survives, and it is not the same for both classes.

| Durability | Counterpart kinds | Candidates | Bytes | Versions | Oldest still backed |
|---|---|---:|---:|---:|---|
| durable | github_release | 18 | 542,393,884 | 3 | v0.35.516 |
| perishable | pypi | 533 | 9,009,582,044 | 145 | v0.35.119 |

- `durable` — a GitHub Release asset. It survives until someone deletes the release, so the counterpart proof does not decay on its own.
- `perishable` — a PyPI file. PyPI prunes old distributions: between 2026-09-25 and 2026-09-27 it dropped the wheels for 34 agent-doc versions, which invalidated 105 rows a two-day-old audit had already called deletable and left the Actions copy as the only copy. Compare the oldest still-backed version above against the project's current prune floor before authorizing these rows.
- To authorize only the non-decaying rows, run the executor with `--durability durable`; it then refuses every perishable-counterpart row the same way it refuses a withheld one.

## By workflow and artifact class

| Workflow | Artifact class | Artifacts | Verified | Unresolved | Candidate bytes |
|---|---|---:|---:|---:|---:|
| Book | github-pages | 13 | 0 | 13 | 0 |
| Deploy mdBook site to Pages | github-pages | 13 | 0 | 13 | 0 |
| PyPI | wheel-aarch64-apple-darwin | 231 | 130 | 101 | 2,112,180,666 |
| PyPI | wheel-bootstrap | 16 | 16 | 0 | 489,788 |
| PyPI | wheel-x86_64-apple-darwin | 231 | 130 | 101 | 2,315,353,410 |
| PyPI | wheel-x86_64-pc-windows-msvc | 228 | 128 | 100 | 2,239,984,492 |
| PyPI | wheel-x86_64-unknown-linux-gnu | 232 | 129 | 103 | 2,341,573,688 |
| Release | agent-doc-aarch64-apple-darwin | 7 | 3 | 4 | 82,733,900 |
| Release | agent-doc-aarch64-unknown-linux-gnu | 10 | 3 | 7 | 84,472,568 |
| Release | agent-doc-x86_64-apple-darwin | 10 | 3 | 7 | 90,872,295 |
| Release | agent-doc-x86_64-pc-windows-msvc | 6 | 3 | 3 | 95,314,236 |
| Release | agent-doc-x86_64-unknown-linux-gnu | 10 | 3 | 7 | 93,990,311 |
| Release | agent-doc-x86_64-unknown-linux-musl | 4 | 3 | 1 | 95,010,574 |
| Release | editor-packages | 3 | 0 | 3 | 0 |

## Unresolved classes

| Reason | Artifacts | Bytes |
|---|---:|---:|
| matching PyPI platform file is missing | 405 | 6,959,984,119 |
| matching GitHub Release asset is missing | 29 | 470,323,642 |
| GitHub Pages artifact has no Release/PyPI counterpart | 26 | 121,953,854 |
| unsupported artifact class | 3 | 24,809,312 |

## Method

- Enumerated every page of [the Actions artifacts API](https://api.github.com/repos/btakita/agent-doc/actions/artifacts).
- Resolved every distinct workflow run id through the Actions runs API so the CSV can be grouped by workflow and run.
- Enumerated every page of [GitHub Releases](https://api.github.com/repos/btakita/agent-doc/releases) and matched native artifacts to the exact tag and archive filename.
- Read the [PyPI project JSON](https://pypi.org/pypi/agent-doc/json) and matched wheel artifacts to the exact version and platform filename.
- Excluded untagged artifacts, GitHub Pages artifacts, missing platform files, and any class without an exact durable URL.

## Purging

This generator never deletes. `scripts/purge-actions-artifacts.py` is the only
command that may call the artifact deletion endpoint, and it is dry run by default:

- Deletion requires **both** `--execute` and `--authorize-deletion <owner/repo>`,
  whose value must equal this audit's repository.
- Every candidate is re-derived against the live Actions API at execution time using
  this generator's own candidacy rule. A live artifact that is gone, expired, renamed,
  resized, or no longer backed by an exact durable counterpart is refused.
- Every artifact id in `artifact-unresolved.csv` is refused unconditionally, including
  when named explicitly with `--only`.
- `--durability durable` narrows the plan to non-decaying counterparts; a  perishable row is then refused even when named explicitly with `--only`.
- A structurally inconsistent handoff (candidate/withheld overlap, a candidate with no
  counterpart, summary totals that disagree with the CSVs, a generation older than
  `--max-handoff-age-days`) aborts before any network call.
- A plan above `--max-deletions` aborts rather than deleting a truncated subset.

Run `python3 scripts/purge-actions-artifacts.py --self-test` for the offline
refusal-path regressions; `make check` runs it via `artifact-purge-check`.

## Files

- `artifact-delete-candidates.csv` — operator delete-candidate list; this generator issues no deletion. `counterpart_durability` records whether each row's counterpart can decay.
- `artifact-unresolved.csv` — artifacts withheld from the candidate list and the exact reason. The purge executor refuses every id listed here.
- `artifact-audit-summary.json` — machine-readable totals and group summaries.
- `purge-dry-run-durable.json` — pre-execution durable-only plan and refusal evidence.
- `purge-execute-durable.json` — authorized deletion receipt for the durable-only plan.
- `purge-post-delete-verification.json` — live proof that the deleted ids are absent and no durable candidate remains.
