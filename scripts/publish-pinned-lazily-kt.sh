#!/usr/bin/env bash
# #lz262resolve / #relcimavenlocal: editors/jetbrains-262 resolves
# io.github.lazily:lazily from mavenLocal()+mavenCentral() only, and lazily is
# NOT on Maven Central. Publish the EXACT version the 262 backend pins, from
# its release tag, into mavenLocal so the build bundles the same
# lazily-<v>.jar the released plugin ZIP does. The pin is read from the build
# file so it cannot drift; a classic/262 pin mismatch fails here rather than as
# an opaque resolution error. Shared by ci.yml and release.yml so the two
# workflows cannot diverge again (v0.35.481 failed to publish because only
# ci.yml carried this step).
set -euo pipefail
pin() { sed -n 's/.*implementation("io\.github\.lazily:lazily:\([^"]*\)").*/\1/p' "$1"; }
v262=$(pin editors/jetbrains-262/backend/build.gradle.kts)
vclassic=$(pin editors/jetbrains/build.gradle.kts)
test -n "$v262" || { echo "no io.github.lazily:lazily pin in editors/jetbrains-262/backend/build.gradle.kts"; exit 1; }
test "$v262" = "$vclassic" || { echo "lazily pin mismatch: jetbrains-262=$v262 classic=$vclassic"; exit 1; }
java_home="${JAVA_HOME_21_X64:-${JAVA_HOME:-}}"
test -n "$java_home" || { echo "JDK 21 required: set JAVA_HOME_21_X64 or JAVA_HOME"; exit 1; }
dest="${RUNNER_TEMP:-$(mktemp -d)}/lazily-kt-$v262"
rm -rf "$dest"
git clone --depth 1 --branch "v$v262" https://github.com/lazily-hub/lazily-kt "$dest"
( cd "$dest" && JAVA_HOME="$java_home" ./gradlew --no-daemon --console=plain -q publishToMavenLocal )
test -f "$HOME/.m2/repository/io/github/lazily/lazily/$v262/lazily-$v262.jar"
