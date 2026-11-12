FROM alpine@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6

RUN apk add --no-cache \
        bash=5.3.9-r1 build-base=0.5-r4 clang22=22.1.3-r2 pkgconf=2.5.1-r0 python3=3.14.10-r1 \
        linux-headers=7.0.0-r1 git=2.54.1-r0 curl=8.23.0-r0 xz=5.8.4-r0 bzip2 tar=1.35-r5 perl=5.42.3-r0 patchelf file=5.47-r2 \
        util-linux coreutils=9.11-r0 findutils diffutils time \
        wayland-dev wayland-protocols alsa-lib-dev \
        vulkan-loader-dev vulkan-headers \
        mesa-vulkan-swrast weston weston-backend-headless weston-clients weston-shell-desktop font-dejavu fontconfig \
        libva-dev libdrm-dev mesa-va-gallium libva-utils \
        rustup ripgrep \
        harfbuzz-dev freetype-dev \
        font-noto font-noto-arabic font-noto-hebrew font-noto-devanagari font-noto-thai \
        font-noto-cjk font-noto-emoji

RUN archive=ffmpeg-fixtures-9.0.2-x86_64-unknown-linux-musl.tar.xz \
 && curl -fsSL --retry 3 "https://github.com/laputa-systems/ffmpeg-build/releases/download/release-ac29c7720327f7dee4ed4677ce8421673fcc3563/$archive" -o "/tmp/$archive" \
 && echo "87e2bab4b54d2454ff34b9ac501cb2a4b53a2c7744538b46b114322db2f273d9  /tmp/$archive" | sha256sum -c - \
 && mkdir -p /opt/ffmpeg-fixtures \
 && tar -xJf "/tmp/$archive" -C /opt/ffmpeg-fixtures --strip-components=1 \
 && rm "/tmp/$archive"

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/ffmpeg-fixtures/bin:/opt/cargo/bin:$PATH
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
