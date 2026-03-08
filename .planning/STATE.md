# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-07)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.6 Production Deployment -- Phase 39 (Grafana Dashboards and Alert Rules)

## Current Position

Phase: 39 of 39 (Grafana Dashboards and Alert Rules) -- IN PROGRESS
Plan: 1 of 2 in current phase (39-01 complete)
Status: Executing Phase 39 -- plan 01 complete
Last activity: 2026-03-08 -- Completed 39-01 (Grafana dashboards and alert rules provisioning)

Progress (overall): 6 milestones shipped (v1.0-v1.5), 38 phases, 101 plans complete
Progress (v1.6): [##########] 95%

## Performance Metrics

**Velocity:**
- Total plans completed: 96
- Total phases completed: 35
- Total execution time: ~12 days across 6 milestones

**By Milestone:**

| Milestone | Phases | Plans | Timeline |
|-----------|--------|-------|----------|
| v1.0 | 13 | 36 | 4 days |
| v1.1 | 4 | 11 | 5 days |
| v1.2 | 4 | 8 | 2 days |
| v1.3 | 4 | 7 | 2 days |
| v1.4 | 4 | 7 | 1 day |
| v1.5 | 4 | 10 | 2 days |
| v1.6 | 6 | TBD | in progress |
| Phase 36 P02 | 8min | 2 tasks | 2 files |
| Phase 38 P01 | 3min | 3 tasks | 3 files |
| Phase 39 P01 | 3min | 2 tasks | 9 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.

- Phase 34-01: Single CDK stack with all resources (no multi-stack for single-developer project)
- Phase 34-01: ECR imported by name not created (preserves existing image history)
- Phase 34-01: No NAT gateway -- public subnet only saves $32/month
- Phase 34-01: Grant helpers for IAM (correctly scoped including ecr:GetAuthorizationToken)
- Phase 34-02: CDK deploy idempotent -- cdk diff shows zero differences after deploy
- Phase 34-02: Secrets placeholder only; real credentials deferred to Phase 35
- Phase 35-01: Secrets injected via .env from Secrets Manager, not mounted volume
- Phase 35-01: systemd manages restart (Restart=on-failure), docker restart="no"
- Phase 35-01: ECR login in fetch-secrets.sh for token refresh on every start
- Phase 35-02: Changed CDK Instance logical ID to force EC2 replacement after terminated instance drift
- Phase 35-02: Verified SIGTERM graceful shutdown produces exit code 0 (HARD-03 confirmed)
- Phase 36-01: Boxed Layer trait objects for conditional JSON/human-readable stdout output
- Phase 36-01: stdout_json defaults false via serde(default) for backward compatibility
- Phase 37-01: AMG workspace deferred -- requires IAM Identity Center (SSO) subscription
- Phase 37-01: AMP workspace ID stored in SSM Parameter Store for EC2 retrieval
- Phase 37-01: Grafana role deployed with scoped APS query permissions even with AMG deferred
- Phase 37-02: Self-hosted Grafana OSS replaces AMG (avoids IAM Identity Center requirement)
- Phase 37-02: SigV4AuthType=default for EC2 instance role credential chain
- Phase 37-02: IMDSv2 hop limit=2 for Docker container metadata access
- [Phase 36]: Replaced awslogs-stream-prefix with tag option (ECS-only limitation)
- Phase 38-01: Constructed EC2 instance ARN manually (CDK Instance lacks instanceArn property)
- Phase 38-01: amazon/aws-cli:2 image for deploy stage (guaranteed SSM wait support)
- Phase 38-01: SSM send-command deploy with health check retry loop (5x5s after 25s sleep)
- [Phase 39]: Used 0.001 threshold for zero-spread alert to avoid float comparison issues
- [Phase 39]: Staleness rejection rate threshold set at 50% as reasonable starting default
- [Phase 39]: noDataState=OK for staleness alert (no data means no computations)

### Pending Todos

None.

### Blockers/Concerns

- HARD-03 (SIGTERM handler): RESOLVED -- Verified exit code 0 on SIGTERM, all subsystems flush cleanly.
- CDK clean slate: RESOLVED -- Manual infrastructure torn down and CDK deployed successfully.

## Session Continuity

Last session: 2026-03-08
Stopped at: Completed 39-01-PLAN.md (Grafana dashboards and alert rules provisioning)
Next action: Execute 39-02 (CDK user-data integration for Grafana provisioning files)
