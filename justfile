# rototo developer task runner. Install `just`:
# https://github.com/casey/just
#
# Recipe groups:
#   01. setup   one-time bootstrap
#   02. format  in-place formatters
#   03. lint    no test execution
#   04. test    test runners
#   05. check   local pre-push gate

default:
    @just --list --unsorted

# Register pre-commit and pre-push hooks. Cheap and idempotent.
[group('01. setup')]
setup-min:
    #!/usr/bin/env bash
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
    #!/usr/bin/env bash
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

# Run the local pre-push gate.
[group('05. check')]
check: lint test
