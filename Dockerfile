# Build stage
FROM rust:1-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release

# Runtime stage — debian-slim + wireguard-tools so the mesh module can drive
# the kernel WireGuard via wg-quick/wg when the container runs with NET_ADMIN.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        iproute2 \
        wireguard-tools \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/sdwanlited /usr/local/bin/sdwanlited
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/sdwanlited"]
CMD ["sdwanlite.toml"]
