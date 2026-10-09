# JetBrains 262 preview provenance

The Remote Dev acceptance gate for GH #218 is pinned to the exact preview built
from PR #222 head `c39de1b045b8b5238f8e5b7fa6870062798d4114`:

- release: `jetbrains-262-preview-c39de1b`
- asset: `agent-doc-jetbrains-262-0.2.511.zip`
- SHA-256: `44474f85ab7f3d8ababb0f99feba244986e24c1b663f6d699895aa9e0819b0d9`
- plugin range: `since-build=262`, `until-build=262.*`

The recorded manifest is
[`preview-artifacts/jetbrains-262-preview-c39de1b.json`](../../preview-artifacts/jetbrains-262-preview-c39de1b.json).
At recording time, a fresh release download had the checksum above, and the
GitHub Releases API reported the same SHA-256 digest for asset id `625841715`.

Download the ZIP independently for the Remote Dev backend and for
Client/Gateway. Before installing either copy or starting the acceptance run,
verify both in one command:

```bash
make verify-jetbrains-262-preview \
  BACKEND_ZIP=/path/from/backend/agent-doc-jetbrains-262-0.2.511.zip \
  CLIENT_GATEWAY_ZIP=/path/from/client/agent-doc-jetbrains-262-0.2.511.zip
```

The verifier fails closed unless:

- PR #222 still has the recorded head and the prerelease still targets it;
- the release and asset ids, timestamps, size, and GitHub digest still match;
- both local files have the recorded byte length and SHA-256;
- both ZIPs contain the frontend, shared, and backend module JARs; and
- `plugin.xml` has the recorded plugin id, version, and exact 262 build range.

Any failure means the bytes are not admissible for the GH #218 acceptance
record. Do not substitute another download, rebuild, version, or PR head; record
a new manifest and acceptance target instead.
