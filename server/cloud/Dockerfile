# Build only a context created by scripts/prepare-cloud-context.py.
FROM rust:1.99.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/social crates/social
COPY connector connector
COPY server/cloud server/cloud
RUN cargo build --release --locked -p cloud-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3
COPY --from=build /src/target/release/cloud-server /usr/local/bin/cloud-server
COPY web /app/web
COPY web-manifest.json /app/web-manifest.json
ENV CLOUD_BIND=0.0.0.0:8787 CLOUD_WEB_DIR=/app/web CLOUD_ATTACHMENTS_DIR=/data/attachments
USER 10001:10001
ENTRYPOINT ["cloud-server"]
