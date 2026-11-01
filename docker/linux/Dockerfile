FROM alpine@sha256:020dfcbaaf4cc1078bf2d9c7ba31a8466e334061dcd2f248001d68f79e52c000

RUN apk add --no-cache \
        bash build-base clang cmake meson ninja samurai pkgconf python3 \
        linux-headers git curl xz bzip2 tar perl nasm patchelf file \
        util-linux coreutils findutils diffutils time \
        wayland-dev wayland-protocols libxkbcommon-dev alsa-lib-dev \
        vulkan-loader-dev vulkan-headers \
        mesa-vulkan-swrast weston weston-backend-headless weston-clients \
        ffmpeg rustup gperf

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN rustup-init -y --profile minimal --default-toolchain none \
 && rustup toolchain install nightly-2026-09-15 --profile minimal \
        -c rust-src -c clippy -t x86_64-unknown-linux-musl \
 && chmod -R a+rX /opt/rustup /opt/cargo

WORKDIR /work
