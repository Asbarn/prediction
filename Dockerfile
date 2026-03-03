# ---- Builder stage ----
FROM rust:latest AS builder

RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

ENV CARGO_BUILD_JOBS=2
RUN cargo build --release

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/prediction .
COPY --from=builder /build/target/release/spread-analytics .
COPY --from=builder /build/target/release/signal-scoring .

EXPOSE 9000 9001

CMD ["./prediction", "--config-dir", "config"]
