---
phase: 36-cloudwatch-logging
plan: 02
subsystem: infra
tags: [cloudwatch, awslogs, docker-logging, cloudwatch-agent, metrics, json-logging]

# Dependency graph
requires:
  - phase: 36-cloudwatch-logging
    provides: Conditional JSON stdout layer and stdout_json config toggle
  - phase: 34-cdk-foundation
    provides: CDK stack with EC2 instance, IAM role, log group
provides:
  - End-to-end container log pipeline (Docker stdout -> CloudWatch Logs)
  - Structured JSON log queries via CloudWatch Logs Insights
  - EC2 host metrics (CPU, memory, disk) in CloudWatch Metrics Prediction/EC2 namespace
affects: [38-grafana-dashboards, 39-alerting]

# Tech tracking
tech-stack:
  added: []
  patterns: [awslogs-docker-driver-with-tag-option, cloudwatch-agent-host-metrics]

key-files:
  created: []
  modified:
    - infra/cdk/lib/prediction-stack.ts
    - config/production.toml

key-decisions:
  - "Replaced awslogs-stream-prefix with tag option due to ECS-only limitation of stream-prefix"
  - "Wrote CloudWatch Agent config on instance via SSM (not in CDK user-data)"
  - "Added stdout_json=true to production config for structured JSON ingestion"

patterns-established:
  - "awslogs driver with tag option: use tag to control log stream naming in non-ECS Docker contexts"
  - "CloudWatch Agent config at /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.json for host metrics"

requirements-completed: [MON-01, MON-09]

# Metrics
duration: 8min
completed: 2026-03-07
---

# Phase 36 Plan 02: CloudWatch Deploy and Verify Summary

**End-to-end CloudWatch logging pipeline: container JSON logs to Logs Insights and EC2 host metrics to Prediction/EC2 namespace**

## Performance

- **Duration:** 8 min (across two sessions with human-verify checkpoint)
- **Started:** 2026-03-07
- **Completed:** 2026-03-07
- **Tasks:** 2 (1 auto + 1 checkpoint)
- **Files modified:** 2

## Accomplishments
- Deployed CDK changes with CloudWatchAgentServerPolicy IAM policy
- Container running with awslogs driver shipping structured JSON logs to /prediction/production log group
- CloudWatch Logs Insights queries successfully filter by level, target, and fields.message
- EC2 host metrics (cpu_usage_system, cpu_usage_idle, cpu_usage_user, disk_free, disk_used_percent, mem_available_percent, mem_used_percent) flowing to Prediction/EC2 namespace
- CloudWatch Agent active on instance collecting host metrics

## Task Commits

Each task was committed atomically:

1. **Task 1: Deploy CDK, rebuild image, and redeploy container** - `e28121d` (fix)
2. **Task 2: Verify CloudWatch logs and metrics in AWS Console** - checkpoint:human-verify (approved)

## Files Created/Modified
- `infra/cdk/lib/prediction-stack.ts` - Updated awslogs driver config (tag option instead of stream-prefix)
- `config/production.toml` - Added stdout_json=true for structured JSON output

## Decisions Made
- Replaced `awslogs-stream-prefix` with `tag` option because stream-prefix is an ECS-only feature not available in standalone Docker
- Wrote CloudWatch Agent configuration directly on the instance via SSM rather than embedding in CDK user-data (more flexible for iteration)
- Set `stdout_json=true` in production config to enable structured JSON log output for CloudWatch ingestion

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Replaced awslogs-stream-prefix with tag option**
- **Found during:** Task 1
- **Issue:** `awslogs-stream-prefix` is an ECS-only Docker option; standalone Docker rejects it
- **Fix:** Used `tag` option instead to control log stream naming
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Committed in:** e28121d

**2. [Rule 3 - Blocking] Wrote missing CloudWatch Agent config on instance**
- **Found during:** Task 1
- **Issue:** CloudWatch Agent had no configuration file at expected path
- **Fix:** Wrote agent config via SSM to /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.json
- **Committed in:** e28121d (operational, no code file)

**3. [Rule 3 - Blocking] Added stdout_json=true to production config**
- **Found during:** Task 1
- **Issue:** Production config lacked the stdout_json toggle, so container was emitting human-readable logs instead of JSON
- **Fix:** Set stdout_json=true in production.toml
- **Committed in:** e28121d

---

**Total deviations:** 3 auto-fixed (3 blocking)
**Impact on plan:** All fixes were necessary to make the logging pipeline functional. No scope creep.

## Issues Encountered
None beyond the deviations listed above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- CloudWatch Logs pipeline fully operational for structured queries
- Host metrics flowing for dashboard creation
- Ready for Phase 37 (Prometheus + AMP monitoring) and Phase 38 (Grafana dashboards)

---
*Phase: 36-cloudwatch-logging*
*Completed: 2026-03-07*

## Self-Check: PASSED
