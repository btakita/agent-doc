#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tools_version="1.8.0"
tools_sha256="58d44845a37a8d776deaf8cf3a623213b59d311bc0ec287bcdfbe148dd11bb3d"
tools_url="https://github.com/tlaplus/tlaplus/releases/download/v${tools_version}/tla2tools.jar"
tools_jar="${TLA_TOOLS_JAR:-${repo_root}/target/tla/tla2tools-${tools_version}.jar}"

if [[ ! -f "${tools_jar}" ]]; then
    command -v curl >/dev/null 2>&1 || {
        echo "[tla] curl is required to download the pinned TLA+ tools" >&2
        exit 1
    }
    mkdir -p "$(dirname "${tools_jar}")"
    partial="${tools_jar}.partial"
    curl --fail --location --silent --show-error "${tools_url}" --output "${partial}"
    printf '%s  %s\n' "${tools_sha256}" "${partial}" | sha256sum --check --status
    mv "${partial}" "${tools_jar}"
fi

printf '%s  %s\n' "${tools_sha256}" "${tools_jar}" | sha256sum --check --status || {
    echo "[tla] checksum mismatch for ${tools_jar}" >&2
    exit 1
}

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/agent-doc-tla.XXXXXX")"
trap 'rm -rf "${work_dir}"' EXIT

modules=(AgentDocCloseout PassiveTmuxSync JetBrainsFileCache CloseoutChurn CrdtLineageFence)
for module in "${modules[@]}"; do
    cp "${repo_root}/formal/tla/${module}.tla" "${work_dir}/"
    cp "${repo_root}/formal/tla/${module}.cfg" "${work_dir}/"
done

(
    cd "${work_dir}"
for module in "${modules[@]}"; do
    if grep -Eq '\(\* --(fair )?algorithm' "${module}.tla"; then
      java -XX:+UseParallelGC -cp "${tools_jar}" pcal.trans "${module}.tla"
    fi
java -XX:+UseParallelGC -cp "${tools_jar}" tlc2.TLC -workers auto "${module}.tla"
done
)
