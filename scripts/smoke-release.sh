#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 BINARY TARGET VERSION" >&2
  exit 64
fi

binary=$1
target=$2
version=$3

if [[ "$("${binary}" --version)" != "da ${version}" ]]; then
  echo "release binary reports the wrong version" >&2
  exit 1
fi

printf 'ls -la' | "${binary}" --read-only
printf 'echo hello' | "${binary}" --autonomous --allow __da_release_smoke_never_match__

if LC_ALL=C grep -a -q 'version https://git-lfs.github.com/spec/v1' "${binary}"; then
  echo "release binary contains a Git LFS pointer" >&2
  exit 1
fi

case "${target}" in
  *-apple-darwin)
    while IFS= read -r dependency; do
      case "${dependency}" in
        /usr/lib/*|/System/Library/*) ;;
        *)
          echo "non-system macOS dependency: ${dependency}" >&2
          exit 1
          ;;
      esac
    done < <(otool -L "${binary}" | tail -n +2 | awk '{ print $1 }')
    ;;
  *-unknown-linux-gnu)
    linkage=$(ldd "${binary}")
    if grep -q 'not found' <<<"${linkage}"; then
      echo "missing Linux runtime dependency" >&2
      echo "${linkage}" >&2
      exit 1
    fi
    while IFS= read -r dependency; do
      case "${dependency}" in
        /lib/*|/usr/lib/*) ;;
        *)
          echo "non-system Linux dependency: ${dependency}" >&2
          exit 1
          ;;
      esac
    done < <(awk '/=> \// { print $3 } /^\// { print $1 }' <<<"${linkage}")
    ;;
  *)
    echo "unsupported release target: ${target}" >&2
    exit 1
    ;;
esac

echo "smoke-tested da ${version} for ${target}"
