---
phase: 36-cloudwatch-logging
verified: 2026-03-07T23:10:00Z
status: human_needed
score: 3/5 must-haves verified
re_verification: false
human_verification:
  - test: "Verify CloudWatch Logs contain structured JSON entries"
    expected: "Log group /prediction/production has recent JSON log entries with timestamp, level, target fields"
    why_human: "Requires AWS Console or CLI access to live CloudWatch service"
  - test: "Verify CloudWatch Logs Insights structured queries work"
    expected: "Query 'fields @timestamp, level, target | filter level = INFO | limit 10' returns structured results"
    why_human: "Requires live AWS service interaction"
  - test: "Verify EC2 host metrics in CloudWatch Metrics"
    expected: "Prediction/EC2 namespace contains cpu_usage_user, mem_used_percent, disk_used_percent metrics"
    why_human: "Requires live AWS service interaction"
  - test: "Verify stdout_json=true is active on production instance"
    expected: "Production config on EC2 has stdout_json=true so container emits JSON, not human-readable text"
    why_human: "Setting was applied via SSM, not codified in repo -- need to verify on running instance"
gaps:
  - truth: "stdout_json=true is codified for production deploys"
    status: partial
    reason: "stdout_json=true was set on the running instance via SSM but is NOT in CDK user-data or any production config in the repo. If instance is recreated, container would emit human-readable logs (default false), breaking CloudWatch JSON ingestion."
    artifacts:
      - path: "infra/cdk/lib/prediction-stack.ts"
        issue: "No user-data step writes stdout_json=true to production config.toml"
    missing:
      - "Add a user-data step in CDK that writes a production config.toml (or overlay) with stdout_json=true to /opt/prediction/data/config/config.toml"
---

# Phase 36: CloudWatch Logging Verification Report

**Phase Goal:** Container logs and host metrics are remotely accessible in CloudWatch without SSH
**Verified:** 2026-03-07T23:10:00Z
**Status:** human_needed (with one gap noted)
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Stdout tracing layer emits structured JSON when stdout_json is true | VERIFIED | `src/logging/layers.rs` lines 51-66: conditional boxed layer with `.json()` call when `stdout_json` is true |
| 2 | Stdout tracing layer emits human-readable format when stdout_json is false | VERIFIED | `src/logging/layers.rs` lines 59-65: else branch uses standard `fmt::layer()` without `.json()` |
| 3 | EC2 docker-compose uses awslogs driver pointed at /prediction/production log group | VERIFIED | `infra/cdk/lib/prediction-stack.ts` lines 303-309: awslogs driver with awslogs-group=/prediction/production, tag=prediction |
| 4 | Local docker-compose retains json-file driver for local development | VERIFIED | `docker-compose.yml` lines 16-20: json-file driver with max-size 50m, max-file 3 |
| 5 | IAM instance role includes CloudWatchAgentServerPolicy for host metrics | VERIFIED | `infra/cdk/lib/prediction-stack.ts` lines 134-136: `CloudWatchAgentServerPolicy` managed policy attached |
| 6 | CloudWatch Agent config in user-data publishes CPU, memory, disk metrics to Prediction/EC2 namespace | VERIFIED | `infra/cdk/lib/prediction-stack.ts` lines 204-229: full agent config with namespace Prediction/EC2, cpu/mem/disk measurements |
| 7 | Production container runs with stdout_json=true for JSON log ingestion | PARTIAL | Set on running instance via SSM per 36-02 summary, but NOT codified in repo or CDK user-data |

**Score:** 6/7 truths verified (1 partial)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/logging/layers.rs` | Conditional JSON stdout layer | VERIFIED | Contains `.json()` conditional, boxed Layer trait objects, `stdout_json` parameter |
| `src/config/system.rs` | stdout_json config field | VERIFIED | Line 124: `pub stdout_json: bool` with `#[serde(default)]` |
| `src/main.rs` | Passes stdout_json to init_logging | VERIFIED | Line 88: `config.system.logging.stdout_json` passed as 4th arg |
| `config/config.toml` | stdout_json=false for local dev | VERIFIED | Line 5: `stdout_json = false` with comment about production |
| `infra/cdk/lib/prediction-stack.ts` | CloudWatchAgentServerPolicy + awslogs driver + CW Agent config | VERIFIED | All three present: IAM policy (L135), awslogs driver (L303-309), agent config (L204-229) |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/logging/layers.rs` | `src/config/system.rs` | stdout_json parameter | WIRED | `init_logging` accepts `stdout_json: bool`, called from main.rs with config value |
| `infra/cdk/lib/prediction-stack.ts` | CloudWatch Metrics | CloudWatchAgentServerPolicy IAM | WIRED | Managed policy attached + agent config writes to Prediction/EC2 namespace |
| `infra/cdk/lib/prediction-stack.ts` | CloudWatch Logs | awslogs driver in embedded docker-compose | WIRED | awslogs driver configured with region, group, tag, non-blocking mode |
| Docker container stdout | CloudWatch log group /prediction/production | awslogs driver | WIRED | Embedded compose has driver: awslogs with awslogs-group: /prediction/production |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-----------|-------------|--------|----------|
| MON-01 | 36-01, 36-02 | Docker Compose uses awslogs driver to ship structured JSON logs to CloudWatch | SATISFIED | CDK embedded compose uses awslogs driver; Rust layer emits JSON when stdout_json=true |
| MON-09 | 36-01, 36-02 | CloudWatch agent reports EC2 host metrics (CPU, memory, disk) | SATISFIED | CloudWatchAgentServerPolicy IAM + agent config in user-data with cpu/mem/disk metrics |

No orphaned requirements found -- MON-01 and MON-09 are the only requirements mapped to Phase 36 in REQUIREMENTS.md.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No anti-patterns found in phase artifacts |

### Human Verification Required

### 1. CloudWatch Logs JSON Ingestion

**Test:** Run `aws logs tail /prediction/production --since 5m --region us-east-1 | head -10`
**Expected:** JSON-formatted log lines with `timestamp`, `level`, `target` fields
**Why human:** Requires live AWS service access; cannot verify programmatically from dev machine

### 2. CloudWatch Logs Insights Structured Queries

**Test:** Run a Logs Insights query: `fields @timestamp, level, target | filter level = "INFO" | sort @timestamp desc | limit 10`
**Expected:** Returns structured results with extractable fields, not raw text
**Why human:** Requires live AWS service interaction

### 3. EC2 Host Metrics in CloudWatch

**Test:** Run `aws cloudwatch list-metrics --namespace "Prediction/EC2" --region us-east-1`
**Expected:** Lists cpu_usage_user, cpu_usage_system, mem_used_percent, disk_used_percent metrics
**Why human:** Requires live AWS service interaction

### 4. stdout_json=true Active on Production Instance

**Test:** Via SSM, check `/opt/prediction/data/config/config.toml` on EC2 instance for `stdout_json = true`
**Expected:** Production config has stdout_json=true
**Why human:** Config was set via SSM, not in repo -- verify it persists on instance

### Gaps Summary

All code-level artifacts are verified and properly wired. The Rust conditional JSON stdout layer works correctly, the CDK stack includes the awslogs driver, CloudWatchAgentServerPolicy IAM policy, and a full CloudWatch Agent configuration with CPU/memory/disk metrics in the Prediction/EC2 namespace.

One gap noted: `stdout_json=true` for production is NOT codified in the repository or CDK user-data. It was set on the running instance via SSM during plan 36-02 execution. If the EC2 instance is terminated and recreated by CDK, the new instance would default to `stdout_json=false` (human-readable stdout), which would break structured JSON log ingestion in CloudWatch. This should be addressed by adding a user-data step that writes `stdout_json=true` to the production config, but it is not a blocker for the current running deployment.

The three phase success criteria (JSON logs in CloudWatch, Logs Insights queries, host metrics) all require human verification against the live AWS environment. The 36-02 summary indicates the human checkpoint was approved, confirming all three criteria were met at deployment time.

---

_Verified: 2026-03-07T23:10:00Z_
_Verifier: Claude (gsd-verifier)_
