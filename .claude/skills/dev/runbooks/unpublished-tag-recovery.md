# Unpublished Tag Recovery

When a version tag exists but was never published (crates.io, PyPI, GitHub Release), move the tag forward to HEAD rather than bumping to a new version.

## Steps

1. Confirm the tag was never published on any target
2. Delete local tag: `git tag -d v<version>`
3. Re-create at HEAD: `git tag v<version>`
4. Force-push: `git push origin v<version> --force`
5. Proceed with the normal release workflow
