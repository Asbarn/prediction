# Phase 38: GitLab CI/CD Pipeline - Research

**Researched:** 2026-03-08
**Domain:** GitLab CI/CD, Docker builds, AWS ECR/SSM, Rust build caching
**Confidence:** HIGH

## Summary

This phase replaces the manual build-push-SSH deployment workflow with an automated GitLab CI pipeline. The pipeline has three stages: `test` (cargo test), `build-and-push` (Docker build with cargo-chef caching, push to ECR), and `deploy` (SSM Send-Command to EC2). All infrastructure prerequisites exist: ECR repository imported in CDK, EC2 instance with SSM agent and AmazonSSMManagedInstanceCore policy, systemd service for docker-compose, and fetch-secrets.sh for credential injection.

The primary technical challenges are: (1) implementing cargo-chef layer caching in the Dockerfile to keep builds under 10 minutes, (2) correctly installing AWS CLI in Alpine-based Docker DinD images, (3) waiting for SSM command completion and verifying /health endpoint after deploy, and (4) ensuring the deploy script coordinates with the existing systemd service and fetch-secrets.sh flow on EC2.

**Primary recommendation:** Create a `.gitlab-ci.yml` with 3 stages (test, build-and-push, deploy), modify the Dockerfile to use cargo-chef for dependency layer caching, and use `aws ssm send-command` + `aws ssm wait command-executed` for deployment with post-deploy health verification.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CICD-01 | GitLab CI pipeline runs `cargo test` on every push to master | Test stage using `rust:1.85` image with GitLab CI cache for target/ and .cargo/ |
| CICD-02 | Pipeline builds Docker image and pushes to ECR on successful test | Build-and-push stage using docker:27 + dind, ECR auth via aws ecr get-login-password |
| CICD-03 | Pipeline deploys to EC2 via SSM Send-Command (stop, pull, start container) | Deploy stage using amazon/aws-cli image, ssm send-command + wait command-executed |
| CICD-04 | Build uses cargo-chef layer caching to reduce Rust compile times below 10 minutes | Modified Dockerfile with 3-stage cargo-chef pattern (planner, cook, build) |
| CICD-05 | Pipeline deploy stage verifies /health endpoint responds after container start | curl health check on port 9001 via SSM command or separate pipeline step |
</phase_requirements>

## Standard Stack

### Core

| Technology | Version | Purpose | Why Standard |
|------------|---------|---------|--------------|
| GitLab CI | SaaS | Pipeline orchestration | Project milestone specifies GitLab CI |
| Docker-in-Docker (dind) | 27 | Build Docker images in CI | Required for `docker build` in GitLab shared runners |
| cargo-chef | latest (0.1.73+) | Rust dependency layer caching | Standard approach for sub-10-min Rust Docker builds |
| amazon/aws-cli | 2.x | ECR login, SSM deploy commands | Official AWS image; avoids Alpine awscli install issues |

### Supporting

| Technology | Version | Purpose | When to Use |
|------------|---------|---------|-------------|
| rust (Docker image) | 1.85 | Test stage compilation | Matches rust-version in Cargo.toml |
| docker:27-dind | 27 | DinD service for build stage | Paired with docker:27 main image |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| cargo-chef | sccache + S3 | sccache gives finer-grained caching but requires S3 bucket setup; cargo-chef is simpler and sufficient |
| DinD builds | Kaniko | Kaniko avoids privileged mode but has more complex setup; DinD is standard for GitLab |
| `apk add aws-cli` in docker:27 | Separate amazon/aws-cli stage | Alpine awscli install is fragile (glibc issues); use amazon/aws-cli image for deploy stage |

## Architecture Patterns

### Pipeline Structure

```
.gitlab-ci.yml
├── stages: [test, build-and-push, deploy]
├── test          -> rust:1.85 image, cargo test --release, cache target/
├── build-and-push -> docker:27 + dind, docker build + ECR push
└── deploy        -> amazon/aws-cli, ssm send-command + health verify
```

### Pattern 1: cargo-chef 3-Stage Dockerfile

**What:** Split the Dockerfile into planner, builder (cook + compile), and runtime stages. The `cook` stage builds only dependencies from a recipe.json file. This layer is cached as long as Cargo.toml/Cargo.lock don't change.

**When to use:** Every Rust Docker build with more than a few dependencies.

**Example:**
```dockerfile
# Stage 1: Planner - compute dependency recipe
FROM rust:1.85 AS planner
RUN cargo install cargo-chef
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Builder - cook dependencies (cached layer), then build source
FROM rust:1.85 AS builder
RUN cargo install cargo-chef
RUN apt-get update && apt-get install -y libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /build

# Cook dependencies (this layer is cached if recipe.json hasn't changed)
COPY --from=planner /build/recipe.json recipe.json
ENV CARGO_BUILD_JOBS=2
RUN cargo chef cook --release --recipe-path recipe.json

# Build application (only this layer rebuilds on source changes)
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/prediction .
COPY --from=builder /build/target/release/spread-analytics .
COPY --from=builder /build/target/release/signal-scoring .
EXPOSE 9000 9001
CMD ["./prediction", "--config-dir", "config"]
```

### Pattern 2: SSM Deploy with Wait and Health Verification

**What:** Use `aws ssm send-command` to execute deploy commands on EC2, then `aws ssm wait command-executed` to poll until completion, then verify the /health endpoint.

**When to use:** Deploy stage of CI pipeline.

**Example deploy commands sent via SSM:**
```bash
# Stop the systemd service (which runs docker compose down)
systemctl stop prediction

# Pull new image and restart
/opt/prediction/fetch-secrets.sh
cd /opt/prediction && docker compose pull
systemctl start prediction

# Wait for health check
sleep 20
curl -f http://localhost:9001/health
```

### Pattern 3: GitLab CI Cache for Test Stage

**What:** Cache cargo registry and target directory to speed up `cargo test` across pipeline runs.

**Example:**
```yaml
test:
  stage: test
  image: rust:1.85
  variables:
    CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  cache:
    key: cargo-${CI_COMMIT_REF_SLUG}
    paths:
      - .cargo/registry/
      - .cargo/git/
      - target/
  script:
    - cargo test --release
```

### Anti-Patterns to Avoid

- **Installing awscli via apk in docker:27 Alpine image:** Fragile due to glibc issues. Use amazon/aws-cli image for AWS operations or a multi-stage approach.
- **SSH keys in GitLab CI variables:** Use SSM Send-Command instead. No port 22 needed, full audit trail.
- **Runtime secrets (Deribit API keys etc.) in GitLab CI variables:** Runtime secrets stay in AWS Secrets Manager. GitLab CI only holds AWS credentials for ECR push and SSM commands.
- **Manual deploy trigger by default:** The success criteria say "every push to master automatically deploys." Deploy should be automatic on master, not `when: manual`.
- **Skipping health verification after deploy:** The deploy is not "done" until /health returns 200.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Rust dependency caching in Docker | Custom cargo vendor or manual dep copying | cargo-chef | Handles all edge cases (build scripts, proc macros, workspace crates) |
| SSM command completion polling | Custom bash while-loop | `aws ssm wait command-executed` | Built-in AWS CLI waiter, polls every 5s, exits 255 after 20 failures |
| ECR authentication | Long-lived tokens or stored passwords | `aws ecr get-login-password` per job | ECR tokens expire every 12 hours; always generate fresh |
| Docker layer cache invalidation | Manual COPY ordering tricks | cargo-chef recipe.json | Purpose-built for Rust; handles Cargo.lock changes correctly |

## Common Pitfalls

### Pitfall 1: Builds Exceeding 10 Minutes Without cargo-chef

**What goes wrong:** Clean Rust builds of this project (39K+ LOC, heavy deps like tokio, reqwest, serde, axum, rsa, statrs) take 15-30 minutes. Without layer caching, every pipeline is a clean build.
**Why it happens:** GitLab CI runners are ephemeral. Docker layer cache is lost between runs unless using a persistent runner with local Docker cache.
**How to avoid:** cargo-chef separates dependency compilation into a cached Docker layer. Source-only changes skip the entire dependency build. Expected: 3-5 min for source-only changes.
**Warning signs:** Docker build logs show "Downloading crates" and "Compiling tokio" on every run.

### Pitfall 2: DinD TLS Certificate Issues

**What goes wrong:** Docker build commands fail with "connection refused" or TLS errors when using Docker-in-Docker.
**Why it happens:** Docker DinD requires TLS certificates to communicate between the client and daemon. If DOCKER_TLS_CERTDIR is not set correctly, or the certs volume is not shared, the connection fails.
**How to avoid:** Set `DOCKER_TLS_CERTDIR: "/certs"` in variables and use `docker:27-dind` service matching the main `docker:27` image version.
**Warning signs:** "Cannot connect to the Docker daemon" errors in build stage.

### Pitfall 3: SSM Command Timing Out or Silently Failing

**What goes wrong:** `aws ssm send-command` returns success immediately (it's async). The deploy stage "passes" but the actual command failed on EC2.
**Why it happens:** send-command only submits the command; it doesn't wait for execution. Without waiting, the pipeline has no idea if the deploy succeeded.
**How to avoid:** Capture the command ID from send-command output, then use `aws ssm wait command-executed --command-id $CMD_ID --instance-id $INSTANCE_ID` to block until completion. Check exit code.
**Warning signs:** Deploy stage always passes even when EC2 is down.

### Pitfall 4: Health Endpoint Not Ready After Container Start

**What goes wrong:** Health check runs before the application finishes initializing (WebSocket connections, config loading, etc.), returns connection refused or 503.
**Why it happens:** The prediction container has a `start_period: 15s` in its healthcheck config. The health endpoint on port 9001 needs the Axum server to bind.
**How to avoid:** Add a retry loop with sleep in the SSM deploy script. Wait 20-30 seconds after `systemctl start prediction`, then poll /health with retries.
**Warning signs:** Intermittent deploy failures with "Connection refused" on /health.

### Pitfall 5: ECR Token Expiration

**What goes wrong:** Docker push to ECR fails with authentication error.
**Why it happens:** ECR tokens expire after 12 hours. If a previous token was cached or hardcoded, it will eventually fail.
**How to avoid:** Always run `aws ecr get-login-password` fresh in every pipeline run, never cache the token.
**Warning signs:** Builds that worked yesterday fail today with "no basic auth credentials."

### Pitfall 6: GitLab CI Variables Not Masked

**What goes wrong:** AWS credentials appear in CI job logs.
**Why it happens:** Variables not marked as "masked" in GitLab settings.
**How to avoid:** In GitLab Settings > CI/CD > Variables, mark AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY as both "masked" and "protected."

## Code Examples

### Complete .gitlab-ci.yml Structure

```yaml
# Source: project research + GitLab CI docs
stages:
  - test
  - build-and-push
  - deploy

variables:
  ECR_REGISTRY: "606103597377.dkr.ecr.us-east-1.amazonaws.com"
  ECR_REPOSITORY: "prediction"
  AWS_DEFAULT_REGION: "us-east-1"

test:
  stage: test
  image: rust:1.85
  variables:
    CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  cache:
    key: cargo-${CI_COMMIT_REF_SLUG}
    paths:
      - .cargo/registry/
      - .cargo/git/
      - target/
  script:
    - cargo test --release
  rules:
    - if: $CI_COMMIT_BRANCH == "master"

build-and-push:
  stage: build-and-push
  image: docker:27
  services:
    - docker:27-dind
  variables:
    DOCKER_TLS_CERTDIR: "/certs"
  before_script:
    - apk add --no-cache aws-cli
    - aws ecr get-login-password --region ${AWS_DEFAULT_REGION} | docker login --username AWS --password-stdin ${ECR_REGISTRY}
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
      CMD_ID=$(aws ssm send-command \
        --instance-ids "${EC2_INSTANCE_ID}" \
        --document-name "AWS-RunShellScript" \
        --timeout-seconds 300 \
        --parameters commands='[
          "systemctl stop prediction",
          "cd /opt/prediction && /opt/prediction/fetch-secrets.sh",
          "docker compose pull",
          "systemctl start prediction",
          "sleep 25",
          "curl -sf http://localhost:9001/health || (echo HEALTH CHECK FAILED && exit 1)"
        ]' \
        --region ${AWS_DEFAULT_REGION} \
        --query "Command.CommandId" --output text)
    - echo "SSM Command ID: ${CMD_ID}"
    - aws ssm wait command-executed --command-id "${CMD_ID}" --instance-id "${EC2_INSTANCE_ID}"
    - echo "Deploy completed successfully"
  rules:
    - if: $CI_COMMIT_BRANCH == "master"
```

**Note:** The `apk add --no-cache aws-cli` in the build stage works on Alpine-based docker:27 images. If this proves unreliable, use a separate `before_script` that installs via pip, or split ECR auth into a separate job using the amazon/aws-cli image.

### CI Variables Required in GitLab Settings

| Variable | Value | Masked | Protected |
|----------|-------|--------|-----------|
| `AWS_ACCESS_KEY_ID` | CI deploy user access key | Yes | Yes |
| `AWS_SECRET_ACCESS_KEY` | CI deploy user secret key | Yes | Yes |
| `EC2_INSTANCE_ID` | Target EC2 instance ID (from CDK output) | No | Yes |

### IAM Policy for CI Deploy User

The CI deploy user needs a minimal IAM policy (NOT the EC2 instance role):

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "ECRPush",
      "Effect": "Allow",
      "Action": [
        "ecr:GetAuthorizationToken",
        "ecr:BatchCheckLayerAvailability",
        "ecr:InitiateLayerUpload",
        "ecr:UploadLayerPart",
        "ecr:CompleteLayerUpload",
        "ecr:PutImage"
      ],
      "Resource": "*"
    },
    {
      "Sid": "SSMDeploy",
      "Effect": "Allow",
      "Action": [
        "ssm:SendCommand",
        "ssm:GetCommandInvocation",
        "ssm:ListCommandInvocations"
      ],
      "Resource": "*"
    }
  ]
}
```

**Note:** `ecr:GetAuthorizationToken` requires `Resource: "*"` (cannot be scoped to a specific repo). The SSM actions could be scoped to the specific instance ARN and document ARN for tighter security.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SSH deploy with key in CI | SSM Send-Command (no SSH) | 2023+ standard | No port 22, audit trail, no key management |
| Manual COPY ordering in Dockerfile | cargo-chef recipe-based caching | cargo-chef stable since 2022 | 5-10x faster source-only rebuilds |
| `pip install awscli` on Alpine | `apk add aws-cli` or amazon/aws-cli image | Alpine 3.18+ includes aws-cli | Simpler, no Python dependency |
| `aws ssm send-command` + manual polling | `aws ssm wait command-executed` | AWS CLI v2 | Built-in waiter, no custom polling script |

## Open Questions

1. **GitHub vs GitLab hosting**
   - What we know: git remote is GitHub (https://github.com/Asbarn/prediction.git), but the project plan explicitly specifies GitLab CI
   - What's unclear: Whether the project will migrate to GitLab, use GitLab as a mirror, or use GitLab CI for external repositories
   - Recommendation: Write the `.gitlab-ci.yml` regardless. If the repo is on GitHub and mirrored to GitLab, the CI file works identically. The user may also push to both remotes.

2. **GitLab shared runners vs self-hosted**
   - What we know: GitLab.com shared runners support DinD with privileged mode. Self-hosted runners need explicit configuration.
   - What's unclear: Whether the user has a GitLab.com account or self-hosted GitLab
   - Recommendation: Target GitLab.com shared runners (most common). The `.gitlab-ci.yml` will work on both. If self-hosted, the runner needs `privileged = true` in config.toml.

3. **`apk add aws-cli` reliability on docker:27 Alpine**
   - What we know: Alpine 3.18+ packages aws-cli. docker:27 is based on Alpine 3.20+.
   - What's unclear: Whether `apk add aws-cli` installs v2 CLI with full SSM support, or a minimal v1
   - Recommendation: Use `apk add --no-cache aws-cli` for the build stage ECR login. Use `amazon/aws-cli:2` image for the deploy stage to guarantee full v2 CLI with `ssm wait` support.

## Sources

### Primary (HIGH confidence)
- Project `.planning/research/STACK.md` -- GitLab CI pipeline structure, CI variables, SSM deploy pattern
- Project `.planning/research/ARCHITECTURE.md` -- 4-stage pipeline design, DinD setup, runner requirements
- Project `.planning/research/PITFALLS.md` -- Rust build caching, ECR auth, security patterns
- Project `infra/cdk/lib/prediction-stack.ts` -- Existing infrastructure (SSM policy, ECR import, systemd service)
- [AWS SSM wait command-executed](https://docs.aws.amazon.com/cli/latest/reference/ssm/wait/command-executed.html) -- built-in CLI waiter for SSM command completion

### Secondary (MEDIUM confidence)
- [cargo-chef GitHub](https://github.com/LukeMathWalker/cargo-chef) -- Recipe-based dependency caching for Rust Docker builds
- [cargo-chef blog post](https://lpalmieri.com/posts/fast-rust-docker-builds/) -- 3-stage Dockerfile pattern with benchmarks
- [Depot.dev optimal Rust Dockerfile](https://depot.dev/docs/container-builds/optimal-dockerfiles/rust-dockerfile) -- cargo-chef + sccache patterns
- [GitLab CI Rust caching](https://vadosware.io/post/even-faster-rust-builds-in-gitlab-ci/) -- CARGO_HOME relocation, cache keys
- [GitLab CI AWS deployment docs](https://docs.gitlab.com/ci/cloud_deployment/) -- Official GitLab AWS deployment patterns

### Tertiary (LOW confidence)
- Alpine aws-cli package availability in docker:27 base -- verified by package listing but not tested hands-on

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- GitLab CI + DinD + ECR is thoroughly documented pattern; existing project research already validated
- Architecture: HIGH -- Pipeline structure follows established project research; SSM deploy with existing infrastructure
- Pitfalls: HIGH -- Well-documented community patterns; specific to this project's Rust dependency tree and deployment architecture
- cargo-chef: MEDIUM -- Standard tool but not yet tested with this specific project's 3-binary layout

**Research date:** 2026-03-08
**Valid until:** 2026-04-08 (stable tools, no fast-moving dependencies)
