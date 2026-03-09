---
phase: 37-prometheus-amp-managed-grafana
plan: 02
subsystem: infra
tags: [prometheus, grafana, amp, sigv4, docker-compose, cdk, monitoring, metrics]

requires:
  - phase: 37-01-amp-grafana-infra
    provides: AMP workspace, SSM parameter for workspace ID, Grafana IAM role
  - phase: 35-compute-bootstrap
    provides: EC2 instance with docker-compose, systemd service
provides:
  - Prometheus sidecar scraping 80+ application metrics every 15s
  - Prometheus remote_write to AMP via SigV4 authentication
  - Self-hosted Grafana OSS with provisioned AMP data source
  - End-to-end metrics pipeline: app -> Prometheus -> AMP -> Grafana
affects: [39-dashboards-alerts, monitoring-setup]

tech-stack:
  added: [prom/prometheus:v3.10.0, grafana/grafana-oss:11.5.2, SigV4 proxy auth]
  patterns: [self-hosted Grafana over AMG, IMDSv2 hop limit for Docker containers, Grafana provisioning via YAML]

key-files:
  created: [grafana/provisioning/datasources/amp.yml]
  modified: [infra/cdk/lib/prediction-stack.ts, docker-compose.yml]

key-decisions:
  - "Self-hosted Grafana OSS replaces Amazon Managed Grafana (AMG requires IAM Identity Center subscription)"
  - "SigV4 default auth type uses EC2 instance role credential chain for AMP queries"
  - "IMDSv2 hop limit set to 2 so Docker containers can access EC2 instance metadata"
  - "Removed GrafanaRole and AMG workspace code from CDK stack (no longer needed)"
  - "Port 3000 opened in security group for Grafana web UI access"

patterns-established:
  - "Grafana provisioning via YAML for automated data source configuration"
  - "IMDSv2 HttpPutResponseHopLimit=2 for Docker containers needing AWS credentials"
  - "Self-hosted monitoring stack (Prometheus + Grafana) in docker-compose"

requirements-completed: [MON-02, MON-03]

duration: 30min
completed: 2026-03-08
---

# Phase 37 Plan 02: Prometheus Sidecar + Self-Hosted Grafana Summary

**Prometheus sidecar scrapes 80+ metrics to AMP; self-hosted Grafana OSS queries AMP via SigV4 with provisioned data source**

## Performance

- **Duration:** 30 min (continuation session -- Tasks 1-2 completed in prior session)
- **Started:** 2026-03-07T22:03:49Z
- **Completed:** 2026-03-08T00:00:00Z
- **Tasks:** 3 (2 prior + 1 continuation)
- **Files modified:** 3

## Accomplishments
- Prometheus sidecar running on EC2, scraping prediction:9000/metrics every 15s
- Metrics flowing to AMP via SigV4-authenticated remote_write (no errors)
- Self-hosted Grafana OSS deployed as docker-compose service with AMP data source auto-provisioned
- Verified end-to-end: `up{job="prediction"}` = 1, `feed_available{venue="deribit"}` = 1 queryable in Grafana
- AMG workspace code and GrafanaRole removed from CDK stack (replaced by self-hosted Grafana)
- Port 3000 opened for Grafana web UI access at http://98.91.186.216:3000

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Prometheus sidecar to CDK user-data and docker-compose** - `50bc938` (feat)
2. **Task 2: Deploy CDK and verify metrics pipeline end-to-end** - (deploy-only, no code changes)
3. **Task 3: Add self-hosted Grafana OSS with AMP data source** - `4ba5202` (feat)

## Files Created/Modified
- `infra/cdk/lib/prediction-stack.ts` - Added Grafana to user-data docker-compose, provisioning config, port 3000 SG rule, IMDSv2 hop limit, APS query permissions on instance role, removed AMG/GrafanaRole
- `docker-compose.yml` - Added grafana service with volumes, ports, SigV4 env vars
- `grafana/provisioning/datasources/amp.yml` - Grafana provisioning for AMP data source with SigV4 default auth

## Decisions Made
- **Self-hosted Grafana OSS over AMG:** Amazon Managed Grafana requires IAM Identity Center (SSO) which requires a pay-as-you-go AWS account upgrade. Self-hosted Grafana OSS is free, runs alongside existing containers, and uses EC2 instance role for SigV4 auth.
- **SigV4AuthType=default:** Uses the standard AWS credential chain which works with EC2 instance role via IMDSv2. Other auth types (ec2_iam_role, keys) are not valid for Grafana.
- **IMDSv2 hop limit=2:** Default hop limit of 1 blocks Docker containers from reaching the EC2 metadata endpoint. Increased to 2 to allow Grafana container to retrieve temporary credentials via instance role.
- **Removed AMG workspace code:** Since self-hosted Grafana replaces AMG, the commented-out CfnWorkspace and the GrafanaRole (which was for AMG's service principal) were fully removed from the CDK stack.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] IMDSv2 hop limit for Docker containers**
- **Found during:** Task 3 (Grafana SigV4 configuration)
- **Issue:** Default IMDSv2 hop limit of 1 prevents Docker containers from accessing EC2 instance metadata, which breaks SigV4 authentication
- **Fix:** Added MetadataOptions with HttpPutResponseHopLimit=2 to EC2 instance via CfnInstance property override
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Verification:** Grafana data source health check returns "Successfully queried the Prometheus API"
- **Committed in:** 4ba5202 (Task 3 commit)

**2. [Rule 1 - Bug] SigV4AuthType must be "default" not "ec2_iam_role"**
- **Found during:** Task 3 (Grafana data source verification)
- **Issue:** Initial provisioning config omitted sigV4AuthType, then tried "ec2_iam_role" -- both resulted in "invalid auth type" errors
- **Fix:** Set sigV4AuthType to "default" which uses the standard AWS credential chain including EC2 instance role
- **Files modified:** grafana/provisioning/datasources/amp.yml, infra/cdk/lib/prediction-stack.ts
- **Verification:** Data source health check passes, metrics queryable
- **Committed in:** 4ba5202 (Task 3 commit)

**3. [Rule 4 -> User Decision] Self-hosted Grafana replaces AMG**
- **Found during:** Task 3 checkpoint (original plan used AMG)
- **Issue:** AMG requires IAM Identity Center which needs pay-as-you-go account upgrade
- **Resolution:** User chose self-hosted Grafana OSS approach. Task 3 was reimplemented accordingly.

---

**Total deviations:** 3 (2 auto-fixed, 1 user decision)
**Impact on plan:** Self-hosted Grafana is a simpler and cheaper approach than AMG. All auto-fixes were necessary for SigV4 authentication to work from Docker containers.

## Issues Encountered
- CDK deploy updated instance in-place (no replacement) because MetadataOptions change alone doesn't trigger replacement. Services were restarted via SSM command to apply docker-compose changes.
- Grafana SigV4 auth required iterating through auth type values to find the correct one ("default").

## User Setup Required

**Grafana is accessible at http://<EC2-PUBLIC-IP>:3000**
- Username: `admin`
- Password: `admin`
- AMP data source is pre-configured and verified working
- Change the admin password on first login

## Next Phase Readiness
- Complete metrics pipeline operational: app -> Prometheus -> AMP -> Grafana
- Ready for Phase 39 dashboard creation and alerting rules
- All application metrics (feed_available, arb_signals_emitted, etc.) queryable in Grafana
- No blockers

## Self-Check: PASSED

---
*Phase: 37-prometheus-amp-managed-grafana*
*Completed: 2026-03-08*
