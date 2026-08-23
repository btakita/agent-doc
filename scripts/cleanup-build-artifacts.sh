#!/usr/bin/env bash
set -euo pipefail

repo_root=""
if [[ "${1:-}" == "--root" ]]; then
    repo_root="${2:-}"
    shift 2
fi
if [[ $# -ne 0 ]]; then
    echo "usage: $0 [--root <agent-doc-repo>]" >&2
    exit 2
fi
if [[ "${AGENT_DOC_CLEAN_BUILD_ARTIFACTS:-1}" == "0" ]]; then
    echo "Build artifact cleanup disabled (AGENT_DOC_CLEAN_BUILD_ARTIFACTS=0)."
    exit 0
fi
if [[ -z "$repo_root" ]]; then
    repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
else
    repo_root="$(cd "$repo_root" && pwd -P)"
fi
if [[ "$repo_root" == "/" || ! -f "$repo_root/Cargo.toml" ]]; then
    echo "refusing build cleanup outside an agent-doc repository: $repo_root" >&2
    exit 1
fi

targets=(
    "$repo_root/target"
    "$repo_root/editors/jetbrains/build"
)
for candidate in "$repo_root"/.agent-doc-build-*; do
    [[ -e "$candidate" ]] || continue
    name="${candidate##*/}"
    if [[ "$name" =~ ^\.agent-doc-build-[A-Za-z0-9._-]+$ ]]; then
        targets+=("$candidate")
    else
        echo "refusing unexpected build scratch path: $candidate" >&2
        exit 1
    fi
done

reclaimed_kib=0
removed=0
for target in "${targets[@]}"; do
    [[ -e "$target" ]] || continue
    case "$target" in
        "$repo_root/target"|"$repo_root/editors/jetbrains/build"|"$repo_root"/.agent-doc-build-*) ;;
        *)
            echo "refusing out-of-scope build cleanup target: $target" >&2
            exit 1
    ;;
  esac
  size_kib="$(du -sk "$target" 2>/dev/null | awk '{print $1}')"
  reclaimed_kib=$((reclaimed_kib + ${size_kib:-0}))
  cleanup_dir="$(mktemp -d "$repo_root/.agent-doc-build-cleanup.XXXXXX")"
  staged_target="$cleanup_dir/payload"
  if ! mv -- "$target" "$staged_target"; then
    rm -rf -- "$cleanup_dir"
    echo "failed to detach build artifact path before cleanup: $target" >&2
    exit 1
  fi
  if ! rm -rf -- "$cleanup_dir"; then
    echo "failed to remove detached build artifact generation: $cleanup_dir" >&2
    exit 1
  fi
  removed=$((removed + 1))
done

echo "Removed $removed repo-owned build artifact path(s); reclaimed approximately $((reclaimed_kib / 1024)) MiB."
echo "Cargo dependency caches were preserved."
