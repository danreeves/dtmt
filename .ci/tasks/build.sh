#!/bin/sh

set -eu

if [ -n "$OUTPUT" ]; then
    OUTPUT="$PWD/$OUTPUT"
else
    OUTPUT=$(mktemp -d)
fi

title() {
    printf "\033[1m%s\033[0m\n" "$1"
}

if [ -n "${PR:-}" ]; then
    title "PR: $(echo "$PR" | jq '.number') - $(echo "$PR" | jq '.title')"
    ref="pr-$(echo "$PR" | jq '.number')-$(git rev-parse --short HEAD 2>/dev/null || echo 'manual')"
else
    ref=$(git describe --tags)
fi

title "Version is '$ref'"
echo "$ref" > "$OUTPUT/version"

cd "repo"
case "$TARGET" in
    msvc)
        cp /src/*.lib ./lib/oodle/

        title "Build project for target $TARGET"
        cargo build --color always --locked --release --target x86_64-pc-windows-msvc -Zbuild-std

        title "Install artifacts"
        install -t "$OUTPUT/" target/x86_64-pc-windows-msvc/release/dtmt.exe
        install -t "$OUTPUT/" target/x86_64-pc-windows-msvc/release/dtmm.exe
        ;;
    linux)
        cp /src/*.a ./lib/oodle/

        title "Build project for target $TARGET"
        cargo build --color always --locked --profile release-lto

        title "Install artifacts"
        install -t "$OUTPUT/" target/release-lto/dtmt
        install -t "$OUTPUT/" target/release-lto/dtmm
        ;;
    *)
        echo "Env var 'TARGET' must either be 'msvc' or 'linux'. Got '$TARGET'." >&2
        exit 1
esac
