# Prediction Market Arbitrage System

Cross-venue arbitrage signal generator in Rust that detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options markets (Deribit). Compares prediction market binary contract prices against options-implied probabilities derived via Black-76 pricing with call spread replication.

## AWS Deployment (Soak Test)

### Current Instance

| Resource | Value |
|----------|-------|
| Instance | `i-02bf54cd8b3afa840` (t3.small, us-east-1) |
| Public IP | `98.80.220.161` |
| Health | `http://98.80.220.161:9001/health` |
| Prometheus | `http://98.80.220.161:9000` |
| SSH | `ssh -i secrets/prediction-soak.pem ec2-user@98.80.220.161` |

### AWS Resources

- **ECR repo**: `606103597377.dkr.ecr.us-east-1.amazonaws.com/prediction`
- **EC2 key pair**: `prediction-soak` (private key at `secrets/prediction-soak.pem`)
- **Security group**: `sg-03331d3fca1af8d2c` (SSH 22, Prometheus 9000, Health 9001)
- **IAM role/profile**: `prediction-ecr-pull` (ECR read-only)

### Deploying a New Version

```bash
# Build and push to ECR
aws ecr get-login-password --region us-east-1 | \
  docker login --username AWS --password-stdin 606103597377.dkr.ecr.us-east-1.amazonaws.com
docker build -t prediction:latest .
docker tag prediction:latest 606103597377.dkr.ecr.us-east-1.amazonaws.com/prediction:latest
docker push 606103597377.dkr.ecr.us-east-1.amazonaws.com/prediction:latest

# On the instance
ssh -i secrets/prediction-soak.pem ec2-user@98.80.220.161
cd /opt/prediction
aws ecr get-login-password --region us-east-1 | \
  docker login --username AWS --password-stdin 606103597377.dkr.ecr.us-east-1.amazonaws.com
docker compose pull
docker compose up -d
```

### Useful Commands

```bash
# SSH in
ssh -i secrets/prediction-soak.pem ec2-user@98.80.220.161

# View live logs
cd /opt/prediction && docker compose logs -f

# Check health
curl http://98.80.220.161:9001/health

# Restart
cd /opt/prediction && docker compose restart

# Update config (edit locally, then push)
scp -i secrets/prediction-soak.pem config/events.toml ec2-user@98.80.220.161:/opt/prediction/config/
```

### Retrieving Logs for Analysis

```bash
# Pull spread logs
scp -i secrets/prediction-soak.pem -r ec2-user@98.80.220.161:/opt/prediction/spread_logs ./

# Pull settlement logs
scp -i secrets/prediction-soak.pem -r ec2-user@98.80.220.161:/opt/prediction/settlement_logs ./

# Run analysis locally
./target/release/spread-analytics --input ./spread_logs --mode distribution
./target/release/signal-scoring --input ./settlement_logs
```

### Instance Data Directories

| Path | Purpose |
|------|---------|
| `/opt/prediction/config/` | TOML config files (config, venues, events) |
| `/opt/prediction/secrets/` | Kalshi private key PEM |
| `/opt/prediction/spread_logs/` | JSONL spread data |
| `/opt/prediction/settlement_logs/` | Settlement outcome JSONL |
| `/opt/prediction/paper_trades/` | Paper trade P&L |
| `/opt/prediction/state/` | Atomic checkpoints |
| `/opt/prediction/logs/` | Application logs |

### Bootstrapping a Fresh Instance

```bash
# 1. Run bootstrap script as root
sudo bash deploy/aws-setup.sh

# 2. Copy files
scp config/*.toml ec2-user@<host>:/opt/prediction/config/
scp .env ec2-user@<host>:/opt/prediction/.env
scp secrets/kalshi_private_key.pem ec2-user@<host>:/opt/prediction/secrets/
scp docker-compose.yml ec2-user@<host>:/opt/prediction/

# 3. Login to ECR and start
aws ecr get-login-password --region us-east-1 | \
  docker login --username AWS --password-stdin 606103597377.dkr.ecr.us-east-1.amazonaws.com
cd /opt/prediction && docker compose pull && docker compose up -d
```

### Environment Variables

| Variable | Source | Description |
|----------|--------|-------------|
| `KALSHI_API_KEY_ID` | `.env` file | Kalshi API key ID |
| `KALSHI_PRIVATE_KEY_PATH` | Set in compose | Path to PEM inside container (`/app/secrets/kalshi_private_key.pem`) |

### Ports

| Port | Service |
|------|---------|
| 9000 | Prometheus metrics exporter |
| 9001 | HTTP health endpoint (`/health`) |

## TODO: Before Soak Test Can Collect Data

The system needs at least 2 venues with overlapping BTC binary markets to generate cross-venue spreads and signals. Active venue pair: **Deribit + Polymarket**.

### 1. ~~Fix Kalshi 401 Unauthorized~~ — DEPRIORITIZED

Kalshi is US-only and inaccessible from Poland. Cannot check dashboard or regenerate API keys. Deribit + Polymarket provide sufficient venue coverage for the soak test.

If Kalshi access becomes available later:
- Check dashboard for API key status (`d8b5f11e-8f71-4af9-ac1a-cd2b0c32ef00`)
- If expired, generate new key and convert to PKCS#8: `openssl pkcs8 -topk8 -inform PEM -outform PEM -nocrypt -in key.pem -out key_pkcs8.pem`
- Upload and restart

### 2. ~~Verify Polymarket Has Active BTC Markets~~ — FIXED

Discovery slug patterns were wrong (`what-price-will-bitcoin-hit-in-march` instead of `what-price-will-bitcoin-hit-in-march-2026`). Fixed in `src/config/events.rs` and `src/events/discovery.rs` — added `{next_year}` placeholder and corrected default patterns. Gamma API now returns ~47 active BTC binary markets across monthly and annual events.

- [x] Check Polymarket manually for active "BTC above $X" type markets
- [x] If markets exist but aren't found, review discovery slug config

### 3. Deploy Slug Fix and Approve Event Mappings

Deploy the Polymarket slug fix, then approve discovered cross-venue mappings.

- [ ] Build and push new image to ECR
- [ ] Pull and restart on instance
- [ ] Wait for a discovery cycle (~5 min)
- [ ] Check for proposals: `ssh ... "cat /opt/prediction/config/events.toml"`
- [ ] Set `approved = true` on good matches and push the file back
- [ ] The system picks up changes via config hot-reload (no restart needed)

### 4. Config Reload Spam (NON-BLOCKING)

The `notify` file watcher fires every ~500ms on the Docker bind mount (Linux inotify behavior). Not harmful but noisy. Low priority — investigate later if log volume becomes a problem.

### Soak Test Timeline

Once Deribit + Polymarket are connected and event mappings approved, let it run 2-3 weeks to accumulate enough settled outcomes. Then pull logs and run:

```bash
scp -i secrets/prediction-soak.pem -r ec2-user@98.80.220.161:/opt/prediction/spread_logs ./
scp -i secrets/prediction-soak.pem -r ec2-user@98.80.220.161:/opt/prediction/settlement_logs ./
./target/release/spread-analytics --input ./spread_logs --mode distribution
./target/release/signal-scoring --input ./settlement_logs
```
