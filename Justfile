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

ci-image-msvc: ci-image-linux
    docker build -t dtmt-ci-base-msvc -f .ci/image/Dockerfile.msvc .ci/image
    docker tag dtmt-ci-base-msvc registry.sclu1034.dev/dtmt-ci-base-msvc
    docker push registry.sclu1034.dev/dtmt-ci-base-msvc

ci-image-linux:
    docker build -t dtmt-ci-base-linux -f .ci/image/Dockerfile.linux .ci/image
    docker tag dtmt-ci-base-linux registry.sclu1034.dev/dtmt-ci-base-linux
    docker push registry.sclu1034.dev/dtmt-ci-base-linux
