# Project Research Summary

**Project:** v1.6 Production Deployment
**Domain:** AWS infrastructure-as-code, CI/CD, and observability for a Rust single-binary crypto arbitrage service
**Researched:** 2026-03-07
**Confidence:** HIGH

## Executive Summary

v1.6 wraps a fully operational 39,176 LOC Rust arbitrage system (v1.0-v1.5 complete) in production-grade AWS infrastructure. The milestone requires zero Rust code changes -- every addition is external to the application binary: AWS CDK (TypeScript) for infrastructure provisioning, GitLab CI for automated build/test/deploy, Prometheus remote_write to Amazon Managed Prometheus for durable metrics, CloudWatch for centralized logging, and AWS Secrets Manager for credential injection. The system already has a multi-stage Dockerfile, Docker Compose orchestration, 80+ Prometheus metrics, structured JSON logging, environment-variable credential loading, and health endpoints. v1.6 codifies the manually-created infrastructure, automates the manual build-push-SSH cycle, and enables remote observability without SSH access.

The recommended approach uses AWS CDK with TypeScript for all infrastructure (VPC, EC2, IAM, Secrets Manager, AMP, AMG), GitLab CI with Docker-in-Docker for the pipeline, the Docker `awslogs` driver for log aggregation (zero agent installation), a Prometheus sidecar container for metrics remote_write to AMP with native SigV4, and SSM Run Command for deployments (no SSH keys in CI). The CDK project lives in `infra/cdk/` within the monorepo, decomposed into five focused stacks (Network, Secrets, Logging, Monitoring, Compute) to allow targeted updates.

The top risks are: (1) CDK creating duplicate infrastructure alongside existing manually-provisioned resources -- mitigate by tearing down manual resources and letting CDK create fresh, importing only the ECR repository by ARN; (2) EC2 instance replacement during CDK updates destroying all data volumes -- mitigate with `RemovalPolicy.RETAIN`, separate EBS data volume, and mandatory `cdk diff` gate before every deploy; (3) CloudWatch log costs exploding from verbose structured JSON at the wrong log level -- mitigate with 14-day retention, `RUST_LOG=info`, and a $10/month billing alarm. All three are preventable with first-phase discipline.

## Key Findings

### Recommended Stack

The entire v1.6 stack is external tooling -- zero new Rust crate dependencies. This is deliberate: the application binary stays cloud-agnostic and testable locally without AWS. See [STACK.md](STACK.md) for full details.

**Core technologies:**
- **AWS CDK v2 (TypeScript):** All infrastructure provisioning -- VPC, SG, EC2, IAM, ECR (import existing), CloudWatch log group, Secrets Manager, AMP, AMG. Single `aws-cdk-lib` package covers all construct modules.
- **GitLab CI:** 3-stage pipeline (test, build-and-push, deploy). Docker-in-Docker for image builds. SSM Run Command for deployment -- no SSH keys.
- **Prometheus >= 2.53.0 (sidecar):** Scrapes app metrics on :9000, remote_writes to AMP with native SigV4 (no proxy sidecar needed since Prometheus 2.26+).
- **Amazon Managed Prometheus + Grafana:** Durable metrics storage (150-day retention) and dashboard visualization. AWS manages availability, auth, upgrades.
- **Docker `awslogs` driver:** Built-in CloudWatch log shipping. Zero agent installation. Replaces `json-file` driver with one config block change.
- **AWS Secrets Manager + bash fetch script:** Boot-time secret injection via AWS CLI + jq. Writes `.env` file for Docker Compose. Zero Rust code changes.

### Expected Features

See [FEATURES.md](FEATURES.md) for full feature landscape and metrics inventory.

**Must have (table stakes):**
- CDK infrastructure stack -- reproducible, version-controlled infrastructure replacing click-ops
- GitLab CI pipeline -- automated test/build/push/deploy replacing manual SSH cycle
- CloudWatch log aggregation -- remote log access, survives instance termination
- Secrets Manager integration -- encrypted credential storage with IAM access control
- Systemd service for Docker Compose -- survives EC2 reboot
- EC2 instance profile with least-privilege IAM

**Should have (differentiators -- "well-operated" vs "deployed"):**
- Grafana dashboards (5): Feed Health, Signal Quality, Spread Distributions, Paper Trade P&L, System Health
- Grafana alert rules: feed down, zero computations, sustained negative P&L
- Prometheus remote_write to AMP -- durable metrics beyond EC2 lifecycle
- CloudWatch Logs Insights saved queries for common investigation patterns

**Defer (post-v1.6):**
- Container image scanning (Trivy)
- CloudWatch anomaly detection
- Cost optimization (spot/reserved instances)
- Automated DR/backup for state files
- Blue/green deployments, ECS/Fargate, Kubernetes

### Architecture Approach

The architecture adds a CI/CD pipeline (GitLab), infrastructure-as-code (CDK in `infra/cdk/`), and an observability pipeline (Prometheus sidecar -> AMP -> AMG) around an unchanged application binary. The CDK project decomposes into five stacks to allow independent updates. The Prometheus sidecar runs alongside the prediction container in Docker Compose, scraping :9000 and remote_writing to AMP. Secrets flow from Secrets Manager through a bash fetch script to a `.env` file consumed by Docker Compose. Logs flow from container stdout through Docker's `awslogs` driver to CloudWatch. See [ARCHITECTURE.md](ARCHITECTURE.md) for full component boundaries and data flow diagrams.

**Major components:**
1. `infra/cdk/` (5 stacks) -- All AWS resource provisioning: NetworkStack, SecretsStack, LoggingStack, MonitoringStack, ComputeStack
2. `.gitlab-ci.yml` -- CI/CD pipeline: cargo test, docker build + ECR push, SSM deploy
3. `deploy/fetch-secrets.sh` -- Boot-time Secrets Manager fetch, writes `.env`
4. `deploy/prometheus.yml` -- Prometheus sidecar config with SigV4 remote_write
5. `docker-compose.yml` (modified) -- awslogs driver, env_file, Prometheus sidecar service

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for all 8 pitfalls with full recovery strategies.

1. **CDK creates duplicate infrastructure** -- Existing EC2, SGs, VPC are console-created. CDK will create duplicates, not manage existing. Fix: tear down manual resources, let CDK create fresh, import ECR by ARN only. The system tolerates minutes of downtime (WebSocket feeds reconnect automatically).
2. **EC2 instance replaced by CDK update, data lost** -- Changing instance type/AMI/subnet triggers CloudFormation REPLACEMENT, destroying all bind-mounted data (events.toml, checkpoint.json, logs). Fix: `RemovalPolicy.RETAIN`, separate EBS data volume, mandatory `cdk diff` before every deploy.
3. **CloudWatch log costs exploding** -- Structured JSON at DEBUG level + "never expire" retention = $50-200/month. Fix: `RUST_LOG=info`, 14-day retention in CDK, $10/month billing alarm, exclude spread_logs/settlement_logs from CloudWatch.
4. **Secrets not available at container startup** -- Docker Compose on EC2 has NO native Secrets Manager integration (unlike ECS). Fix: `fetch-secrets.sh` runs before `docker compose up`, writes `.env` file.
5. **GitLab CI Rust builds taking 20-40 minutes** -- Ephemeral runners discard build cache. Fix: cargo-chef in Dockerfile for dependency layer caching, sccache with S3 backend, GitLab CI cargo registry cache.

## Implications for Roadmap

Based on combined research, the build order follows strict dependency chains. Each phase produces a testable, observable result. All four research files converge on the same 6-phase structure.

### Phase 1: CDK Infrastructure Foundation
**Rationale:** Everything else depends on AWS resources existing in a codified, reproducible state. VPC, security groups, IAM roles, log groups, and secret shells must exist before any deployment, logging, or monitoring can work.
**Delivers:** NetworkStack, SecretsStack, LoggingStack deployed. VPC, SG, CloudWatch log group, Secrets Manager secret shell created. Manual infrastructure torn down.
**Addresses:** CDK infrastructure stack (table stakes), EC2 instance profile (table stakes)
**Avoids:** Pitfall 1 (duplicate infrastructure -- clean slate approach), Pitfall 5 (CDK bootstrap -- explicit first step), Pitfall 8 (data loss -- RemovalPolicy.RETAIN, separate EBS)

### Phase 2: Compute + Secrets Integration
**Rationale:** The application cannot run without credentials. ComputeStack depends on all Phase 1 stacks. This validates the most critical integration (secrets flow) early on real infrastructure.
**Delivers:** EC2 instance managed by CDK, IAM instance profile, user-data bootstrap, `fetch-secrets.sh` populating `.env`, application running on CDK-created infrastructure with secrets from Secrets Manager.
**Addresses:** Secrets Manager integration (table stakes), Systemd service (table stakes)
**Avoids:** Pitfall 6 (secrets not available -- fetch script verified before pipeline automation)

### Phase 3: CloudWatch Logging
**Rationale:** One config block change (`json-file` to `awslogs`) that immediately provides remote log access. Having logs in CloudWatch before tackling monitoring means debugging the Prometheus sidecar and CI pipeline is possible without SSH.
**Delivers:** Container logs in CloudWatch Logs, Logs Insights queries working on structured JSON fields, 14-day retention configured.
**Addresses:** CloudWatch log aggregation (table stakes), CloudWatch Logs Insights saved queries (differentiator)
**Avoids:** Pitfall 2 (log costs -- retention and log level set from the start)

### Phase 4: Prometheus + AMP + AMG (Monitoring Pipeline)
**Rationale:** The Prometheus sidecar is a new container requiring configuration and IAM permissions. AMP and AMG workspace creation has more moving parts than logging. With CloudWatch already working, sidecar issues are debuggable via remote logs.
**Delivers:** MonitoringStack deployed (AMP + AMG workspaces), Prometheus sidecar in docker-compose.yml, metrics flowing from app -> Prometheus -> AMP -> AMG, AMP data source configured in Grafana.
**Addresses:** Prometheus remote write to AMP (differentiator), AMG workspace setup
**Avoids:** Pitfall 3 (Grafana cannot reach Prometheus -- use AMP as intermediary, not direct VPC connection)

### Phase 5: GitLab CI/CD Pipeline
**Rationale:** Manual deploys work fine during infrastructure buildout. CI automates an already-working manual process. Building CI before the deployment target is stable wastes iteration cycles on pipeline debugging.
**Delivers:** `.gitlab-ci.yml` with test/build-push/deploy stages. Cargo test with cache. Docker build with cargo-chef. ECR push. SSM deploy with manual trigger. No more manual build/push/SSH workflow.
**Addresses:** GitLab CI pipeline (table stakes)
**Avoids:** Pitfall 4 (WebSocket drops during deploy -- graceful shutdown verified), Pitfall 7 (slow CI builds -- cargo-chef + sccache from the start)

### Phase 6: Grafana Dashboards + Alert Rules
**Rationale:** Dashboards are a consumption layer. Building them before metrics flow through AMP is wasteful -- real data is needed to design meaningful visualizations. This phase enables "operate without SSH."
**Delivers:** 5 Grafana dashboards (Feed Health, Signal Quality, Spreads, Paper Trade P&L, System Health), alert rules for critical conditions (feed down, zero computations, negative P&L).
**Addresses:** All 5 dashboard differentiators, Grafana alert rules (differentiator)
**Avoids:** No critical pitfalls -- this is visualization of already-flowing data.

### Phase Ordering Rationale

- CDK foundation first because every subsequent phase needs AWS resources (VPC, IAM, log groups) to exist
- Secrets + compute second because the application cannot run without credentials and this validates the hardest integration early
- Logging third because it is a trivial change that immediately enables remote debugging for all subsequent phases
- Monitoring fourth because the Prometheus sidecar adds container orchestration complexity best debugged with CloudWatch already available
- CI fifth because manual deploys work during buildout and CI should automate an already-stable process
- Dashboards last because they require real data flowing through the metrics pipeline

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1 (CDK Foundation):** Clean slate migration from manual infrastructure requires careful sequencing of teardown and recreation. The ECR import-by-ARN pattern and `RemovalPolicy.RETAIN` configuration need verification against CDK docs during phase planning.
- **Phase 4 (Monitoring Pipeline):** Prometheus sidecar SigV4 configuration, AMP workspace ID injection into prometheus.yml, and AMG data source auto-configuration have multiple moving parts. Research the exact CDK output -> config injection path.

Phases with standard patterns (skip research-phase):
- **Phase 2 (Compute + Secrets):** EC2 user-data + Secrets Manager fetch is a well-documented AWS pattern with official reference implementations.
- **Phase 3 (CloudWatch Logging):** One config block swap from `json-file` to `awslogs`. Docker official docs are definitive.
- **Phase 5 (GitLab CI):** GitLab CI + Docker-in-Docker + ECR push is a thoroughly documented pattern. cargo-chef has clear Dockerfile examples.
- **Phase 6 (Dashboards):** Standard Grafana panel configuration against known Prometheus metrics. The metrics inventory in FEATURES.md is complete.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All technologies are mature AWS managed services with stable APIs. Zero new Rust dependencies. CDK v2, Prometheus, CloudWatch awslogs driver are all production-proven. |
| Features | HIGH | Feature list derived from direct codebase analysis of existing infrastructure gaps. 80+ Prometheus metrics already emitted. Structured JSON logging already in place. |
| Architecture | HIGH | Based on official AWS docs, verified CDK patterns, and direct analysis of existing Dockerfile/docker-compose/deploy scripts. All integration points use documented, first-party patterns. |
| Pitfalls | HIGH | 8 pitfalls identified from official docs, community patterns, and direct infrastructure analysis. All have concrete prevention strategies with phase assignments. |

**Overall confidence:** HIGH

### Gaps to Address

- **Managed Grafana vs direct Prometheus scrape:** PITFALLS.md recommends VPC connection for direct scrape (avoiding AMP cost), while STACK.md and ARCHITECTURE.md recommend AMP as intermediary. Recommendation: use AMP. The $3-5/month cost is negligible and eliminates VPC connectivity complexity. AMP also provides durable storage beyond EC2 lifecycle. Resolve during Phase 4 planning.

- **SIGTERM handler in Rust binary:** Pitfall 4 (WebSocket drops during deploy) notes that `tokio::signal::ctrl_c()` catches SIGINT but not SIGTERM on Unix. Verify whether the existing signal handler covers SIGTERM before Phase 5. If not, this is a minor Rust code change that contradicts the "zero Rust changes" premise -- but it is a one-line fix (`signal::unix::signal(SignalKind::terminate())`).

- **cargo-chef Dockerfile restructuring:** Adding cargo-chef requires modifying the multi-stage Dockerfile (new prepare + cook stages). This is a functional change to an existing file, not just infrastructure. Plan the Dockerfile change as part of Phase 5.

- **EC2 instance sizing:** ARCHITECTURE.md suggests t3.small; current manual instance type is unknown. Verify current instance type and memory usage before CDK provisioning. The prediction container + Prometheus sidecar together need adequate RAM.

## Sources

### Primary (HIGH confidence)
- [AWS CDK v2 TypeScript Guide](https://docs.aws.amazon.com/cdk/v2/guide/work-with-cdk-typescript.html) -- CDK setup, module structure, construct levels
- [AWS CDK EC2 Instance Construct](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ec2.Instance.html) -- instance profile, user-data
- [AMP Remote Write from EC2](https://docs.aws.amazon.com/prometheus/latest/userguide/AMP-onboard-ingest-metrics-remote-write-EC2.html) -- SigV4 native config, IAM requirements
- [Docker awslogs Driver](https://docs.docker.com/engine/logging/drivers/awslogs/) -- configuration options, IAM permissions, non-blocking mode
- [AWS CDK Bootstrap Guide](https://docs.aws.amazon.com/cdk/v2/guide/bootstrapping.html) -- one-time setup, trust configuration
- [Prometheus SigV4 Native Support](https://aws.amazon.com/blogs/opensource/prometheus-2-26-0-adds-aws-signature-version-4-support/) -- confirmed since 2.26.0

### Secondary (MEDIUM confidence)
- [GitLab CI Docker-in-Docker](https://docs.gitlab.com/ee/ci/docker/using_docker_build.html) -- DinD setup, privileged runners
- [GitLab CI ECR Push Pattern](https://gist.github.com/tanmay-bhat/6fa65b9cd9d5f7f5e780dbe3efcb1fb7) -- ECR login, tag/push
- [cargo-chef for Docker Layer Caching](https://github.com/LukeMathWalker/cargo-chef) -- Dockerfile restructuring for dependency caching
- [Secrets Manager on EC2](https://repost.aws/questions/QUNHr37DAhQxqTUQgeUUFPEA/ec2-and-secret-manager) -- CLI fetch pattern
- [Amazon Managed Grafana VPC Configuration](https://docs.aws.amazon.com/grafana/latest/userguide/AMG-configure-vpc.html) -- VPC connectivity options

### Tertiary (LOW confidence)
- None. All findings verified against at least two sources.

---
*Research completed: 2026-03-07*
*Ready for roadmap: yes*
