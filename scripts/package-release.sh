#!/usr/bin/env bash
set -euo pipefail

crate_name="tidyfs"
target="${RELEASE_TARGET:-x86_64-unknown-linux-musl}"

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

case "${target}" in
  x86_64-unknown-linux-musl) ;;
  *)
    echo "unsupported release target: ${target}; distributable Linux releases must use x86_64-unknown-linux-musl" >&2
    exit 1
    ;;
esac

cargo build --release --locked --target "${target}"

binary="target/${target}/release/${crate_name}"
if [[ ! -x "${binary}" ]]; then
  echo "release binary missing or not executable: ${binary}" >&2
  exit 1
fi

command -v readelf >/dev/null || {
  echo "readelf is required to verify release portability" >&2
  exit 1
}

program_headers="$(readelf -l "${binary}" 2>&1)"
if grep -q 'INTERP' <<<"${program_headers}"; then
  echo "release binary unexpectedly contains a dynamic ELF interpreter" >&2
  exit 1
fi

# A static PIE may still contain a dynamic section for relocations. Runtime
# portability depends on the absence of shared-library and runtime search-path
# dependencies, not on arbitrary diagnostic/build strings embedded in the file.
dynamic_section="$(readelf -d "${binary}" 2>&1)"
if grep -Eq '\((NEEDED|RPATH|RUNPATH)\)' <<<"${dynamic_section}"; then
  echo "release binary unexpectedly contains a dynamic runtime dependency or search path" >&2
  printf '%s\n' "${dynamic_section}" >&2
  exit 1
fi

"${binary}" --version >/dev/null

bundle="${crate_name}-${version}-${target}"
dist_dir="dist"
bundle_dir="${dist_dir}/${bundle}"
archive="${dist_dir}/${bundle}.tar.gz"
checksum="${archive}.sha256"

rm -rf "${dist_dir}"
mkdir -p "${bundle_dir}/bin" "${bundle_dir}/share/man/man1"
cp "${binary}" "${bundle_dir}/bin/${crate_name}"
ln -s "bin/${crate_name}" "${bundle_dir}/${crate_name}"
cp "man/${crate_name}.1" "${bundle_dir}/share/man/man1/${crate_name}.1"
cp README.md LICENSE-MIT LICENSE-APACHE "${bundle_dir}/"

test -x "${bundle_dir}/bin/${crate_name}"
test -L "${bundle_dir}/${crate_name}"
test "$(readlink "${bundle_dir}/${crate_name}")" = "bin/${crate_name}"
test -x "${bundle_dir}/${crate_name}"
test -r "${bundle_dir}/share/man/man1/${crate_name}.1"
"${bundle_dir}/bin/${crate_name}" --help >/dev/null
"${bundle_dir}/bin/${crate_name}" --version | grep -Fx "${crate_name} ${version}" >/dev/null
MANPATH="${bundle_dir}/share/man" man -w "${crate_name}" >/dev/null

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
extract_dir="$(mktemp -d)"
trap 'rm -f "$actual_contents" "$expected_contents"; rm -rf "$extract_dir"' EXIT

tar -tzf "${archive}" | sort > "${actual_contents}"
printf '%s\n' \
  "${bundle}/" \
  "${bundle}/LICENSE-APACHE" \
  "${bundle}/LICENSE-MIT" \
  "${bundle}/README.md" \
  "${bundle}/${crate_name}" \
  "${bundle}/bin/" \
  "${bundle}/bin/${crate_name}" \
  "${bundle}/share/" \
  "${bundle}/share/man/" \
  "${bundle}/share/man/man1/" \
  "${bundle}/share/man/man1/${crate_name}.1" | sort > "${expected_contents}"

diff -u "${expected_contents}" "${actual_contents}"
(
  cd "${dist_dir}"
  sha256sum --check "${bundle}.tar.gz.sha256"
)

tar -xzf "${archive}" -C "${extract_dir}"
extracted_bundle="${extract_dir}/${bundle}"
test -x "${extracted_bundle}/bin/${crate_name}"
test -L "${extracted_bundle}/${crate_name}"
test "$(readlink "${extracted_bundle}/${crate_name}")" = "bin/${crate_name}"
test -x "${extracted_bundle}/${crate_name}"
test -r "${extracted_bundle}/share/man/man1/${crate_name}.1"
env -i PATH=/usr/bin:/bin "${extracted_bundle}/bin/${crate_name}" --version | grep -Fx "${crate_name} ${version}" >/dev/null
env -i PATH=/usr/bin:/bin "${extracted_bundle}/${crate_name}" --version | grep -Fx "${crate_name} ${version}" >/dev/null
MANPATH="${extracted_bundle}/share/man" man -w "${crate_name}" >/dev/null

echo "verified static portable binary ${binary}"
echo "verified extracted artifact contract ${bundle}"
echo "verified ${archive}"
echo "verified ${checksum}"
