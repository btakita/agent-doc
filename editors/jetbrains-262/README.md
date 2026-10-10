# Agent Doc JetBrains 262 split distribution

This build is the Plugin Model v2 distribution for IntelliJ Platform 2026.2
(branch/build `262`) only. It keeps the existing plugin ID
`com.github.btakita.agent-doc` and plugin version line, with
`since-build="262" until-build="262.*"`.
The classic build in `../jetbrains` remains the distribution for 242 through
261 and is clamped to `until-build="261.*"`. `make release-version VERSION=...`
assigns fresh, distinct classic and modular update versions under the shared
Marketplace plugin ID and records both source digests. Repeating the projection
is idempotent; changed source bytes allocate a new pair above both prior update
versions, so the two ranged artifacts cannot overwrite one another.

The ZIP is one plugin, not a companion plugin. Its `lib/modules` directory
contains `agent.doc.shared.jar`, `agent.doc.frontend.jar`, and
`agent.doc.backend.jar`. The backend module integrates the existing Agent Doc
implementation; the frontend and shared modules add the typed surface-snapshot
bridge. `required-if-available` module dependencies load frontend code in a
regular IDE or JetBrains Client and backend code in a regular IDE or Remote Dev
backend.

### One-artifact role-selection contract

The distribution does not publish separate frontend and backend ZIPs and does
not guess its role from host names, environment variables, or connection state.
IntelliJ Platform selects content modules from the capabilities of the process
that is loading the same ZIP:

| Process | Platform capability | Agent Doc modules loaded |
|---|---|---|
| JetBrains Client | `intellij.platform.frontend` | shared + frontend |
| Remote Dev backend | `intellij.platform.backend` | shared + backend |
| Regular monolithic IDE | frontend + backend | shared + frontend + backend |

The shared module is always required. The frontend and backend modules are
optional where their matching platform capability is absent and required where
it is present. This is declarative process-role recognition owned by the
IntelliJ Platform plugin loader; Agent Doc must not add a second imperative
role detector that could disagree with the module/classloader boundary.

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
the exact frontend/backend/both role-selection matrix, exact module
JAR/descriptor pairing and dependencies, and module presence in the
distribution. `verifySplitModeSandboxes` proves that the complete same-artifact
module set is placed in both development sandboxes.

## Distribution and installation

The classic and 262 ZIPs are compatible updates of the same plugin ID with
non-overlapping IDE ranges. Every tag publishes one JetBrains custom-repository
listing per range (`agent-doc-jetbrains-classic.xml`,
`agent-doc-jetbrains-262.xml`), generated and validated by
`scripts/jetbrains-custom-repository.py`; each IDE process selects the update
whose `since-build`/`until-build` contains its build. Marketplace hosting of the
same two ranged updates is the equivalent alternative but is not automated. See
[`docs/reference/jetbrains-distribution.md`](../../docs/reference/jetbrains-distribution.md).

This package has no restart-free dynamic upgrade entry point: unlike the
classic plugin jar, `agent.doc-<pluginVersion>.jar` carries no
`JetBrainsPluginUpgradeBootstrap` launcher. `agent-doc plugin install` against a
running 262 IDE replaces the files and reports that a restart is required
(`declined_by=modular_package`).

Remote Dev plugin synchronization currently resolves the other side only for
Marketplace-hosted plugins. A custom repository must be configured and the
same update installed on both sides; installing the 262 ZIP locally on only the
backend or only JetBrains Client does not copy it to the other process. For
local ZIP testing, install the same ZIP explicitly in both backend and
Client/Gateway, or use `runIdeSplitMode`. A one-sided install is unsupported and
the surface bridge must be treated as unavailable.

Every Agent Doc tag packages the modular ZIP as
`agent-doc-jetbrains-262-<pluginVersion>.zip` alongside the compatibility-ranged
classic ZIP and the VS Code package. The package-generation fence tracks both
the modular build and the classic implementation sources reused by its backend.
`make check` builds and verifies both compatibility-ranged ZIPs plus the modular
backend/frontend sandboxes, while the tag workflow rejects equal update versions
and copies both JetBrains artifacts by exact versioned name. The
generic `agent-doc plugin install jetbrains` asset resolver deliberately accepts
only the classic numeric filename shape; it must not choose the exact-262 ZIP by
shared prefix until it can prove the target IDE build. Marketplace/custom-repo
compatibility selection or an explicit two-sided local install owns the modular
path in the meantime.

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
