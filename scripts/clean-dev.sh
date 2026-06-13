#!/usr/bin/env bash
set -euo pipefail

paths=(
    ".rototo/dev"
    "apps/console/dist"
    "apps/console/tsconfig.tsbuildinfo"
    "sdks/typescript/dist"
    "sdks/typescript/index.d.ts"
    "sdks/typescript/index.js"
    "sdks/java/target"
    "sdks/go/target"
)

for path in "${paths[@]}"; do
    if [[ -e "$path" ]]; then
        rm -rf "$path"
        printf "removed %s\n" "$path"
    fi
done

printf "clean-dev complete; source fixtures, staged work, and .git were not touched.\n"
