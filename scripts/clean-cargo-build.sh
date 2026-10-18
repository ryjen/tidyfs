#!/usr/bin/env bash
# Retire only this checkout's generated Cargo outputs after evidence is retained.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
case "${1:-root}" in
  root)
    target="$root/target"
    manifest="$root/Cargo.toml"
    ;;
  fuzz)
    target="$root/fuzz/target"
    manifest="$root/fuzz/Cargo.toml"
    ;;
  *)
    echo "usage: $0 [root|fuzz]" >&2
    exit 2
    ;;
esac
if [[ -L "$target" ]]; then
  echo "refusing to clean a symlinked Cargo target: $target" >&2
  exit 2
fi
if [[ ! -e "$target" ]]; then
  echo "no task-local Cargo build output to clean: $target"
  exit 0
fi
if [[ ! -d "$target" ]]; then
  echo "refusing unexpected non-directory Cargo target: $target" >&2
  exit 2
fi
cargo clean --manifest-path "$manifest" --target-dir "$target"
