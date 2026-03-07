# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit, Derive). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (shipped 2026-02-26) | [Full details](milestones/v1.1-ROADMAP.md)
- v1.2 Automated Event Management -- Phases 18-21 (shipped 2026-02-27) | [Full details](milestones/v1.2-ROADMAP.md)
- v1.3 Live Subscription Management -- Phases 22-25 (shipped 2026-02-28) | [Full details](milestones/v1.3-ROADMAP.md)
- v1.4 Analysis Tooling -- Phases 26-29 (shipped 2026-03-02) | [Full details](milestones/v1.4-ROADMAP.md)
- v1.5 Derive.xyz Venue Integration -- Phases 30-33 (shipped 2026-03-06) | [Full details](milestones/v1.5-ROADMAP.md)
- v1.6 Production Deployment -- Phases 34-39 (in progress)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-13) -- SHIPPED 2026-02-24</summary>

- [x] Phase 1: Foundation (3/3 plans) -- completed 2026-02-22
- [x] Phase 2: Deribit Feed and Data Pipeline (4/4 plans) -- completed 2026-02-22
- [x] Phase 3: Feed Infrastructure (3/3 plans) -- completed 2026-02-22
- [x] Phase 4: Multi-Venue Feeds (3/3 plans) -- completed 2026-02-22
- [x] Phase 5: Event Mapping (3/3 plans) -- completed 2026-02-22
- [x] Phase 6: Prediction Market Spreads (4/4 plans) -- completed 2026-02-23
- [x] Phase 7: Options Pricing Engine (5/5 plans) -- completed 2026-02-23
- [x] Phase 8: Cross-Asset Signal Generation (2/2 plans) -- completed 2026-02-23
- [x] Phase 9: Replay and Hardening (3/3 plans) -- completed 2026-02-23
- [x] Phase 10: Critical Pipeline Wiring (1/1 plan) -- completed 2026-02-24
- [x] Phase 11: BasisRiskScore Downstream Consumption (2/2 plans) -- completed 2026-02-24
- [x] Phase 12: Kalshi Feed Hardening (1/1 plan) -- completed 2026-02-24
- [x] Phase 13: Phase 4 Verification & Cleanup (2/2 plans) -- completed 2026-02-24

</details>

<details>
<summary>v1.1 Paper Trading Validation (Phases 14-17) -- SHIPPED 2026-02-26</summary>

- [x] Phase 14: Failure Alerting (2/2 plans) -- completed 2026-02-24
- [x] Phase 15: State Persistence (2/2 plans) -- completed 2026-02-24
- [x] Phase 16: Settlement Outcome Tracking (4/4 plans) -- completed 2026-02-26
- [x] Phase 17: Signal Analysis Tooling (3/3 plans) -- completed 2026-02-26

</details>

<details>
<summary>v1.2 Automated Event Management (Phases 18-21) -- SHIPPED 2026-02-27</summary>

- [x] Phase 18: Discovery Infrastructure Hardening (2/2 plans) -- completed 2026-02-26
- [x] Phase 19: Polymarket Discovery and Cross-Venue Matching (2/2 plans) -- completed 2026-02-27
- [x] Phase 20: Proposal Workflow and Operator Interface (2/2 plans) -- completed 2026-02-27
- [x] Phase 21: Lifecycle Management and Integration (2/2 plans) -- completed 2026-02-27

</details>

<details>
<summary>v1.3 Live Subscription Management (Phases 22-25) -- SHIPPED 2026-02-28</summary>

- [x] Phase 22: Subscription Manager Core (2/2 plans) -- completed 2026-02-27
- [x] Phase 23: Dynamic Supervisor Subscriptions (1/1 plan) -- completed 2026-02-27
- [x] Phase 24: Hardening and Observability (2/2 plans) -- completed 2026-02-27
- [x] Phase 25: Tech Debt Sweep (2/2 plans) -- completed 2026-02-28

</details>

<details>
<summary>v1.4 Analysis Tooling (Phases 26-29) -- SHIPPED 2026-03-02</summary>

- [x] Phase 26: Analysis Infrastructure (2/2 plans) -- completed 2026-02-28
- [x] Phase 27: Spread Analytics CLI (1/1 plan) -- completed 2026-02-28
- [x] Phase 28: Signal Scoring CLI (2/2 plans) -- completed 2026-02-28
- [x] Phase 29: End-to-End Verification (2/2 plans) -- completed 2026-02-28

</details>

<details>
<summary>v1.5 Derive.xyz Venue Integration (Phases 30-33) -- SHIPPED 2026-03-06</summary>

- [x] Phase 30: Venue Type Foundation (2/2 plans) -- completed 2026-03-04
- [x] Phase 31: Derive Feed and Normalization (4/4 plans) -- completed 2026-03-04
- [x] Phase 32: Pipeline Wiring and Observability (2/2 plans) -- completed 2026-03-05
- [x] Phase 33: Discovery and Matching (2/2 plans) -- completed 2026-03-06

</details>

### v1.6 Production Deployment (Phases 34-39)

**Milestone Goal:** Deploy the system to production-hardened AWS infrastructure with CI/CD, monitoring, log aggregation, and infrastructure as code. Replace manual build-push-SSH workflow with automated pipeline and enable remote observability without SSH access.

- [x] **Phase 34: CDK Infrastructure Foundation** - VPC, security groups, IAM, EBS, log groups, secrets, and ECR import via CDK -- completed 2026-03-07
- [x] **Phase 35: Compute, Secrets, and Hardening** - EC2 instance, user-data bootstrap, secrets injection, systemd service, and SIGTERM graceful shutdown (completed 2026-03-07)
- [ ] **Phase 36: CloudWatch Logging** - Docker awslogs driver and EC2 host metrics for remote log access
- [ ] **Phase 37: Prometheus + AMP + Managed Grafana** - Metrics pipeline from app through Prometheus sidecar to AMP with Grafana workspace
- [ ] **Phase 38: GitLab CI/CD Pipeline** - Automated test, build, push, and deploy with cargo-chef caching
- [ ] **Phase 39: Grafana Dashboards and Alert Rules** - Five operational dashboards and critical alert rules

## Phase Details

### Phase 34: CDK Infrastructure Foundation
**Goal**: All foundational AWS resources exist in version-controlled CDK stacks, reproducible via a single `cdk deploy`
**Depends on**: Nothing (first phase of v1.6)
**Requirements**: INFRA-01, INFRA-02, INFRA-04, INFRA-05, INFRA-06, INFRA-07
**Success Criteria** (what must be TRUE):
  1. Running `cdk deploy` creates VPC, security groups, IAM instance profile, CloudWatch log group, and Secrets Manager secret shells from scratch
  2. CDK imports the existing ECR repository by ARN without creating a duplicate
  3. IAM instance profile grants least-privilege access (ECR pull, CloudWatch Logs, Secrets Manager read, AMP remote write) and nothing else
  4. Separate EBS data volume is provisioned with RemovalPolicy.RETAIN so persistent data survives instance replacement
  5. Running `cdk destroy` and `cdk deploy` again produces identical infrastructure (idempotent)
**Plans:** 2/2 plans complete
Plans:
- [x] 34-01-PLAN.md -- Initialize CDK project and implement PredictionStack with all resources
- [x] 34-02-PLAN.md -- Validate synthesized template and deploy to AWS

### Phase 35: Compute, Secrets, and Hardening
**Goal**: Application runs on CDK-managed EC2 with secrets injected from Secrets Manager, auto-restarts on failure, and shuts down gracefully on SIGTERM
**Depends on**: Phase 34
**Requirements**: INFRA-03, HARD-01, HARD-02, HARD-03
**Success Criteria** (what must be TRUE):
  1. EC2 instance boots via user-data that installs Docker, CloudWatch agent, and configures systemd for docker-compose auto-start
  2. `fetch-secrets.sh` retrieves venue API credentials from Secrets Manager and exports them as environment variables before container start
  3. Sending SIGTERM to the container flushes checkpoints, closes WebSocket connections, and exits with code 0 (not killed by SIGKILL after timeout)
  4. After EC2 reboot, the systemd service automatically starts docker-compose and the application resumes operation without manual intervention
**Plans:** 2/2 plans complete
Plans:
- [ ] 35-01-PLAN.md -- CDK user-data bootstrap, secrets template fix, and production docker-compose
- [ ] 35-02-PLAN.md -- Deploy to AWS and verify full bootstrap chain with graceful shutdown

### Phase 36: CloudWatch Logging
**Goal**: Container logs and host metrics are remotely accessible in CloudWatch without SSH
**Depends on**: Phase 35
**Requirements**: MON-01, MON-09
**Success Criteria** (what must be TRUE):
  1. Structured JSON log lines from the prediction container appear in the CloudWatch log group within seconds of emission
  2. CloudWatch Logs Insights queries can filter by structured fields (level, target, correlation_id) across container logs
  3. EC2 host metrics (CPU utilization, memory usage, disk usage) appear in CloudWatch Metrics via the CloudWatch agent
**Plans**: TBD

### Phase 37: Prometheus + AMP + Managed Grafana
**Goal**: All 80+ Prometheus metrics flow from the application through a sidecar to Amazon Managed Prometheus with Grafana connected as visualization layer
**Depends on**: Phase 35
**Requirements**: MON-02, MON-03
**Success Criteria** (what must be TRUE):
  1. Prometheus sidecar container scrapes the application's :9001/metrics endpoint and remote_writes to AMP with SigV4 authentication
  2. Amazon Managed Grafana workspace connects to AMP as a data source and can query all application metrics (feed_available, arb_signals_emitted, etc.)
  3. Metrics persist in AMP beyond EC2 instance lifecycle -- redeploying the instance does not lose historical metrics
**Plans**: TBD

### Phase 38: GitLab CI/CD Pipeline
**Goal**: Every push to master automatically tests, builds a Docker image, pushes to ECR, and deploys to EC2 -- replacing the manual build-push-SSH workflow
**Depends on**: Phase 35
**Requirements**: CICD-01, CICD-02, CICD-03, CICD-04, CICD-05
**Success Criteria** (what must be TRUE):
  1. Pushing to master triggers a pipeline that runs `cargo test` and blocks deployment on failure
  2. On successful tests, the pipeline builds a Docker image using cargo-chef layer caching and pushes it to ECR in under 10 minutes
  3. The deploy stage uses SSM Send-Command to stop the old container, pull the new image, and start the new container on EC2 (no SSH keys in CI)
  4. After deployment, the pipeline verifies the /health endpoint responds 200 before marking the deploy stage as successful
  5. The operator never needs to SSH into the instance or manually run docker commands to deploy
**Plans**: TBD

### Phase 39: Grafana Dashboards and Alert Rules
**Goal**: Five operational dashboards and critical alert rules enable monitoring and operating the system entirely through Grafana
**Depends on**: Phase 37
**Requirements**: MON-04, MON-05, MON-06, MON-07, MON-08
**Success Criteria** (what must be TRUE):
  1. Feed Health dashboard shows per-venue feed availability, reconnection rate, and message latency with data updating in real time
  2. Signal Quality dashboard shows arb signals emitted, net edge in basis points, confidence scores, and staleness rejection counts
  3. Paper Trade P&L dashboard shows daily P&L, cumulative net P&L, win rate, and settlement latency
  4. System Health dashboard shows active expiries, subscription counts, lifecycle poll activity, proposal counts, and alert state
  5. Alert rules fire notifications when any feed is down for 5 minutes, zero spread computations occur for 30 minutes, or staleness rejection rate exceeds threshold
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 34 -> 35 -> 36 -> 37 -> 38 -> 39
(Phases 36, 37, and 38 can proceed in parallel after Phase 35, but 39 depends on 37)

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1-13 | v1.0 MVP | 36/36 | Complete | 2026-02-24 |
| 14-17 | v1.1 Paper Trading | 11/11 | Complete | 2026-02-26 |
| 18-21 | v1.2 Automated Event Mgmt | 8/8 | Complete | 2026-02-27 |
| 22-25 | v1.3 Subscription Mgmt | 7/7 | Complete | 2026-02-28 |
| 26-29 | v1.4 Analysis Tooling | 7/7 | Complete | 2026-03-02 |
| 30-33 | v1.5 Derive Integration | 10/10 | Complete | 2026-03-06 |
| 34. CDK Infrastructure Foundation | v1.6 | Complete    | 2026-03-07 | 2026-03-07 |
| 35. Compute, Secrets, Hardening | 2/2 | Complete   | 2026-03-07 | - |
| 36. CloudWatch Logging | v1.6 | 0/TBD | Not started | - |
| 37. Prometheus + AMP + Grafana | v1.6 | 0/TBD | Not started | - |
| 38. GitLab CI/CD Pipeline | v1.6 | 0/TBD | Not started | - |
| 39. Grafana Dashboards + Alerts | v1.6 | 0/TBD | Not started | - |
