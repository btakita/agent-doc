# JetBrains 262 preview provenance

The Remote Dev acceptance gate for GH #218 is pinned to the exact preview built
from PR #222 head `34842a23a57739c9ad97e0de7c80b036770a7dc9`, the first head
with green CI (run `38002449411`) after the `#lz262resolve` and `#ci25gradle`
fixes:

- release: `jetbrains-262-preview-34842a2` (release id `408534966`)
- asset: `agent-doc-jetbrains-262-0.2.511.zip` (asset id `626772959`, 2556136 bytes)
- SHA-256: `2ef328588d1491446f1a45a83775ee220c13507f5cdd9bca4f5c2111f9e864a7`
- plugin range: `since-build=262`, `until-build=262.*`

It supersedes `jetbrains-262-preview-c39de1b`, which was built before those CI
fixes landed. That older prerelease is no longer admissible for the acceptance
record.

The recorded manifest is
[`preview-artifacts/jetbrains-262-preview-34842a2.json`](../../preview-artifacts/jetbrains-262-preview-34842a2.json).
At recording time, two fresh release downloads had the checksum above, and the
GitHub Releases API reported the same SHA-256 digest for asset id `626772959`.

Download the ZIP independently for the Remote Dev backend and for
Client/Gateway. Before installing either copy or starting the acceptance run,
verify both in one command:

```bash
make verify-jetbrains-262-preview \
  BACKEND_ZIP=/path/from/backend/agent-doc-jetbrains-262-0.2.511.zip \
  CLIENT_GATEWAY_ZIP=/path/from/client/agent-doc-jetbrains-262-0.2.511.zip
```

The verifier fails closed unless:

- PR #222's head is the recorded source commit, or a descendant of it whose
  only changes since that commit are provenance bookkeeping (`preview-artifacts/`,
  `scripts/verify_jetbrains_preview.py`, and this document), as reported by the
  GitHub compare API. Recording the manifest is itself a commit on the PR
  branch, so the head always advances past the source commit by that much. Any
  other change, a rebase, a diverged history, or a truncated file list is
  refused;
- the prerelease still targets the recorded source commit;
- the release and asset ids, timestamps, size, and GitHub digest still match;
- both local files have the recorded byte length and SHA-256;
- both ZIPs contain the frontend, shared, and backend module JARs; and
- `plugin.xml` has the recorded plugin id, version, and exact 262 build range.

Any failure means the bytes are not admissible for the GH #218 acceptance
record. Do not substitute another download, rebuild, version, or PR head; record
a new manifest and acceptance target instead.
