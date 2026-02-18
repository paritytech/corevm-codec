#!/bin/sh
set -ex
git config --global --add safe.directory "$PWD"
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets --quiet -- -Dwarnings
cargo test --workspace
./scripts/bench.sh native
