---
phase: 35-compute-secrets-and-hardening
plan: 01
subsystem: infra
tags: [cdk, ec2, docker, systemd, secrets-manager, cloudwatch, user-data]

requires:
  - phase: 34-cdk-infrastructure-foundation
    provides: CDK stack with EC2 instance, Secrets Manager secret, IAM role, EBS volume
provides:
  - Full EC2 bootstrap user-data (Docker, docker-compose, CW agent, jq)
  - fetch-secrets.sh for Secrets Manager to .env injection
  - systemd prediction.service with auto-restart and secret fetch on start
  - Production docker-compose.yml with graceful shutdown and env_file
  - CloudWatch agent config for CPU/memory/disk metrics
affects: [36-ci-cd-pipeline, 37-monitoring, 38-operational-runbook]

tech-stack:
  added: [docker-compose-v2, amazon-cloudwatch-agent, systemd]
  patterns: [user-data-bootstrap, secrets-manager-env-injection, systemd-managed-containers]

key-files:
  created:
    - infra/cdk/lib/cloudwatch-agent-config.json
  modified:
    - infra/cdk/lib/prediction-stack.ts
    - docker-compose.yml

key-decisions:
  - "Secrets injected via .env file from Secrets Manager, not mounted volume"
  - "systemd manages container lifecycle with Restart=on-failure, docker restart='no'"
  - "CloudWatch agent embedded in user-data heredoc, JSON config file kept as reference"
  - "ECR login happens in fetch-secrets.sh (ExecStartPre) before every docker compose up"

patterns-established:
  - "User-data heredoc pattern: cat > path <<'EOF' with each line as addCommands argument"
  - "Secret rotation: update Secrets Manager, systemctl restart prediction"
  - "Service lifecycle: systemd -> fetch-secrets -> ECR login -> docker compose up"

requirements-completed: [INFRA-03, HARD-01, HARD-02, HARD-03]

duration: 3min
completed: 2026-03-07
---

# Phase 35 Plan 01: Compute Bootstrap and Secrets Summary

**Full EC2 bootstrap with Docker/systemd/CloudWatch, Secrets Manager env injection via fetch-secrets.sh, and production docker-compose with 30s graceful shutdown**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-07T19:11:49Z
- **Completed:** 2026-03-07T19:14:40Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- CDK user-data extended with complete bootstrap: Docker, docker-compose v2 plugin, jq, CloudWatch agent
- fetch-secrets.sh reads Secrets Manager and writes .env with all 5 credential keys matching credentials.rs
- systemd prediction.service auto-starts on boot with secret fetch, ECR login, and on-failure restart
- docker-compose.yml updated for production: stop_grace_period 30s, env_file, EBS volume paths
- Secrets Manager template fixed to use correct env var names (DERIBIT_API_KEY not DERIBIT_CLIENT_ID)

## Task Commits

Each task was committed atomically:

1. **Task 1: Update CDK stack with full user-data bootstrap and fix Secrets Manager template** - `c288674` (feat)
2. **Task 2: Update docker-compose.yml for production deployment** - `7306209` (feat)

## Files Created/Modified
- `infra/cdk/lib/prediction-stack.ts` - Extended with 130+ lines of user-data: Docker install, docker-compose plugin, CW agent, fetch-secrets.sh, systemd unit, docker-compose.yml
- `infra/cdk/lib/cloudwatch-agent-config.json` - Reference CloudWatch agent config for CPU/memory/disk metrics
- `docker-compose.yml` - Production-ready with stop_grace_period, env_file, EBS volume paths, restart: "no"

## Decisions Made
- Secrets injected via environment variables (.env file) rather than mounted secrets volume -- aligns with Secrets Manager pattern
- systemd owns restart policy (Restart=on-failure), docker restart set to "no" to avoid conflicting restart behavior
- CloudWatch agent config embedded in user-data heredoc; standalone JSON file kept as reference documentation
- ECR login placed in fetch-secrets.sh so it runs on every service start (token refresh)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- `docker compose config` validation requires .env file to exist locally -- created temporary .env for validation, removed after

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- EC2 instance will fully bootstrap on next CDK deploy (docker, systemd, secrets, CW agent)
- Real credentials still need to be populated in Secrets Manager (deferred to operational runbook)
- Ready for Phase 35-02 (SIGTERM handler and remaining hardening)

## Self-Check: PASSED

All files verified present. All commits verified in git log.

---
*Phase: 35-compute-secrets-and-hardening*
*Completed: 2026-03-07*
