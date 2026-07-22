---
name: reference_cargo_publish_token_passage
description: The working crates.io publish token lives in passage at btak/CARGO_REGISTRY_TOKEN; ~/.cargo/credentials.toml is stale (403).
metadata:
  type: reference
---

For `cargo publish` to crates.io under the `btakita` account, the token in
`~/.cargo/credentials.toml` is **stale/revoked** (publish returns
`403 authentication failed`). The current valid token is stored in `passage` at
`btak/CARGO_REGISTRY_TOKEN`.

Publish without touching the credentials file or echoing the secret:

```bash
CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)" cargo publish
```

Verified 2026-07-06 publishing `eval-runner v0.1.0` (first crates.io release).
Relates to [[reference_lazily_release_targets]] (manual `cargo publish` flow for
btakita Rust crates).
