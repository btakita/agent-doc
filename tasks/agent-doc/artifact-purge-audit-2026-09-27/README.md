# GitHub Actions artifact durability audit

Generated: 2026-09-27T05:56:08.338552+00:00

Repository: [btakita/agent-doc](https://github.com/btakita/agent-doc)

No artifacts were deleted. This report is a read-only candidate handoff.

## Result

- Live nonexpired artifacts: 3,237, totaling 52,809,917,681 bytes (52.81 GB; 49.18 GiB).
- Verified delete candidates: 2,666, totaling 43,834,598,531 bytes (43.83 GB; 40.82 GiB).
- Not candidates: 571, totaling 8,975,319,150 bytes (8.98 GB; 8.36 GiB).
- Candidate proof requires an exact tagged version plus an exact target-specific GitHub Release asset or PyPI filename.
- The candidate CSV is sorted by workflow, run, artifact class, and artifact id; each row records id, size, expiry, run URL, counterpart URL, and counterpart durability.

## Counterpart durability

Every candidate below has an exact durable counterpart **right now**. Durability records how long that proof survives, and it is not the same for both classes.

| Durability | Counterpart kinds | Candidates | Bytes | Versions | Oldest still backed |
|---|---|---:|---:|---:|---|
| durable | github_release | 2,136 | 34,825,051,886 | 406 | v0.34.69 |
| perishable | pypi | 530 | 9,009,546,645 | 142 | v0.35.119 |

- `durable` — a GitHub Release asset. It survives until someone deletes the release, so the counterpart proof does not decay on its own.
- `perishable` — a PyPI file. PyPI prunes old distributions: between 2026-09-25 and 2026-09-27 it dropped the wheels for 34 agent-doc versions, which invalidated 105 rows a two-day-old audit had already called deletable and left the Actions copy as the only copy. Compare the oldest still-backed version above against the project's current prune floor before authorizing these rows.
- To authorize only the non-decaying rows, run the executor with `--durability durable`; it then refuses every perishable-counterpart row the same way it refuses a withheld one.

## By workflow and artifact class

| Workflow | Artifact class | Artifacts | Verified | Unresolved | Candidate bytes |
|---|---|---:|---:|---:|---:|
| Book | github-pages | 16 | 0 | 16 | 0 |
| Deploy mdBook site to Pages | github-pages | 14 | 0 | 14 | 0 |
| PyPI | wheel-aarch64-apple-darwin | 257 | 130 | 127 | 2,112,180,666 |
| PyPI | wheel-bootstrap | 13 | 13 | 0 | 454,389 |
| PyPI | wheel-x86_64-apple-darwin | 257 | 130 | 127 | 2,315,353,410 |
| PyPI | wheel-x86_64-pc-windows-msvc | 254 | 128 | 126 | 2,239,984,492 |
| PyPI | wheel-x86_64-unknown-linux-gnu | 257 | 129 | 128 | 2,341,573,688 |
| Release | agent-doc-aarch64-apple-darwin | 416 | 411 | 5 | 6,157,728,386 |
| Release | agent-doc-aarch64-unknown-linux-gnu | 418 | 410 | 8 | 6,284,353,732 |
| Release | agent-doc-x86_64-apple-darwin | 418 | 410 | 8 | 6,725,421,993 |
| Release | agent-doc-x86_64-pc-windows-msvc | 412 | 409 | 3 | 6,898,849,691 |
| Release | agent-doc-x86_64-unknown-linux-gnu | 418 | 410 | 8 | 6,993,707,413 |
| Release | agent-doc-x86_64-unknown-linux-musl | 87 | 86 | 1 | 1,764,990,671 |

## Unresolved classes

| Reason | Artifacts | Bytes |
|---|---:|---:|
| matching PyPI platform file is missing | 500 | 8,254,029,207 |
| matching GitHub Release asset is missing | 33 | 508,565,612 |
| GitHub Pages artifact has no Release/PyPI counterpart | 30 | 115,533,354 |
| workflow run is not associated with a version tag | 8 | 97,190,977 |

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
