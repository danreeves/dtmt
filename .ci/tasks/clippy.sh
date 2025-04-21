#!/bin/sh

set -eu

title() {
    printf "\033[1m%s\033[0m\n" "$1"
}

title "Install clippy"
rustup component add clippy

title "Run clippy"
cargo clippy --color always --no-deps -- -D warnings

title "Done"
