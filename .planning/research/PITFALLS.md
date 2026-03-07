# Pitfalls Research

**Domain:** Production deployment infrastructure for existing Rust/Docker arbitrage service on AWS EC2
**Researched:** 2026-03-07
**Confidence:** HIGH (CDK, CloudWatch, Secrets Manager, Docker deployment patterns verified against official AWS docs), MEDIUM (Managed Grafana VPC connectivity -- verified against docs but not hands-on tested), HIGH (GitLab CI Rust caching -- well-documented community patterns)

## Critical Pitfalls

### Pitfall 1: CDK Creates Duplicate Infrastructure Instead of Managing Existing Resources

**What goes wrong:**
The EC2 instance, ECR repository, security groups, and VPC already exist from manual setup. Running `cdk deploy` creates NEW duplicates of everything. You end up with two EC2 instances, two security groups, and confusion about which is "real." If you then try `cdk import` after the fact, the resource properties must match EXACTLY -- missing a single tag, KMS key, or lifecycle policy causes silent drift on the next deploy. High-level L2 constructs like `ec2.Vpc()` create multiple underlying CloudFormation resources; some need importing while others are created, and CDK cannot automatically determine which is which.

**Why it happens:**
CDK assumes greenfield deployment. Developers write CDK stacks describing desired state, deploy, and discover CDK created a second copy of everything alongside the manual infrastructure. The `cdk import` path exists but is fragile: there is no validation that specified properties match the actual resource, so you must run drift detection immediately after import.

**How to avoid:**
For this single-EC2 system, use the clean slate approach:
1. Export the ECR repository name/ARN (to preserve image history) and reference it via `ecr.Repository.fromRepositoryArn()` in CDK
2. Tear down the manually-created EC2 instance, security groups, and other resources
3. Let CDK create everything fresh -- the system tolerates minutes of downtime (all 4 WebSocket feeds reconnect automatically via exponential backoff)
4. Set `RemovalPolicy.RETAIN` on EC2 instance and EBS volumes in CDK to prevent accidental deletion on future stack updates
5. Always commit `cdk.context.json` to git -- VPC lookups return different results on different machines without it

**Warning signs:**
- `cdk diff` shows resources being created that already exist manually
- AWS console shows duplicate security groups or EC2 instances
- CloudFormation stack and manual resources coexist with overlapping purposes

**Phase to address:**
Infrastructure-as-Code phase (first phase). Must be settled before anything depends on CDK-managed resources.

---

### Pitfall 2: CloudWatch Log Costs Exploding from Verbose Structured JSON Logging

**What goes wrong:**
The system emits structured JSON logs via `tracing-subscriber` with JSON output and correlation IDs. CloudWatch charges $0.50/GB for log ingestion on EC2 (flat rate, no Lambda tiered pricing). Four WebSocket feeds receiving order book updates every 100ms produce substantial log volume at INFO level. At DEBUG level, volume increases 10-100x. With default retention ("never expire"), storage costs compound indefinitely. A system that currently costs near-zero for EC2 compute can easily generate $50-200+/month in CloudWatch logging costs alone.

**Why it happens:**
Developers enable verbose logging during development and forget to restrict production levels. Default CloudWatch log group retention is "never expire." The CloudWatch agent ships everything unless explicitly filtered. Additionally, if Data Protection Scanning is enabled (for PII masking), it adds a 24% surcharge to ingestion -- wasteful for a trading system with no PII in logs.

**How to avoid:**
- Set `RUST_LOG=info` for production; use per-module overrides (`RUST_LOG=info,prediction::feed=warn`) to suppress high-frequency feed logging
- Configure CloudWatch log retention to 14 days in CDK (`new logs.LogGroup({ retention: RetentionDays.TWO_WEEKS })`) -- not "never expire"
- Keep the existing Docker `json-file` driver with 50m/3 files as local buffer; ship only structured application logs to CloudWatch, not raw feed recordings
- Do NOT enable Data Protection Scanning unless required
- Set a CloudWatch billing alarm at $10/month threshold via CDK
- Filter at the CloudWatch agent level: exclude log groups for spread_logs and settlement_logs (high volume, low value for centralized logging)

**Warning signs:**
- CloudWatch bill exceeds $10/month for a single-instance service
- Log group storage growing faster than 1GB/day
- Retention policy shows "Never expire" in console
- `spread_logs/` or `settlement_logs/` content appearing in CloudWatch

**Phase to address:**
Logging/monitoring phase. Retention policies and log level configuration must be part of CloudWatch agent setup, not an afterthought.

---

### Pitfall 3: Amazon Managed Grafana Cannot Reach Self-Hosted Prometheus on EC2

**What goes wrong:**
The system runs a Prometheus metrics exporter on port 9000 inside Docker on EC2. Amazon Managed Grafana runs in AWS-managed infrastructure OUTSIDE your VPC. By default, Managed Grafana cannot reach a Prometheus endpoint inside your VPC. The Grafana workspace deploys, dashboards are configured, but the Prometheus data source shows "connection refused" or times out. Hours are wasted debugging what appears to be a configuration error but is actually a network topology problem.

**Why it happens:**
Developers assume Managed Grafana works like self-hosted Grafana where you point it at `http://localhost:9090`. Managed Grafana lives outside your VPC. There are two paths, and picking the wrong one wastes significant time:
1. **VPC connection from Managed Grafana** -- Grafana creates an ENI in your VPC (correct for single-instance setup)
2. **Amazon Managed Prometheus (AMP) as intermediary** -- remote_write to AMP, Grafana reads from AMP (adds unnecessary managed service and cost)

**How to avoid:**
Use the VPC connection approach (not AMP):
- Configure Managed Grafana workspace with VPC connection via `CfnWorkspace` with `vpcConfiguration` in CDK
- The VPC connection creates an ENI in your VPC's subnet
- Security group on EC2 must allow inbound on port 9000 from the Managed Grafana ENI security group
- Prometheus data source URL in Grafana uses the EC2 **private IP** (not public IP, not localhost)
- Note: you can connect only ONE Managed Grafana workspace to ONE VPC endpoint per region per account

Do NOT use Amazon Managed Prometheus unless you need multi-region or multi-account metric aggregation. For a single EC2 instance, it adds $0.03/GB ingestion + $0.003/1000 queries for zero benefit.

**Warning signs:**
- Grafana data source health check shows red/error despite Prometheus being accessible from the EC2 instance itself
- Prometheus queries return "no data" in all dashboards
- Security group has no inbound rule for the Grafana ENI's security group
- Grafana workspace has no VPC connection configured

**Phase to address:**
Monitoring/Grafana phase. VPC connectivity must be configured and verified before any dashboard creation work begins.

---

### Pitfall 4: WebSocket Connections Drop During Deployment with No Graceful Shutdown

**What goes wrong:**
Running `docker compose pull && docker compose up -d` kills the running container (SIGTERM, then SIGKILL after 10s default). All 4 WebSocket connections drop simultaneously. The new container starts, but the system misses arbitrage signals during the gap and loses all in-memory state: order books, spread calculations, active paper trade tracking. Worse, if the Rust binary does not handle SIGTERM, Docker escalates to SIGKILL after 10 seconds, potentially corrupting the state checkpoint mid-write.

**Why it happens:**
Docker Compose's default `stop_grace_period` is 10 seconds. The system needs to: close 4 WebSocket connections, flush pending TOML writes, checkpoint state to disk, and complete any in-flight settlement HTTP requests. With `docker compose up -d` on the same service, Docker performs stop-then-start (not start-then-stop) -- there is no overlap period for connection draining.

**How to avoid:**
- Increase `stop_grace_period` to 30s in docker-compose.yml
- Verify the Rust binary handles SIGTERM (not just SIGINT) -- Docker sends SIGTERM; tokio's `signal::ctrl_c()` only catches SIGINT on Unix. Use `tokio::signal::unix::signal(SignalKind::terminate())` for SIGTERM
- The SIGTERM handler should: (a) stop accepting new work, (b) flush state checkpoint atomically, (c) close WebSocket connections, (d) exit cleanly
- Accept brief downtime (seconds) during deploys -- this is a paper trading system, not high-frequency trading. The reconnection supervisors with exponential backoff handle restarts correctly
- Do NOT over-engineer blue-green for a single-instance Docker Compose service -- the complexity is not justified at this scale

**Warning signs:**
- `state/checkpoint.json` has stale timestamps after deploy (checkpoint was not flushed)
- Paper trade positions reset to zero after restart
- All 4 venue reconnections happen simultaneously, hitting exchange rate limits
- Container logs show `SIGKILL` instead of clean shutdown messages

**Phase to address:**
Deployment automation phase. Graceful shutdown must be verified before CI/CD automates deployments.

---

### Pitfall 5: CDK Bootstrap Missing or Corrupted

**What goes wrong:**
`cdk deploy` fails with cryptic errors: "This stack uses assets, so the toolkit stack must be deployed" or "Access Denied" on the S3 staging bucket. If a previous bootstrap was interrupted, the CloudFormation stack gets stuck in `REVIEW_IN_PROGRESS` state, and re-running `cdk bootstrap` hangs indefinitely.

**Why it happens:**
Bootstrap is a one-time setup step that tutorials mention once and never revisit. It creates an S3 bucket, ECR repository, and IAM roles for CDK deployments. Problems arise when: (a) bootstrap was never run for us-east-1, (b) the CDKToolkit stack was manually modified in CloudFormation console, (c) IAM permissions changed after bootstrap, (d) CDK CLI version in GitLab CI differs from the version used to bootstrap (version mismatch causes subtle failures).

**How to avoid:**
- Run `cdk bootstrap aws://ACCOUNT_ID/us-east-1` explicitly as the documented first step
- Use `--trust` flag if the GitLab CI runner uses a different IAM role than the bootstrapping user
- Pin CDK CLI version in GitLab CI (`npm install -g aws-cdk@2.x.y`) to match the version used locally
- If the CDKToolkit stack is stuck, manually delete it from CloudFormation console and re-bootstrap
- Add a CI pipeline check that verifies CDKToolkit stack is in a healthy state before deploy

**Warning signs:**
- `cdk deploy` fails with S3 or ECR permission errors
- CDKToolkit stack in CloudFormation shows anything other than `CREATE_COMPLETE` or `UPDATE_COMPLETE`
- CDK CLI version in CI differs from local development version (check with `cdk --version`)

**Phase to address:**
Infrastructure-as-Code phase (first phase). Bootstrap before any stack deployment.

---

### Pitfall 6: Secrets Not Available at Container Startup (EC2 + Docker Compose != ECS)

**What goes wrong:**
Unlike ECS (which natively injects Secrets Manager values as environment variables via task definitions), Docker Compose on EC2 has NO built-in integration with AWS Secrets Manager. The container starts, tries to read API keys from environment variables or the secrets volume, finds them empty, and either crashes or runs degraded (currently Kalshi is skipped when `KALSHI_API_KEY_ID` is absent). The deploy "succeeds" but the system operates without critical venue connections.

**Why it happens:**
AWS documentation focuses on ECS/Fargate secrets injection. Developers assume Docker Compose on EC2 has similar native support. It does not. On EC2, you must build your own secrets-fetching mechanism: a deploy script that calls the Secrets Manager API and writes values to a file before starting containers.

**How to avoid:**
Use a deploy script pattern (keep the Rust binary unaware of AWS SDK):
1. Deploy script runs on EC2 (triggered by SSM Run Command from CI, not SSH)
2. Script uses the EC2 instance's IAM role to call `aws secretsmanager get-secret-value`
3. Script writes values to `/app/secrets/.env` (the existing `./secrets:/app/secrets:ro` volume mount already supports this)
4. Script runs `docker compose up -d`
5. The Rust binary reads secrets from environment variables or the mounted secrets directory -- no AWS SDK dependency added to application code

For cost optimization: use SSM Parameter Store (free for standard parameters, up to 10,000) for non-rotating secrets like API keys. Reserve Secrets Manager ($0.40/secret/month) only if you need automatic rotation.

**Warning signs:**
- Container logs show "KALSHI_API_KEY_ID not set, skipping Kalshi feed" in production
- The secrets directory is empty or contains placeholder values after deploy
- EC2 instance profile is missing `secretsmanager:GetSecretValue` permission

**Phase to address:**
Secrets management phase. Must be solved before CI/CD can deploy automatically without human intervention.

---

### Pitfall 7: GitLab CI Rust Builds Taking 20-40 Minutes per Pipeline

**What goes wrong:**
A clean Rust build of 39K+ LOC with the project's dependency tree (tokio, reqwest, serde, axum, statrs, rsa, etc.) takes 15-30+ minutes. In GitLab CI without proper caching, every pipeline is a clean build because runners are ephemeral. With 3 binaries (`prediction`, `spread-analytics`, `signal-scoring`), each push triggers a build developers wait 30+ minutes for. CI becomes the development bottleneck.

**Why it happens:**
GitLab CI runners discard state between jobs. The current Dockerfile does a clean `cargo build --release` with no dependency caching layer. Docker layer caching helps only if `Cargo.toml`/`Cargo.lock` haven't changed, but any dependency bump invalidates the entire build layer. Without explicit caching strategy, every CI run downloads all crates and recompiles everything from scratch.

**How to avoid:**
Layer the solution for cumulative speedup:
1. **cargo-chef in Dockerfile:** Split into `prepare` (generate dependency recipe), `cook` (build dependencies only), `build` (compile source). The dependency layer is cached unless Cargo.toml changes. This alone cuts 15+ minutes off source-only changes.
2. **sccache with S3 backend:** Cache individual compilation artifacts in S3 (`RUSTC_WRAPPER=sccache`, `SCCACHE_BUCKET=prediction-ci-cache`). Even when one dependency changes, all unchanged crates are served from cache.
3. **GitLab CI cache:** Cache `~/.cargo/registry` and `~/.cargo/git` directories between pipeline runs using branch-name cache keys.
4. **Build natively on Linux CI, not cross-compile:** The Dockerfile already builds for Linux in Docker. The CI runner just needs Docker -- no cross-compilation toolchain required. Do NOT try to build on a Windows runner and cross-compile.

Expected improvement: 20-30 min clean build drops to 3-5 min for source-only changes with warm cache.

**Warning signs:**
- CI pipeline exceeds 10 minutes for source-only changes (no Cargo.toml modifications)
- Docker build output shows "Downloading crates" on every run
- The `cook` step (dependency compilation) runs on every push, not just on Cargo.toml changes
- sccache hit rate below 50% (check with `sccache --show-stats`)

**Phase to address:**
CI/CD pipeline phase. Build optimization is integral to pipeline setup, not a post-hoc improvement.

---

### Pitfall 8: EC2 Instance Replaced by CDK Update -- All Data Volumes Lost

**What goes wrong:**
A CDK stack update that changes certain EC2 properties (instance type, AMI, subnet, or sometimes even user data) triggers CloudFormation to REPLACE the EC2 instance rather than update it in-place. Replacement means: old instance terminated, new instance launched. All data on the old instance's EBS volumes is lost unless volumes are configured to persist. For this system, that means: `config/events.toml` (approved instrument mappings), `state/checkpoint.json` (paper trade state), `spread_logs/`, `settlement_logs/`, `paper_trades/` -- all gone.

**Why it happens:**
CloudFormation's update behavior varies by property. Some EC2 properties support in-place updates; others require replacement. The CDK documentation does not make this obvious at the construct level. Developers run `cdk deploy` expecting an update and get a replacement. Without `RemovalPolicy.RETAIN` on volumes and without checking `cdk diff` first, the damage is done before anyone notices.

**How to avoid:**
- ALWAYS run `cdk diff` before `cdk deploy` and check for "replace" in the output. Make this a CI pipeline gate.
- Set `RemovalPolicy.RETAIN` on the EC2 instance and all EBS volumes
- Use a separate EBS volume for data (`/app/config`, `/app/state`, `/app/spread_logs`, etc.), not the root volume. Mount it explicitly.
- Back up `events.toml` and `checkpoint.json` to S3 on a schedule (cron or systemd timer) -- these are the hardest to recreate
- Consider using EFS for the data directory if you ever need instance flexibility, but for a single-instance system, a persistent EBS volume is simpler

**Warning signs:**
- `cdk diff` output contains the word "replace" for the EC2 instance resource
- After deploy, the EC2 instance has a different instance ID
- Data directories are empty after a "routine" deploy
- events.toml has no approved instruments after deploy

**Phase to address:**
Infrastructure-as-Code phase (first phase). Data persistence strategy must be defined before the EC2 instance is managed by CDK.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| SSH deploy instead of SSM Run Command | Works today, zero IAM setup | No audit trail, requires SSH key management, port 22 open in security group | Only during initial CDK setup; replace with SSM before CI/CD is complete |
| Hardcoded EC2 instance ID in deploy scripts | Quick to get working | Instance replacement (intentional or CDK-triggered) requires script updates | Never -- use CDK outputs or SSM parameter to store instance ID dynamically |
| All logs to CloudWatch at DEBUG level | Maximum visibility during initial deployment | $50-200/month log costs, noise drowns real alerts | First 24 hours of production deploy only, then restrict to INFO |
| Single Secrets Manager secret with all API keys as JSON | One API call, $0.40/month total | Rotation rotates everything; blast radius is all venues simultaneously | Acceptable at this scale (4 API keys, no rotation needed for exchange keys) |
| Skipping `cdk diff` before `cdk deploy` in CI | Faster pipeline, fewer steps | Unintended EC2 instance replacement = data loss + downtime | Never |
| Not committing `cdk.context.json` to git | Cleaner repo, fewer merge conflicts | VPC lookups return different results on different machines; deploys are non-deterministic | Never -- always commit this file |

## Integration Gotchas

Common mistakes when connecting to external services.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Managed Grafana + Prometheus | Assuming Grafana can reach EC2 Prometheus without VPC config | Configure VPC connection on Managed Grafana workspace; add security group inbound rule from Grafana ENI SG to EC2 SG on port 9000 |
| CloudWatch Agent + Docker JSON logs | Shipping raw Docker `json-file` logs (double-JSON-encoded: Docker wraps the structured log in another JSON object) | Configure CloudWatch agent to parse the Docker JSON wrapper and extract the inner structured message; or use `awslogs` Docker logging driver instead of `json-file` |
| Secrets Manager + Docker Compose on EC2 | Expecting ECS-style native env var injection | Deploy script fetches secrets via CLI and writes `.env` file before `docker compose up` |
| CDK + existing ECR repository | CDK creates a new ECR repo; old images with known-working tags are orphaned in the original repo | Reference existing ECR by ARN with `ecr.Repository.fromRepositoryArn()`; do not let CDK create a new one |
| GitLab CI + ECR authentication | Hardcoding ECR login credentials or caching a token (tokens expire every 12 hours) | Use `aws ecr get-login-password` in every CI job; authenticate via IAM role, not long-lived credentials |
| CDK + EC2 UserData | UserData script runs only on FIRST launch, not on stack updates | Use SSM Run Command for post-deploy actions (pulling images, restarting containers); UserData is only for initial instance provisioning |
| Managed Grafana + IAM | Using service-managed permissions from the AWS management account | Use customer-managed IAM permissions from a member account (AWS best practice); create the workspace in a member account |
| CDK + EC2 property changes | Changing instance type or AMI in CDK and deploying without checking diff | Always run `cdk diff` before `cdk deploy`; some property changes trigger instance REPLACEMENT, not in-place update |

## Performance Traps

Patterns that work at small scale but fail as usage grows.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| CloudWatch Logs Insights on unstructured or inconsistently structured logs | Queries take 30s+, return partial results, fields not extractable | Ensure all logs use consistent JSON schema via `tracing-subscriber` JSON layer; create CloudWatch metric filters for frequent queries | >1GB/day log volume |
| All 4 venue WebSocket reconnections simultaneously after deploy | Rate limit errors from exchanges (especially Deribit at 20 req/s), delayed recovery, missed signals during reconnection storm | Stagger reconnections with per-venue jitter (already configured: `randomization_factor = 0.5` in backoff config); verify this works correctly during deploy | Every deploy |
| CDK asset accumulation in bootstrap S3 bucket | Slowly growing S3 costs; after many deploys, old Docker images and Lambda code pile up | Set S3 lifecycle policy on CDKToolkit bucket to expire objects after 30 days; set ECR lifecycle policy to retain only last 10 images | After ~50 deployments |
| CloudWatch PutLogEvents throttling | Log delivery delays, gaps in CloudWatch logs | Batch log events; use CloudWatch agent (not direct API calls) which handles batching and retry automatically | >5 PutLogEvents calls/second per log stream |

## Security Mistakes

Domain-specific security issues beyond general web security.

| Mistake | Risk | Prevention |
|---------|------|------------|
| EC2 instance profile with wildcard (`*`) permissions | Any container compromise gives full AWS account access; exchange API keys extractable from Secrets Manager | Least-privilege IAM policy: only `secretsmanager:GetSecretValue` (scoped to specific secret ARNs), `ecr:GetAuthorizationToken`, `ecr:BatchGetImage`, `logs:PutLogEvents`, `logs:CreateLogStream` |
| Prometheus metrics endpoint (port 9000) open to internet | Exposes internal system state: order book counts, spread values, signal rates, reconnection patterns -- enough to infer trading activity | Security group allows port 9000 inbound ONLY from Managed Grafana ENI security group; no public access |
| Health endpoint (port 9001) open to internet | Information disclosure, potential DoS vector | Security group allows port 9001 only from within VPC; use Managed Grafana or CloudWatch for external health monitoring |
| API keys in docker-compose.yml `environment` block | Keys visible in `docker inspect`, in version control, in CI logs | Use the existing secrets volume mount (`./secrets:/app/secrets:ro`); never put secrets in environment block or docker-compose.yml |
| SSH key for EC2 stored in GitLab CI variables | Key compromise gives direct shell access to production instance | Use SSM Session Manager instead of SSH; no key management, full audit trail, no port 22 needed |
| CDK bootstrap with default trust policy | Any IAM principal in the account can deploy via CDK, including compromised CI runners for other projects | Scope bootstrap trust to specific CI/CD role ARN using `--trust` and `--cloudformation-execution-policies` |

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **CDK Stack:** Deploys successfully but manual resources still exist alongside -- verify no duplicate security groups, EC2 instances, or ECR repos
- [ ] **CDK Stack:** `cdk diff` shows clean but drift has occurred -- run `cdk drift` after first deploy and after any console changes
- [ ] **CloudWatch Logs:** Agent installed but retention policy not set -- check every log group shows 14 days, not "Never expire"
- [ ] **CloudWatch Logs:** Logs appearing but double-JSON-encoded -- verify structured fields are queryable in Logs Insights (`@message` contains parseable JSON, not escaped JSON string)
- [ ] **Managed Grafana:** Workspace created but data source shows error -- verify VPC connection configured AND security group inbound rule exists for Grafana ENI
- [ ] **Managed Grafana:** Data source green but dashboards show "No data" -- verify Prometheus data source URL uses EC2 private IP (not `localhost`, not public IP)
- [ ] **CI/CD Pipeline:** Builds and pushes image but deploy step uses SSH with hardcoded IP -- verify deploy uses SSM Run Command with instance ID from CDK output
- [ ] **CI/CD Pipeline:** Pipeline passes but builds take 25 minutes -- verify cargo-chef layer caching is working (check Docker build logs for "Using cache" on dependency layer)
- [ ] **Secrets Manager:** Secrets created but container starts without them -- verify deploy script fetches secrets BEFORE `docker compose up`; check container logs for missing env var warnings
- [ ] **Docker restart:** `restart: unless-stopped` is set but container does not survive EC2 reboot -- verify Docker daemon starts on boot (`systemctl enable docker`)
- [ ] **Graceful shutdown:** Container stops without error but checkpoint is stale -- verify SIGTERM handler flushes state; check `checkpoint.json` timestamp matches last shutdown time
- [ ] **ECR lifecycle:** Images pushed successfully but old images never cleaned -- verify ECR lifecycle policy retains only last N images (recommend 10)

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| CDK creates duplicate infrastructure | MEDIUM | Delete CDK CloudFormation stack; clean up duplicates in console; decide import-vs-clean-slate; redeploy |
| CloudWatch log cost spike | LOW | Set retention policy (applies retroactively), reduce `RUST_LOG` level, add CloudWatch agent exclusion filters; costs stabilize within billing cycle |
| Grafana cannot reach Prometheus | LOW | Add VPC connection to Managed Grafana workspace, create/update security group rules; no data loss, just delayed dashboard setup |
| WebSocket connections drop during deploy | LOW | System auto-reconnects via backoff supervisors; order books rebuild from snapshots on all 4 venues; verify `checkpoint.json` was flushed pre-shutdown |
| Secrets not injected at container startup | LOW | SSH/SSM to instance, run deploy script manually, verify `.env` file, restart container; fix CI pipeline |
| CI builds take 30+ minutes | MEDIUM | Add cargo-chef to Dockerfile (requires restructuring multi-stage build), configure sccache S3 bucket, rebuild pipeline cache from scratch |
| CDK bootstrap corrupted | LOW | Delete CDKToolkit CloudFormation stack from console, re-run `cdk bootstrap` with correct trust/policy flags |
| EC2 instance replaced by CDK update, data lost | HIGH | If no backup: events.toml must be manually reconstructed from git history; paper trade state is lost; spread/settlement logs are lost. Prevention (RemovalPolicy.RETAIN + S3 backup) is essential |
| Double-JSON-encoded CloudWatch logs | LOW | Switch Docker logging driver to `awslogs` or configure CloudWatch agent JSON parser; re-query logs after fix |

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| CDK duplicate infrastructure | IaC/CDK setup (Phase 1) | `cdk diff` shows no unexpected creates/deletes; no duplicate resources in AWS console; `cdk drift` returns clean |
| CDK bootstrap issues | IaC/CDK setup (Phase 1) | CDKToolkit stack in `CREATE_COMPLETE` or `UPDATE_COMPLETE`; `cdk deploy` succeeds on first try |
| EC2 instance replacement risk | IaC/CDK setup (Phase 1) | EC2 has `RemovalPolicy.RETAIN`; `cdk diff` reviewed before every deploy; data volume is separate EBS |
| CloudWatch log costs | Logging setup phase | Log group retention set to 14 days; billing alert at $10/month configured; `RUST_LOG=info` in production compose |
| Double-JSON-encoded logs | Logging setup phase | CloudWatch Logs Insights query `fields @timestamp, level, message` returns parsed fields, not escaped JSON |
| Grafana VPC connectivity | Monitoring/Grafana phase | Prometheus data source health check shows green; a test query returns metric data |
| WebSocket deploy disruption | Deployment automation phase | Deploy script runs; all 4 feeds reconnect within 60s (verify via `feed_reconnections_total` Prometheus counter) |
| Graceful shutdown / SIGTERM | Deployment automation phase | `docker compose stop` shows clean shutdown in logs; `checkpoint.json` timestamp updates on shutdown |
| Secrets injection on EC2 | Secrets management phase | Container starts with all expected env vars populated; all configured venue feeds connect |
| CI build time | CI/CD pipeline phase | Pipeline completes in <10 minutes for source-only changes; cargo-chef cache hits confirmed in build log |
| CDK diff gate in CI | CI/CD pipeline phase | Pipeline includes `cdk diff` step that outputs changes for review before deploy; no auto-deploy on "replace" |

## Sources

- [AWS CDK Troubleshooting](https://docs.aws.amazon.com/cdk/v2/guide/troubleshooting.html)
- [AWS CDK Bootstrap Guide](https://docs.aws.amazon.com/cdk/v2/guide/bootstrapping.html)
- [CDK Bootstrap Troubleshooting](https://docs.aws.amazon.com/cdk/v2/guide/bootstrapping-troubleshoot.html)
- [CDK Best Practices 2026 -- Towards The Cloud](https://towardsthecloud.com/blog/aws-cdk-best-practices)
- [CDK Drift Detection](https://docs.aws.amazon.com/cdk/v2/guide/ref-cli-cmd-drift.html)
- [CDK Common Mistakes -- TurboGeek](https://www.turbogeek.co.uk/aws-cdk-common-mistakes/)
- [Importing Existing Resources into CDK -- AWS DevOps Blog](https://aws.amazon.com/blogs/devops/how-to-import-existing-resources-into-aws-cdk-stacks/)
- [CloudWatch Logs Pricing -- Hykell](https://hykell.com/knowledge-base/aws-cloudwatch-logs-pricing/)
- [CloudWatch Cost Optimization -- AWS Docs](https://docs.aws.amazon.com/AmazonCloudWatch/latest/monitoring/cloudwatch_billing.html)
- [Amazon Managed Grafana VPC Configuration](https://docs.aws.amazon.com/grafana/latest/userguide/AMG-configure-vpc.html)
- [Managed Grafana Interface VPC Endpoints](https://docs.aws.amazon.com/grafana/latest/userguide/VPC-endpoints.html)
- [Managed Grafana Prometheus Data Sources](https://docs.aws.amazon.com/grafana/latest/userguide/prometheus-data-source.html)
- [Secrets Manager Pricing](https://aws.amazon.com/secrets-manager/pricing/)
- [SSM Parameter Store vs Secrets Manager -- cloudonaut](https://cloudonaut.io/managing-application-secrets-ssm-parameter-store-vs-secrets-manager/)
- [cargo-chef for Docker Layer Caching](https://github.com/LukeMathWalker/cargo-chef)
- [Optimal Rust Dockerfiles -- Depot](https://depot.dev/blog/rust-dockerfile-best-practices)
- [sccache for Rust Build Caching -- Earthly](https://earthly.dev/blog/rust-sccache/)
- [Docker Compose Graceful Shutdown -- vsupalov](https://vsupalov.com/docker-compose-stop-slow/)
- [Zero-Downtime Blue-Green with Docker Compose on EC2](https://abdullahob.medium.com/zero-downtime-deployments-implementing-blue-green-with-docker-compose-on-aws-ec2-79cad234c65e)
- [Zero-Downtime WebSocket Deployments](https://github.com/crummy/zero-downtime-websockets/blob/main/README.md)
- [GitLab CI Rust Build Caching](https://vadosware.io/post/even-faster-rust-builds-in-gitlab-ci/)

---
*Pitfalls research for: Production deployment infrastructure (CDK, CI/CD, Managed Grafana, CloudWatch, Secrets Manager, EC2 deployment) for Rust arbitrage system*
*Researched: 2026-03-07*
