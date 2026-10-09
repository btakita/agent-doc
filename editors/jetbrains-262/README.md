# Agent Doc JetBrains 262 split distribution

This build is the Plugin Model v2 distribution for IntelliJ Platform 2026.2
(branch/build `262`) only. It keeps the existing plugin ID
`com.github.btakita.agent-doc` and plugin version line, with
`since-build="262" until-build="262.*"`.
The classic build in `../jetbrains` remains the distribution for 242 through
261. Because this post-release feature branch is forbidden from bumping the
classic package generation, its source and open-ended range stay byte-identical.
Before the two alternatives are published, an authorized release must assign
distinct update versions and clamp the classic artifact to `until-build="261.*"`
through Marketplace metadata or its package-generation bump.

The ZIP is one plugin, not a companion plugin. Its `lib/modules` directory
contains `agent.doc.shared.jar`, `agent.doc.frontend.jar`, and
`agent.doc.backend.jar`. The backend module integrates the existing Agent Doc
implementation; the frontend and shared modules add the typed surface-snapshot
bridge. `required-if-available` module dependencies load frontend code in a
regular IDE or JetBrains Client and backend code in a regular IDE or Remote Dev
backend.

## Build and split-mode checks

IntelliJ 2026.2 requires a Java 25 toolchain for compilation in this build.

```shell
gradle test buildPlugin verifySplitArtifact
gradle verifyPlugin
gradle runIdeSplitMode
```

The build sets `splitMode = true` and `pluginInstallationTarget = BOTH`.
`runIdeSplitMode` therefore installs the same built plugin into the local
backend and frontend sandboxes. `verifySplitArtifact` checks the 262 range,
root content declarations, exact module JAR/descriptor pairing, and module
presence in the distribution.

## Distribution and installation

Publish the classic and 262 ZIPs as compatible updates of the same Marketplace
plugin ID, with non-overlapping IDE ranges. Marketplace selects the compatible
artifact and checks/installs the modular plugin on both backend and frontend
where its module dependencies are satisfied. A custom plugin repository can
provide the same two ranged updates, and each IDE process selects the update
whose `since-build`/`until-build` contains its build.

Remote Dev plugin synchronization currently resolves the other side only for
Marketplace-hosted plugins. A custom repository must be configured and the
same update installed on both sides; installing the 262 ZIP locally on only the
backend or only JetBrains Client does not copy it to the other process. For
local ZIP testing, install the same ZIP explicitly in both backend and
Client/Gateway, or use `runIdeSplitMode`. A one-sided install is unsupported and
the surface bridge must be treated as unavailable.

## Fail-closed integration boundary

Frontend snapshots are authenticated and generation-fenced on the backend,
then sent as one `editor_view_snapshot_observe` controller request. They are
never fanned out through legacy `editor_surface_observe`, because that path can
apply focus/layout effects before main-versus-detached policy sees the complete
snapshot. The payload sets `terminal_capable=true` only after the exact-262
runtime probe verifies every internal editor/terminal shape used by the isolated
adapter. Otherwise it remains false and controller policy freezes. A controller
without the new atomic ingress rejects the command, so no detached surface
reaches the legacy effect path.

The controller returns an explicit, revision-fenced presentation list. The
frontend adapter resolves the exact split containing the document, mounts a
controller-session terminal or non-writable placeholder, restores originals on
close/rejoin, and returns typed applied/refused/stale receipts. These exact-262
internal effects remain unproven in a real Remote Dev backend + Client run, so
issue #218 remains open despite compile, unit, packaging, and split-sandbox gates.
