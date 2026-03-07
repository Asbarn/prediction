# Feature Landscape

**Domain:** Production deployment infrastructure for a Rust single-binary crypto arbitrage service on AWS
**Researched:** 2026-03-07
**Confidence:** HIGH (well-established AWS patterns; system already has Docker/ECR/Prometheus foundations)

**Scope note:** This research covers ONLY v1.6 production deployment features. The arbitrage system itself (4-venue feeds, spread engines, signal generation, paper trading, analysis CLIs) is complete and operational at 39,176 LOC Rust. This milestone wraps it in production-grade infrastructure.

**Existing infrastructure this builds on:**

| Asset | Location | Status | How v1.6 Uses It |
|-------|----------|--------|-------------------|
| Multi-stage Dockerfile | `Dockerfile` | Complete | CI/CD pipeline builds from this; no changes needed |
| Docker Compose | `docker-compose.yml` | Complete | Production deployment template; logging driver changes for CloudWatch |
| ECR repository | `606103597377.dkr.ecr.us-east-1.amazonaws.com/prediction` | Live | Import into CDK; CI/CD pushes here |
| Prometheus metrics (80+) | Port 9001, `metrics::counter!/gauge!/histogram!` | Complete | Grafana dashboards consume these; zero instrumentation work |
| Health endpoint | `GET /health` on port 9001 | Complete | Deployment verification, ALB health checks if needed |
| Structured JSON logging | tracing-subscriber JSON formatter | Complete | CloudWatch Logs Insights auto-discovers all fields |
| Env-var credentials | `src/config/credentials.rs` | Complete | Secrets Manager injects via env vars; no code changes |
| Log rotation | Docker json-file driver, 50m x 3 | Complete | Replaced by CloudWatch awslogs driver in production |
| Health checks + auto-restart | docker-compose.yml | Complete | Systemd unit wraps docker-compose on EC2 |

---

## Table Stakes

Features required for the system to be considered "production deployed." Missing any = manual operations remain, infrastructure is undocumented, or the system is fragile.

| Feature | Why Expected | Complexity | Dependencies | Notes |
|---------|--------------|------------|--------------|-------|
| CDK infrastructure stack | Console-created resources are undocumented, unreproducible, drift-prone. IaC is the baseline for any production system. | Medium-High | Node.js, TypeScript, aws-cdk-lib | VPC, SG, EC2, IAM, ECR (import), CW log group, Secrets Manager |
| GitLab CI pipeline | Manual build-push-SSH cycle is error-prone and slow. Blocks iteration velocity. | Medium | CDK deployed first (ECR, EC2 exist) | Stages: cargo test, docker build, ECR push, EC2 deploy |
| CloudWatch log aggregation | Logs on EC2 instance disk are inaccessible during incidents, lost on instance termination | Low | EC2 IAM role with logs permissions, awslogs Docker driver | Change docker-compose logging from json-file to awslogs. Zero code changes. |
| Secrets Manager integration | API keys in plaintext env vars or files on disk are a security liability | Low-Med | CDK creates secrets, entrypoint script fetches them | Wrapper script: aws secretsmanager get-secret-value, export as env vars, exec container |
| Systemd service for Docker Compose | docker-compose must survive EC2 reboot without manual SSH | Low | EC2 user-data in CDK | systemd unit that runs docker-compose up on boot |
| EC2 instance profile (IAM) | Container needs permissions for ECR pull, CloudWatch logs, Secrets Manager, AMP remote write | Low-Med | CDK IAM role + instance profile | Single role with least-privilege policies |

---

## Differentiators

Features that elevate from "deployed" to "well-operated." Not strictly required -- system runs fine with log-based monitoring -- but high-value for unattended operation.

| Feature | Value Proposition | Complexity | Dependencies | Notes |
|---------|-------------------|------------|--------------|-------|
| Grafana dashboard: Feed Health | Real-time venue connectivity: feed_available (4 venues), reconnection rates, message latency heatmap. Replaces SSH + grep. | Medium | Amazon Managed Grafana + AMP | 4-panel row per venue; reconnection spike = investigate |
| Grafana dashboard: Signal Quality | Core business metrics: arb_signals_emitted rate, net_edge_bps distribution, confidence histogram, staleness rejections. "Are we finding real opportunities?" | Medium | Same workspace | Time-series + histogram panels; edge erosion visible immediately |
| Grafana dashboard: Spread Distributions | spread_net by event/pattern, rolling mean+stddev. Reveals market regime shifts. | Medium | Same workspace | Per-event drill-down; stddev expansion = volatility regime change |
| Grafana dashboard: Paper Trade P&L | daily_pnl, win_rate, open positions, net_pnl distribution, settlement latency. "Is the strategy making money?" | Medium | Same workspace | The bottom line; sustained negative = re-evaluate thresholds |
| Grafana dashboard: System Health | pricing_active_expiries, subscription_active counts, lifecycle polls, proposals_pending, alert_active, checkpoints_written | Low-Med | Same workspace | Operational health of all subsystems |
| Grafana alert rules | Alerts for: feed_available=0 (any venue), zero spread_computations for 30min, sustained negative P&L, high staleness rejection rate | Low-Med | Grafana alerting + SNS | Email or SNS notification; supplements existing alert_monitor |
| Prometheus remote write to AMP | Durable metrics storage beyond EC2 lifecycle. Without this, metrics vanish on instance replacement. | Medium | AMP workspace, ADOT collector sidecar or Prometheus remote_write | ADOT collector in docker-compose scrapes :9001, remote_writes to AMP |
| CloudWatch Logs Insights saved queries | Pre-built queries for: error rate by target, venue-specific errors, signal events, settlement outcomes, staleness warnings | Low | CW log group exists | JSON auto-discovery means field.name dot notation works immediately |
| EC2 host metrics via CloudWatch agent | CPU, memory, disk usage on the host. Catch resource exhaustion before it causes feed drops. | Low | CW agent in EC2 user-data | Pre-configured in CDK; standard amazon-cloudwatch-agent package |

---

## Anti-Features

Features to explicitly NOT build for v1.6. The system is a single container for a solo trader -- resist infrastructure complexity.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| ECS/Fargate deployment | Massive complexity for one container. Task definitions, service mesh, load balancers, target groups -- none needed for a single instance. ECS adds 5+ new resource types to manage. | Docker Compose on EC2 with systemd. SSH-accessible, debuggable, simple. |
| Multi-AZ / auto-scaling | Arb detection does not benefit from horizontal scaling. The system is one instance by design. Downtime tolerance is minutes, not seconds. | Single EC2 in one AZ. Accept brief downtime on instance failure. |
| Blue/green or canary deployments | Solo trader tolerates a 30-second restart. Rolling deployment infra (two target groups, ALB, CodeDeploy) costs more than it saves. | Stop-pull-start: stop container, pull new ECR image, docker-compose up. Health check confirms recovery. |
| Kubernetes / EKS | Orchestration overkill. K8s adds cluster management, node groups, ingress controllers, helm charts -- for one pod. | Docker Compose is the correct abstraction for a single container. |
| Terraform instead of CDK | CDK is TypeScript-native with superior L2/L3 constructs for AWS. Terraform's multi-cloud flexibility is wasted when targeting AWS exclusively. CDK synthesizes to CloudFormation natively. | AWS CDK with TypeScript. |
| Self-hosted Prometheus + Grafana | Hosting monitoring infrastructure on the same EC2 defeats the purpose. Prometheus and Grafana have their own operational burden (storage, upgrades, auth). | Amazon Managed Grafana + Amazon Managed Prometheus (AMP). AWS manages availability. |
| CI/CD via CodePipeline / CodeDeploy | Adds AWS service complexity when GitLab CI is already the SCM platform. CodeDeploy needs an agent, appspec.yml, deployment groups. | GitLab CI stages with direct SSH or SSM deploy command. |
| Separate staging environment | Paper trading IS the staging environment. The system has no real money at risk until v2 execution. A second environment doubles infrastructure cost with no safety benefit. | Single environment running in paper-trade mode. |
| Container image scanning in CI | Nice-to-have for public-facing services. This is a private single-user system with no inbound traffic except health checks. | Defer. Add Trivy scan as non-blocking CI step post-v1.6 if desired. |
| Log database (ELK, Loki) | CloudWatch Logs Insights handles structured JSON queries. The system generates modest log volume (not GB/hour). A log database adds operational burden. | CloudWatch Logs with Insights queries. Retention policy handles cleanup. |
| Custom AMI for EC2 | User-data script installs Docker + CloudWatch agent on standard Amazon Linux. Build time is 2-3 minutes. A custom AMI saves minutes but adds AMI lifecycle management. | Amazon Linux 2023 with user-data bootstrap. |

---

## Feature Dependencies

```
CDK Infrastructure Stack
  |
  +-> VPC + Security Groups (ingress: 9001 for Prometheus scrape/health, SSH; egress: all)
  +-> EC2 instance (Amazon Linux 2023, t3.medium, user-data: Docker + CW agent + docker-compose)
  +-> IAM Role + Instance Profile
  |     +-> ecr:GetAuthorizationToken, ecr:BatchGetImage, ecr:GetDownloadUrlForLayer
  |     +-> logs:CreateLogStream, logs:PutLogEvents
  |     +-> secretsmanager:GetSecretValue (specific secret ARNs)
  |     +-> aps:RemoteWrite (AMP workspace)
  +-> ECR Repository (import existing 606103597377)
  +-> CloudWatch Log Group (/prediction/production, 30-day retention)
  +-> Secrets Manager Secrets (KALSHI_API_KEY_ID, KALSHI_PRIVATE_KEY, DERIBIT_API_KEY)
  +-> Amazon Managed Prometheus Workspace
  +-> Amazon Managed Grafana Workspace (SSO or API key auth)

GitLab CI Pipeline (depends on: CDK deployed)
  |
  +-> Stage 1: cargo test (Rust CI image)
  +-> Stage 2: docker build + ECR push (docker:dind or BuildKit)
  +-> Stage 3: EC2 deploy via SSH/SSM (stop, pull, start, health check)
  +-> Variables: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION, EC2_HOST, ECR_REPO

CloudWatch Log Aggregation (depends on: CDK IAM role)
  |
  +-> docker-compose.yml logging driver: awslogs
  +-> awslogs-group, awslogs-region, awslogs-stream-prefix options
  +-> EC2 IAM role permits logs:CreateLogStream + logs:PutLogEvents

Secrets Manager Integration (depends on: CDK secrets + IAM)
  |
  +-> Entrypoint wrapper script on EC2 (not in container)
  +-> Fetches each secret via: aws secretsmanager get-secret-value
  +-> Exports as environment variables
  +-> Runs docker-compose up with --env-file or -e flags
  +-> Existing credentials.rs reads from env vars unchanged

Prometheus -> AMP -> Grafana Pipeline (depends on: CDK AMP + Grafana workspaces)
  |
  +-> ADOT collector sidecar in docker-compose
  |     +-> Scrapes localhost:9001/metrics
  |     +-> Remote-writes to AMP endpoint with SigV4 auth
  +-> Grafana workspace with AMP data source (auto-configured via CDK)
  +-> Dashboard JSON files provisioned via Grafana API or CDK

Grafana Dashboards (depends on: AMP receiving metrics, Grafana workspace)
  |
  +-> Dashboard 1: Feed Health (feed_available, reconnections, latency)
  +-> Dashboard 2: Signal Quality (signals, edge, confidence, staleness)
  +-> Dashboard 3: Spread Distributions (spread_net, rolling stats)
  +-> Dashboard 4: Paper Trade P&L (daily_pnl, win_rate, positions)
  +-> Dashboard 5: System Health (pricing, subscriptions, lifecycle, alerts)
  +-> Alert rules on critical panels (feed down, zero computations)
```

---

## MVP Recommendation

### Phase 1: Infrastructure Foundation (CDK + Secrets + Logs)
Prioritize:
1. **CDK stack** -- VPC, SG, EC2, IAM, CloudWatch log group, Secrets Manager secrets
2. **CloudWatch log aggregation** -- awslogs driver in docker-compose (trivial once IAM exists)
3. **Secrets Manager integration** -- entrypoint wrapper script
4. **EC2 user-data** -- installs Docker, CloudWatch agent, pulls docker-compose, starts systemd service

Rationale: Everything else depends on infrastructure being codified. This phase eliminates all manual console-created resources and makes the deployment reproducible.

### Phase 2: CI/CD Pipeline
Prioritize:
1. **GitLab CI .gitlab-ci.yml** -- cargo test, docker build, ECR push, SSH deploy stages
2. **Deploy script on EC2** -- pulls new image, restarts container, verifies health
3. **Production docker-compose** -- awslogs driver, secrets env injection

Rationale: Automated deployment is the highest-value operational improvement. Manual SSH deploy is the current bottleneck and the most error-prone step.

### Phase 3: Monitoring and Dashboards
Prioritize:
1. **AMP workspace** in CDK + ADOT collector sidecar in docker-compose
2. **Amazon Managed Grafana** workspace with AMP data source
3. **Five dashboards** (feed health, signal quality, spreads, paper trade P&L, system health)
4. **Alert rules** for critical conditions (feed down, zero computations, negative P&L)
5. **CloudWatch Logs Insights saved queries** for common investigation patterns

Rationale: Dashboards are high-value but non-blocking. System operates fine with log-based monitoring today. This phase enables "operate without SSH."

### Defer to post-v1.6:
- Container image scanning (Trivy in CI, non-blocking)
- CloudWatch anomaly detection
- Cost optimization (spot instances, reserved instances)
- Automated DR/backup for state files

---

## Metrics Inventory for Grafana Dashboards

The system already emits 80+ Prometheus metrics. Zero instrumentation work is needed -- only visualization.

### Dashboard 1: Feed Health
| Metric | Type | Labels | Panel Type |
|--------|------|--------|------------|
| `feed_available` | gauge | venue | Stat (green/red per venue) |
| `feed_reconnections_total` | counter | venue | Time series (rate) |
| `feed_latency_ms` | histogram | venue | Heatmap |
| `feed_last_latency_ms` | gauge | venue | Time series |
| `feed_messages_total` | counter | venue | Time series (rate, throughput) |

### Dashboard 2: Signal Quality
| Metric | Type | Labels | Panel Type |
|--------|------|--------|------------|
| `arb_signals_emitted_total` | counter | -- | Time series (rate) |
| `arb_signals_filtered_total` | counter | -- | Time series (rate) |
| `arb_signal_net_edge_bps` | histogram | -- | Histogram panel |
| `arb_signal_confidence` | histogram | -- | Histogram panel |
| `arb_staleness_rejections` | counter | -- | Time series (rate) |
| `arb_computations_total` | counter | -- | Stat (total throughput) |
| `arb_events_tracked` | gauge | -- | Stat (current count) |

### Dashboard 3: Spread Distributions
| Metric | Type | Labels | Panel Type |
|--------|------|--------|------------|
| `spread_net` | histogram | event, pattern | Histogram (per-event variable) |
| `spread_rolling_mean` | gauge | event | Time series (per-event) |
| `spread_rolling_stddev` | gauge | event | Time series (per-event) |
| `spread_computations_total` | counter | event | Time series (rate) |
| `spread_staleness_rejections` | counter | event, venue | Time series |
| `spread_signals_total` | counter | event, pattern | Time series (rate) |

### Dashboard 4: Paper Trade P&L
| Metric | Type | Labels | Panel Type |
|--------|------|--------|------------|
| `paper_trade_daily_pnl` | gauge | -- | Time series (the bottom line) |
| `paper_trade_daily_trades` | gauge | -- | Stat |
| `paper_trade_daily_win_rate` | gauge | -- | Gauge (0-100%) |
| `paper_trades_open` | gauge | -- | Stat |
| `paper_trade_net_pnl` | histogram | -- | Histogram (P&L distribution) |
| `paper_trade_settlement_latency_seconds` | histogram | -- | Histogram |
| `paper_trades_settled_total` | counter | event | Time series |
| `signal_analysis_daily_settled` | gauge | -- | Stat |
| `signal_analysis_daily_net_hit_rate` | gauge | -- | Gauge |

### Dashboard 5: System Health
| Metric | Type | Labels | Panel Type |
|--------|------|--------|------------|
| `pricing_active_expiries` | gauge | -- | Stat |
| `pricing_iv_solves_total` | counter | -- | Time series (rate) |
| `pricing_confidence` | histogram | -- | Histogram |
| `subscription_active` | gauge | venue | Stat (per venue) |
| `subscription_activations_total` | counter | venue | Time series |
| `lifecycle_discovery_polls` | counter | venue | Time series (rate) |
| `proposals_pending` | gauge | -- | Stat |
| `proposals_total` | counter | -- | Time series |
| `alert_active` | gauge | type | Stat (red if > 0) |
| `alert_monitor_active_alerts` | gauge | -- | Stat |
| `persistence_checkpoints_written` | counter | -- | Time series |
| `lifecycle_events_archived` | counter | -- | Time series |

---

## CloudWatch Logs Insights Query Templates

Since the system outputs structured JSON via tracing, all fields are auto-discovered. Example queries:

```
# Error rate over time
filter @message like /ERROR/
| stats count(*) as errors by bin(5m)

# Venue-specific feed issues
filter target like /feed/ and level = "WARN"
| fields @timestamp, message, venue
| sort @timestamp desc

# Signal events
filter target = "prediction::signal::engine" and message like /Signal/
| fields @timestamp, event_id, net_edge, confidence

# Settlement outcomes
filter target like /settlement/ and message like /settled/
| fields @timestamp, event_id, settlement_price, outcome

# Staleness warnings
filter message like /stale/
| stats count(*) as stale_count by venue, bin(15m)

# Discovery activity
filter target like /lifecycle/ and message like /proposal/
| fields @timestamp, instrument, venue, confidence
```

---

## Sources

- [Docker awslogs driver](https://docs.docker.com/engine/logging/drivers/awslogs/) -- Docker's built-in CloudWatch integration
- [CloudWatch Logs Insights field discovery](https://docs.aws.amazon.com/AmazonCloudWatch/latest/logs/CWL_AnalyzeLogData-discoverable-fields.html) -- JSON auto-discovery, up to 200 fields
- [Amazon Managed Grafana + AMP data source](https://docs.aws.amazon.com/grafana/latest/userguide/prometheus-data-source.html) -- native integration
- [AMP for EC2 monitoring](https://aws.amazon.com/blogs/opensource/using-amazon-managed-service-for-prometheus-to-monitor-ec2-environments/) -- remote_write from EC2
- [AWS CDK single-ec2 sample](https://github.com/aws-samples/single-ec2-cdk) -- reference pattern for single EC2 CDK deployment
- [GitLab CI AWS deployment](https://docs.gitlab.com/ci/cloud_deployment/) -- official GitLab AWS deployment docs
- [GitLab CI BuildKit ECR pipeline](https://aronschueler.de/blog/2025/09/15/gitlab-ci-buildkit-ecr-pipeline/) -- modern BuildKit approach for CI
- [AWS Secrets Manager best practices](https://docs.aws.amazon.com/AmazonECS/latest/bestpracticesguide/security-secrets-management.html) -- injection patterns
- [Grafana Prometheus getting started](https://grafana.com/docs/grafana/latest/getting-started/get-started-grafana-prometheus/) -- dashboard setup reference
- [Setting up Grafana on EC2 for AMP](https://aws.amazon.com/blogs/opensource/setting-up-grafana-on-ec2-to-query-metrics-from-amazon-managed-service-for-prometheus/) -- SigV4 auth configuration

---
*Feature research for: v1.6 Production Deployment*
*Researched: 2026-03-07*
