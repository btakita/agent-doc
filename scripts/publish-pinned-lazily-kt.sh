#!/usr/bin/env bash
# #lz262resolve / #relcimavenlocal: editors/jetbrains-262 resolves
# io.github.lazily:lazily from mavenLocal()+mavenCentral() only, and lazily is
# NOT on Maven Central. Publish the EXACT version the 262 backend pins, from
# its release tag, into mavenLocal so the build embeds the same Lazily classes
# the released plugin ZIP does. The pin is read from every dependency
# configuration in the build file and must resolve to one unique version, so
# compileOnly/test/embedded declarations cannot make the release helper lose
# the pin; a classic/262 mismatch fails here rather than as an opaque
# resolution error. Shared by ci.yml and release.yml so the two
# workflows cannot diverge again (v0.35.481 failed to publish because only
# ci.yml carried this step).
set -euo pipefail

pin() {
  local file=$1
  local versions
  versions=$(sed -n 's/.*"io\.github\.lazily:lazily:\([^"]*\)".*/\1/p' "$file" | sort -u)
  if [[ -z "$versions" ]]; then
    echo "no io.github.lazily:lazily pin in $file" >&2
    return 1
  fi
  if [[ "$versions" == *$'\n'* ]]; then
    echo "multiple io.github.lazily:lazily pins in $file: ${versions//$'\n'/, }" >&2
    return 1
  fi
  printf '%s\n' "$versions"
}

main() {
  local v262 vclassic java_home dest
  v262=$(pin editors/jetbrains-262/backend/build.gradle.kts)
  vclassic=$(pin editors/jetbrains/build.gradle.kts)
  test "$v262" = "$vclassic" || { echo "lazily pin mismatch: jetbrains-262=$v262 classic=$vclassic"; exit 1; }
  java_home="${JAVA_HOME_21_X64:-${JAVA_HOME:-}}"
  test -n "$java_home" || { echo "JDK 21 required: set JAVA_HOME_21_X64 or JAVA_HOME"; exit 1; }
  dest="${RUNNER_TEMP:-$(mktemp -d)}/lazily-kt-$v262"
  rm -rf "$dest"
  git clone --depth 1 --branch "v$v262" https://github.com/lazily-hub/lazily-kt "$dest"
  ( cd "$dest" && JAVA_HOME="$java_home" ./gradlew --no-daemon --console=plain -q publishToMavenLocal )
  test -f "$HOME/.m2/repository/io/github/lazily/lazily/$v262/lazily-$v262.jar"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
