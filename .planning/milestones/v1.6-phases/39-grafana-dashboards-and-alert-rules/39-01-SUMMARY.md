---
phase: 39-grafana-dashboards-and-alert-rules
plan: 01
subsystem: infra
tags: [grafana, dashboards, alerting, promql, provisioning]

requires:
  - phase: 37-prometheus-amp-managed-grafana
    provides: Self-hosted Grafana OSS with AMP data source provisioning
provides:
  - Four operational Grafana dashboards (feed health, signal quality, paper trade P&L, system health)
  - Three alert rules (feed down, zero spread computations, high staleness rejection rate)
  - Dashboard provider config for file-based provisioning
  - Stable data source UID for consistent dashboard references
affects: [39-02-cdk-integration]

tech-stack:
  added: []
  patterns: [grafana-file-provisioning, promql-dashboard-queries, grafana-unified-alerting]

key-files:
  created:
    - grafana/provisioning/dashboards/provider.yml
    - grafana/provisioning/dashboards/json/feed-health.json
    - grafana/provisioning/dashboards/json/signal-quality.json
    - grafana/provisioning/dashboards/json/paper-trade-pnl.json
    - grafana/provisioning/dashboards/json/system-health.json
    - grafana/provisioning/alerting/rules.yml
    - grafana/provisioning/alerting/contact-points.yml
    - grafana/provisioning/alerting/notification-policies.yml
  modified:
    - grafana/provisioning/datasources/amp.yml

key-decisions:
  - "Used 0.001 threshold instead of 0 for zero-spread-computations alert to avoid float comparison issues"
  - "Staleness rejection rate threshold set at 50% (0.5) as reasonable starting default"
  - "noDataState=OK for staleness alert (no data means no computations, not a problem)"

patterns-established:
  - "Dashboard JSON provisioning: all panels reference datasource uid 'amp' for stable cross-deployment references"
  - "Alert rule two-step evaluation: PromQL query (refId A) + threshold expression (refId B using __expr__)"

requirements-completed: [MON-04, MON-05, MON-06, MON-07, MON-08]

duration: 3min
completed: 2026-03-08
---

# Phase 39 Plan 01: Grafana Dashboards and Alert Rules Summary

**Four operational Grafana dashboards with PromQL queries for 30+ metrics plus three critical alert rules for feed health, signal quality, and system monitoring**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-08T07:52:06Z
- **Completed:** 2026-03-08T07:55:00Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- Feed Health dashboard with 6 panels: availability (stat with UP/DOWN mappings), reconnection rate, message latency, P95 latency, message rate, heartbeat timeouts
- Signal Quality dashboard with 8 panels: events tracked, staleness rejection rate (gauge 0-1), computation rate, arb signals emitted/filtered, staleness rejections, spread signals
- Paper Trade P&L dashboard with 8 panels: daily P&L (big number), win rate (gauge), daily/open trades, settled trades by outcome, settlement timeouts, cumulative P&L, trade volume
- System Health dashboard with 8 panels: active expiries, subscriptions, proposals pending, active alerts, candidates discovered, alert state (table), lifecycle polls, proposals total
- Three alert rules: feed down (critical, 5m), zero spread computations (warning, 30m), high staleness rejection rate (warning, 5m, >50%)
- Stable UID added to AMP data source for consistent dashboard references across deployments

## Task Commits

Each task was committed atomically:

1. **Task 1: Create data source UID fix, dashboard provider, and all four dashboard JSON files** - `8076084` (feat)
2. **Task 2: Create alert rules and notification provisioning files** - `e2d135d` (feat)

## Files Created/Modified
- `grafana/provisioning/datasources/amp.yml` - Added uid: amp for stable datasource references
- `grafana/provisioning/dashboards/provider.yml` - Dashboard provider config pointing to JSON directory
- `grafana/provisioning/dashboards/json/feed-health.json` - Feed health monitoring dashboard (MON-04)
- `grafana/provisioning/dashboards/json/signal-quality.json` - Signal quality monitoring dashboard (MON-05)
- `grafana/provisioning/dashboards/json/paper-trade-pnl.json` - Paper trading P&L dashboard (MON-06)
- `grafana/provisioning/dashboards/json/system-health.json` - System health monitoring dashboard (MON-07)
- `grafana/provisioning/alerting/rules.yml` - Three alert rules for critical monitoring (MON-08)
- `grafana/provisioning/alerting/contact-points.yml` - Default contact point for Grafana UI visibility
- `grafana/provisioning/alerting/notification-policies.yml` - Notification routing by folder and alert name

## Decisions Made
- Used 0.001 threshold instead of 0 for zero-spread-computations alert to avoid float comparison issues
- Staleness rejection rate threshold set at 50% (0.5) as reasonable starting default
- noDataState=OK for staleness alert (no data means no computations, not a problem)
- noDataState=Alerting for feed-down and zero-spread alerts (no data likely means system is down)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 9 provisioning files ready for CDK user-data integration in Plan 02
- Dashboard JSON files are complete with proper gridPos layouts and PromQL queries
- Alert rules use two-step evaluation pattern compatible with Grafana 11.x unified alerting

---
*Phase: 39-grafana-dashboards-and-alert-rules*
*Completed: 2026-03-08*
