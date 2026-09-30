# Build from the repository root, which holds the protocol crate the server shares with the browser:
#   docker build -f server/Dockerfile -t vsesvit-sync-server .
FROM rust:1-trixie AS build
WORKDIR /src
COPY crates/vsesvit-sync-proto crates/vsesvit-sync-proto
COPY server server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/server/target \
    cd server \
    && cargo build --release --locked \
    && cp target/release/vsesvit-sync-server /vsesvit-sync-server

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data vsesvit \
    && mkdir /data \
    && chown vsesvit /data
COPY --from=build /vsesvit-sync-server /usr/local/bin/vsesvit-sync-server
USER vsesvit
WORKDIR /data
ENV DATABASE_URL=sqlite:///data/vsesvit-sync.db?mode=rwc \
    BIND_ADDRESS=0.0.0.0:8080
VOLUME /data
EXPOSE 8080
CMD ["vsesvit-sync-server"]
