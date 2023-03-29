#!/bin/sh

set -ux

script="$1"
context="$2"
desc="$3"

if [ -z "$script" ]; then
    echo "No script to run" >&2
    exit 1
fi

if [ -z "$context" ]; then
    echo "Missing 'context' for CI status report" >&2
    exit 1
fi

if [ -z "$REF" ]; then
    echo "Environment variable 'REF' must be set to a valid Git ref." >&2
    exit 1
fi

if [ -z "$GITEA_API_KEY" ]; then
    echo "Environment variable 'GITEA_API_KEY' must be set." >&2
    exit 1
fi

notify() {
    curl -X 'POST' \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json' \
        -H "Authorization: token $GITEA_API_KEY" \
        "https://git.sclu1034.dev/api/v1/repos/bitsquid_dt/dtmt/statuses/$REF" \
        --data @- <<EOF
{
"context": "$2",
"description": "$3",
"state": "$1"
}
EOF
}

notify 'pending' "$context" "$desc"

if sh "$script"; then
    notify 'success' "$context" "$desc"
else
    notify 'failure' "$context" "$desc"
fi
