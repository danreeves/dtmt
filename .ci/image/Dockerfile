# https://jake-shadle.github.io/xwin/
FROM debian:bullseye-slim AS xwin

# renovate: datasource=github-releases depName=xwin packageName=Jake-Shadle/xwin
ARG XWIN_VERSION=0.6.6
ARG XWIN_PREFIX="xwin-$XWIN_VERSION-x86_64-unknown-linux-musl"
ADD https://github.com/Jake-Shadle/xwin/releases/download/$XWIN_VERSION/$XWIN_PREFIX.tar.gz /root/$XWIN_PREFIX.tar.gz

RUN set -eux; \
    apt-get update; \
    apt-get install --no-install-recommends -y \
        tar \
        ; \
    # Install xwin to cargo/bin via github release. Note you could also just use `cargo install xwin`.
    tar -xzv -f /root/$XWIN_PREFIX.tar.gz -C /usr/bin --strip-components=1 $XWIN_PREFIX/xwin; \
    apt-get remove -y --auto-remove; \
    rm -rf \
        /var/lib/apt/lists/* \
        /root/$XWIN_PREFIX.tar.gz;

RUN set -eux; \
    # Splat the CRT and SDK files to /xwin/crt and /xwin/sdk respectively
    xwin \
        --log-level debug \
        --cache-dir /root/.xwin-cache \
        --manifest-version 16 \
        --accept-license \
        splat \
        --output /xwin; \
    # Even though this build step only exists temporary, to copy the
    # final data out of, it still generates a cache entry on the Docker host.
    # And to keep that to a minimum, we still delete the stuff we don't need.
    rm -rf /root/.xwin-cache;

FROM rust:slim-bullseye AS linux

RUN set -eux; \
    apt-get update; \
    apt-get install --no-install-recommends -y \
        build-essential \
        cmake \
        curl \
        git \
        gpg \
        jq \
        libatk1.0-dev \
        libclang-13-dev \
        libglib2.0-dev \
        libgtk-3-dev \
        libpango1.0-dev \
        libssl-dev \
        libzstd-dev \
        pkg-config; \
    apt-get remove -y --auto-remove; \
    rm -rf /var/lib/apt/lists/*; \
    rustup default nightly

WORKDIR /src/dtmt

COPY lib/oodle/*.so lib/oodle/*.a /src/

FROM linux AS msvc

# renovate: datasource=github-releases depName=llvm packageName=llvm/llvm-project
ARG LLVM_VERSION=20
ENV KEYRINGS /usr/local/share/keyrings

ADD https://apt.llvm.org/llvm-snapshot.gpg.key /root/llvm-snapshot.gpg.key
ADD https://dl.winehq.org/wine-builds/winehq.key /root/winehq.key

RUN set -eux; \
    mkdir -p $KEYRINGS; \
    # clang/lld/llvm
    gpg --dearmor > $KEYRINGS/llvm.gpg < /root/llvm-snapshot.gpg.key; \
    # wine
    gpg --dearmor > $KEYRINGS/winehq.gpg < /root/winehq.key; \
    echo "deb [signed-by=$KEYRINGS/llvm.gpg] http://apt.llvm.org/bullseye/ llvm-toolchain-bullseye-${LLVM_VERSION} main" > /etc/apt/sources.list.d/llvm.list; \
    echo "deb [signed-by=$KEYRINGS/winehq.gpg] https://dl.winehq.org/wine-builds/debian/ bullseye main" > /etc/apt/sources.list.d/winehq.list; \
    dpkg --add-architecture i386; \
    apt-get update; \
    apt-get install --no-install-recommends -y \
        libclang-${LLVM_VERSION}-dev \
        gcc-mingw-w64-x86-64 \
        clang-${LLVM_VERSION} \
        llvm-${LLVM_VERSION} \
        lld-${LLVM_VERSION} \
        winehq-staging \
        ; \
    # ensure that clang/clang++ are callable directly
    ln -s clang-${LLVM_VERSION} /usr/bin/clang && ln -s clang /usr/bin/clang++ && ln -s lld-${LLVM_VERSION} /usr/bin/ld.lld; \
    # We also need to setup symlinks ourselves for the MSVC shims because they aren't in the debian packages
    ln -s clang-${LLVM_VERSION} /usr/bin/clang-cl && ln -s llvm-ar-${LLVM_VERSION} /usr/bin/llvm-lib && ln -s lld-link-${LLVM_VERSION} /usr/bin/lld-link; \
    # Verify the symlinks are correct
    clang++ -v; \
    ld.lld -v; \
    # Doesn't have an actual -v/--version flag, but it still exits with 0
    llvm-lib -v; \
    clang-cl -v; \
    lld-link --version; \
    # Use clang instead of gcc when compiling and linking binaries targeting the host (eg proc macros, build files)
    update-alternatives --install /usr/bin/cc cc /usr/bin/clang 100; \
    update-alternatives --install /usr/bin/c++ c++ /usr/bin/clang++ 100; \
    update-alternatives --install /usr/bin/ld ld /usr/bin/ld.lld 100; \
    rustup target add x86_64-pc-windows-msvc; \
    rustup component add rust-src; \
    # Remove unneeded files to reduce image size
    apt-get remove -y --auto-remove; \
    rm -rf \
        /var/lib/apt/lists/* \
        /root/*.key;

COPY lib/oodle/*.lib /src
COPY --from=xwin /xwin /xwin

# Note that we're using the full target triple for each variable instead of the
# simple CC/CXX/AR shorthands to avoid issues when compiling any C/C++ code for
# build dependencies that need to compile and execute in the host environment
ENV CC_x86_64_pc_windows_msvc="clang-cl" \
    CXX_x86_64_pc_windows_msvc="clang-cl" \
    AR_x86_64_pc_windows_msvc="llvm-lib" \
    # wine can be quite spammy with log messages and they're generally uninteresting
    WINEDEBUG="-all" \
    # Use wine to run test executables
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER="wine" \
    # Note that we only disable unused-command-line-argument here since clang-cl
    # doesn't implement all of the options supported by cl, but the ones it doesn't
    # are _generally_ not interesting.
    CL_FLAGS="-Wno-unused-command-line-argument -fuse-ld=lld-link /imsvc/xwin/crt/include /imsvc/xwin/sdk/include/ucrt /imsvc/xwin/sdk/include/um /imsvc/xwin/sdk/include/shared" \
    # Let cargo know what linker to invoke if you haven't already specified it
    # in a .cargo/config.toml file
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="lld-link" \
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Lnative=/xwin/crt/lib/x86_64 -Lnative=/xwin/sdk/lib/um/x86_64 -Lnative=/xwin/sdk/lib/ucrt/x86_64"

# These are separate since docker/podman won't transform environment variables defined in the same ENV block
ENV CFLAGS_x86_64_pc_windows_msvc="$CL_FLAGS" \
    CXXFLAGS_x86_64_pc_windows_msvc="$CL_FLAGS"

# Run wineboot just to setup the default WINEPREFIX so we don't do it every
# container run
RUN wine wineboot --init
