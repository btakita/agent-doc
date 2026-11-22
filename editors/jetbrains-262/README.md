# Agent Doc JetBrains modular distributions

This source project builds the one Plugin Model v2 distribution for IntelliJ
Platform 2026.2 (`262`) through 2026.3 (`263.*`), `agent-doc-jetbrains-262-<pluginVersion>.zip`
(`#jbdocdrift`). It keeps the existing plugin ID
`com.github.btakita.agent-doc`; `gradle.properties` sets the artifact name, the
compile platform (`platformVersion`, 262), the upper verification platform
(`upperPlatformVersion`, 263), and the compatibility range. The classic source in
`../jetbrains` produces only the distribution for 242 through 261. Separate
exact-262 and exact-263 ZIPs (the latter configured by the removed
`../jetbrains-263`) shipped before the combined ZIP.
`make release-version VERSION=...` assigns fresh, distinct versions to both
updates under the shared Marketplace plugin ID and records their source digests.
Repeating the projection is idempotent; changed source bytes allocate fresh
versions above every prior update, so the ranged artifacts cannot overwrite one another.

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

IntelliJ 2026.2 and 2026.3 require a Java 25 toolchain for compilation in this build.

```shell
gradle test buildPlugin verifySplitArtifact
gradle verifyPlugin        # IU platformVersion (262) and upperPlatformVersion (263)
gradle -PplatformVersion=263.6800.24 test   # what `make jetbrains-263-check` runs
gradle runIdeSplitMode
```

`verifyPlugin` also accepts local IDE homes, for example an installed Rider
2026.2 and IntelliJ IDEA 2026.3:
`gradle verifyPlugin "-PverifyLocalIdePaths=/opt/rider-2026.2,/opt/idea-IU-263"`.

The build sets `splitMode = true` and `pluginInstallationTarget = BOTH`.
`runIdeSplitMode` therefore installs the same plugin into the local backend and
frontend sandboxes. `verifySplitArtifact` checks the patched 262..263.* range,
the exact frontend/backend/both role-selection matrix, exact module
JAR/descriptor pairing and dependencies, module presence, and that the isolated
backend module embeds its private Lazily runtime instead of relying on a root
plugin-library classloader. `verifySplitModeSandboxes` proves that the complete
same-artifact module set, including those backend runtime classes, is placed in
both development sandboxes.

## Distribution and installation

The classic and modular ZIPs are compatible updates of the same plugin ID with
non-overlapping IDE ranges. Every tag publishes one JetBrains custom-repository
listing per range (`agent-doc-jetbrains-classic.xml` and
`agent-doc-jetbrains-262.xml`, plus `agent-doc-jetbrains-263.xml`, a
byte-identical alias of the modular listing kept for existing 263
subscribers), generated and validated by
`scripts/jetbrains-custom-repository.py`; each IDE process selects the update
whose `since-build`/`until-build` contains its build. The same release job first
validates and publishes both updates to Marketplace, which synchronizes the
selected modular ZIP across Remote Dev peers; custom repositories remain the
manual fallback. See
[`docs/reference/jetbrains-distribution.md`](../../docs/reference/jetbrains-distribution.md).

This package has no restart-free dynamic upgrade entry point: unlike the
classic plugin jar, `agent.doc-<pluginVersion>.jar` carries no
`JetBrainsPluginUpgradeBootstrap` launcher. `agent-doc plugin install` against a
running modular IDE replaces the files and reports that a restart is required
(`declined_by=modular_package`).

Remote Dev plugin synchronization currently resolves the other side only for
Marketplace-hosted plugins. When using a custom repository or local ZIP, the
same update must be installed on both sides; installing a modular ZIP on only the
backend or only JetBrains Client does not copy it to the other process. For
local ZIP testing, install the same ZIP explicitly in both backend and
Client/Gateway, or use `runIdeSplitMode`. A one-sided install is unsupported and
the pane-tagged surface bridge must be treated as unavailable. If a remote
client remains connected without frontend authority after schema negotiation,
the backend logs `frontend layout publisher unavailable` with
`readiness=blocked`,
`readiness_reason=frontend_authority_absent_or_generation_mismatched`,
`geometry_reason=flat_evidence_has_no_column_geometry`, and the two-sided
same-version install/restart remedy; route failures surface the same actionable
reason. When the frontend module is absent, this is a distribution/readiness
fault, not geometry the backend may reconstruct and not a transient that
retrying can repair. When both modules are loaded but the backend project is
not resolvable during frontend startup, the lease flow completes without
authority; the frontend retries that normal empty completion with bounded
1–30 second backoff. The client and backend logs then pair
`frontend/backend ingress completed` with either `ingress deferred` or
`ingress accepted`, so a transport/module fault is distinguishable from the
transient project-registration race. The backend does not observe client-session
selection while no frontend snapshot has been accepted: that flow has no supported
Marketplace API. Structural routing remains blocked and retains the existing tmux layout.
Detached-window identity and frontend presentation remain unavailable until the
matching frontend module connects and publishes an accepted snapshot.

Every Agent Doc tag packages the modular ZIP as
`agent-doc-jetbrains-262-<pluginVersion>.zip` alongside the compatibility-ranged
classic ZIP and the VS Code package. The package-generation fence tracks both
the modular build and the classic implementation sources reused by its backend.
`make check` builds and verifies both compatibility-ranged ZIPs plus the modular
backend/frontend sandboxes, plugin-verifies the modular ZIP against IntelliJ
IDEA 262 and 263, and runs the modular tests on the 263 platform, while the tag
workflow rejects equal update versions and copies the artifacts by exact
versioned name.
`agent-doc plugin install jetbrains` derives the target build from the
versioned IDE data directory and selects the classic ZIP for 242-261 or the
modular ZIP for 262 and 263. The installer fails closed if it cannot prove a supported
build. Marketplace synchronizes the compatible ZIP across Remote Dev peers.
For a local or custom-repository install, install the same ranged ZIP in each
peer, using `--plugins-dir <PATH>` when needed.

## Fail-closed integration boundary

Frontend snapshots are authenticated and generation-fenced on the backend,
then sent as one `editor_view_snapshot_observe` controller request. They are
never fanned out through legacy `editor_surface_observe`, because that path can
apply focus/layout effects before main-versus-detached policy sees the complete
snapshot. The payload sets `terminal_capable=true` only after the exact-262
runtime probe (a 262 baseline build, so never on 263) verifies every internal
editor/terminal shape used by the isolated adapter. Otherwise it remains false and controller policy freezes. A controller
without the new atomic ingress rejects the command, so no detached surface
reaches the legacy effect path.

The main window's layout reaches pane layout only after that atomic request has
been delivered (GH #320 option B). Surface snapshot schema 2 carries each editor
window's measured on-screen bounds; the backend derives the main window's columns
from them with the classic `LayoutDetector` grouping and forwards those columns
into the controller surface Source (`editor_surface_observe`) labelled
`column_order=editor`, on its own `jetbrains-rd@<installation>-pid:<pid>` family
and one `remote-client:<clientId>:<connectionGeneration>` endpoint per frontend
connection. While that forward is live, the backend's remote-client membership
fold does not publish layout. Acquiring this layout authority is conditional on the authenticated
client id resolving to a remote `ClientType`. The modular frontend also opens an authenticated
lease in a monolithic IDE; that local lease remains valid for the atomic snapshot/presentation
protocol but cannot retire or suppress the classic window projection and is never forwarded on a
`jetbrains-rd` endpoint. The classification therefore gates the authority transition itself, not
only the later forward. Schema negotiation
(`supportedSnapshotSchemaVersion()`) keeps mixed versions working: a backend that
predates schema 2 never receives the bounds key, and a schema-1 frontend keeps the
backend's `unknown`-labelled fallback.

Every snapshot ingress writes `controller_editor_view_snapshot_observed` to
`.agent-doc/logs/ops.log`, including client/generation/sequence, `accepted`, status,
completeness, terminal capability, and surface count. Parse, identity, projection,
stale, and frozen refusals are therefore distinguishable from accepted projections.

The controller's `(connection_generation, sequence)` fence orders snapshots
within one backend process. The backend restarts its connection generation and
sequence with the IDE, so the controller also records the publishing process
from the connection's peer credentials (`SO_PEERCRED`; no wire field). A
regressed revision from a different process, while the process that set the
fence is gone, retires that predecessor (its detached bindings release as
`client_retired`), applies at once, and logs
`controller_editor_view_frontend_instance_reset` with both pids and revisions
(`#viewsnapshotgenreset`). A regression from the same process, from a second
live process sharing the client id, or with an unknown peer pid stays `stale`.

macOS Unix sockets report no peer pid, so that reset cannot fire there. The
backend therefore seeds its connection generation from wall-clock milliseconds
when the ingress state is created (`#viewsnapshotgenseedmac`): a restarted IDE's
first lease is above every generation the dead IDE issued, and the controller
accepts it as an ordinary newer revision on every platform with no wire field.

The controller returns an explicit, revision-fenced presentation list. The
frontend adapter resolves the exact split containing the document, mounts a
controller-session terminal or non-writable placeholder, restores originals on
close/rejoin, and returns typed applied/refused/stale receipts. These exact-262
internal effects remain unproven in a real Remote Dev backend + Client run, so
issue #218 remains open despite compile, unit, packaging, and split-sandbox gates.
