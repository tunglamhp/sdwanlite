#!/usr/bin/env bash
set -euo pipefail

echo "==> Format Rust"
cargo fmt --all

echo "==> Clippy fix (best effort)"
cargo clippy --workspace --fix --allow-no-vcs || true

echo "==> Format web UI"
pushd web-ui >/dev/null
npm run lint -- --fix || true
popd >/dev/null

echo "==> Auto-fix complete"
