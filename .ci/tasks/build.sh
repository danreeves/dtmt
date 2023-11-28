#!/bin/bash

set -eu

if [ -n "$OUTPUT" ]; then
    OUTPUT="$PWD/$OUTPUT"
else
    OUTPUT=$(mktemp -d)
fi

title() {
    printf "\033[1m%s\033[0m\n" "$1"
}

install_artifact() {
    install -v -t "$OUTPUT/" "$1"
    sha256sum "$1" | cut -d' ' -f1 > "$OUTPUT/$(basename "$1").sha256"
}

cd "repo"

PR=${PR:-}

if [ -n "$PR" ]; then
    title "PR: $(echo "$PR" | jq '.number') - $(echo "$PR" | jq '.title')"
    ref="pr-$(echo "$PR" | jq '.number')-$(git rev-parse --short "$(cat .git/ref || echo "HEAD")" 2>/dev/null || echo 'manual')"
else
    ref=$(git describe --tags)
fi

title "Version: '$ref'"
echo "$ref" > "$OUTPUT/version"

case "$TARGET" in
    msvc)
        cp /src/*.lib ./lib/oodle/

        title "Building project for target $TARGET"
        cargo build --color always --locked --release --target x86_64-pc-windows-msvc -Zbuild-std

        title "Install artifacts"
        install_artifact target/x86_64-pc-windows-msvc/release/dtmt.exe
        install_artifact target/x86_64-pc-windows-msvc/release/dtmm.exe
        ;;
    linux)
        cp /src/*.a ./lib/oodle/

        title "Building project for target $TARGET"
        cargo build --color always --locked --profile release-lto

        title "Installing artifacts"
        install_artifact target/release-lto/dtmt
        install_artifact target/release-lto/dtmm
        ;;
    *)
        echo -e "\033[31;1mEnv var 'TARGET' must either be 'msvc' or 'linux'. Got '$TARGET'.\033[0m" >&2
        exit 1
esac

title "Done"
