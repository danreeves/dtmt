fly_target := "main"

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

ci-image: ci-image-msvc ci-image-linux

ci-image-msvc:
    docker build -t dtmt-ci-base-msvc -f .ci/image/Dockerfile.msvc .
    docker tag dtmt-ci-base-msvc registry.sclu1034.dev/dtmt-ci-base-msvc
    docker push registry.sclu1034.dev/dtmt-ci-base-msvc

ci-image-linux:
    docker build -t dtmt-ci-base-linux -f .ci/image/Dockerfile.linux .
    docker tag dtmt-ci-base-linux registry.sclu1034.dev/dtmt-ci-base-linux
    docker push registry.sclu1034.dev/dtmt-ci-base-linux

set-base-pipeline:
    fly -t {{fly_target}} set-pipeline \
        --pipeline dtmt \
        --config .ci/pipelines/base.yml \
        -v gitea_api_key=${GITEA_API_KEY} \
        -v owner=bitsquid_dt \
        -v repo=dtmt

set-pr-pipeline pr:
    curl \
        -H "Authorization: ${GITEA_API_KEY}" \
        -H 'Accept: application/json' \
        'https://git.sclu1034.dev/api/v1/repos/bitsquid_dt/dtmt/pulls/{{pr}}' \
        | yq -y '.' - > 'pr-{{pr}}.yaml' 
    fly -t main set-pipeline \
        --pipeline dtmt-pr \
        --config .ci/pipelines/pr.yml \
        -v gitea_api_key=${GITEA_API_KEY} \
        -i number={{pr}} \
        -y branch="$(yq -y '.head.ref' 'pr-{{pr}}.yaml')" \
        -y pr="$(cat 'pr-{{pr}}.yaml')"

