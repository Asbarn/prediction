# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-07)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.6 Production Deployment -- Phase 35 (Compute, Secrets, and Hardening)

## Current Position

Phase: 35 of 39 (Compute, Secrets, and Hardening)
Plan: 1 of 2 in current phase
Status: Phase 35 in progress, Plan 01 complete
Last activity: 2026-03-07 -- Completed 35-01 (EC2 bootstrap, secrets, docker-compose)

Progress (overall): 6 milestones shipped (v1.0-v1.5), 34 phases, 93 plans complete
Progress (v1.6): [###-------] 30%

## Performance Metrics

**Velocity:**
- Total plans completed: 92
- Total phases completed: 34
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

### Pending Todos

None.

### Blockers/Concerns

- HARD-03 (SIGTERM handler): Research notes existing `tokio::signal::ctrl_c()` may not catch SIGTERM on Unix. Verify during Phase 35 planning whether `signal::unix::signal(SignalKind::terminate())` is needed.
- CDK clean slate: RESOLVED -- Manual infrastructure torn down and CDK deployed successfully.

## Session Continuity

Last session: 2026-03-07
Stopped at: Completed 35-01-PLAN.md (EC2 bootstrap, secrets, docker-compose)
Next action: Execute 35-02-PLAN.md (SIGTERM handler and remaining hardening)
