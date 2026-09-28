#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# v1.8.0 is the upstream rolling prerelease: its tag and release asset are
# replaced by CI, so a pinned checksum eventually rejects a different binary at
# the same URL. Use the immutable stable release for reproducible model checks.
tools_version="1.7.4"
tools_sha256="936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
tools_url="https://github.com/tlaplus/tlaplus/releases/download/v${tools_version}/tla2tools.jar"
tools_jar="${TLA_TOOLS_JAR:-${repo_root}/target/tla/tla2tools-${tools_version}.jar}"

if [[ -f "${tools_jar}" ]] && ! printf '%s  %s\n' "${tools_sha256}" "${tools_jar}" | sha256sum --check --status; then
  echo "[tla] cached tools checksum changed; refreshing ${tools_jar}" >&2
  rm -f "${tools_jar}"
fi

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

modules=(AgentDocCloseout PassiveTmuxSync JetBrainsFileCache CloseoutChurn CrdtLineageFence ResponseCheckpoint PaneExecutionAuthority SupervisorGenerationTransition ReactiveTopology EditorReplicaStrand TransientRefusalLatch SubmoduleFocusRoot)

# Non-vacuity obligations. Each entry is `Module:Config` that MUST be reported as
# a violation. A safety or liveness property that cannot fail is not evidence,
# and this harness had been certifying exactly that: `JetBrainsFileCache` proved
# `EventuallyConverged` while production wedged for months, because its
# reregister step was an unconditional assignment that could not be rejected.
#
# There are two kinds of obligation here, and a module that models a recovery
# path wants both:
#
#   * a WEDGE config disables the recovery edge and must violate the liveness
#     property - proof that the edge is load-bearing rather than decorative;
#   * a REACH config asserts the negation of the desired end state and must be
#     violated too - proof that the happy path is still reachable, so a
#     conditional property cannot pass because its antecedent never holds.
#
# Together they mean the positive run is checking something. Either one alone
# leaves a way to go vacuous.
must_violate=(
    EditorReplicaStrand:EditorReplicaStrandWedge
    JetBrainsFileCache:JetBrainsFileCacheWedge
    JetBrainsFileCache:JetBrainsFileCacheReach
    # `TransientRefusalLatch` has one wedge PER KNOB rather than one for the
    # module. The three knobs are independent fixes for the same class, so a
    # single wedge would let two of them go vacuous the moment the third landed.
    TransientRefusalLatch:TransientRefusalLatchUnclassified
    TransientRefusalLatch:TransientRefusalLatchBlindRetry
    TransientRefusalLatch:TransientRefusalLatchCrashAsVerdict
    TransientRefusalLatch:TransientRefusalLatchReach
    # `SubmoduleFocusRoot` needs both: the wedge proves the owning-root
    # resolution edge is load-bearing (without it the nested document never
    # focuses), and the reach proves both in-scope documents actually do focus,
    # so the liveness property cannot pass on an antecedent that never holds.
    SubmoduleFocusRoot:SubmoduleFocusRootWedge
    SubmoduleFocusRoot:SubmoduleFocusRootReach
)

for module in "${modules[@]}"; do
    cp "${repo_root}/formal/tla/${module}.tla" "${work_dir}/"
    cp "${repo_root}/formal/tla/${module}.cfg" "${work_dir}/"
done
for entry in "${must_violate[@]}"; do
    cp "${repo_root}/formal/tla/${entry#*:}.cfg" "${work_dir}/"
done

(
    cd "${work_dir}"
for module in "${modules[@]}"; do
    if grep -Eq '\(\* --(fair )?algorithm' "${module}.tla"; then
      java -XX:+UseParallelGC -cp "${tools_jar}" pcal.trans "${module}.tla"
    fi
# `-metadir` is explicit because TLC otherwise derives it from the current time
# to the SECOND, and two checks that start inside the same second collide with
# "that directory already exists". With the same module now checked under several
# configs that is not a rare race, it is the common case.
java -XX:+UseParallelGC -cp "${tools_jar}" tlc2.TLC -workers auto \
    -metadir "${work_dir}/states-${module}" "${module}.tla"
done

for entry in "${must_violate[@]}"; do
    module="${entry%%:*}"
    config="${entry#*:}"
    log="${module}-${config}.log"
    # `set -e` is active, so guard the intentionally-failing run.
    if java -XX:+UseParallelGC -cp "${tools_jar}" tlc2.TLC -workers auto \
        -metadir "${work_dir}/states-${module}-${config}" \
        -config "${config}.cfg" "${module}.tla" >"${log}" 2>&1; then
        echo "[tla] NON-VACUITY FAILURE: ${module} with ${config}.cfg was expected to" >&2
        echo "[tla] report a violation and instead passed. The recovery edge that" >&2
        echo "[tla] config disables is no longer load-bearing, or the property no" >&2
        echo "[tla] longer constrains it - the positive run is now vacuous." >&2
        tail -40 "${log}" >&2
        exit 1
    fi
    if ! grep -Eq 'Error: (Deadlock reached|Temporal properties were violated|Invariant .* is violated)' "${log}"; then
        echo "[tla] NON-VACUITY FAILURE: ${module} with ${config}.cfg failed, but not" >&2
        echo "[tla] with a deadlock / property violation - so the wedge it is meant to" >&2
        echo "[tla] exhibit was not what TLC actually reported." >&2
        tail -40 "${log}" >&2
        exit 1
    fi
    echo "[tla] non-vacuity confirmed: ${module} with ${config}.cfg violates as required"
done
)
