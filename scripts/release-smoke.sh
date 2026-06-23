#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
if [[ -z "$version" ]]; then
    echo "usage: scripts/release-smoke.sh <version>" >&2
    exit 2
fi

if [[ "$version" =~ ^([0-9]+\.[0-9]+\.[0-9]+)-alpha\.([0-9]+)$ ]]; then
    python_version="${BASH_REMATCH[1]}a${BASH_REMATCH[2]}"
else
    python_version="$version"
fi

echo "Checking crates.io rototo $version"
curl -fsS -A rototo-release-smoke \
    "https://crates.io/api/v1/crates/rototo/$version" >/dev/null

echo "Checking npm rototo $version"
npm_version="$(npm view "rototo@$version" version)"
if [[ "$npm_version" != "$version" ]]; then
    echo "npm rototo@$version returned version $npm_version" >&2
    exit 1
fi

echo "Checking PyPI rototo $python_version"
curl -fsS "https://pypi.org/pypi/rototo/$python_version/json" >/dev/null

echo "Checking Maven Central dev.rototo:rototo:$version"
curl -fsS \
    "https://repo.maven.apache.org/maven2/dev/rototo/rototo/$version/rototo-$version.pom" \
    >/dev/null
