#!/usr/bin/env bash
set -euo pipefail

echo "Dependency freshness report"
echo

if command -v cargo >/dev/null; then
    echo "- Rust: run 'cargo update -w --dry-run' locally when dependency review is needed."
fi

if command -v npm >/dev/null; then
    echo "- Console npm:"
    (cd apps/console && npm outdated || true)
    echo "- TypeScript SDK npm:"
    (cd sdks/typescript && npm outdated || true)
fi

if command -v python3 >/dev/null; then
    echo "- Python: review sdks/python/pyproject.toml and requirements-dev.txt."
fi

if command -v mvn >/dev/null; then
    echo "- Maven:"
    (cd sdks/java && mvn -B versions:display-dependency-updates || true)
fi

if command -v go >/dev/null; then
    echo "- Go:"
    go list -m -u all || true
fi
