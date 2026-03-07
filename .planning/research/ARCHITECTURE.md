# Architecture Patterns: v1.6 Production Deployment

**Domain:** Production deployment infrastructure for Rust arbitrage system
**Researched:** 2026-03-07
**Confidence:** HIGH (well-trodden AWS patterns, official documentation verified)

## Existing Architecture (Baseline)

```
Developer Machine                    EC2 Instance (us-east-1)
+------------------+                 +----------------------------------+
| Rust source      |  manual build   | /opt/prediction/                 |
| Dockerfile       | ─────────────>  |   docker-compose.yml             |
| deploy/ecr-push  |  ECR push       |   config/*.toml (bind mount)     |
|                  |  SSH deploy      |   secrets/ (bind mount, manual)  |
+------------------+                 |                                  |
                                     |   prediction container           |
                                     |   ├─ :9000 Prometheus metrics    |
                                     |   ├─ :9001 /health (axum)        |
                                     |   ├─ stdout → json-file driver   |
                                     |   └─ bind mounts for logs/state  |
                                     +----------------------------------+
```

**Current pain points:**
- Manual `docker build` + `ecr-push.sh` + SSH deploy cycle
- No CI/CD -- broken builds discovered at deploy time
- Prometheus metrics only available on EC2 localhost:9000 (no remote dashboards)
- Logs accessible only via SSH + `docker compose logs`
- Secrets are files in a `secrets/` bind mount, manually placed via SCP
- Infrastructure is click-ops (console-created EC2, security groups, ECR)
- No alerting beyond the internal Prometheus alert rules

## Recommended Architecture (Target)

```
GitLab (CI/CD)                           AWS (us-east-1)
+--------------------+                   +------------------------------------------+
| .gitlab-ci.yml     |                   |                                          |
|                    |                   | CDK-managed infrastructure:              |
| Stages:            |                   |                                          |
|  1. test           |                   | VPC ─── public subnet ─── IGW           |
|  2. build          |                   |   │                                      |
|  3. push-ecr       | ──── push ────>  |   ECR repo: prediction                   |
|  4. deploy         | ──── SSM ─────>  |   │                                      |
|                    |                   |   EC2 (t3.small, AL2023)                 |
+--------------------+                   |   ├─ IAM instance profile                |
                                         |   │  ├─ ECR pull                         |
infra/cdk/           (CDK deploy from    |   │  ├─ Secrets Manager read             |
├─ bin/app.ts        local or CI)        |   │  ├─ CloudWatch Logs write            |
├─ lib/              ──── synth ─────>   |   │  └─ AMP remote write                 |
│  ├─ network.ts                         |   │                                      |
│  ├─ compute.ts                         |   ├─ Docker + Compose (user-data)        |
│  ├─ secrets.ts                         |   ├─ prediction container                |
│  ├─ monitoring.ts                      |   │  ├─ :9000 Prometheus /metrics        |
│  └─ logging.ts                         |   │  ├─ :9001 /health                    |
└─ cdk.json                              |   │  ├─ awslogs driver → CloudWatch      |
                                         |   │  └─ env vars from Secrets Manager    |
                                         |   │                                      |
                                         |   └─ prometheus container (sidecar)      |
                                         |      ├─ scrapes localhost:9000            |
                                         |      └─ remote_write → AMP workspace     |
                                         |                                          |
                                         | Amazon Managed Prometheus (AMP)          |
                                         |   └─ workspace for metric storage        |
                                         |                                          |
                                         | Amazon Managed Grafana (AMG)             |
                                         |   ├─ AMP as data source (SigV4)          |
                                         |   ├─ dashboards (provisioned)            |
                                         |   └─ alert rules                         |
                                         |                                          |
                                         | CloudWatch Logs                          |
                                         |   └─ /prediction/production log group    |
                                         |                                          |
                                         | Secrets Manager                          |
                                         |   └─ prediction/prod/credentials         |
                                         |      (DERIBIT_API_KEY, etc.)             |
                                         +------------------------------------------+
```

## Component Boundaries

| Component | Responsibility | New vs Modified | Communicates With |
|-----------|---------------|-----------------|-------------------|
| `infra/cdk/` | IaC for all AWS resources | **NEW** | AWS CloudFormation |
| `.gitlab-ci.yml` | CI/CD pipeline definition | **NEW** | GitLab runners, ECR, EC2 |
| `Dockerfile` | Multi-stage Rust build | **MODIFIED** (minor: optimize caching) | GitLab CI build stage |
| `docker-compose.yml` | Container orchestration on EC2 | **MODIFIED** (awslogs driver, env_file, prometheus sidecar) | Docker daemon on EC2 |
| `deploy/ecr-push.sh` | Manual ECR push | **DEPRECATED** (replaced by CI) | -- |
| `deploy/aws-setup.sh` | Manual EC2 bootstrap | **DEPRECATED** (replaced by CDK user-data) | -- |
| Prometheus sidecar | Scrape + remote_write to AMP | **NEW** (Docker Compose service) | prediction container, AMP |
| `deploy/fetch-secrets.sh` | Pull secrets from SM on boot | **NEW** | Secrets Manager API |
| `deploy/prometheus.yml` | Prometheus scrape + remote_write config | **NEW** | Prometheus sidecar |

**Zero Rust code changes.** The application binary is untouched. All changes are infrastructure, configuration, and deployment tooling.

---

## Integration Point 1: CDK Project Structure

### Decision: `infra/cdk/` subdirectory in the monorepo

**Why not a separate repo:** Single developer, single deployment target. Co-located IaC means infrastructure changes can be reviewed alongside code changes. Separate repos add overhead with zero benefit at this scale.

**Why not root-level CDK:** The Rust project owns the root. Putting a `package.json` at root would create toolchain confusion. Isolating CDK in `infra/cdk/` keeps Node/TypeScript tooling contained.

```
prediction/                    # Rust project root
├── Cargo.toml
├── src/
├── infra/
│   └── cdk/
│       ├── package.json       # CDK dependencies (isolated)
│       ├── tsconfig.json
│       ├── cdk.json
│       ├── bin/
│       │   └── app.ts         # CDK app entry point
│       └── lib/
│           ├── network-stack.ts    # VPC, subnets, security groups
│           ├── compute-stack.ts    # EC2, instance profile, user-data
│           ├── secrets-stack.ts    # Secrets Manager secret shells
│           ├── monitoring-stack.ts # AMP workspace, AMG workspace
│           └── logging-stack.ts    # CloudWatch log group
├── .gitlab-ci.yml
├── docker-compose.yml
├── deploy/
│   ├── fetch-secrets.sh       # NEW: pull secrets on EC2 boot
│   └── prometheus.yml         # NEW: Prometheus remote_write config
└── Dockerfile
```

### CDK Stack Decomposition

Use multiple small stacks rather than one monolith. This allows targeted updates (e.g., change a security group without touching secrets).

| Stack | Resources | Depends On |
|-------|-----------|------------|
| `NetworkStack` | VPC, public subnet, internet gateway, security groups (SSH from operator IP, 9001 health from VPC, egress all) | -- |
| `SecretsStack` | Secrets Manager secret (empty shell -- values set manually via console/CLI) | -- |
| `MonitoringStack` | AMP workspace, AMG workspace, AMG data source for AMP | -- |
| `LoggingStack` | CloudWatch log group `/prediction/production` with 30-day retention | -- |
| `ComputeStack` | EC2 instance (t3.small), instance profile, IAM role, user-data script, EIP | NetworkStack, SecretsStack, MonitoringStack, LoggingStack |

**Confidence:** HIGH -- standard CDK patterns verified from AWS CDK docs and aws-samples.

---

## Integration Point 2: GitLab CI Pipeline

### Decision: 4-stage pipeline with Docker-in-Docker

```yaml
# .gitlab-ci.yml conceptual structure
stages:
  - test
  - build
  - push
  - deploy

variables:
  DOCKER_HOST: tcp://docker:2376
  DOCKER_TLS_CERTDIR: "/certs"
  ECR_REGISTRY: "606103597377.dkr.ecr.us-east-1.amazonaws.com"
  IMAGE_NAME: prediction
```

### Stage Details

**Stage 1: `test`**
- Image: `rust:1.85` (matches rust-version in Cargo.toml)
- Runs: `cargo test --release`
- Caching: `target/` directory cached by `Cargo.lock` hash
- Duration: ~5-8 min with warm cache, ~15 min cold
- No DinD needed -- pure Rust compilation and testing

**Stage 2: `build`**
- Image: `docker:27` with `docker:27-dind` service
- Runs: `docker build -t $IMAGE_NAME:$CI_COMMIT_SHA .`
- The Dockerfile handles Rust compilation internally (multi-stage build)
- Duration: ~8-12 min (Rust compile inside Docker)
- Docker layer caching via `--cache-from` if ECR pull-through cache is configured

**Stage 3: `push`**
- Same DinD service as build stage (or combined with build)
- Authenticates to ECR: `aws ecr get-login-password | docker login`
- Tags: `$CI_COMMIT_SHA` and `latest`
- Pushes to ECR
- Requires: `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` as GitLab CI/CD variables (these are CI-only credentials for ECR push, not runtime secrets)

**Stage 4: `deploy`**
- Image: lightweight with AWS CLI
- Uses SSM Run Command (preferred over SSH -- no key management):
  ```bash
  aws ssm send-command \
    --instance-ids $EC2_INSTANCE_ID \
    --document-name "AWS-RunShellScript" \
    --parameters commands=["cd /opt/prediction && ./deploy/fetch-secrets.sh && docker compose pull && docker compose up -d"]
  ```
- Only on `master` branch
- **Manual trigger** (`when: manual`) for safety -- deploy is deliberate, not automatic
- Alternative: SSH deploy via `appleboy/ssh-action` if SSM is not yet configured

### Runner Requirements

- GitLab shared runners or self-hosted runner with Docker executor
- DinD requires privileged mode (`privileged: true` in runner config)
- For self-hosted: minimum 4GB RAM, 2 vCPU for Rust compilation inside Docker
- If using GitLab.com shared runners: the `docker:27-dind` service works out of the box

### Cross-Compilation Consideration

The question mentions `x86_64-unknown-linux-musl`. **Recommendation: skip musl cross-compile for v1.6.** Rationale:
- The current Dockerfile uses `rust:latest` builder + `debian:bookworm-slim` runtime, which works
- Musl would require replacing `native-tls` feature on `tokio-tungstenite` (currently uses OpenSSL via native-tls) -- `reqwest` already uses `rustls-tls` but tungstenite does not
- The benefit (smaller image, static binary) is marginal for a single-target deployment
- If musl is desired later, switch Dockerfile builder to `rust:1.85-alpine`, add `apk add musl-dev`, and change `tokio-tungstenite` feature to `rustls-tls-webpki-roots`

**Confidence:** HIGH -- GitLab CI DinD + ECR push is well-documented pattern.

---

## Integration Point 3: Prometheus Metrics to Amazon Managed Grafana

### Decision: Prometheus sidecar container with native SigV4 remote_write

**Why not direct scrape from AMG:** Amazon Managed Grafana cannot reach into an EC2 instance to scrape. Metrics must be pushed to an intermediate store (AMP).

**Why Prometheus sidecar, not OpenTelemetry Collector:** The application already exposes a standard Prometheus `/metrics` endpoint on :9000 via `metrics-exporter-prometheus`. A lightweight Prometheus instance scrapes locally and remote_writes to AMP. Zero application code changes. OTEL Collector could work but adds unnecessary abstraction for a single-source single-destination setup.

**Why not Grafana Alloy (formerly Grafana Agent):** Prometheus itself is the most documented path for AMP ingestion from EC2, per AWS official docs. Alloy would work but adds a less-documented dependency.

### Architecture

```
prediction container (:9000/metrics)
        │
        │ scrape every 15s
        ▼
prometheus sidecar container
        │
        │ remote_write (SigV4 signed, native Prometheus 2.26+)
        ▼
Amazon Managed Prometheus workspace
        │
        │ PromQL queries (SigV4 via AMG service role)
        ▼
Amazon Managed Grafana dashboards + alerts
```

### Prometheus Sidecar Config (`deploy/prometheus.yml`)

```yaml
global:
  scrape_interval: 15s
  external_labels:
    environment: production
    service: prediction

scrape_configs:
  - job_name: prediction
    static_configs:
      - targets: ['prediction:9000']
    metrics_path: /metrics

remote_write:
  - url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/WORKSPACE_ID/api/v1/remote_write
    queue_config:
      max_samples_per_send: 1000
      max_shards: 200
      capacity: 2500
    sigv4:
      region: us-east-1
```

The `WORKSPACE_ID` is output from CDK's `MonitoringStack` and injected into this config during deployment (sed replacement in user-data, or templated via CDK Asset).

### Docker Compose Addition

```yaml
services:
  prometheus:
    image: prom/prometheus:v2.51.0    # 2.26+ required for native SigV4
    volumes:
      - ./deploy/prometheus.yml:/etc/prometheus/prometheus.yml:ro
    depends_on:
      - prediction
    restart: unless-stopped
```

Prometheus scrapes `prediction:9000` using Docker Compose's built-in DNS resolution.

### IAM Requirements

EC2 instance profile needs `AmazonPrometheusRemoteWriteAccess` managed policy. CDK handles this in `ComputeStack`.

### AMG Data Source Connection

Amazon Managed Grafana connects to the AMP workspace as a Prometheus-type data source. Configure SigV4 auth in the AMG data source settings with the workspace URL (without `/api/v1/query` suffix) and the appropriate region. This is a one-time manual configuration in the AMG console, or automated via AMG workspace API.

**Confidence:** HIGH -- verified against [AWS official docs for EC2 remote_write to AMP with SigV4](https://docs.aws.amazon.com/prometheus/latest/userguide/AMP-onboard-ingest-metrics-remote-write-EC2.html). Native SigV4 support confirmed since Prometheus 2.26.0.

---

## Integration Point 4: CloudWatch Log Aggregation

### Decision: `awslogs` Docker log driver (not Fluent Bit)

**Why awslogs driver:**
- Zero additional containers or configuration -- built into Docker daemon
- The application already writes structured JSON to stdout via `tracing-subscriber` with JSON formatter
- awslogs driver ships stdout/stderr directly to CloudWatch Logs
- CloudWatch Logs Insights can query JSON fields natively (e.g., `filter level = "ERROR"`)
- Single container, single destination -- Fluent Bit's routing/filtering power is wasted here

**Why NOT Fluent Bit sidecar:**
- Adds a container to manage and monitor
- Fluent Bit shines when you need multi-destination routing, log transformation, or buffering at scale
- None of those apply -- one container, one destination
- Would need its own health monitoring (ironic overhead)

### Docker Compose Modification

Replace the existing `json-file` logging block:

```yaml
services:
  prediction:
    image: 606103597377.dkr.ecr.us-east-1.amazonaws.com/prediction:latest
    logging:
      driver: awslogs
      options:
        awslogs-region: us-east-1
        awslogs-group: /prediction/production
        awslogs-stream-prefix: prediction
        awslogs-create-group: "true"
    # ... rest unchanged
```

### IAM Requirements

EC2 instance profile needs CloudWatch Logs permissions: `logs:CreateLogStream`, `logs:PutLogEvents`, `logs:CreateLogGroup`. CDK adds an inline policy or the managed `CloudWatchLogsFullAccess` (scoped down to the specific log group ARN).

### Impact on Existing Logging

The bind mount `./logs:/app/logs` remains for `tracing-appender` file-based debug logs. CloudWatch captures stdout (info level structured JSON), while the file appender captures debug-level logs locally. These are complementary:
- CloudWatch: remote access, queryable, alertable, retained 30 days
- Local files: debug-level detail, available via SSH for deep troubleshooting

### CloudWatch Logs Insights Query Examples

```sql
-- Find errors in the last hour
fields @timestamp, @message
| filter level = "ERROR"
| sort @timestamp desc
| limit 50

-- Signal generation events
fields @timestamp, event_id, spread_bps, signal_type
| filter target = "prediction::signals"
| sort @timestamp desc

-- Feed disconnection events
fields @timestamp, venue, message
| filter message like /reconnect/
| sort @timestamp desc
```

**Confidence:** HIGH -- [Docker awslogs driver](https://docs.docker.com/engine/logging/drivers/awslogs/) is the canonical Docker-to-CloudWatch integration.

---

## Integration Point 5: Secrets Manager Injection

### Decision: Boot-time fetch script writing `.env` file for Docker Compose

**Why not ECS native secrets integration:** We are on raw EC2 with Docker Compose, not ECS. ECS has built-in `secrets` in task definitions that resolve Secrets Manager ARNs. Docker Compose on EC2 has no such mechanism.

**Why not application-level AWS SDK calls:** The application reads credentials from environment variables (`std::env::var("DERIBIT_API_KEY")` etc. in `src/config/credentials.rs`). Changing to SDK calls would require adding `aws-sdk-secretsmanager` crate, async initialization before config load, and handling SDK errors. This is invasive for zero functional benefit.

**Why boot-time `.env` fetch:** A shell script runs before `docker compose up`, pulls the JSON secret from Secrets Manager via AWS CLI, writes key=value pairs to a `.env` file, and Docker Compose injects those as environment variables via `env_file`. This preserves the existing `std::env::var` loading pattern with zero Rust changes.

### Secret Structure in AWS Secrets Manager

One JSON secret: `prediction/prod/credentials`

```json
{
  "DERIBIT_API_KEY": "...",
  "DERIBIT_API_SECRET": "...",
  "POLYMARKET_PRIVATE_KEY": "...",
  "KALSHI_API_KEY_ID": "...",
  "KALSHI_PRIVATE_KEY": "-----BEGIN RSA PRIVATE KEY-----\n..."
}
```

CDK creates the secret shell (empty JSON). Values are populated manually via AWS Console or CLI after stack deployment. This is intentional -- secret values never appear in IaC code or version control.

### Fetch Script (`deploy/fetch-secrets.sh`)

```bash
#!/usr/bin/env bash
set -euo pipefail

SECRET_ID="prediction/prod/credentials"
REGION="us-east-1"
ENV_FILE="/opt/prediction/.env"

# Fetch secret JSON and convert to KEY=VALUE format
aws secretsmanager get-secret-value \
  --secret-id "$SECRET_ID" \
  --region "$REGION" \
  --query 'SecretString' \
  --output text \
  | python3 -c "
import sys, json
secret = json.load(sys.stdin)
for k, v in secret.items():
    print(f'{k}={v}')
" > "$ENV_FILE"

chmod 600 "$ENV_FILE"
echo "Secrets written to $ENV_FILE ($(wc -l < "$ENV_FILE") keys)"
```

### Docker Compose Modification

```yaml
services:
  prediction:
    env_file:
      - .env    # Contains secrets fetched from Secrets Manager
    environment:
      - RUST_LOG=info
    # Remove the old: volumes: ./secrets:/app/secrets:ro
```

### Credential Flow (Before vs After)

**Before:** Developer SCPs files to `secrets/` directory on EC2. Application reads from bind-mounted files or env vars set manually.

**After:** `fetch-secrets.sh` runs on boot (via user-data or systemd). Pulls from Secrets Manager. Writes `.env`. Docker Compose reads `.env` into container environment. Application reads via `std::env::var` -- unchanged.

### Secret Rotation

For v1.6, rotation is manual: update secret in Secrets Manager, SSH to EC2, re-run `fetch-secrets.sh`, `docker compose restart`. Automated rotation can be added later via Lambda rotation function + SSM Run Command to trigger restart.

### IAM Requirements

EC2 instance profile needs `secretsmanager:GetSecretValue` scoped to `arn:aws:secretsmanager:us-east-1:ACCOUNT:secret:prediction/prod/*`.

**Confidence:** HIGH -- standard EC2 + Secrets Manager pattern per [AWS re:Post guidance](https://repost.aws/questions/QUNHr37DAhQxqTUQgeUUFPEA/ec2-and-secret-manager).

---

## Integration Point 6: EC2 User-Data (CDK-managed Bootstrap)

### Decision: CDK user-data script replaces manual `aws-setup.sh`

The existing `deploy/aws-setup.sh` installs Docker and creates directories. CDK's `ec2.Instance` construct supports `userData` scripts that run on first boot via cloud-init. This becomes the single source of truth for EC2 bootstrapping.

### User-Data Responsibilities

1. Install Docker and Docker Compose plugin (Amazon Linux 2023)
2. Authenticate to ECR (`aws ecr get-login-password`)
3. Create `/opt/prediction/` directory structure with bind mount dirs
4. Download `docker-compose.yml` and `deploy/prometheus.yml` from S3 (CDK Asset)
5. Run `fetch-secrets.sh` to populate `.env`
6. `docker compose pull && docker compose up -d`

### How Config Gets to EC2

**CDK `Asset`** uploads docker-compose.yml, prometheus.yml, and fetch-secrets.sh to an S3 bucket. User-data downloads them to `/opt/prediction/`. This ensures the deployed config is versioned and matches the CDK deployment.

Config TOML files (`config.toml`, `venues.toml`, `events.toml`) continue as bind mounts, managed separately -- they change at runtime (events.toml is written by the discovery engine) and should not be baked into deployments.

**Confidence:** HIGH -- CDK user-data with Asset is a standard pattern.

---

## Data Flow Changes Summary

### Before (v1.5)

```
prediction binary
  ├── stdout → Docker json-file driver → local disk only
  ├── :9000 /metrics → accessible only from EC2 localhost
  ├── :9001 /health → Docker healthcheck only
  ├── config/*.toml ← bind mount (manually SCP'd)
  └── env vars ← manually set or from secrets/ bind mount
```

### After (v1.6)

```
prediction binary
  ├── stdout → awslogs driver → CloudWatch Logs → Logs Insights queries
  ├── :9000 /metrics → Prometheus sidecar → remote_write → AMP → AMG dashboards
  ├── :9001 /health → Docker healthcheck + Grafana alert on absence
  ├── config/*.toml ← bind mount (deployed via CDK Asset initial, updated in-place)
  └── env vars ← .env file ← fetch-secrets.sh ← Secrets Manager
```

### What Does NOT Change

- **Application binary code** -- zero Rust changes in this milestone
- **Config file format and loading** -- TOML config unchanged
- **Prometheus metrics endpoint** -- still :9000/metrics, same metric names
- **Health endpoint** -- still :9001/health, same response format
- **Structured JSON log format** -- same tracing-subscriber JSON output
- **Environment variable credential loading** -- `std::env::var` calls unchanged
- **Bind mounts** for spread_logs, settlement_logs, paper_trades, state -- local persistence continues
- **events.toml** -- still written by discovery engine, still bind-mounted

---

## Suggested Build Order

The build order follows dependency chains and validates incrementally. Each phase produces a testable, observable result.

```
Phase 1: CDK Foundation
    │     NetworkStack + SecretsStack + LoggingStack (no compute yet)
    │     Validates: VPC, SGs, secret shell, log group exist in AWS
    │
Phase 2: ComputeStack + Secrets Integration
    │     EC2 instance with IAM role, user-data, fetch-secrets.sh
    │     Modify docker-compose.yml for env_file
    │     Test: deploy manually to CDK-created EC2, verify credentials load
    │     Validates: End-to-end secrets flow, EC2 runs application
    │
Phase 3: CloudWatch Logging
    │     Switch docker-compose.yml logging driver to awslogs
    │     Test: deploy, verify logs appear in CloudWatch, run Insights query
    │     Validates: Structured JSON queryable remotely
    │
Phase 4: Prometheus + AMP + AMG (MonitoringStack + sidecar)
    │     Create AMP workspace, add prometheus sidecar to docker-compose.yml
    │     Configure remote_write with SigV4
    │     Create AMG workspace, add AMP as data source
    │     Test: verify metrics visible in AMG, build initial dashboard
    │     Validates: Full metrics pipeline from app to remote dashboard
    │
Phase 5: GitLab CI Pipeline
    │     Write .gitlab-ci.yml with test/build/push/deploy stages
    │     Configure GitLab CI/CD variables (AWS creds for ECR push)
    │     Test: push commit, verify automated build/test/push cycle
    │     Test: manual deploy trigger, verify hands-off deployment
    │     Validates: No more manual build/push/SSH workflow
    │
Phase 6: Grafana Dashboards + Alert Rules
          Build production dashboards in AMG (feed health, spread rates, signals, reconnections)
          Configure alert rules (feed silence, health endpoint down, high error rate)
          Test: trigger alert conditions, verify notification
          Validates: Operational monitoring and alerting complete
```

### Build Order Rationale

1. **CDK foundation first** because every subsequent phase needs AWS resources (VPC, IAM roles, log groups) to exist.
2. **Secrets + compute second** because the application cannot run without credentials. This validates the most critical integration (secrets flow) early.
3. **Logging third** because it is a one-line config change (swap `json-file` for `awslogs`) and immediately provides remote observability for debugging later phases.
4. **Monitoring fourth** because the Prometheus sidecar is a new container requiring configuration, and AMP/AMG creation has more moving parts. Having logging already working means you can debug sidecar issues via CloudWatch.
5. **CI fifth** because manual deploys work fine during infrastructure buildout. CI automates an already-working manual process -- it should not be the first thing built when the deployment target is still being configured.
6. **Dashboards last** because they are a consumption layer. Building dashboards before metrics flow through AMP is wasteful -- you need real data to design meaningful visualizations.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: ECS Migration
**What:** Moving from Docker Compose on EC2 to ECS Fargate or ECS on EC2.
**Why bad:** Massive scope increase for zero benefit. ECS adds task definitions, service discovery, ALB, target groups, ECS-specific IAM, and ECS agent management. Docker Compose on a single EC2 instance is simpler, cheaper ($15/month t3.small vs ECS overhead), and sufficient for a solo-trader single-container system.
**Instead:** Keep Docker Compose. Reconsider ECS only if auto-scaling or multi-instance HA becomes a requirement.

### Anti-Pattern 2: Running Self-Hosted Grafana on EC2
**What:** Adding a Grafana container alongside prediction and prometheus in docker-compose.yml.
**Why bad:** Consumes EC2 CPU/RAM (Grafana is not lightweight), requires TLS/auth setup, needs its own persistent storage, and must be updated/patched.
**Instead:** Amazon Managed Grafana. It is managed, integrates natively with AMP via SigV4, supports AWS SSO auth, and has zero infrastructure overhead.

### Anti-Pattern 3: Putting Runtime Secrets in GitLab CI Variables
**What:** Storing `DERIBIT_API_KEY` etc. as GitLab CI/CD variables and passing them through the deploy stage to EC2.
**Why bad:** Secrets transit through CI job logs/environment. Rotation requires updating GitLab variables AND redeploying. Secrets and deployment should be decoupled.
**Instead:** Runtime secrets live exclusively in AWS Secrets Manager. EC2 pulls them at boot. GitLab CI only holds ECR push credentials (which are separate, lower-privilege credentials).

### Anti-Pattern 4: CloudWatch Agent Instead of awslogs Driver
**What:** Installing CloudWatch Unified Agent on EC2 to ship Docker container logs.
**Why bad:** CloudWatch Agent is designed for host-level metrics and file-based log collection. For Docker stdout, the `awslogs` log driver is purpose-built, requires zero installation, and is the Docker-native integration path.
**Instead:** Use the `awslogs` Docker log driver. Reserve CloudWatch Agent for host-level metrics if needed (CPU, disk, memory) -- but AMP/Prometheus already covers application metrics.

### Anti-Pattern 5: Single Monolith CDK Stack
**What:** Putting VPC, EC2, Secrets Manager, AMP, AMG, CloudWatch, and ECR all in one CDK stack.
**Why bad:** Any change to any resource triggers a full stack update. A security group tweak risks modifying the EC2 instance. Stack updates are slow and risky.
**Instead:** Split into focused stacks (Network, Compute, Secrets, Monitoring, Logging). Deploy individually. Cross-stack references via CDK exports.

---

## Scalability Considerations

| Concern | Current (1 instance) | If 2-3 instances needed | If auto-scaling needed |
|---------|---------------------|------------------------|----------------------|
| Deployment | Docker Compose on EC2 | Duplicate ComputeStack with params | Migrate to ECS |
| Secrets | .env file per instance | Same pattern, each pulls own copy | ECS native secrets |
| Logging | awslogs per container | Same, stream-prefix distinguishes | Same |
| Metrics | Prometheus sidecar per instance | Each remote_writes to AMP | Same |
| Config | Bind mount from CDK Asset | Same, S3 download per instance | S3 + config service |

The architecture handles "2-3 instances" with minimal changes. ECS migration is only warranted at auto-scaling requirements, which is out of scope per PROJECT.md.

---

## Sources

- [AMP Remote Write from EC2 (AWS Official Docs)](https://docs.aws.amazon.com/prometheus/latest/userguide/AMP-onboard-ingest-metrics-remote-write-EC2.html) -- HIGH confidence, SigV4 config verified
- [Docker awslogs Driver (Docker Official Docs)](https://docs.docker.com/engine/logging/drivers/awslogs/) -- HIGH confidence, configuration options verified
- [GitLab CI Docker-in-Docker (GitLab Docs)](https://docs.gitlab.com/ee/ci/docker/using_docker_build.html) -- HIGH confidence
- [Prometheus SigV4 Native Support (AWS Blog)](https://aws.amazon.com/blogs/opensource/prometheus-2-26-0-adds-aws-signature-version-4-support/) -- HIGH confidence, confirmed since Prometheus 2.26.0
- [AWS CDK EC2 Instance Construct](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ec2.Instance.html) -- HIGH confidence
- [Secrets Manager on EC2 (AWS re:Post)](https://repost.aws/questions/QUNHr37DAhQxqTUQgeUUFPEA/ec2-and-secret-manager) -- MEDIUM confidence
- [Fluent Bit vs awslogs (AWS Blog)](https://aws.amazon.com/blogs/opensource/centralized-container-logging-fluent-bit/) -- HIGH confidence for comparison rationale
- [GitLab CI ECR Push Pattern (Gist)](https://gist.github.com/tanmay-bhat/6fa65b9cd9d5f7f5e780dbe3efcb1fb7) -- MEDIUM confidence
- [CDK in Existing Project (DEV Community)](https://dev.to/alexvladut/how-to-add-aws-cdk-to-an-existing-project-2d30) -- MEDIUM confidence
- [AMP EC2 Monitoring (AWS Blog)](https://aws.amazon.com/blogs/opensource/using-amazon-managed-service-for-prometheus-to-monitor-ec2-environments/) -- HIGH confidence
- Direct source analysis of `src/config/credentials.rs` -- verified env var loading pattern
- Direct source analysis of `docker-compose.yml` -- verified current logging/volume/health config
- Direct source analysis of `deploy/ecr-push.sh` and `deploy/aws-setup.sh` -- verified current manual workflow

---
*Architecture research for: v1.6 Production Deployment*
*Researched: 2026-03-07*
