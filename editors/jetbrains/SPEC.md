# JetBrains Plugin Specification

Extends `editors/SPEC.md` with JetBrains-specific behavior.

## Plugin Metadata

- **ID:** `com.github.btakita.agent-doc`
- **Name:** Agent Doc
- **Restart:** Required for every package upgrade on builds with asynchronous classloader retirement; compatible builds may upgrade dynamically
- **Native upgrades:** Safe in-process generation handoff
- **Distribution:** The 242-261 classic and exact-262/exact-263 modular updates share this ID but use distinct versions and disjoint compatibility ranges. A tag must validate the complete three-ZIP set before publication; when Marketplace credentials are configured, it revalidates that set before uploading each update. Missing Marketplace onboarding defers only that channel and never blocks the core GitHub/PyPI release. GitHub retains one validated custom-repository listing per range, and `agent-doc plugin install jetbrains` selects the ZIP from the target IDE build without guessing. Marketplace is the only stock automatic Remote Dev synchronization channel for installing the selected modular ZIP on both peers. CLI, local-ZIP, and custom-repository installs affect only the selected peer and must say so; a non-Marketplace setup installs the identical version explicitly in JetBrains Client/Gateway and the backend. The modular `AgentDoc.UpdateChannel` action is registered only in `agent.doc.frontend`: JetBrains `UpdateSettings` is process-local, so a backend-owned toggle would strand the Client on the old generation while appearing to succeed. The classic distribution keeps its local action. See [`docs/reference/jetbrains-distribution.md`](../../docs/reference/jetbrains-distribution.md)

## Implementation Details

### Document lint inspection

**Inspect Document Directives** runs `agent-doc lint <FILE>` for the focused
Markdown document. The command is read-only and resolves the same authoritative
editor content and lint policy as closeout gates. Clean results use a transient
hint; warnings and blocking findings are retained in the Agent Doc Event Log so
the full rule, position, and hint remain available even when a compact/write
failure notification is intentionally concise.

IDE notifications render their content as HTML, so every Agent Doc notification
escapes the plain-text message (`&`, `<`, `>`, `"`) and turns line breaks into
`<br>`. CLI text such as a lint hint's `preset=<value>` placeholder must reach the
operator verbatim instead of being swallowed as an unknown tag (GH #227).

When registration carries a retained native CRDT state vector, a controller may
force a full canonical bootstrap for a durable projection without discarding the
vector. The response reports whether canonical causally covers that frontier.
JetBrains may resolve a retained-projection ambiguity by adopting canonical only
when the visible buffer still equals its settled shadow and coverage is explicitly
true. Missing or false proof keeps the operator buffer and retry hold intact.

An installed-library mtime change or typed `reload_library` intent enters one
application-wide handoff. On Linux, JetBrains marshals JNA calls onto a bounded
generation-owned worker pool. Calls for distinct document replicas may run in
parallel, while each replica remains serialized by its document worker and its
Rust per-replica lock. A call that times out before it starts is cancelled
without poisoning the generation; only a call that actually ran beyond the
lease disables it. Reload stops and joins every CRDT/listener worker, drains
calls, asks the old cdylib to quiesce replicas, terminates all owner threads so
Rust TLS destructors run, and closes the old handle. If glibc retains that
closed Rust cdylib mapping because another JVM thread once acquired Rust TLS,
the mapping is inert and remains on disk until process exit; it does not cause
the plugin to reopen stale code. The replacement loads from a distinct
per-install shadow path/inode and becomes the only published generation.
Controller launch from the cdylib uses a short-lived external helper, so no
child-reaper thread retains active old-generation work. Failure at any quiesce,
drain, owner-thread, close, replacement ABI, or replacement-load boundary
publishes no second generation and requires restart; a replacement-load
failure may restore the old named shadow. Durable reliable-sync
outboxes live in the project controller; the reloadable cdylib sends typed
controller RPCs and retains no SQLite connection.

A reload intent whose target library is the loaded build (same mtime), or a
build that already failed validation, is satisfied before any handoff work: no
listener or replica is quiesced (`#steerreplicachurn`). Quiescing tears down
every open document's CRDT replica, and a repeated `reload_library` fan-out for
one install used to deregister and re-register all of them every 10-60 s while
the native generation never changed. Replicas are rebuilt after a reload attempt
only when its quiesce actually disposed the replica managers; a checkpoint that
missed its deadline returns before disposing anything, so the live replicas are
left attached rather than force-refreshed.

Each checkpoint is queued behind the document's already-accepted work and resolves
the current attached forwarder when that serialized task runs. If projection
recovery replaced the snapshotted forwarder with a successfully reattached one in
front of the checkpoint, the replacement supplies the resume state (GH #281); stale
object identity alone must not make a fail-closed reload permanent or require a
later, unrelated IPC write to retry it.

Before native reload reattaches any CRDT replica, JetBrains republishes each
open session document's reliable-liveness registration with the current plugin
classloader endpoint identity. Existing local presence tags are retained; if
the local liveness graph was rebuilt, presence is reopened before registration.
The controller may therefore keep its strict superseded-endpoint admission
fence: a delayed old classloader cannot reclaim replica membership, while the
current classloader is admitted without depending on a duplicate-open edge.

The reattach awaits each document's registration for the ordinary 750 ms
attach wait; a registration that outlives it keeps running on the document
worker, or its register retry fires, and usually attaches while later
documents are still awaited. The `[native] replica restart` line therefore
recounts every document's replica when it is reported (GH #276: a reload whose
30 replicas had all attached used to log `attached=26/30 failed=<4 paths>`).
After the awaited pass, the coordinator waits up to 3 s, while it still holds
the reload gate, for registrations still converging, and stops as soon as every
replica is attached or nothing is left converging. An unattached document whose
attach is still in flight or whose register retry is armed is reported
`pending` (`replica restart pending ... pending=<paths>`, INFO); only one that
nothing will retry is `failed`, which makes the restart `incomplete` (WARN).

The package omits `require-restart`, so JetBrains may unload the plugin and
replace its classloader during an update. A plugin-owned project service is the
parent disposable for every programmatic startup listener, including VFS
subscriptions, and releases the project's static manager registries. No
listener may use the longer-lived project itself as its disposable parent. An
application service additionally cleans every still-open project during plugin
unload.
Application-global workers and JVM hooks are generation resources, not project
resources. Each such owner registers an idempotent closer when it is first
created. After the generation is fenced and every project is disposed, unload
closes those resources in reverse creation order. In particular it cancels the
current-document reporter, controller-socket watchdog, and route-attempt ledger;
quiesces, drains, and terminates the four native-call workers off the EDT; closes
the native handle only when quiesce and drain are proven; and removes the JVM
shutdown hook that would otherwise root the outgoing classloader. Once this
registry is closed, late work may neither create another worker nor reload the
native library. Repeated unload cleanup is a no-op.
Generation-owned projections that must be looked up from extension callbacks
use plugin-static per-project registries, not IntelliJ light-service lookup:
project containers can retain a light-service adapter by implementation class
name while a replacement classloader is activating, which would return an old
generation instance to new bytecode. Unload cleanup removes and disposes every
registry entry before the replacement initializes open projects.
On compatible builds, an unload leak is a defect and JetBrains' explicit
unload-failure prompt is the package-update restart fallback. Builds that retire
plugin classloaders asynchronously cannot satisfy agent-doc's synchronous
retirement proof: agent-doc deliberately declines dynamic replacement, stages
every package upgrade, and requires an IDE restart to load it. An IDE that
already loaded a package generation declaring `require-restart="true"` also
requires a restart before the dynamic lifecycle can govern later upgrades on a
compatible build.

Package selection is per target IDE. The classic package
(`agent-doc-jetbrains-<version>.zip`, `editors/jetbrains`) covers builds
242-261; the modular source (`editors/jetbrains-262`) produces both
`agent-doc-jetbrains-262-<version>.zip` for exactly 262 and
`agent-doc-jetbrains-263-<version>.zip` for exactly 263, with the latter's
metadata in `editors/jetbrains-263`. The presentation adapter remains guarded
to exact 262, while frontend frame snapshots are compiled and verified against
both platform branches.
All three update versions, artifact names, repository listings, and ranges are
distinct. A new agent-doc release retains a package's update version when its
recorded `pluginSourceDigest` still matches its source tree, while updating the
package's `pluginReleaseVersion` ownership metadata; this keeps the binary paired
with the already-running generation and makes installed-plugin reconciliation a
no-op without a restart (GH #295). A changed digest allocates a fresh version
above every compatibility line. The release generation fence refuses a Kotlin
delta made only of comments/formatting or consistent identifier renames and
guards the `client_measured_layout` native-before-floor call order, so a
restart-only exact-263 package is not regenerated for non-behavioral churn or
for a grouping rule that belongs in the hot-reloaded library (GH #346).
`agent-doc plugin install|update
jetbrains`, the `agent-doc upgrade` installed-plugin reconciliation, and `plugin install jetbrains --local
--all-installed` derive each target's platform build from its versioned IDE data
directory (`IntelliJIdea2026.3` -> 263) and select only that range's asset or
local ZIP. A target whose build cannot be proven, or is outside all ranges, is
refused with guidance. During `agent-doc upgrade` reconciliation (GH #233) a
target whose proven build is outside all ranges is instead reported as
`UnsupportedPlatform`: nothing is installed, its plugin stays in place, the
summary prints one `WARNING:` line naming the build, the supported ranges and
the installed version (no retry, `plugin install`, or "upgrade agent-doc"
advice, since none can succeed on the newest release), `ops.log` records
`unsupported_platform=<n>`, and the upgrade exits 0; it does not count as a
change for the `--auto` watcher. The `stale_plugin` warning likewise drops its
"install {expected} first" remedy when the live IDE's mapped plugin tree sits
in such a data directory and names the build, the ranges and the plugin left
installed instead; `--local --all-installed` resolves every target before
replacing any, so one unprovable target or a missing ranged build changes
nothing. All archive roots install into the canonical `agent-doc-jetbrains/`
tree, and stale ranged roots are removed so one plugin ID
never has two on-disk roots. `make install-editor-plugins` builds all packages
before the all-installed convergence.

Restart-free dynamic upgrade is a classic-line capability
(`#jb262dynupgrade`). Only the classic 242-261 plugin jar carries the
`JetBrainsPluginUpgradeBootstrap` `Main-Class`/`Agent-Class` launcher, and that
launcher replaces one classic `agent-doc-jetbrains/` tree. The exact-262 and
exact-263 packages are Plugin Model v2 split-mode distributions (descriptor-only
`agent.doc-<version>.jar` plus unversioned `lib/modules/agent.doc.*.jar`, with a
frontend half that may run in a separate JetBrains Client) and expose no
launcher. The upgrader classes do ship in `agent.doc.backend.jar`; their absence
from the root manifest is not the final blocker and adding a `Main-Class` would
not restore a safe upgrade (GH #290).

JetBrains `DynamicPlugins` computes and validates a replacement state from the
main plugin descriptor and its resolved module graph. A split-mode probe on IU
263.7031.23 on 2026-10-19 supplied the missing platform evidence (GH #299): a
filtered launcher containing only the attach bridge reached the backend without
adding plugin implementation classes to the system classloader. `DynamicPlugins`
computed a state excluding Agent Doc and logged that the plugin would be removed
from the context, then rejected the state because Agent Doc was expected not to
be loaded but remained included in the module graph. `unloadPlugin` returned
false. This happens before a replacement generation can load, so that measured
build has no safe restart-free whole-package swap through the supported platform
API. Package-line classification does not repeat the attach probe during an
install and must never describe this dated result as a live measurement of the
current upgrade. A different exact-263 platform build must be qualified with the
filtered probe before the module-graph result is attributed to it.

The modular package is also installed on both sides of split mode. Attaching
only to the backend cannot atomically update `agent.doc.frontend` in the separate
JetBrains Client JVM, and local-archive plugin synchronization supplies no
cross-JVM handoff receipt. A backend-only swap while the client keeps the old
frontend is therefore not a valid fallback after the measured module-graph
refusal, and remains the package-line blocker while a later 263 build is
unqualified.

When a live IDE owns either modular target, the installer recognizes the package
line from its `agent.doc-<version>.jar`, does not attempt an attach, replaces the
plugin files on disk, and reports restart-required. Exact-262 names its
classloader/frontend safety boundaries; exact-263 cites the dated 263.7031.23
module-graph refusal as prior evidence, says that the current install did not
repeat it, and names the independent frontend boundary. The absent launcher is
deliberate, and invoking the already-shipped backend upgrader would not bypass
those boundaries (`ops.log`: `declined_by=modular_package`). It never reports
either package as one that "predates restart-free dynamic upgrade support"; that
wording is reserved for a classic jar without a `Main-Class`. The live-jar
probes (`#pluginbyteidentity` superseded-bytes detection, the controller's
`editor_route` admission, the install's `#jbdynamicfalsereport` load proof and
the `#pluginactivationprobe` loaded-generation reading) read versions only from
the versioned plugin jars, `agent-doc-jetbrains-<version>.jar` and
`agent.doc-<version>.jar`.

A live exact-263 IDE maps only the unversioned content-module jars
`lib/modules/agent.doc.{backend,frontend,shared}.jar`; it reads the
descriptor-only `agent.doc-<version>.jar` without mapping it. So superseded-bytes
detection also classifies those module jars (`#jb263probeblind`). After the
atomic tree swap the mapping reads
`<plugins>/.agent-doc-jetbrains-install/backup-*/lib/modules/agent.doc.<module>.jar (deleted)`.
A `(deleted)` module mapping, or one that still resolves inside a `backup-*`
tree (`Displaced`), is superseded. Neither modular line has a restart-free
upgrade, so no newer module generation can be live in the same process. A
superseded module verdict therefore outranks a current root-jar mapping:
something reading the installed descriptor does not show which code runs. A
current module mapping carries no version, so it never becomes a `Current`
verdict. An IDE that maps only current module jars stays `Unknown` and fails
open: no warning, no advisory, no refusal. The version-reading probes behave as
before. The restart verdict is found through the `lib/modules` path, and the
data root is resolved from it in the same way. Classic and exact-262 root-jar
mappings classify as before. The one change for them is that a still-linked
root jar inside a `backup-*` tree is `Displaced` rather than `Current`.

Because nothing in that fallback reaches the running IDE, the plugin itself
prompts for the restart. Each plugin generation polls, every 30 seconds, the
versioned plugin jar in its own installed tree (`<plugin dir>/lib`) and the
restart-required marker beside that tree
(`.agent-doc-jetbrains-restart-required`). When the installed jar names any
version other than the one this JVM loaded, or the marker's `staged_version=`
names a newer one, and the same pending version is seen on two consecutive
polls, it raises one sticky `Agent Doc Plugin Update` balloon per pending
version. The balloon names the loaded and pending versions and the marker's
first-line reason, and offers **Restart IDE** (`Application.restart()`) and
**Later**. A marker alone never prompts: after the restart the loaded version
matches the tree and a leftover marker is ignored. The poller is a
`PluginGeneration` resource, so a classic restart-free upgrade cancels it with
the old generation, and the two-poll rule keeps that swap's brief
files-before-unload window from prompting. The prompt does not make the modular
upgrade restart-free; it only replaces the operator having to notice the CLI
warning or a later `plugin_bytes_superseded_advisory`.

#### Why a plugin-owned child loader is not the remaining hot-upgrade boundary (GH #333/#339)

The remaining Kotlin orchestration cannot be made replaceable merely by moving
its current classes into a versioned JAR behind a plugin-owned child classloader.
IntelliJ registers listeners, services, extension implementations, actions, and
their `Disposable` parents with objects whose implementation classes belong to a
particular loader. `CrdtReplicaManager`, patch-watch, and route orchestration also
own callbacks, worker state, static registries, and values whose class identity
crosses those registrations. Loading a second copy would either leave the old
loader rooted by platform callbacks or allow old and new class identities to
meet in one document lifecycle. Closing the child loader does not unregister
those references, and the exact-263 backend still cannot atomically replace the
frontend half in the JetBrains Client JVM.

A future child-loader design would therefore require a different architecture,
not a packaging change: the stable plugin generation must own every IntelliJ
registration and disposable; the reloadable implementation may exchange only
bootstrap/JDK types or version-neutral serialized data across the loader
boundary; handoff must quiesce and acknowledge every callback and worker, carry
state without implementation-class identity, verify the versioned JAR, and
coordinate the backend and frontend JVMs. Until all of those conditions have a
tested protocol, child-loading the orchestration is not a safe restart-free path.
Pure policy continues to move behind `agent_doc_editor_derive_json`; changes to
IntelliJ-facing orchestration remain restart-bound.

**Exact-263 decision (GH #339).** The current modular distribution has no safe
whole-package restart-free upgrade path. This is the steady state, not an
installer bug hidden by the absent root-jar launcher:

- Replacing `agent.doc.shared`'s `loading="required"` with optional loading is
  not a sufficient fix. It may change module selection, but it does not create
  a handoff protocol and receipt between the Remote Dev backend and the
  JetBrains Client JVM. Shipping that descriptor change from a backend-only
  unload probe could leave the two processes running different Agent Doc
  generations.
- A plugin-owned child loader is safe only after the stable shell owns every
  IntelliJ registration and `Disposable`, all generation traffic crosses a
  JDK/serialized boundary, and both processes quiesce and acknowledge the same
  verified artifact before activation. The current plugin does not have that
  shell or protocol; loading its existing implementation classes a second time
  is therefore forbidden.
- Moving a policy rule behind `agent_doc_editor_derive_json` remains the
  supported restart-free route for that rule. It does not make changes to
  IntelliJ evidence capture, registrations, or lifecycle orchestration
  restart-free.

Accordingly, a release whose recorded JetBrains source digest changes allocates
a new plugin version and requires an IDE restart on exact-263. A release whose
digest does not change retains the installed plugin version, while native-only
logic may hot-reload. This boundary may change only with tests proving both
(1) that across an implementation swap the modular module graph, the plugin
descriptor, and every platform registration (actions, extensions, listeners,
services, RPC providers) are unchanged, and the retired implementation
generation's classloader becomes unreachable and is collected; and (2)
generation-fenced backend/frontend activation receipts for the same verified
artifact. A backend-only success is not acceptance evidence. (Point (1) was
restated with the operator's agreement on 2026-10-26; it previously required
"removal and replacement of the full modular graph", which the shell design
below never performs.)

The design that could eventually lift this boundary without unloading the
plugin is in the next section. It is a design and a test-only spike, and
nothing in it changes the decision above until its migration step R4 lands.

### Restart-free implementation generations: shell plus child loader (GH #339 design)

**Status.** R0 (this design plus the test-only proof of concept in
`src/test/kotlin/com/github/btakita/agentdoc/shellspike/`) and R1 have landed,
except `verifyImplJar` (no implementation jar exists before R2) and the
measured remainder that "R1 status" lists. R1 moves the actions, the project
lifecycle, the Tier A programmatic listeners and the editor and language
extensions behind shell forwarders; routes EDT and pooled work through
per-generation shell owners; creates the user-data keys in the shell; delegates
the Fleet RPC ingress into the generation; classifies the services, settings
and JNA binding as shell; splits the shell and implementation digests; and
publishes the activation receipt on both JVMs and in the backend's controller
registration. "R1 status" below lists each registration class. The
implementation still loads in the plugin classloader, so behaviour is unchanged
for users. Until migration step R4 below lands with the receipts it names, the
"Exact-263 decision" above stays in force, and every JetBrains source-digest
change stays restart-bound. The restart-required prompt remains the fallback
for every case this design cannot cover.

**Idea.** Stop asking IntelliJ to unload anything. The installed plugin keeps
its plugin ID, its one package, and its module graph. It becomes a stable
*shell* that IntelliJ loads once per IDE start, and there is no separately
installed shell plugin. The shell loads the real implementation from a
versioned jar into an agent-doc-owned child classloader. An upgrade then swaps
that implementation *generation* in place. `DynamicPlugins.unloadPlugin` is
never called, so the measured 263.7031.23 module-graph refusal does not apply:
the descriptor, the content modules, and the IntelliJ `PluginClassLoader` never
change while the IDE runs. This is the same move the native library already
makes. `libagent_doc` hot-reloads under a plugin generation that does not
change (`[native] hot-reloaded libagent_doc`). This design adds a second
reloadable layer, in Kotlin, between the two.

**Expected yield (estimate).** Path classification is used as a proxy. Between
2026-09-26 and 2026-10-26, 82 of 139 tagged releases changed plugin main
sources, and so restarted every 263 IDE. 50 of those 82 touched only files
that this design classifies as implementation (classic `src/main/kotlin`
except `NativeLib.kt` and `PluginLifecycleListener.kt`, and modular
`backend/src/main`). The other 32 also touched shell-bound files: descriptors,
resources, `NativeLib.kt`, the lifecycle listener, or the frontend, shared, or
root modular modules. By commit, the split is 138 implementation-only and 49
shell-touching, out of 187. The true yield depends on how small the shell
stays, and R1 must measure it again.

#### Packaging and verification

- **Shell:** the root plugin jar (classic) or the `agent.doc.*` module jars
  (262/263), as today. They hold only descriptors, forwarders, the generation
  host, the shell SPI, the JNA binding (`NativeLib`, `NativeReloadCoordinator`),
  `PersistentStateComponent` settings classes, the Fleet RPC contract in
  `agent.doc.shared`, and the frontend module.
- **Implementation:** `impl/agent-doc-impl-<implVersion>.jar` at the plugin
  root. It must never sit under `lib/` or `lib/modules/`, because IntelliJ adds
  those to the plugin classpath at startup. That would define the same classes
  a second time in the `PluginClassLoader` and split class identity. The
  manifest carries `Agent-Doc-Impl-Class`, `Agent-Doc-Impl-Version` and
  `Agent-Doc-Shell-Spi-Version`. The installer writes the jar first, then
  atomically renames an `impl/current` pointer into place, holding
  `version=`, `sha256=` and `epoch=`.
- **Digests:** `pluginShellDigest` covers the shell sources, the descriptors,
  and the artifact-shaping build files. `pluginImplDigest` covers the
  implementation. The plugin version is still allocated from their union, so
  Marketplace and the custom repositories see a normal update. The installer
  chooses between swap and restart from the shell digest alone.
- **Verification:** the shell reads the jar once into memory, checks it
  against the pointer's `sha256`, and defines classes from those same bytes.
  Nothing reopens the path, there is no `JarFile`/`URLClassLoader` cache, and
  no Windows file lock, for the same reason native reload loads a unique
  per-mtime copy of the library. The shell refuses a jar that defines any class
  outside the implementation package prefix, and treats a jar built for
  another `Agent-Doc-Shell-Spi-Version` as restart-required. The plugin
  verifier does not see a jar outside `lib/`, so R1 must add a
  `verifyImplJar` Gradle task. That task runs the same
  `INTERNAL_API_USAGES`/compatibility checks against a synthetic plugin that
  wraps the implementation jar, plus an ASM scan for the forbidden calls listed
  below.

#### Classloader

`ImplClassLoader` takes the shell's `PluginClassLoader` as its parent
(`PluginManagerCore.getPlugin(PluginId)?.pluginClassLoader`). Delegation is
parent-first for the JDK, the Kotlin stdlib, the IntelliJ platform, the bundled
plugin dependencies (terminal, Markdown) and the shell SPI. That way every
generation sees one `ImplGeneration` type. Delegation is **child-only** for the
implementation prefix (`com.github.btakita.agentdoc.impl.`). An implementation
class is defined from this generation's jar or not at all, and it is never
resolved from the parent. Private libraries the implementation embeds, such
as lazily-kt, go under a relocated child-only prefix, so two generations can
carry different versions. Implementation code can read platform state and call
IntelliJ APIs through the parent. What it may register is governed below.

Classes this loader defines are not `PluginAwareClassLoader` classes, so
IntelliJ cannot attribute an implementation exception or freeze to Agent Doc by
itself. The shell catches every failure at the forwarder boundary. It converts
the failure to a shell-owned throwable that holds only strings and a fresh copy
of the stack frames, and wraps that as `com.intellij.diagnostic.PluginException`
with the Agent Doc `PluginId` before it reaches `Logger.error`. This matters
for the leak rule too: the IDE error pool (`MessagePool`) keeps reported
throwables indefinitely, and an implementation-class throwable there would pin
its loader.

#### Who owns IntelliJ registrations and callbacks

Rule: **the platform holds only shell objects.** An implementation generation
stages handlers through the `GenerationContext` the shell lends it. A shell
forwarder, registered once per IDE run, delivers to the handler of the
generation that is currently active, through a generation slot. Retirement
clears that slot, so after a swap no platform-held reference reaches the old
loader.

| Registration (today's count, by text search of `src/main/kotlin`) | IntelliJ API / extension point | Shell owner | Across a swap |
|---|---|---|---|
| Actions (27 `<action>` ids, shortcuts, groups) | `plugin.xml` `<action class=…>`, `AnAction.update`/`actionPerformed`, `getActionUpdateThread` | one forwarding `AnAction` per id, declared by the shell | unchanged; it dispatches by id to the active generation. A new id or default shortcut is a shell change. `ActionManager.registerAction(id, action, pluginId)` could add ids at runtime, but a default keymap shortcut cannot be declared that way. |
| Services and settings (`applicationService` ×3, `projectService`, two `applicationConfigurable`) | `@Service`/XML services, `PersistentStateComponent`, `Configurable` | shell classes | the service owns the host. `XmlSerializer` binds settings state to its class, so settings stay in the shell. |
| Project lifecycle (`<projectListeners>` `ProjectManagerListener`) | message-bus topic in XML | shell `PluginLifecycleListener` | forwards `projectOpened`/`projectClosing` |
| Programmatic listeners (`FileEditorManagerListener.FILE_EDITOR_MANAGER`, `ApplicationActivationListener.TOPIC`, `EditorFactory.getInstance().eventMulticaster.addDocumentListener`, VFS events) | `MessageBus.connect(Disposable).subscribe(topic, …)`, `addDocumentListener(l, Disposable)` | **Tier A:** one shell forwarder per topic, connected under `ProjectPluginLifecycleService` | unchanged; it delivers to the slot. **Tier B (allowed):** registration under `GenerationContext.disposable`, which `Disposer.dispose` releases at retirement. |
| Editor/language extensions (`completion.contributor`, `lang.foldingBuilder`, `codeInsight.lineMarkerProvider`, `lang.structureViewExtension`, `statusBarWidgetFactory`, `editorNotificationProvider`, two `notificationGroup`) | XML extension points | shell forwarder per extension | the shell refreshes what the platform cached from the old generation: `EditorNotifications.updateAllNotifications()`, `StatusBarWidgetsManager.updateWidget(factory)`, `DaemonCodeAnalyzer.restart()`. The implementation returns data such as text, icon keys and action ids. The shell builds the Swing components, because platform-held components of implementation classes would pin the loader. |
| Modular extensions (`postStartupActivity`, `fileEditorProvider`, `platform.rpc.backend.remoteApiProvider`) and the Fleet `@Rpc` contract | XML; kotlinx-serialization class identity | shell (`agent.doc.shared`, frontend, backend shell) | new implementation features travel as version-neutral strings inside an RPC field, the way native derivations use `agent_doc_editor_derive_json`. |
| EDT work (~45 `invokeLater` call sites) | `Application.invokeLater(Runnable, ModalityState, Condition expired)` | `GenerationContext.invokeLater` | the queued runnable is a shell `FencedTask`. Its `expired` condition is "the generation retired", and retirement clears its implementation reference, because the event queue may still hold it. |
| Pooled work and threads (~46 `executeOnPooledThread`, 11 `AppExecutorUtil`, 15 `Thread(`, 10 `Executors.new*`, one `Alarm`) | `AppExecutorUtil.createBoundedApplicationPoolExecutor`, service `CoroutineScope` | executor and `CoroutineScope` for each generation, children of the shell service's injected scope | quiesce stops intake, cancels, and joins with a deadline. A generation with code still running on a thread is never retired. |
| Native library (JNA) | `Native.load`, native callbacks | shell (`NativeLib`, `NativeReloadCoordinator`) | JNA caches interface proxies, and native code would retain implementation callbacks, so the binding stays in the shell. New native features go behind `*_json` entry points, so `NativeLib` stays frozen. |
| Controller identity | editor registration (`editor_id`, `plugin_version`) | shell | the shell keeps one `editor_id` and adds `impl_sha256` and `epoch`. A quiesced or retired generation cannot register, because its context refuses, which closes the `#pluginunloadresurrect` class of stale endpoint. |
| Per-platform-object state (4 `putUserData`, 3 `Key.create`) | `UserDataHolder`, `Key` | keys created by the shell | the implementation stores only shell or JDK values. The shell sweeps generation-owned keys at retirement. |

**Forbidden in implementation code** (`verifyImplJar` scans for these): any
`MessageBus.connect()` or listener registration without a generation
disposable, `Disposer.register` under a platform parent, raw `Thread`
creation, `ThreadLocal`, `Runtime.addShutdownHook`, `Native.load` or a JNA
`Callback`, `Key.create`, static registries keyed by platform objects, and
building `JComponent`s that the platform retains.

#### Swap protocol inside one JVM

1. **Notice.** The shell sees a new `impl/current` through the existing 30 s
   plugin-tree poll of `PluginRestartRequiredNotifier`, or through a controller
   `reload_impl` intent shaped like today's `reload_library`.
2. **Verify** the bytes, the implementation prefix, and the SPI version (see
   above). A failure is either `Refused` (the active generation never stopped)
   or `RestartRequired`.
3. **Load and instantiate** in a fresh `ImplClassLoader`, with no platform
   contact.
4. **Quiesce** the active generation. Forwarders stop delivering: editor and
   document events are buffered and coalesced into a resync at activation,
   and actions present as disabled ("Agent Doc is updating"). Executors and
   coroutines are drained with a deadline. `quiesce()` exports version-neutral
   state (JSON, with its own schema version).
5. **Prepare** the new generation with that state. A state schema it does not
   know imports as `null`, a cold start, and replicas re-register the way a
   native reload's replica restart does. The controller stays the authority,
   so the handoff never has to carry document text.
6. **Activate.** Flip the slot, replay buffered events, refresh the platform
   caches above, and publish the activation receipt.
7. **Retire** the old generation. Clear its slot, `Disposer.dispose` its
   generation disposable, cancel its queued `FencedTask`s, and drop the
   loader. A `WeakReference` with a `ReferenceQueue` watches that loader under
   natural GC. If it has not cleared by the next swap, the host logs
   `impl_generation_leak` and disables hot swap until restart, so leaked
   generations never pile up.
8. **Roll back** when steps 3 to 5 fail. The old generation resumes (`resume()`),
   the attempt is logged, and a pending generation that keeps failing falls
   back to the restart prompt.

#### Remote Dev: proving the JetBrains Client runs the same generation

Each JVM runs its own shell and generation host. After every activation, each
JVM emits one version-neutral receipt line:

```text
agent_doc_activation_receipt/v1 role=<backend|frontend> jvm=<instance-uuid> shell=<pluginShellDigest> spi=<n> impl_version=<v|none> impl_sha256=<hex|none> generation=<local-counter> epoch=<backend-assigned>
```

The backend reports its receipt in the controller registration. The frontend
reports its receipt to the backend in a field of the existing Fleet RPC
surface snapshot, added once in the R1 shell release. The backend assigns
`epoch` for each swap, and a JVM that activates the swap echoes it. `generation`
is local to one JVM and is never compared across JVMs. The **activation fence**
admits work that depends on the frontend, meaning measured layout and
pane-tagged surface authority, only when two things hold. First, the
backend's receipt matches the expected shell digest, implementation sha, and
epoch. Second, if a remote client is connected, a receipt from a different
JVM reports the same shell digest and satisfies the frontend policy. Otherwise
the backend keeps the existing
`frontend_authority_absent_or_generation_mismatched` readiness, so it holds
rather than acting on a half-upgraded pair.

- **Recommended policy, `SHELL_ONLY`:** the frontend module is shell code and
  hosts no implementation generation. The frontend's receipt reports
  `impl_sha256=none`. Any frontend change is a shell change and restarts both
  sides, as today. That cost is small: 11 of the 187 plugin commits in the
  window above touched the frontend. In return, a Remote Dev backend never
  executes new code inside the operator's local client JVM.
- **Operator decision, `SAME_IMPL` (not adopted):** the frontend also loads
  the implementation jar at the same epoch. The JetBrains Client usually runs
  on another machine, and the agent-doc installer runs on the backend host, so
  the jar would have to be pushed over the RPC and verified on the client. That
  would let a Remote Dev backend run new code in the client without a plugin
  install, which is a security-boundary change. Only the operator can make
  that choice.
- The fence proves that both JVMs reported the same verified artifacts at the
  same epoch. It does not make the swap atomic across JVMs. Between the backend
  activating and the client's receipt arriving, frontend-dependent work is held,
  not wrong. Under `SHELL_ONLY`, that window is only the latency of the receipt.

This design never replaces the module graph, so point (1) of the "Exact-263
decision" reopen rule was restated (operator-agreed 2026-10-26) to what the
swap protocol asserts: graph, descriptor and registrations unchanged across the
swap, and the retired generation collected. See "Restatement of the Exact-263
reopen rule" below.

#### When a restart is still required

- A shell-digest change. That covers any descriptor change (action ids,
  default shortcuts, extensions, listener topics, services, notification
  groups, configurables, module dependencies), the forwarders, the host or the
  SPI, the Fleet RPC types in `agent.doc.shared`, the frontend module (under
  `SHELL_ONLY`), the settings state classes, a JNA signature in `NativeLib`,
  and the classic attach launcher.
- An implementation jar built for another `Agent-Doc-Shell-Spi-Version`.
- A detected generation leak (`impl_generation_leak`), until the next IDE
  start.
- A pending generation that cannot quiesce within its deadline or keeps
  failing to prepare.
- R1 itself, because installing the shell is one more restart.
- Updates driven by IntelliJ itself: Marketplace, custom repositories, Remote
  Dev plugin sync. IntelliJ's updater compares plugin versions and goes through
  `DynamicPlugins`, so on exact-263 it still asks for a restart. Only
  agent-doc's own installer (`agent-doc plugin install` / `upgrade`) takes the
  swap path. That could be avoided only by pinning the Marketplace version and
  shipping implementation jars out of band, which is a distribution change
  this design does not propose.

The restart-required prompt remains the fallback: the
`.agent-doc-jetbrains-restart-required` marker, the `declined_by=` diagnostics
and the sticky `Agent Doc Plugin Update` balloon are unchanged. The notifier
gains one rule: a pending plugin version does not prompt when its shell digest
equals the loaded shell's and the host has already activated its
implementation sha. The installer gains `plugin_dynamic_upgrade
outcome=impl_swapped impl_version=<v> epoch=<n>` and, when the shell changed,
`declined_by=shell_changed`. Both names are proposed, not shipped.

#### Where it cannot work, and what it costs

- **Cost.** This is a rewrite of the plugin's ownership model, not a packaging
  change. About 1.6 MB of Kotlin across 89 files is affected. Every callback
  site counted in the table must go through a generation context, and every
  application-global `object` registry (replica, patch-watch and route
  orchestration) must become generation-scoped. The state handoff for each
  subsystem needs its own schema and test.
- **Platform caches with no unregister path** must stay in the shell: XML
  extension classes, `PersistentStateComponent` state, Fleet RPC serializers,
  `Key` instances, `ClientId`-scoped services, and Swing components held by the
  IDE.
- **Detecting a leak in production is late.** It relies on natural GC, and an
  explicit `System.gc()` is not acceptable in an IDE. A leak costs one
  generation's metaspace until restart, and it disables further swaps.
- **Weaker error and freeze attribution**, because these are not
  `PluginAwareClassLoader` classes. The `PluginException` wrapping helps only
  where the shell catches.
- **Marketplace policy has to be checked before R3.** The implementation jar
  ships inside the reviewed plugin ZIP. The installer must only ever activate
  bytes identical to a published ZIP's `impl/` entry, never code downloaded
  separately.
- **Every swap costs a brief pause and a re-attach** (quiesce plus replica
  re-registration), comparable to a native reload.

#### Migration plan, release by release

- **R0 (this change):** this section and the test-only spike. No shipped
  change and no version bump.
- **R1, shell in place in the same loader (restart-bound; landed, see
  "R1 status"):** add the SPI,
  `GenerationHost`, forwarders for every XML registration, and the generation
  context. Move the implementation behind the SPI, still compiled into the same
  jar and loaded through the shell by the plugin loader. Add the receipt line
  to the controller registration and to the RPC (snapshot schema 5). Split
  `pluginShellDigest` from `pluginImplDigest`, and add `verifyImplJar`. The
  acceptance test is an audit showing zero direct platform registrations from
  implementation packages.
- **R2, child loader at startup (restart-bound):** package the implementation
  into `impl/` and load it with `ImplClassLoader` at IDE start only, with the
  leak watcher on. This proves class-identity separation and attribution in the
  field before any live swap.
- **R3, live swap on the classic 242 to 261 line:** where the attach upgrader
  still backs it up, the installer swaps when the shell digest is unchanged.
  Soak on `impl_generation_leak` and on how long the quiesce takes.
- **R4, live swap on the exact-262/263 backend with a `SHELL_ONLY` frontend:**
  activation admitted only behind the receipt fence, plus the operator-agreed
  restatement of the "Exact-263 decision" reopen rule (adopted 2026-10-26).
- **R5 and later:** move CrdtReplicaManager, PatchWatcher, EditorTabSyncListener
  and route orchestration into the implementation one subsystem per release.
  Each move brings its own handoff schema and a collectability test. Pure policy
  keeps moving behind `agent_doc_editor_derive_json`, which stays the cheaper
  lever.

#### R1 status

R1 is restart-bound like every shell change: installing each R1 release
costs one IDE restart. It keeps the frontend shell-only (`SHELL_ONLY`) and
keeps the restart-required prompt, marker, and diagnostics unchanged.

| Registration class | R1 status | What exists, and what remains |
|---|---|---|
| Actions (27 ids) | **done** | Every `plugin.xml` `<action>` names a `com.github.btakita.agentdoc.shell.<Id>Forwarder` (`ShellActionForwarders.kt`). The forwarder owns the id, text, shortcuts, groups, `DumbAware`, and the update thread (`BGT`, which every action already used). It hands the platform's own event to the `ImplAction` that `AgentDocImplGeneration.ACTION_HANDLERS` maps to the id. The 27 former `AnAction` classes now implement `ImplAction`, so no implementation class is an `AnAction`. `ActionWrapperUtil` is not used, because its update copies the delegate's empty template presentation. The 262/263 backend descriptor still copies 26 of the ids, and `AgentDoc.UpdateChannel` stays frontend-owned. |
| Services and settings | **done (shell classification)** | The three `applicationService`s, the `projectService`, both `applicationConfigurable`s and the settings state classes are shell files in `shell-sources.txt`. The shell's own `ShellGenerationHost` is a light application service. The audit refuses an `@Service` in implementation files. `PluginLifecycleListener.kt`, which declares two of those services, names no implementation type (enforced). |
| Project lifecycle (`<projectListeners>`) | **done** | `PluginLifecycleListener` forwards `projectOpened` and `projectClosed` to `ImplGeneration.projectOpened`/`disposeProject`; `ProjectPluginLifecycleService.dispose` forwards to `disposeProject`. The attach bridge's static hooks `initializeOpenProjectsAfterDynamicLoad` and `disposeOpenProjectsForDynamicUnload` stay on the listener class, which the bridge resolves by name, and forward to the generation. The startup body, the dynamic-load and dynamic-unload bodies, and project teardown moved unchanged into the implementation's `AgentDocProjectLifecycle`. `PluginUnloadCleanupService` keeps the unload order: it retires `PluginGeneration`, retires the implementation generation through `ShellGenerationHost.retireActiveGeneration()` (`ImplGeneration.retire`: dispose every open project, then remove the turn-state strips), then closes the classloader-global resources. The host's own disposal retires the generation too, so whichever service IntelliJ disposes first runs it, once. `generateModularLifecycle` now copies `AgentDocProjectLifecycle.kt` into `AgentDocProjectLifecycle262Generated.kt` (the 262/263 backend excludes the classic file and compiles the shell listener as is). |
| Programmatic listeners (Tier A) | **done** | `ShellListenerHub` holds one forwarder per topic or `add…Listener` key: a JDK proxy defined by the shell's classloader, subscribed once under the hub owner (`ShellGenerationHost` for the application bus, the editor event multicaster and the editor factory; `ProjectPluginLifecycleService` for each project bus). It delivers every interface method, default methods included, to the staged handlers in staging order; every handler sees the event even when one throws, and the first failure is rethrown with the rest suppressed. Implementation code stages all 17 former direct registrations (8 project-bus and 3 application-bus subscriptions, 3 document listeners, 2 editor-factory listeners and 1 multicaster mouse listener) through `GenerationListeners`, which calls `GenerationContext.subscribe`/`addDocumentListener`/`addEditorMouseListener`/`addEditorFactoryListener`. Each handler keeps its former parent disposable, and the removal is registered through `Disposer.register(parent, …)`, so a disposed parent refuses it exactly as `MessageBus.connect(parent)` did. A retired generation stages nothing. The audit enforces `message_bus_subscribe`, `message_bus_connect`, `document_or_vfs_listener` and `editor_factory_or_multicaster_listener` at zero in implementation files and pins the 17 staged registrations. AWT and per-editor focus listeners (`awt_or_component_listener=3`) are not message-bus topics and stay measured. |
| Editor/language extensions | **done** | All six XML extension classes are shell forwarders in `shell/ShellEditorForwarders.kt`: `ShellSlashCommandCompletionContributor`, `ShellComponentFoldingBuilder`, `ShellComponentLineMarkerProvider` and `ShellComponentStructureViewExtension` (the last three in `agent-doc-markdown.xml`), `ShellTurnStateStatusBarWidgetFactory` with its `ShellTurnStateStatusBarWidget`, and `ShellTurnStateBannerProvider`. Each asks the active generation's `ImplEditorExtensions` (`ImplGeneration.editorExtensions()`, served by the implementation's `AgentDocEditorExtensions`) for plain data (`SlashCompletions`, `ComponentFold`, a marker's component name and tooltip, `StructureNode` trees with a stable string key, `TurnStateStatus`, `TurnStateBanner`) and builds the lookup elements, fold descriptors, line markers, structure elements, widget and banner strip itself; the strip builder and its `#staleturnbanner`/`#duplicateturnbanner` sweep moved to `shell/ShellTurnStateStrips.kt`. The widget stages its turn-state listener under a shell disposable it disposes with the widget. The two notification groups were already XML-only. The audit enforces that the six classes are shell forwarders and that no implementation class extends a platform extension or widget type. Refreshing what the platform cached from an old generation (`EditorNotifications.updateAllNotifications()`, `StatusBarWidgetsManager.updateWidget(factory)`, `DaemonCodeAnalyzer.restart()`) belongs to the swap (R3): in R1 no generation is replaced while the platform holds its results. The `Notification` objects the implementation posts are measured (`notification=21`), not moved. |
| Modular extensions and Fleet RPC | **done** | The RPC contract carries the receipt (snapshot schema 5). The frontend module, `SurfaceSnapshotRemoteApiProvider`, `BackendSurfaceSnapshotRpcApi`, `AuthenticatedSurfaceIngressService` and the new `SurfaceIngressSpi.kt` are shell files in `editors/jetbrains-262/shell-sources.txt`. The ingress project service only delegates: it asks the active generation for its `SurfaceIngressFactory` (`ImplGeneration.moduleService`, which `AgentDocImplGeneration` resolves by class name from its own classloader, so the classic package, which has no split backend, answers null) and forwards every lease, snapshot and presentation-receipt call to the `SurfaceIngress` it creates. `AuthenticatedSurfaceIngressState`, the snapshot validator, the controller publisher and the layout forwarder are implementation: the state machine and validator moved to `AuthenticatedSurfaceIngressState.kt`, which brings ingress-only commits back to implementation-only (see "Re-measured yield"). `SplitShellOwnershipTest` (262 backend) enforces that the backend shell files name no backend implementation type and that the service declares nothing else. |
| EDT work | **done** | Implementation code queues all 43 EDT tasks through `GenerationWork.invokeLater`, which calls `GenerationContext.invokeLater`. The shell's `ShellGenerationWork` wraps each in a `FencedTask` and queues it with `ModalityState.defaultModalityState()` (what `Application.invokeLater(Runnable)` used) and an `expired` condition that is the generation's fence, so the event queue holds only shell tasks. A task drops its implementation runnable once it ran. Retirement takes a policy: a swap (R3) retires with `CANCEL`, which expires every queued task and clears its runnable before it runs; R1's only retirement, plugin unload, retires with `DRAIN`, so queued tasks still run exactly as before R1 (unload teardown itself queues EDT work, such as an AWT listener detached from a pooled dispose path). |
| Pooled work and threads | **done** | `GenerationContext.work` (`ShellGenerationWork`, one per generation) serves `executeOnPooledThread` (21 sites), `schedule`/`scheduleWithFixedDelay`/`submit` on the application pools (7), `newSingleThreadScheduledExecutor`/`newSingleThreadExecutor` with named daemon threads (9), `newThread` and `threadFactory` (14 raw threads), `newPooledAlarm` (1), and one `CoroutineScope` (`SupervisorJob` + `Dispatchers.Default`) that retirement cancels. Pooled tasks are counted while they run; executors and threads are tracked weakly, and retirement logs `[shell] implementation generation <n> retired pending_edt=… pooled_in_flight=… live_executors=… live_threads=…`. R1 does not stop or join them at retirement: the implementation's own unload owners (`PluginGeneration.registerResource`, `disposeProject`) still do, as before. Joining with a deadline belongs to the swap's quiesce (R3). The implementation reaches all of it through `GenerationWork`, which falls back to the platform call it replaced when no generation exists. The audit enforces `invoke_later`, `pooled_thread`, `app_executor_util`, `raw_thread`, `executors_new` and `alarm` at zero in implementation files and pins the staged counts. |
| Native library (JNA) | **done (shell classification)** | `NativeLib.kt` and `NativeReloadCoordinator.kt` are shell files. |
| Controller identity | **done** | Every `Register` op the backend's `ReliableSyncLivenessGraph` builds carries this JVM's `agent_doc_activation_receipt/v1` line as `activation_receipt` (`ShellGenerationHost.receipt()`, read when the op is built, so a native-reload republish reports the generation active then). It reuses the controller's existing `EditorRegistration.activation_receipt` (`agent_doc_reliable_sync_io::liveness`, serde default, omitted when absent), which the VS Code adapter introduced with `role=workspace`; the field is omitted when no host exists, so an older controller reads the same registration. `editors/jetbrains/src/test/resources/reliable-sync/jetbrains-register-with-receipt.json` is the exact op: `ReliableSyncLivenessGraphTest` asserts the plugin produces it, and `agent-doc-reliable-sync-io/tests/jetbrains_registration_receipt.rs` decodes it as a `role=backend` receipt and re-encodes it unchanged. `agent-doc reliable-sync-status` prints the parsed receipt. The receipt still carries `impl_sha256=none` and `epoch=0` until R2/R3, and a retired generation's context still refuses new listener registrations. |
| Per-platform-object state | **done** | The shell creates every user-data `Key` (`ShellUserDataKeys`, once per IDE run, lent as `GenerationContext.userDataKey`); implementation code takes its three keys from `GenerationKeys.create` (`key_create=0`, enforced). The highlight identity that each `RangeHighlighter` carried as the implementation's `VisualHighlightKey` is now stored as a JDK list, so platform objects hold only JDK and platform values. The four `putUserData` sites stay measured (`put_user_data=4`); the retirement sweep of shell keys belongs to the swap (R3). |

The counts come from `ShellRegistrationOwnershipTest`, whose pending-class
test prints them as `[shell-audit] implementation registrations still direct:`
from regex searches of the implementation file set. The design table counted
the whole source tree, so its numbers are larger. When a class lands, its
pattern moves from the printed list to an enforced zero.

**Activation receipt as implemented.** `shell/receipt/ActivationReceipt.kt` is
JDK-only. The classic jar compiles it, and so does the modular
`agent.doc.shared` module, which keeps one copy, and therefore one JVM instance
id, for the backend and frontend content modules. It is in its own package so
that no package is split across those modules. The line is exactly the
documented `agent_doc_activation_receipt/v1` format. A whitespace or `=`
character inside a value is replaced with `_`, so a receipt always stays one
line. Values in R1:

- `role=backend` on classic and on the 262/263 backend; `role=frontend` from
  the JetBrains Client frontend module.
- `jvm=` is a UUID stored in the `agent.doc.shell.jvm.instance` system
  property. Every classloader in one JVM reads the same value, and it survives
  a dynamic plugin reload.
- `shell=` is `shell_digest` from the build-time
  `com/github/btakita/agentdoc/shell/receipt/shell-identity.properties`;
  `unknown` when a build skipped `generateShellIdentity`.
- `spi=1` (`SHELL_SPI_VERSION`).
- Backend: `impl_version=<plugin version> impl_sha256=none generation=1
  epoch=0`. There is no separate implementation jar before R2, so nothing was
  verified by sha256. Frontend: `impl_version=none impl_sha256=none
  generation=0 epoch=0`.

Where it is published:

- The backend's `ShellGenerationHost` logs `[shell] <receipt>` once per plugin
  load, on first activation from `projectOpened`.
- The frontend logs `[shell] <receipt>` once per JVM. It sends the receipt in
  `FrontendSurfaceSnapshot.activationReceipt` (schema 5, never encoded while
  null), and `forSchema` drops it for a schema-4 backend.
- The backend records the latest receipt for each connection and logs
  `[shell] frontend client=<id> connection_generation=<n> <receipt>` when it
  changes. Nothing fences on receipts yet; that starts at R4.
- Every controller registration (`Register` op) from the backend carries the
  backend receipt as `activation_receipt` (see "Controller identity" above).

**Digests.** `editors/jetbrains/shell-sources.txt` (classic) and
`editors/jetbrains-262/shell-sources.txt` (added for 262/263) list the shell
file set; implementation is classic `src/main/kotlin` plus modular
`backend/src/main` outside it. `shell-identity.gradle.kts` hashes each set,
path plus bytes in path order, into `shell_digest` and `impl_digest`.
`gradle.properties` is in neither set, because its version and recorded source
digest change on every release. The digests are computed at build time and
not yet recorded: `pluginSourceDigest` and `scripts/check_plugin_versions.py`
still allocate versions from the union, as designed. Recording the shell digest
for the installer is R3 work.

**Re-measured yield.** The measurement takes the commits that `git log` lists
between 2026-09-26 and 2026-10-26 under `editors/jetbrains` and
`editors/jetbrains-262`. Under this classification, 110 of the 184 commits
that touched plugin sources or build files changed only implementation files.
The design's rule, applied to the same commits, gives 115 of 174; its earlier
figure of 138 of 187 used a different selection of commits. The 10 extra
commits in the total are build-script-only changes, which now count as shell.
Five commits that the design's rule called implementation-only now touch shell
files: `AuthenticatedSurfaceIngressService.kt` accounts for 3 and a
`build.gradle.kts` for 3, and one commit has both. R1 moved
`AuthenticatedSurfaceIngressState` out of the service file, so a change to the
ingress state machine is again implementation-only.

**Not yet present.** There is no `verifyImplJar` while no implementation jar
exists; `ShellRegistrationOwnershipTest` is the R1 audit in its place. The
forwarders do not yet wrap implementation failures as `PluginException`, which
matters only once a child loader defines the implementation (R2).

**What remains of R1, in order:**

1. ~~Project lifecycle and Tier A programmatic listeners~~ (done; see the table).
2. ~~Editor and language extensions~~ (done; the platform-cache refreshes come
   with the swap in R3).
3. ~~EDT and pooled work~~ (done; `CANCEL` retirement and the quiesce join come
   with the swap in R3).
4. ~~Controller identity~~ (done; the Rust field already existed from the
   VS Code receipt work).
5. ~~Per-platform-object `Key`s and the Fleet RPC delegation~~ (done).

R1's registration classes are all in place. What the platform still holds
from implementation code is measured, not moved: AWT and per-editor focus
listeners (`awt_or_component_listener=3`), posted `Notification`s
(`notification=21`) and the four `putUserData` values. R2 packages the
implementation into `impl/` and adds `verifyImplJar`.

#### Restatement of the Exact-263 reopen rule (adopted 2026-10-26)

**Adopted with the operator's agreement on 2026-10-26.** Under this design the
module graph is never removed or replaced, so the original point (1) ("removal
and replacement of the full modular graph") could never be met. The
"Exact-263 decision" rule above now reads:

> This boundary may change only with tests proving both (1) that across an
> implementation swap the modular module graph, the plugin descriptor, and
> every platform registration (actions, extensions, listeners, services, RPC
> providers) are unchanged, and the retired implementation generation's
> classloader becomes unreachable and is collected; and (2)
> generation-fenced backend/frontend activation receipts for the same verified
> artifact. A backend-only success is not acceptance evidence.

Point (2) is unchanged. The restated point (1) is what the swap protocol
asserts: forwarders registered once per IDE run, retirement through the
generation slot, and the `impl_generation_leak` watcher. Adopting it changed
the rule's text only. The receipts, the fence, and the R4 tests it names must
still exist before any exact-263 release skips a restart.

#### What the spike proves, and what it does not

`ShellGenerationSwapSpikeTest` packs each spike generation's compiled classes
into `agent-doc-impl-<version>.jar` on disk and loads it through
`ImplClassLoader` into a `GenerationHost`. A `FakePlatform` stands in for the
IDE and strongly retains its subscribers, its queued `invokeLater` tasks, and
its reported errors. The test proves:

- a verified versioned jar loads in a child-only loader that shares the SPI
  type with the shell;
- a swap moves the registration without re-registering. The platform holds one
  shell forwarder across generations, and state crosses only as a string;
- the retired loader is collected (`WeakReference` plus forced GC) even though
  the platform still holds the forwarder, one of the retired generation's
  queued tasks, and its reported failure. A stale queued task is inert. Two
  mutations each turn this test red: keeping the task's implementation
  reference, and keeping the original throwable as the failure's cause. A
  third mutation, keeping the implementation reference inside a retired slot,
  changes nothing, because no retained object reaches the slot;
- the negative control: a generation that subscribes to the platform directly
  pins its loader. The leak check reports it and further swaps return
  `RestartRequired`, so the detector is not vacuous;
- refusal and rollback: a sha256 mismatch and a jar defining a class outside
  the implementation prefix are refused while the active generation keeps
  running, an SPI mismatch is restart-required, and a replacement that fails
  in `prepare` rolls back to the quiesced generation;
- receipts round-trip as one line, and the fence admits only a backend plus a
  different-JVM client on the expected shell, implementation and epoch, under
  both frontend policies.

It does not exercise real IntelliJ: no `Disposer`, EDT, `ModalityState`,
`MessageBus`, Tier B registrations, Swing rebuilds, JNA, coroutine or thread
draining, the Fleet RPC, Remote Dev, Marketplace, `PluginException`
attribution, natural-GC leak timing, or metaspace cost. Those belong to R1
through R4.

### Restart-free logic upgrades: native derivations (GH #326)

The package swap above is closed on exact-263, but `libagent_doc` hot-reloads
under the running IDE (`hot-reloaded libagent_doc v<release>`). That is the
second lever of GH #295 ("path 2"): a rule that lives behind the native boundary
changes without a restart, while a rule written in Kotlin waits for one. The
0.35.528 restart (`restart_required=1`, plugin stale for 31m37s) was forced by
two Kotlin layout-sync rules exactly of that kind.

**Boundary.** The plugin reports what IntelliJ state *is*; the native library
derives what it *means*. Kotlin captures plain evidence (on the EDT when it is
IntelliJ state) and asks one named derivation through the single, stable entry
point `agent_doc_editor_derive_json(request_json)`:
`{"derivation": "<name>", "input": {...}}` answers
`{"derivation": "<name>", "output": {...}}`. A new or changed rule is a native
change with no new JNA method and no plugin generation. Shipped derivations:

- `window_layout`: the classic local `EditorWindow` layout. Evidence per window
  is its origin in the frame's splitter coordinates (absent when unreadable),
  its selected tab, and its session-document tabs, plus the project base path
  and the IDE selection history restricted to those tabs. Native owns the
  `#stickymdpane` window document, geometry-known collapse (GH #81), shared-origin
  columns (GH #77), x-tolerance grouping, the known single-window layout, the
  GH #320 order provenance, and the `[layout-detect] observed` line.
- `client_measured_layout`: the JetBrains Client's measured main window forwarded
  by the modular backend (GH #320 option B). Evidence per window is its ordinal,
  selected session document, whether it holds any session document, and its
  measured origin. The schema-version and main-surface checks stay with the RPC
  contract in Kotlin. Schema 3's frontend-authored `layoutColumns` is a frozen
  compatibility floor only: every live native generation receives the measured
  evidence first, so grouping changes remain restart-free (GH #346).
- `surface_projection` (phase 2): the automatic editor surface that
  `EditorTabSyncListener` publishes. Evidence is what one EDT turn reads: the
  surface frame's windows (`window_layout` evidence plus whether the frame could be
  measured), the backend window count, `FileEditorManager.selectedFiles`, every
  open session document, the Remote Dev clients' visible/selected/open session
  documents and backend focus, whether a measured client layout owns the project
  (`RemoteClientLayoutAuthority`), the anchor window's and selected text editor's
  session document, the triggering event's preferred/previous document, its
  authority and remaining settling passes, and its `force_reconcile` /
  `preserve_focus` flags. Native owns the remote-client authority decision
  (GH #154/#177), every readiness deferral (`capture deferred reason=...`), the
  `#stickymdpane` visible-document union including the GH #320 Remote Dev diagnostic union,
  active-document resolution, open-tab proximity, stale-selection settling (the
  exhausted-pass old-to-new edge, on visible documents and local columns), the local layout and
  its withholding when it omits the preferred document,
  the `[layout-detect]` lines, and the controller payload including
  `column_order`. Flat Remote Dev backend evidence remains focus/liveness-only; deferred capture
  and repeated observations cannot advance structural state. The plugin keeps the payload as JSON, so a payload field a
  newer library adds reaches the controller without a plugin generation.
- `sync_layout` (phase 3): the manual Sync Tmux Layout `--col` snapshot
  decisions, and the root choice and path normalization the automatic surface
  applies to the `surface_projection` payload. Evidence is the project base
  path, the focused document's own project root as the plugin resolved it, the
  focused and visible session documents, the detected split layout (absent when
  unreadable), and whether a foreground action asks. Native owns the root that
  spans the visible layout (the shared root of the visible documents, else the
  workspace base path for a cross-root layout or for rootless documents outside
  the focused root, else the focused root), the nearest `.agent-doc` root of
  each visible document (a filesystem read the IO crate makes off the EDT), the
  layout's rewrite into that root and to absolute paths (an empty column keeps
  its pane, an all-empty layout is none), the `--col` columns (one per detected
  column of a multi-column layout, else one per visible document, GH #81), their
  order label (GH #112/#185/#320), and the exact-visible decision: publish,
  refuse an unreadable split (GH #154), or refuse a foreground split that omits
  the focused document. The order label passes through as a string, so a label
  a newer layout derivation adds is kept. Phase 3b moved the last two callers of
  these rules onto the derivation. An optional `chosen_root` input is a root the
  caller already chose. When it is present and not blank, the snapshot addresses
  it instead of the spanning root and normalizes the layout against it. Two output
  fields were added. `layout_mode` is what the columns assert (GH #111): `exact`
  for a detected multi-column layout, otherwise `ensure`. `candidate_roots` lists
  the controller roots that may own the tmux focus, most specific first: the
  focused root, the spanning root (only with a focused root and visible
  documents), each visible document's own root (its nearest `.agent-doc` root,
  else the base path, else its directory), then the base path. Run Agent Doc's
  `--col` fallback (`TerminalUtil.editorSnapshotRouteLayout`) passes the route's
  own root as `chosen_root` and sends the derived columns, order label, and
  layout mode. The plugin's only step is the wire shape: the one empty column
  answered for an observation with no visible document becomes no `--col`. The
  `TmuxPaneFocusSync` poll asks the controllers in `candidate_roots`.
- `remote_layout` (phase 4): the legacy GH #134 Remote Dev observation boundary. Evidence is the
  project root, each Remote Dev client's
  visible/selected/open session documents, and the backend-focused session
  documents. Native classifies this flat evidence as focus/liveness-only and never derives,
  retains, or reconstructs structural columns from it (GH #134/#175/#185/#234/#343).
  Per-project memory, when present, is only an exact vector previously authored by a
  geometry-capable frontend; legacy observations preserve it byte-for-byte or remain unknown.
  The derivation shares this behavior with `surface_projection` and with the pre-#326
  `agent_doc_editor_surface_resolve_remote_layout_json` symbol. The plugin asks this derivation first and
  that symbol only when the loaded library predates the derivation.
- `remote_client_surface` (phase 4): what the 262/263 backend's
  `RemoteClientLayoutForwarder` publishes for a JetBrains Client's measured main
  window. Evidence is the project base path, the measured columns and selected
  document (the `client_measured_layout` answer), and that document's own project
  root as the forwarder resolved it on its worker. Native owns the root that
  spans the layout and the paths normalized against it (the `sync_layout` rules,
  so the forward, the manual sync, and the automatic surface agree on the root)
  and the `editor_surface` payload: `editor` order provenance and
  `preserve_focus`, because the snapshot names each split's selected document but
  not which split has keyboard focus. The payload stays JSON, so a field a newer
  library adds reaches the controller without a plugin generation.
- `registration_projection` (phase 5): the direction a CRDT replica
  registration projects (`CrdtReplicaManager.forwarderFor`). Evidence is facts,
  never text: whether a captured local delta owns the buffer, whether the
  controller retained a canonical projection or awaits a fresh-controller reseed,
  whether canonical covers the cached native frontier, which of the settled
  shadow, the live buffer, and the canonical are present, which of them are
  equal, and the answers of the native containment proof and clean three-way
  merge. The plugin compares the three texts itself over the exact UTF-16 text it
  holds, so no document crosses the boundary and no encoding can make two
  different buffers equal. Native owns the action (adopt canonical, publish the
  buffer from its proven shadow, merge forward, defer to the captured delta, or
  hold) and the hold reason the failure records
  (`ambiguous-retained-projection`, `retained-reseed-missing-settled-shadow`,
  `ambiguous-reregister-projection`). Executing the action, the
  publication-edge rechecks, and the order of their effects stay in Kotlin. An
  action this plugin generation cannot execute takes the frozen floor, so a new
  action needs a plugin generation; a new hold reason does not.
  The registration's clean-merge fact also answers true when native proves the
  live buffer already carries every canonical change since the settled shadow
  (`#lagcanonicalrollforward`, in `agent-doc-merge` `rebase`); the merge it then
  publishes is the buffer itself. When canonical also carries a controller
  change the buffer lacks (a response, an appended row), native answers a
  row-aware merge instead (`#lagcanonicalrowmerge`): the buffer's text with
  that change rolled forward and canonical's claim marker on the buffer's row
  of the same work id. `forwarderFor` releases the retained native
  resume state only at its two commit points
  (`releaseRetainedResumeStateOnCommitUtil`, `#resumefrontierlostonfailedattach`),
  and logs a swap refusal with whether that frontier was kept.
  `#ackedreplicabase`: the containment proof and the clean merge are first asked
  from the settled shadow; when that proves neither, they are asked again from
  the cached endpoint's replica text (`registrationCausalProofUtil`). That text
  is a proven common ancestor only when the controller answers
  `canonical_covers_retained_frontier = true` for the state vector captured from
  the same replica, and the replica text read before and after that capture is
  identical (`acknowledgedReplicaBaseUtil`; a library-reload resume state has no
  text and never qualifies). A buffer equal to it reports containment (canonical
  holds everything the buffer has); a canonical equal to it merges to the buffer
  itself; otherwise the native containment proof and rebase run from it. The
  settled shadow advances only on an accepted visible projection, which every
  keystroke defers, so an operator who kept typing while the controller wrote
  another line (the resume id) held `ambiguous-retained-projection` although the
  two sides touched different lines (agent-doc-bugs.md, 2026-10-28 12:57:53).
  A merge-forward recomputes from the ancestor its decision was proven from, and
  an adoption proven from the replica text refuses (and retries) unless the live
  buffer is still exactly the text the proof covered. A same-line conflict still
  holds. Model: `formal/tla/RegistrationAcknowledgedBase.tla`.
- `attach_failure_diagnosis` (phase 5b): what a CRDT replica attach failure
  means for the operator. Evidence is the recorded failure reason, for the
  operator notification the document's file name, and `live_replica_attached`
  when `forwarders[filePath]` is still attached (checked before diagnosing;
  sent only when true). Native owns the one-line
  remedy, the `attach_refused_<token>` ops-log status token, whether the hold is
  persisted and surfaced without waiting for an editor action (the three
  projection-hold reasons), whether the refusal needs a reliable-liveness
  republish (`replica_register_stale_editor_endpoint`), whether retry is automatic
  or must await operator evidence, the hold notification text, and
  `hold_notification` (`error` or `log_only`). A
  projection hold beside a live replica refused only a replacement
  registration, so native answers no remedy, a message that names no repair,
  and `log_only`; the plugin records the hold event and logs the message
  without a notification. Without an attached replica the close/repair/reopen
  remedy and error notification are unchanged. The frozen floor composes only
  `log_only` from the same fact; an older library that omits the field keeps
  the floor's value. The surfaced-hold memory keys a live-replica hold apart
  (`surfacedAttachFailureKeyUtil`), so a later hold with no attached replica
  still notifies.
  `CrdtReplicaManager.recordRegisterFailure` asks it once per
  failure, after the retired-generation check, and executes the answer in the
  pre-#326 order: the hold event and notification, then the liveness republish,
  then either a bounded retry or an operator-evidence pause. A hold reason is
  surfaced once per continuous refusal episode. Its per-path surface memory is
  separate from retry projection state because an operator-evidence pause
  deliberately clears the retry projection; repeated explicit recovery with
  the same unresolved reason therefore cannot stack notifications or duplicate
  hold events merely because its failure count starts again at one. Successful
  registration, tab close, or terminal reclassification clears that surface
  memory and rearms a later episode. The three
  projection holds never arm a timer: close/repair/reopen (or an explicit typed
  recovery request) supplies the next evidence, so an unchanged buffer cannot
  drive a background register/deregister loop. On tab close, a refused hold with
  no attached forwarder releases editor liveness without publishing the
  ambiguous cut; detached repair remains the authority-reconciliation boundary.
  Compact Exchange asks it on its pooled thread, records the
  refusal event, formats the refusal there, and only shows it on the EDT. A new
  reason, remedy, or wording needs no plugin generation; a new *kind* of effect
  does.
- `route_outcome` (`#jbshrinkrestarts`): what a finished Run Agent Doc route's
  output means for the operator (`TerminalUtil.sendToTerminal`). Evidence is the
  route's output text and the document's relative path. Native owns the outcome
  kind (still running, startup not ready, steering delivered, queued behind the
  owner, paused queue, deferred harness switch, unproven dispatch start,
  protected prompt input, or a persistent failure), the paused-queue details
  (reason, stale-supervisor redirect, stale pid), the deferred switch (previous
  and target harness, paused queue, forced restart needed, restart already in
  flight, supervisor unavailable), the notification text of every non-persistent
  kind, the latest `snapshot_path=`, and the two success notices
  (`[agent-doc] owned_by_other_lane:` and the GH #243 `[route] pane-not-visible:`).
  The route thread asks once per route and executes the answer: the notification
  type, its actions, the ops-log line, the persisted failure output, and the
  state projection stay in Kotlin. `RouteOutcome.frozen` composes the pre-move
  `TerminalUtil` rules and is never extended. A kind token this plugin generation
  cannot execute takes the whole floor, so a new *kind* needs a plugin
  generation; new wording, or a new output shape for an existing kind, does not.
  The native rules reproduce `java.util.regex`'s ASCII `\s`, `\S`, `\d`, and
  `\b`, Kotlin's `trim`/`isBlank` whitespace (which keeps U+0085 and strips
  U+001C..U+001F), and `lines()` splitting at CRLF, LF, and CR. The parity file
  `editor-derivations/route_outcome.json` is answered by both.
- `exchange_prefix_repair` (`#jbshrinkrestarts`): the patch watcher's `❯ `
  prompt-prefix repair (`PatchWatcher.computePatchOnWorker`, after the patches
  and the boundary reposition). Evidence is the patched document and the patch's
  `normalize_prefix_lines`. Native owns the operator region (before the last
  boundary marker), the target match (trailing whitespace ignored; every
  occurrence), the response-block heuristic (response headings; which lines read
  as a fresh prompt, a response label, list or quoted context, or response prose),
  and the repaired text. The answer is `{"changed": false}` or
  `{"changed": true, "document": ...}`, so an unchanged document does not cross
  back. The patch worker asks off the EDT; with no prefix targets nothing is
  asked. `ExchangePrefixRepair.frozen` is the pre-move
  `normalizeExchangePrefixesUtil` and is never extended. This is a port of the
  plugin's rule, not the binary's `normalize_exchange_prefixes_for_targets`
  (which counts target occurrences and tracks prefixed response headings);
  converging the two is now a native-only change. The parity file
  `editor-derivations/exchange_prefix_repair.json` (named cases plus a seeded,
  generated mix including CRLF, no-break and other Kotlin whitespace, U+0085,
  dotted capital I, and astral text) is answered by both.
- `session_command_outcome` (`#jbsessioncmdoutcome`): what a finished Clear
  Session Context or Restart session command's output means for the operator
  (`TerminalUtil.clearSessionContext` and `runRestartCommand`). Evidence is the
  command (`clear` or `restart`; agent and supervisor share the wording),
  whether it exited zero, whether a restart was forced, the trimmed output, and
  the document's relative path. Native owns the outcome kind (`cleared`,
  `clear_queued` behind a live turn, a settling prompt, or an accepted Run Agent
  Doc dispatch, `clear_deferred` under a busy auto-loop, `clear_refused` for a
  busy pane, protected prompt input, or a live turn no supervisor owns,
  `restart_refused_editor_holds_pane`, `restart_refused_busy`,
  `restart_refused_starting`, or `generic` for the adapter's own failure or
  restart-success handling), the matching detail (exactly the one its kind
  names), and the notification text. A forced restart reads only the
  editor-holds-pane refusal, which `--force` cannot bypass; a successful restart
  is always `generic` (its notice comes from the ops-log telemetry, which stays
  in Kotlin). The command thread asks once per command and executes the answer:
  the notification type, its actions (Refresh and retry only for a non-protected
  clear refusal; no Interrupt and restart for an editor holding the pane), the
  Copy details hint, and the clear-command bookkeeping stay in Kotlin.
  `SessionCommandOutcome.frozen` composes the pre-move `TerminalUtil` rules and
  is never extended. A kind token this plugin generation cannot execute, or a
  refusal kind without readable details, takes the whole floor, so a new *kind*
  needs a plugin generation; new wording, or a new output shape for an existing
  kind, does not. The native rules reproduce `java.util.regex`'s ASCII `\s`,
  `\S`, `\d`, and `\b` with DOT_MATCHES_ALL headers, Kotlin `trim`/`isBlank`
  whitespace, `toLongOrNull`, `substringBefore`, `removeSuffix`, and
  `removeSurrounding`. The parity file
  `editor-derivations/session_command_outcome.json` is answered by both.
- `capabilities`: the derivations the loaded library answers, and (phase 5c)
  `captured_splice_inputs`, the optional inputs its captured-splice calls accept.

**Threads.** Native calls stay forbidden on the EDT. The automatic surface
captures evidence on the EDT and derives `surface_projection` on the
generation-owned surface delivery worker; the EDT runs no surface rule. A
deferred projection keeps its observation in the pending slot (it is released
only once a ready projection goes to delivery) and the worker schedules the next
bounded EDT re-read, or, with no settling pass left, retains the observation and
retires surface endpoints the capture proved closed. Because the slot is now held
until the worker answers, a lower-authority edge that arrives meanwhile, which
the ordering still refuses to let replace the pending observation, re-reads that
observation on a later EDT turn instead of being dropped against a stale
capture. After the derivation, the worker asks `sync_layout` for the root that
spans the visible documents and the columns normalized against it. Run Agent
Doc and Sync Tmux Layout capture the same evidence and derive it on their
background thread; a deferral there is `surface_capture_deferred reason=...`
and the command keeps its `--col` snapshot. Sync Tmux Layout reads only editor
state on the EDT (the focused and visible session documents, then the command
surface or, for the `--col` snapshot, the split layout evidence:
`LayoutDetector.captureEditorLayoutEvidence`). Its background thread resolves
the focused root, derives the layout from that evidence (`window_layout` or the
Remote Dev non-structural observation, with no EDT budget), asks `sync_layout`, refuses before
preparing the terminal, and then prepares the terminal and submits. A direct
EDT caller of `window_layout` (claim, terminal and layout-change detectors)
marshals only the immutable evidence to a worker and waits at most 250 ms. Run
Agent Doc's `--col` fallback reads only editor state on the EDT (the visible and
focused documents and the split layout evidence); its route thread derives the
layout from that evidence and asks `sync_layout`. The `TmuxPaneFocusSync` poll
resolves the focused root and asks `sync_layout` on its own poll thread. The
measured-layout forwarder already runs on its own worker, where it also resolves the focused document's root and asks
`remote_client_surface`. Remote Dev backend observation runs on whichever worker holds the
captured client evidence; a direct EDT caller marshals only that evidence. No
register failure is recorded on the EDT (phase 5b): the register retry timer
reads the editor file and document on the EDT and hands an unavailable one to
the replica control worker as the last effect of its turn
(`recordRegisterFailureOffEdt`), and the native-reload target collection only
reads editor state inside its EDT turn, then records each unavailable document
on the reload thread, in collection order, before any registration is awaited.
Restart Agent (the action, and the Interrupt and Restart dialog's confirmation)
runs on the EDT. It reads only the open buffer's text there, then hands that
text to a pooled thread (`TerminalUtil.launchRestartCommandOffEdt`), which reads
`agent:` through the native `agent_doc_frontmatter_agent` export and launches
`session restart-agent --expect-agent`. A failed native read, or no worker to
run it, launches without `--expect-agent` (the `#restartagentharness` EDT fix). Plugin 0.2.866 made
that native read on the EDT, so every Restart Agent threw the bridge's EDT
guard and nothing restarted.

**Compatibility.** After a hot reload the library is newer than the plugin
generation calling it, so a shipped derivation is never removed or renamed,
every input field stays optional, outputs only gain fields, and the plugin
ignores output fields it does not know. An unknown derivation is an error. A
library without the symbol is remembered per native generation, so the next
hot-reloaded library is asked again. Whenever no native generation answers, the
plugin uses a **frozen** Kotlin copy of the GH #320 rules (`frozenWindowLayout`,
`RemoteClientMeasuredLayout.frozenDerive`, and `SurfaceProjection.frozen` over the
pre-#326 `SurfaceReport` / `SelectionProjectionSettling` rules, and
`SyncLayoutSnapshot.frozen` over the pre-#326 `SyncLayoutAction` rules
`chooseSyncProjectRoot`, `normalizeEditorLayout`, `absolutizeEditorLayout`,
`undetectedLayoutColumns`, and `exactVisibleSyncDecision`, plus, since phase
3b, the pre-#326 Run Agent Doc fallback for a chosen root, `routeLayoutMode`, and
the `TmuxPaneFocusSync.candidateProjectRoots` probe, and
`RemoteClientSurfacePayload.frozen` over the pre-#326 forward rules: those
`SyncLayoutAction` root and path rules, then `SurfaceReport.buildSurface`, plus
`RegistrationProjection.frozen` and, since phase 5b, `AttachFailureDiagnosis.frozen`
over `attachFailureRemedy`, `attachFailureStatusToken`,
`retainedProjectionHoldNeedsOperatorSurfaceUtil`,
`registerFailureNeedsLivenessRepublishUtil`, and
`retainedProjectionHoldMessageUtil`, and, since phase 5c,
`RegistrationSpliceShaping.frozenOperatorEditsContained` over
`withoutBinaryOwnedMarkersUtil` and `singleSpliceBatchUtil`, and
`cleanRegistrationMergeUtil`). It is
a compatibility floor, never extended: new rules land only in the native
derivation. One parity file per derivation under
`editors/jetbrains/src/test/resources/editor-derivations/` (`window_layout.json`,
`surface_projection.json`, `sync_layout.json`, `remote_client_surface.json`,
`registration_projection.json`, `attach_failure_diagnosis.json`,
`registration_splice_shaping.json`) is
answered by both the Rust derivation and the frozen floor, so a divergence is
always a deliberate fixture edit. An `attach_failure_diagnosis` answer that
leaves out a field takes that field from the floor. A library older than phase 3b ignores
`chosen_root`. An answer that addresses another root therefore takes the floor,
and an answer without `layout_mode` or `candidate_roots` gets them from the
floor's rules. `sync_layout.json` and
`remote_client_surface.json` map each document to its nearest project root in
place of the filesystem; the 262/263 backend's test resources include the
directory so its floor answers the same file.

**The Remote Dev compatibility observation has no frozen copy.** It has been native since GH #134;
the plugin's only Kotlin rule for it was a memoryless fallback
(`uniqueRemoteSplitSelection`: one client naming two or more visible, else
selected, documents) for a library without the compatibility symbol. Phase 4 retired it.
With no native answer, through neither `remote_layout` nor the pre-#326 symbol,
the Remote Dev layout is unknown (`[layout-detect] unknown ...
reason=native_remote_layout_unavailable`), which is structurally inert (an
observation without columns, GH #105): the automatic surface publishes no
columns, so it may select the focused document's pane but never changes pane
membership, and Sync Tmux Layout refuses the unreadable split (GH #154). Only a
plugin with no loaded library, or one beside a library older than v0.35.454 (the
first with the compatibility symbol), reaches that answer.
`remote_layout.json` is therefore a contract file, not a parity file: the Rust
classifier answers its steps in order from an empty memory, and `LayoutDetectorTest`
pins that the plugin sends each step's input exactly and publishes each answer.

**Still restart-bound.** Changing the *evidence* a derivation needs (a new
IntelliJ fact to capture), adding a call site, `plugin.xml`/action/UI changes,
and logic not yet behind the boundary still ship as a plugin generation. The
Remote Dev client-evidence capture (which client editors are visible, selected,
and open; the frontend snapshot's windows) reads IntelliJ state and stays in
Kotlin by design. The largest remaining Kotlin rule sets are the CRDT replica,
patch-watch, and route clients (phase 5). Run Agent Doc's `--col` fallback and the
`TmuxPaneFocusSync` candidate-root probe moved behind `sync_layout` in phase 3b.
The automatic surface's settling loop itself (how many EDT re-reads, the
pending-slot ordering) is orchestration and stays in Kotlin. Each remaining rule
set moves behind `agent_doc_editor_derive_json` the same way.

**Phase 5 audit: the CRDT replica, patch-watch, and route clients.** Phase 5
listed every pure decision in `CrdtReplicaManager`, `PatchWatcher`, and
`CpRouteClient` (the 262/263 backends compile these classic sources; they have
no counterparts of their own) and ranked them by `git log` since 2026-09-20:
46 commits touched `CrdtReplicaManager`, 16 `PatchWatcher`, 13 `CpRouteClient`.
Most of that churn is in orchestration, not in pure decisions: `forwarderFor`,
`ensureOpenDocumentReplica`, and `forwardLocalEditsFromShadow` in the replica
manager, and the intent dispatch in `PatchWatcher.handleSocketMessageV2`. Those
order registration, deregistration, IntelliJ document mutation, and receipts,
so they stay in Kotlin by design.

*Moved.* The registration direction
(`retainedRegistrationProjectionActionForAttachUtil`,
`retainedRegistrationProjectionActionUtil`, and the hold reason) changed in six
commits between 2026-09-24 and 2026-10-07, more than any other pure decision in
the three classes. It is the `registration_projection` derivation, asked on the
replica worker that already runs the socket registration and the native
containment and merge proofs. `RegistrationProjection.frozen` composes those
pre-#326 rules and is never extended. The shared parity file
`registration_projection.json` holds named cases with real document text (the
plugin's fact capture is checked against them, including composed versus
decomposed accents, CRLF, and astral text) and an exhaustive table of all 5184
combinations of the facts. Both the Rust derivation and the frozen floor answer
every entry.

*Still restart-bound, and why.*
- Native-reload decisions (`nativeReloadReplicaRestartReport`,
  `settledNativeReloadReplicaRestartReport`, `mergeReplicaRestartReports`,
  `dynamicLoadReattachReceipt`, `nativeReloadCheckpointTarget`,
  `nativeReloadRegistrationStateUtil`, and the GH #297 recovery-capture miss
  `replicaRecoveryCaptureMissReasonUtil` /
  `replicaRecoveryCaptureMissIsDeferredUtil`) answer while the native library is
  being handed off or the dynamic plugin load reattaches. They cannot depend on
  the library they describe, so they stay in Kotlin.
- The attach-failure diagnosis moved in phase 5b (the
  `attach_failure_diagnosis` derivation). First its EDT callers moved: the
  register retry timer and the native-reload target collection now record a
  failure on a worker, and Compact Exchange formats its refusal on its pooled
  thread. The Compact Exchange refusal sentence itself (`attachRefusalMessage`)
  is UI wording around the derived remedy and stays in Kotlin.
- Local-edit capture (`prepareLocalEditorEditsUtil`,
  `reconstructCapturedLocalEditorBatchUtil`, `currentEpochCapturedEditsUtil`,
  `singleSpliceCapturedEditUtil`, `singleSpliceBatchUtil`) maps IntelliJ UTF-16
  document offsets on the keystroke path. The rebase that consumes it is already
  native (`agent-doc-merge` captured splices).
- The registration text shaping (`withoutBinaryOwnedMarkersUtil` and the single
  splice that `cleanRegistrationMergeUtil` and the containment proof build) moved
  in phase 5c, into the native containment and rebase calls themselves (see
  below). It never crosses the derivation channel.
- `closingDocumentCutRequiresTextUtil` / `closingDocumentCutRequiresReplicaUtil`
  changed because the read-permit order on close changed (GH #287/#298), which is
  ordering, not a rule.
- `PatchWatcher`: no pure decision changed more than twice. Its text utilities
  (boundary reposition, exchange prefix repair, replay detection) are stable, and
  `extractResumeDecision` is a three-way operation-id match added once. The
  prefix repair moved anyway (`#jbshrinkrestarts`, the `exchange_prefix_repair`
  derivation): it is a heuristic word list, the one PatchWatcher rule an editor
  wording fix would touch. The rest of PatchWatcher is the socket intent
  dispatch, IntelliJ document mutation, and receipt ordering, which stay in
  Kotlin by design; its document text operations already call `NativePatching`.
- `TerminalUtil` (not in the phase 5 audit): the Run Agent Doc route-output rules
  changed in more commits since 2026-08-01 than any other `TerminalUtil` code and
  moved as the `route_outcome` derivation (`#jbshrinkrestarts`). The remaining
  `TerminalUtil` code spawns `agent-doc` subprocesses, builds their argument
  lists, and shows notifications. The clear and restart refusal parsers
  (`parseBusySessionClearRefusal`, `parseLiveTurnQueuedClear`, and the restart
  refusals) are the next candidates of the same shape.
- `CpRouteClient`: the most changed functions build the controller request
  (`editorRoutePayload`, `editorRouteRequest`, `editorCommandSubmitRequest`, five
  commits each). Every change added a captured field the controller reads (order
  provenance, layout mode, navigation lane, surface reference). A new captured
  field needs a plugin generation anyway, and the controller already derives the
  route from the payload (`surface_layout_reference_v1`), so the builders stay
  wire formatting.

**Phase 5c: registration shaping inside the native captured-splice calls.**
A replica registration asks two native calls about its three generations (the
settled shadow, the live buffer, the controller canonical): the containment
proof (`agent_doc_captured_splices_contained`, `#ambiguousholdforever`) and the
clean merge (`agent_doc_rebase_captured_splices`, `#editorauth2`). Before phase
5c the plugin shaped their input itself. For the proof it removed the
binary-owned markers from all three texts (`withoutBinaryOwnedMarkersUtil`,
`#ambiguousholdforever2`), answered true when the buffer then equaled the shadow
or canonical, and otherwise sent the one splice from shadow to buffer
(`singleSpliceBatchUtil`). For the merge it sent the one splice over the raw
texts. Both symbols now also accept an optional `registration` input in their
`edits_json` argument in place of a batch:
`{"registration": {"buffer": "<live buffer>", "operatorText": true|false}}`. The
`base` and `canonical` arguments stay the raw shadow and canonical. Native then
does the shaping itself (`agent-doc-merge` `captured_splice_request`):
`operatorText` asks it to compare operator text only, so native owns *which*
markers are binary-owned (`registration_operator_text`), the operator-text
shortcut, and the code-point splice (`single_splice`). A rebase refuses
`operatorText`, because its answer is written text. A change to the
normalization or the shaping therefore ships with a library hot reload. Kotlin
still applies the raw guards (a missing generation, or two equal raw
generations, asks no native call), runs both calls at the same point on the same
replica worker, and executes the answer; no side effect moved.

*Compatibility.* The plugin sends `registration` only to a library whose
`capabilities` derivation lists it in the new output field
`captured_splice_inputs`. It reads that once per native generation and sends the
call to the same generation (`NativePatching.askCapturedSplicesWithRegistration`).
A library that does not advertise it, or no library, gets the frozen floor:
`RegistrationSpliceShaping.frozenOperatorEditsContained` and
`cleanRegistrationMergeUtil` shape the batch exactly as before and send it to
the same symbols, which still answer a plain batch exactly as before. A folded
answer that refuses or cannot be read is "not proven" for the proof and "no
merge" (hold the buffer) for the merge, as a refusal always was. The native rule
reproduces the plugin's `java.util.regex` rule byte for byte: a boundary line is
optional spaces and tabs, `<!-- agent:boundary:ID -->` with `ID` matching
`[a-z0-9][a-z0-9:-]*`, then optional spaces and tabs, removed with a following
`\n` or `\r\n` (any other terminator stays); then a trailing ` (HEAD)` plus spaces
or tabs is dropped from a line that starts with one to six `#` and a space; lines
end at `\n`, `\r\n`, `\r`, U+0085, U+2028, and U+2029. Splice offsets count code
points. The shared parity file `editor-derivations/registration_splice_shaping.json`
pins the normalization (its expected values come from the `java.util.regex`
rule; the cases include CRLF, a lone CR, U+0085/U+2028/U+2029, composed versus
decomposed accents, astral and ZWJ emoji, and look-alike markers that are
operator text) and, per registration, the three operator texts, the shortcut,
both splices, and both native answers. agent-doc-merge answers every entry
through the `registration` input and through the pre-5c batch path, and
`RegistrationSpliceShapingTest` checks the frozen floor and the request the
plugin sends against the same file. This differs from the controller's
`without_binary_owned_markers` (the retained-write proof), which accepts more
boundary ids and headings; registration keeps the plugin's rule until it is
deliberately changed in native code and the parity file.

**Keystroke latency gate (`#jbshrinkrestarts`).** A rule moves behind the
boundary only if the keystroke path does not get slower. `make
jetbrains-keystroke-bench` builds the local-install profile and runs
`runKeystrokeLatencyBenchmark` (`KeystrokeLatencyBenchmarkMain.kt`), a task the
init script `benchmark/keystroke-latency.init.gradle.kts` adds so that changing
the benchmark never changes `pluginSourceDigest`. It is
headless, like the cross-editor harness. Every timed call goes through the
shipped `AgentDocLib` proxy and Kotlin helpers, against the `libagent_doc` that
`agent-doc lib-path` resolves on `PATH`. The document is a deterministic,
synthetic ~125 KB session document (multi-byte text, a queue, a backlog, and an
exchange of prompt and `### Re:` cycles). `AGENT_DOC_BENCH_FIXTURE` swaps in a
*copied* document; it never points at a live one. The document's path is outside
every `.agent-doc` project, so no controller or socket is reached. What remains
is exactly the cost a rule move could change.
Cases, in microseconds per operation:
- `keystroke.typing_report`: TypingTracker's quiet-boundary report, which sends
  the whole buffer through `agent_doc_lazily_current_observed_v1`.
- `keystroke.replica_flush_1` and `_16`: CrdtReplicaManager's splice flush for a
  1- and a 16-keystroke burst: `prepareLocalEditorEditsUtil`, one `apply_local`
  per splice, then the state vector and the incremental diff. Each step is also
  reported as `<case>.<step>`.
- `keystroke.replica_flush_after_remote`: the 1-keystroke flush, each one the
  first after a concurrent peer's one-character update. The update is applied
  and projected the way `CrdtReplicaForwarder.applyRemoteUpdate` does it:
  `apply_update`, then the state vector and the text. Only the flush is the case
  figure. The remote apply is reported as its `.remote_apply` step.
- `boundary.worker_hop`: the hand-off to a worker thread and back.
- `boundary.derive_capabilities`: the smallest `agent_doc_editor_derive_json`
  round trip.
- `move.<rule>.native` and `.frozen`: each moved rule, asked through the
  derivation channel next to its frozen Kotlin floor, on the same input.

The keystroke and boundary cases run first, as a phase of their own, so the
gated cases see the same process whatever moved rules a build adds; the
`move.*` cases follow. A library that predates a moved rule skips its cases
(`skipped_moves`), so one harness measures both sides of a move. Within a
phase the cases run interleaved in five rounds, so a slow stretch of the
machine lands on every case. `p50_best` is the median of a case's fastest
round; two runs of one build agree on it to within about 10%.
`JETBRAINS_BENCH_BASELINE=<report.json>` compares this run with an earlier
report. The run fails when the `p50_best` of `typing_report`, `replica_flush_1`,
`replica_flush_16`, or `replica_flush_after_remote` grows by more than
max(5 us, 15%). A baseline report that lacks a case does not gate it. Other cases, and the
per-step breakdowns, are reported but not gated.

The 2026-10-26 baseline (0.35.556, release-local, synthetic 127,583-byte
document, two runs) puts the cost of the keystroke path in the replica diff,
not in the plugin. `typing_report` is about 106 us. A flush costs about 50 to
55 ms whether the burst is 1 or 16 keystrokes: `agent_doc_replica_diff` takes
about 47 to 53 ms of it and the state vector about 1.4 ms. `prepare` (about
51 us), `apply_local` (about 14 us for one splice, 37 us for 16), the worker
hop (2.4 us), and the smallest derivation round trip (6.5 us) are small next to
it. On a copied 32 KB live session document the diff takes about 4.9 ms, so
the diff cost grows faster than linearly with the document's size. The diff is
native, so a fix to it ships with a library hot reload.

`#replicadiffcost` made the native flush proportional to the edit. The fix is
described in `specs/14-realtime-workflow.md`: local minting from a visible-order
index, and diffs answered from a journal of local mints. Against a fresh
baseline of the unmodified build on the same machine (synthetic document,
`p50_best`):

| Document | Case | Before | After |
|----------|------|--------|-------|
| 127,583 B | `replica_flush_1` | 21,718 us | 59 us |
| 127,583 B | `replica_flush_16` | 22,798 us | 83 us |
| 127,583 B | `replica_flush_1.diff` | 20,627 us | 16 us |
| 127,583 B | `replica_flush_1.state_vector` | 982 us | 2.4 us |
| 34,701 B | `replica_flush_1` | 5,343 us | 43 us |
| 34,701 B | `replica_flush_1.diff` | 5,055 us | 17 us |
| 528,985 B | `replica_flush_1` | 305,279 us | 203 us |
| 528,985 B | `replica_flush_1.diff` | 297,927 us | 17 us |

`typing_report` stayed within noise: 105.7 to 108.0 us at 125 KB. The 2026-10-26
reference baseline (54,672 and 55,156 us for the two flush cases) passes the gate
too. The diff and the state vector now cost the same at every document size.
What still grows with the document is the Kotlin `prepare` step, which reads the
whole shadow text: about 30 us at 125 KB and about 170 us at 500 KB.

`#replicaremoteflush` folds remote updates into that index as they arrive, so the
first local flush after a remote update no longer rebuilds it from the operation
graph. Two interleaved runs per build against the unmodified build, on a loaded
machine (synthetic 127,583-byte document, `p50_best`):

| Case | Before | After |
|------|--------|-------|
| `replica_flush_after_remote` | 46,131 to 73,775 us | 166 to 172 us |
| `replica_flush_after_remote.apply_local` | 45,951 to 73,531 us | 9.2 to 11.1 us |
| `replica_flush_after_remote.diff` | 114 to 139 us | 82 to 89 us |
| `replica_flush_after_remote.remote_apply` | 61,907 to 86,133 us | 63,766 to 71,024 us |
| `replica_flush_1` | 57.7 to 64.1 us | 59.4 to 59.7 us |
| `replica_flush_16` | 89.2 to 96.8 us | 91.4 to 97.4 us |
| `typing_report` | 107.7 to 116.0 us | 107.9 to 108.1 us |

The gate passes against the unmodified build. The remote apply did not change:
the CRDT's own text projection dominates it, and the editor needs that text
anyway. The fold and the order's spelling check are small beside it.

`#kotlinflushprep` took the per-character work out of `prepare`. It used to copy
the shadow into a `StringBuilder` and count code points from the end of the
document (`String.codePointCount` over the whole shadow, then
`Character.codePointCount` back to the splice). Now
`LocalEditorSplicePreparation.kt` applies the burst to one edit window over the
shadow. It reads only the window, and it converts offsets with an index of the
shadow's surrogate pairs. That index is cached by String identity, and each
flush stores the index of the text it produces, so typing never rescans a
shadow. The native edits are the same as before. `LocalEditorSplicePreparationTest`
runs the old walk next to the new one on randomized bursts: clustered and
scattered splices, multibyte text, surrogate pairs, unpaired halves, and the
three refusals. It also checks the window's code point count for any range.
Six mutations of the window and index code were each caught.

What still grows with the document is building `resultingText`. The shadow,
the forwarder's `knownReplicaText`, and the next flush's base are `String`s, so
every flush allocates and copies the whole next shadow. A JVM microbenchmark of
that copy alone measures about 35 us at 125 KB and about 170 us at 500 KB, which
matches what `prepare` still costs. Removing it needs a non-`String` shadow,
such as a rope or a piece table, in `CrdtReplicaManager` and
`CrdtReplicaForwarder`. Synthetic document, `p50_best`, same machine and native
build, before -> after:

| Document | `prepare` (1 / 16) | `replica_flush_1` | `replica_flush_16` | `typing_report` |
|----------|--------------------|-------------------|--------------------|-----------------|
| 34,701 B | 8.8 / 10.5 -> 5.6 / 7.3 us | 25.5 -> 20.8 us | 54.8 -> 52.7 us | 38.0 -> 39.2 us |
| 127,583 B | 33.8 / 36.3 -> 25.7 / 23.1 us | 54.2 -> 57.0 us | 94.7 -> 71.7 us | 116.8 -> 108.1 us |
| 528,985 B | 229.3 / 269.0 -> 177.1 / 179.3 us | 254.9 -> 200.5 us | 332.2 -> 228.8 us | 493.9 -> 399.7 us |

The gate passes at all three sizes.

The two `#jbshrinkrestarts` moves left every gated case within noise of that
baseline (`typing_report` 106.1 -> 105.8 us, `replica_flush_1` 54,672 ->
49,029 us, `replica_flush_16` 55,156 -> 54,264 us). They are not on the
keystroke path. `route_outcome` costs 38 us natively against 25 us for the
frozen floor, once per Run Agent Doc. `exchange_prefix_repair` costs 1.2 ms
natively against 0.3 ms for the floor on the 125 KB document when the repair
changes it, because the document crosses both ways. That happens once per
patch that names prefix targets, on the patch worker.

Local package convergence compares every ZIP payload byte and relative path
with the installed plugin tree before replacing it. A byte-identical package is
a true no-op: the installer leaves the existing files and inodes in place so a
live IDE does not retain deleted mappings of the same generation. For a changed
package, the installer discovers live JetBrains JVMs and attaches the packaged
system-classloader upgrade bridge. The bridge first verifies that the live
plugin root belongs to the installation being updated, then explicitly invokes
the outgoing generation's open-project cleanup hook before unload. This cleanup
must stop every document listener, CRDT worker, and static project registry; two
plugin-classloader generations must never observe the same IntelliJ `Document`,
because each would classify the other's remote projection as operator input and
rebroadcast it. For the first upgrade from a generation predating the public
cleanup hook, the bridge calls its compatible Kotlin companion cleanup entry
point reflectively. A missing or failed cleanup is fail-closed. The bridge then
unloads the current descriptor with JetBrains' explicit update semantics (`disable=false`,
`isUpdate=true`). It must prove that descriptor is no longer loaded before
calling JetBrains' dynamic install-and-load API with the complete ZIP, and it
must reject an identity- or classloader-equal result. This keeps the plugin set
on one live generation across the unload, on-disk replacement, and activation
sequence; the replacement must initialize every already-open project and synchronously
reattach each eligible open document before the installer reports success. The
installer must verify the loaded version, those live replica receipts, and the
final package bytes. It
may replace the directory directly only when every discovered IDE reports that
it does not own that plugin root. Attach or dynamic-unload failure is fail-closed
and must not be papered over by unlinking the JAR behind the live process.
This package lifecycle is independent of the native-library handoff.

### Claim — Split Position Detection

Two strategies for detecting the file's position in the editor split:

1. **Splitter tree walk:** Get the `EDITOR` component from action context, walk the Swing `Splitter` tree to determine if it's in the first child (left/top) or second child (right/bottom).
2. **Window index fallback:** If no EDITOR context (e.g., context menu), enumerate `FileEditorManagerEx.windows`, find which window contains the file, determine position from the Splitter tree or use window index + orientation as heuristic.

On a cross-session claim reject, the first recovery choice is **New Pane in This Session**. The plugin invokes `agent-doc claim <file> --new-pane` without `--position` or `--force`; all allocation/session decisions remain binary-owned. The existing Force Claim and Switch Project Session choices remain explicit destructive/migration alternatives.

### Prompt Panel

- Rendered as a `JLayeredPane` overlay at `POPUP_LAYER` — no `JDialog` (avoids WM leaks and focus-loss dismissal).
- Uses IDE editor font via `EditorColorsManager`.
- Single-row, non-wrapping layout. The panel height is fixed to one prompt row and does not grow on narrow windows.
- Question and option labels truncate with ellipsis; full text is preserved in tooltips.
- Secondary detail (hotkeys, pending-count context) lives in tooltip/secondary UI instead of the main prompt row.

### Tab Sync Listener

- “Show Tab in New Window” is a first-class editor surface, not another project. The listener resolves each `EditorWindow` through its AWT frame to JetBrains' persisted `ToolWindowPane.paneId`, reports only the windows owned by that editor surface, and includes that stable `surface_id` in structural and focus-only observations. The controller retains projections by `(project_root, client_id, surface_id)`. A closed detached frame is retired by exact surface id; it must not retire sibling frames or the whole plugin generation. Mouse/focus and structural AWT ingress include every live editor surface, not only the main `EditorsSplitters` tree.
- The IDE-hosted agent terminal uses a dedicated `Agent Doc Terminal` tool window so relocation never moves unrelated stock Terminal tabs. After the exact controller focus receipt applies, the controller's `terminal_decision` may mount the dedicated tool window in one `ToolWindowPane`, keep it there, exclude another frame, or stash it. Internal pane-placement APIs are capability-probed; absence leaves terminal presentation untouched. Plugin/project disposal unregisters the dedicated tool window and clears retained placement.

- Registered once per IntelliJ project from `PluginLifecycleListener`; `plugin.xml`
  owns only the project lifecycle listener.
- Returning to an IntelliJ frame through an application-activation event is a
  visible-surface source edge. Window-manager workspace changes can reconstruct
  or resize the terminal tool window without emitting editor selection or split
  events, so activation republishes the bounded-settled surface and lets the
  controller repair tmux automatically.
- Split orientation detected by walking the Swing component tree for `Splitter` nodes.
- Selection settling reads visible membership from each restored editor window rather than
  `FileEditorManager.selectedFiles`, whose aggregate can lag behind the individual split
  selections during restart. The restored window selections and detected split columns must
  contain the event's new document before publishing. If one advances before the other,
  bounded later-EDT reads continue; on exhaustion, the exact event old→new edge repairs each
  stale projection independently, so current window membership cannot mask stale one-column
  geometry.
- Agent-document `fileOpened` is also a visible-surface source edge. IDEA may restore split
  containers before their files and then emit neither a selection nor another
  structural event, so the listener retains the opened file as projection intent
  across bounded later-EDT reads. A surface is not published while any restored
  editor window still has no selected file; the completed file lifecycle edge
  republishes the settled surface without invoking tmux directly.
- A Remote Dev selection can arrive before its visible-editor membership update.
  That selection and every later flat visible/selected/open observation update focus/liveness
  evidence only and preserve the last authoritative columns exactly; they never choose a
  replacement slot or change membership, even after indefinite repetition. Only a complete
  positive vector from a geometry-capable frontend may change structure (GH #250, GH #253,
  GH #343).
- In the exact-262/263 modular package, an accepted frontend surface snapshot is the
  authoritative Remote Dev selection/layout source. A one-sided backend install has no supported
  IntelliJ API for observing client-scoped editor selection, so it fails closed with the explicit
  two-sided-install remedy instead of linking Marketplace-prohibited client-session internals.
  A connected remote frontend without an accepted controller publication remains the pane-layout
  authority fence (the backend never guesses geometry), but exposes readiness as blocked with
  `frontend_connected_without_accepted_publication`. The backend routes the complete snapshot to
  the controller root resolved from every classified session-document path, never to an unrelated
  IDE `Project.basePath`; delivery failure therefore names a real controller boundary rather than
  silently disabling frontend publication.
  An empty backend `EditorWindow` collection in this topology is
  unknown liveness evidence, not an authoritative empty surface set: publishing
  `surface-unavailable` must not immediately retire that endpoint (GH #260).
- An exact `(client, surface)` `editor_surface_forget` is terminal for the plugin's
  process-lifetime connection generation: the controller fences every later observation of that
  endpoint (`controller_editor_surface_observed ... accepted=false rejected=retired_endpoint`)
  until a newer plugin generation or a fresh controller. The listener therefore retires an
  endpoint only when it is really gone (closed frame, superseded controller root, shutdown, or an
  authority handoff). A delivery that returns from the controller after a newer observation was
  admitted keeps and records its endpoint: the newer observation republishes the same endpoint at a
  higher sequence. Only a whole-projection retirement (`retireControllerProjection`, the frontend
  authority handoff) advances the listener's authority epoch, under the same lock that drains the
  recorded endpoints, so an in-flight delivery either is drained by the handoff or retires its own
  endpoint afterwards (`#tmuxautosyncretired`). Before this rule, a quick tab switch while the
  previous delivery was still in the controller retired the live frame endpoint and muted every
  later automatic tab-switch sync until a plugin reload or controller recycle, while manual Sync
  and Run kept working.
- A live frame recovers from a retired endpoint in one round trip (`#surfaceretireformal`, model
  `formal/tla/PassiveTmuxSyncRetireNet.tla`). `CpRouteClient` keeps each lane's connection
  generation in an `AtomicLong` (seeded from `System.nanoTime()` per process). When an
  `editor_surface_observe` receipt is the typed rejection `rejected = "retired_endpoint"` with
  `reregister_above_generation = G` and the caller's `mayReregister` says the frame is still its
  own, the transport moves the lane to `max(current, G + 1)` (`SurfaceEndpointReregistration`,
  idempotent and monotone across racing receipts) and re-sends the same observation once with a
  fresh sequence; forgets use the current generation. `mayReregister` defaults to `false`;
  `EditorTabSyncListener` passes `projectionLive` (not closed, and not between
  `retireControllerProjection` and `resumeControllerProjection`) for both the surface and focus
  lanes. If a handoff retires the projection while the re-send is in flight, the transport retires
  the re-registered endpoint at its new generation. A frame the listener does not own keeps the
  fence.
- Marketplace artifacts must not statically link IntelliJ internal or override-only APIs. Gradle
  treats both Plugin Verifier categories as hard failures for classic, exact-262, and exact-263
  packages; artifact verification also rejects the private imports named by Marketplace review.
- Every agent-document selection and visible-layout change reports one editor-surface observation containing the focused document, visible documents, open documents, and detected columns. Session-document classification reads the live editor buffer and requires agent-doc frontmatter; a plain Markdown plan is never a desired tmux target. When a non-session tab is selected, that split retains its last classified agent document and the event republishes spanning layout only. Layout, file-open, and IDE-activation observations set `preserve_focus`: they may reconcile pane membership but cannot derive `Focus`, update controller focus tracking, or attach a focus target to structural sync. This keeps a background project/root publication from overwriting a newer explicit focus projection in a mixed-root split surface. An agent-document `selectionChanged` event carries pane-focus authority only when its file is selected in JetBrains' active editor window; background-split selection remains a layout-only observation and omits the event file as preferred focus. The selection callback captures only this typed authority plus its event file and schedules projection; a later EDT pass snapshots immutable editor state, while project-root discovery, filesystem access, patch-watch registration, JSON construction, native transport, and controller delivery run on a serialized background executor. Native transport adds an ordered reload generation/cursor and publishes it to an existing controller. The controller's process-scoped reactive graph derives idle or layout reconciliation and owns the tmux effect. One spanning editor surface has exactly one active controller-root subscription. Candidate roots are recovered from all open session documents on every publication; before admitting the replacement observation, the listener requests same-family retirement for incompatible roots. The controller stops every retained generation of the requester's own client family no newer than the requester, so a prior JVM cannot contract the tmux surface while the replacement controller is still reconciling, while a delayed old-JVM retirement cannot stop the replacement. A family names one IDE installation, not the JetBrains brand: the surface lane is `jetbrains@<productCode>.<config-dir hash>-pid:<pid>` (the IntelliJ Platform admits one running instance per config directory), so a restart of the same IDE retires its predecessor while a concurrently running second IDE on the same project (IntelliJ IDEA and Rider, or a stable and an EAP install) keeps its own subscription when the other IDE retires a root on a cross-root focus move or shutdown (`#idearidereditors`). A legacy unscoped `jetbrains-pid:` observation from a pre-`#idearidereditors` plugin is retired by any scoped family on the same lane. A failed pre-publication forget stays tracked and is retried after publication and on the next observation. Active selection and component-focus also publish one generation-fenced `focus_only` surface Source to the focused agent document's own controller root under a distinct `jetbrains-focus@<installation>-pid:<pid>` family. The controller graph derives `Focus` and must never infer structural `Sync` from that deliberately narrow payload. When its exact receipt reports a stashed, dead, reaped, missing, or non-focusable owner, the adapter generation-fences the receipt and republishes the complete spanning surface as a forced focus-authoritative structural edge; only that layout graph may restore the owner and reconcile pane cardinality. Component-focus also republishes the spanning surface directly. This separate retained handoff is required when one visible editor surface spans a superproject document and a submodule document. The plugin never chooses a pane or tmux window, and a missing-owner outcome installs no reverse-focus suppression lease.
- A manual Sync Tmux Layout or Load Tmux Window surface-reference fallback reuses the first pass's
  focused file and visible-document request. It captures only layout evidence on the EDT and sends
  the snapshot from that request; it never re-enters focus discovery under a Remote Dev backend
  client context.
- A Remote Dev backend capture classifies session documents from each `ClientFileEditorManager`
  directly and joins those client-scoped paths with backend-local open documents. An empty or stale
  backend `FileEditorManager.openFiles` set must not collapse a visible client surface to
  `no_visible_session_documents`.
- Project-internal paths containing a `.agent-doc` path component are never session documents,
  even when their text contains copied `agent_doc_*` frontmatter or component markers. The plugin
  rejects them before its attached-replica fast path and before `forwarderFor`; the controller
  independently rejects them before consulting any existing relay authority. Thus an
  editor-restored legacy snapshot tab cannot acquire, retain, or be served by a document hub.
- Concurrent JetBrains IDEs on one project (`#idearidereditors`), e.g. IntelliJ IDEA and Rider,
  are independent editor installations. Each registers its own PID-scoped CRDT replica per open
  document, so text edits, agent responses, and turn-state presentation converge in both buffers.
  Navigation is not independent yet: both IDEs publish surface observations to the same Project
  Controller root, which owns one tmux `agent-doc` window per project. The controller already keys
  layout ownership and the `tmux_focus_state` projection by navigation lane
  (`#ideriderindependentnav`); in the default `shared` mode every client runs on the shared lane
  (per-installation lanes are below). Only the installation that
  most recently gained focus drives that window (`#idefocusowner`). The focus lane
  (`jetbrains-focus@<installation>-pid:<pid>`, `focus_only`) publishes only while that IDE's
  project frame is the OS-active window, so an accepted focus-lane fact is the focus-gain event;
  spanning layout observations never claim, because their selection flags can come from an
  unfocused IDE (for example the reverse tmux-to-editor mirror, `focusEditor=false`). The
  controller folds claims into a per-root `InstallationFocusOwner` (a `Source` of the pure
  `InstallationFocus` fold with a `Computed` owner in the controller `ProcessScope`) and gives every
  surface graph a `Computed` `SurfaceAuthority` projection of it. A non-owner's observations are
  accepted and keep its facts current, but fold to `Idle` and return `deferred_to=<owner
  installation>` in the receipt (and `deferred_to_focus_owner=` in ops.log); a retained failed-effect
  retry is not replayed for it, and a tmux refold cannot make it drive. Ownership moves only on a
  claim by a different installation, each move starting a new ownership epoch; the first
  observation a surface folds under a new epoch re-projects its whole layout unless tmux is
  observed to already match it. When the owner's last surface on a root retires (shutdown, or a
  cross-root move of both lanes) ownership falls back to the most recently focused live
  installation under a fresh epoch. Unscoped clients (`jetbrains-pid:`, VS Code) neither claim nor
  defer, and before any claim every installation drives as before. Known limits: an IDE activated
  while a non-session file is selected publishes no focus-lane fact and does not claim until an
  agent document gains focus there; a focus-lane fact claims only on the focused document's own
  controller root.
- Per-installation navigation lanes (`#ideriderindependentnav` P3, opt-in with
  `editor_navigation = "per_installation"`): the plugin names its surface client
  (`jetbrains@<installation>-pid:<pid>`) everywhere the controller derives a navigation lane from
  it. `tmux_focus_state` sends it as `caller`; every `pane_layout_desired` publication (including
  the manual Sync Tmux Layout) carries it as `client_id`; and the IDE terminal ensure passes
  `agent-doc tmux ensure <file> --json --ide-terminal --client-id <id>`, so this IDE's agent-doc
  terminal attaches to its own lane session. `TmuxPaneFocusSync` mirrors a tmux focus projection
  only when it has no `navigation_lane` (the shared lane) or names this installation; a
  projection of another installation's lane is never mirrored (defense in depth for "focus
  mirroring goes only to the IDE that owns the window"). In `shared` mode the controller maps the
  same client id to the shared lane, so nothing changes there.
- Explicit lane handoff (`#ideriderindependentnav` P4): the layout publication of Sync Tmux Layout,
  and the one Claim for Tmux Pane, Force Claim, and Load Tmux Window run after `agent-doc claim`,
  carries `caller_kind = "manual"`; the controller treats it as an explicit claim and moves a
  contended document's pane from another IDE's lane into this IDE's lane before arranging it. Passive
  publications (`automatic` heartbeats, `resync`) never move a pane. No plugin change was needed:
  the plugin already sent `caller_kind` and `client_id` on every publication.
- New panes start in the requesting IDE's lane (`#ideriderindependentnav` P6): the Run Agent Doc
  `editor_route` payload (classic and command-plane) carries `client_id` = this IDE's surface
  client id, so in `per_installation` mode the controller arranges this IDE's lane for the route
  and a pane the route starts lives in this IDE's lane session. Claim for Tmux Pane, its New Pane
  recovery, and Force Claim run `agent-doc claim ... --client-id <id>`, so a pane the claim
  provisions starts in this IDE's lane and a pane already in this IDE's lane passes the claim's
  cross-session check. An existing pane is used where it lives; in `shared` mode the client id
  changes nothing. The plugin and binary must ship together: an older binary rejects
  `claim --client-id` (an older controller ignores the payload field).
- Per-installation lanes are available (`#ideriderindependentnav` P5): switching
  `editor_navigation` either way needs no plugin action. Enabling moves no pane; after switching
  back to `shared`, `agent-doc gc` folds every lane's document panes into the shared session and
  closes the lane sessions. A closed lane is provisioned again by this IDE's next terminal attach
  or layout sync, and gc never closes a lane session a terminal is still attached to. The project
  controller runs the same reconcile itself (`#ridernavp6gc`) when an IDE's last surface retires
  (the plugin's existing surface forget / client-family retirement) and at its start under
  `shared`, so no plugin action and usually no `gc` is needed.
- An observation with no detected columns is structurally `unknown`, not a one-column layout.
  It may select the focused document's existing pane, but it cannot add, stash, reorder, or
  provision panes. A proven local single editor window publishes one column. The presence of
  Remote Dev client editor-manager sessions selects the remote evidence path even when the
  backend happens to expose an incidental local window (for example a source/Find Usages window);
  backend window count is not remote-layout authority. Remote client-selected files name only the
  focused document, so the plugin reports per-client `visible` (the Remote Dev editor tracker's active client editors),
  `selected`, and `open` session documents to the shared native fold (GH #134): the
  `remote_layout` compatibility resolver, or `agent_doc_editor_surface_resolve_remote_layout_json` on a
  library that predates it (GH #326 phase 4). The plugin snapshots those
  client/editor collections and the backend-focused files on the EDT, then runs the resolver from the
  generation-fenced surface delivery worker; direct synchronous detection also marshals only the
  immutable observation off the EDT. The native bridge's EDT guard therefore remains intact.
  The plugin keeps no memoryless Kotlin fallback: with no native answer the layout is unknown.
  Remote client `visible`, `selected`, and `open` lists are flat focus/liveness evidence, not split
  geometry. They cannot establish, add, remove, replace, or reorder columns, regardless of how
  often they repeat or whether one list changes. Every successful `[layout-detect] observed` line
  records those raw lists for diagnosis. Without an exact local or connected-frontend column
  vector, the observation stays structurally `unknown`. The controller's coalesced ingress receipt joins
  `layout=unknown|observed|focus_only` to `pane_action=none|focus_only|structural_sync` on one
  line so every pane effect names its authority.
- Remote Dev backend observations never reconstruct a split from retained slots or flat client
  lists (GH #185, GH #234, GH #343). The legacy `remote_layout` resolver owns no production
  structural memory and always returns no columns. Separately, the controller retains the last
  exact vector delivered by the modular frontend lane. Focus/liveness evidence may focus a
  document already present in that controller target, but an uncovered document defers; it cannot
  replace a column. Until an exact vector arrives, the backend performs no structural tmux effect.
  Between exact vectors the controller preserves the last accepted target byte-for-byte.
- When the frontend module is connected, its generation-fenced measured layout is the source
  of truth. The backend forwards that exact vector as `column_order=editor`; the controller
  copies it into the desired tmux effect without folding, reordering, or reconstructing it.
  Conflicting retained backend state is stale. Incomplete frontend evidence preserves the last
  accepted frontend snapshot or defers, and focus-only escalation cannot invent a column.
  Backend tmux observation is solely a convergence receipt.
- Explicit layout publications (`Sync Tmux Pane`, claim, resync, and the Run Agent Doc route's
  `--col` list) name their order source as `column_order` (GH #112). By surface reference the
  label is the surface's own `column_order`, carried by the controller. Only locally detected
  multi-column geometry is `editor`; the undetected fallback lists `selectedFiles`, which IntelliJ
  orders focused-window first, so it is `unknown`. The controller resolves an `unknown` order
  against the order it already retains for those documents (`agent_doc_tmux::order_layout_columns`)
  only outside an active connected-frontend authority lease. During that lease, an `unknown`
  publication cannot mutate the exact frontend target. On a Remote Dev backend whose detector
  stays `unknown` (GH #97), the backend therefore performs no structural tmux effect before the
  first frontend snapshot and preserves the last exact frontend target between snapshots. A cold
  `unknown` publication is not a fold basis, does not become order authority, and cannot anchor a
  restarted editor reconstruction (GH #332).
  `pane_layout_desired_published` and every
  `tmux_layout_effect_state` line record `caller_kind` and
  `column_order=editor|retained|unknown`.
- The automatic editor surface is state, and it names its column-order provenance the same way
  (GH #320; the rule is `specs/13-state-backbone.md`, "State Propagation, Not Effect Inference").
  The surface payload's `column_order` is `editor` only for local windows laid out at distinct
  origins (and for the JetBrains Client's measured windows forwarded by the 262/263 backend,
  below); windows that share an origin (never measured) publish `unknown`. Legacy Remote Dev
  fallback observations carry only focus/liveness evidence and publish no structural replacement.
  The modular `RemoteClientLayoutForwarder` separately carries the exact `editor` vector directly
  into the controller's desired tmux effect; `unknown` or selection-only input cannot be converted
  into geometry. While frontend authority is active, tmux drift checks compare against the last
  exact frontend vector. On a Remote Dev backend the diagnostic capture reads the remote clients'
  `visible`/`selected` session documents (mapped to the open session documents' absolute paths)
  together with the backend's own selection; it never defers with
  `no_visible_session_documents` because the backend's `selectedFiles` names no session document
  (`activeWindow=<none>`); those flat lists remain non-structural until the frontend publishes an
  exact vector.
- **Remote Dev pane structure is authored by the connected JetBrains Client (GH #320/#343).** The
  backend has no client split structure to observe: `GuestFileEditorManager` (the
  backend's `ClientFileEditorManager` for a Remote Dev client) holds a flat composite list and one
  selected composite, the RD `FileEditorCompositeModel` carries a per-composite `visible` flag but
  no window or split identity, and `RdServerEditorTracker.activeEditors` is a flat list. agent-doc
  does not infer order from the editor remove/re-open events. Geometry exists only in the JetBrains
  Client. Snapshot schema 2 carries measured bounds; schema 3 additionally carries the frontend's
  typed `layoutColumns` compatibility vector, frozen at the 8 px x-tolerance and top-to-bottom
  stacking rule. Schema 4 carries `focusedEditorPath`, which is present only when the permanent
  AWT focus owner belongs to that exact editor window in the active frontend frame. The backend
  sends the window evidence to the hot-reloaded native
  `client_measured_layout` derivation before consulting that floor, so the connected frontend
  remains the source of structural evidence without making Kotlin the live grouping authority.
  After the same snapshot's
  `editor_view_snapshot_observe` was delivered (whatever the detached-view projection status: 263
  freezes it without terminal capability), it forwards
  them into the controller surface Source (`editor_surface_observe`) as `column_order=editor`.
  A schema-4 focused-editor proof publishes that document with `preserve_focus=false`, so moving
  from the left editor split to the right selects the corresponding tmux pane (GH #368). Without
  that proof—including older schemas and focus in an embedded terminal—the forward remains
  `preserve_focus=true`; selected tabs alone are never promoted to focus authority. The forward
  publishes on its own family,
  `jetbrains-rd@<installation>-pid:<pid>`: the same installation as the classic surface and focus
  lanes (so the same navigation lane and focus owner), with its own generation, because the
  frontend authority handoff retires the whole classic `jetbrains@` family. Authority begins when
  an authenticated ingress lease whose client id resolves to a remote `ClientType` opens, before
  its first snapshot: the frontend intentionally
  waits 75 ms to coalesce event bursts, then captures on the EDT; first-lease schema negotiation
  may wait up to 5 s, and transport may fail. Throughout that pending window the backend fallback
  is fenced and may only leave the last accepted frontend structure in force. It cannot invent,
  fold, or publish a replacement. Each frontend
  connection publishes its own endpoint `remote-client:<clientId>:<connectionGeneration>`, retired
  when that connection retires or the backend project closes; before this JVM's first forward to
  a root it retires the installation's `jetbrains-rd` family there, so a dead backend JVM's forward
  cannot keep driving. Only a remote session (`ClientType.isRemote`) acquires that authority and is
  forwarded. A monolithic IDE's own authenticated frontend remains valid detached-view transport,
  but it leaves `layoutAuthorityActive=false`, does not retire the classic
  `EditorTabSyncListener` projection, and does not open a `jetbrains-rd` layout endpoint: its
  windows are already measured by the classic path. That classic path must also stay
  *subscribed*: the modular backend compiles a generated `PluginLifecycleListener`, and the
  generator keeps the classic block that subscribes `EditorTabSyncListener` to the project
  `FileEditorManager` topic, installs `EditorFocusSyncListener`, and republishes on IDE
  activation, appending the one-sided Remote Dev fallback diagnostic after it. The generator fails
  the build if any of those edges is missing. Stripping them left a local 263 IDE with no
  tab-selection publisher at all: tmux followed a tab switch only at the next native-library
  reload (each `make install`), Run Agent Doc, or editor container change. This classification gates the authority
  transition itself, not merely the later delivery; otherwise the local lease can retire the
  classic publisher before the forwarder rejects it as `local_client`, leaving automatic tmux sync
  with no source. A schema-1 peer has no
  geometry and a schema-2 peer uses its positive bounds only for compatibility. For schema 3,
  a missing column vector is `Unavailable` only when no native generation can answer; a native
  answer is authoritative and the last accepted frontend snapshot stays in force when it reports
  unavailable.
  The same hold applies to a split whose selected tab is not a session document
  while its tabs hold one (`#stickymdpane`: that column is unknown, not gone); a split holding no
  session document contributes no column, so closing a split still collapses the layout. While a
  authenticated remote layout-authority lease is live for a project
  (`RemoteClientLayoutAuthority`), the backend's remote-client
  membership fold defers (`capture deferred reason=frontend_measured_layout_authority`); a Run
  Agent Doc whose surface reference is unavailable for that reason sends its `--col` fallback with
  `layout_authority = "frontend_measured"`, so the controller takes or awaits the measured forward
  instead of failing on an empty fallback (GH #376). The
  remembered-order fallback, labelled `unknown`/`retained`, remains only for classic installs and
  one-sided installs with no authenticated frontend lease. Schema negotiation keeps old peers
  working: Fleet RPC decodes with `ignoreUnknownKeys = false` and `encodeDefaults = true`, so
  versioned optional keys are never encoded while null (`@EncodeDefault(NEVER)`), and the frontend asks
  `SurfaceSnapshotRpcApi.supportedSnapshotSchemaVersion()` once per lease; a backend that predates
  the call fails it (or does not answer within 5 s) and the frontend publishes schema 1 without
  geometry. The backend validator accepts schemas 1 through 4. Explicit retirement/release is the
  only transition that hands authority back to the backend fallback. Evidence: the client logs
  `[split-surface] snapshot schema negotiated version=<n> backend_offered=<n|none>
  geometry=measured|unavailable`; the backend logs `[rd-layout] forwarded measured columns
  surface=<id> root=<root> columns=<n> [<left> | <right>] column_order=editor`,
  `[rd-layout] measured layout unavailable surface=<id> reason=<reason>`, and `[rd-layout] retired
  measured layout surface=<id>`; ops.log records `controller_editor_surface_observed
  client=jetbrains-rd@<installation>-pid:<pid> surface=remote-client:<clientId>:<generation>` and
  `pane_layout_desired_published ... column_order=editor`. `Sync Tmux Layout` recovers stale tmux
  structure only by publishing the current positive frontend column vector; an unknown or partial
  capture preserves/defer structure and may update focus intent only.
- **A modular version mismatch is a distribution state, not a layout input (GH #372).** JetBrains'
  paired-plugin state enforcer owns the pre-RPC transition: equal, loaded frontend/backend plugin
  versions may open the authenticated ingress lease; `VERSION_MISMATCH`, missing, or unloaded on
  either peer disables the inconsistent pair before Agent Doc can negotiate a snapshot schema.
  Agent Doc must not turn backend `visible`/`selected`/`open` lists into columns while that gate is
  closed. Its only fallback remains focus/liveness for a positively observed document, with
  structural routing blocked. Because `UpdateSettings` is process-local, the modular GitHub
  update-channel action is a frontend-module action: it mutates the JetBrains Client repository
  list, after which the operator installs the identical update on both peers and restarts both.
  The backend content module must not register that action. Classic IDEs keep the ordinary local
  action because their UI and plugin runtime are one process.
- **Run Agent Doc and Sync Tmux Layout act on the propagated surface, not a click-time snapshot**
  (`surface_layout_reference_v1`; the rule is `specs/13-state-backbone.md`, "State Propagation, Not
  Effect Inference"). On the EDT the command captures the surface through the same
  `EditorTabSyncListener` capture the automatic path uses; its background thread publishes that
  capture as an ordinary ordered `editor_surface_observe` fact of the listener's own `(client,
  surface)` endpoint, with the delivery's endpoint bookkeeping (recorded, or retired when an authority
  handoff overtook it). The observe reply lists the controller's `capabilities`. When it includes
  `surface_layout_reference_v1`, the command sends `surface_reference = { surface_id, min_sequence }`
  (the sequence it just published) and **no** `--col`, `column_order`, `layout_mode`, or sync
  `columns`/`focus` of its own. The controller reads its retained surface Source for the command's
  `client_id` and derives the layout from it: a sync uses the surface's columns in the automatic
  `SurfaceIntent::Sync` shape with the surface's own `column_order` and focus; a route uses an
  `exact` publication of a multi-column split, or `ensure` with one column per visible document
  (order `unknown`) otherwise. A newer observation from the same client is newer state and is used
  as is. The plugin keeps the `--col` snapshot only as a logged rollout fallback
  (`[route]`/`[sync] surface reference unavailable reason=<reason>; sending the --col snapshot`):
  a controller whose reply lacks the capability (an older binary), a rejected or failed publication,
  a cross-root command (the surface Source lives in another controller), a listener whose projection
  a Remote Dev frontend surface took over, a root this listener's projection is not active on, or a
  capture the editor deferred. The CLI's `agent-doc sync --col ...` is unchanged and stays an
  explicit operator `editor` publication.
- Manual `Sync Tmux Layout` publishes `exact_visible=true` only when layout detection returns
  known columns. By surface reference the controller makes the two refusals below when it
  resolves the reference (`surface_reference_unknown_layout`, `surface_reference_stale_focus`),
  and the plugin shows the same sentences; on the snapshot fallback the plugin makes them itself. If columns are unreadable, a local split refuses before terminal preparation or controller
  publication and tells the operator that the retained tmux layout was not changed; a focused-only
  fallback must never narrow a retained multi-column surface to one column (GH #154). Remote Dev
  flat evidence (`EditorLayoutEvidence.Remote`) on a manual sync does not show that refusal up
  front (GH #382). After terminal preparation and the native-generation handoff it submits one
  unknown-order column containing only the focused document, with
  `unstarted_session_bootstrap=true`, and `layout_authority=frontend_measured` only while
  `RemoteClientLayoutAuthority` is active. The controller keeps the unknown-layout refusal when
  that document already has a started agent session. A `no_autostart` sync still refuses locally. The detected
  columns and focused document are one snapshot: if the detector's columns omit the focused file,
  the plugin refuses the stale/interstitial snapshot before controller submission and asks the
  operator to retry after selection settles.
- A fresh editor selection is authoritative for the next projection even if the split component tree is briefly interstitial and still exposes the opposite editor as selected. When a tab-chrome click initially looks like a background-split selection, the listener performs one generation-fenced authority check after IntelliJ's focus manager reports settled focus; it emits focus only if that exact file has become the active editor window while the project frame is active. A generic next-EDT check is insufficient because real split/tab transfers can span multiple EDT turns. This closes the opposite-tab event gap without promoting programmatic or genuinely background selections. Component-focus republishes the spanning surface when no selection is pending, but cannot replace a pending document-selection observation; the shared generation guard still collapses repeated focus callbacks.
- The listener first re-reads an interstitial selection snapshot on bounded later EDT turns. If it still contains the old selected file when that budget is exhausted, the listener preserves the last authoritative visible/layout snapshot and publishes only focus intent; it never synthesizes structural placement.
- Reverse tmux-to-editor focus sync is installed at project startup and reads the Project Controller-owned `tmux_focus_state` projection through the Project Controller socket. That projection yields a document only when the configured tmux session's current window is `agent-doc`, so switching to another tmux window must not recall the editor selection from the stale active pane in the hidden agent-doc window. If the active pane still has an exact route-owned process-tree binding but its actor projection was pruned, the controller reports that bound document instead of `active_pane_unbound`; foreign-root and ambiguous owners remain unbound. An actually focused editor component remains authoritative over background controller focus changes. The IDE frame alone is not focus authority: selecting a pane in an embedded terminal leaves the frame active but permits the corresponding visible editor document to be selected, including a document from another project root in the same split surface. A hidden foreign-root target remains suppressed. A reverse mirror may select a different editor tab but must pass `focusEditor=false`, so it cannot reactivate the JetBrains desktop window after the operator moves to another i3 window. Before `Claim for Tmux Pane` starts a CLI claim, JetBrains records the current tmux-focused document as already seen; if the claim fails and tmux focus remains on the previous pane, the reverse focus mirror must not reopen that previous document over the editor-selected claim target.
- The plugin applies only a short event-storm debounce and generation guard before reporting the final surface. Its separate focus lane keeps only an in-memory generation and a micro-coalescing delay, then publishes retained focus state to the Project Controller; it does not submit an editor command. The controller-owned Lazily graph supplies transition deduplication and the exact selection effect receipt. The plugin stores no previous layout signature, last-focused file, pending retry, or durable controller copy.
- A `sync` effect target is the product of columns and focused document. The controller first reconciles the passive layout, then applies the requested pane through a generation-fenced effect. Matching columns or a successful `select-pane` receipt alone cannot retire the target: observation must show the focused document's actor pane active in the target window. A repeated foreground command republishes a physical observation even when its desired value is identical, reactivating the retained effect after focus or geometry drift. A newer surface generation cancels stale focus before it reaches tmux.
- Safe-passive exact-visible reconciliation is atomic across stale-supervisor admission. If any requested editor column is gated while its pane recycles, automatic sync preserves the current tmux layout instead of realizing a narrower subset; the recycle-settled generation republishes the complete surface. Manual `Sync Tmux Layout` keeps full repair authority and may realize a non-empty admitted remainder.
- The retained focus projection may surface a proven live pane from stash inside
  the controller's latest-wins focus fence, then selects only after a live-window
  recheck. The listener models that controller call as a bounded request/response
  effect: the request carries the focus generation admitted with its spanning
  surface observation, and the response is interpreted under the lifecycle lock.
  Any newer admitted surface observation invalidates and interrupts the in-flight
  request. The socket has a one-second deadline as a transport backstop, so a slow
  controller cannot block newer focus work. Only a current successful response may
  install a focus lease or, for `actor_pane_not_visible`, republish the complete
  editor surface with forced reconciliation. Stale, timed-out, and failed responses
  are inert. The full surface remains exact-layout authority; other focus failures
  do not trigger layout repair. The controller classifies
  `actor_pane_not_visible` from the selected pane's own window before applying the
  active-window guard: a stashed selected pane therefore requests layout repair,
  while a pane already in the `agent-doc` window still reports
  `outside_agent_doc_window` when the operator is viewing another tmux window.
- If a Project Controller-backed manual `Sync Tmux Layout` terminal outcome later reports that the current layout was preserved because a visible protected pane could not detach yet, the command receipt/log must retain the protected pane id, open-cycle phase, and document path so the user can tell which pane is delaying sync. Current controller builds should attach/focus the requested document around the protected pane instead of emitting that deferred-sync marker.
- Automatic layout sync completes at desired-state publication rather than waiting for that exact plane version to become observed. The controller owns a single latest-wins worker, interrupts obsolete retry waits when a newer generation arrives, and never reports a superseded automatic version as a user-visible failure. Manual sync keeps its terminal receipt boundary.
- **Resync / Fix Sessions** first runs registry/liveness cleanup, which must not
  promote registered stash panes. On successful cleanup the same operator
  action captures the current editor columns on the EDT and publishes one
  exact-visible desired state with `caller_kind=resync` and
  `no_autostart=true`. The distinct caller kind forces a new retained
  reconciliation generation even when the documents match the last automatic
  surface. Repeated resyncs preserve editor pane cardinality; they never derive
  visible panes by enumerating the session registry.
- Manual `Sync Tmux Layout` submits `agent-doc.sync_tmux_layout.v1` to the Project Controller command plane
  with `no_autostart=false` and waits for the terminal command receipt. The controller runs the full sync
  path and repairs window order before reconciliation: `0:agent-doc`, `1:stash`, then adjacent
  overflow `N:stash` windows. When the requested document has no pane, that terminal boundary includes
  pane creation in the resolved tmux session, registration, harness readiness, and document-route submission;
  any failure is shown with the controller diagnostic. Its nested deadlines are strictly ordered:
  the pane effect owns 30 seconds, the controller waits 35 seconds for the effect's terminal projection,
  and the plugin waits 40 seconds for the controller command. Thus a still-current `Applying` projection
  cannot win a same-deadline race and surface as a terminal `tmux_effect_in_flight` error. The
  manual action uses the same live-buffer session classification and ignores non-agent Markdown tabs;
  if the focused split currently shows one, its last classified agent document
  remains the requested member. Automatic editor-surface
  sync publishes the same autostart-capable desired layout with
  `caller_kind=automatic`, but completes at desired-state admission; the binary
  owns safe cold-start checks, ambiguity refusal, and inactive-desktop focus
  suppression. Visible restored files may therefore acquire missing panes
  without an imperative republish, while merely open background files remain
  outside the desired columns.
- A native-library generation handoff has one shared five-second CRDT-worker
  shutdown deadline across all open projects. Operator-triggered Compact
  Exchange and manual layout sync wait on the handoff completion edge instead
  of observing the intentional manager/listener removal window. After endpoint
  restart, the coordinator republishes every open project's current editor
  surface so tab/focus-to-tmux synchronization resumes without another click.
- If passive `agent-doc sync --no-autostart ...` output from an older build reports that it preserved the current layout because a visible protected pane could not detach yet, JetBrains must treat that as deferred rather than complete for both the generic `[sync] sync preserved...` marker and the safe-passive `[sync] safe passive sync preserved...` marker: leave dedup state unchanged and schedule bounded retries until the requested selection applies or a newer request supersedes it.
- If a passive sync terminal outcome contains `[sync] safe_passive_sync_lock_contention_retry`, JetBrains must treat the command as deferred, keep the dedup state unchanged, and retry the newest pending automatic selection/layout request rather than waiting for the CLI's full sync-lock budget. Manual `Sync Tmux Layout` uses Project Controller command supersede/admission instead of a long-lived editor-side native sync guard.
- JetBrains split-editor focus follows editor focus-gained events, editor-content mouse activation,
  and mouse presses anywhere in the owning project's editor-split component tree. The last source
  includes tab chrome and preview/custom editors, which can change the active editor window without
  emitting either of the narrower callbacks. An editor-tree press schedules a latest-wins next-EDT
  probe and publishes only the then-current active window's selected session document; presses
  outside the project tree, callbacks after disposal, superseded callbacks, and callbacks while the
  project frame is inactive are inert. The process-wide AWT listener is paired with project/plugin
  disposal. Consecutive events for the same markdown path are deduped locally, but alternating paths
  such as A -> B -> A must each attempt the Project Controller focus handoff.
- Installing split-editor focus sync must not require the editor split tree to exist yet. On
  IntelliJ 2026.3 (263) `projectOpened` can run before the platform assigns
  `FileEditorManagerImpl.mainSplitters`, and `getSplitters()` then throws
  `UninitializedPropertyAccessException`. The install treats that exception, and
  only that one, as "not built yet": the editor focus and editor-mouse listeners attach
  immediately, and the editor-tree listener plus its seed of the already-focused split wait for
  the project's first `FileEditorManager` `fileOpened` or `selectionChanged` event, attach once,
  and then drop that subscription. Every event-time read of the split tree uses the same guard.
  A focus-sync install that still fails is disposed, logged at error level, and left unrecorded so
  a later install can retry; it must never abort the remaining `projectOpened` startup steps.
  Platform cancellation still propagates.
- Local operator splices are retained immediately but published after a 250 ms trailing quiet window, coalescing human typing bursts into one ordered durable push, broadcast, and settled projection while preserving the existing per-document retry fence.
- The structural layout-change detector only reports a new editor surface observation into `EditorTabSyncListener`; it has no second CLI planner, lock, or tmux process. The JetBrains/native boundary validates and enqueues that observation without waiting for a controller probe or tmux consequence. One delivery worker serializes publications and controller-root handoff off the EDT; each controller's process-scoped graph replaces any not-yet-started surface with the newest one. Tab selection and structural changes therefore share one latest-wins surface graph, while component focus uses only the separate selection lane, without letting a blocked controller call freeze or disable the IDE bridge.
- A failed editor-surface publication remains generation-fenced retry work. The
  listener retries the latest observation with capped exponential backoff (100,
  200, 400, 800, 1600, then 2000 ms) so a controller recycle or brief socket
  refusal cannot permanently disable automatic tmux pane synchronization. A
  successful publication resets the retry counter, and a newer generation
  supersedes every older retry.
- JetBrains bounds every command-plane automatic and manual sync call. On timeout or failure, it releases the plugin-local request guard, logs the failure, and leaves automatic tab-sync dedup state unchanged so the latest queued selection or a manual retry can run again. A dead or externally killed tmux pane may delay one request, but it cannot leave a second editor-side sync owner wedged.

### Prompt Poller Removed

- JetBrains must not start a defensive `PromptPoller` / `PromptPanel`, poll `agent-doc prompt --all`, auto-save tracked documents, or run timer-based merge/reload logic from prompt handling.
- Permission prompts remain in the owning agent/tmux surface.
- Interactive Codex, Claude, and OpenCode prompts are detected by the supervisor and projected as
  controller-stream `input_required` state. JetBrains shows an important notification and editor
  banner with a `Focus Agent Terminal` action on the false-to-true transition; it does not poll or
  become a second answer authority.
- A queued Clear Session Context the supervisor later drops (a newer prompt started before the
  turn ended) arrives as controller-stream `deferred_clear_superseded`. `TurnStateBannerRefresher`
  shows one information notification per dropped `clear_epoch` ("Queued clear not sent") with a
  `Clear Session Context` action; `DeferredClearSupersededNotices` seeds its baseline from the first
  ready frame per document so reopen/reconnect never re-announces an old drop
  (`#clearsupersedenotice`).
- A Clear Session Context the binary queues behind a prompt that never started a turn
  (`session_clear queued for <FILE> — the last prompt in pane <pane> has not started a turn ...
  settles_in_secs=<n>`, `#clearctxslow`) is an accepted, queued outcome, not a failure. The
  information notice's first line says when the clear runs ("Clear Session Context queued for
  <path>: it runs automatically in about <n>s."; "is already queued" on a re-press) and the
  second says the last prompt never started a turn, that pressing again does not make it
  sooner, and that Interrupt and clear clears immediately. It never says a turn is running.
  A clear queued behind a live turn keeps the "it runs when the turn in pane <pane> ends" notice.
- A Clear Session Context the binary queues behind a Run Agent Doc that was accepted but has not
  started its turn (`session_clear queued for <FILE> — Run Agent Doc for pane <pane> was accepted
  <n>s ago and has not started its turn yet (... evidence=dispatch_turn_pending
  evidence_age_secs=<n> pre_turn_grace_secs=<g> ...)`, `#clearaftrun`) is likewise a queued
  outcome. The first line is "Clear Session Context queued for <path>: it runs after the Run Agent
  Doc accepted <n>s ago finishes." ("is already queued" on a re-press); the second says the run
  has not started its turn so nothing is running, that the run does not cancel the clear, when the
  clear runs if the run never starts (`<g> - <n>`), and that pressing again or Interrupt and clear
  does not make it sooner. It never says a turn is running and never offers Interrupt and clear as
  "clear now".

### Run Feedback

- The active-turn banner is derived from the Project Controller
  `state_subscribe` closeout payload, including `realtime_steering` kind/count
  and the full aggregate as hover text. The identity-keyed `elements` object is
  preserved verbatim from the Rust projection. Its `observed_content_hash` is
  the controller's canonical CRDT generation receipt; a receipt-only empty set
  renders no steering label. Kotlin must not re-read disk or derive steering.
- Exchange prompt-prefix normalization must remain response-aware. A Markdown
  blockquote under `### Re:` is quoted response context even when its complete
  text is present in the normalization target set; it cannot transition the
  parser into prompt mode or cause later response prose to acquire `❯`.
- Native visual-token projection classifies the complete binary-authored
  `> **Queue prompt:**` blockquote as `prompt`, including its continuation quote
  lines. JetBrains therefore applies prompt styling consistently to queue turns
  emitted by Codex, Claude, and OpenCode without harness-specific parsing.
- Native visual-token projection classifies the
  `[#subagent-operator-request-<unique-id>]` handback marker as `tracked_id`. JetBrains MUST
  render it with the same tracked-ID colors used in todo, backlog, queue,
  icebox, and review items.
- `Run Agent Doc` saves only the active markdown document and immediately dispatches a Project Controller `editor_route` request carrying dispatch-only, plain-trigger routing with a 15-second ready wait. The request's layout is a `surface_reference` the controller resolves into `--col` / `--focus` arguments from its retained surface Source (the `--col` snapshot only on the logged fallback; see "Run Agent Doc and Sync Tmux Layout act on the propagated surface"). That complete `--col` / `--focus` layout includes empty physical split placeholders and is an exact-visible layout boundary: the controller canonicalizes those facts, may materialize a missing focus only into one unique empty split, and converges the result before dispatch so route cannot append a pane beside stale visible state. When Remote Dev publishes flat focus evidence with `flat_evidence_has_no_column_geometry` and the controller has no retained complete split, this explicit action may instead use a positive live-tmux observation: focus the already-live target or replace the complete observed width with the route's complete named visible set at unchanged width. A retained route-owned unknown-order replacement remains dispatch evidence rather than editor structure, so each later explicit route may repeat that live survey and safe full-width replacement; controller restart is not required. The one-column case remains valid; a two-column named set may replace two stashed live slots. Partial replacement and widening stay forbidden. The replacement remains unknown-order dispatch evidence and cannot become editor layout authority. `Sync Tmux Panes` still requires measured columns to replace structure; after its flat publication is retained, the typed command decision succeeds by preserving the retained tmux layout without submitting a structural effect or surfacing `flat_evidence_has_no_column_geometry` as an operator failure. A manual Sync of a document with no started agent session is the GH #382 exception: Remote Dev flat evidence starts that one document, or uses the installation's measured columns when they show it, instead of the install-plugin refusal. When the frontend publisher stays unavailable, the backend warning and every route refusal caused by the missing geometry name `flat_evidence_has_no_column_geometry` and prescribe installing the same modular plugin ZIP in JetBrains Client/Gateway and the Remote Dev backend, restarting both, then retrying. The editor path does not block on the typing debounce and does not save unrelated open documents; the active document save is the editor-owned flush boundary before the controller route request runs. Even after `Clear Session Context`, this action still sends the plain `agent-doc <FILE>` reopen into the live session instead of restarting Codex.
- Editor selection never forks `Run Agent Doc` onto another transport. Selected-text invocations and saved-document diff steering use the exact same Project Controller request as an unselected click and submit the same bare `agent-doc <FILE>` trigger; Rust route policy decides whether that starts, replaces, or steers the live turn. Kotlin must not send `selected_text` / `steering_id`, await a turn-steering acknowledgement, or derive actor-state admission independently.
- Every `Run Agent Doc` click writes a durable attempt ledger entry for click receipt, active document save, `editor_route` request construction/start, retry/dedupe, and terminal route outcome. The ledger may persist diagnostic route shape and route output summaries, but it must not persist raw document prompt text; prompt/trigger proof is represented by byte counts and hashes in binary/controller diagnostics.
- Repeating `Run Agent Doc` while an `editor_route` request is still in flight coalesces with that request instead of canceling and recreating route/controller work. The duplicate click is recorded as deduped and gets an already-dispatching hint; a fresh click is eligible as soon as the bounded request completes.
- That JetBrains request-level coalescing must not leak into controller admission for an already-authorized operator reopen. `managed_reopen` and `dispatch_only_reopen` bypass stale same-generation in-flight dispatch receipts so a fresh `Run Agent Doc` request cannot settle successfully without reaching the pane; automatic/non-operator redispatches remain coalesced.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the JetBrains action layer before route/clear work is submitted. Repeated Run clicks coalesce with the first alive `editor_route` request; `Clear Session Context` pressed while a Run is still dispatching becomes the active editor command and runs the normal binary-owned `agent-doc session clear <relative-path>` path, but it does not cancel or abandon that Run: the Run is a controller-side `editor_route` command the editor cannot cancel, so the action layer keeps waiting for it and still reports its outcome, and the binary orders the clear after that run (`dispatch_turn_pending`, `#clearaftrun`). Before `#clearaftrun` the action layer "canceled" the request, which only abandoned the wait: the run was still typed into the pane (2026-10-15 sdk.md, logged `routeCanceled=true` 12s before the trigger landed) and its outcome was never shown. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear completes synchronously. A clear accepted for deferred delivery by the binary does not release the queued Run immediately.
- `Run Agent Doc` submits its controller-owned `editor_route` directly and never
  creates, attaches, selects, focuses, or reveals an IDE terminal tab. The
  controller route owns cold start and preserves the live actor's tmux session.
  An autostarting Sync may call `agent-doc tmux ensure <FILE> --json`; its strict
  JSON receipt is parsed from stdout only, with stderr kept separate. An
  already-attached client remains authoritative even when a surviving IDE tab
  exists, so Sync does not reveal that tab. For a detached session whose host is
  `ide`, Sync may reuse or create one live `agent-doc` tab, execute the
  binary-provided attach command, and then resume. The Terminal plugin remains
  optional; failures expose the exact attach command with a Copy action.
- The accepted async command executes as controller-owned work with the local
  reactive document projection installed. Nested authority reads must not
  self-RPC through the controller socket; the generic 5-second external-client
  deadline is therefore not a terminal `Run Agent Doc` outcome. The binary must
  not retry the whole route after an ambiguous deadline because dispatch may
  already have occurred.
- If route fails only because the authoritative actor is still in its startup window, `Run Agent Doc` performs one short retry before surfacing the final route failure. This includes dispatch-only `latest run is still booting ... (timed_out)` results; active-turn blockers get a still-running notification, and protected-input blockers such as shell history search are not retried. Repeated clicks while that bounded retry is active coalesce with it.
- A stale startup record must not keep a settled actor in the boot wait. If the requested pane's authoritative actor is `Ready`, dispatch-eligible, and the pane has a recognized busy or interactive blocker, routing exits startup probing and applies the normal busy/queue policy.
- If route succeeds by queueing a prompt behind an already-busy authoritative actor instead of injecting a duplicate trigger, `Run Agent Doc` must surface a visible queued/still-running warning rather than treating the route request as silent success. Repeating `Run Agent Doc` after editing that same prompt must replace the sole live route-owned `agent:run` prompt instead of leaving stale wording queued behind the active turn. The plugin accepts the new `active agent:run` route diagnostic and the older `agent:run auto` wording for compatibility.
- If an explicit send has already delivered steering to the owning turn, `Run Agent Doc` classifies `ui_outcome=steering_delivered_to_owner` as informational success, clears any stale route-failure artifact, and completes the route projection without recording a blocked dispatch. The native notification names the delivered count (`Sent N update(s) to the Agent Doc turn already running for <path>`) and says no second turn was started. A legacy `Error:` wrapper, or the older `No second trigger was injected` clause, does not make it a failure notification. The notification is concise and may expose full machine-readable details only through its copy action. The route log for this outcome is informational, not a warning.
- A successful route into an off-screen authoritative pane is not silent success. The binary appends its `[route] pane-not-visible:` notice when the pane's session is unattached, its window is inactive, or a zoomed sibling hides it; the existing route-output parser raises a warning containing the binary-provided attach or exact reveal command. The plugin does not select tmux windows or panes itself.
- If `Run Agent Doc` observes an explicit frontmatter `agent:` change while the authoritative pane still runs the previous harness, route accepts a typed boundary handoff instead of returning a restart recovery error. No trigger is sent into the old harness; the supervisor preserves an active turn, switches at the next safe idle boundary, and auto-triggers the document on the new harness. A paused queue reports that the accepted handoff is held for queue resume and must not offer supervisor restart as the normal recovery.
- When Codex hook tracking is installed, `Run Agent Doc` must not report success from a live reroute that only proved tmux acceptance. If the bare reopen was accepted but Codex never records routed submission proof, the binary must fail once with that exact stage-specific reason and must not precede it with an optimistic success/progress line.
- If the binary exhausts its bounded submit or dispatch-start proof window for `Run Agent Doc`, it must also file a deduped `#jbrunautobug #agent-doc-bug` item in the session document backlog. The item must include the saved route diagnostic path, failure class, document, stage, pane, best-effort actor generation, editor attempt id, dispatch proof, and `ops.log` marker/path; repeated failures for the same document/stage/failure append evidence to the existing item instead of creating duplicate backlog work.
- When the same `agent-doc <FILE>` reopen is already drafted in a Codex/Claude composer, the binary must press one bare `Enter` instead of appending a duplicate trigger. A visible trigger with a later idle prompt below it is stale scrollback and must not receive an Enter.
- A delayed direct-pane resubmit may press `Enter` only while the pane still exposes a recognizable harness surface. It must preserve the draft and refuse injection after the harness has exited to a bare shell.
- The action is silent on route progress/success. Failures are logged to the IDE Event Log / notification tool window instead of showing bottom-right balloon popups.
- Operator-triggered document-command and manual tmux-load failures also show a bounded top-of-editor
  strip for the affected markdown file. The strip keeps the complete binary diagnostic in its
  tooltip and uses recovery-specific summaries for invalid session frontmatter and a missing actor
  (`Load Tmux Window`); it expires after 30 seconds, and an older expiry cannot clear a newer
  failure. If the binary's diagnostic source line is no longer present in the live editor buffer,
  the strip identifies it as stale controller frontmatter while editor sync retries. Event Log
  remains the durable diagnostic surface.
- An exact operator splice whose recorded pre-edit length differs from the CRDT shadow normally
  rebases through its recorded agent-patch lineage. If that process-local lineage was lost but the
  captured edits can be reversed exactly from one consistent visible-editor cut, the replica must
  reconstruct their pre-edit base and rebase only those operator edits onto controller canonical.
  It must not apply the reconstructed batch directly to the stale shadow or consume newer pending
  editor events. This recovery prevents a corrected frontmatter line from remaining editor-only
  while pane-open commands repeatedly parse an obsolete controller generation.
- A failed route persists the exact `editor_route` output under `.agent-doc/state/editor-route-errors/`. Its notification leads with a bounded actionable cause before path/timing metadata, so the collapsed IDE surface exposes the failure instead of truncating it behind a long document path. Invalid YAML with a tabbed excerpt is summarized as the offending line and the spaces-only fix. Copy/open actions retain the complete unmodified output so startup-miss and pending-drift diagnostics remain inspectable after the toast moment. Typed recovery states such as queued, paused, protected prompt input, dispatch-start-unproven, busy/running, and actor-switch defer do not persist generic route-error files. A later successful `Run Agent Doc`, binary route, or focused sync for that document deletes the saved route-error file so the editor cannot keep showing an obsolete startup/proof failure after route recovery.
- Route session targeting follows the same root-aware chooser as sync: a nested document reroute uses that file's nearest `.agent-doc` root, while a mixed-root visible layout stays pinned to the shared workspace root instead of the focused child repo.

### Patch Application Safety

- JetBrains defers socket and file-watch patch application until the target markdown document has been idle long enough for the typing debounce. If the bounded wait times out, the plugin logs the timeout, does not mutate the document, and retries file-watch patches after another debounce window. Socket patches fail closed so the CLI can retry or fall back through the binary-owned closeout path.
- JetBrains has no broad `save_document` or save-all command. The generation-fenced `persist_current` intent enters the same per-document FIFO worker lane as accepted local and remote CRDT updates, so it cannot overtake an earlier update signal. After the lane reaches the command, the intent is accepted only when the post-registration IntelliJ `Document` still matches the requested hash/byte length and its active replica. It calls `saveDocument` without replacing the buffer, verifies the same bytes on disk, and publishes `disk_persisted`; if IntelliJ defers the write behind File Cache Conflict, the later VFS content event publishes the receipt after the user-approved overwrite, provided disk, editor, and the same replica remain exact. A mismatch requests CRDT redelivery and never replaces the editor from disk. Every rejection records a typed reason plus the expected and observed revision evidence so a retained latest-durable intent can retry or be superseded without treating operator typing as an error. The controller admits the intent for all three release-paired JetBrains packages and any build at or above the native-save protocol floor `0.2.497` (GH #236, `editors/PLUGIN-SPEC.md` §6); a build below the floor is fenced and its retained write falls back to the exact-disk proof without failing closeout.
- **A structurally invalid visible roll-forward is terminal (`#visibleguardterminal`, GH #344).** The project-lifetime `TemplateValidationPlane` retains the shared Rust guard's exact diagnostic with its revision-keyed projection, and every non-exact guard log includes that diagnostic (the failed rule/context, not only `state=Invalid`). When the controller's exhausted-rebuild recovery finds `VisibleStructureInvalid`, the serialized document lane returns `VisibleRollForwardResult.Rejected`; the socket maps that to a rejected receipt, never the retryable `projection_changed` receipt. Lock contention, a missing or disposed replica, a replica behind canonical, and an in-flight remote apply remain `Deferred`. The plugin leaves the buffer byte-exact in every refusal case.
- Replica refresh registers from the controller bootstrap, reports reconnect propagation, and schedules an ordinary canonical-projection drain. The observed editor cut is only a swap fence; it is never seeded or published as a whole-document recovery baseline.
- A successful `replica_pull` with `refused=true, reason=missing_replica` means the cached client belongs to a prior controller generation. JetBrains must classify it as transport invalidation, atomically replace the cached forwarder through registration, and then resume pull/projection. It must not treat that typed refusal as an idle empty update list.
- Ordinary dirty `DocumentEvent`s retain their exact UTF-16 range and old/new fragments, validate that splice against the current replica shadow, translate it to CRDT code-point units, and publish a bounded splice batch. The debounce may batch transport publication but may not reread the whole `Document` and infer one replacement. Whole-text events and clean incremental cache reloads are non-operator projections: they advance the projection epoch, fence queued operator splices, and enter the explicit refresh/recovery path.
- `#splicebaselength` / `#agentpatchlineage`: every captured splice records the Document's UTF-16 length immediately before it and the `patch_id` of the last agent-applied patch (`withAgentAppliedEditorMutation(file, patchId, postText)`, which also records the exact post-patch text hash). A splice whose pre-edit length differs from the shadow at that point is never replayed by offset; a pure insertion has no range text to mismatch, so without this check it lands as many characters away as the shadow is short (live 2026-10-09: an applied response the replica never received shifted a queue line 769 chars into the backlog). When the splices' lineage names the last agent patch and undoing them from a read-locked editor cut reproduces exactly that patch's post-text, the operator splices are transformed through the missing patch (before it: same offset; after it: shifted by its length change; overlapping it: not rebased) and ONLY the operator text is forwarded — the agent text stays the controller's to fold (`#appliedresponsefold`), so nothing is inserted twice, and the editor's visible state is not projected from the operator-only text. Without a proven lineage the splices are held and a remote drain requested until the missing base arrives. An agent mutation without a patch id clears the lineage. A fully witnessed batch whose base is reconstructed from the visible cut (`#devtmuxfeedback`) is rebased onto controller canonical by the native planner, which merges it forward (`#heldbatchmerge`, GH #273) when its changes only sit beside canonical's (a moved ` (HEAD)` suffix, a closeout's struck line or response): the hold, and the canonical projection it defers on every re-registration, remain only for a true overlap or evidence that canonical already holds the operator's text.
- `#jetbrainscleanregistrati`: a registration whose settled shadow, live buffer, and controller canonical are three distinct generations merges forward only through `cleanRegistrationMergeUtil`: the native captured-splice rebase of the one shadow-to-buffer splice (`singleSpliceBatchUtil`), asked once, with its whole-line anchoring. A refusal is no merge, so registration holds the operator's buffer exactly as typed (`HoldOperatorBuffer`). The plugin does not bind the char-level `agent_doc_reconcile_text`: it re-anchored a canonical close onto a queue item the operator typed (`- do [#sdkexamples] (closed)`) with zero conflicts. The rebase's narrow `#struckqueuepruneattach` exception accepts a buffer that differs from its settled shadow only by deletion of already-struck queue rows: it removes exactly those stable keys from canonical, preserving every live row and canonical advance. Live-row deletion, survivor edits/reordering, replacement typing, duplicate keys, and malformed components retain the ordinary merge-or-hold behavior. See `specs/14-realtime-workflow.md` § Registration merge.
- `#captureddeltalivelock`: a captured burst the replica applied whose durable `pushDocumentOps` was refused or timed out (`forwardLocalEdits ... durable=false`) is recorded as an `UndurableLocalForward` checkpoint (holding endpoint, shadow, exact captured prefix, batch, projection epoch). `CrdtReplicaForwarder` keeps the frontier preceding those ops (`holdsUndurableLocalEdits`) across remote applies and `republishUndurableLocalEdits()` re-sends exactly those ops from it. Every local flush first runs `settleUndurableLocalForward`: the holding endpoint, if still attached, republishes; the checkpoint then settles only when controller canonical equals the burst's result or `NativePatching.capturedSplicesContained(shadow, canonical, batch)` is true. Settling advances the shadow to the burst's result (`recovery=undurable-local-delta-settled`) and forwards only the splices typed after it, so a burst the controller ingested despite the lost receipt is never rebased again together with the later typing (which refuses forever as "canonical already carries captured operator text"). Without proof the whole burst stays retained; after `UNDURABLE_LOCAL_FORWARD_SETTLE_ATTEMPTS` (8) retries it is rebased from the shadow as before (`recovery=undurable-local-delta-unsettled`). The visible buffer is never overwritten by the controller copy on this path.
- Component intents must honor an explicit JSON `op` (`replace`, `append`, or `prepend`) ahead of the component marker's `patch=` / `mode=` attribute. Lazily convergence uses `op: "replace"` for an `agent:exchange patch=append` body so recovery cannot append a second response; it never falls back behind an attached editor.
- JetBrains must not apply a socket or file-watch patch while IntelliJ has a pending File Cache Conflict for that document. The plugin treats the conflict as a terminal editor-side refusal for that IPC payload: socket IPC returns failure, file-watch IPC records `file_cache_conflict_pending`, deletes the queued patch file, refreshes agent-doc visual highlighting, and leaves the response for the binary-owned retry path. It must not retain a conflict-deferred patch id, wait for the dialog to resolve, or replay the old payload after the user keeps memory changes or later accepts filesystem changes. The binary-side follow-up contract for older already-written Cancel-shaped closeouts remains: if the response already reached the working tree/snapshot but not `HEAD`, the next `agent-doc preflight` auto-commits the missing boundary rather than surfacing the older manual `write --commit` recovery requirement.
- **A queued memory/disk conflict must be searchable as `File Cache Conflict` (GH #384).** Current IntelliJ queues the conflict on `MemoryDiskConflictResolver.isInConflictQueue` and logs `conflict queued for <file>; a merge can still resolve it before the dialog runs` plus `documentStamp` / `oldFileStamp`. That platform line does not contain the phrase `File Cache Conflict`, does not record that the dialog was shown, which button was chosen, or a production stack. Older platforms expose the same queue as `hasConflict`. The plugin treats either method as pending and prefers `isInConflictQueue` when both exist. On the VFS content change that observes a newly queued conflict for an owned markdown document, it writes one `idea.log` warning containing the exact phrase `File Cache Conflict`, the file path, the resolver probe name, `documentStamp`, and `oldFileStamp`, with the observation stack. The line does not invent a dialog button. A repeated observation of the same pending edge does not write another line.
- Every disk change observed while an IntelliJ buffer is open is retained by the binary as an independent pending external-disk candidate. Accepting filesystem changes produces a clean document event; JetBrains asks the shared resolver for the exact candidate, resets and re-registers the replica from the visible buffer, and reports settlement only after successful CRDT propagation. Keeping memory changes never receives or merges the disk candidate: a later edit or save clears it in the shared binary state. Closing the final editor clears the candidate and falls back to disk; closing one of several editor replicas does not.
- JetBrains file-watch IPC must also accept reposition-only patches emitted by commit cleanup (`patches: []`, `reposition_boundary: true`, `preserve_head: true`). These patches are applied through `Document.setText` / VFS APIs, reuse the committed boundary id when provided, and preserve visible ` (HEAD)` response markers so closeout cleanup does not trigger IDE file-cache conflict dialogs.
- `Run Agent Doc` does not use the patch-application typing debounce. It saves the active markdown document and routes immediately; patch/socket mutation paths keep their bounded idle guards.
- `Claim for Tmux Pane` and `Force Claim Current Pane` save only the target document, fail closed if it remains dirty, and temporarily make every editor view of that file read-only while the bounded background CLI mutation runs. Completion refreshes that virtual file synchronously before restoring editor writability, so IntelliJ cannot race user typing against claim scaffolding or surface a File Cache Conflict from the plugin's own action.
- Editor-op capture (`TypingTracker`, `#opcapturedormant`, GH #242): each captured `PendingEditorOp` carries `eventSeq`, a per-`Document` count of the `DocumentEvent`s the listener observed, bumped inside `documentChanged`; the drain snapshot reads the same counter with `document.text` in one read action (`DocumentSnapshot(text, eventSeq)`). `Document.modificationStamp` is NOT the race clock: IntelliJ restores the pre-change stamp on undo and re-stamps on save / Remote Dev sync without firing a `DocumentEvent`, so a stamp guard re-refused one undo 32,277 times. `classifyBurstAgainstSnapshotUtil` decides: `CURRENT` replays now; `OUTRAN_BY_LATER_EVENT` requeues the burst and logs `doc_advanced_during_drain` but schedules no retry (the later event's own `documentChanged` already scheduled the quiet boundary that can succeed); `SNAPSHOT_AHEAD_WITHOUT_PENDING_EVENT` is decided immediately by shadow replay (handed over, or a terminal `shadow_replay_mismatch` with `snapshot=<relation>` in the detail) and never requeued. Identical refusal receipts are coalesced in the FFI (`coalesced_repeats=<n>`, 2 s window per document). VS Code reports each change immediately with no drain/requeue loop, so it has no equivalent retry path.
- Edit-time component-marker check (`#componentownclose`, gap 1 of the todo-collapse fix): the recorded editor ops are offsets and lengths only, so before `reportEditorOps` records a burst, `TypingTracker.checkEditorOpComponentMarkers` passes the burst's lineage `preText` / `postText` to native `agent_doc_check_editor_ops_component_markers`. Native owns the rule (the live relay fence's `edit_removed_component_marker` predicate) and writes or clears the `⚠ agent-doc:` marker diagnostic in `.agent-doc/state/editor-route-errors/<doc>.txt`. Kotlin only maps the return code (`EditorOpMarkerCheck`), re-renders the status label from the last controller frame (`TurnStateBannerRefresher.reapplyLastAuthority`, dropped if a newer frame landed), and raises one Event Log warning per break (`EditorOpMarkerWarnings`; a repair re-arms it). The editor text is never changed and the burst is still recorded; an older cdylib without the export skips the check.

### Native string encoding

- GH #274: every string that crosses the JNA boundary to `libagent_doc` is byte-exact UTF-8, whatever the IDE's locale. JNA's default string encoding follows the JVM's `native.encoding` (the platform locale), not `file.encoding`. A backend started under `locale=en_US` (`sun.jnu.encoding=ANSI_X3.4-1968`) therefore encoded every `String` argument as US-ASCII with replacement. Every em dash, `·`, `→` and emoji in document text became a literal `?` (valid UTF-8, invisible to Rust), and every non-ASCII byte Rust returned through an un-encoded `Pointer.getString(0)` decoded to U+FFFD. A committed response came back as a second, lossy copy and was committed.
- `AgentDocLib` generations are bound with `Library.Handler(path, AgentDocLib::class.java, NATIVE_LIBRARY_OPTIONS)` (`OPTION_STRING_ENCODING = "UTF-8"`), which covers every `String` parameter, every `String` return, and `nativeLibrary.getFunction` lookups. A handler bound with `emptyMap()`, or `Native.load`, is forbidden.
- Every returned `char*` is read with `Pointer.readNativeUtf8()` (`NativeStrings.kt`), a strict decoder that throws on malformed UTF-8 instead of substituting U+FFFD. `Pointer.getString(...)` is not used.
- The generation proxy calls `requireLosslessNativeStringArgs(method.name, args)` before every native call. A `String` argument with an unpaired UTF-16 surrogate, the one input that UTF-8 encoding also replaces with `?`, refuses the call with `LossyNativeStringException` and never crosses.
- `NativeStringEncodingTest` forces JNA's default to US-ASCII. It proves the default is the issue's lossy encoder (`dispatched ? opus-5.5 ?`), and that the bound handler crosses the issue's text and 500 seeded random non-ASCII documents byte-exact (length and round trip). It also proves the surrogate and malformed-UTF-8 refusals, and source-scans the plugin so no un-encoded crossing returns.

### Session Operator Actions

- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in an IDE notification instead of re-deriving status inside the plugin. A successful status response deletes the saved route-error file for that document because the old failure is no longer the latest observed state.
- `Recycle Supervisor` runs `agent-doc session restart-supervisor <relative-path>` and keeps recycle ownership in the binary/supervisor path. The action ID remains `AgentDoc.RestartSupervisorProcess`. If the binary refuses because the pane is busy or the authoritative actor is still starting, JetBrains must show the typed restart warning with `Interrupt and restart`, `Show status`, and `Copy details` actions; the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session. The binary owns live pane status for diagnostics and clear safety: stale busy actor/supervisor projection is reconciled when direct live pane evidence is `alive-idle` with `prompt_ready=true`, including a bottom Codex model/cwd/context footer below older transcript text. A live `agent-doc` wrapper process is not by itself proof that the session is still running; ordinary active/status panes must proceed through the normal clear submit path. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary; repeated clear clicks report the existing queued clear instead of injecting another `/clear`, and JetBrains surfaces that output as accepted deferred work. Clear may fail closed when the captured pane contains protected prompt input such as a permission prompt, queued draft, shell history search, or drafted user text, or when it shows an explicit busy cue that cannot be deferred such as an active Codex turn, hook-review prompt, or help screen; the operator can then choose the explicit interrupt-clear discard path.
- `Clear Session Context` must not consult plugin-local response-status or busy flags before invoking the binary. Live pane idleness is binary-owned for status/diagnostics; stale editor-side status must not block clear.
- Protected prompt-input clear refusals must show a typed warning with the relevant interrupt action, `Show status`, and `Copy details` instead of the generic command-failed text. Legacy alive-busy and `active_agent_doc` refusals may still use the busy-session warning, but the binary must not emit that warning solely because the live pane is the `agent-doc` wrapper.
- `Interrupt and clear` requires an explicit IDE confirmation and then runs `agent-doc session interrupt-clear <relative-path>`, leaving harness-specific interrupt keys, Vim/Neovim prompt recovery, idle/closed waiting, and the final clear retry in the binary-owned operator path. If the binary finds the live pane already idle with `prompt_ready=true`, it must skip the interrupt key sequence and proceed directly to the clear retry so the standalone `Interrupt and Clear Session Context` action cannot perturb an idle Codex pane into an editor or other terminal mode. It is available both from busy/protected clear-refusal notifications and as the standalone `Interrupt and Clear Session Context` IDE action. It is the only action that intentionally discards protected prompt input.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>`, copies the exact output, and keeps the binary-owned diagnostics text available for bug reports.
- Plugin verification must cover exact session-status display, `session clear` command wiring, and persistent route-failure retention for stage-specific dispatch diagnostics.
- `About Agent Doc` (`AgentDoc.About`, Tools/editor/project-view menus and the popup's More Actions) runs `agent-doc version --json` (falling back to `--version`) with the binary from `TerminalUtil.resolveAgentDoc(projectRoot)` on a pooled thread, reads the loaded native generation through `AgentDocLib.aboutSnapshot()` (canonical path, validated `agent_doc_version()`, `agent_doc_build_info_json()`; never loads or reloads), and shows a dialog with a Copy button. The plugin version is the IntelliJ plugin descriptor version. Mismatch rules follow `editors/SPEC.md` section 6b.
- Binary/native resolution follows the IDE process `PATH` before workspace and home-directory fallbacks, and logs every attempted install. A native load failure names both the executable that supplied `lib-path` and the library; recognizable GNU/musl loader failures are labelled as libc mismatches. Because `admin reload-lib` requires a loaded native endpoint, initial-FFI remedies direct the operator to correct/remove the conflicting install and rely on the loader's automatic retry instead of presenting `reload-lib` as a reachable first step.
- `Agent Doc Dashboard` (`AgentDoc.Dashboard`, `DashboardAction`, `gvqv`) is project-scoped: it runs `agent-doc dashboard --write` from `TerminalUtil.cleanupProjectRoot`, then refreshes and opens `<root>/.agent-doc/dashboard.md` on the EDT. The controller keeps that projection current; `isAgentDocDocumentTextUtil` rejects it, so tab sync, CRDT replicas, and layout sync ignore the open tab. It is listed last in the popup's numbered group. `DashboardActionTest` covers the arguments, path, menu declarations, popup reachability, and session-classification rejection.

### Safe Sync Surface

- JetBrains startup must not run automatic `agent-doc resync` or `resync --fix`. Session repair/audit is explicit operator action only, because startup audits can traverse large process/session graphs and make the IDE unresponsive.
- Editor-driven layout syncs report absolute file paths to the binary sync surface, preserve empty column placeholders for mixed markdown/non-markdown splits, and keep cross-root markdown siblings in the reported layout even when the focused file lives in a nested submodule.
- When the visible markdown layout spans multiple nested agent-doc roots, JetBrains uses the workspace root `.agent-doc/` as the sync project root instead of the focused file's nearest submodule root. This keeps shared `state.db` column memory stable when focus moves from a workspace session doc to an unmanaged spec/doc file inside a child repo.
- The Rust binary owns passive autostart, ambiguity handling, remembered-column restoration, and tmux window targeting.
- An automatic pane-layout generation whose focused document has no file-to-pane assignment settles the exact visible structure without focus and retires. Replaying the same generation cannot create that assignment; a later editor-surface observation owns any structural change. Manual focus, failed `select-pane`, and pane-window co-visibility drift remain fail-closed.
- JetBrains handles a session-document rename or move through a retained Lazily path-transition projection. It publishes new-path liveness before old-path removal, waits for the existing Project Controller's convergence receipt, then registers/swaps the new-path replica before retiring the old forwarder. The exact liveness frame is retained across enqueue/flush failure. When a move crosses a nested project boundary, controller resolution chooses the narrowest available root that contains both old and new paths; it must not retain retries against the new nested controller when that controller cannot own the old identity. This path does not launch the CLI and does not invoke layout sync, so the existing pane, window, and horizontal/vertical layout remain untouched.
- A `<stem>.done.md` archive candidate must never become a path-transition alias for a still-existing canonical `<stem>.md` document. The editor rejects that VFS edge before moving liveness, and the controller independently refuses it before relay rekey or alias publication. A real rename whose old path is absent and an independently addressed `.done.md` document retain their own identities.
- JetBrains CRDT replica IPC uses the Project Controller socket (`.agent-doc/controller.sock`) with the controller `crdt_replica` envelope and includes `ProcessHandle.current().pid()` as `editor_pid`. That process-scoped proof lets a detached agent-doc registration establish editor authority through the relay; the controller must not require authority to pre-exist registration. It must not connect to per-session supervisor sockets for replica register/update/pull/projection/deregister/current-text work.
- JetBrains advertises `coordinated_extract_v1` through the same shared capability list used by initial/open-document registration, liveness refresh, and editor-content receipts; a receipt-local shadow list must never be the only advertisement. The PID-scoped `freeze_for_extract` intent makes the named document read-only, fences every automatic re-registration path, and waits for replica deregistration. While that durable endpoint remains editor authority, the binary compare-and-swaps controller canonical under a zero-live-replica fence and writes disk only as the exact projection of the advanced canonical; it must not use a disk-only force write that resume could supersede with the old canonical. The matching operation-id-scoped `resume_after_extract` intent reloads that projection, waits for fresh CRDT registration bootstrapped from the transferred canonical, and only then restores the prior writable state. The ordinary 750 ms attach wait is a pending receipt rather than a refusal: resume waits for the already-admitted serialized attach to settle before re-arming a freeze. A failed resume keeps the tab read-only, so an old buffer cannot overwrite a completed cross-document extraction. A dynamic plugin unload restores every `Document` writable state captured by that watcher before discarding its in-memory freeze ownership, so reload cannot strand a tab read-only.
- CRDT registration eligibility is derived from the current editor text at every open-document and local-edit ingress. When no attached replica exists and the text carries neither `agent_doc_*` frontmatter nor an `<!-- agent:` marker, JetBrains records `not_agent_doc_document`, cancels any armed registration backoff, advances the local-only shadow, and completes the splice batch without a controller round trip or another local retry. The decision is not sticky: a later text observation that adds a session marker is eligible immediately. (GH #282)
- The Project Controller is the final identity-admission boundary: a registration whose `agent_doc_session` is durably owned by another canonical document path/hash receives terminal `duplicate_session_identity` before any existing relay authority is consulted. JetBrains cancels registration retry state and deregisters any cached endpoint on that refusal. Thus an opened or refreshed plain copy cannot acquire or retain a replica, schedule its copied queue, or be rewritten by retained effects; a genuine rename remains eligible through the durable path/hash ownership rule. (GH #349)
- A CRDT replica refresh captures the current IntelliJ `Document` only as an expected-editor-text generation fence, then registers and swaps a replacement opened from the controller bootstrap. Pending local work or editor drift rejects the swap without replacing the cached member. The retained Lazily target is projected downstream after registration; no editor text-adopt or full-state request surface exists.
- JetBrains drains remote CRDT deliveries from targeted `EditorIntent` events and the Lazily-backed controller subscription. It must not watch a filesystem event directory or run a fixed interval remote-update pull loop.
- Remote CRDT delivery into the editor is backpressured by one keyed RelayCell hot head per document. The merge retains the oldest guarded editor baseline and newest converged text. Every document has one FIFO replica worker (with idle thread timeout), so attach, normalization, local deltas, remote drain, and projection work stay ordered within that document without blocking another open document.
- After integrating the coalesced remote frontier, JetBrains publishes one full visible-state hash. The controller derives the cumulative represented prefix from that hash; the plugin keeps no pending-ACK sidecar and sends no replay request.
- Before mutating a clean document, the plugin refreshes that target file's VFS stamp and rechecks that the document stayed clean. Novel external disk text rejects the delivery without mutating or overwriting it. IntelliJ's ordinary save lifecycle produces the separate persistence projection.
- JetBrains turn-state projection is event-driven, cached, and read from the Project Controller `state_subscribe` Lazily projection. It does not read filesystem state for ordinary turn-state UI. If the Project Controller request fails, the status bar shows `agent-doc: Project Controller disconnected`, with no fallback authority. Projection drains cap each work slice and yield between backlog slices so bursts cannot monopolize a plugin worker or indirectly starve the UI.
- The turn banner tracks a document once any editor surface shows it (GH #323). `TurnStateBannerProvider.collectNotificationData` admits the document itself (`TurnStateBannerRefresher.observeDisplayed`), in addition to project `FileEditorManager` `fileOpened`/`selectionChanged` and the startup open-file scan, because a Remote Dev client's open and selection edges never reach the project topic (GH #262); before this, a document opened in the client after the backend started never entered the tracked set, every controller frame for it was dropped, and the banner stayed hidden through an `awaiting_response` turn. `TurnStateDocumentTracking` owns the tracked set and one controller subscription per document: a frame for a subscribed but not-yet-displayed document (one subscribed by `requestRefresh`) is held and replayed on admission, and a frame arriving after release is dropped. `fileClosed` releases only through `DocumentCloseDecision.route("turn-state-banner", ...)`, so removing one of several editors while another client still shows the document keeps its banner (GH #241 shape). Admission, release, and each per-document presentation change log at INFO (`[turn-state] tracking`, `[turn-state] released`, `[turn-state] projection changed ... banner=<bool>`). The banner reads no document content: `agent:status`/`status:sync` structure does not affect it (malformed `status:sync` is repaired by `agent-doc status repair`).
- Prompt steering is Project Controller-owned. JetBrains must not treat stale supervisor freshness as a local editor-IPC apply/receipt/repair veto; supervisor recycle is only an explicit session action.

### Component Folding, Gutter Markers, and Structure View

GH #19 (plugin UX phases 5-7); shared contract in `editors/SPEC.md` § 12.

- Registered in `agent-doc-markdown.xml`, loaded through an optional dependency on the bundled
  Markdown plugin (`org.intellij.plugins.markdown`): `lang.foldingBuilder` and
  `codeInsight.lineMarkerProvider` for language `Markdown`, plus a `lang.structureViewExtension`
  that adds component nodes under the Markdown structure view's file root (the heading outline is
  kept). Without the Markdown plugin none of them load.
- Boundaries come from `agent_doc_parse_components` (`NativePatching.componentSpansOrNull`), whose
  UTF-8 byte offsets `AgentDocComponentOutline` maps to UTF-16 editor offsets. The parse runs on
  `VisualHighlighterManager`'s debounced (120 ms) background refresh with the same text snapshot
  as the visual tokens, and only for a session document (`isAgentDocDocumentTextUtil`); a plain
  Markdown file installs an empty outline, so every extension is inert there.
- `ComponentOutlineStore` (owned by the project's `VisualHighlighterManager`, weak document keys)
  keeps each component as open/close `RangeMarker`s, so between refreshes boundaries track edits
  exactly. The folding builder, line-marker provider, and structure extension read only that store
  and the live document text: no native call, no disk read, no PSI walk per keystroke. A refresh
  restarts the daemon for the file only when the installed outline differs from what the markers
  already track (component added, removed, renamed, or re-bounded). A failed or unavailable native
  parse (library missing, unclosed marker mid-edit) keeps the last outline. Disposal clears the
  store and its markers.
- Folding: each multi-line component folds from its open marker through its close marker to
  `<!-- agent:NAME · N items -->`; nothing is collapsed by default. Gutter: one
  `AllIcons.Nodes.Template` icon on the leaf holding each open marker; the tooltip names the
  component, its item count and inline attributes; clicking toggles that component's fold.
  Structure view: `agent:NAME` nodes with the item count as location, nested components and
  items as children, each navigating to its line.

### Agent Doc Actions popup

- `AgentDocPopupAction` (`AgentDoc.Popup`) defaults to `Ctrl+Shift+Alt+D`. It never uses `Alt+Space` (`#gh116`: Windows consumes it for the window system menu, so the IDE never receives it) in `$default`. The `Default for XWin` keymap adds `Alt+Space` on top of `Ctrl+Shift+Alt+D` (`altshiftmenu`: i3 and other bare X window managers deliver it, and it was the Linux operator's working menu key until #gh116 removed it everywhere); GNOME and KDE keymaps do not get it because those desktops claim Alt+Space. The `$default` shortcut is declared first so the XWin shortcut is added to, not substituted for, the inherited one. In IntelliJ IDEA Ultimate, `Ctrl+Shift+Alt+D` is also the bundled Database grid's `Console.TableResult.CloneColumn`, which is disabled outside a data grid, so it does not compete in a markdown editor. It installs no `ActionPromoter`, so native `Alt+Enter` intentions stay intact. The popup is also in the Tools menu and editor context menu, and like every `AgentDoc.*` action it can be rebound under Settings > Keymap.
- Every declared `AgentDoc.*` action is listed in the popup's primary or More Actions group; `AgentDocPopupActionTest` enforces this and rejects OS-reserved default keystrokes.
- The popup resolves its ids through `AgentDocPopupAction.resolveActions` and never passes an unregistered action to `DefaultActionGroup.add`, which rejects null. Under Remote Dev the backend builds the popup from its own `ActionManager`, which holds only `agent.doc.backend.xml`; ids in `CLIENT_ONLY_ACTION_IDS` (the modular `AgentDoc.UpdateChannel`, registered only in `agent.doc.frontend`) are omitted there and stay reachable from the JetBrains Client's Tools menu. Any other unregistered id is logged and skipped, so one missing registration cannot keep the popup from opening (GH #375: the 0.2.827 backend threw `IllegalArgumentException` and the popup never opened). A single-process IDE registers the frontend module in the same `ActionManager`, so the client-only actions still appear. `BackendPopupActionRegistrationTest` (exact-262/263 backend) requires every popup id to be in the backend descriptor or client-only, and every client-only id to be frontend-owned and absent from the backend.

### Action update threading

- Every `AnAction` subclass in the plugin — each `plugin.xml` action and group, the runtime-built `DumbAwareGroup`, and any anonymous action — overrides `getActionUpdateThread()` itself. An action that inherits it gets the platform's deprecated `OLD_EDT` default, so its `update()` runs on the EDT; reading a rule-provided data key there (`CommonDataKeys.VIRTUAL_FILE` on build 263) makes `PreCachedDataContext` log a SEVERE "requested on EDT ... See ActionUpdateThread javadoc" blamed on Agent Doc, once per action per IDE session (`#gh265`: Run, Claim, Force Claim, Fix Document, Initialize Session, Lint Document, and Run with Junie, raised from the Project View popup).
- The value is `ActionUpdateThread.BGT`: every agent-doc `update()` only reads the data context, the file extension, and plugin state, never Swing or other EDT-only APIs. An `update()` that genuinely needs the EDT returns `ActionUpdateThread.EDT` and must not read rule-provided data keys; `OLD_EDT` is never used.
- `ActionUpdateThreadTest` enforces this from the compiled plugin classes (every concrete `AnAction` subclass in the plugin package, failing on any class it cannot load) and from the `plugin.xml` registrations, not from a hand-maintained list, and fails if any action resolves `getActionUpdateThread()` from a platform class or returns `OLD_EDT`.

### Logging

- Uses `com.intellij.openapi.diagnostic.Logger` (IntelliJ platform logger).
- Enable debug output: `Help > Diagnostic Tools > Debug Log Settings` → add `#com.github.btakita.agentdoc`.
- Output appears in `idea.log`. No temp files.

#### Layout sync diagnostics

Navigation/tab-switch layout decisions are traceable end to end across these prefixes:

- `[layout-detect]` (`LayoutDetector.detectEditorLayout`) — the **editor-side input** to every sync. Logs the editor window count, each window's on-screen position and selected `.md` file (`x=… y=… file=…`), and the resulting column grouping (e.g. `grouped into 2 column(s): [left.md] | [right.md]`). This is the line that explains *why* a navigation produced an N-editor-pane layout: a 2-column grouping is what makes sync provision two editor panes plus the agent-doc pane. Detection failures are logged via `LOG.warn` instead of being silently swallowed.
- `[layout-sync]` (`EditorTabSyncListener`) — `selectionChanged`, IDE activation, micro-coalesced command-plane `focus`, the single surface-graph observation enqueue (including the columns derived from `[layout-detect]`), plus debounce/guard/deferred/timeout/generation diagnostics. One editor click's selection and component-focus notifications collapse into one project-scoped intent; inactive or expired intent is refused, and `LayoutChangeDetector` only reports structural changes into the authoritative graph. Selection precedence ends as soon as the EDT captures a self-consistent surface: controller delivery is in-flight state, not pending editor observation, so a later genuine focus event can supersede a slow layout realization instead of being discarded.
- `[sync:PHASE]` (controller-owned tmux-router reconciler) — `GLOBAL` window+pane state at sync-start/sync-end and `SELECT`/`ATTACH`/`DETACH`/`REORDER`/`VERIFY`/`SWAP` phases. The `DETACH` phase names the reason each pane is kept or stashed (last pane in window, protected busy pane, registered to another session, stashed/broke, focus-steal), which explains *why* an extra pane survived into the visible window.

Binary auto-start forensics also land in `/tmp/agent-doc-sync.log` and the per-document `.agent-doc/logs/ops.log` (`[sync] auto-started %XX for <file>` plus batch summaries and per-phase latency).

### Dynamic Lifecycle

- Closing plain Markdown with no attached CRDT replica is a successful no-op: there is no editor authority or final cut to publish, so the plugin emits the ordinary native close hint, clears local unsynced-edit bookkeeping, and does not warn. This decision precedes editor-text access when no project owns the path or the owning manager has already classified the path as non-agent-doc. If classification still requires the current text, the dedicated close worker waits for IntelliJ read access rather than treating a transient read-action refusal as a failed publish. If a file still has an attached replica (including an agent-doc whose markers were removed before close), its exact final editor cut must settle before authority is released; a real publish failure remains fail-closed and retains authority (GH #287, GH #298).
- `PluginLifecycleListener` handles `projectOpened`/`projectClosing`.
- Startup root discovery and native listener registration run on the pooled application executor, not the IDEA event-dispatch thread. This keeps fallback scans away from UI startup when Linux inotify watches are exhausted.
- Editor layout/window snapshots and document mutations are captured on the event-dispatch thread, while project-root discovery, filesystem walks, patch-watch registration, command-plane/native work, and tmux consequences run in background executors. Remote Dev's zero-window layout path snapshots client-visible, selected, open, and focused session documents on the EDT, but classifies that flat evidence as non-structural on the surface delivery worker; exact frontend column vectors pass through unchanged. Socket patch apply first captures an immutable document text/stamp proof on the EDT, performs every native replay/component/normalization calculation on the listener worker, then returns only the computed target to the EDT for a proof-fenced minimal edit. The immutable post-write content receipt is published from the socket worker before acknowledgement. The native generation bridge rejects event-dispatch-thread calls so a busy controller cannot freeze IDEA, and an adapter path must not catch that rejection as an ordinary patch failure caused by scheduling native computation inside an EDT closure.
- `fileClosed` fires when ONE editor for a file is removed, not only when the file stops being open (GH #241: a Remote Dev guest opened a second editor for an open document and one was removed). `PluginLifecycleListener.fileClosed` and `ReliableSyncLivenessListener.fileClosed` route through `DocumentCloseDecision.route`, which observes `FileEditorManager.isFileOpen` on the backend manager and on the event's source manager plus every Remote Dev `ClientFileEditorManager.getAllFiles()`. Only when no client still has the file open does it release (`TypingTracker.clearOpenDocumentReport`: closing cut, replica deregister, `agent_doc_document_closed_for_editor`; liveness: the observed-tag `Close`). A still-open verdict keeps the replica, calls `CrdtReplicaManager.ensureOpenDocumentReplica(..., "editor-removed-file-still-open")` (a no-op while attached), logs `decision=keep_still_open`, and re-observes once after `DocumentCloseDecision.STILL_OPEN_RECHECK_MS`, releasing then if every client has dropped the file. `EditorTracker.activeEditors` is deliberately not an input (GH #175 hidden tabs). `selectionChanged` on a markdown document also calls `ensureOpenDocumentReplica(..., "selection")`, so an open document whose replica went missing without a transport loss re-attaches on the next selection instead of waiting for a peer replica pull.
- Project close and plugin unload dispose CRDT replica, patch watcher, layout detector, and visual highlighter resources. Queued visual-refresh callbacks check their manager generation's disposed state before scheduling or applying, and a scheduler-shutdown race is inert rather than an IDE exception.

## Keybindings

| Action | Default Shortcut |
|--------|-----------------|
| Run | `Ctrl+Shift+Alt+A` |
| Initialize Session | none |
| Fix Document | none |
| Claim | `Ctrl+Shift+Alt+C` |
| Sync Layout | `Ctrl+Shift+Alt+L` |
| Agent Doc Actions popup | `Ctrl+Shift+Alt+D` (also `Alt+Space` in the `Default for XWin` keymap) |
| Run with Junie | `Ctrl+Shift+Alt+J` |
| Load Tmux Window | `Ctrl+Shift+Alt+W` |
| Refresh Environment | `Ctrl+Shift+Alt+R` |

All defaults are registered in the `$default` keymap and can be changed under Settings > Keymap (search "Agent Doc").

## Context Menu

Run, Initialize Session, Fix Document, Claim, Compact Exchange, Sync Layout, Show Session Status, Recycle Supervisor, Restart Agent, Stop Agent, Clear Session Context, Interrupt and Clear Session Context, and Copy Session Diagnostics are available in:
- Tools menu
- Editor right-click context menu
- Project view right-click context menu (Run, Initialize Session, Fix Document, Claim, and session operator actions)
