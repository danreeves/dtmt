ci-image: ci-image-msvc ci-image-linux

ci-image-msvc:
    docker build -t dtmt-ci-base-msvc -f .ci/image/Dockerfile.msvc .ci/image
    docker tag dtmt-ci-base-msvc registry.sclu1034.dev/dtmt-ci-base-msvc
    docker push registry.sclu1034.dev/dtmt-ci-base-msvc

ci-image-linux:
    docker build -t dtmt-ci-base-linux -f .ci/image/Dockerfile.linux .ci/image
    docker tag dtmt-ci-base-linux registry.sclu1034.dev/dtmt-ci-base-linux
    docker push registry.sclu1034.dev/dtmt-ci-base-linux
