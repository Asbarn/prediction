# Phase 36: CloudWatch Logging - Research

**Researched:** 2026-03-07
**Domain:** Docker awslogs driver, CloudWatch Logs, CloudWatch Agent host metrics, tracing-subscriber stdout format
**Confidence:** HIGH

## Summary

Phase 36 connects two existing systems: the Rust application's structured logging (via `tracing-subscriber`) and the CloudWatch log group already provisioned by CDK (Phase 34). The primary change is swapping Docker Compose's `json-file` logging driver to `awslogs`, which ships container stdout directly to CloudWatch Logs with zero additional agents or containers.

A critical discovery: the application's stdout layer currently outputs **human-readable** format (not JSON). The file layer uses `.json()` but writes to local disk. To meet the success criteria of "structured JSON log lines in CloudWatch queryable by level, target, correlation_id," the stdout layer must be changed to JSON format. This is a small Rust code change (~2 lines in `src/logging/layers.rs`).

The CloudWatch Agent for host metrics (MON-09) is **already installed and configured** in the user-data from Phase 35, but the IAM instance role is missing the `CloudWatchAgentServerPolicy` managed policy needed for the agent to publish metrics. This is a one-line CDK addition.

**Primary recommendation:** Three changes -- (1) add `.json()` to stdout layer in Rust, (2) swap docker-compose logging driver to `awslogs` with non-blocking mode, (3) add `CloudWatchAgentServerPolicy` to IAM role.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MON-01 | Docker Compose uses awslogs driver to ship structured JSON logs to CloudWatch | Swap logging driver from `json-file` to `awslogs` in both local docker-compose.yml and user-data embedded compose. Change stdout tracing layer to JSON format. IAM permissions already granted via `logGroup.grantWrite()`. |
| MON-09 | CloudWatch agent reports EC2 host metrics (CPU, memory, disk) | Agent already installed and configured in Phase 35 user-data. Only missing piece is `CloudWatchAgentServerPolicy` IAM managed policy on instance role. |
</phase_requirements>

## Standard Stack

### Core
| Component | Version | Purpose | Why Standard |
|-----------|---------|---------|--------------|
| Docker awslogs driver | Built into Docker Engine | Ship container stdout to CloudWatch Logs | Zero-install, Docker-native CloudWatch integration |
| CloudWatch Logs | AWS managed | Log storage and querying | Already provisioned (INFRA-06), 14-day retention |
| CloudWatch Logs Insights | AWS managed | Structured log querying | Native JSON field extraction for tracing-subscriber output |
| CloudWatch Agent | amazon-cloudwatch-agent (AL2023) | Host metrics (CPU, mem, disk) | Already installed in Phase 35 user-data |

### Supporting
| Component | Version | Purpose | When to Use |
|-----------|---------|---------|-------------|
| tracing-subscriber | 0.3.x | Rust logging framework with JSON output | Already in use; stdout layer needs `.json()` added |
| tracing-appender | 0.2.x | Non-blocking file writer for debug logs | Remains unchanged; local debug logs complement CloudWatch |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| awslogs driver | Fluent Bit sidecar | Overkill for single container; adds a process to manage |
| awslogs driver | CloudWatch Agent log collection | Agent is for file-based logs; awslogs is purpose-built for Docker stdout |
| Changing stdout to JSON | Shipping file-layer JSON via CW Agent | Double indirection; awslogs on stdout is simpler and direct |

## Architecture Patterns

### What Changes

```
src/logging/layers.rs           # Add .json() to stdout layer
docker-compose.yml              # Swap json-file -> awslogs driver (local copy)
infra/cdk/lib/prediction-stack.ts  # (1) Add CloudWatchAgentServerPolicy
                                   # (2) Update embedded docker-compose in user-data
```

### Pattern 1: awslogs Driver Configuration

**What:** Docker's built-in CloudWatch log driver ships stdout/stderr to a CloudWatch log group.
**When to use:** Any Docker container on EC2 that needs remote log access.

```yaml
# docker-compose.yml logging block
logging:
  driver: awslogs
  options:
    awslogs-region: us-east-1
    awslogs-group: /prediction/production
    awslogs-stream-prefix: prediction
    mode: non-blocking
    max-buffer-size: 4m
```

Key options:
- `awslogs-group`: Must match the CDK-provisioned log group name (`/prediction/production`)
- `awslogs-stream-prefix`: Creates streams like `prediction/<container_name>/<container_id>`
- `mode: non-blocking`: Prevents application blocking if CloudWatch is temporarily unreachable
- `max-buffer-size: 4m`: In-memory buffer for non-blocking mode (default 1m is too small for burst logging)
- Do NOT use `awslogs-create-group: "true"` -- the log group already exists via CDK

### Pattern 2: Structured JSON Stdout for CloudWatch Querying

**What:** Change the tracing-subscriber stdout layer from human-readable to JSON format.
**Why:** The awslogs driver ships raw stdout lines. CloudWatch Logs Insights can parse JSON fields automatically. Human-readable output is not queryable by field.

```rust
// BEFORE (human-readable):
let stdout_layer = fmt::layer()
    .with_target(true)
    .with_level(true)
    .with_filter(stdout_filter);

// AFTER (structured JSON):
let stdout_layer = fmt::layer()
    .json()
    .with_target(true)
    .with_level(true)
    .with_filter(stdout_filter);
```

This produces lines like:
```json
{"timestamp":"2026-03-07T12:00:00Z","level":"INFO","target":"prediction::feed::health","fields":{"venue":"deribit","status":"connected"},"spans":[]}
```

CloudWatch Logs Insights can then query:
```sql
fields @timestamp, level, target, fields.venue
| filter level = "ERROR"
| sort @timestamp desc
```

### Pattern 3: CloudWatch Agent IAM Policy

**What:** Add `CloudWatchAgentServerPolicy` managed policy to the instance role.
**Why:** The agent needs `cloudwatch:PutMetricData` to publish custom metrics. The existing `logGroup.grantWrite()` only covers CloudWatch Logs, not CloudWatch Metrics.

```typescript
instanceRole.addManagedPolicy(
  iam.ManagedPolicy.fromAwsManagedPolicyName('CloudWatchAgentServerPolicy')
);
```

### Anti-Patterns to Avoid

- **Using `awslogs-create-group: "true"` when group exists via CDK:** Creates a log group without the CDK retention policy. The CDK-managed group already has 14-day retention.
- **Blocking mode (default):** If CloudWatch API is slow or rate-limited, the application process blocks on every log write. Always use `mode: non-blocking` for production.
- **Leaving stdout as human-readable:** Makes CloudWatch Logs Insights queries impossible. Fields like `level`, `target` are not extractable from unstructured text.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Log shipping to CloudWatch | Custom log shipper or CloudWatch SDK calls | Docker awslogs driver | Built into Docker, zero code, battle-tested |
| JSON log formatting | Custom serialization in log macros | `tracing-subscriber` `.json()` layer | Handles all structured fields, spans, timestamps automatically |
| Host metrics collection | Custom scripts writing to CloudWatch API | CloudWatch Agent | Already installed; handles batching, retry, credential rotation |
| Log rotation in CloudWatch | Custom lifecycle scripts | CloudWatch log group retention policy | Already set to 14 days in CDK |

## Common Pitfalls

### Pitfall 1: `docker logs` and `docker compose logs` Stop Working
**What goes wrong:** After switching to awslogs driver, `docker logs <container>` returns "no logs are available with the 'awslogs' log driver."
**Why it happens:** The awslogs driver sends logs directly to CloudWatch; they are not stored locally. Docker's local log commands only work with `json-file` and `journald` drivers.
**How to avoid:** This is expected behavior, not a bug. Use `aws logs tail /prediction/production --follow` for live log tailing, or CloudWatch Logs Insights for queries.
**Warning signs:** Operators trying to debug with `docker logs` and seeing no output.

### Pitfall 2: Double JSON Encoding
**What goes wrong:** If the stdout layer outputs JSON and Docker's awslogs driver wraps it in another JSON layer, CloudWatch receives `{"log":"{\"timestamp\":...}","stream":"stdout"}`.
**Why it happens:** This is a json-file driver problem, not an awslogs driver problem. The awslogs driver ships raw lines without wrapping.
**How to avoid:** The awslogs driver does NOT double-encode. Each stdout line becomes one CloudWatch log event verbatim. No action needed -- just verify after deployment.

### Pitfall 3: Missing IAM Permissions for CloudWatch Agent Metrics
**What goes wrong:** CloudWatch Agent starts but silently fails to publish metrics. No error in agent log unless you check `/opt/aws/amazon-cloudwatch-agent/logs/amazon-cloudwatch-agent.log`.
**Why it happens:** `logGroup.grantWrite()` only grants CloudWatch Logs permissions. The CloudWatch Agent needs `cloudwatch:PutMetricData` for metrics, which is in `CloudWatchAgentServerPolicy` but not in the logs-only grant.
**How to avoid:** Add `CloudWatchAgentServerPolicy` managed policy to the instance role.
**Warning signs:** Metrics namespace `Prediction/EC2` not appearing in CloudWatch Metrics console.

### Pitfall 4: Container Fails to Start Due to awslogs Driver Error
**What goes wrong:** `docker compose up` fails with "failed to initialize logging driver: failed to create CloudWatch log stream."
**Why it happens:** IAM credentials not available to Docker daemon (instance profile not attached, or STS token expired).
**How to avoid:** The Docker daemon uses the EC2 instance metadata service for credentials (same as AWS CLI). Ensure instance profile is attached (it is, via CDK). The awslogs driver retries automatically on transient failures.
**Warning signs:** Container in `Created` state but never transitions to `Running`.

### Pitfall 5: Non-Blocking Mode Log Loss
**What goes wrong:** Under extreme burst logging, the in-memory buffer fills and logs are silently dropped.
**Why it happens:** `max-buffer-size` defaults to 1MB. If the application emits logs faster than they can be shipped to CloudWatch, the buffer overflows.
**How to avoid:** Set `max-buffer-size: 4m` (4MB). For this application's volume (~50-100 lines/minute during normal operation), 4MB provides minutes of buffer.
**Warning signs:** Gaps in CloudWatch log timeline during high-activity periods.

## Code Examples

### docker-compose.yml (Production)
```yaml
services:
  prediction:
    image: 606103597377.dkr.ecr.us-east-1.amazonaws.com/prediction:latest
    env_file: .env
    stop_grace_period: 30s
    ports:
      - "9000:9000"
      - "9001:9001"
    volumes:
      - /opt/prediction/data/config:/app/config
      - /opt/prediction/data/spread_logs:/app/spread_logs
      - /opt/prediction/data/settlement_logs:/app/settlement_logs
      - /opt/prediction/data/paper_trades:/app/paper_trades
      - /opt/prediction/data/state:/app/state
      - /opt/prediction/data/logs:/app/logs
    logging:
      driver: awslogs
      options:
        awslogs-region: us-east-1
        awslogs-group: /prediction/production
        awslogs-stream-prefix: prediction
        mode: non-blocking
        max-buffer-size: 4m
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9001/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 15s
    restart: "no"
```

### src/logging/layers.rs (Modified Stdout Layer)
```rust
// Stdout layer: structured JSON for CloudWatch Logs Insights querying
let stdout_layer = fmt::layer()
    .json()
    .with_target(true)
    .with_level(true)
    .with_filter(stdout_filter);
```

### CDK IAM Addition
```typescript
// CloudWatch Agent metrics publishing (MON-09)
instanceRole.addManagedPolicy(
  iam.ManagedPolicy.fromAwsManagedPolicyName('CloudWatchAgentServerPolicy')
);
```

### CloudWatch Logs Insights Verification Queries
```sql
-- Verify structured fields are queryable (success criteria #2)
fields @timestamp, level, target
| filter level = "INFO"
| sort @timestamp desc
| limit 10

-- Filter by target module
fields @timestamp, level, target, @message
| filter target like /prediction::feed/
| sort @timestamp desc
| limit 20

-- Error investigation
fields @timestamp, level, target, @message
| filter level = "ERROR" or level = "WARN"
| sort @timestamp desc
| limit 50
```

### CloudWatch Agent Metrics Verification
```bash
# Verify host metrics appear (success criteria #3)
aws cloudwatch list-metrics --namespace "Prediction/EC2" --region us-east-1

# Check specific metric
aws cloudwatch get-metric-statistics \
  --namespace "Prediction/EC2" \
  --metric-name "mem_used_percent" \
  --start-time "$(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S)" \
  --end-time "$(date -u +%Y-%m-%dT%H:%M:%S)" \
  --period 300 \
  --statistics Average \
  --region us-east-1
```

### Live Log Tailing (Replaces `docker logs`)
```bash
# Via AWS CLI
aws logs tail /prediction/production --follow --region us-east-1

# Via SSM on the instance
aws ssm start-session --target <instance-id>
# Then: journalctl -u prediction -f  (for systemd-level output)
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| CloudWatch Agent for Docker logs | awslogs Docker driver | Docker 1.9+ (2015) | Zero-agent log shipping; agent reserved for host metrics |
| Blocking awslogs mode (default) | Non-blocking mode | Docker 18.07 (2018) | Prevents application hangs from CloudWatch API latency |
| ECS default blocking mode | ECS default non-blocking mode | June 2025 | Industry shift toward availability over completeness |

## Existing Infrastructure Summary

Already provisioned (no changes needed):
- CloudWatch log group `/prediction/production` with 14-day retention (CDK Phase 34)
- IAM `logs:CreateLogStream` and `logs:PutLogEvents` permissions (CDK `logGroup.grantWrite()`)
- CloudWatch Agent installed with CPU/mem/disk config (user-data Phase 35)
- Systemd service for container lifecycle (Phase 35)

Changes needed:
1. **Rust code:** Add `.json()` to stdout tracing layer (~2 lines)
2. **docker-compose.yml:** Swap logging driver to awslogs (both local and user-data copies)
3. **CDK IAM:** Add `CloudWatchAgentServerPolicy` managed policy
4. **CDK deploy** to apply IAM change
5. **Redeploy container** with new image (JSON stdout) and new compose config (awslogs driver)

## Open Questions

1. **Stdout JSON format change affects local development**
   - What we know: Changing stdout to JSON makes local terminal output hard to read
   - What's unclear: Whether to make this conditional (e.g., env var toggle) or always-on
   - Recommendation: Add a `stdout_json` boolean to config. Default true in production, false for local dev. Or use an environment variable like `LOG_FORMAT=json` to toggle. The simplest approach: always JSON in the Docker image, human-readable only when running `cargo run` locally (check if `LOG_FORMAT` env var is set).

2. **Local docker-compose.yml vs user-data embedded compose**
   - What we know: The repo has `docker-compose.yml` (local dev) and user-data embeds a copy for EC2
   - What's unclear: Whether to change both or only the EC2 version
   - Recommendation: Change both. The local copy should match production for testing. Developers can override with `docker compose -f docker-compose.override.yml` if needed. However, `awslogs` will fail locally without AWS credentials -- so the local copy may need to stay as `json-file` or use a compose override.

## Sources

### Primary (HIGH confidence)
- [Docker awslogs driver official docs](https://docs.docker.com/engine/logging/drivers/awslogs/) -- all options, non-blocking mode, IAM permissions
- [CloudWatchAgentServerPolicy reference](https://docs.aws.amazon.com/aws-managed-policy/latest/reference/CloudWatchAgentServerPolicy.html) -- required IAM permissions for CW Agent
- [AWS Create IAM roles for CloudWatch agent](https://docs.aws.amazon.com/AmazonCloudWatch/latest/monitoring/create-iam-roles-for-cloudwatch-agent.html) -- IAM setup guidance
- Existing project code: `infra/cdk/lib/prediction-stack.ts`, `src/logging/layers.rs`, `docker-compose.yml`

### Secondary (MEDIUM confidence)
- [AWS Blog: Non-blocking awslogs mode](https://aws.amazon.com/blogs/containers/preventing-log-loss-with-non-blocking-mode-in-the-awslogs-container-log-driver/) -- non-blocking mode rationale and buffer sizing
- Prior project research: `.planning/research/STACK.md`, `.planning/research/ARCHITECTURE.md`, `.planning/research/PITFALLS.md`

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- awslogs driver, CloudWatch Logs, and CW Agent are all mature, well-documented AWS services
- Architecture: HIGH -- changes are minimal and well-understood; prior project research already validated the approach
- Pitfalls: HIGH -- double-JSON encoding, docker logs unavailability, and IAM gaps are well-documented issues
- Rust code change: HIGH -- `tracing-subscriber` `.json()` is a single method call; verified in existing file layer code

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable technologies, no expected breaking changes)
