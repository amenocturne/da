#!/usr/bin/env bash
set -euo pipefail

model=classifier/model.onnx
pointer=$(git show "HEAD:${model}")
expected_oid=$(awk '/^oid sha256:/ { sub(/^oid sha256:/, ""); print }' <<<"${pointer}")
expected_size=$(awk '/^size / { print $2 }' <<<"${pointer}")

if [[ -z "${expected_oid}" || -z "${expected_size}" ]]; then
  echo "${model} is not recorded as a Git LFS object" >&2
  exit 1
fi

git lfs fsck

actual_oid=$(shasum -a 256 "${model}" | awk '{ print $1 }')
actual_size=$(wc -c < "${model}" | tr -d ' ')

if [[ "${actual_oid}" != "${expected_oid}" || "${actual_size}" != "${expected_size}" ]]; then
  echo "${model} does not match its Git LFS pointer" >&2
  echo "expected sha256=${expected_oid} size=${expected_size}" >&2
  echo "actual   sha256=${actual_oid} size=${actual_size}" >&2
  exit 1
fi

echo "verified ${model}: sha256=${actual_oid} size=${actual_size}"
