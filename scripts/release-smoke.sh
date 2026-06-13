#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
if [[ -z "$version" ]]; then
    echo "usage: scripts/release-smoke.sh <version>" >&2
    exit 1
fi

echo "Release smoke checks for $version"
echo "- crates.io: https://crates.io/crates/rototo/$version"
echo "- PyPI: https://pypi.org/project/rototo/${version/-alpha./a}/"
echo "- npm: https://www.npmjs.com/package/rototo/v/$version"
echo "- Maven Central: https://central.sonatype.com/artifact/dev.rototo/rototo/$version"
echo "- Go module: https://pkg.go.dev/github.com/manasgarg/rototo/sdks/go@v$version"
echo
echo "Run registry install smoke tests from an isolated temp directory after publish."
