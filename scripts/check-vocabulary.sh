#!/usr/bin/env bash
set -euo pipefail

tmp="$(mktemp -t rototo-vocabulary.XXXXXX)"
trap 'rm -f "$tmp"' EXIT

rg -n --glob 'src/**/*.rs' --glob 'docs/src/**/*.md' --glob 'README.md' \
    --glob 'CONTRIBUTING.md' --glob 'sdks/**/*.rs' --glob 'sdks/**/*.go' \
    --glob 'sdks/**/*.java' --glob 'sdks/**/*.ts' \
    '(PackageCommand|PackageCommands|PackagesCommand|package://|packages/|rototo package\b|\bpackage (list|get|lint|resolve|resolve-all)\b)' >"$tmp" || true

if [[ -s "$tmp" ]]; then
    echo "The removed rototo package model appears in public code or docs."
    echo "Use workspace, qualifier, variable, catalog, schema, or value vocabulary instead."
    echo
    cat "$tmp"
    exit 1
fi
