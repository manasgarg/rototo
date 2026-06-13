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
#   08. console local console development

default:
    @just --list --unsorted

# Verify local tools and generated setup state.
[group('01. setup')]
doctor:
    bash scripts/dev-doctor.sh

# Register pre-commit and pre-push hooks. Cheap and idempotent.
[group('01. setup')]
_install-hooks:
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

# Install/verify the local toolchain, frontend dependencies, and local hooks.
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
    if cargo watch --version >/dev/null 2>&1; then
        cargo watch --version
    elif command -v mise >/dev/null && mise exec -- cargo watch --version >/dev/null 2>&1; then
        mise exec -- cargo watch --version
    else
        echo "cargo-watch not found; run 'mise install' or install it with 'cargo install cargo-watch --locked'" >&2
        exit 1
    fi
    just _install-hooks
    just _install-console-deps
    just _install-typescript-sdk-deps
    just doctor
    echo "Done. Run 'just check' to verify."

# Remove generated local-only dev artifacts.
[group('01. setup')]
clean-dev:
    bash scripts/clean-dev.sh

# Install the console UI dependencies.
[group('01. setup')]
_install-console-deps:
    npm --prefix apps/console ci

# Install the TypeScript SDK dependencies.
[group('01. setup')]
_install-typescript-sdk-deps:
    npm --prefix sdks/typescript ci

# Run the full console development stack: Rust API plus Vite UI.
[group('08. console')]
console-dev:
    #!/bin/bash
    set -euo pipefail
    public_url="${ROTOTO_CONSOLE_DEV_PUBLIC_URL:-https://dev.rototo.dev}"
    observability_dir="${ROTOTO_CONSOLE_DEV_OBSERVABILITY:-.rototo/dev/observability}"
    mkdir -p "$observability_dir"
    touch "$observability_dir/console-api.ndjson" "$observability_dir/console-ui.ndjson" "$observability_dir/console-dev.log"
    export ROTOTO_CONSOLE_DEV_OBSERVABILITY="$observability_dir"
    export RUST_LOG="${RUST_LOG:-rototo=info,warn}"
    log="$observability_dir/console-dev.log"

    cargo_watch() {
        if cargo watch --version >/dev/null 2>&1; then
            cargo watch "$@"
        elif command -v mise >/dev/null && mise exec -- cargo watch --version >/dev/null 2>&1; then
            mise exec -- cargo watch "$@"
        else
            echo "cargo-watch not found; run 'just setup' before 'just console-dev'" >&2
            exit 1
        fi
    }

    (cargo_watch -w src -w Cargo.toml -w Cargo.lock -w build.rs -x "run -- console --public-url $public_url" 2>&1 | tee -a "$log") &
    api_pid=$!
    trap 'kill "$api_pid" 2>/dev/null || true; wait "$api_pid" 2>/dev/null || true' EXIT
    ready=0
    for _ in $(seq 1 120); do
        if curl --silent --output /dev/null --max-time 1 http://127.0.0.1:7686/api/me; then
            ready=1
            break
        fi
        if ! kill -0 "$api_pid" 2>/dev/null; then
            wait "$api_pid"
        fi
        sleep 0.25
    done
    if [[ "$ready" -ne 1 ]]; then
        echo "console API did not become ready on http://127.0.0.1:7686" >&2
        exit 1
    fi
    npm --prefix apps/console run dev -- --force 2>&1 | tee -a "$log"

# Run only the auto-reloading console API server for the dev.rototo.dev Caddy target.
[group('08. console')]
console-api:
    #!/bin/bash
    set -euo pipefail
    public_url="${ROTOTO_CONSOLE_DEV_PUBLIC_URL:-https://dev.rototo.dev}"
    observability_dir="${ROTOTO_CONSOLE_DEV_OBSERVABILITY:-.rototo/dev/observability}"
    mkdir -p "$observability_dir"
    touch "$observability_dir/console-api.ndjson" "$observability_dir/console-ui.ndjson" "$observability_dir/console-dev.log"
    export ROTOTO_CONSOLE_DEV_OBSERVABILITY="$observability_dir"
    export RUST_LOG="${RUST_LOG:-rototo=info,warn}"

    if cargo watch --version >/dev/null 2>&1; then
        cargo watch -w src -w Cargo.toml -w Cargo.lock -w build.rs -x "run -- console --public-url $public_url" 2>&1 | tee -a "$observability_dir/console-dev.log"
    elif command -v mise >/dev/null && mise exec -- cargo watch --version >/dev/null 2>&1; then
        mise exec -- cargo watch -w src -w Cargo.toml -w Cargo.lock -w build.rs -x "run -- console --public-url $public_url" 2>&1 | tee -a "$observability_dir/console-dev.log"
    else
        echo "cargo-watch not found; run 'just setup' before 'just console-api'" >&2
        exit 1
    fi

# Run only the console UI dev server, proxying /api to ROTOTO_CONSOLE_API or 127.0.0.1:7686.
[group('08. console')]
console-ui:
    npm --prefix apps/console run dev -- --force

# Run the embedded console behind demo.rototo.dev.
[group('08. console')]
console-demo: console-build
    #!/bin/bash
    set -euo pipefail
    bind="${ROTOTO_CONSOLE_DEMO_BIND:-127.0.0.1:7687}"
    public_url="${ROTOTO_CONSOLE_DEMO_PUBLIC_URL:-https://demo.rototo.dev}"
    cargo run -- console --bind "$bind" --public-url "$public_url"

# Run a production-like local console with embedded frontend assets.
[group('08. console')]
console-preview: console-build
    cargo run -- console

# Build the console UI bundle that release binaries embed.
[group('08. console')]
console-build:
    npm --prefix apps/console run build

# Install dependencies from the lockfile, typecheck, and build the console UI.
[group('04. test')]
console-ci:
    npm --prefix apps/console ci
    npm --prefix apps/console run build

# Format Rust code and available Go sources.
[group('02. format')]
fmt:
    #!/bin/bash
    set -euo pipefail
    cargo fmt --all
    npm --prefix apps/console run format --if-present
    npm --prefix sdks/typescript run format --if-present
    if command -v gofmt >/dev/null; then
        gofmt -w sdks/go/*.go
    elif command -v mise >/dev/null && mise exec -- go version >/dev/null 2>&1; then
        mise exec -- gofmt -w sdks/go/*.go
    else
        echo "gofmt not found; skipping Go formatting" >&2
    fi

# Verify formatting without rewriting files.
[group('02. format')]
fmt-check:
    #!/bin/bash
    set -euo pipefail
    cargo fmt --all -- --check
    npm --prefix apps/console run format:check --if-present
    npm --prefix sdks/typescript run format:check --if-present
    if command -v gofmt >/dev/null; then
        unformatted="$(gofmt -l sdks/go/*.go)"
    elif command -v mise >/dev/null && mise exec -- go version >/dev/null 2>&1; then
        unformatted="$(mise exec -- gofmt -l sdks/go/*.go)"
    else
        echo "gofmt not found; skipping Go format check" >&2
        unformatted=""
    fi
    if [[ -n "$unformatted" ]]; then
        echo "$unformatted" >&2
        echo "Go files need formatting; run 'just fmt'" >&2
        exit 1
    fi
    cargo test --locked --test docs_consistency package_readmes_are_generated_from_sdk_reference_docs

# Run linters and typechecks without executing tests.
[group('03. lint')]
lint:
    #!/bin/bash
    set -euo pipefail
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    npm --prefix apps/console run lint
    bash scripts/check-vocabulary.sh

# Run all maintained test slices.
[group('04. test')]
test: test-rust test-console test-sdk-python test-sdk-typescript test-sdk-java test-sdk-go test-sdk-java-package

# Run the Rust test suite.
[group('04. test')]
test-rust:
    cargo test --workspace --all-targets

# Run the console typecheck and bundle build.
[group('04. test')]
test-console: console-ci

# Run the Python SDK test suite.
[group('04. test')]
test-sdk-python:
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

# Backward-compatible alias.
[group('04. test')]
python-sdk-test: test-sdk-python

# Run the TypeScript SDK test suite.
[group('04. test')]
test-sdk-typescript:
    #!/bin/bash
    set -euo pipefail
    (cd sdks/typescript && npm ci && npm run check)

# Backward-compatible alias.
[group('04. test')]
typescript-sdk-test: test-sdk-typescript

# Run the Java SDK test suite.
[group('04. test')]
test-sdk-java:
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

    "${JAVA[@]}" -Drototo.native.path="$native_path" -cp "$classes:$test_classes" dev.rototo.JavaSdkTest

    mkdir -p "$resources/dev/rototo/native/$resource_platform"
    cp "$native_path" "$resources/dev/rototo/native/$resource_platform/$native_file"
    "${JAR[@]}" --create --file "$jar_file" -C "$classes" . -C "$resources" .
    "${JAVA[@]}" -cp "$test_classes:$jar_file" dev.rototo.PackageSmokeTest

# Backward-compatible alias.
[group('04. test')]
java-sdk-test: test-sdk-java

# Run the Go SDK test suite.
[group('04. test')]
test-sdk-go:
    #!/bin/bash
    set -euo pipefail
    if command -v go >/dev/null; then
        GO=(go)
    elif command -v mise >/dev/null && mise exec -- go version >/dev/null 2>&1; then
        GO=(mise exec -- go)
    else
        echo "Go SDK tests require Go; skipping because go is not on PATH" >&2
        exit 0
    fi

    cargo build --locked --package rototo-go

    case "$(uname -s)" in
        Linux*) native_file="librototo_go.so" ;;
        Darwin*) native_file="librototo_go.dylib" ;;
        MINGW*|MSYS*|CYGWIN*) native_file="rototo_go.dll" ;;
        *) echo "unsupported Go SDK test platform: $(uname -s)" >&2; exit 1 ;;
    esac

    export ROTOTO_GO_NATIVE_PATH="$PWD/target/debug/$native_file"
    "${GO[@]}" test ./sdks/go

# Backward-compatible alias.
[group('04. test')]
go-sdk-test: test-sdk-go

# Verify the Java SDK Maven package shape when Maven is available.
[group('04. test')]
test-sdk-java-package:
    #!/bin/bash
    set -euo pipefail
    if command -v mvn >/dev/null; then
        MVN=(mvn)
    else
        echo "Java SDK Maven package check requires Maven; skipping because mvn is not on PATH" >&2
        exit 0
    fi

    cargo build --locked --package rototo-java

    case "$(uname -s)" in
        Linux*) native_file="librototo_java.so"; resource_platform="linux-$(uname -m)" ;;
        Darwin*) native_file="librototo_java.dylib"; resource_platform="darwin-$(uname -m)" ;;
        MINGW*|MSYS*|CYGWIN*) native_file="rototo_java.dll"; resource_platform="windows-$(uname -m)" ;;
        *) echo "unsupported Java SDK package-check platform: $(uname -s)" >&2; exit 1 ;;
    esac
    case "$resource_platform" in
        *-x86_64|*-amd64) resource_platform="${resource_platform%-*}-x86_64" ;;
        *-aarch64|*-arm64) resource_platform="${resource_platform%-*}-aarch64" ;;
    esac

    resources="sdks/java/target/generated-resources/native"
    native_path="$PWD/target/debug/$native_file"
    rm -rf "$resources/dev/rototo/native"
    mkdir -p "$resources/dev/rototo/native/$resource_platform"
    cp "$native_path" "$resources/dev/rototo/native/$resource_platform/$native_file"

    (cd sdks/java && "${MVN[@]}" -B -Dgpg.skip=true -Dcentral.skipPublishing=true verify)

# Backward-compatible alias.
[group('04. test')]
java-sdk-package-check: test-sdk-java-package

# Run the local pre-push gate.
[group('05. check')]
check:
    #!/bin/bash
    set -euo pipefail
    timings="$(mktemp -t rototo-check-timings.XXXXXX)"
    trap 'rm -f "$timings"' EXIT
    run_step() {
        local name="$1"
        shift
        local start end elapsed
        start="$(date +%s)"
        echo "==> $name"
        "$@"
        end="$(date +%s)"
        elapsed=$((end - start))
        printf "%s\t%s\n" "$elapsed" "$name" >> "$timings"
    }
    run_step console-deps just _install-console-deps
    run_step typescript-sdk-deps just _install-typescript-sdk-deps
    run_step fmt-check just fmt-check
    run_step lint just lint
    run_step test just test
    echo
    echo "Slowest just check steps:"
    sort -rn "$timings" | awk -F '\t' '{ printf "%5ss  %s\n", $1, $2 }'

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
    if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
        echo "release-check expects canonical SemVer, got: $version" >&2
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
    go_readme="$(mktemp -t rototo-go-readme.XXXXXX)"
    trap 'rm -f "$python_readme" "$typescript_readme" "$java_readme" "$go_readme"' EXIT
    cargo run --locked -- docs --package-readme python --out "$python_readme"
    cargo run --locked -- docs --package-readme typescript --out "$typescript_readme"
    cargo run --locked -- docs --package-readme java --out "$java_readme"
    cargo run --locked -- docs --package-readme go --out "$go_readme"
    diff -u sdks/python/README.md "$python_readme"
    diff -u sdks/typescript/README.md "$typescript_readme"
    diff -u sdks/java/README.md "$java_readme"
    diff -u sdks/go/README.md "$go_readme"
    node scripts/release-artifact-manifest.mjs "$version"

# Build publishable artifacts without publishing where local tooling is available.
[group('07. release')]
release-package-dry-run version:
    #!/bin/bash
    set -euo pipefail
    version="{{version}}"
    just console-build
    cargo publish --package rototo --dry-run --locked
    if command -v npm >/dev/null; then
        (cd sdks/typescript && npm pack --dry-run)
    fi
    if command -v mvn >/dev/null; then
        (cd sdks/java && mvn -B -Dgpg.skip=true -Dcentral.skipPublishing=true verify)
    fi
    bash scripts/release-smoke.sh "$version"

# Print post-publish smoke check links for every registry.
[group('07. release')]
release-smoke version:
    bash scripts/release-smoke.sh "{{version}}"

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

    for manifest in Cargo.toml sdks/python/Cargo.toml sdks/python/pyproject.toml sdks/typescript/Cargo.toml sdks/java/Cargo.toml sdks/go/Cargo.toml; do
        perl -0pi -e 's/^version = "[^"]+"/version = "'"$version"'"/m' "$manifest"
    done

    (cd sdks/typescript && npm version "$version" --no-git-tag-version --allow-same-version)
    perl -0pi -e 's|<version>[^<]+</version>|<version>'"$version"'</version>|' sdks/java/pom.xml
    cargo update -w
    cargo run --locked -- docs --package-readme python --out sdks/python/README.md
    cargo run --locked -- docs --package-readme typescript --out sdks/typescript/README.md
    cargo run --locked -- docs --package-readme java --out sdks/java/README.md
    cargo run --locked -- docs --package-readme go --out sdks/go/README.md
    just release-check "$version"
    just check

# Summarize console dev observability files.
[group('08. console')]
console-observe:
    node scripts/console-observe.mjs

# Keep summarizing console dev observability files.
[group('08. console')]
console-observe-watch:
    node scripts/console-observe.mjs --watch

# Fail when console dev observability contains actionable findings above thresholds.
[group('08. console')]
console-observe-check:
    node scripts/console-observe.mjs --check

# Report dependency freshness without blocking normal PR checks.
[group('05. check')]
dependency-freshness:
    bash scripts/check-dependency-freshness.sh

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
