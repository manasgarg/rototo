# rototo developer task runner. Install `just`:
# https://github.com/casey/just
#
# Recipe groups:
#   01. setup   one-time bootstrap
#   02. format  in-place formatters
#   03. lint    no test execution
#   04. test    test runners
#   05. check   local pre-push gate
#   06. docs    documentation publishing previews
#   07. release release preparation and validation

default:
    @just --list --unsorted

# Register pre-commit and pre-push hooks. Cheap and idempotent.
[group('01. setup')]
setup-min:
    #!/bin/bash
    set -euo pipefail
    if command -v mise >/dev/null && mise where python >/dev/null 2>&1; then
        py() { mise exec -- python3 "$@"; }
    elif command -v python3 >/dev/null; then
        py() { python3 "$@"; }
    else
        echo "python3 not found; run 'mise install' or install Python 3 to install pre-commit" >&2
        exit 1
    fi
    py -m pip install --quiet -r requirements-dev.txt
    py -m pre_commit install -t pre-commit -t pre-push

# Install/verify the local toolchain and install local hooks.
[group('01. setup')]
setup:
    #!/bin/bash
    set -euo pipefail
    if command -v mise >/dev/null; then
        mise install
    else
        echo "mise not found; install it to apply .tool-versions" >&2
    fi
    rustup show active-toolchain
    cargo fmt --version
    cargo clippy --version
    just setup-min
    echo "Done. Run 'just check' to verify."

# Format Rust code.
[group('02. format')]
fmt:
    cargo fmt --all

# Run Rust linters without executing tests.
[group('03. lint')]
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Run the Rust test suite.
[group('04. test')]
test:
    cargo test --workspace --all-targets

# Run the Python SDK test suite.
[group('04. test')]
python-sdk-test:
    #!/bin/bash
    set -euo pipefail
    venv="${ROTOTO_PYTHON_SDK_VENV:-/tmp/rototo-python-sdk-venv}"
    python3 -m venv "$venv"
    "$venv/bin/python" -m pip install --quiet --upgrade pip
    "$venv/bin/python" -m pip install --quiet maturin==1.13.3
    export VIRTUAL_ENV="$venv"
    export PATH="$venv/bin:$PATH"
    (cd sdks/python && "$venv/bin/python" -m maturin develop)
    "$venv/bin/python" -m unittest discover -s sdks/python/tests

# Run the local pre-push gate.
[group('05. check')]
check: lint test python-sdk-test

# Validate that a release tag version matches all package version surfaces.
[group('07. release')]
release-check version:
    #!/bin/bash
    set -euo pipefail
    version="{{version}}"

    if [[ "$version" == v* ]]; then
        echo "release-check expects a version without the leading v tag prefix" >&2
        exit 1
    fi

    manifest_version="$(
        cargo metadata --locked --format-version=1 --no-deps |
        python3 -c 'import json, sys; data = json.load(sys.stdin); print(next(package["version"] for package in data["packages"] if package["name"] == "rototo"))'
    )"

    if [[ "$manifest_version" != "$version" ]]; then
        echo "tag version $version does not match Cargo.toml version $manifest_version" >&2
        exit 1
    fi

    cargo test --locked --test release_versions

    readme="$(mktemp -t rototo-python-readme.XXXXXX)"
    trap 'rm -f "$readme"' EXIT
    cargo run --locked -- docs --package-readme python --out "$readme"
    diff -u sdks/python/README.md "$readme"

# Update package versions and generated SDK packaging content for a release.
[group('07. release')]
release-prep version:
    #!/bin/bash
    set -euo pipefail
    version="{{version}}"

    if [[ "$version" == v* ]]; then
        echo "release-prep expects a version without the leading v tag prefix" >&2
        exit 1
    fi

    for manifest in Cargo.toml sdks/python/Cargo.toml sdks/python/pyproject.toml; do
        perl -0pi -e 's/^version = "[^"]+"/version = "'"$version"'"/m' "$manifest"
    done

    cargo update -w
    cargo run --locked -- docs --package-readme python --out sdks/python/README.md
    just release-check "$version"
    just check

# Export docs and publish a Cloudflare Pages preview deployment.
[group('06. docs')]
docs-preview branch="docs-dev":
    #!/bin/bash
    set -euo pipefail

    if [[ "{{branch}}" == "main" ]]; then
        echo "docs-preview refuses branch=main; production docs deploy from the GitHub workflow" >&2
        exit 1
    fi

    for name in CLOUDFLARE_ACCOUNT_ID CLOUDFLARE_API_TOKEN; do
        if [[ -z "${!name:-}" ]]; then
            echo "$name is required for Cloudflare Pages preview deploys" >&2
            exit 1
        fi
    done

    if ! command -v mise >/dev/null; then
        echo "mise is required for the pinned Wrangler tool in .tool-versions; run just setup after installing mise" >&2
        exit 1
    fi

    project="${CLOUDFLARE_PAGES_PROJECT:-rototo-docs}"
    out="$(mktemp -d -t rototo-docs-site.XXXXXX)"
    trap 'rm -rf "$out"' EXIT

    cargo run --locked -- docs --export "$out"
    mise exec -- wrangler pages deploy "$out" \
        --project-name="$project" \
        --branch="{{branch}}" \
        --commit-dirty=true
