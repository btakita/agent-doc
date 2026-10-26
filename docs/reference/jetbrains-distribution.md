# JetBrains plugin distribution

Agent Doc ships three compatibility-ranged updates under the single plugin ID
`com.github.btakita.agent-doc`:

| Update | IDE range | Release artifact |
|---|---|---|
| Classic | `242` through `261.*` | `agent-doc-jetbrains-<pluginVersion>.zip` |
| Plugin Model v2 (262) | exactly `262` through `262.*` | `agent-doc-jetbrains-262-<pluginVersion>.zip` |
| Plugin Model v2 (263) | exactly `263` through `263.*` | `agent-doc-jetbrains-263-<pluginVersion>.zip` |

The update versions are distinct. The ranges do not overlap, and all ZIPs are
built and verified before the tag workflow may create a GitHub release. A
missing ZIP, an equal version, an unexpected plugin ID, or a widened range stops
the workflow before publication.

## Production repository URLs

In addition to Marketplace, every stable GitHub release publishes three
JetBrains
[simple custom-repository listings](https://plugins.jetbrains.com/docs/intellij/custom-plugin-repository.html)
as a fallback channel:

- Classic IDEs: `https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-classic.xml`
- IDEs on branch 262: `https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-262.xml`
- IDEs on branch 263: `https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-263.xml`

JetBrains permits a plugin ID only once in a simple static listing, so the three
updates cannot safely share one XML file. Each listing contains exactly one
update and repeats the ID, version, and compatibility range read from the built
ZIP's patched `plugin.xml`. The listing itself is reached through
`releases/latest/download/`, but its download URL is pinned to the tag that
produced it (`releases/download/<tag>/agent-doc-jetbrains[-262|-263]-<version>.zip`),
so a listing and the archive it names always come from the same release.

Configure only the listing for the IDE's build in Settings → Plugins → gear →
Manage Plugin Repositories. The classic listing cannot select the modular ZIP,
and the 262 listing cannot offer the classic ZIP. Because the ranges are
disjoint, configuring the wrong listing (or both) is still safe: an IDE only
offers an update whose `since-build`/`until-build` contains its own build, so a
242–261 IDE never sees the 262 update and a 262 IDE never sees the classic one.
Builds below 242 or above `263.*` are offered nothing.

The plugin also exposes **Tools → Enable Agent Doc GitHub Updates** (and the
same command under **Agent Doc Actions → More Actions**). Like IdeaVim's EAP
toggle, it adds the compatibility-matched repository URL to JetBrains'
`storedPluginHosts`; running it again removes the Agent Doc URL without touching
other custom repositories. In a modular Remote Dev installation the action is
owned by `agent.doc.frontend` and therefore changes the JetBrains Client's
process-local update settings, not only the backend's settings. This GitHub
channel receives the verified release artifacts before Marketplace availability,
which makes it the opt-in early update path for local/custom-repository installs.
The backend still needs the identical update through its own repository or CLI
installation path before both processes restart.

### Marketplace

When `JETBRAINS_MARKETPLACE_TOKEN` is configured, every tagged release uploads
all three ZIPs as updates of the same plugin ID,
`com.github.btakita.agent-doc`. Immediately before the first upload, the release
job re-reads each packaged `plugin.xml` and refuses the whole set unless the
artifact names, shared ID, distinct versions, and exact ranges are `242` through
`261.*`, `262` through `262.*`, and `263` through `263.*`. A missing token emits
a notice and defers only Marketplace publication: the core GitHub/PyPI release,
three verified ZIPs, and custom-repository listings continue normally.

Marketplace applies compatibility selection independently to the Remote Dev
backend and JetBrains Client/Gateway and synchronizes the selected modular ZIP
to both peers. This removes the custom-repository requirement to install the
same ZIP manually on both sides. A release marker is written to the GitHub
release after each accepted upload, so retrying a failed tag workflow resumes
at the first update that Marketplace did not accept.

`make release-version` keeps a plugin generation unchanged when its sources are
byte-identical to the previous release (`#marketplaceretainedversion`), so
Marketplace may already hold that exact version from an earlier tag. Its
duplicate-version refusal (`already contains version <version> in channel`) for
the artifact's own version counts as accepted and records the marker; every
other upload refusal still fails the job.

#### Marketplace onboarding

The repository owner may complete these one-time external steps independently
of the core release schedule:

1. Create or join the Marketplace vendor account, accept the Marketplace
   agreement, and create the `com.github.btakita.agent-doc` plugin listing.
   JetBrains requires the first update to be uploaded through the Marketplace UI
   so listing metadata, licensing, source, and support URLs can be reviewed. Keep
   that bootstrap update hidden; automated releases supply the three production
   ranges.
2. In the Marketplace profile's **My Tokens** page, create a permanent token for
   the account that owns the plugin.
3. Add the token to this GitHub repository as the Actions secret
   `JETBRAINS_MARKETPLACE_TOKEN`. Never put the token in Gradle properties,
   workflow input, an artifact, or a release asset.
4. Rerun the existing tag's Release workflow after adding the secret. On that
   first automated Marketplace publication, verify that the three pending
   updates have the exact ranges and distinct versions above before approving or
   unhiding them. Marketplace review remains external to the workflow.

The Marketplace upload still fails closed when a configured token is rejected
or an artifact upload fails. JetBrains Marketplace onboarding and approval are
external operations; local builds can prove the artifacts and workflow wiring
but cannot manufacture the vendor account, listing, permanent token, or
approval. Those external steps do not hold the CLI, GitHub, PyPI, ZIP, or custom
repository release channels.

## Remote Development and fail-closed behavior

Marketplace installs synchronize the compatible modular update to both Remote
Dev peers. When testing a local ZIP or using the custom-repository fallback,
run `agent-doc plugin install jetbrains` in each peer's filesystem context (or
pass its versioned directory with `--plugins-dir <PATH>`). The CLI derives the
IDE platform build from that path and selects classic 242-261, modular 262, or
modular 263 without guessing. Alternatively, configure the matching repository
URL and install the same update on both the Remote Dev backend and JetBrains
Client/Gateway. Both packages use the
frontend-owned surface snapshot bridge, so
documents detached with Open in New Window retain their frame identity instead
of being flattened into the main window's split columns. A custom repository
does not provide Marketplace's automatic cross-side synchronization.

The modular surface bridge is available only after both processes load the same
plugin generation and establish the authenticated, generation-fenced bridge.
JetBrains' paired-plugin state enforcer classifies a version mismatch as an
inconsistent pair and disables the pair before Agent Doc's RPC can establish a
lease; an Agent Doc timeout or compatibility fallback cannot override that
platform fence. A missing, inactive, or mismatched peer is a one-sided install:
detached-view and frontend presentation effects remain unavailable. While no
accepted frontend snapshot exists, selection and layout publication remain
blocked: JetBrains exposes the client-session editor flow only through a private
API that Marketplace plugins may not link. This degradation does not fabricate
detached-window identities and is not a reason to widen either update's range.
For a Remote Dev `CONTROLLER` session, the delivered selection is also positive
focus evidence: the backend must not require a Swing active window that exists
only in the thin client. Focus-projection gates and task failures are logged at
INFO/WARN before controller transport so a missing publication is diagnosable.

After the bounded frontend startup grace, flat selection evidence without an
authenticated same-generation surface publication is classified
`readiness=blocked` with
`readiness_reason=frontend_authority_absent_or_generation_mismatched` and
`geometry_reason=flat_evidence_has_no_column_geometry`. Structural routing stays
blocked and preserves the existing tmux layout. A retry cannot load an absent or incompatible client
module, and flat evidence cannot safely reconstruct editor columns. Verify that
both peers report the exact same plugin version, restart both, and retry only
after the two-sided install is ready. Marketplace hosting is the automatic way
to synchronize a compatible plugin to the Remote Dev peer; custom-repository and
local-ZIP installs require this explicit two-sided update.

### Virgin Coder workspace with stock Toolbox

There is no backend-initiated stock install that can place an unpublished
split plugin into both processes. JetBrains' plugin synchronization currently
resolves the other peer only through Marketplace metadata. Running
`agent-doc plugin install jetbrains` in a Coder workspace therefore updates the
backend filesystem only; it cannot write the JetBrains Client installation on
the operator's machine. The command prints this scope when it selects a modular
262 or 263 package rather than implying that Remote Dev is ready.

Before Marketplace onboarding is complete, the reliable fallback is explicit
two-sided installation:

1. In the Coder workspace, install the matching modular package into the Remote
   Dev backend with `agent-doc plugin install jetbrains`, or add the matching
   custom-repository URL to the backend plugin manager.
2. In JetBrains Client/Gateway on the operator's machine, use **Tools → Enable
   Agent Doc GitHub Updates** and check for plugin updates, or add the same
   listing under Settings → Plugins → gear → Manage Plugin Repositories.
   Install the same Agent Doc version there. The modular action deliberately
   runs in the frontend process; installing from the backend does not perform
   this step.
3. Confirm that both plugin managers report the identical Agent Doc version,
   restart both processes, and require an authenticated same-generation surface
   publication before treating the split plugin as ready.

This procedure uses stock JetBrains plugin management, but it is manual on each
new Remote Dev environment. Marketplace publication is the only stock path that
automatically synchronizes the compatible package to both peers. Organizations
using JetBrains IDE Services may instead provision a corporate-repository plugin,
but that is an additional managed service rather than the stock Toolbox/Coder
flow.

## Release proof

`scripts/jetbrains-custom-repository.py` reads all three distribution ZIPs, validates
the shared identity, distinct update versions, exact non-overlapping ranges
(`242`..`261.*`, `262`..`262.*`, and `263`..`263.*`), and canonical artifact names,
then emits the three listings. Validation runs before any listing is written, so
a refused set publishes none. The tag workflow repeats that validation with
`--verify-artifacts` immediately before Marketplace upload and writes one
GitHub-release marker after each accepted update so retries can resume safely.
`make jetbrains-repository-self-test` (part of
`make check-fast` and `make check`) proves:

- IntelliJ range semantics (prefix `since-build`, wildcard `until-build`);
- range selection over the generated listings: 242–261 builds receive only the
  classic update, 262 builds receive only the modular update, 263 builds receive
  only the 263 modular update, builds outside all three receive nothing,
  and every branch 242–263 is covered;
- refusal, with no listing written, for equal versions, an overlapping or
  widened range, an open-ended classic range, a foreign plugin ID, a missing
  ranged artifact, swapped or misnamed artifacts, and a non-HTTPS base URL;
- that a build matching two updates is a distribution error, never a choice.

The tag workflow additionally runs each Gradle artifact verifier, verifies both
modular development sandboxes, copies artifacts by declared version instead of
glob, generates the listings, and then, as a separate assertion over the files
it is about to upload, re-reads all listings with `--verify-listings
<classic> <modular> <modern>`: each must carry exactly one update with the declared
version and exact range, and a build sweep from 241 through 264 must select the
classic update for 242–261, the modular update for 262, the modular update
for 263, and nothing otherwise. Finally it asserts the exact three-ZIP/three-listing
shape. The plugin job uploads the packages, listings, VSIX, and their separate
checksum manifest as soon as they verify. The release finalizer depends on that
job, so editor failure blocks the all-platform completion marker without
withholding already successful platform archives.

## Live verification still required

The automated gates above cover artifacts and listings. They cannot prove IDE
behavior, so before calling a release generally installable:

1. On a clean 262 Remote Dev host, add the 262 listing URL to the backend IDE
   and install Agent Doc from it; confirm the installed version is the modular
   update and `lib/modules` holds the shared/frontend/backend JARs.
2. Add the same 262 listing to JetBrains Client/Gateway on the local machine
   and install the same version there; confirm the frontend module loads and
   the surface bridge authenticates (detached-view snapshots reach the
   controller as `editor_view_snapshot_observe`).
3. On a 242 IDE and a 261 IDE, add all listings and confirm only the classic
   update is offered and installed.
4. On the 262 pair, downgrade or remove the plugin on one side only (or install
   a different modular version) and confirm detached-view and frontend effects
   stay unavailable with no classic fallback, and recover once both sides match.
5. On a 263 Remote Dev pair, add the 263 listing to both sides, confirm only the
   263 package is offered, and confirm a document detached with Open in New
   Window does not become a main-window tmux pane.
   Then move a session document to the opposite main-window split and confirm
   tmux pane order follows (GH #320 option B): the client logs
   `[split-surface] snapshot schema negotiated version=2 ... geometry=measured`,
   the backend logs `[rd-layout] forwarded measured columns ... column_order=editor`,
   and ops.log has `pane_layout_desired_published ... column_order=editor`.
6. After the tag workflow runs, confirm the release carries all three ZIPs and
   listings, and that each listing names its tag-pinned artifact and exact range.
