# The Linux Incipit is built and run in by compose.yaml, the same on any system that has Docker
# (docs/development.md, "In Docker"): Ubuntu 24.04, what GPUI links against (the packages of the
# Linux job in .github/workflows/ci.yml, which this list follows), Rust, and a screen the window
# draws on, seen in a browser.
#
# - The window draws with Vulkan; a container has no GPU, so Mesa's software Vulkan (lavapipe, in
#   mesa-vulkan-drivers) draws it, on the processor.
# - The screen is virtual (Xvfb), shared over VNC (x11vnc) and shown in a browser by noVNC; the
#   `desktop` script starts the three.
# - Fonts: Inter, the first the app asks for on Linux, and DejaVu Sans Mono for monospace.
# - The source is not in the image: compose.yaml shares the checkout at /work.
# - The user is `dev`, with the host user's numbers on Linux (INCIPIT_UID, INCIPIT_GID in
#   compose.yaml), so files written into the checkout belong to the person, not to root. Ubuntu's
#   image has a user `ubuntu` with 1000, which is taken out first.
#
# The base image is pinned by digest (the multi-platform index, for x86_64 and arm64): a tag can
# be moved, a digest cannot. Rust is the stable release of the day the image is built, as CI's is.

FROM ubuntu:24.04@sha256:73b999393d07f2bf65a40373778a44dc9a6d54ad7718d29247d8ff9413c4d6a8

ARG UID=1000
ARG GID=1000

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential pkg-config clang libclang-dev cmake lld \
        libasound2-dev libudev-dev \
        libx11-dev libx11-xcb-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
        libxcursor-dev libxrandr-dev libxi-dev libfontconfig-dev libfreetype-dev libxcb1-dev \
        libdbus-1-dev \
        libvulkan1 mesa-vulkan-drivers \
        xvfb x11vnc novnc python3-websockify \
        fonts-inter fonts-dejavu-core \
        ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/*

RUN if id ubuntu >/dev/null 2>&1; then userdel --remove ubuntu; fi \
    && groupadd --gid "$GID" dev \
    && useradd --uid "$UID" --gid "$GID" --create-home --shell /bin/bash dev \
    && mkdir -p /work/target /work/lib \
    && chown -R dev:dev /work

# Docker Desktop (macOS, Windows) shows the shared checkout's top folder as root's, and git refuses
# a repository another user owns ("dubious ownership"). /work is the person's own checkout, shared
# on purpose.
RUN git config --system --add safe.directory /work

# --chmod: a checkout on Windows does not keep the file runnable.
COPY --chmod=755 desktop /usr/local/bin/desktop

USER dev
ENV PATH=/home/dev/.cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --component clippy,rustfmt \
    && mkdir -p /home/dev/.cargo/registry /home/dev/.cargo/git
# Programs are linked with LLVM's lld, not GNU ld (arm64 Linux's default): GNU ld was killed for
# want of memory linking the debug build in a Docker Desktop VM with 7.7 GiB.
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"

WORKDIR /work
ENTRYPOINT ["desktop"]
CMD ["sleep", "infinity"]
