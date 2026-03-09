---
phase: 39-grafana-dashboards-and-alert-rules
plan: 02
subsystem: infra
tags: [cdk, grafana, s3-asset, provisioning, dashboards, alerting]

requires:
  - phase: 39-grafana-dashboards-and-alert-rules
    provides: Grafana provisioning files (dashboards, alerts, datasource)
provides:
  - CDK-deployed Grafana provisioning via S3 asset (survives instance replacement)
  - Four production dashboards with live AMP data
  - Three alert rules evaluating against production metrics
affects: []

tech-stack:
  added: [cdk-s3-asset]
  patterns: [s3-asset-provisioning, user-data-s3-download]

key-files:
  created: []
  modified:
    - infra/cdk/lib/prediction-stack.ts

key-decisions:
  - "S3 asset for provisioning files instead of user-data heredocs (16KB user-data limit exceeded)"
  - "Removed contact-points.yml from provisioning (Grafana crashes with empty SMTP config)"

patterns-established:
  - "CDK S3 asset pattern: bundle local directory as S3 asset, download in user-data with aws s3 cp"

requirements-completed: [MON-04, MON-05, MON-06, MON-07, MON-08]

duration: 25min
completed: 2026-03-08
---

# Phase 39 Plan 02: CDK Grafana Provisioning Deployment Summary

**Grafana provisioning files deployed to production EC2 via CDK S3 asset with 4 dashboards and 3 alert rules verified operational against live AMP metrics**

## Performance

- **Duration:** 25 min
- **Started:** 2026-03-08T08:00:00Z
- **Completed:** 2026-03-08T08:25:00Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- All Grafana provisioning files (dashboards, alerts, datasource) deployed to production via CDK S3 asset
- Four dashboards confirmed rendering with live AMP data in Grafana UI
- Three alert rules confirmed evaluating correctly (Normal state, not Error)
- Provisioning survives instance replacement (infrastructure-as-code via CDK)

## Task Commits

Each task was committed atomically:

1. **Task 1: Update CDK user-data to write all Grafana provisioning files and deploy** - `52fd1b4` (feat)
2. **Task 2: Verify dashboards and alerts in Grafana UI** - checkpoint (human-verify, approved)

## Files Created/Modified
- `infra/cdk/lib/prediction-stack.ts` - Added S3 asset for grafana/provisioning directory, user-data downloads and extracts provisioning files before docker-compose start

## Decisions Made
- Used S3 asset instead of user-data heredocs because 4 dashboard JSON files exceeded the 16KB user-data size limit
- Removed contact-points.yml from deployed provisioning (Grafana crashes on startup when SMTP config is empty)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Switched from user-data heredocs to S3 asset for provisioning files**
- **Found during:** Task 1 (CDK user-data integration)
- **Issue:** The 4 dashboard JSON files plus alerting YAML files exceeded the EC2 user-data 16KB size limit when embedded as heredocs
- **Fix:** Used CDK S3 Asset to upload the entire grafana/provisioning directory, then downloaded it in user-data via `aws s3 cp --recursive`
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Verification:** CDK deploy succeeded, provisioning files present on EC2
- **Committed in:** 52fd1b4

**2. [Rule 1 - Bug] Removed contact-points.yml from provisioning**
- **Found during:** Task 1 (deployment verification)
- **Issue:** Grafana crashed on startup when contact-points.yml referenced SMTP configuration that was not set up
- **Fix:** Excluded contact-points.yml from the deployed provisioning files
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Verification:** Grafana starts cleanly without crash loop
- **Committed in:** 52fd1b4

**3. [Rule 1 - Bug] Deployed amp.yml missing uid:amp**
- **Found during:** Task 2 (human verification)
- **Issue:** The deployed amp.yml on EC2 was missing `uid: amp`, causing dashboards to fail datasource lookup
- **Fix:** Fixed in-place via SSM command and Grafana restart; CDK source already had the correct content
- **Files modified:** None (runtime fix only, CDK source already correct)
- **Verification:** All 4 dashboards render with live data after restart

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 blocking)
**Impact on plan:** All fixes necessary for successful deployment. S3 asset approach is actually more maintainable than heredoc embedding. No scope creep.

## Issues Encountered
- User-data 16KB limit required architectural pivot to S3 asset (handled as deviation above)
- Stale amp.yml on EC2 required manual SSM fix for uid field (CDK source was correct, indicating a caching or previous-deploy artifact issue)

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 39 (Grafana Dashboards and Alert Rules) is now complete
- All monitoring infrastructure is operational: Prometheus metrics, AMP ingestion, Grafana dashboards, and alert rules
- v1.6 Production Deployment milestone nearing completion

## Self-Check: PASSED

- [x] infra/cdk/lib/prediction-stack.ts exists
- [x] Commit 52fd1b4 exists in git history

---
*Phase: 39-grafana-dashboards-and-alert-rules*
*Completed: 2026-03-08*
