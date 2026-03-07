---
phase: 37-prometheus-amp-managed-grafana
plan: 01
subsystem: infra
tags: [amp, grafana, prometheus, cdk, ssm, iam, monitoring]

requires:
  - phase: 34-aws-cdk-foundation
    provides: CDK stack with EC2 instance role, VPC, security group
provides:
  - AMP workspace (prediction-metrics) for Prometheus remote_write target
  - Grafana IAM role with APS query permissions
  - SSM parameter /prediction/amp-workspace-id for EC2 retrieval
affects: [37-02-prometheus-sidecar, grafana-dashboard-setup]

tech-stack:
  added: [aws-aps CfnWorkspace, aws-grafana CfnWorkspace (commented), aws-ssm StringParameter]
  patterns: [SSM parameter for cross-resource ID sharing, managed service IAM role pattern]

key-files:
  created: []
  modified: [infra/cdk/lib/prediction-stack.ts]

key-decisions:
  - "AMG workspace commented out: requires IAM Identity Center (SSO) subscription not yet enabled"
  - "AMP workspace ID stored in SSM Parameter Store for EC2 retrieval at boot time"
  - "Grafana role deployed with scoped APS query permissions even though AMG is deferred"

patterns-established:
  - "SSM parameter for dynamic resource ID sharing between CDK and EC2 user-data"
  - "Managed service IAM role with scoped inline policy (not managed policy)"

requirements-completed: [MON-02, MON-03]

duration: 13min
completed: 2026-03-07
---

# Phase 37 Plan 01: AMP + Grafana Infrastructure Summary

**AMP workspace provisioned with SSM parameter for workspace ID; AMG deferred pending IAM Identity Center enablement**

## Performance

- **Duration:** 13 min
- **Started:** 2026-03-07T21:18:35Z
- **Completed:** 2026-03-07T21:31:25Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- AMP workspace `prediction-metrics` deployed (ID: ws-622e90d2-1edc-48ed-95dd-5d4938ca6659)
- SSM parameter `/prediction/amp-workspace-id` created with correct workspace ID value
- Grafana IAM role deployed with scoped aps:QueryMetrics/GetSeries/GetLabels/GetMetricMetadata permissions
- AMP Prometheus endpoint available: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/ws-622e90d2-1edc-48ed-95dd-5d4938ca6659/
- Instance role granted SSM parameter read access for workspace ID retrieval

## Task Commits

Each task was committed atomically:

1. **Task 1: Add AMP, AMG, Grafana role, and SSM parameter to CDK stack** - `28703a8` (feat)
2. **Task 2: Deploy CDK stack to provision AMP and AMG** - `216a34e` (feat)

## Files Created/Modified
- `infra/cdk/lib/prediction-stack.ts` - Added AMP workspace, Grafana IAM role, AMG workspace (commented), SSM parameter, CfnOutputs, instance role SSM grant

## Decisions Made
- **AMG deferred:** Deploy failed with 403 "needs a subscription for the service" -- IAM Identity Center (SSO) not enabled in AWS account. AMG workspace code commented out with documentation. AMP deployed successfully without AMG dependency.
- **SSM for workspace ID:** Stored AMP workspace ID in SSM Parameter Store rather than hardcoding, enabling dynamic retrieval by Prometheus sidecar in Plan 02.
- **Grafana role deployed early:** Even with AMG commented out, the Grafana IAM role was deployed so it is ready when AMG is enabled.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] AMG workspace commented out due to missing SSO subscription**
- **Found during:** Task 2 (CDK deploy)
- **Issue:** AMG CfnWorkspace creation failed with 403 "The AWS Access Key Id needs a subscription for the service" -- IAM Identity Center not enabled
- **Fix:** Commented out AMG workspace and its CfnOutputs per plan's explicit fallback instructions. AMP deployed successfully without AMG.
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Verification:** CDK deploy succeeded on second attempt. AMP workspace and SSM parameter verified via AWS CLI.
- **Committed in:** 216a34e (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking -- known prerequisite per RESEARCH.md Pitfall 1)
**Impact on plan:** Expected deviation. AMP is the critical resource for Plan 02 (Prometheus sidecar). AMG is a visualization layer that can be enabled independently after IAM Identity Center setup.

## Issues Encountered
- Git Bash on Windows converts SSM parameter paths starting with `/` to Windows paths (e.g., `/prediction/amp-workspace-id` becomes `C:/Program Files/Git/prediction/amp-workspace-id`). Workaround: set `MSYS_NO_PATHCONV=1` before AWS CLI commands with SSM paths.

## User Setup Required

**IAM Identity Center must be enabled before AMG workspace can be deployed.**
1. Go to AWS Console > IAM Identity Center
2. Enable IAM Identity Center (one-time setup)
3. Create at least one SSO user
4. Uncomment the AMG workspace block in `infra/cdk/lib/prediction-stack.ts`
5. Run `cd infra/cdk && npx cdk deploy --require-approval never`

## Next Phase Readiness
- AMP workspace ready for Prometheus remote_write (Plan 02)
- AMP workspace ID in SSM parameter ready for EC2 retrieval
- Grafana IAM role ready for AMG workspace when SSO is enabled
- Blocker: AMG workspace requires IAM Identity Center enablement (manual step)

## Self-Check: PASSED

- FOUND: infra/cdk/lib/prediction-stack.ts
- FOUND: commit 28703a8 (Task 1)
- FOUND: commit 216a34e (Task 2)
- FOUND: 37-01-SUMMARY.md

---
*Phase: 37-prometheus-amp-managed-grafana*
*Completed: 2026-03-07*
