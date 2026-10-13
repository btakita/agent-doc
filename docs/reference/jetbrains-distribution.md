# JetBrains plugin distribution

Agent Doc ships three compatibility-ranged updates under the single plugin ID
`com.github.btakita.agent-doc`:

| Update | IDE range | Release artifact |
|---|---|---|
| Classic | `242` through `261.*` | `agent-doc-jetbrains-<pluginVersion>.zip` |
| Plugin Model v2 | exactly `262` through `262.*` | `agent-doc-jetbrains-262-<pluginVersion>.zip` |
| Classic source for 263 | exactly `263` through `263.*` | `agent-doc-jetbrains-263-<pluginVersion>.zip` |

The update versions are distinct. The ranges do not overlap, and all ZIPs are
built and verified before the tag workflow may create a GitHub release. A
missing ZIP, an equal version, an unexpected plugin ID, or a widened range stops
the workflow before publication.

## Production repository URLs

The repository's release automation does not upload these updates to JetBrains
Marketplace. Instead, every stable GitHub release publishes three JetBrains
[simple custom-repository listings](https://plugins.jetbrains.com/docs/intellij/custom-plugin-repository.html):

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

### Marketplace

The equivalent Marketplace path is to upload all three ZIPs as updates of the same
plugin ID, where Marketplace performs the same per-build selection and also
synchronizes the plugin to the Remote Dev peer. Release automation does not
upload to Marketplace today; the custom-repository listings are the supported
production channel until a Marketplace upload step (with its own three-range
assertion) is added.

## Remote Development and fail-closed behavior

For branch 262 Remote Development, configure the 262 repository URL and install
the same modular update on both the Remote Dev backend and JetBrains
Client/Gateway. A custom repository does not provide Marketplace's automatic
cross-side synchronization.

The modular surface bridge is available only after both processes load the same
plugin generation and establish the authenticated, generation-fenced bridge. A
missing, inactive, or mismatched peer is a one-sided install: detached-view and
frontend presentation effects remain unavailable rather than falling back to
the classic single-process path. This is a compatibility refusal, not a reason
to widen either update's range.

## Release proof

`scripts/jetbrains-custom-repository.py` reads all three distribution ZIPs, validates
the shared identity, distinct update versions, exact non-overlapping ranges
(`242`..`261.*`, `262`..`262.*`, and `263`..`263.*`), and canonical artifact names,
then emits the three listings. Validation runs before any listing is written, so
a refused set publishes none. `make jetbrains-repository-self-test` (part of
`make check-fast` and `make check`) proves:

- IntelliJ range semantics (prefix `since-build`, wildcard `until-build`);
- range selection over the generated listings: 242–261 builds receive only the
  classic update, 262 builds receive only the modular update, 263 builds receive
  only the 263 classic-source update, builds outside all three receive nothing,
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
classic update for 242–261, the modular update for 262, the classic-source update
for 263, and nothing otherwise. Finally it asserts the exact three-ZIP/three-listing
shape. The release job depends on that plugin job and uploads all six files together,
so an incomplete release cannot report success.

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
5. On a 263 IDE, add the 263 listing and confirm only the 263 package is offered.
6. After the tag workflow runs, confirm the release carries all three ZIPs and
   listings, and that each listing names its tag-pinned artifact and exact range.
