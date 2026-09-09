#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: $0 <new-version>"
  exit 1
fi

NEW_VERSION="$1"

if [[ ! "$NEW_VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Version must match vX.Y.Z"
  exit 1
fi

TAG="$NEW_VERSION"

# Update Cargo workspace version if workspace root Cargo.toml exists.
if [ -f "Cargo.toml" ]; then
  if grep -q '^name = "sdwanlite"' Cargo.toml; then
    echo "==> Bump Cargo workspace to $TAG"
    sed -i 's/^version = ".*"$/version = "'"${TAG#v}"'"/' Cargo.toml || true
  fi
fi

# Update web-ui package version.
if [ -f "web-ui/package.json" ]; then
  echo "==> Bump web-ui package to $TAG"
  node -e '
    const fs = require("fs");
    const path = "web-ui/package.json";
    const pkg = JSON.parse(fs.readFileSync(path, "utf8"));
    pkg.version = process.argv[1];
    fs.writeFileSync(path, JSON.stringify(pkg, null, 2) + "\n");
  ' "${TAG#v}"
fi

# Update README version badge if present.
if [ -f "README.md" ]; then
  echo "==> Update README version badge to $TAG"
  sed -i 's/Version \*\*.*\*\*/Version **'"$TAG"'**/' README.md || true
fi

# Update ROADMAP done marker only if needed.
if [ -f "ROADMAP.md" ]; then
  echo "==> Refresh ROADMAP markers"
  sed -i 's/Version \*\*.*\*\*/Version **'"$TAG"'**/' ROADMAP.md || true
fi

# Update CHANGELOG unreleased header if present.
if [ -f "CHANGELOG.md" ]; then
  echo "==> Update CHANGELOG header to $TAG"
  sed -i 's/## \[Unreleased\]/## ['"$TAG"'] - '"$(date -u +%Y-%m-%d)"'/' CHANGELOG.md || true
fi

echo "==> Version bumped to $TAG"
