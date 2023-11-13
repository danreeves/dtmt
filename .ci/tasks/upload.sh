#!/bin/bash

set -eu

artifacts="$PWD/artifacts"
repo="$PWD/repo"

base_url="${GITEA_URL}/api/packages/${GITEA_USER}/generic"

cd "$repo"

if [ -n "$PR" ]; then
    echo "PR: $(echo "$PR" | jq '.number') - $(echo "$PR" | jq '.title')"
    ref="pr-$(echo "$PR" | jq '.number')-$(git rev-parse --short HEAD 2>/dev/null || echo 'manual')"
else
    ref=$(git describe --tags)
fi

echo "ref: $ref"

# TODO: If this is a tag, check the tag name to determine which
# binary was affected and only upload that.
for f in dtmt dtmt.exe dtmm dtmm.exe; do
    if [ -f "$artifacts/$f" ]; then
        url="$base_url/$(basename -s .exe $f)/$ref/$f"
        echo "$url"
        curl -i -X 'PUT' \
            --user "concourse:$GITEA_API_KEY" \
            --upload-file "$artifacts/$f" \
            "$url"
    fi
done
