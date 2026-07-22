#!/usr/bin/env bash
# tsift-autoindex-hook-version: 3
# The UI hook only starts this helper. The tsift binary runs after re-exec in a
# detached, debounced, low-priority, workspace-single-flight worker.

if [ "${TSIFT_AUTOINDEX_WORKER:-0}" != "1" ]; then
TSIFT_AUTOINDEX_WORKER=1 nohup "$0" </dev/null >/dev/null 2>&1 &
exit 0
fi

command -v tsift >/dev/null 2>&1 || exit 0
root="/home/brian/work/btakita/agent-loop"
cpu_affinity=""
max_runtime_seconds="${TSIFT_AUTOINDEX_MAX_SECONDS:-120}"
case "$max_runtime_seconds" in
''|*[!0-9]*) max_runtime_seconds=120 ;;
esac
worker_started_seconds=$SECONDS
mkdir -p "$root/.tsift" || exit 0

# Coalesce simultaneous prompts from multiple UI windows. On platforms without
# flock, tsift's native index.lock still prevents competing heavy writers.
if command -v flock >/dev/null 2>&1; then
exec 9>"$root/.tsift/autoindex-hook.lock"
flock -n 9 || exit 0
fi

sleep "${TSIFT_AUTOINDEX_DEBOUNCE_SECONDS:-0.25}"

run_tsift() {
local -a runner=()
local remaining_seconds="$max_runtime_seconds"
if command -v nice >/dev/null 2>&1; then
runner+=(nice -n 10)
fi
if [ "$max_runtime_seconds" != "0" ] && command -v timeout >/dev/null 2>&1; then
remaining_seconds=$((max_runtime_seconds - (SECONDS - worker_started_seconds)))
[ "$remaining_seconds" -gt 0 ] || return 124
runner+=(timeout --signal=TERM --kill-after=5 "${remaining_seconds}s")
fi
if [ -n "$cpu_affinity" ] && command -v taskset >/dev/null 2>&1; then
runner+=(taskset -c "$cpu_affinity")
fi
command "${runner[@]}" tsift "$@"
}

refresh_indexes() {
  run_tsift index --check --exit-code --submodule "agent-doc" "$root" || run_tsift index --submodule "agent-doc" "$root"
  run_tsift index --check --exit-code --submodule "tsift" "$root" || run_tsift index --submodule "tsift" "$root"
}

refresh_indexes
