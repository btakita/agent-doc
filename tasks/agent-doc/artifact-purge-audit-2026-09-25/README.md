# GitHub Actions artifact durability audit

Generated: 2026-09-25T17:02:35.723915+00:00

Repository: [btakita/agent-doc](https://github.com/btakita/agent-doc)

No artifacts were deleted. This report is a read-only candidate handoff.

## Result

- Live nonexpired artifacts: 3,247, totaling 52,903,808,136 bytes (52.90 GB; 49.27 GiB).
- Verified delete candidates: 2,781, totaling 45,557,799,985 bytes (45.56 GB; 42.43 GiB).
- Not candidates: 466, totaling 7,346,008,151 bytes (7.35 GB; 6.84 GiB).
- Candidate proof requires an exact tagged version plus an exact target-specific GitHub Release asset or PyPI filename.
- The candidate CSV is sorted by workflow, run, artifact class, and artifact id; each row records id, size, expiry, run URL, and counterpart URL.

## By workflow and artifact class

| Workflow | Artifact class | Artifacts | Verified | Unresolved | Candidate bytes |
|---|---|---:|---:|---:|---:|
| Book | github-pages | 14 | 0 | 14 | 0 |
| Deploy mdBook site to Pages | github-pages | 16 | 0 | 16 | 0 |
| PyPI | wheel-aarch64-apple-darwin | 257 | 156 | 101 | 2,488,455,545 |
| PyPI | wheel-bootstrap | 13 | 13 | 0 | 454,389 |
| PyPI | wheel-x86_64-apple-darwin | 257 | 156 | 101 | 2,727,295,053 |
| PyPI | wheel-x86_64-pc-windows-msvc | 254 | 154 | 100 | 2,647,272,693 |
| PyPI | wheel-x86_64-unknown-linux-gnu | 257 | 156 | 101 | 2,777,551,715 |
| Release | agent-doc-aarch64-apple-darwin | 418 | 413 | 5 | 6,174,599,175 |
| Release | agent-doc-aarch64-unknown-linux-gnu | 420 | 412 | 8 | 6,302,050,673 |
| Release | agent-doc-x86_64-apple-darwin | 420 | 412 | 8 | 6,744,090,041 |
| Release | agent-doc-x86_64-pc-windows-msvc | 414 | 411 | 3 | 6,917,662,119 |
| Release | agent-doc-x86_64-unknown-linux-gnu | 420 | 412 | 8 | 7,013,377,911 |
| Release | agent-doc-x86_64-unknown-linux-musl | 87 | 86 | 1 | 1,764,990,671 |

## Unresolved classes

| Reason | Artifacts | Bytes |
|---|---:|---:|
| matching PyPI platform file is missing | 395 | 6,622,546,457 |
| matching GitHub Release asset is missing | 33 | 508,565,612 |
| GitHub Pages artifact has no Release/PyPI counterpart | 30 | 117,705,105 |
| workflow run is not associated with a version tag | 8 | 97,190,977 |

## Method

- Enumerated every page of [the Actions artifacts API](https://api.github.com/repos/btakita/agent-doc/actions/artifacts).
- Resolved every distinct workflow run id through the Actions runs API so the CSV can be grouped by workflow and run.
- Enumerated every page of [GitHub Releases](https://api.github.com/repos/btakita/agent-doc/releases) and matched native artifacts to the exact tag and archive filename.
- Read the [PyPI project JSON](https://pypi.org/pypi/agent-doc/json) and matched wheel artifacts to the exact version and platform filename.
- Excluded untagged artifacts, GitHub Pages artifacts, missing platform files, and any class without an exact durable URL.

## Files

- `artifact-delete-candidates.csv` — operator delete-candidate list; no deletion command is included.
- `artifact-unresolved.csv` — artifacts withheld from the candidate list and the exact reason.
- `artifact-audit-summary.json` — machine-readable totals and group summaries.
