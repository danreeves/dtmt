set positional-arguments

repo := `git_url=$(git remote get-url origin 2>/dev/null || git remote | sed -n '1p'); \
         printf '%s\n' "$git_url" | sed -n 's/\.git$//; s/.*[:\/]\([[:alnum:]._-]\{1,\}\/[[:alnum:]._-]\{1,\}\)$/\1/p'`

build-perf-dtmt:
    cargo build --profile perf --bin dtmt

perf-dtmt *args='': build-perf-dtmt
    perf record --call-graph dwarf ./target/perf/dtmt "$@"

ci-build: ci-build-msvc ci-build-linux

ci-build-msvc:
    docker run --rm -ti --user $(id -u) -v ./:/src/dtmt dtmt-ci-base-msvc cargo --color always build --release --target x86_64-pc-windows-msvc --locked -Zbuild-std

ci-build-linux:
    docker run --rm -ti --user $(id -u) -v ./:/src/dtmt dtmt-ci-base-linux cargo --color always build --profile release-lto --locked

build-image: build-image-msvc build-image-linux

build-image-msvc:
    docker build -f .ci/Dockerfile.msvc .

build-image-linux:
    docker build -f .ci/Dockerfile.linux .

ci-image:
    # The MSVC image depends on the Linux image. So by building that first,
    # we actually build both, and cache them, so that "building" the
    # Linux image afterwards merely needs to pull the cache.
    docker build --target msvc -t dtmt-ci-base-msvc -f .ci/image/Dockerfile .
    docker build --target linux -t dtmt-ci-base-linux -f .ci/image/Dockerfile .
    docker tag dtmt-ci-base-msvc registry.sclu1034.dev/dtmt-ci-base-msvc
    docker tag dtmt-ci-base-linux registry.sclu1034.dev/dtmt-ci-base-linux
    docker push registry.sclu1034.dev/dtmt-ci-base-msvc
    docker push registry.sclu1034.dev/dtmt-ci-base-linux

actions workflow *args='':
    forgejo-runner exec \
        -W .forgejo/workflows/{{ workflow }}.yml \
        --container-opts "--volume='${XDG_CACHE_HOME:-$HOME/.local/cache}/forgejo-runner-cache:/cache'" \
        --forgejo-instance {{ env('FORGEJO_SERVER_URL') }} \
        --default-actions-url {{ env('FORGEJO_SERVER_URL') }} \
        --secret RELEASE_TOKEN={{ env('FORGEJO_RELEASE_TOKEN') }} \
        --secret DOWNLOAD_TOKEN={{ env('LIB_FILE_DOWNLOAD_TOKEN') }} \
        --env GITHUB_REPOSITORY={{ repo }} \
        {{ args }}
