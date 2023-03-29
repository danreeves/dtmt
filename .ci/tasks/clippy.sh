#!/bin/sh

set -eux

rustup component add clippy

cargo clippy --color always --no-deps
