FROM rust:1-bookworm AS builder
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./
COPY server/src ./src
COPY server/templates ./templates
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/tvdisplay /usr/local/bin/tvdisplay
ENV HOST=::
ENV PORT=8080
ENV DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD ["tvdisplay", "health"]
CMD ["tvdisplay"]
