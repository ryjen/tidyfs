#!/usr/bin/env bash
set -euo pipefail

crate_name="tidyfs"
target="${RELEASE_TARGET:-x86_64-unknown-linux-gnu}"

version="$({ cargo metadata --no-deps --format-version 1; } | python3 -c 'import json, sys; data = json.load(sys.stdin); print(data["packages"][0]["version"])')"
expected_tag="v${version}"
release_tag="${RELEASE_TAG:-}"

if [[ -n "${release_tag}" ]]; then
  if [[ ! "${release_tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "release tag must match vX.Y.Z: ${release_tag}" >&2
    exit 1
  fi
  if [[ "${release_tag}" != "${expected_tag}" ]]; then
    echo "release tag ${release_tag} does not match Cargo.toml version ${expected_tag}" >&2
    exit 1
  fi
fi

host="$(rustc -vV | sed -n 's/^host: //p')"
if [[ "${host}" != "${target}" ]]; then
  echo "release target ${target} does not match Rust host ${host}" >&2
  exit 1
fi

cargo build --release --locked

bundle="${crate_name}-${version}-${target}"
dist_dir="dist"
bundle_dir="${dist_dir}/${bundle}"
archive="${dist_dir}/${bundle}.tar.gz"
checksum="${archive}.sha256"

rm -rf "${dist_dir}"
mkdir -p "${bundle_dir}"
cp "target/release/${crate_name}" "${bundle_dir}/${crate_name}"
cp README.md LICENSE-MIT LICENSE-APACHE "${bundle_dir}/"

source_date_epoch="$(git log -1 --format=%ct)"
tar \
  --sort=name \
  --mtime="@${source_date_epoch}" \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -C "${dist_dir}" \
  -cf - \
  "${bundle}" | gzip -n > "${archive}"

(
  cd "${dist_dir}"
  sha256sum "${bundle}.tar.gz" > "${bundle}.tar.gz.sha256"
)

actual_contents="$(mktemp)"
expected_contents="$(mktemp)"
trap 'rm -f "${actual_contents}" "${expected_contents}"' EXIT

tar -tzf "${archive}" | sort > "${actual_contents}"
printf '%s\n' \
  "${bundle}/" \
  "${bundle}/LICENSE-APACHE" \
  "${bundle}/LICENSE-MIT" \
  "${bundle}/README.md" \
  "${bundle}/${crate_name}" | sort > "${expected_contents}"

diff -u "${expected_contents}" "${actual_contents}"
sha256sum --check "${checksum}"

echo "verified ${archive}"
echo "verified ${checksum}"
