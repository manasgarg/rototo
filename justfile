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

# Run the TypeScript SDK test suite.
[group('04. test')]
typescript-sdk-test:
    #!/bin/bash
    set -euo pipefail
    (cd sdks/typescript && npm ci && npm run check)

# Run the Java SDK test suite.
[group('04. test')]
java-sdk-test:
    #!/bin/bash
    set -euo pipefail
    if command -v javac >/dev/null && command -v java >/dev/null && command -v jar >/dev/null; then
        JAVAC=(javac)
        JAVA=(java)
        JAR=(jar)
    elif command -v mise >/dev/null && mise exec -- javac -version >/dev/null 2>&1; then
        JAVAC=(mise exec -- javac)
        JAVA=(mise exec -- java)
        JAR=(mise exec -- jar)
    else
        echo "Java SDK tests require a JDK; skipping because javac/java/jar is not on PATH" >&2
        exit 0
    fi

    cargo build --locked --package rototo-java

    classes="sdks/java/target/classes"
    test_classes="sdks/java/target/test-classes"
    resources="sdks/java/target/package-resources"
    jar_file="sdks/java/target/rototo-java-test.jar"
    rm -rf "$classes" "$test_classes" "$resources" "$jar_file"
    mkdir -p "$classes" "$test_classes"

    find sdks/java/src/main/java -name '*.java' -print > sdks/java/target/main-sources.txt
    find sdks/java/src/test/java -name '*.java' -print > sdks/java/target/test-sources.txt
    "${JAVAC[@]}" --release 11 -d "$classes" @sdks/java/target/main-sources.txt
    "${JAVAC[@]}" --release 11 -cp "$classes" -d "$test_classes" @sdks/java/target/test-sources.txt

    case "$(uname -s)" in
        Linux*) native_file="librototo_java.so"; resource_platform="linux-$(uname -m)" ;;
        Darwin*) native_file="librototo_java.dylib"; resource_platform="darwin-$(uname -m)" ;;
        MINGW*|MSYS*|CYGWIN*) native_file="rototo_java.dll"; resource_platform="windows-$(uname -m)" ;;
        *) echo "unsupported Java SDK test platform: $(uname -s)" >&2; exit 1 ;;
    esac
    case "$resource_platform" in
        *-x86_64|*-amd64) resource_platform="${resource_platform%-*}-x86_64" ;;
        *-aarch64|*-arm64) resource_platform="${resource_platform%-*}-aarch64" ;;
    esac
    native_path="$PWD/target/debug/$native_file"

    "${JAVA[@]}" -Drototo.native.path="$native_path" -cp "$classes:$test_classes" com.rototo.JavaSdkTest

    mkdir -p "$resources/com/rototo/native/$resource_platform"
    cp "$native_path" "$resources/com/rototo/native/$resource_platform/$native_file"
    "${JAR[@]}" --create --file "$jar_file" -C "$classes" . -C "$resources" .
    "${JAVA[@]}" -cp "$test_classes:$jar_file" com.rototo.PackageSmokeTest

# Run the local pre-push gate.
[group('05. check')]
check: lint test python-sdk-test typescript-sdk-test java-sdk-test

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

    python_readme="$(mktemp -t rototo-python-readme.XXXXXX)"
    typescript_readme="$(mktemp -t rototo-typescript-readme.XXXXXX)"
    java_readme="$(mktemp -t rototo-java-readme.XXXXXX)"
    trap 'rm -f "$python_readme" "$typescript_readme" "$java_readme"' EXIT
    cargo run --locked -- docs --package-readme python --out "$python_readme"
    cargo run --locked -- docs --package-readme typescript --out "$typescript_readme"
    cargo run --locked -- docs --package-readme java --out "$java_readme"
    diff -u sdks/python/README.md "$python_readme"
    diff -u sdks/typescript/README.md "$typescript_readme"
    diff -u sdks/java/README.md "$java_readme"

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

    for manifest in Cargo.toml sdks/python/Cargo.toml sdks/python/pyproject.toml sdks/typescript/Cargo.toml sdks/java/Cargo.toml; do
        perl -0pi -e 's/^version = "[^"]+"/version = "'"$version"'"/m' "$manifest"
    done

    (cd sdks/typescript && npm version "$version" --no-git-tag-version --allow-same-version)
    perl -0pi -e 's|<version>[^<]+</version>|<version>'"$version"'</version>|' sdks/java/pom.xml
    cargo update -w
    cargo run --locked -- docs --package-readme python --out sdks/python/README.md
    cargo run --locked -- docs --package-readme typescript --out sdks/typescript/README.md
    cargo run --locked -- docs --package-readme java --out sdks/java/README.md
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
