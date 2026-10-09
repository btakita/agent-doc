# Agent Doc JetBrains 262 split distribution

This build is the Plugin Model v2 distribution for IntelliJ Platform 2026.2
(branch/build `262`) and later. It keeps the existing plugin ID
`com.github.btakita.agent-doc` and plugin version line, but has `since-build="262"`.
The classic build in `../jetbrains` remains the distribution for 242 through
261 and its build patches `until-build="261.*"`. The two distributions need
distinct Marketplace update versions at release time because Marketplace
update versions are unique; an IDE then selects the newest update compatible
with its build. This branch intentionally does not bump or publish either
version.

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
snapshot. The payload sets `terminal_capable=false` until a frontend terminal
host is proven, so controller policy freezes rather than authorizing a legacy
main-window effect. A controller without the new atomic ingress rejects the
command, so no detached surface reaches the legacy effect path.

The workspace controller implements the atomic ingress/fold, durable binding,
and main-layout exclusion policy. A per-surface frontend terminal host,
controller lifecycle-effect settlement, and main-owned placeholder rendering
are still required before `terminal_capable` may become true. Until those
effects and their receipts are proven, issue #218 is not complete and this
artifact intentionally cannot attach a terminal to a detached window.
