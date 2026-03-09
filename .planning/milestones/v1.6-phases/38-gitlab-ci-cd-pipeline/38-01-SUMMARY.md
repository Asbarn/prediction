---
phase: 38-gitlab-ci-cd-pipeline
plan: 01
subsystem: infra
tags: [gitlab-ci, docker, cargo-chef, ecr, ssm, iam, ci-cd]

requires:
  - phase: 34-cdk-infrastructure
    provides: "CDK stack with ECR, EC2, IAM resources"
  - phase: 35-ec2-deploy
    provides: "fetch-secrets.sh, systemd unit, docker-compose on EC2"
provides:
  - "3-stage GitLab CI/CD pipeline (test, build-and-push, deploy)"
  - "cargo-chef Dockerfile with dependency layer caching"
  - "CI deploy IAM user with least-privilege ECR push + SSM permissions"
affects: [38-02-gitlab-variables, deploy-workflow]

tech-stack:
  added: [cargo-chef, gitlab-ci]
  patterns: [ssm-send-command-deploy, ecr-push-ci, cargo-chef-caching]

key-files:
  created: [.gitlab-ci.yml]
  modified: [Dockerfile, infra/cdk/lib/prediction-stack.ts]

key-decisions:
  - "Constructed EC2 instance ARN manually (CDK Instance lacks instanceArn property)"
  - "Used amazon/aws-cli:2 image for deploy stage (guaranteed SSM wait support)"
  - "SSM send-command with health check retry loop (5 attempts x 5s after 25s sleep)"

patterns-established:
  - "CI deploy pattern: SSM send-command instead of SSH for zero-inbound-port deployments"
  - "Docker build pattern: cargo-chef 3-stage for Rust dependency caching"

requirements-completed: [CICD-01, CICD-02, CICD-03, CICD-04, CICD-05]

duration: 3min
completed: 2026-03-08
---

# Phase 38 Plan 01: GitLab CI/CD Pipeline Summary

**3-stage GitLab CI/CD pipeline with cargo-chef Docker caching and IAM deploy user for automated test-build-deploy on master push**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-07T23:30:48Z
- **Completed:** 2026-03-07T23:34:02Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments
- Refactored Dockerfile to 3-stage cargo-chef build (planner, builder, runtime) for dependency layer caching
- Created .gitlab-ci.yml with test (cargo test), build-and-push (ECR), and deploy (SSM) stages gated to master
- Added prediction-ci-deploy IAM user to CDK with scoped ECR push and SSM send-command permissions

## Task Commits

Each task was committed atomically:

1. **Task 1: Refactor Dockerfile with cargo-chef dependency caching** - `1df081f` (feat)
2. **Task 2: Create .gitlab-ci.yml with test, build-and-push, and deploy stages** - `1e2e485` (feat)
3. **Task 3: Add CI deploy IAM user and policy to CDK stack** - `db8a3eb` (feat)

## Files Created/Modified
- `Dockerfile` - 3-stage cargo-chef build (planner prepares recipe, builder cooks deps + builds, runtime copies binaries)
- `.gitlab-ci.yml` - Complete CI/CD pipeline: test with cargo cache, Docker build+push to ECR, SSM deploy with health verification
- `infra/cdk/lib/prediction-stack.ts` - CiDeployUser IAM user with ECR push + SSM send-command policies and CfnOutput

## Decisions Made
- Constructed EC2 instance ARN manually via template string (`arn:aws:ec2:${region}:${account}:instance/${instanceId}`) because CDK `Instance` construct does not expose `instanceArn` property
- Used `amazon/aws-cli:2` Docker image for deploy stage instead of apk-installed awscli, guaranteeing SSM wait command support
- Deploy health check uses 25s initial sleep + 5-attempt retry loop (5s apart) to handle container startup jitter

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed instanceArn reference in CDK**
- **Found during:** Task 3 (CDK IAM user)
- **Issue:** Plan specified `instance.instanceArn` but CDK `ec2.Instance` construct does not have an `instanceArn` property
- **Fix:** Constructed ARN manually: `arn:aws:ec2:${this.region}:${this.account}:instance/${instance.instanceId}`
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Verification:** `cdk synth` passes, CiDeployUser appears in template
- **Committed in:** db8a3eb (Task 3 commit)

**2. [Rule 3 - Blocking] Moved CI deploy user after instance declaration**
- **Found during:** Task 3 (CDK IAM user)
- **Issue:** Plan placed CI user before Compute section, but it references `instance` variable (forward reference)
- **Fix:** Moved CI deploy user section after instance + user-data block, before Outputs
- **Files modified:** infra/cdk/lib/prediction-stack.ts
- **Verification:** TypeScript compiles, `cdk synth` passes
- **Committed in:** db8a3eb (Task 3 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both fixes necessary for CDK compilation. No scope creep.

## Issues Encountered
None beyond the deviations noted above.

## User Setup Required
None - no external service configuration required. Access keys for prediction-ci-deploy user must be created manually in IAM console and added as GitLab CI variables (covered in plan 38-02).

## Next Phase Readiness
- Pipeline YAML ready; requires GitLab CI variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, EC2_INSTANCE_ID) configured
- CDK deploy needed to create the CiDeployUser IAM resource
- Plan 38-02 covers variable configuration and first pipeline run

---
*Phase: 38-gitlab-ci-cd-pipeline*
*Completed: 2026-03-08*
