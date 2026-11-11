FROM alpine@sha256:020dfcbaaf4cc1078bf2d9c7ba31a8466e334061dcd2f248001d68f79e52c000

RUN apk add --no-cache \
        bash=5.3.9-r2 build-base=0.5-r4 clang muon meson samurai pkgconf=3.0.10-r0 python3=3.14.10-r1 \
        linux-headers=7.2.1-r0 git=2.56.1-r0 curl=8.23.0-r0 xz=5.8.4-r0 bzip2 tar=1.35-r5 perl=5.44.0-r0 patchelf file=5.48-r0 \
        util-linux coreutils=9.11-r3 findutils diffutils time \
        wayland-dev wayland-protocols alsa-lib-dev \
        vulkan-loader-dev vulkan-headers \
        mesa-vulkan-swrast weston weston-backend-headless weston-clients weston-shell-desktop font-dejavu fontconfig \
        libva-dev libdrm-dev mesa-va-gallium libva-utils \
        ffmpeg rustup ripgrep \
        harfbuzz-dev freetype-dev \
        font-noto font-noto-arabic font-noto-hebrew font-noto-devanagari font-noto-thai \
        font-noto-cjk font-noto-emoji

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN rustup-init -y --profile minimal --default-toolchain none

COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN channel="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' /tmp/rust-toolchain.toml)" \
 && test -n "$channel" \
 && rustup toolchain install "$channel" --profile minimal \
        -c rust-src -c clippy -t x86_64-unknown-linux-musl \
 && rustup default "$channel" \
 && rm /tmp/rust-toolchain.toml \
 && chmod -R a+rX /opt/rustup /opt/cargo

WORKDIR /work
