# Stack Research: v1.6 Production Deployment

**Domain:** AWS infrastructure-as-code, CI/CD, and observability for a Rust single-binary service
**Researched:** 2026-03-07
**Confidence:** HIGH (all technologies are mature AWS managed services with stable APIs)

## Scope

This document covers ONLY the stack additions needed for v1.6 Production Deployment. The existing Rust application stack (v1.0-v1.5) is unchanged. No new Rust crate dependencies are needed. All additions are infrastructure tooling (TypeScript CDK, YAML CI config, AWS managed services).

---

## Executive Finding: Zero Rust Code Changes

v1.6 adds zero new Rust dependencies. The entire deployment stack is external to the application binary:

- **AWS CDK** (TypeScript) for infrastructure provisioning
- **GitLab CI** (YAML) for build/test/deploy pipeline
- **Prometheus** (standalone binary on EC2) for metrics scraping and remote write
- **AWS managed services** (AMP, AMG, CloudWatch, Secrets Manager) configured via CDK
- **Docker compose** config change (logging driver swap)
- **Bash scripts** for secrets injection at container startup

---

## Recommended Stack

### Infrastructure as Code: AWS CDK (TypeScript)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| aws-cdk-lib | ^2.241.0 | All AWS construct libraries in one package | CDK v2 monolith package; single dependency covers EC2, ECR, IAM, Secrets Manager, CloudWatch, APS, Grafana |
| aws-cdk (CLI) | ^2.1109.0 | Synthesize and deploy CloudFormation stacks | Required companion CLI for `cdk deploy` |
| constructs | ^10.0.0 | Base construct library | Peer dependency of aws-cdk-lib |
| TypeScript | ^5.4 | CDK language | CDK's primary language; best documentation coverage and most examples |
| Node.js | >=18.x LTS | CDK runtime | Required by CDK v2; use 18 or 20 LTS |

**CDK Construct Modules (all from aws-cdk-lib, zero additional npm packages):**

| Module | Construct Level | Resources Created |
|--------|-----------------|-------------------|
| `aws_ec2` | L2 | Vpc, SecurityGroup, Instance, UserData, KeyPair |
| `aws_ecr` | L2 | Repository.fromRepositoryName (reference existing repo) |
| `aws_iam` | L2 | Role, ManagedPolicy, PolicyStatement for instance profile |
| `aws_secretsmanager` | L2 | Secret, SecretStringGenerator; `.grantRead()` helper |
| `aws_logs` | L2 | LogGroup with retention period |
| `aws_aps` | **L1** (CfnWorkspace) | Amazon Managed Prometheus workspace |
| `aws_grafana` | **L1** (CfnWorkspace) | Amazon Managed Grafana workspace |

L1 note: APS and Grafana only have auto-generated CloudFormation-level constructs in aws-cdk-lib. This is acceptable because these resources are created once and rarely modified. Do NOT pull community L2 wrapper packages; they add maintenance risk for negligible benefit on one-time provisioning.

### CI/CD: GitLab CI

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| GitLab CI | N/A (SaaS) | Pipeline orchestration | Project is already on GitLab; native CI avoids external tooling |
| Docker-in-Docker (dind) | 27.x | Build Docker images inside CI runners | Required for `docker build` in GitLab shared runners |
| `rust:latest` image | stable | Rust compilation stage | Official image; matches existing Dockerfile builder stage |
| `amazon/aws-cli` image | 2.x | ECR login, SSM deploy commands | Official AWS image for deployment steps |

### Observability: Prometheus + AMP + AMG

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| Prometheus server | >=2.53.0 | Scrape app metrics on :9000, remote_write to AMP | Runs on EC2 alongside container; native SigV4 since 2.26 eliminates signing proxy |
| Amazon Managed Prometheus (AMP) | managed | Long-term Prometheus metrics storage | No self-hosted TSDB; automatic scaling; 150-day default retention |
| Amazon Managed Grafana (AMG) | managed | Dashboard visualization and alerting | No self-hosted Grafana upgrades/auth/plugins; native AMP data source |
| CloudWatch Logs (awslogs driver) | Docker built-in | Container log aggregation | Zero-agent approach: Docker's built-in driver ships structured JSON logs to CloudWatch |

### Secrets Management

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| AWS Secrets Manager | managed | Store venue API keys (Deribit, Derive wallet key, Kalshi) | KMS encryption at rest; IAM-gated access; CloudTrail audit trail |
| AWS CLI v2 | 2.x | Fetch secrets in bash startup script | Already on EC2; simple `get-secret-value` call; no Rust SDK needed |
| jq | latest | Parse JSON secret values into env vars | Standard CLI tool; 1 line per secret extraction |

---

## Architecture Decisions

### 1. Secrets Injection: Bash Startup Script (NOT Rust SDK)

**Decision:** Use a bash wrapper script that fetches secrets from Secrets Manager via AWS CLI and exports them as environment variables before `docker compose up`.

**Do NOT** add `aws-sdk-secretsmanager` to Cargo.toml.

Rationale:
- The Rust binary already reads API keys from environment variables / config files
- Adding the AWS SDK would introduce ~15 new transitive crate dependencies
- A 10-line bash script achieves identical result with zero application code changes
- Secrets rotate rarely (API keys, not short-lived tokens)
- Keeps the binary cloud-agnostic (testable locally without AWS)

Pattern:
```bash
#!/bin/bash
# /app/start.sh on EC2
set -euo pipefail

SECRET=$(aws secretsmanager get-secret-value \
  --secret-id prediction/prod/api-keys \
  --query SecretString --output text \
  --region us-east-1)

export DERIBIT_CLIENT_ID=$(echo "$SECRET" | jq -r .deribit_client_id)
export DERIBIT_CLIENT_SECRET=$(echo "$SECRET" | jq -r .deribit_client_secret)
export DERIVE_WALLET_KEY=$(echo "$SECRET" | jq -r .derive_wallet_key)
# ... remaining keys

cd /app && docker compose up -d
```

### 2. Prometheus Remote Write: Native SigV4 (NOT Proxy Sidecar)

**Decision:** Run Prometheus >=2.26 on EC2 with native `sigv4` block in `remote_write` config. Do NOT use the `aws-sigv4-proxy` sidecar container.

Rationale:
- Native SigV4 support was added in Prometheus 2.26 (April 2021) and is the recommended AWS approach
- Eliminates a proxy container and its failure modes
- EC2 instance role provides credentials automatically via IMDS -- no key management
- Prometheus also serves as the local scraper (15s interval on :9000)

Configuration (prometheus.yml on EC2):
```yaml
scrape_configs:
  - job_name: prediction
    static_configs:
      - targets: ['localhost:9000']
    scrape_interval: 15s

remote_write:
  - url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/WORKSPACE_ID/api/v1/remote_write
    queue_config:
      max_samples_per_send: 1000
      max_shards: 200
      capacity: 2500
    sigv4:
      region: us-east-1
```

### 3. CloudWatch Logs: awslogs Docker Driver (NOT CloudWatch Agent)

**Decision:** Change docker-compose.yml logging driver from `json-file` to `awslogs`. Do NOT install the CloudWatch Unified Agent.

Rationale:
- Application already outputs JSON-structured logs via `tracing` crate
- `awslogs` is built into Docker Engine -- zero installation, zero extra processes
- Current `json-file` driver with 50MB rotation loses all logs on instance termination
- CloudWatch Agent would add another daemon to monitor on a single-container host

docker-compose.yml change:
```yaml
logging:
  driver: awslogs
  options:
    awslogs-region: us-east-1
    awslogs-group: /prediction/prod
    awslogs-stream: "prediction-{{.ID}}"
    awslogs-create-group: "true"
    mode: non-blocking
    max-buffer-size: 4m
```

Required IAM permissions on EC2 instance role:
- `logs:CreateLogGroup`
- `logs:CreateLogStream`
- `logs:PutLogEvents`

### 4. Managed Grafana Auth: AWS IAM Identity Center (SSO)

Amazon Managed Grafana requires an identity provider. Use **AWS IAM Identity Center** as the auth provider. For a solo trader, this means:
- One SSO user with ADMIN role on the Grafana workspace
- SSO is free for this scale
- No SAML IdP or Active Directory needed

### 5. Deploy Mechanism: SSM Send-Command (NOT SSH)

**Decision:** Deploy from GitLab CI via `aws ssm send-command` to the EC2 instance. Do NOT use SSH keys in CI variables.

Rationale:
- No SSH key management or rotation in CI
- SSM provides CloudTrail audit trail of all commands
- Works through NAT gateways (no public SSH port needed)
- EC2 instances with Amazon Linux / Ubuntu have SSM agent pre-installed
- SecurityGroup can have zero inbound rules (SSH port 22 closed)

### 6. No Cross-Compilation Needed

The existing Dockerfile builds inside `rust:latest` (Linux x86_64 glibc), producing a Linux binary. Docker handles the build environment regardless of the CI runner's OS. The `docker build` command in GitLab CI produces an identical image to local builds.

---

## GitLab CI Pipeline Structure

```yaml
# .gitlab-ci.yml
stages:
  - test
  - build
  - deploy

variables:
  ECR_REGISTRY: 606103597377.dkr.ecr.us-east-1.amazonaws.com
  ECR_REPOSITORY: prediction

test:
  stage: test
  image: rust:latest
  cache:
    key: cargo-${CI_COMMIT_REF_SLUG}
    paths:
      - target/
      - .cargo/registry/
  variables:
    CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  script:
    - cargo test --release
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
    - if: $CI_COMMIT_BRANCH == "master"

build-and-push:
  stage: build
  image: docker:27
  services:
    - docker:27-dind
  variables:
    DOCKER_TLS_CERTDIR: "/certs"
  before_script:
    - apk add --no-cache aws-cli
    - aws ecr get-login-password --region us-east-1 | docker login --username AWS --password-stdin ${ECR_REGISTRY}
  script:
    - docker build -t ${ECR_REGISTRY}/${ECR_REPOSITORY}:${CI_COMMIT_SHA} -t ${ECR_REGISTRY}/${ECR_REPOSITORY}:latest .
    - docker push ${ECR_REGISTRY}/${ECR_REPOSITORY}:${CI_COMMIT_SHA}
    - docker push ${ECR_REGISTRY}/${ECR_REPOSITORY}:latest
  rules:
    - if: $CI_COMMIT_BRANCH == "master"

deploy:
  stage: deploy
  image: amazon/aws-cli:2
  script:
    - |
      aws ssm send-command \
        --instance-ids "${EC2_INSTANCE_ID}" \
        --document-name "AWS-RunShellScript" \
        --parameters 'commands=["cd /app && docker compose pull && /app/start.sh"]' \
        --region us-east-1
  rules:
    - if: $CI_COMMIT_BRANCH == "master"
      when: manual
```

CI variables to set in GitLab Settings > CI/CD > Variables:
- `AWS_ACCESS_KEY_ID` -- CI deploy user (not EC2 role)
- `AWS_SECRET_ACCESS_KEY` -- CI deploy user
- `AWS_DEFAULT_REGION` -- `us-east-1`
- `EC2_INSTANCE_ID` -- target EC2 instance ID

---

## EC2 Instance IAM Role Policies

The instance profile needs these policies (provisioned by CDK):

| Policy / Permission | Purpose |
|---------------------|---------|
| `AmazonPrometheusRemoteWriteAccess` (managed) | Prometheus remote_write to AMP workspace |
| `AmazonSSMManagedInstanceCore` (managed) | SSM agent for remote deploy commands |
| `AmazonEC2ContainerRegistryReadOnly` (managed) | Pull images from ECR |
| Custom: `secretsmanager:GetSecretValue` on `arn:aws:secretsmanager:*:*:secret:prediction/prod/*` | Read API key secrets |
| Custom: `logs:CreateLogGroup`, `logs:CreateLogStream`, `logs:PutLogEvents` | Docker awslogs driver |

---

## Installation

```bash
# CDK infrastructure project (new directory alongside Rust project)
mkdir -p infra && cd infra
npx cdk init app --language typescript
# aws-cdk-lib and constructs are auto-installed by cdk init

# Verify versions
npx cdk --version  # Should show 2.x
```

```bash
# On EC2 instance (via CDK UserData script)
# 1. Docker + Docker Compose (Amazon Linux 2023)
sudo yum install -y docker
sudo systemctl enable docker && sudo systemctl start docker
sudo curl -L "https://github.com/docker/compose/releases/latest/download/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
sudo chmod +x /usr/local/bin/docker-compose

# 2. Prometheus for remote_write
sudo useradd --no-create-home --shell /bin/false prometheus
PROM_VERSION=2.53.0
curl -LO https://github.com/prometheus/prometheus/releases/download/v${PROM_VERSION}/prometheus-${PROM_VERSION}.linux-amd64.tar.gz
tar xzf prometheus-${PROM_VERSION}.linux-amd64.tar.gz
sudo cp prometheus-${PROM_VERSION}.linux-amd64/prometheus /usr/local/bin/
# Configure as systemd service with prometheus.yml

# 3. jq for secrets parsing
sudo yum install -y jq
```

---

## Alternatives Considered

| Recommended | Alternative | Why Not |
|-------------|-------------|---------|
| AWS CDK (TypeScript) | Terraform | CDK produces CloudFormation natively; L2 constructs handle IAM/SG defaults; TypeScript is more expressive than HCL for this scale |
| AWS CDK (TypeScript) | Pulumi | Smaller ecosystem; fewer AWS-specific examples; CDK has official AWS backing |
| AWS CDK (TypeScript) | Raw CloudFormation | Verbose YAML/JSON; no type checking; CDK generates it with type safety |
| Amazon Managed Prometheus | Self-hosted Prometheus with EBS | Eliminates TSDB storage management, backup scripts, retention policies |
| Amazon Managed Grafana | Self-hosted Grafana on EC2 | Eliminates upgrade maintenance, auth config, plugin management, HTTPS cert renewal |
| awslogs Docker driver | CloudWatch Agent | Agent is another process to manage; awslogs is built into Docker |
| awslogs Docker driver | Fluentd / Fluent Bit | Overkill for single-container deployment; adds operational complexity |
| Bash secrets injection | AWS SDK in Rust binary | Zero Rust code changes; no new deps; keeps binary cloud-agnostic |
| Bash secrets injection | Docker secrets / .env files on disk | Secrets Manager provides encryption, IAM access control, CloudTrail audit |
| SSM Send-Command deploy | SSH from CI | No SSH key management; SSM provides audit trail; no public SSH port |
| SSM Send-Command deploy | AWS CodeDeploy | Heavyweight agent for a single-instance pull-and-restart; SSM is simpler |
| EC2 single instance | ECS Fargate | More expensive; adds ECS task definition complexity; overkill for one container |
| EC2 single instance | EKS | Massively overkill; Kubernetes operational burden for one container |
| Prometheus on EC2 | AWS Distro for OpenTelemetry (ADOT) | Prometheus is simpler for this use case; app already exposes /metrics; ADOT adds collector complexity |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `aws-sdk-secretsmanager` Rust crate | ~15 transitive deps for what a 10-line bash script does; couples binary to AWS | AWS CLI in startup script |
| `aws-sigv4-proxy` container | Unnecessary since Prometheus 2.26+ has native SigV4 | `sigv4:` block in prometheus.yml |
| CloudWatch Agent | Another daemon to manage and monitor on a single-container host | Docker `awslogs` driver (built-in) |
| Self-hosted Grafana | Upgrade maintenance, auth setup, TLS cert management for solo operator | Amazon Managed Grafana |
| Community CDK L2 constructs for Grafana/APS | Unclear maintenance; small packages; L1 CfnWorkspace is sufficient for one-time setup | `CfnWorkspace` from aws-cdk-lib |
| Docker Swarm / Kubernetes | Container orchestration adds complexity with zero benefit for single binary | docker-compose with `restart: unless-stopped` |
| Ansible / Chef / Puppet | No fleet to manage; CDK UserData handles instance bootstrapping | CDK `ec2.UserData.forLinux()` |
| cargo-chef in Dockerfile | Optimization for later; current build works; adds Dockerfile complexity | Existing two-stage Dockerfile |
| GitHub Actions | Project is on GitLab | GitLab CI |

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| aws-cdk-lib ^2.241.0 | constructs ^10.0.0 | Peer dependency; installed together by `cdk init` |
| aws-cdk-lib ^2.241.0 | Node.js >=18.x | Node 16 is EOL; use 18.x or 20.x LTS |
| aws-cdk CLI ^2.1109.0 | aws-cdk-lib ^2.241.0 | CLI version can be higher; lib version determines available constructs |
| Prometheus >=2.53.0 | AMP remote_write with SigV4 | Any 2.26+ works for SigV4; 2.53 is current stable |
| Docker awslogs driver | Docker Engine 27.x | Built-in driver; no compatibility concern |
| GitLab CI dind service | docker:27-dind | Service image version should match main image |
| Amazon Linux 2023 AMI | Docker, SSM Agent, AWS CLI v2 | All pre-available or in default repos |

---

## Dependency Growth Summary

| Milestone | New Rust Crates | New Infrastructure Tools |
|-----------|----------------|--------------------------|
| v1.0 | Baseline (19 direct deps) | Docker, docker-compose |
| v1.1 | 0 | -- |
| v1.2 | 1 (strsim) | -- |
| v1.3 | 0 | -- |
| v1.4 | 2 (comfy-table, csv) | -- |
| v1.5 | 1 (k256) | -- |
| **v1.6** | **0** | **CDK (TypeScript), GitLab CI, Prometheus, AMP, AMG, Secrets Manager** |

The zero-new-Rust-crate pattern for v1.6 is deliberate. All deployment infrastructure is external to the application binary, maintaining the existing supply chain size.

---

## Sources

- [AWS CDK v2 TypeScript Guide](https://docs.aws.amazon.com/cdk/v2/guide/work-with-cdk-typescript.html) -- CDK setup and module structure (HIGH confidence)
- [aws-cdk-lib on npm](https://www.npmjs.com/package/aws-cdk-lib) -- current version 2.241.0 (HIGH confidence)
- [aws-cdk CLI on npm](https://www.npmjs.com/package/aws-cdk) -- current version 2.1109.0 (HIGH confidence)
- [aws-cdk-lib.aws_ec2 module](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ec2-readme.html) -- EC2 L2 constructs (HIGH confidence)
- [aws-cdk-lib.aws_grafana module](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_grafana-readme.html) -- L1 constructs only confirmed (HIGH confidence)
- [CfnWorkspace (APS)](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_aps.CfnWorkspace.html) -- AMP workspace L1 construct (HIGH confidence)
- [CfnWorkspace (Grafana)](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_grafana.CfnWorkspace.html) -- Grafana workspace L1 construct (HIGH confidence)
- [Prometheus remote_write for EC2](https://docs.aws.amazon.com/prometheus/latest/userguide/AMP-onboard-ingest-metrics-remote-write-EC2.html) -- SigV4 native config, IAM role requirement, Prometheus >=2.26 (HIGH confidence)
- [Docker awslogs driver](https://docs.docker.com/engine/logging/drivers/awslogs/) -- all options, IAM permissions, non-blocking mode (HIGH confidence)
- [GitLab CI Rust patterns](https://dev.to/hatembentayeb/optimizing-ci-cd-pipeline-for-rust-projects-gitlab-docker-hc9) -- caching, dind setup (MEDIUM confidence)
- [GitLab CI ECR push gist](https://gist.github.com/tanmay-bhat/6fa65b9cd9d5f7f5e780dbe3efcb1fb7) -- ECR login pattern (MEDIUM confidence)
- [Secrets Manager on EC2](https://repost.aws/questions/QUNHr37DAhQxqTUQgeUUFPEA/ec2-and-secret-manager) -- CLI fetch pattern for Docker env vars (HIGH confidence)
- [ECS default log mode change](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/using_awslogs.html) -- non-blocking default since June 2025 (MEDIUM confidence)

---
*Stack research for: v1.6 Production Deployment*
*Researched: 2026-03-07*
