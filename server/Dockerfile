# Build from the repository root, which holds the protocol crate the server shares with the browser:
#   docker build -f server/Dockerfile -t vsesvit-sync-server .
#
# The binary links musl statically, so the runtime image is Alpine with CA certificates and
# nothing else.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY crates/vsesvit-sync-proto crates/vsesvit-sync-proto
COPY server server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/server/target \
    cd server \
    && cargo build --release --locked \
    && install -Dm755 target/release/vsesvit-sync-server /out/vsesvit-sync-server

FROM alpine:3
RUN apk add --no-cache ca-certificates \
    && addgroup -S -g 10001 vsesvit \
    && adduser -S -D -H -u 10001 -G vsesvit -h /data vsesvit \
    && mkdir /data \
    && chown vsesvit:vsesvit /data
COPY --from=build /out/vsesvit-sync-server /usr/local/bin/vsesvit-sync-server
USER vsesvit
WORKDIR /data
ENV DATABASE_URL=sqlite:///data/vsesvit-sync.db?mode=rwc \
    BIND_ADDRESS=0.0.0.0:8080
VOLUME /data
EXPOSE 8080
CMD ["vsesvit-sync-server"]
