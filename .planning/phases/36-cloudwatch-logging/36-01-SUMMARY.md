---
phase: 36-cloudwatch-logging
plan: 01
subsystem: infra
tags: [tracing, cloudwatch, awslogs, json-logging, iam]

# Dependency graph
requires:
  - phase: 34-cdk-foundation
    provides: CDK stack with log group and IAM instance role
provides:
  - Conditional JSON stdout logging layer
  - stdout_json config field for toggling output format
  - CloudWatchAgentServerPolicy IAM policy (already in stack from phase 37)
  - awslogs Docker driver in embedded docker-compose (already in stack from phase 37)
affects: [37-prometheus-monitoring, 38-grafana-dashboards]

# Tech tracking
tech-stack:
  added: []
  patterns: [boxed-layer-trait-objects-for-conditional-tracing-output]

key-files:
  created: []
  modified:
    - src/logging/layers.rs
    - src/config/system.rs
    - src/main.rs
    - config/config.toml

key-decisions:
  - "Boxed Layer trait objects for conditional JSON/human-readable stdout (type erasure needed because .json() changes concrete type)"
  - "stdout_json defaults to false via serde(default) for backward compatibility"
  - "CDK IAM and awslogs changes already present from phase 37 execution -- no duplicate changes needed"

patterns-established:
  - "Boxed tracing layers: use Box<dyn Layer<_> + Send + Sync> when conditional layer configuration changes concrete types"

requirements-completed: [MON-01, MON-09]

# Metrics
duration: 6min
completed: 2026-03-07
---

# Phase 36 Plan 01: CloudWatch Logging Summary

**Conditional JSON stdout tracing layer with boxed trait objects, plus stdout_json config toggle for CloudWatch ingestion**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-07T21:18:35Z
- **Completed:** 2026-03-07T21:25:04Z
- **Tasks:** 1
- **Files modified:** 4

## Accomplishments
- Added `stdout_json` bool field to `LoggingConfig` with `#[serde(default)]` for backward compatibility
- Refactored `init_logging` to use `Box<dyn Layer<_> + Send + Sync>` for conditional JSON vs human-readable stdout
- Updated call site in `main.rs` to pass `stdout_json` from config
- Verified CDK stack already contains `CloudWatchAgentServerPolicy` and `awslogs` driver from prior phase 37 execution

## Task Commits

Each task was committed atomically:

1. **Task 1: Add conditional JSON stdout layer and awslogs driver config** - `55ecd41` (feat)

## Files Created/Modified
- `src/logging/layers.rs` - Conditional JSON stdout layer using boxed trait objects
- `src/config/system.rs` - Added `stdout_json: bool` field to `LoggingConfig`
- `src/main.rs` - Pass `stdout_json` to `init_logging`
- `config/config.toml` - Added `stdout_json = false` for local development

## Decisions Made
- Used boxed `Layer` trait objects (`Box<dyn Layer<_> + Send + Sync>`) because `.json()` changes the concrete layer type, making a simple if/else impossible without type erasure
- Made `stdout_json` default to `false` via `#[serde(default)]` so all existing config files continue working without modification
- CDK changes (CloudWatchAgentServerPolicy and awslogs driver) were already present from phase 37-01 execution -- no duplicate modifications needed

## Deviations from Plan

None - plan executed exactly as written. CDK changes were already present from a prior phase, so only the Rust and config changes were committed.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Structured JSON stdout ready for CloudWatch Logs Insights queries when `stdout_json = true`
- Production docker-compose already configured with awslogs driver
- Ready for Phase 36-02 (CloudWatch alarms and dashboards)

---
*Phase: 36-cloudwatch-logging*
*Completed: 2026-03-07*
