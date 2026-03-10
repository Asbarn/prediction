# Prediction Market Arbitrage System

Cross-venue arbitrage signal generator in Rust that detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options markets (Deribit). Compares prediction market binary contract prices against options-implied probabilities derived via Black-76 pricing with call spread replication.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│               Prediction Market Arb System                   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │Polymarket│  │  Kalshi  │  │  Deribit  │  │  Derive  │   │
│  │  Feed    │  │  Feed    │  │  Feed     │  │  Feed    │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘   │
│       └──────────────┴─────────────┴──────────────┘         │
│                          │                                   │
│  ┌───────────────────────▼──────────────────────┐           │
│  │          Unified Market Data Bus              │           │
│  │   (normalized books, funding, positions)      │           │
│  └───────────────────────┬──────────────────────┘           │
│                          │                                   │
│  ┌───────────────────────▼──────────────────────┐           │
│  │          Spread Engine                        │           │
│  │   (cross-venue comparison, signal scoring)    │           │
│  └───────────────────────┬──────────────────────┘           │
│                          │                                   │
│  ┌───────────────────────▼──────────────────────┐           │
│  │          Analytics & Monitoring               │           │
│  │   (P&L tracking, Prometheus, Grafana)         │           │
│  └──────────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────────┘
```

## Features

- **Multi-venue WebSocket feeds** — real-time order book streaming from Polymarket, Deribit, Derive, Kalshi
- **Black-76 implied probability** — converts options markets to binary probabilities via call spread replication
- **Cross-venue spread detection** — identifies pricing discrepancies between prediction and options markets
- **Signal scoring** — statistical validation of arbitrage signals with autocorrelation-corrected confidence intervals
- **Cost modeling** — fee-aware opportunity filtering (maker/taker fees, slippage estimation)
- **Event lifecycle management** — automatic discovery, approval, and expiry of cross-venue event mappings
- **Prometheus metrics** — full observability with custom Grafana dashboards
- **CLI diagnostic tools** — spread-analytics, signal-scoring, match-audit, cost-audit, book-depth, cost-validate, go-no-go

## Stack

- **Rust** with tokio async runtime
- **rust_decimal** for financial calculations
- **tokio-tungstenite** for WebSocket connections
- **tracing** for structured logging
- **prometheus** for metrics
- **axum** for health/metrics HTTP endpoints
- **AWS CDK** (TypeScript) for infrastructure-as-code
- **Docker** for containerized deployment

## Building

```bash
cargo build --release
```

## Configuration

Configuration is split across three TOML files in `config/`:

- `config.toml` — general settings, thresholds, logging
- `venues.toml` — exchange API endpoints and credentials (via env vars)
- `events.toml` — cross-venue event mappings (auto-discovered + manually approved)

## CLI Tools

```bash
# Spread distribution analysis
./target/release/spread-analytics --input ./spread_logs --mode distribution

# Signal quality scoring
./target/release/signal-scoring --input ./settlement_logs

# Go/no-go decision report
./target/release/go-no-go --signal-dir ./signal_logs --spread-dir ./spread_logs

# Order book depth analysis
./target/release/book-depth --input ./spread_logs

# Cost model validation
./target/release/cost-validate --input ./spread_logs
```

## Docker

```bash
# Build
docker build -t prediction:latest .

# Run locally
docker compose up -d
```

## Infrastructure

The `infra/cdk/` directory contains AWS CDK infrastructure-as-code for deploying to EC2 with:
- ECR for container registry
- Secrets Manager for API credentials
- CloudWatch for log aggregation
- Amazon Managed Prometheus for metrics
- Self-hosted Grafana OSS for dashboards

## Project Status

**Concluded.** Go/no-go analysis after multi-week soak test showed mean edge of -0.444 with 0% hit rate. Transaction costs exceeded every identified opportunity. The system works correctly as an engineering artifact but the underlying arbitrage strategy is not profitable.

## Ports

| Port | Service |
|------|---------|
| 9000 | Prometheus metrics exporter |
| 9001 | HTTP health endpoint (`/health`) |
