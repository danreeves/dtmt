#!/bin/sh

set -eux

case "$TARGET" in
    msvc)
        cp /src/*.lib ./lib/oodle/
        cargo build --color always --locked --release --target x86_64-pc-windows-msvc -Zbuild-std

        if [ -d "$OUTPUT" ]; then
            install -t "$OUTPUT/" target/x86_64-pc-windows-msvc/release/dtmt.exe
            install -t "$OUTPUT/" target/x86_64-pc-windows-msvc/release/dtmm.exe
        fi
        ;;
    linux)
        cp /src/*.so ./lib/oodle/
        cargo build --color always --locked --profile release-lto

        if [ -d "$OUTPUT" ]; then
            install -t "$OUTPUT/" target/release/dtmt
            install -t "$OUTPUT/" target/release/dtmm
        fi
        ;;
    *)
        echo "Env var 'TARGET' must either be 'msvc' or 'linux'. Got '$TARGET'." >&2
        exit 1
esac
