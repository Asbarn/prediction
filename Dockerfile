# ---- Planner stage (cargo-chef) ----
FROM rust:1.85 AS planner

RUN cargo install cargo-chef

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo chef prepare --recipe-path recipe.json

# ---- Builder stage (cargo-chef cook + build) ----
FROM rust:1.85 AS builder

RUN cargo install cargo-chef

RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cook dependencies from recipe (this layer is cached when deps unchanged)
COPY --from=planner /build/recipe.json recipe.json
ENV CARGO_BUILD_JOBS=2
RUN cargo chef cook --release --recipe-path recipe.json

# Build application (only this layer rebuilds on source changes)
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
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
