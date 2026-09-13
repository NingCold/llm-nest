FROM ubuntu:24.04

ARG RUST_VERSION=1.97.0
ARG NODE_VERSION=24.18.0
ARG PNPM_VERSION=11.22.0
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential ca-certificates curl file git pkg-config libssl-dev \
    libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
    python3 xvfb xauth dbus-x11 libfuse2t64 \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
ENV PATH=/opt/cargo/bin:/opt/node/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup.sh \
    && sh /tmp/rustup.sh -y --profile minimal --default-toolchain "$RUST_VERSION" \
    && rustup component add rustfmt clippy \
    && rm /tmp/rustup.sh
RUN case "$(dpkg --print-architecture)" in amd64) arch=x64 ;; arm64) arch=arm64 ;; *) exit 1 ;; esac \
    && cd /tmp \
    && curl -fsSLO "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${arch}.tar.xz" \
    && curl -fsSLO "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" \
    && grep " node-v${NODE_VERSION}-linux-${arch}.tar.xz\$" SHASUMS256.txt | sha256sum -c - \
    && mkdir /opt/node \
    && tar -xJf "node-v${NODE_VERSION}-linux-${arch}.tar.xz" --strip-components=1 -C /opt/node \
    && npm install -g "pnpm@${PNPM_VERSION}" \
    && rm node-*.tar.xz SHASUMS256.txt

ENV APPIMAGE_EXTRACT_AND_RUN=1
WORKDIR /workspace
CMD ["bash", "scripts/linux/build.sh"]
