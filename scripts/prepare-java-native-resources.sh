#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <downloaded-native-artifact-dir> <generated-resource-dir>" >&2
    exit 2
fi

native_root="$1"
resources="$2"

copy_native() {
    local artifact="$1"
    local file="$2"
    local platform="$3"
    local source="$native_root/java-native-$artifact/$file"
    local target="$resources/dev/rototo/native/$platform"

    if [[ ! -f "$source" ]]; then
        echo "missing Java native library: $source" >&2
        exit 1
    fi

    mkdir -p "$target"
    cp "$source" "$target/$file"
}

rm -rf "$resources/dev/rototo/native"

copy_native "linux-x86_64" "librototo_java.so" "linux-x86_64"
copy_native "linux-aarch64" "librototo_java.so" "linux-aarch64"
copy_native "darwin-x86_64" "librototo_java.dylib" "darwin-x86_64"
copy_native "darwin-aarch64" "librototo_java.dylib" "darwin-aarch64"
copy_native "windows-x86_64" "rototo_java.dll" "windows-x86_64"
