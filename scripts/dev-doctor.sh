#!/usr/bin/env bash
set -euo pipefail

failures=0

check_required() {
    local name="$1"
    local command="$2"
    local version_command="$3"
    if bash -lc "$command" >/dev/null 2>&1; then
        local version
        version="$(bash -lc "$version_command" 2>/dev/null | head -n 1 || true)"
        printf "ok   %-18s %s\n" "$name" "${version:-available}"
    else
        printf "fail %-18s missing\n" "$name"
        failures=$((failures + 1))
    fi
}

check_optional() {
    local name="$1"
    local command="$2"
    local version_command="$3"
    if bash -lc "$command" >/dev/null 2>&1; then
        local version
        version="$(bash -lc "$version_command" 2>/dev/null | head -n 1 || true)"
        printf "ok   %-18s %s\n" "$name" "${version:-available}"
    else
        printf "warn %-18s missing\n" "$name"
    fi
}

check_required "rustup" "command -v rustup" "rustup show active-toolchain"
check_required "cargo" "command -v cargo" "cargo --version"
check_required "just" "command -v just" "just --version"
check_required "mise" "command -v mise" "mise --version"
check_required "python3" "command -v python3" "python3 --version"
check_required "pre-commit" "python3 -m pre_commit --version" "python3 -m pre_commit --version"
check_required "node" "command -v node" "node --version"
check_required "npm" "command -v npm" "npm --version"
check_required "cargo-watch" "cargo watch --version || (command -v mise && mise exec -- cargo watch --version)" "cargo watch --version || mise exec -- cargo watch --version"

check_optional "gh" "command -v gh" "gh --version"
check_optional "go" "command -v go || (command -v mise && mise exec -- go version)" "go version || mise exec -- go version"
check_optional "javac" "command -v javac || (command -v mise && mise exec -- javac -version)" "javac -version || mise exec -- javac -version"
check_optional "mvn" "command -v mvn" "mvn --version"
if command -v sqlite3 >/dev/null 2>&1; then
    sqlite_version="$(sqlite3 --version 2>/dev/null | head -n 1 || true)"
    printf "ok   %-18s %s\n" "sqlite3 cli" "${sqlite_version:-available}"
else
    printf "warn %-18s optional; install only if you want to inspect console dev databases\n" "sqlite3 cli"
fi

if [[ -d apps/console/node_modules ]]; then
    printf "ok   %-18s installed\n" "console deps"
else
    printf "fail %-18s run 'just setup'\n" "console deps"
    failures=$((failures + 1))
fi

if [[ -d sdks/typescript/node_modules ]]; then
    printf "ok   %-18s installed\n" "ts sdk deps"
else
    printf "fail %-18s run 'just setup'\n" "ts sdk deps"
    failures=$((failures + 1))
fi

pre_commit_hook="$(git rev-parse --path-format=absolute --git-path hooks/pre-commit 2>/dev/null || true)"
pre_push_hook="$(git rev-parse --path-format=absolute --git-path hooks/pre-push 2>/dev/null || true)"
if [[ -n "$pre_commit_hook" && -n "$pre_push_hook" && -x "$pre_commit_hook" && -x "$pre_push_hook" ]]; then
    printf "ok   %-18s installed\n" "git hooks"
else
    printf "fail %-18s run 'just setup'\n" "git hooks"
    failures=$((failures + 1))
fi

if [[ "$failures" -gt 0 ]]; then
    printf "\n%d required check(s) failed.\n" "$failures" >&2
    exit 1
fi

printf "\nDeveloper environment looks ready.\n"
