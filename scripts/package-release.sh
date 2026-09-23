#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 TARGET BINARY OUTPUT_DIR" >&2
  exit 64
fi

target=$1
binary=$2
output_dir=$3
name="da-${target}"
archive="${output_dir}/${name}.tar.gz"

mkdir -p "${output_dir}"
stage_root=$(mktemp -d "${output_dir}/${name}.stage.XXXXXX")
trap 'rm -rf "${stage_root}"' EXIT
stage="${stage_root}/${name}"
mkdir -p "${stage}/bin" "${stage}/share/da"
cp "${binary}" "${stage}/bin/da"
cp LICENSE README.md "${stage}/share/da/"
tar -C "${stage_root}" -czf "${archive}" "${name}"
shasum -a 256 "${archive}" | sed "s#  ${output_dir}/#  #" > "${archive}.sha256"

echo "${archive}"
