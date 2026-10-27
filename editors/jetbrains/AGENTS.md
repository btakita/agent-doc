# agent-doc JetBrains Plugin

## Build

Current version lives in `gradle.properties` as `pluginVersion = <x.y.z>`. **Never hardcode the version in docs or scripts** — read it from `gradle.properties` or glob the zip filename.

To bump the patch version and build both zips in one shot:

```bash
# from src/agent-doc
make bump-plugin
```

Or manually:

```bash
cd agent-doc/editors/jetbrains
./gradlew buildPlugin signPlugin
```

Output (where `<version>` comes from `gradle.properties`):
- `build/distributions/agent-doc-jetbrains-<version>.zip` (unsigned)
- `build/distributions/agent-doc-jetbrains-<version>-signed.zip` (signed)

Reference zips via glob: `build/distributions/agent-doc-jetbrains-*-signed.zip`.

## Install

IDEA → Settings → Plugins → gear icon → "Install Plugin from Disk..." → select the zip.

The plugin is dynamically unloadable on compatible JetBrains builds. Builds that retire plugin classloaders asynchronously cannot complete agent-doc's synchronous safety proof: every package update is staged for the next IDE start and requires a restart there. Programmatic listeners must use `ProjectPluginLifecycleService` as their parent disposable, and static project registries must be released by the lifecycle services; on compatible builds, inspect an explicit unload failure through the `com.intellij.ide.plugins.DynamicPlugins` log category.

## Logging

Uses `com.intellij.openapi.diagnostic.Logger`. No temp files.

Enable debug output: IDEA → `Help > Diagnostic Tools > Debug Log Settings` → add `#com.github.btakita.agentdoc`. Output appears in `idea.log`.

## Conventions

- Plugin is a thin wrapper — business logic lives in the `agent-doc` binary and project control plane.
- CLI subprocesses and CP-backed native calls use the resolved project root directory.
- Automatic startup/layout sync paths stay thin: they use report-only `agent-doc resync`, and automatic `agent-doc sync` receives only layout/focus file paths while the binary owns autostart, ambiguity handling, and tmux targeting.
- Submit/route waits for the markdown typing debounce before saving and dispatching, and stays silent on progress and success.
- Repeating `Run Agent Doc` should supersede any stale plugin-spawned route process and dispatch again immediately.
- Error feedback is routed to the IDE Event Log / notification tool window instead of bottom-right balloon popups.
- **Rules behind the hot-reloaded native boundary (GH #326):** a Kotlin rule change forces an IDE restart on exact-263; a native one does not. Capture IntelliJ evidence in Kotlin and derive through `agent_doc_editor_derive_json` (`EditorDerivation`); `EditorTabSyncListener.captureSurface` captures evidence only, and the surface is the `surface_projection` derivation on the delivery worker. Sync Tmux Layout reads editor state on the EDT and derives `sync_layout` (root, normalization, `--col` columns, exact-visible decision) on its background thread. Run Agent Doc's `--col` fallback does the same on its route thread, passing the route root as `chosen_root`. The `TmuxPaneFocusSync` probe asks `sync_layout` for `candidate_roots`. `remote_layout` is a compatibility observation boundary (the pre-#326 symbol only for an older library): flat backend evidence is always non-structural and only an exact connected-frontend vector may change columns. It has no Kotlin copy; never reintroduce a Kotlin remote split rule or backend reconstruction. The 262/263 measured-layout forwarder derives frontend window evidence through `client_measured_layout`, then derives its payload through `remote_client_surface`; `frontendLayoutColumns` is only the frozen schema-3 floor when native is unavailable. Never extend the frozen Kotlin fallbacks (`frozenWindowLayout`, `RemoteClientMeasuredLayout.frozenDerive`, `SurfaceProjection.frozen` and the `SurfaceReport` rules it composes, `SyncLayoutSnapshot.frozen` and the `SyncLayoutAction` root/path/column rules, `routeLayoutMode`, and the frozen candidate-root rule it composes, `RemoteClientSurfacePayload.frozen`); change the native derivation and its parity fixture instead. See `SPEC.md` "Restart-free logic upgrades".
- `plugin.xml` action IDs are stable — only change `text` attributes for renames.
- **Replica registration direction (GH #326 phase 5):** `CrdtReplicaManager.forwarderFor` captures presence/equality facts about the settled shadow, live buffer, and canonical (never the text) and asks `RegistrationProjection.decide` (`registration_projection`); it still executes the answer, rechecks at the publication edge, and orders the effects itself. Never extend `RegistrationProjection.frozen` or call `retainedRegistrationProjectionActionForAttachUtil` from a new site; change the native derivation and `registration_projection.json` instead. Native-reload decisions stay in Kotlin because they answer while the library is handed off. See `SPEC.md` "Phase 5 audit".
- **Attach-failure diagnosis (GH #326 phase 5b):** what a replica attach failure reason means (remedy, `attach_refused_<token>` status, operator-surface and liveness-republish predicates, hold notification) is the `attach_failure_diagnosis` derivation, asked through `AttachFailureDiagnosis.diagnose` by `CrdtReplicaManager.recordRegisterFailure` and Compact Exchange's pooled thread. Never call `recordRegisterFailure` on the EDT (use `recordRegisterFailureOffEdt`), never format an attach remedy inside `invokeLater`, and never extend `AttachFailureDiagnosis.frozen` or call `attachFailureRemedy`, `attachFailureStatusToken`, `retainedProjectionHoldNeedsOperatorSurfaceUtil`, `registerFailureNeedsLivenessRepublishUtil`, or `retainedProjectionHoldMessageUtil` from a new site; change the native derivation and `attach_failure_diagnosis.json` instead.
- **Registration splice shaping (GH #326 phase 5c):** the containment proof and clean merge at replica registration go through `RegistrationSpliceShaping.operatorEditsContained` / `.cleanMerge`, which send the raw shadow and canonical plus the `registration` input so native normalizes the markers and shapes the single splice. Never call `withoutBinaryOwnedMarkersUtil` or `cleanRegistrationMergeUtil` from a new site and never extend `RegistrationSpliceShaping.frozenOperatorEditsContained`; change native `captured_splice_request` and `registration_splice_shaping.json` instead.
- **Route outcome (`#jbshrinkrestarts`):** what a Run Agent Doc route's output means (outcome kind, paused-queue and deferred-switch details, notification text, snapshot path, success notices) is the native `route_outcome` derivation, asked through `RouteOutcome.derive` on the route thread. Never call `classifyRunAgentDocRouteFailure`, the `parseRunAgentDoc*` parsers, the `buildRunAgentDoc*Message` builders, `routeOwnedByOtherLaneNotice`, or `routePaneNotVisibleNotice` from a new site, and never extend `RouteOutcome.frozen`; change the native derivation and `route_outcome.json` instead.
- **No prompt poller:** do not reintroduce the defensive `PromptPoller` / `PromptPanel` path. JetBrains must not poll `agent-doc prompt --all`, auto-save tracked documents, refresh tracked files, or merge/reload editor buffers from a prompt UI timer.
