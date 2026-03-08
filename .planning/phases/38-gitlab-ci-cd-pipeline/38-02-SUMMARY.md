---
phase: 38-gitlab-ci-cd-pipeline
plan: 02
subsystem: infra
tags: [gitlab-ci, iam, cdk-deploy, ci-cd-variables, e2e-pipeline]

requires:
  - phase: 38-01
    provides: ".gitlab-ci.yml pipeline, cargo-chef Dockerfile, CDK CiDeployUser"
provides:
  - "Deployed CI deploy IAM user with access keys"
  - "GitLab CI variables configured (AWS creds + instance ID)"
  - "Verified end-to-end pipeline: test, build-and-push, deploy"
affects: [production-deploys, developer-workflow]

tech-stack:
  added: []
  patterns: [gitlab-ci-variables-masked-protected, iam-access-key-rotation]

key-files:
  created: []
  modified: [infra/cdk/lib/prediction-stack.ts, .gitlab-ci.yml, Dockerfile]

key-decisions:
  - "Bumped Rust image to 1.92 (comfy-table 7.2.2 requires Rust >= 1.87)"
  - "Switched from amazon/aws-cli:2 to amazon/aws-cli:latest (:2 major tag does not exist)"
  - "Override aws-cli ENTRYPOINT and use --query flags instead of python3 for output parsing"

patterns-established:
  - "GitLab CI variables: mask AWS secrets, protect all variables to master-only"

requirements-completed: [CICD-01, CICD-02, CICD-03, CICD-05]

duration: 15min
completed: 2026-03-08
---

# Phase 38 Plan 02: Deploy CDK, Configure GitLab CI, and Verify Pipeline Summary

**End-to-end GitLab CI/CD pipeline verified: CDK-deployed IAM user with GitLab CI variables drives automated test, build-push, and SSM deploy on every master push**

## Performance

- **Duration:** ~15 min (across multiple checkpoint interactions)
- **Started:** 2026-03-08
- **Completed:** 2026-03-08
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments
- Deployed CDK stack with prediction-ci-deploy IAM user; cdk diff confirms zero drift
- Configured GitLab CI variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, EC2_INSTANCE_ID) with proper masking and protection
- Verified complete pipeline execution: all 3 stages (test, build-and-push, deploy) pass, /health endpoint responds after deploy

## Task Commits

Each task was committed atomically:

1. **Task 1: Deploy CDK stack with CI deploy IAM user** - No code commit (deploy action of existing CDK code from plan 01)
2. **Task 2: Configure GitLab CI variables** - No commit (manual user action in GitLab UI)
3. **Task 3: Verify end-to-end pipeline** - 3 fix commits during E2E verification:
   - `60cffdf` fix(38): bump Rust image to 1.92 to fix comfy-table 7.2.2 build
   - `eb4c972` fix(38): use amazon/aws-cli:latest -- :2 major tag does not exist
   - `ba83e3e` fix(38): override aws-cli entrypoint and use --query instead of python

## Files Created/Modified
- `Dockerfile` - Bumped Rust image from 1.85 to 1.92 for comfy-table 7.2.2 compatibility
- `.gitlab-ci.yml` - Fixed deploy stage: amazon/aws-cli:latest with entrypoint override, --query flags instead of python3
- `infra/cdk/lib/prediction-stack.ts` - No changes in this plan (deployed as-is from plan 01)

## Decisions Made
- Bumped Rust Docker image to 1.92 because comfy-table 7.2.2 (a transitive dependency) requires Rust >= 1.87; the previous 1.85 image failed to compile
- Switched aws-cli image tag from `:2` to `:latest` because Amazon does not publish a `:2` major version tag on Docker Hub
- Replaced python3-based output parsing in deploy stage with `aws --query` flags and overrode the `aws` ENTRYPOINT that amazon/aws-cli sets by default, which broke GitLab's shell execution

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Bumped Rust image to 1.92**
- **Found during:** Task 3 (E2E pipeline verification)
- **Issue:** comfy-table 7.2.2 requires Rust >= 1.87, but Dockerfile used rust:1.85
- **Fix:** Changed `FROM rust:1.85` to `FROM rust:1.92` in all build stages
- **Files modified:** Dockerfile
- **Verification:** Pipeline test stage passes
- **Committed in:** 60cffdf

**2. [Rule 3 - Blocking] Fixed aws-cli Docker image tag**
- **Found during:** Task 3 (E2E pipeline verification)
- **Issue:** `amazon/aws-cli:2` tag does not exist; deploy stage failed to pull image
- **Fix:** Changed to `amazon/aws-cli:latest`
- **Files modified:** .gitlab-ci.yml
- **Verification:** Deploy stage image pulls successfully
- **Committed in:** eb4c972

**3. [Rule 3 - Blocking] Fixed aws-cli entrypoint and output parsing**
- **Found during:** Task 3 (E2E pipeline verification)
- **Issue:** amazon/aws-cli sets `aws` as ENTRYPOINT, breaking GitLab shell execution; python3 not available in the image for output parsing
- **Fix:** Added `entrypoint: [""]` override and replaced python3 with `--query` flags for AWS CLI output parsing
- **Files modified:** .gitlab-ci.yml
- **Verification:** Deploy stage completes, health check passes
- **Committed in:** ba83e3e

---

**Total deviations:** 3 auto-fixed (3 blocking)
**Impact on plan:** All fixes were necessary for pipeline execution. No scope creep -- each fix addressed a runtime failure discovered during E2E verification.

## Issues Encountered
- The plan specified `amazon/aws-cli:2` but this tag does not exist. Amazon publishes only `amazon/aws-cli:latest` and specific version tags (e.g., `2.x.y`).
- The amazon/aws-cli Docker image uses `aws` as its ENTRYPOINT, which conflicts with GitLab CI's shell execution model. Required `entrypoint: [""]` override.

## User Setup Required
None remaining -- all manual steps (IAM access key creation, GitLab CI variable configuration) were completed during execution.

## Next Phase Readiness
- Phase 38 complete: automated CI/CD pipeline fully operational
- Phase 39 (Grafana Dashboards) can proceed independently (depends on Phase 37, not 38)
- All future master pushes will automatically test, build, push, and deploy

## Self-Check: PASSED

- SUMMARY.md: FOUND
- Commit 60cffdf: FOUND
- Commit eb4c972: FOUND
- Commit ba83e3e: FOUND

---
*Phase: 38-gitlab-ci-cd-pipeline*
*Completed: 2026-03-08*
