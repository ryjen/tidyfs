#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo fmt --manifest-path fuzz/Cargo.toml -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
