# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-03-07
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.6 Requirements

Requirements for production deployment milestone. Each maps to roadmap phases.

### Infrastructure

- [ ] **INFRA-01**: CDK stack provisions VPC, security groups, EC2 instance, and IAM instance profile in a single `cdk deploy`
- [ ] **INFRA-02**: CDK imports existing ECR repository rather than creating a duplicate
- [ ] **INFRA-03**: EC2 user-data installs Docker, CloudWatch agent, and configures systemd service for docker-compose auto-start
- [ ] **INFRA-04**: IAM instance profile grants least-privilege access to ECR pull, CloudWatch Logs, Secrets Manager read, and AMP remote write
- [ ] **INFRA-05**: Separate EBS volume for persistent data (state, logs, config) survives instance replacement
- [ ] **INFRA-06**: CDK provisions CloudWatch log group with 14-day retention policy
- [ ] **INFRA-07**: CDK provisions Secrets Manager secrets for venue API credentials

### CI/CD

- [ ] **CICD-01**: GitLab CI pipeline runs `cargo test` on every push to master
- [ ] **CICD-02**: Pipeline builds Docker image and pushes to ECR on successful test
- [ ] **CICD-03**: Pipeline deploys to EC2 via SSM Send-Command (stop, pull, start container)
- [ ] **CICD-04**: Build uses cargo-chef layer caching to reduce Rust compile times below 10 minutes
- [ ] **CICD-05**: Pipeline deploy stage verifies /health endpoint responds after container start

### Monitoring

- [ ] **MON-01**: Docker Compose uses awslogs driver to ship structured JSON logs to CloudWatch
- [ ] **MON-02**: Prometheus sidecar scrapes :9001/metrics and remote_writes to Amazon Managed Prometheus with SigV4 auth
- [ ] **MON-03**: Amazon Managed Grafana workspace connects to AMP as data source
- [ ] **MON-04**: Grafana dashboard: Feed Health (feed_available per venue, reconnection rate, message latency)
- [ ] **MON-05**: Grafana dashboard: Signal Quality (arb_signals_emitted, net_edge_bps, confidence, staleness rejections)
- [ ] **MON-06**: Grafana dashboard: Paper Trade P&L (daily_pnl, win_rate, net_pnl, settlement latency)
- [ ] **MON-07**: Grafana dashboard: System Health (active expiries, subscriptions, lifecycle polls, proposals, alerts)
- [ ] **MON-08**: Grafana alert rules for: feed down, zero spread computations 30min, high staleness rejection rate
- [ ] **MON-09**: CloudWatch agent reports EC2 host metrics (CPU, memory, disk)

### Hardening

- [ ] **HARD-01**: fetch-secrets.sh script retrieves credentials from Secrets Manager and exports as environment variables before container start
- [ ] **HARD-02**: Systemd unit runs docker-compose, auto-starts on boot, restarts on failure
- [ ] **HARD-03**: Container handles SIGTERM gracefully (flush checkpoints, close WebSocket connections, exit cleanly)

## Future Requirements

### Execution Engine (v2)
- **EXEC-01**: Order placement via venue APIs (Deribit, Polymarket)
- **EXEC-02**: Position tracking and real-time P&L
- **EXEC-03**: Risk limits engine and kill switch
- **EXEC-04**: Margin monitoring

### Multi-Asset
- **MULTI-01**: ETH binary event support
- **MULTI-02**: SOL binary event support

## Out of Scope

| Feature | Reason |
|---------|--------|
| ECS/Fargate deployment | Massive complexity for one container; Docker Compose on EC2 is correct abstraction |
| Multi-AZ / auto-scaling | Single instance by design; downtime tolerance is minutes |
| Blue/green deployments | Solo trader tolerates 30-second restart; infra cost exceeds benefit |
| Kubernetes / EKS | Orchestration overkill for single container |
| Terraform | CDK chosen for TypeScript-native AWS constructs |
| Self-hosted Prometheus + Grafana | Hosting monitoring on same EC2 defeats purpose; use managed services |
| Separate staging environment | Paper trading IS staging; no real money at risk until v2 |
| Container image scanning | Private single-user system; defer Trivy to post-v1.6 |
| Log database (ELK/Loki) | CloudWatch Logs Insights sufficient for modest log volume |
| Custom AMI | User-data bootstrap is 2-3 min; AMI lifecycle management not worth it |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| INFRA-01 | Phase 34 | Pending |
| INFRA-02 | Phase 34 | Pending |
| INFRA-03 | Phase 35 | Pending |
| INFRA-04 | Phase 34 | Pending |
| INFRA-05 | Phase 34 | Pending |
| INFRA-06 | Phase 34 | Pending |
| INFRA-07 | Phase 34 | Pending |
| CICD-01 | Phase 38 | Pending |
| CICD-02 | Phase 38 | Pending |
| CICD-03 | Phase 38 | Pending |
| CICD-04 | Phase 38 | Pending |
| CICD-05 | Phase 38 | Pending |
| MON-01 | Phase 36 | Pending |
| MON-02 | Phase 37 | Pending |
| MON-03 | Phase 37 | Pending |
| MON-04 | Phase 39 | Pending |
| MON-05 | Phase 39 | Pending |
| MON-06 | Phase 39 | Pending |
| MON-07 | Phase 39 | Pending |
| MON-08 | Phase 39 | Pending |
| MON-09 | Phase 36 | Pending |
| HARD-01 | Phase 35 | Pending |
| HARD-02 | Phase 35 | Pending |
| HARD-03 | Phase 35 | Pending |

**Coverage:**
- v1.6 requirements: 24 total
- Mapped to phases: 24
- Unmapped: 0

---
*Requirements defined: 2026-03-07*
*Last updated: 2026-03-07 after roadmap creation*
