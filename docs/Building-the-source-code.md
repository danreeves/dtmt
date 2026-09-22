## Build with Docker

### Prerequisites

* [Docker](https://docs.docker.com/desktop/)
* [Git](https://git-scm.com/)

### Steps

The Docker images set up all the requirements and dependencies for both the native Linux build, and the cross-platform Windows build.

1. Clone the repository. Make sure to initialize submodules (e.g. `git clone --recurse-submodules`).
2. Place the Oodle library files into `lib/oodle`. See [Obtaining-the-Oodle-library](Obtaining-the-Oodle-library) for how to acquire them.
3. Build the Linux image, which also serves as base for the cross-platform Windows image: `docker build -t dtmt-ci-base-linux -f .ci/image/Dockerfile.linux .`
4. (Optional) Build the Windows image: `docker build -t dtmt-ci-base-msvc -f .ci/image/Dockerfile.msvc .`
5. Run the build command:
  - Linux: `docker run --rm -ti --user $(id -u) -v ./:/src/dtmt dtmt-ci-base-linux cargo --color always build --profile release-lto --locked`
  - Windows: `docker run --rm -ti --user $(id -u) -v ./:/src/dtmt dtmt-ci-base-msvc cargo --color always build --release --target x86_64-pc-windows-msvc --locked -Zbuild-std`
6. Find the exectuables in `target/release-lto` or `target/x86_64-pc-windows-msvc/release`.

## Native build

### Prerequisites

* On Windows: [MSVC](https://visualstudio.microsoft.com/vs/features/cplusplus/)
* On Linux: The usual `build-essential` packages
* [Rust](https://www.rust-lang.org/tools/install)
* [Git](https://git-scm.com/)
* [CMake](https://cmake.org/)
* [Oodle](Obtaining-the-Oodle-library)

### Steps

1. Clone the repository. Make sure to initialize submodules (e.g. `git clone --recurse-submodules`).
2. Place the Oodle library files into `lib/oodle`. See the linked page above for how to acquire them.
3. Run `cargo build --release`.
4. Find the executables in `target/release`.