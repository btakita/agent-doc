#!/usr/bin/env python3
"""Build the two static JetBrains custom-repository listings for a release.

JetBrains' simple custom-repository format permits a plugin ID only once per
listing.  Agent Doc therefore publishes one listing for the classic update and
one for the exact-262 modular update.  The generator reads the patched
``plugin.xml`` from each distribution ZIP and refuses to publish unless the two
artifacts have the shared ID, distinct versions, and the expected disjoint IDE
ranges.
"""

from __future__ import annotations

import argparse
import io
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import quote, urlparse


PLUGIN_ID = "com.github.btakita.agent-doc"
CLASSIC_RANGE = ("242", "261.*")
MODULAR_RANGE = ("262", "262.*")
CLASSIC_LISTING = "agent-doc-jetbrains-classic.xml"
MODULAR_LISTING = "agent-doc-jetbrains-262.xml"


class DistributionError(ValueError):
    """The release artifacts cannot form a safe two-update repository."""


@dataclass(frozen=True)
class PluginUpdate:
    artifact: Path
    plugin_id: str
    version: str
    since_build: str
    until_build: str


def _text(root: ET.Element, name: str) -> str:
    value = root.findtext(name)
    if value is None or not value.strip():
        raise DistributionError(f"plugin descriptor is missing <{name}>")
    return value.strip()


def _descriptor_from_distribution(path: Path) -> ET.Element:
    if not path.is_file():
        raise DistributionError(f"plugin distribution does not exist: {path}")

    matches: list[ET.Element] = []
    try:
        with zipfile.ZipFile(path) as distribution:
            for member in distribution.namelist():
                if not member.endswith(".jar"):
                    continue
                try:
                    with zipfile.ZipFile(io.BytesIO(distribution.read(member))) as jar:
                        try:
                            descriptor = ET.fromstring(jar.read("META-INF/plugin.xml"))
                        except KeyError:
                            continue
                except zipfile.BadZipFile as error:
                    raise DistributionError(
                        f"{path}: nested plugin artifact is not a valid JAR: {member}"
                    ) from error
                if descriptor.findtext("id", "").strip() == PLUGIN_ID:
                    matches.append(descriptor)
    except zipfile.BadZipFile as error:
        raise DistributionError(f"plugin distribution is not a valid ZIP: {path}") from error

    if len(matches) != 1:
        raise DistributionError(
            f"{path}: expected exactly one {PLUGIN_ID} plugin.xml, found {len(matches)}"
        )
    return matches[0]


def read_update(path: Path) -> PluginUpdate:
    descriptor = _descriptor_from_distribution(path)
    idea_version = descriptor.find("idea-version")
    if idea_version is None:
        raise DistributionError(f"{path}: plugin descriptor is missing <idea-version>")
    since_build = idea_version.get("since-build", "").strip()
    until_build = idea_version.get("until-build", "").strip()
    if not since_build or not until_build:
        raise DistributionError(
            f"{path}: idea-version must declare both since-build and until-build"
        )
    return PluginUpdate(
        artifact=path,
        plugin_id=_text(descriptor, "id"),
        version=_text(descriptor, "version"),
        since_build=since_build,
        until_build=until_build,
    )


def _parse_build(build: str) -> tuple[int, ...]:
    """Parse a JetBrains build number (``262``, ``261.22158.8``) to a tuple."""
    parts = build.strip().split(".")
    if not parts or any(not part.isdigit() for part in parts):
        raise DistributionError(f"not a concrete IDE build number: {build!r}")
    return tuple(int(part) for part in parts)


def _bound_parts(bound: str) -> tuple[tuple[int, ...], bool]:
    """Split a since/until bound into its numeric prefix and wildcard flag."""
    parts = bound.strip().split(".")
    wildcard = parts[-1] == "*"
    if wildcard:
        parts = parts[:-1]
    if not parts or any(not part.isdigit() for part in parts):
        raise DistributionError(f"malformed compatibility bound: {bound!r}")
    return tuple(int(part) for part in parts), wildcard


def build_in_range(build: str, since_build: str, until_build: str) -> bool:
    """Apply the IntelliJ compatibility rule for one build against one range.

    ``since-build`` is inclusive by prefix (``262`` admits ``262.1``);
    ``until-build`` ending in ``*`` admits every build sharing its prefix.
    """
    value = _parse_build(build)
    since, since_wild = _bound_parts(since_build)
    if since_wild:
        raise DistributionError(f"since-build may not be a wildcard: {since_build!r}")
    if value[: len(since)] < since:
        return False
    until, until_wild = _bound_parts(until_build)
    if until_wild:
        return value[: len(until)] <= until
    return value <= until


def ranges_overlap(first: PluginUpdate, second: PluginUpdate) -> bool:
    """True when any branch build could satisfy both ranges.

    Each range is reduced to the closed interval of IDE branches it can admit;
    the two updates share a plugin ID, so any shared branch would let the IDE
    pick either ZIP and is refused.
    """

    def branches(update: PluginUpdate) -> tuple[int, int]:
        since, _ = _bound_parts(update.since_build)
        until, _ = _bound_parts(update.until_build)
        return since[0], until[0]

    a_low, a_high = branches(first)
    b_low, b_high = branches(second)
    return a_low <= b_high and b_low <= a_high


def select_update(build: str, updates: list[PluginUpdate]) -> PluginUpdate | None:
    """Return the single update an IDE at ``build`` may install, or ``None``.

    More than one match is a distribution defect, never a choice.
    """
    matches = [
        update
        for update in updates
        if build_in_range(build, update.since_build, update.until_build)
    ]
    if len(matches) > 1:
        raise DistributionError(
            f"IDE build {build} matches {len(matches)} updates of {PLUGIN_ID}"
        )
    return matches[0] if matches else None


def validate_pair(classic: PluginUpdate, modular: PluginUpdate) -> None:
    for label, update, expected_range in (
        ("classic", classic, CLASSIC_RANGE),
        ("modular", modular, MODULAR_RANGE),
    ):
        if update.plugin_id != PLUGIN_ID:
            raise DistributionError(
                f"{label} plugin ID {update.plugin_id!r} does not match {PLUGIN_ID!r}"
            )
        actual_range = (update.since_build, update.until_build)
        if actual_range != expected_range:
            raise DistributionError(
                f"{label} range {actual_range!r} does not match {expected_range!r}"
            )

    if ranges_overlap(classic, modular):
        raise DistributionError(
            "classic and modular compatibility ranges overlap under the shared plugin ID"
        )

    if classic.version == modular.version:
        raise DistributionError(
            "classic and modular updates must have distinct versions under the shared plugin ID"
        )

    expected_classic = f"agent-doc-jetbrains-{classic.version}.zip"
    expected_modular = f"agent-doc-jetbrains-262-{modular.version}.zip"
    for label, update, expected_name in (
        ("classic", classic, expected_classic),
        ("modular", modular, expected_modular),
    ):
        if update.artifact.name != expected_name:
            raise DistributionError(
                f"{label} artifact must be named {expected_name!r}, got {update.artifact.name!r}"
            )


def _download_url(base_url: str, artifact: Path) -> str:
    parsed = urlparse(base_url)
    if parsed.scheme != "https" or not parsed.netloc:
        raise DistributionError("download base URL must be an absolute HTTPS URL")
    return f"{base_url.rstrip('/')}/{quote(artifact.name)}"


def listing(update: PluginUpdate, base_url: str) -> bytes:
    plugins = ET.Element("plugins")
    plugin = ET.SubElement(
        plugins,
        "plugin",
        {
            "id": update.plugin_id,
            "url": _download_url(base_url, update.artifact),
            "version": update.version,
        },
    )
    ET.SubElement(
        plugin,
        "idea-version",
        {"since-build": update.since_build, "until-build": update.until_build},
    )
    ET.indent(plugins, space="  ")
    return b'<?xml version="1.0" encoding="UTF-8"?>\n' + ET.tostring(
        plugins, encoding="utf-8", short_empty_elements=True
    ) + b"\n"


def generate(
    classic_path: Path,
    modular_path: Path,
    output_dir: Path,
    download_base_url: str,
) -> tuple[Path, Path]:
    classic = read_update(classic_path)
    modular = read_update(modular_path)
    validate_pair(classic, modular)

    output_dir.mkdir(parents=True, exist_ok=True)
    classic_listing = output_dir / CLASSIC_LISTING
    modular_listing = output_dir / MODULAR_LISTING
    classic_listing.write_bytes(listing(classic, download_base_url))
    modular_listing.write_bytes(listing(modular, download_base_url))
    return classic_listing, modular_listing


def read_listing(path: Path) -> PluginUpdate:
    """Parse a generated listing back into the one update it advertises."""
    if not path.is_file():
        raise DistributionError(f"custom-repository listing does not exist: {path}")
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError as error:
        raise DistributionError(f"{path.name}: listing is not valid XML") from error
    entries = root.findall("plugin") if root.tag == "plugins" else []
    if len(entries) != 1:
        raise DistributionError(
            f"{path.name}: a listing must carry exactly one update, found {len(entries)}"
        )
    entry = entries[0]
    idea_version = entry.find("idea-version")
    if idea_version is None:
        raise DistributionError(f"{path.name}: update is missing <idea-version>")
    url = entry.get("url", "")
    return PluginUpdate(
        artifact=Path(urlparse(url).path),
        plugin_id=entry.get("id", ""),
        version=entry.get("version", ""),
        since_build=idea_version.get("since-build", ""),
        until_build=idea_version.get("until-build", ""),
    )


# Representative builds per branch for the published-listing sweep: the first
# classic branch, every intermediate branch, and the exact-262 branch, plus the
# out-of-range neighbours on both sides.
SWEEP_BUILDS = [f"{branch}.1" for branch in range(241, 264)] + ["242", "261.99999.99", "262"]


def verify_listings(output_dir: Path, classic_version: str, modular_version: str) -> None:
    """Re-read both published listings and prove the two-sided selection.

    Independent of :func:`generate`: the release workflow runs this as its own
    assertion over the files it is about to upload.
    """
    classic = read_listing(output_dir / CLASSIC_LISTING)
    modular = read_listing(output_dir / MODULAR_LISTING)
    validate_pair(classic, modular)
    if (classic.version, modular.version) != (classic_version, modular_version):
        raise DistributionError(
            "listings advertise versions "
            f"{(classic.version, modular.version)!r}, expected "
            f"{(classic_version, modular_version)!r}"
        )
    for build in SWEEP_BUILDS:
        branch = _parse_build(build)[0]
        chosen = select_update(build, [classic, modular])
        expected = classic if 242 <= branch <= 261 else modular if branch == 262 else None
        if chosen != expected:
            raise DistributionError(
                f"IDE build {build} would select "
                f"{chosen.version if chosen else None!r}, expected "
                f"{expected.version if expected else None!r}"
            )


def _fixture_distribution(
    path: Path,
    *,
    version: str,
    since_build: str,
    until_build: str | None,
    plugin_id: str = PLUGIN_ID,
) -> None:
    until = f' until-build="{until_build}"' if until_build is not None else ""
    descriptor = f"""<idea-plugin>
  <id>{plugin_id}</id>
  <name>Agent Doc</name>
  <version>{version}</version>
  <idea-version since-build="{since_build}"{until}/>
</idea-plugin>
""".encode()
    jar_bytes = io.BytesIO()
    with zipfile.ZipFile(jar_bytes, "w") as jar:
        jar.writestr("META-INF/plugin.xml", descriptor)
    with zipfile.ZipFile(path, "w") as distribution:
        distribution.writestr("agent-doc/lib/agent-doc.jar", jar_bytes.getvalue())


BASE_URL = "https://example.invalid/releases/download/v0.0.0"


def _expect_refusal(root: Path, label: str, needle: str, classic: Path, modular: Path) -> None:
    output = root / f"rejected-{label}"
    try:
        generate(classic, modular, output, BASE_URL)
    except DistributionError as error:
        assert needle in str(error), f"{label}: unexpected refusal {error}"
    else:
        raise AssertionError(f"{label}: the pair must be refused")
    # Fail closed: a refused pair publishes neither listing, so one side can
    # never be live without the other.
    assert not (output / CLASSIC_LISTING).exists(), label
    assert not (output / MODULAR_LISTING).exists(), label


def self_test() -> None:
    # IntelliJ range semantics.
    assert build_in_range("242", "242", "261.*")
    assert build_in_range("242.20224.300", "242", "261.*")
    assert build_in_range("261.99999.1", "242", "261.*")
    assert not build_in_range("262.1", "242", "261.*")
    assert not build_in_range("241.18034.62", "242", "261.*")
    assert build_in_range("262.4852.1", "262", "262.*")
    assert not build_in_range("263.1", "262", "262.*")
    assert not build_in_range("261.1", "262", "262.*")

    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp)
        classic = root / "agent-doc-jetbrains-1.2.3.zip"
        modular = root / "agent-doc-jetbrains-262-1.2.4.zip"
        _fixture_distribution(classic, version="1.2.3", since_build="242", until_build="261.*")
        _fixture_distribution(modular, version="1.2.4", since_build="262", until_build="262.*")
        outputs = generate(classic, modular, root / "repository", BASE_URL)
        assert tuple(path.name for path in outputs) == (CLASSIC_LISTING, MODULAR_LISTING)

        published = [read_listing(path) for path in outputs]
        for update, version, expected_range, artifact in (
            (published[0], "1.2.3", CLASSIC_RANGE, classic.name),
            (published[1], "1.2.4", MODULAR_RANGE, modular.name),
        ):
            assert update.plugin_id == PLUGIN_ID
            assert update.version == version
            assert (update.since_build, update.until_build) == expected_range
            assert update.artifact.name == artifact
        assert not ranges_overlap(published[0], published[1])

        # Range selection over the published listings: 242-261 receive only
        # the classic update, 262 receives only the modular update, and builds
        # outside both ranges are offered nothing.
        for build, expected in (
            ("241.19416.15", None),
            ("242", "1.2.3"),
            ("243.21565.193", "1.2.3"),
            ("251.23774.435", "1.2.3"),
            ("261.99999.99", "1.2.3"),
            ("262", "1.2.4"),
            ("262.4852.1", "1.2.4"),
            ("263.1", None),
        ):
            chosen = select_update(build, published)
            assert (chosen.version if chosen else None) == expected, (build, chosen)
        for branch in range(242, 263):
            assert select_update(f"{branch}.1", published) is not None, branch

        def fixture(name: str, **kwargs: object) -> Path:
            path = root / name
            _fixture_distribution(path, **kwargs)  # type: ignore[arg-type]
            return path

        _expect_refusal(
            root, "equal-version", "distinct versions", classic,
            fixture("agent-doc-jetbrains-262-1.2.3.zip", version="1.2.3",
                    since_build="262", until_build="262.*"),
        )
        _expect_refusal(
            root, "modular-overlap", "modular range", classic,
            fixture("agent-doc-jetbrains-262-1.2.5.zip", version="1.2.5",
                    since_build="261", until_build="262.*"),
        )
        _expect_refusal(
            root, "classic-widened", "classic range",
            fixture("agent-doc-jetbrains-1.2.6.zip", version="1.2.6",
                    since_build="242", until_build="262.*"),
            modular,
        )
        _expect_refusal(
            root, "classic-open-ended", "until-build",
            fixture("agent-doc-jetbrains-1.2.7.zip", version="1.2.7",
                    since_build="242", until_build=None),
            modular,
        )
        _expect_refusal(
            root, "id-mismatch", "expected exactly one",
            classic,
            fixture("agent-doc-jetbrains-262-1.2.8.zip", version="1.2.8",
                    since_build="262", until_build="262.*",
                    plugin_id="com.example.other"),
        )
        _expect_refusal(
            root, "one-sided", "does not exist",
            classic, root / "agent-doc-jetbrains-262-9.9.9.zip",
        )
        _expect_refusal(root, "swapped", "classic range", modular, classic)
        _expect_refusal(
            root, "misnamed", "must be named",
            classic,
            fixture("agent-doc-jetbrains-1.3.0.zip", version="1.3.0",
                    since_build="262", until_build="262.*"),
        )

        # The workflow's independent listing assertion accepts the real pair and
        # refuses tampered or mismatched listings.
        verify_listings(root / "repository", "1.2.3", "1.2.4")
        for label, mutate, needle in (
            ("wrong-version", None, "listings advertise versions"),
            ("widened", ('until-build="261.*"', 'until-build="262.*"'), "classic range"),
            ("duplicated", ("</plugins>", '<plugin id="x" url="https://e/x.zip" version="9"><idea-version since-build="262" until-build="262.*"/></plugin></plugins>'), "exactly one update"),
        ):
            tampered = root / f"tampered-{label}"
            tampered.mkdir()
            for path in outputs:
                (tampered / path.name).write_bytes(path.read_bytes())
            if mutate is not None:
                target = tampered / CLASSIC_LISTING
                text = target.read_text()
                assert mutate[0] in text, label
                target.write_text(text.replace(mutate[0], mutate[1]))
            expected_versions = ("1.2.3", "9.9.9") if mutate is None else ("1.2.3", "1.2.4")
            try:
                verify_listings(tampered, *expected_versions)
            except DistributionError as error:
                assert needle in str(error), f"{label}: unexpected refusal {error}"
            else:
                raise AssertionError(f"{label}: tampered listings must be refused")
        missing = root / "tampered-one-sided"
        missing.mkdir()
        (missing / CLASSIC_LISTING).write_bytes(outputs[0].read_bytes())
        try:
            verify_listings(missing, "1.2.3", "1.2.4")
        except DistributionError as error:
            assert "does not exist" in str(error)
        else:
            raise AssertionError("a one-sided listing set must be refused")

        # Overlap detection is independent of the exact-range pins.
        overlap = [
            PluginUpdate(classic, PLUGIN_ID, "1", "242", "262.*"),
            PluginUpdate(modular, PLUGIN_ID, "2", "262", "262.*"),
        ]
        assert ranges_overlap(overlap[0], overlap[1])
        try:
            select_update("262.1", overlap)
        except DistributionError as error:
            assert "matches 2 updates" in str(error)
        else:
            raise AssertionError("an ambiguous build must be refused")

        try:
            generate(classic, modular, root / "http", "http://example.invalid/x")
        except DistributionError as error:
            assert "HTTPS" in str(error)
        else:
            raise AssertionError("a non-HTTPS download base must be refused")

    print("jetbrains custom repository self-test: ok")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--classic", type=Path)
    parser.add_argument("--modular", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--download-base-url")
    parser.add_argument(
        "--verify-listings",
        nargs=2,
        metavar=("CLASSIC_VERSION", "MODULAR_VERSION"),
        help="re-read the listings in --output-dir and assert the two-sided selection",
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0
    if args.verify_listings:
        if args.output_dir is None:
            parser.error("--verify-listings requires --output-dir")
        try:
            verify_listings(args.output_dir, *args.verify_listings)
        except DistributionError as error:
            print(f"jetbrains custom repository: {error}", file=sys.stderr)
            return 1
        print("jetbrains custom repository: both ranged listings verified")
        return 0
    missing = [
        flag
        for flag, value in (
            ("--classic", args.classic),
            ("--modular", args.modular),
            ("--output-dir", args.output_dir),
            ("--download-base-url", args.download_base_url),
        )
        if value is None
    ]
    if missing:
        parser.error(f"required unless --self-test: {', '.join(missing)}")
    try:
        outputs = generate(
            args.classic,
            args.modular,
            args.output_dir,
            args.download_base_url,
        )
    except DistributionError as error:
        print(f"jetbrains custom repository: {error}", file=sys.stderr)
        return 1
    for output in outputs:
        print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
