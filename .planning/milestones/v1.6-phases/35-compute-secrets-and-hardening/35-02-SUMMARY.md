---
phase: 35-compute-secrets-and-hardening
plan: 02
subsystem: infra
tags: [cdk, ec2, systemd, docker, secrets-manager, cloudwatch, sigterm]

# Dependency graph
requires:
  - phase: 35-compute-secrets-and-hardening/01
    provides: CDK stack with EC2 user-data, fetch-secrets.sh, systemd unit, docker-compose
provides:
  - Verified production EC2 instance running prediction container
  - End-to-end bootstrap chain (boot -> secrets -> container -> health)
  - Graceful SIGTERM shutdown (exit code 0)
  - Auto-restart after reboot via systemd
  - CloudWatch agent reporting host metrics
affects: [36-monitoring-and-alerting, 37-ci-cd]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - CDK logical ID change to force EC2 replacement when user-data changes
    - Full bootstrap verification via SSM Session Manager

key-files:
  created: []
  modified:
    - infra/cdk/lib/prediction-stack.ts

key-decisions:
  - "Changed CDK Instance logical ID to Instance2 to force replacement after terminated instance caused CloudFormation drift"
  - "Verified SIGTERM graceful shutdown produces exit code 0 (not 137) confirming HARD-03"

patterns-established:
  - "CDK instance replacement: change logical ID when user-data needs re-execution on existing instances"

requirements-completed: [INFRA-03, HARD-01, HARD-02, HARD-03]

# Metrics
duration: 15min
completed: 2026-03-07
---

# Phase 35 Plan 02: Deploy and Verify Summary

**CDK deploy with EC2 replacement and full bootstrap chain verification: secrets retrieval, systemd auto-start, graceful SIGTERM (exit 0), and reboot survival**

## Performance

- **Duration:** ~15 min (deploy + manual verification)
- **Started:** 2026-03-07T20:29:00Z
- **Completed:** 2026-03-07T20:40:01Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- Deployed updated CDK stack, forcing EC2 replacement via logical ID change (Instance -> Instance2)
- Verified full bootstrap chain: fetch-secrets.sh retrieves 5 keys from Secrets Manager, writes .env, ECR login works
- Confirmed systemd prediction.service auto-starts after reboot (verified via SSM after 2-min wait)
- Confirmed container exits with code 0 on SIGTERM (graceful shutdown -- all subsystems flushed and cancelled)
- CloudWatch agent active and reporting metrics
- Health endpoint returns ok with 366 active events and Deribit connected

## Task Commits

Each task was committed atomically:

1. **Task 1: Deploy updated CDK stack to AWS** - `6e6fd20` (feat)
2. **Task 2: Verify full bootstrap chain and graceful shutdown** - human-verify checkpoint, approved by user

## Files Created/Modified
- `infra/cdk/lib/prediction-stack.ts` - Changed Instance logical ID to Instance2 to force replacement

## Decisions Made
- Changed CDK Instance logical ID to Instance2 to force EC2 replacement after the original instance was terminated (CloudFormation drift). This ensured the new instance booted with the updated user-data from Plan 35-01.
- Instance ID changed from i-0aad98de6b901811c to i-004ffd908e41f04d0 as a result.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Changed CDK logical ID to force instance replacement**
- **Found during:** Task 1 (CDK deploy)
- **Issue:** After terminating the old EC2 instance, CloudFormation drifted. Simply redeploying would not recreate the instance with the correct logical ID.
- **Fix:** Changed the Instance logical ID from `Instance` to `Instance2` in prediction-stack.ts, forcing CloudFormation to create a new instance.
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Verification:** `cdk deploy` succeeded, new instance booted with full user-data
- **Committed in:** 6e6fd20

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary to complete deployment. No scope creep.

## Issues Encountered
None beyond the logical ID change documented above.

## User Setup Required
None - credentials were populated in Secrets Manager during verification.

## Next Phase Readiness
- Production EC2 instance is running with all Phase 35 requirements verified
- All 4 requirements satisfied: INFRA-03, HARD-01, HARD-02, HARD-03
- Ready for Phase 36 (monitoring and alerting) or Phase 37 (CI/CD)

## Self-Check: PASSED

- FOUND: infra/cdk/lib/prediction-stack.ts
- FOUND: commit 6e6fd20
- FOUND: 35-02-SUMMARY.md

---
*Phase: 35-compute-secrets-and-hardening*
*Completed: 2026-03-07*
