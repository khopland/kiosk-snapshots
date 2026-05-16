FROM rust:bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
COPY migrations ./migrations

RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    chromium \
    ca-certificates \
    tzdata \
    fonts-dejavu-core \
    fonts-noto-core \
    fonts-noto-cjk \
    && rm -rf /var/lib/apt/lists/*

ENV APP_BIND=0.0.0.0:3000
ENV APP_DATA_DIR=/data
ENV RUST_LOG=info

VOLUME ["/data/db", "/data/captures"]
EXPOSE 3000

COPY --from=builder /app/target/release/screenshot-pi /usr/local/bin/screenshot-pi

CMD ["screenshot-pi"]
