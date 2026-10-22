FROM rust:1.99.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY agent agent
COPY connector connector
COPY server server
COPY client/desktop/src-tauri client/desktop/src-tauri
RUN cargo build --release --locked -p cloud-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/cloud-server /usr/local/bin/cloud-server
COPY client/web/dist /app/web
ENV CLOUD_BIND=0.0.0.0:8787 CLOUD_WEB_DIR=/app/web CLOUD_ATTACHMENTS_DIR=/data/attachments
USER 10001:10001
ENTRYPOINT ["cloud-server"]
